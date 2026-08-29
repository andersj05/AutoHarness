use std::time::{SystemTime, UNIX_EPOCH};

use autoharness_domain::{
    AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId, EventId, InputId,
    MemoryCommandEnvelope, MemoryCommandPayload, MemoryId, MemoryRevisionId, MemorySequence,
    PermissionDecisionId, SessionId, TimestampMillis, ToolCallId, WorkspaceId,
};
use autoharness_engine::{EventMetadataSource, GeneratedEventMetadata};
use uuid::Uuid;

/// Process-independent UUID and wall-clock source used only for new live work.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeMetadata;

impl EventMetadataSource for RuntimeMetadata {
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata {
        GeneratedEventMetadata::new(event_id(), now())
    }
}

/// Constructs a command with fresh single-use causation and correlation IDs.
#[must_use]
pub fn command(payload: CommandPayload) -> CommandEnvelope {
    CommandEnvelope::new(
        CommandId::new(tagged("command")).expect("UUID command IDs are valid"),
        CorrelationId::new(tagged("correlation")).expect("UUID correlation IDs are valid"),
        payload,
    )
}

/// Creates a globally unique session identity.
#[must_use]
pub fn session_id() -> SessionId {
    SessionId::new(tagged("session")).expect("UUID session IDs are valid")
}

/// Creates a globally unique admitted-input identity.
#[must_use]
pub fn input_id() -> InputId {
    InputId::new(tagged("input")).expect("UUID input IDs are valid")
}

/// Creates a globally unique provider-attempt identity.
#[must_use]
pub fn attempt_id() -> AttemptId {
    AttemptId::new(tagged("attempt")).expect("UUID attempt IDs are valid")
}

/// Creates a globally unique durable memory-item identity.
#[must_use]
pub fn memory_id() -> MemoryId {
    MemoryId::new(tagged("memory")).expect("UUID memory IDs are valid")
}

/// Creates a globally unique immutable memory-revision identity.
#[must_use]
pub fn memory_revision_id() -> MemoryRevisionId {
    MemoryRevisionId::new(tagged("memory-revision")).expect("UUID memory revision IDs are valid")
}

/// Creates one opaque persisted local-workspace authority identity.
#[must_use]
pub fn workspace_id() -> WorkspaceId {
    WorkspaceId::new(tagged("workspace")).expect("UUID workspace IDs are valid")
}

/// Constructs one trusted typed memory command with fresh single-use identities.
#[must_use]
pub fn memory_command(
    memory_id: MemoryId,
    expected_sequence: Option<MemorySequence>,
    payload: MemoryCommandPayload,
) -> MemoryCommandEnvelope {
    MemoryCommandEnvelope::new_v1(
        CommandId::new(tagged("memory-command")).expect("UUID memory command IDs are valid"),
        memory_id,
        expected_sequence,
        CorrelationId::new(tagged("memory-correlation"))
            .expect("UUID memory correlation IDs are valid"),
        payload,
    )
    .expect("create and update memory commands use matching sequence semantics")
}

/// Creates a globally unique local tool-call identity.
#[must_use]
pub fn tool_call_id() -> ToolCallId {
    ToolCallId::new(tagged("tool-call")).expect("UUID tool-call IDs are valid")
}

/// Creates a globally unique permission-decision identity.
#[must_use]
pub fn permission_decision_id() -> PermissionDecisionId {
    PermissionDecisionId::new(tagged("permission")).expect("UUID permission IDs are valid")
}

fn event_id() -> EventId {
    EventId::new(tagged("event")).expect("UUID event IDs are valid")
}

fn tagged(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

pub(crate) fn now() -> TimestampMillis {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_millis());
    TimestampMillis::new(i64::try_from(millis).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_unique_and_domain_valid() {
        assert_ne!(session_id(), session_id());
        assert_ne!(input_id(), input_id());
        assert_ne!(attempt_id(), attempt_id());
        assert_ne!(tool_call_id(), tool_call_id());
        assert_ne!(permission_decision_id(), permission_decision_id());
        assert_ne!(workspace_id(), workspace_id());
    }
}
