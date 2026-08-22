use std::collections::BTreeMap;

use autoharness_domain::{ProviderCallId, RetryAdvice, ToolArguments, ToolName};
use autoharness_provider::{
    CompletionReason, ProviderError, ProviderErrorKind, ProviderStreamEvent, ProviderToolCall,
    SecretAccumulator, SseFrame, TextDelta, UsageSnapshot,
};
use reqwest::StatusCode;
use serde_json::Value;

use crate::RouterCredential;

const MAX_TOOL_CALLS_PER_TURN: usize = 64;
const MAX_PENDING_TOOL_BYTES: usize = 256 * 1024;
const MAX_PENDING_ID_BYTES: usize = 256;
const MAX_PENDING_NAME_BYTES: usize = 128;

pub(crate) struct NativeStreamState {
    pending_completion: Option<CompletionReason>,
    tool_calls: BTreeMap<u64, PendingToolCall>,
    pending_tool_bytes: usize,
    tool_calls_emitted: bool,
    text_secret_accumulator: SecretAccumulator,
    tool_id_secret_accumulator: SecretAccumulator,
    tool_name_secret_accumulator: SecretAccumulator,
    tool_argument_secret_accumulator: SecretAccumulator,
}

#[derive(Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl NativeStreamState {
    pub(crate) const fn new() -> Self {
        Self {
            pending_completion: None,
            tool_calls: BTreeMap::new(),
            pending_tool_bytes: 0,
            tool_calls_emitted: false,
            text_secret_accumulator: SecretAccumulator::new(),
            tool_id_secret_accumulator: SecretAccumulator::new(),
            tool_name_secret_accumulator: SecretAccumulator::new(),
            tool_argument_secret_accumulator: SecretAccumulator::new(),
        }
    }

    pub(crate) fn handle(
        &mut self,
        frame: SseFrame,
        credential: &RouterCredential,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        if frame.data().trim() == "[DONE]" {
            return self.pending_completion.take().map_or_else(
                || Err(protocol_error()),
                |reason| Ok(vec![ProviderStreamEvent::Completed { reason }]),
            );
        }
        let value: Value = serde_json::from_str(frame.data()).map_err(|_| protocol_error())?;
        if value.get("error").is_some() {
            return Err(classify_error_value(&value, None));
        }

        let mut events = Vec::new();
        if let Some(usage) = usage(&value) {
            events.push(ProviderStreamEvent::Usage(usage));
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(protocol_error)?;
        if choices.len() > 1 {
            return Err(protocol_error());
        }
        if let Some(choice) = choices.first() {
            if let Some(content) = choice
                .pointer("/delta/content")
                .and_then(Value::as_str)
                .filter(|content| !content.is_empty())
            {
                let content = credential.redact(content);
                if credential.observe_text(&mut self.text_secret_accumulator, &content) {
                    return Err(protocol_error());
                }
                events.push(ProviderStreamEvent::TextDelta(TextDelta::new(content)?));
            }
            if let Some(tool_calls) = choice
                .pointer("/delta/tool_calls")
                .and_then(Value::as_array)
            {
                for call in tool_calls {
                    self.append_tool_call(call, credential)?;
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                let reason = completion_reason(reason);
                if reason == CompletionReason::ToolCalls && !self.tool_calls_emitted {
                    events.extend(self.complete_tool_calls(credential)?);
                    self.tool_calls_emitted = true;
                }
                self.pending_completion = Some(reason);
            }
        }
        Ok(events)
    }

    fn append_tool_call(
        &mut self,
        value: &Value,
        credential: &RouterCredential,
    ) -> Result<(), ProviderError> {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(protocol_error)?;
        if !self.tool_calls.contains_key(&index) && self.tool_calls.len() >= MAX_TOOL_CALLS_PER_TURN
        {
            return Err(protocol_error());
        }
        let mut additional_bytes = 0_usize;
        let pending = self.tool_calls.entry(index).or_default();
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            let id = credential.redact(id);
            if credential.observe_text(&mut self.tool_id_secret_accumulator, &id) {
                return Err(protocol_error());
            }
            if id.len() > MAX_PENDING_ID_BYTES {
                return Err(protocol_error());
            }
            if pending.id.as_ref().is_some_and(|existing| existing != &id) {
                return Err(protocol_error());
            }
            if pending.id.is_none() {
                additional_bytes = additional_bytes.saturating_add(id.len());
            }
            pending.id = Some(id);
        }
        if let Some(name) = value.pointer("/function/name").and_then(Value::as_str) {
            let name = credential.redact(name);
            if credential.observe_text(&mut self.tool_name_secret_accumulator, &name) {
                return Err(protocol_error());
            }
            if name.len() > MAX_PENDING_NAME_BYTES {
                return Err(protocol_error());
            }
            if pending
                .name
                .as_ref()
                .is_some_and(|existing| existing != &name)
            {
                return Err(protocol_error());
            }
            if pending.name.is_none() {
                additional_bytes = additional_bytes.saturating_add(name.len());
            }
            pending.name = Some(name);
        }
        if let Some(arguments) = value.pointer("/function/arguments").and_then(Value::as_str) {
            if pending.arguments.len().saturating_add(arguments.len()) > 64 * 1024 {
                return Err(protocol_error());
            }
            additional_bytes = additional_bytes.saturating_add(arguments.len());
            pending.arguments.push_str(arguments);
        }
        let next = self.pending_tool_bytes.saturating_add(additional_bytes);
        if next > MAX_PENDING_TOOL_BYTES {
            return Err(protocol_error());
        }
        self.pending_tool_bytes = next;
        Ok(())
    }

    fn complete_tool_calls(
        &mut self,
        credential: &RouterCredential,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        if self.tool_calls.is_empty() {
            return Err(protocol_error());
        }
        let mut events = Vec::with_capacity(self.tool_calls.len());
        for pending in self.tool_calls.values() {
            let id = pending.id.as_deref().ok_or_else(protocol_error)?;
            let name = pending.name.as_deref().ok_or_else(protocol_error)?;
            let arguments: Value =
                serde_json::from_str(&pending.arguments).map_err(|_| protocol_error())?;
            if credential.observe_structured(&mut self.tool_argument_secret_accumulator, &arguments)
            {
                return Err(protocol_error());
            }
            events.push(ProviderStreamEvent::ToolCall(ProviderToolCall {
                provider_call_id: ProviderCallId::new(id).map_err(|_| protocol_error())?,
                tool_name: ToolName::new(name).map_err(|_| protocol_error())?,
                arguments: ToolArguments::new(arguments).map_err(|_| protocol_error())?,
            }));
        }
        Ok(events)
    }
}

fn usage(value: &Value) -> Option<UsageSnapshot> {
    let usage = value.get("usage")?;
    let snapshot = UsageSnapshot {
        input_tokens: number(usage, &["prompt_tokens", "input_tokens"]),
        output_tokens: number(usage, &["completion_tokens", "output_tokens"]),
        cached_input_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        reasoning_tokens: usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        tool_tokens: usage
            .pointer("/completion_tokens_details/tool_tokens")
            .and_then(Value::as_u64),
        total_tokens: number(usage, &["total_tokens"]),
    };
    if snapshot.input_tokens.is_some()
        || snapshot.output_tokens.is_some()
        || snapshot.cached_input_tokens.is_some()
        || snapshot.reasoning_tokens.is_some()
        || snapshot.tool_tokens.is_some()
        || snapshot.total_tokens.is_some()
    {
        Some(snapshot)
    } else {
        None
    }
}

fn number(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn completion_reason(reason: &str) -> CompletionReason {
    match reason.to_ascii_lowercase().as_str() {
        "stop" => CompletionReason::Stop,
        "length" | "max_tokens" => CompletionReason::Length,
        "content_filter" | "safety" => CompletionReason::Safety,
        "recitation" => CompletionReason::Recitation,
        "tool_calls" | "function_call" => CompletionReason::ToolCalls,
        _ => CompletionReason::Other,
    }
}

pub(crate) fn classify_error_value(value: &Value, status: Option<StatusCode>) -> ProviderError {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error/type").and_then(Value::as_str))
        .or_else(|| value.get("code").and_then(Value::as_str))
        .map(str::to_ascii_lowercase);
    let (kind, retry) = match code.as_deref() {
        Some("rate_limit_exceeded" | "rate_limit_error") => {
            (ProviderErrorKind::RateLimited, RetryAdvice::Backoff)
        }
        Some("insufficient_quota" | "quota_exceeded") => {
            (ProviderErrorKind::QuotaExceeded, RetryAdvice::Never)
        }
        Some("authentication_error" | "invalid_api_key") => {
            (ProviderErrorKind::Authentication, RetryAdvice::Never)
        }
        Some("permission_error" | "permission_denied") => {
            (ProviderErrorKind::PermissionDenied, RetryAdvice::Never)
        }
        Some("model_not_found") => (ProviderErrorKind::ModelNotFound, RetryAdvice::Never),
        Some("invalid_request_error" | "invalid_request") => {
            (ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
        }
        Some("server_error" | "api_error" | "service_unavailable") => {
            (ProviderErrorKind::Unavailable, RetryAdvice::Backoff)
        }
        Some("content_filter" | "content_policy_violation") => {
            (ProviderErrorKind::ContentBlocked, RetryAdvice::Never)
        }
        _ => classify_status(status),
    };
    let error = ProviderError::new(kind, retry);
    status.map_or(error.clone(), |status| {
        error.with_http_status(status.as_u16())
    })
}

fn classify_status(status: Option<StatusCode>) -> (ProviderErrorKind, RetryAdvice) {
    match status.map(|value| value.as_u16()) {
        Some(400 | 412 | 422) => (ProviderErrorKind::InvalidRequest, RetryAdvice::Never),
        Some(401) => (ProviderErrorKind::Authentication, RetryAdvice::Never),
        Some(403) => (ProviderErrorKind::PermissionDenied, RetryAdvice::Never),
        Some(404) => (ProviderErrorKind::ModelNotFound, RetryAdvice::Never),
        Some(408) => (ProviderErrorKind::Timeout, RetryAdvice::Backoff),
        Some(409) => (ProviderErrorKind::Conflict, RetryAdvice::Immediate),
        Some(429) => (ProviderErrorKind::RateLimited, RetryAdvice::Backoff),
        Some(500 | 502 | 503) => (ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
        Some(501) => (ProviderErrorKind::Unsupported, RetryAdvice::Never),
        Some(504) => (ProviderErrorKind::Timeout, RetryAdvice::Never),
        Some(code) if (400..500).contains(&code) => {
            (ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
        }
        Some(code) if code >= 500 => (ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
        _ => (ProviderErrorKind::Protocol, RetryAdvice::Never),
    }
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_waits_for_done_so_late_usage_is_preserved() {
        let credential = RouterCredential::new("secret").expect("credential");
        let mut state = NativeStreamState::new();
        let first = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":"stop"}]}"#
                        .to_owned(),
                },
                &credential,
            )
            .expect("chunk");
        let usage = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}"#
                        .to_owned(),
                },
                &credential,
            )
            .expect("usage");
        let done = state
            .handle(
                SseFrame {
                    event: None,
                    data: "[DONE]".to_owned(),
                },
                &credential,
            )
            .expect("done");

        assert!(matches!(first[0], ProviderStreamEvent::TextDelta(_)));
        assert!(matches!(usage[0], ProviderStreamEvent::Usage(_)));
        assert_eq!(
            done,
            vec![ProviderStreamEvent::Completed {
                reason: CompletionReason::Stop
            }]
        );
    }

    #[test]
    fn fragmented_tool_call_is_emitted_complete_before_tool_completion() {
        let credential = RouterCredential::new("secret").expect("credential");
        let mut state = NativeStreamState::new();
        let first = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"fs_read","arguments":"{\"path\":\""}}]},"finish_reason":null}]}"#
                        .to_owned(),
                },
                &credential,
            )
            .expect("first fragment");
        let second = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"README.md\"}"}}]},"finish_reason":"tool_calls"}]}"#
                        .to_owned(),
                },
                &credential,
            )
            .expect("second fragment");
        let done = state
            .handle(
                SseFrame {
                    event: None,
                    data: "[DONE]".to_owned(),
                },
                &credential,
            )
            .expect("done");

        assert!(first.is_empty());
        let ProviderStreamEvent::ToolCall(call) = &second[0] else {
            panic!("expected complete tool call");
        };
        assert_eq!(call.provider_call_id.as_str(), "call_1");
        assert_eq!(call.tool_name.as_str(), "fs_read");
        assert_eq!(
            call.arguments.to_value(),
            serde_json::json!({"path":"README.md"})
        );
        assert_eq!(
            done,
            vec![ProviderStreamEvent::Completed {
                reason: CompletionReason::ToolCalls
            }]
        );
    }

    #[test]
    fn pending_tool_call_count_is_bounded_before_completion() {
        let credential = RouterCredential::new("secret").expect("credential");
        let mut state = NativeStreamState::new();
        for index in 0..MAX_TOOL_CALLS_PER_TURN {
            let frame = serde_json::json!({
                "choices":[{"delta":{"tool_calls":[{
                    "index":index,
                    "id":format!("call-{index}"),
                    "function":{"name":"fs_read","arguments":"{\"path\":\"README.md\"}"}
                }]},"finish_reason":null}]
            });
            assert!(
                state
                    .handle(
                        SseFrame {
                            event: None,
                            data: frame.to_string(),
                        },
                        &credential,
                    )
                    .is_ok()
            );
        }
        let overflow = serde_json::json!({
            "choices":[{"delta":{"tool_calls":[{
                "index":MAX_TOOL_CALLS_PER_TURN,
                "id":"call-overflow",
                "function":{"name":"fs_read","arguments":"{\"path\":\"README.md\"}"}
            }]},"finish_reason":null}]
        });

        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: overflow.to_string(),
                    },
                    &credential,
                )
                .is_err()
        );
    }

    #[test]
    fn aggregate_pending_tool_argument_bytes_are_bounded() {
        let credential = RouterCredential::new("secret").expect("credential");
        let mut state = NativeStreamState::new();
        let chunk = "a".repeat(64 * 1024);
        for index in 0..4 {
            let frame = serde_json::json!({
                "choices":[{"delta":{"tool_calls":[{
                    "index":index,
                    "function":{"arguments":chunk}
                }]},"finish_reason":null}]
            });
            assert!(
                state
                    .handle(
                        SseFrame {
                            event: None,
                            data: frame.to_string(),
                        },
                        &credential,
                    )
                    .is_ok()
            );
        }
        let overflow = serde_json::json!({
            "choices":[{"delta":{"tool_calls":[{
                "index":4,
                "function":{"arguments":"x"}
            }]},"finish_reason":null}]
        });

        assert!(
            state
                .handle(
                    SseFrame {
                        event: None,
                        data: overflow.to_string(),
                    },
                    &credential,
                )
                .is_err()
        );
    }

    #[test]
    fn structured_credential_fragments_are_rejected_before_emission() {
        let credential = RouterCredential::new("split-credential").expect("credential");
        let arguments = serde_json::json!({"path":"split-","content":"credential"});
        let frame = serde_json::json!({
            "choices":[{"delta":{"tool_calls":[{
                "index":0,
                "id":"call-1",
                "function":{"name":"fs_write","arguments":arguments.to_string()}
            }]},"finish_reason":"tool_calls"}]
        });

        assert!(
            NativeStreamState::new()
                .handle(
                    SseFrame {
                        event: None,
                        data: frame.to_string(),
                    },
                    &credential,
                )
                .is_err()
        );
    }

    #[test]
    fn credential_split_across_text_events_is_rejected_before_second_emission() {
        let credential = RouterCredential::new("split-credential").expect("credential");
        let mut state = NativeStreamState::new();
        let first = state
            .handle(
                SseFrame {
                    event: None,
                    data: r#"{"choices":[{"delta":{"content":"split-"},"finish_reason":null}]}"#
                        .to_owned(),
                },
                &credential,
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
                        data: r#"{"choices":[{"delta":{"content":"credential"},"finish_reason":null}]}"#
                            .to_owned(),
                    },
                    &credential,
                )
                .is_err()
        );
    }

    #[test]
    fn credential_split_across_tool_calls_is_rejected_before_emission() {
        let credential = RouterCredential::new("split-credential").expect("credential");
        let mut state = NativeStreamState::new();
        for (index, path) in [(0, "split-"), (1, "credential")] {
            let arguments = serde_json::json!({"path":path});
            let frame = serde_json::json!({
                "choices":[{"delta":{"tool_calls":[{
                    "index":index,
                    "id":format!("call-{index}"),
                    "function":{"name":"fs_read","arguments":arguments.to_string()}
                }]},"finish_reason":if index == 1 { Some("tool_calls") } else { None }}]
            });
            let result = state.handle(
                SseFrame {
                    event: None,
                    data: frame.to_string(),
                },
                &credential,
            );
            if index == 0 {
                assert!(result.expect("first pending call").is_empty());
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn credential_split_across_provider_call_ids_is_rejected_before_emission() {
        let credential = RouterCredential::new("split-credential").expect("credential");
        let mut state = NativeStreamState::new();
        for (index, id) in [(0, "split-"), (1, "credential")] {
            let arguments = serde_json::json!({"path":"README.md"});
            let frame = serde_json::json!({
                "choices":[{"delta":{"tool_calls":[{
                    "index":index,
                    "id":id,
                    "function":{"name":"fs_read","arguments":arguments.to_string()}
                }]},"finish_reason":if index == 1 { Some("tool_calls") } else { None }}]
            });
            let result = state.handle(
                SseFrame {
                    event: None,
                    data: frame.to_string(),
                },
                &credential,
            );
            if index == 0 {
                assert!(result.expect("first pending call").is_empty());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
