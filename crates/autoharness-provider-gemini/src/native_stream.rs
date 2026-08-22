use std::collections::{BTreeMap, BTreeSet, HashSet};

use autoharness_domain::{ProviderCallId, RetryAdvice, ToolArguments, ToolName};
use autoharness_provider::{
    CompletionReason, ProviderError, ProviderErrorKind, ProviderStreamEvent, ProviderToolCall,
    SecretAccumulator, TextDelta, UsageSnapshot,
};
use serde_json::Value;

use crate::GeminiApiKey;
use crate::client::classify_error_value;
use autoharness_provider::SseFrame;

const MAX_INTERACTION_STEPS_PER_TURN: usize = 64;
const MAX_TOOL_ARGUMENT_BYTES_PER_TURN: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Transport {
    Interactions,
    GenerateContent,
}

pub(crate) struct NativeStreamState {
    transport: Transport,
    model_output_steps: HashSet<u64>,
    pending_tool_steps: BTreeMap<u64, PendingToolCall>,
    emitted_tool_steps: BTreeSet<u64>,
    tool_argument_bytes: usize,
    observed_tool_call: bool,
    text_secret_accumulator: SecretAccumulator,
    tool_id_secret_accumulator: SecretAccumulator,
    tool_name_secret_accumulator: SecretAccumulator,
    tool_argument_fragment_secret_accumulator: SecretAccumulator,
    tool_argument_secret_accumulator: SecretAccumulator,
}

#[derive(Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    complete_arguments: Option<Value>,
    partial_arguments: String,
}

impl NativeStreamState {
    pub(crate) fn new(transport: Transport) -> Self {
        Self {
            transport,
            model_output_steps: HashSet::new(),
            pending_tool_steps: BTreeMap::new(),
            emitted_tool_steps: BTreeSet::new(),
            tool_argument_bytes: 0,
            observed_tool_call: false,
            text_secret_accumulator: SecretAccumulator::new(),
            tool_id_secret_accumulator: SecretAccumulator::new(),
            tool_name_secret_accumulator: SecretAccumulator::new(),
            tool_argument_fragment_secret_accumulator: SecretAccumulator::new(),
            tool_argument_secret_accumulator: SecretAccumulator::new(),
        }
    }

    pub(crate) fn handle(
        &mut self,
        frame: SseFrame,
        key: &GeminiApiKey,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        if frame.data.trim() == "[DONE]" {
            return Err(protocol_error());
        }
        let value: Value = serde_json::from_str(&frame.data).map_err(|_| protocol_error())?;
        match self.transport {
            Transport::Interactions => {
                self.handle_interactions(frame.event.as_deref(), &value, key)
            }
            Transport::GenerateContent => self.handle_generate_content(&value, key),
        }
    }

    fn handle_interactions(
        &mut self,
        sse_event: Option<&str>,
        value: &Value,
        key: &GeminiApiKey,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        let event_type = sse_event
            .filter(|event| !event.is_empty())
            .or_else(|| value.get("event_type").and_then(Value::as_str))
            .or_else(|| value.get("type").and_then(Value::as_str))
            .unwrap_or("");

        match event_type {
            "interaction.created" => Ok(Vec::new()),
            "interaction.status_update" => status_update(value),
            "step.start" => {
                if step_type(value) == Some("function_call") {
                    self.start_function_call(value, key)?;
                    return Ok(Vec::new());
                }
                if step_type(value) != Some("model_output") {
                    return Ok(Vec::new());
                }
                if let Some(index) = step_index(value) {
                    if !self.model_output_steps.contains(&index)
                        && self.model_output_steps.len() >= MAX_INTERACTION_STEPS_PER_TURN
                    {
                        return Err(protocol_error());
                    }
                    self.model_output_steps.insert(index);
                }
                model_output_start_text(value, key, &mut self.text_secret_accumulator)
            }
            "step.delta" => {
                if matches!(
                    value.pointer("/delta/type").and_then(Value::as_str),
                    Some("arguments" | "function_call_arguments")
                ) {
                    self.append_function_arguments(value, key)?;
                    return Ok(Vec::new());
                }
                let index = step_index(value);
                let belongs_to_model_output = index
                    .is_some_and(|index| self.model_output_steps.contains(&index))
                    || (index.is_none() && self.model_output_steps.len() == 1);
                if !belongs_to_model_output {
                    return Ok(Vec::new());
                }
                let Some(delta) = value.get("delta") else {
                    return Ok(Vec::new());
                };
                if !matches!(
                    delta.get("type").and_then(Value::as_str),
                    Some("text" | "text_delta")
                ) {
                    return Ok(Vec::new());
                }
                let Some(text) = delta.get("text").and_then(Value::as_str) else {
                    return Ok(Vec::new());
                };
                normalized_text_event(text, key, &mut self.text_secret_accumulator)
                    .map(|event| event.into_iter().collect())
            }
            "step.stop" => {
                let mut events = step_index(value)
                    .filter(|index| self.pending_tool_steps.contains_key(index))
                    .map_or_else(
                        || Ok(Vec::new()),
                        |index| self.complete_function_call(index, value, key),
                    )?;
                let usage = value
                    .get("step_usage")
                    .or_else(|| value.get("usage"))
                    .or_else(|| value.get("step").and_then(|step| step.get("usage")));
                if let Some(usage) = usage.and_then(interactions_usage) {
                    events.push(ProviderStreamEvent::Usage(usage));
                }
                Ok(events)
            }
            "interaction.completed" => {
                let status = value
                    .get("interaction")
                    .and_then(|interaction| interaction.get("status"))
                    .or_else(|| value.get("status"))
                    .and_then(Value::as_str);
                if matches!(status, Some("failed" | "incomplete")) {
                    return Err(ProviderError::new(
                        ProviderErrorKind::Unavailable,
                        RetryAdvice::Never,
                    ));
                }
                if status == Some("cancelled") {
                    return Ok(vec![ProviderStreamEvent::Cancelled]);
                }
                let usage = value
                    .get("interaction")
                    .and_then(|interaction| interaction.get("usage"))
                    .or_else(|| value.get("usage"));
                let mut events = self.complete_pending_function_calls(key)?;
                if let Some(usage) = usage.and_then(interactions_usage) {
                    events.push(ProviderStreamEvent::Usage(usage));
                }
                events.push(ProviderStreamEvent::Completed {
                    reason: if self.observed_tool_call {
                        CompletionReason::ToolCalls
                    } else {
                        CompletionReason::Stop
                    },
                });
                Ok(events)
            }
            "error" => {
                let classified = classify_error_value(value, None);
                Err(ProviderError::new(classified.kind(), RetryAdvice::Never))
            }
            // Forward compatibility requires unknown lifecycle and delta types
            // to be ignored rather than crashing the stream.
            _ => Ok(Vec::new()),
        }
    }

    fn start_function_call(
        &mut self,
        value: &Value,
        key: &GeminiApiKey,
    ) -> Result<(), ProviderError> {
        let index = step_index(value).ok_or_else(protocol_error)?;
        if self.emitted_tool_steps.contains(&index) {
            return Ok(());
        }
        if !self.pending_tool_steps.contains_key(&index)
            && self
                .pending_tool_steps
                .len()
                .saturating_add(self.emitted_tool_steps.len())
                >= MAX_INTERACTION_STEPS_PER_TURN
        {
            return Err(protocol_error());
        }
        let step = value.get("step").unwrap_or(value);
        let id = step
            .get("id")
            .or_else(|| step.get("call_id"))
            .and_then(Value::as_str)
            .ok_or_else(protocol_error)?;
        let name = step
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(protocol_error)?;
        let pending = self.pending_tool_steps.entry(index).or_default();
        if pending.id.as_deref().is_some_and(|existing| existing != id)
            || pending
                .name
                .as_deref()
                .is_some_and(|existing| existing != name)
        {
            return Err(protocol_error());
        }
        if pending.id.is_none() {
            if key.observe_text(&mut self.tool_id_secret_accumulator, id) {
                return Err(protocol_error());
            }
            pending.id = Some(id.to_owned());
        }
        if pending.name.is_none() {
            if key.observe_text(&mut self.tool_name_secret_accumulator, name) {
                return Err(protocol_error());
            }
            pending.name = Some(name.to_owned());
        }
        if let Some(arguments) = step.get("arguments") {
            self.set_complete_arguments(index, arguments.clone())?;
        }
        Ok(())
    }

    fn append_function_arguments(
        &mut self,
        value: &Value,
        key: &GeminiApiKey,
    ) -> Result<(), ProviderError> {
        let index = step_index(value).ok_or_else(protocol_error)?;
        if self.emitted_tool_steps.contains(&index) {
            return Err(protocol_error());
        }
        let delta = value.get("delta").ok_or_else(protocol_error)?;
        if let Some(fragment) = delta
            .get("partial_arguments")
            .or_else(|| delta.get("arguments"))
            .and_then(Value::as_str)
        {
            let total = self.tool_argument_bytes.saturating_add(fragment.len());
            if total > MAX_TOOL_ARGUMENT_BYTES_PER_TURN
                || key.observe_text(
                    &mut self.tool_argument_fragment_secret_accumulator,
                    fragment,
                )
            {
                return Err(protocol_error());
            }
            let pending = self
                .pending_tool_steps
                .get_mut(&index)
                .ok_or_else(protocol_error)?;
            pending.partial_arguments.push_str(fragment);
            self.tool_argument_bytes = total;
            return Ok(());
        }
        if let Some(arguments) = delta.get("arguments") {
            return self.set_complete_arguments(index, arguments.clone());
        }
        Err(protocol_error())
    }

    fn set_complete_arguments(
        &mut self,
        index: u64,
        arguments: Value,
    ) -> Result<(), ProviderError> {
        let pending = self
            .pending_tool_steps
            .get_mut(&index)
            .ok_or_else(protocol_error)?;
        if pending
            .complete_arguments
            .as_ref()
            .is_some_and(|existing| existing != &arguments)
        {
            return Err(protocol_error());
        }
        if pending.complete_arguments.is_none() && pending.partial_arguments.is_empty() {
            let bytes = serde_json::to_vec(&arguments)
                .map_err(|_| protocol_error())?
                .len();
            let total = self.tool_argument_bytes.saturating_add(bytes);
            if total > MAX_TOOL_ARGUMENT_BYTES_PER_TURN {
                return Err(protocol_error());
            }
            self.tool_argument_bytes = total;
        }
        pending.complete_arguments = Some(arguments);
        Ok(())
    }

    fn complete_pending_function_calls(
        &mut self,
        key: &GeminiApiKey,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        let indexes = self.pending_tool_steps.keys().copied().collect::<Vec<_>>();
        let mut events = Vec::with_capacity(indexes.len());
        for index in indexes {
            events.extend(self.complete_function_call(index, &Value::Null, key)?);
        }
        Ok(events)
    }

    fn complete_function_call(
        &mut self,
        index: u64,
        value: &Value,
        key: &GeminiApiKey,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        if self.emitted_tool_steps.contains(&index) {
            return Ok(Vec::new());
        }
        if let Some(arguments) = value
            .get("step")
            .and_then(|step| step.get("arguments"))
            .cloned()
        {
            self.set_complete_arguments(index, arguments)?;
        }
        let pending = self
            .pending_tool_steps
            .remove(&index)
            .ok_or_else(protocol_error)?;
        let id = pending.id.ok_or_else(protocol_error)?;
        let name = pending.name.ok_or_else(protocol_error)?;
        let arguments = if pending.partial_arguments.is_empty() {
            pending
                .complete_arguments
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
        } else {
            let partial: Value =
                serde_json::from_str(&pending.partial_arguments).map_err(|_| protocol_error())?;
            if pending.complete_arguments.as_ref().is_some_and(|complete| {
                complete
                    .as_object()
                    .is_some_and(|object| !object.is_empty())
                    && complete != &partial
            }) {
                return Err(protocol_error());
            }
            partial
        };
        if key.observe_structured(&mut self.tool_argument_secret_accumulator, &arguments) {
            return Err(protocol_error());
        }
        let event = ProviderStreamEvent::ToolCall(ProviderToolCall {
            provider_call_id: ProviderCallId::new(id).map_err(|_| protocol_error())?,
            tool_name: ToolName::new(name).map_err(|_| protocol_error())?,
            arguments: ToolArguments::new(arguments).map_err(|_| protocol_error())?,
        });
        self.emitted_tool_steps.insert(index);
        self.observed_tool_call = true;
        Ok(vec![event])
    }

    fn handle_generate_content(
        &mut self,
        value: &Value,
        key: &GeminiApiKey,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        if value.get("error").is_some() {
            let classified = classify_error_value(value, None);
            return Err(ProviderError::new(classified.kind(), RetryAdvice::Never));
        }

        let mut events = Vec::new();
        let mut completion = None;
        if let Some(candidates) = value.get("candidates").and_then(Value::as_array) {
            for candidate in candidates {
                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str)
                            && let Some(event) =
                                normalized_text_event(text, key, &mut self.text_secret_accumulator)?
                        {
                            events.push(event);
                        }
                    }
                }
                if completion.is_none() {
                    completion = candidate
                        .get("finishReason")
                        .and_then(Value::as_str)
                        .map(completion_reason);
                }
            }
        }

        if let Some(usage) = value.get("usageMetadata").and_then(generate_content_usage) {
            events.push(ProviderStreamEvent::Usage(usage));
        }
        if let Some(reason) = completion {
            events.push(ProviderStreamEvent::Completed { reason });
        } else if value
            .get("promptFeedback")
            .and_then(|feedback| feedback.get("blockReason"))
            .and_then(Value::as_str)
            .is_some()
        {
            events.push(ProviderStreamEvent::Completed {
                reason: CompletionReason::Safety,
            });
        }
        Ok(events)
    }
}

fn status_update(value: &Value) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
    let status = value
        .get("interaction")
        .and_then(|interaction| interaction.get("status"))
        .or_else(|| value.get("status"))
        .and_then(Value::as_str);
    match status {
        Some("cancelled") => Ok(vec![ProviderStreamEvent::Cancelled]),
        Some("failed" | "incomplete") => Err(ProviderError::new(
            ProviderErrorKind::Unavailable,
            RetryAdvice::Never,
        )),
        _ => Ok(Vec::new()),
    }
}

fn normalized_text_event(
    text: &str,
    key: &GeminiApiKey,
    accumulator: &mut SecretAccumulator,
) -> Result<Option<ProviderStreamEvent>, ProviderError> {
    let redacted = key.redact(text);
    if redacted.is_empty() {
        return Ok(None);
    }
    if key.observe_text(accumulator, &redacted) {
        return Err(protocol_error());
    }
    TextDelta::new(redacted)
        .ok()
        .map(ProviderStreamEvent::TextDelta)
        .map_or(Ok(None), |event| Ok(Some(event)))
}

fn model_output_start_text(
    value: &Value,
    key: &GeminiApiKey,
    accumulator: &mut SecretAccumulator,
) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
    let mut events = Vec::new();
    for text in value
        .get("step")
        .and_then(|step| step.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
    {
        if let Some(event) = normalized_text_event(text, key, accumulator)? {
            events.push(event);
        }
    }
    Ok(events)
}

fn step_type(value: &Value) -> Option<&str> {
    value
        .get("step")
        .and_then(|step| step.get("type"))
        .and_then(Value::as_str)
        .or_else(|| value.get("step_type").and_then(Value::as_str))
}

fn step_index(value: &Value) -> Option<u64> {
    value
        .get("index")
        .or_else(|| value.get("step_index"))
        .or_else(|| value.get("step").and_then(|step| step.get("index")))
        .and_then(Value::as_u64)
}

fn interactions_usage(value: &Value) -> Option<UsageSnapshot> {
    let usage = UsageSnapshot {
        input_tokens: number(value, &["total_input_tokens", "input_tokens"]),
        output_tokens: number(value, &["total_output_tokens", "output_tokens"]),
        cached_input_tokens: number(value, &["total_cached_tokens", "cached_tokens"]),
        reasoning_tokens: number(value, &["total_thought_tokens", "thought_tokens"]),
        tool_tokens: number(value, &["total_tool_use_tokens", "tool_use_tokens"]),
        total_tokens: number(value, &["total_tokens"]),
    };
    has_usage(usage).then_some(usage)
}

fn generate_content_usage(value: &Value) -> Option<UsageSnapshot> {
    let usage = UsageSnapshot {
        input_tokens: number(value, &["promptTokenCount"]),
        output_tokens: number(value, &["candidatesTokenCount"]),
        cached_input_tokens: number(value, &["cachedContentTokenCount"]),
        reasoning_tokens: number(value, &["thoughtsTokenCount"]),
        tool_tokens: number(value, &["toolUsePromptTokenCount"]),
        total_tokens: number(value, &["totalTokenCount"]),
    };
    has_usage(usage).then_some(usage)
}

fn number(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn has_usage(usage: UsageSnapshot) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.cached_input_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.tool_tokens.is_some()
        || usage.total_tokens.is_some()
}

fn completion_reason(reason: &str) -> CompletionReason {
    match reason {
        "STOP" => CompletionReason::Stop,
        "MAX_TOKENS" => CompletionReason::Length,
        "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => CompletionReason::Safety,
        "RECITATION" => CompletionReason::Recitation,
        _ => CompletionReason::Other,
    }
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoharness_domain::ClassifiedError as _;

    use autoharness_provider::SseDecoder;

    fn decode_fixture(chunks: &[&[u8]]) -> Vec<ProviderStreamEvent> {
        let key = GeminiApiKey::new("gemini-secret-sentinel").expect("key");
        let mut decoder = SseDecoder::new(1024 * 1024);
        let mut state = NativeStreamState::new(Transport::Interactions);
        let mut output = vec![ProviderStreamEvent::Started];
        let mut terminal = false;

        'chunks: for chunk in chunks {
            for frame in decoder.push(chunk).expect("valid fragmented SSE") {
                let events = state.handle(frame, &key).expect("valid native event");
                for event in events {
                    terminal = matches!(
                        event,
                        ProviderStreamEvent::Completed { .. } | ProviderStreamEvent::Cancelled
                    );
                    output.push(event);
                    if terminal {
                        break 'chunks;
                    }
                }
            }
        }
        assert!(terminal, "fixture must reach a terminal lifecycle event");
        output
    }

    fn canonical_events() -> Vec<ProviderStreamEvent> {
        vec![
            ProviderStreamEvent::Started,
            ProviderStreamEvent::TextDelta(TextDelta::new("Hé").expect("text")),
            ProviderStreamEvent::TextDelta(TextDelta::new("🙂").expect("text")),
            ProviderStreamEvent::Usage(UsageSnapshot {
                input_tokens: Some(2),
                output_tokens: Some(1),
                cached_input_tokens: None,
                reasoning_tokens: None,
                tool_tokens: None,
                total_tokens: Some(3),
            }),
            ProviderStreamEvent::Usage(UsageSnapshot {
                input_tokens: Some(2),
                output_tokens: Some(3),
                cached_input_tokens: Some(1),
                reasoning_tokens: Some(1),
                tool_tokens: Some(0),
                total_tokens: Some(6),
            }),
            ProviderStreamEvent::Completed {
                reason: CompletionReason::Stop,
            },
        ]
    }

    #[test]
    fn ignores_thought_steps_and_redacts_key_echoes() {
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        for data in [
            r#"{"event_type":"step.start","index":0,"step":{"type":"thought"}}"#,
            r#"{"event_type":"step.delta","index":0,"delta":{"type":"text","text":"hidden"}}"#,
            r#"{"event_type":"step.start","index":1,"step":{"type":"model_output"}}"#,
        ] {
            let events = state
                .handle(
                    SseFrame {
                        event: None,
                        data: data.to_owned(),
                    },
                    &key,
                )
                .expect("valid event");
            assert!(events.is_empty());
        }
        let events = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":"echo secret-sentinel"}}"#.to_owned(),
                },
                &key,
            )
            .expect("valid text event");

        let ProviderStreamEvent::TextDelta(delta) = &events[0] else {
            panic!("expected text delta");
        };
        assert_eq!(delta.as_str(), "echo [REDACTED]");
        let serialized = serde_json::to_string(&events).expect("normalized events");
        assert!(!serialized.contains("secret-sentinel"));
    }

    #[test]
    fn model_output_step_start_preserves_initial_text_and_redacts_key_echoes() {
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let start = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"event_type":"step.start","index":1,"step":{"type":"model_output","content":[{"type":"text","text":"Once upon secret-sentinel"}]}}"#.to_owned(),
                },
                &key,
            )
            .expect("valid model-output start");
        let delta = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":" a time..."}}"#.to_owned(),
                },
                &key,
            )
            .expect("valid model-output delta");

        assert_eq!(
            start,
            vec![ProviderStreamEvent::TextDelta(
                TextDelta::new("Once upon [REDACTED]").expect("text")
            )]
        );
        assert_eq!(
            delta,
            vec![ProviderStreamEvent::TextDelta(
                TextDelta::new(" a time...").expect("text")
            )]
        );
    }

    #[test]
    fn interaction_function_call_count_is_bounded() {
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        for index in 0..MAX_INTERACTION_STEPS_PER_TURN {
            let frame = serde_json::json!({
                "event_type":"step.start",
                "index":index,
                "step":{
                    "type":"function_call",
                    "id":format!("call-{index}"),
                    "name":"fs_read",
                    "arguments":{"path":"README.md"}
                }
            });
            assert!(
                state
                    .handle(
                        SseFrame {
                            event: None,
                            data: frame.to_string(),
                        },
                        &key,
                    )
                    .is_ok()
            );
        }
        let overflow = serde_json::json!({
            "event_type":"step.start",
            "index":MAX_INTERACTION_STEPS_PER_TURN,
            "step":{
                "type":"function_call",
                "id":"call-overflow",
                "name":"fs_read",
                "arguments":{"path":"README.md"}
            }
        });

        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: overflow.to_string(),
                    },
                    &key,
                )
                .is_err()
        );
    }

    #[test]
    fn interaction_tool_argument_bytes_are_bounded_across_calls() {
        let key = GeminiApiKey::new("secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let content = "a".repeat(60 * 1024);
        for index in 0..4 {
            let frame = serde_json::json!({
                "event_type":"step.start",
                "index":index,
                "step":{
                    "type":"function_call",
                    "id":format!("call-{index}"),
                    "name":"fs_write",
                    "arguments":{"path":format!("file-{index}"),"content":content}
                }
            });
            assert!(
                state
                    .handle(
                        SseFrame {
                            event: None,
                            data: frame.to_string(),
                        },
                        &key,
                    )
                    .is_ok()
            );
        }
        let overflow = serde_json::json!({
            "event_type":"step.start",
            "index":4,
            "step":{
                "type":"function_call",
                "id":"call-overflow",
                "name":"fs_write",
                "arguments":{"path":"overflow","content":content}
            }
        });

        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: overflow.to_string(),
                    },
                    &key,
                )
                .is_err()
        );
    }

    #[test]
    fn structured_key_fragments_are_rejected_before_emission() {
        let key = GeminiApiKey::new("split-key").expect("key");
        let frame = serde_json::json!({
            "event_type":"step.start",
            "index":0,
            "step":{
                "type":"function_call",
                "id":"call-1",
                "name":"fs_write",
                "arguments":{"path":"split-","content":"key"}
            }
        });
        let mut state = NativeStreamState::new(Transport::Interactions);
        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: frame.to_string(),
                    },
                    &key,
                )
                .expect("start is buffered")
                .is_empty()
        );
        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: r#"{"event_type":"interaction.completed","interaction":{"status":"completed"}}"#
                            .to_owned(),
                    },
                    &key,
                )
                .is_err()
        );
    }

    #[test]
    fn key_split_across_text_events_is_rejected_before_second_emission() {
        let key = GeminiApiKey::new("split-key").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"event_type":"step.start","index":1,"step":{"type":"model_output"}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("model output start");
        let first = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":"split-"}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("first safe fragment");

        assert_eq!(
            first,
            vec![ProviderStreamEvent::TextDelta(
                TextDelta::new("split-").expect("text")
            )]
        );
        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":"key"}}"#
                            .to_owned(),
                    },
                    &key,
                )
                .is_err()
        );
    }

    #[test]
    fn key_split_across_tool_calls_is_rejected_before_second_emission() {
        let key = GeminiApiKey::new("split-key").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let first = serde_json::json!({
            "event_type":"step.start",
            "index":1,
            "step":{
                "type":"function_call",
                "id":"call-1",
                "name":"fs_read",
                "arguments":{"path":"split-"}
            }
        });
        let second = serde_json::json!({
            "event_type":"step.start",
            "index":2,
            "step":{
                "type":"function_call",
                "id":"call-2",
                "name":"fs_read",
                "arguments":{"path":"key"}
            }
        });

        state
            .handle(
                SseFrame {
                    event: None,
                    data: first.to_string(),
                },
                &key,
            )
            .expect("first safe call start");
        assert_eq!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: r#"{"event_type":"step.stop","index":1}"#.to_owned(),
                    },
                    &key,
                )
                .expect("first safe call completion")
                .len(),
            1
        );
        state
            .handle(
                SseFrame {
                    event: None,
                    data: second.to_string(),
                },
                &key,
            )
            .expect("second call start");
        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: r#"{"event_type":"step.stop","index":2}"#.to_owned(),
                    },
                    &key,
                )
                .is_err()
        );
    }

    #[test]
    fn key_split_across_provider_call_ids_is_rejected_before_second_emission() {
        let key = GeminiApiKey::new("split-credential").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        for (index, id) in [(1, "split-"), (2, "credential")] {
            let frame = serde_json::json!({
                "event_type":"step.start",
                "index":index,
                "step":{
                    "type":"function_call",
                    "id":id,
                    "name":"fs_read",
                    "arguments":{"path":"README.md"}
                }
            });
            let result = state.handle(
                SseFrame {
                    event: None,
                    data: frame.to_string(),
                },
                &key,
            );
            if index == 1 {
                assert!(result.expect("first safe call start").is_empty());
                assert_eq!(
                    state
                        .handle(
                            SseFrame {
                                event: None,
                                data: r#"{"event_type":"step.stop","index":1}"#.to_owned(),
                            },
                            &key,
                        )
                        .expect("first call completion")
                        .len(),
                    1
                );
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn usage_is_a_snapshot_not_a_derived_total() {
        let value: Value = serde_json::from_str(
            r#"{"total_input_tokens":3,"total_output_tokens":5,"total_thought_tokens":7,"total_tokens":19}"#,
        )
        .expect("usage fixture");
        let usage = interactions_usage(&value).expect("usage");

        assert_eq!(usage.input_tokens, Some(3));
        assert_eq!(usage.output_tokens, Some(5));
        assert_eq!(usage.reasoning_tokens, Some(7));
        assert_eq!(usage.total_tokens, Some(19));
    }

    #[test]
    fn complete_lifecycle_survives_every_two_way_fragmentation() {
        let fixture = include_str!("../tests/fixtures/interactions-stream.sse")
            .replace('\n', "\r\n")
            .into_bytes();
        let expected = canonical_events();

        for split in 0..=fixture.len() {
            assert_eq!(
                decode_fixture(&[&fixture[..split], &fixture[split..]]),
                expected,
                "split at byte {split}"
            );
        }
    }

    #[test]
    fn complete_lifecycle_survives_one_byte_and_seeded_random_chunks() {
        let fixture = include_bytes!("../tests/fixtures/interactions-stream.sse");
        let one_byte = fixture.iter().map(std::slice::from_ref).collect::<Vec<_>>();
        assert_eq!(decode_fixture(&one_byte), canonical_events());

        let mut seed = 0x7a31_49d2_u32;
        let mut offset = 0;
        let mut chunks = Vec::new();
        while offset < fixture.len() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let length = usize::try_from(seed % 23 + 1).expect("small chunk length");
            let end = offset.saturating_add(length).min(fixture.len());
            chunks.push(&fixture[offset..end]);
            offset = end;
        }
        assert_eq!(decode_fixture(&chunks), canonical_events());
    }

    #[test]
    fn recorded_function_call_stream_waits_for_all_argument_bytes() {
        let fixture = include_bytes!("../tests/fixtures/interactions-tool-stream.sse");
        let one_byte = fixture.iter().map(std::slice::from_ref).collect::<Vec<_>>();
        let events = decode_fixture(&one_byte);

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Started,
                ProviderStreamEvent::ToolCall(ProviderToolCall {
                    provider_call_id: ProviderCallId::new("call-read-1").expect("provider call ID"),
                    tool_name: ToolName::new("fs_read").expect("tool name"),
                    arguments: ToolArguments::new(serde_json::json!({"path":"README.md"}))
                        .expect("tool arguments"),
                }),
                ProviderStreamEvent::Completed {
                    reason: CompletionReason::ToolCalls,
                },
            ]
        );
    }

    #[test]
    fn done_without_completed_is_a_protocol_failure() {
        let key = GeminiApiKey::new("gemini-secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let error = state
            .handle(
                SseFrame {
                    event: None,
                    data: "[DONE]".to_owned(),
                },
                &key,
            )
            .expect_err("DONE is not a terminal lifecycle event");

        assert_eq!(error.kind(), ProviderErrorKind::Protocol);
        assert_eq!(error.retry_advice(), RetryAdvice::Never);
    }

    #[test]
    fn interaction_function_call_is_normalized_and_changes_completion_reason() {
        let key = GeminiApiKey::new("gemini-secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let start = state
            .handle(
                SseFrame {
                    event: Some("step.start".to_owned()),
                    data: r#"{"event_type":"step.start","index":2,"step":{"type":"function_call","id":"call-1","name":"fs_read","arguments":{"path":"README.md"}}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("function call start");
        assert!(start.is_empty());
        let completed = state
            .handle(
                SseFrame {
                    event: Some("interaction.completed".to_owned()),
                    data: r#"{"event_type":"interaction.completed","interaction":{"status":"completed"}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("completion");

        let ProviderStreamEvent::ToolCall(call) = &completed[0] else {
            panic!("expected normalized tool call");
        };
        assert_eq!(call.provider_call_id.as_str(), "call-1");
        assert_eq!(
            call.arguments.to_value(),
            serde_json::json!({"path":"README.md"})
        );
        assert_eq!(
            completed.last(),
            Some(&ProviderStreamEvent::Completed {
                reason: CompletionReason::ToolCalls,
            })
        );
    }

    #[test]
    fn interaction_function_call_waits_for_streamed_argument_fragments() {
        let key = GeminiApiKey::new("gemini-secret-sentinel").expect("key");
        let mut state = NativeStreamState::new(Transport::Interactions);
        let start = state
            .handle(
                SseFrame {
                    event: Some("step.start".to_owned()),
                    data: r#"{"event_type":"step.start","index":2,"step":{"type":"function_call","id":"call-1","name":"http_request","arguments":{}}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("function call start");
        let first = state
            .handle(
                SseFrame {
                    event: Some("step.delta".to_owned()),
                    data: r#"{"event_type":"step.delta","index":2,"delta":{"type":"arguments","partial_arguments":"{\"method\":\"GET\",\"url\":\"https://"}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("first argument fragment");
        let second = state
            .handle(
                SseFrame {
                    event: Some("step.delta".to_owned()),
                    data: r#"{"event_type":"step.delta","index":2,"delta":{"type":"arguments","partial_arguments":"example.com/news\"}"}}"#
                        .to_owned(),
                },
                &key,
            )
            .expect("second argument fragment");
        let stopped = state
            .handle(
                SseFrame {
                    event: Some("step.stop".to_owned()),
                    data: r#"{"event_type":"step.stop","index":2}"#.to_owned(),
                },
                &key,
            )
            .expect("function call completion");

        assert!(start.is_empty());
        assert!(first.is_empty());
        assert!(second.is_empty());
        let ProviderStreamEvent::ToolCall(call) = &stopped[0] else {
            panic!("expected complete normalized tool call");
        };
        assert_eq!(
            call.arguments.to_value(),
            serde_json::json!({"method":"GET","url":"https://example.com/news"})
        );
    }
}
