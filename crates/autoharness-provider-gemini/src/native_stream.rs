use std::collections::HashSet;

use autoharness_domain::RetryAdvice;
use autoharness_provider::{
    CompletionReason, ProviderError, ProviderErrorKind, ProviderStreamEvent, TextDelta,
    UsageSnapshot,
};
use serde_json::Value;

use crate::GeminiApiKey;
use crate::client::classify_error_value;
use crate::sse::SseFrame;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Transport {
    Interactions,
    GenerateContent,
}

pub(crate) struct NativeStreamState {
    transport: Transport,
    model_output_steps: HashSet<u64>,
}

impl NativeStreamState {
    pub(crate) fn new(transport: Transport) -> Self {
        Self {
            transport,
            model_output_steps: HashSet::new(),
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
                if step_type(value) != Some("model_output") {
                    return Ok(Vec::new());
                }
                if let Some(index) = step_index(value) {
                    self.model_output_steps.insert(index);
                }
                Ok(model_output_start_text(value, key))
            }
            "step.delta" => {
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
                normalized_text_event(text, key)
                    .map_or_else(|| Ok(Vec::new()), |event| Ok(vec![event]))
            }
            "step.stop" => {
                let usage = value
                    .get("step_usage")
                    .or_else(|| value.get("usage"))
                    .or_else(|| value.get("step").and_then(|step| step.get("usage")));
                Ok(usage
                    .and_then(interactions_usage)
                    .map_or_else(Vec::new, |usage| vec![ProviderStreamEvent::Usage(usage)]))
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
                let mut events = Vec::new();
                if let Some(usage) = usage.and_then(interactions_usage) {
                    events.push(ProviderStreamEvent::Usage(usage));
                }
                events.push(ProviderStreamEvent::Completed {
                    reason: CompletionReason::Stop,
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

    fn handle_generate_content(
        &self,
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
                            && let Some(event) = normalized_text_event(text, key)
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

fn normalized_text_event(text: &str, key: &GeminiApiKey) -> Option<ProviderStreamEvent> {
    let redacted = key.redact(text);
    if redacted.is_empty() {
        return None;
    }
    TextDelta::new(redacted)
        .ok()
        .map(ProviderStreamEvent::TextDelta)
}

fn model_output_start_text(value: &Value, key: &GeminiApiKey) -> Vec<ProviderStreamEvent> {
    value
        .get("step")
        .and_then(|step| step.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .filter_map(|text| normalized_text_event(text, key))
        .collect()
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

    use crate::sse::SseDecoder;

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
}
