use autoharness_domain::RetryAdvice;
use autoharness_provider::{
    CompletionReason, ProviderError, ProviderErrorKind, ProviderStreamEvent, SseFrame, TextDelta,
    UsageSnapshot,
};
use reqwest::StatusCode;
use serde_json::Value;

use crate::RouterCredential;

pub(crate) struct NativeStreamState {
    pending_completion: Option<CompletionReason>,
}

impl NativeStreamState {
    pub(crate) const fn new() -> Self {
        Self {
            pending_completion: None,
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
                events.push(ProviderStreamEvent::TextDelta(TextDelta::new(content)?));
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.pending_completion = Some(completion_reason(reason));
            }
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
}
