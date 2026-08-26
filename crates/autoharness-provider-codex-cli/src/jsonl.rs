use autoharness_domain::RetryAdvice;
use autoharness_provider::{
    CompletionReason, ProviderError, ProviderErrorKind, ProviderStreamEvent, TextDelta,
    UsageSnapshot,
};
use serde::Deserialize;
use serde::de::IgnoredAny;

const MAX_THREAD_ID_BYTES: usize = 512;
const MAX_TEXT_DELTA_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct JsonlState {
    phase: Phase,
}

#[derive(Default)]
enum Phase {
    #[default]
    AwaitThread,
    AwaitTurn,
    Running,
    Terminal,
}

impl JsonlState {
    pub(crate) fn handle_line(
        &mut self,
        line: &str,
    ) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        let event = serde_json::from_str::<WireEvent>(line).map_err(|_| protocol_error())?;
        match event {
            WireEvent::ThreadStarted { thread_id } => {
                if !matches!(self.phase, Phase::AwaitThread) || !valid_thread_id(&thread_id) {
                    return Err(protocol_error());
                }
                self.phase = Phase::AwaitTurn;
                Ok(Vec::new())
            }
            WireEvent::TurnStarted {} => {
                if !matches!(self.phase, Phase::AwaitTurn) {
                    return Err(protocol_error());
                }
                self.phase = Phase::Running;
                Ok(vec![ProviderStreamEvent::Started])
            }
            WireEvent::ItemStarted { item: _ } | WireEvent::ItemUpdated { item: _ } => {
                if !matches!(self.phase, Phase::Running) {
                    return Err(protocol_error());
                }
                Ok(Vec::new())
            }
            WireEvent::ItemCompleted { item } => {
                if !matches!(self.phase, Phase::Running) {
                    return Err(protocol_error());
                }
                let CompletedItem::AgentMessage { text, .. } = item else {
                    // Codex-owned command, MCP, file, and planning items never become
                    // AutoHarness tool calls or any other AutoHarness authority.
                    return Ok(Vec::new());
                };
                if text.is_empty() {
                    return Ok(Vec::new());
                }
                if text.len() > MAX_TEXT_DELTA_BYTES {
                    return Err(limit_error());
                }
                let delta = TextDelta::new(text).map_err(|_| protocol_error())?;
                Ok(vec![ProviderStreamEvent::TextDelta(delta)])
            }
            WireEvent::TurnCompleted { usage } => {
                if !matches!(self.phase, Phase::Running) {
                    return Err(protocol_error());
                }
                self.phase = Phase::Terminal;
                Ok(vec![
                    ProviderStreamEvent::Usage(usage.into_snapshot()?),
                    ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    },
                ])
            }
            WireEvent::TurnFailed { error: _ } | WireEvent::Error { message: _ } => {
                Err(unavailable_error())
            }
        }
    }
}

fn valid_thread_id(thread_id: &str) -> bool {
    !thread_id.is_empty()
        && thread_id.len() <= MAX_THREAD_ID_BYTES
        && !thread_id.chars().any(char::is_control)
}

/// Wire shape includes discarded Codex-owned fields that never cross the provider boundary.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum WireEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted { thread_id: String },
    #[serde(rename = "turn.started")]
    TurnStarted {},
    #[serde(rename = "turn.completed")]
    TurnCompleted { usage: WireUsage },
    #[serde(rename = "turn.failed")]
    TurnFailed { error: WireError },
    #[serde(rename = "item.started")]
    ItemStarted { item: OpaqueItem },
    #[serde(rename = "item.updated")]
    ItemUpdated { item: OpaqueItem },
    #[serde(rename = "item.completed")]
    ItemCompleted { item: CompletedItem },
    #[serde(rename = "error")]
    Error { message: IgnoredAny },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireUsage {
    input_tokens: i64,
    cached_input_tokens: i64,
    #[serde(default)]
    cache_write_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
}

impl WireUsage {
    fn into_snapshot(self) -> Result<UsageSnapshot, ProviderError> {
        let _ = as_nonnegative_u64(self.cache_write_input_tokens)?;
        Ok(UsageSnapshot {
            input_tokens: Some(as_nonnegative_u64(self.input_tokens)?),
            output_tokens: Some(as_nonnegative_u64(self.output_tokens)?),
            cached_input_tokens: Some(as_nonnegative_u64(self.cached_input_tokens)?),
            reasoning_tokens: Some(as_nonnegative_u64(self.reasoning_output_tokens)?),
            tool_tokens: None,
            // The official CLI does not report a total, and adapters must not derive one.
            total_tokens: None,
        })
    }
}

fn as_nonnegative_u64(value: i64) -> Result<u64, ProviderError> {
    u64::try_from(value).map_err(|_| protocol_error())
}

/// Failure details are parsed only to validate the documented event shape.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireError {
    message: IgnoredAny,
}

/// A non-terminal item is structurally checked but otherwise discarded.
/// `item.started` and `item.updated` intentionally require only the common
/// documented fields because those events may carry partial item state.
#[derive(Deserialize)]
struct OpaqueItem {
    #[serde(rename = "id")]
    _id: IgnoredAny,
    #[serde(rename = "type")]
    _kind: ItemKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ItemKind {
    AgentMessage,
    Reasoning,
    CommandExecution,
    FileChange,
    McpToolCall,
    CollabToolCall,
    WebSearch,
    TodoList,
    Error,
}

/// A completed item retains only final agent text. Every other documented
/// Codex item kind is validated then ignored and cannot cross the provider
/// boundary.
/// Completed Codex items retain only assistant text and discard every other payload.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CompletedItem {
    AgentMessage {
        id: IgnoredAny,
        text: String,
    },
    Reasoning {
        id: IgnoredAny,
        text: IgnoredAny,
    },
    CommandExecution {
        id: IgnoredAny,
        command: IgnoredAny,
        aggregated_output: IgnoredAny,
        exit_code: IgnoredAny,
        status: IgnoredAny,
    },
    FileChange {
        id: IgnoredAny,
        changes: IgnoredAny,
        status: IgnoredAny,
    },
    McpToolCall {
        id: IgnoredAny,
        server: IgnoredAny,
        tool: IgnoredAny,
        arguments: IgnoredAny,
        result: IgnoredAny,
        error: IgnoredAny,
        status: IgnoredAny,
    },
    CollabToolCall {
        id: IgnoredAny,
        tool: IgnoredAny,
        sender_thread_id: IgnoredAny,
        receiver_thread_ids: IgnoredAny,
        prompt: IgnoredAny,
        agents_states: IgnoredAny,
        status: IgnoredAny,
    },
    WebSearch {
        id: IgnoredAny,
        query: IgnoredAny,
        action: IgnoredAny,
    },
    TodoList {
        id: IgnoredAny,
        items: IgnoredAny,
    },
    Error {
        id: IgnoredAny,
        message: IgnoredAny,
    },
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

fn limit_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::LimitExceeded, RetryAdvice::Never)
}

fn unavailable_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_jsonl_lifecycle_emits_only_agent_text_and_terminal_usage() {
        let mut state = JsonlState::default();
        assert!(
            state
                .handle_line(r#"{"type":"thread.started","thread_id":"thread-1"}"#)
                .expect("thread")
                .is_empty()
        );
        assert_eq!(
            state
                .handle_line(r#"{"type":"turn.started"}"#)
                .expect("turn"),
            vec![ProviderStreamEvent::Started]
        );
        assert!(state
            .handle_line(
                r#"{"type":"item.started","item":{"id":"item-1","type":"command_execution","command":"ignored","status":"in_progress"}}"#,
            )
            .expect("partial command item")
            .is_empty());
        assert!(state
            .handle_line(
                r#"{"type":"item.completed","item":{"id":"item-1","type":"command_execution","command":"ignored","aggregated_output":"ignored","exit_code":0,"status":"completed"}}"#,
            )
            .expect("command item")
            .is_empty());
        assert_eq!(
            state
                .handle_line(
                    r#"{"type":"item.completed","item":{"id":"item-2","type":"agent_message","text":"answer"}}"#,
                )
                .expect("agent item"),
            vec![ProviderStreamEvent::TextDelta(
                TextDelta::new("answer").expect("text delta"),
            )]
        );
        assert_eq!(
            state
                .handle_line(
                    r#"{"type":"turn.completed","usage":{"input_tokens":8,"cached_input_tokens":3,"output_tokens":2,"reasoning_output_tokens":1}}"#,
                )
                .expect("completion"),
            vec![
                ProviderStreamEvent::Usage(UsageSnapshot {
                    input_tokens: Some(8),
                    cached_input_tokens: Some(3),
                    output_tokens: Some(2),
                    reasoning_tokens: Some(1),
                    tool_tokens: None,
                    total_tokens: None,
                }),
                ProviderStreamEvent::Completed {
                    reason: CompletionReason::Stop,
                },
            ]
        );
    }

    #[test]
    fn unknown_or_out_of_order_protocol_events_fail_closed() {
        let mut state = JsonlState::default();
        assert_eq!(
            state
                .handle_line(r#"{"type":"turn.started"}"#)
                .expect_err("turn must follow thread")
                .kind(),
            ProviderErrorKind::Protocol
        );
        assert_eq!(
            JsonlState::default()
                .handle_line(r#"{"type":"unrecognized"}"#)
                .expect_err("unknown event")
                .kind(),
            ProviderErrorKind::Protocol
        );
    }
}
