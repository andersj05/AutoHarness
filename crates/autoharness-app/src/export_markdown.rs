//! Human-readable Markdown transcript export.
//!
//! The export is derived only from authoritative durable events and shares
//! its source of truth with the [JSON archive](crate::export) format; this
//! module renders the same history for people instead of machines.

use autoharness_domain::{EventEnvelope, EventPayload};

/// One conversation row rebuilt from durable events.
enum Row {
    /// An admitted user prompt.
    You { prompt: String },
    /// One assistant attempt with its settled state.
    Assistant {
        text: String,
        usage: Option<(u64, u64)>,
        failure: Option<(String, String)>,
        retry_of: Option<String>,
    },
    /// One durable tool call with its settled state.
    Tool {
        tool_name: String,
        resource: String,
        outcome: String,
        output: Option<String>,
    },
}

/// Renders one session's complete event history as Markdown.
///
/// Returns the document bytes; the caller owns naming and writing so the
/// storage thread stays the only writer beside the database.
pub fn render_markdown(
    session_id: &autoharness_domain::SessionId,
    summary: &autoharness_store::SessionSummary,
    events: &[EventEnvelope],
) -> Vec<u8> {
    let rows = rebuild(events);
    let mut document = String::new();
    document.push_str(&format!("# {}\n", summary.display_title()));
    document.push('\n');
    document.push_str(&format!("- Session: `{}`\n", session_id.as_str()));
    if let Some(model) = summary.selected_model() {
        document.push_str(&format!(
            "- Model: {} / {}\n",
            model.provider_id().as_str(),
            model.model_id().as_str()
        ));
    }
    document.push_str(&format!(
        "- Status: {}\n",
        match summary.status() {
            autoharness_store::SessionStatus::Active => "active",
            autoharness_store::SessionStatus::Archived => "archived",
        }
    ));
    document.push('\n');

    for row in &rows {
        match row {
            Row::You { prompt } => {
                document.push_str("## You\n\n");
                for line in prompt.lines() {
                    document.push_str(&format!("> {}\n", line));
                }
                if prompt.is_empty() || prompt.ends_with('\n') {
                    document.push_str(">\n");
                }
                document.push('\n');
            }
            Row::Assistant {
                text,
                usage,
                failure,
                retry_of,
            } => {
                document.push_str("## Assistant");
                if retry_of.is_some() {
                    document.push_str(" (retry)");
                }
                document.push_str("\n\n");
                if text.is_empty() {
                    document.push_str("_(no text)_\n\n");
                } else {
                    for line in text.lines() {
                        document.push_str(&format!("{line}\n"));
                    }
                    document.push('\n');
                }
                if let Some((input, output)) = usage {
                    document.push_str(&format!(
                        "_{input} input tokens, {output} output tokens_\n\n"
                    ));
                }
                if let Some((code, message)) = failure {
                    document.push_str(&format!("> **Error [{code}]:** {message}\n\n"));
                }
            }
            Row::Tool {
                tool_name,
                resource,
                outcome,
                output,
            } => {
                document.push_str(&format!("## Tool: {tool_name}\n\n"));
                document.push_str(&format!("- Resource: `{resource}`\n"));
                document.push_str(&format!("- Outcome: {outcome}\n\n"));
                if let Some(output) = output {
                    document.push_str("```\n");
                    for line in output.lines().take(40) {
                        document.push_str(&format!("{line}\n"));
                    }
                    document.push_str("```\n\n");
                }
            }
        }
    }
    document.into_bytes()
}

/// Rebuilds conversation rows in durable order from lifecycle events.
fn rebuild(events: &[EventEnvelope]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut open_attempts: Vec<(String, usize)> = Vec::new();
    let mut open_tools: Vec<(String, usize)> = Vec::new();

    for event in events {
        match event.payload() {
            EventPayload::InputAdmitted { prompt, .. } => {
                rows.push(Row::You {
                    prompt: prompt.as_str().to_owned(),
                });
            }
            EventPayload::AttemptPrepared {
                attempt_id,
                retry_of,
                ..
            } => {
                open_attempts.push((attempt_id.as_str().to_owned(), rows.len()));
                rows.push(Row::Assistant {
                    text: String::new(),
                    usage: None,
                    failure: None,
                    retry_of: retry_of.as_ref().map(|id| id.as_str().to_owned()),
                });
            }
            EventPayload::AttemptTextAppended { attempt_id, text } => {
                if let Some(&(_, index)) = open_attempts
                    .iter()
                    .rev()
                    .find(|(id, _)| id == attempt_id.as_str())
                    && let Row::Assistant { text: existing, .. } = &mut rows[index]
                {
                    existing.push_str(text.as_str());
                }
            }
            EventPayload::AttemptUsageRecorded { attempt_id, usage } => {
                if let Some(&(_, index)) = open_attempts
                    .iter()
                    .rev()
                    .find(|(id, _)| id == attempt_id.as_str())
                    && let Row::Assistant { usage: slot, .. } = &mut rows[index]
                {
                    *slot = Some((
                        usage.input_tokens().unwrap_or_default(),
                        usage.output_tokens().unwrap_or_default(),
                    ));
                }
            }
            EventPayload::AttemptFailed {
                attempt_id,
                failure,
            } => {
                if let Some(&(_, index)) = open_attempts
                    .iter()
                    .rev()
                    .find(|(id, _)| id == attempt_id.as_str())
                    && let Row::Assistant { failure: slot, .. } = &mut rows[index]
                {
                    *slot = Some((
                        failure.code().as_str().to_owned(),
                        failure.message().as_str().to_owned(),
                    ));
                }
            }
            EventPayload::ToolCallProposed { call, .. } => {
                open_tools.push((call.tool_call_id.as_str().to_owned(), rows.len()));
                rows.push(Row::Tool {
                    tool_name: call.tool_name.as_str().to_owned(),
                    resource: call.capability.resource.as_str().to_owned(),
                    outcome: "proposed".to_owned(),
                    output: None,
                });
            }
            EventPayload::ToolPermissionRecorded {
                tool_call_id,
                outcome,
                ..
            } => {
                if let Some(&(_, index)) = open_tools
                    .iter()
                    .rev()
                    .find(|(id, _)| id == tool_call_id.as_str())
                    && let Row::Tool { outcome: slot, .. } = &mut rows[index]
                {
                    *slot = match outcome {
                        autoharness_domain::PermissionOutcome::Deny => {
                            "denied by policy".to_owned()
                        }
                        autoharness_domain::PermissionOutcome::Ask => {
                            "waiting for approval".to_owned()
                        }
                        autoharness_domain::PermissionOutcome::Allow => "allowed".to_owned(),
                    };
                }
            }
            EventPayload::ToolPermissionAnswered { tool_call_id, .. } => {
                if let Some(&(_, index)) = open_tools
                    .iter()
                    .rev()
                    .find(|(id, _)| id == tool_call_id.as_str())
                    && let Row::Tool { outcome: slot, .. } = &mut rows[index]
                {
                    *slot = "approved".to_owned();
                }
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                output,
            } => {
                if let Some(&(_, index)) = open_tools
                    .iter()
                    .rev()
                    .find(|(id, _)| id == tool_call_id.as_str())
                    && let Row::Tool {
                        outcome: slot,
                        output: out_slot,
                        ..
                    } = &mut rows[index]
                {
                    *slot = "completed".to_owned();
                    *out_slot = Some(output.content().to_owned());
                }
            }
            EventPayload::ToolCallFailed {
                tool_call_id,
                failure,
            } => {
                if let Some(&(_, index)) = open_tools
                    .iter()
                    .rev()
                    .find(|(id, _)| id == tool_call_id.as_str())
                    && let Row::Tool { outcome: slot, .. } = &mut rows[index]
                {
                    *slot = format!("failed ({})", failure.code().as_str());
                }
            }
            EventPayload::ToolCallDenied { tool_call_id } => {
                if let Some(&(_, index)) = open_tools
                    .iter()
                    .rev()
                    .find(|(id, _)| id == tool_call_id.as_str())
                    && let Row::Tool { outcome: slot, .. } = &mut rows[index]
                {
                    *slot = "denied".to_owned();
                }
            }
            _ => {}
        }
    }
    rows
}
