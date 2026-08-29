use std::time::{SystemTime, UNIX_EPOCH};

use autoharness_domain::{
    AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId, EventId, InputId,
    MemoryCommandEnvelope, MemoryCommandPayload, MemoryEvidenceId, MemoryId, MemoryRevisionId,
    MemorySequence, PermissionDecisionId, SessionId, TimestampMillis, ToolCallId, WorkspaceId,
};
use autoharness_engine::{EventMetadataSource, GeneratedEventMetadata};
use sha2::{Digest, Sha256};
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

/// Derives the durable memory identity owned by one exact proposal tool call.
#[must_use]
pub fn memory_proposal_memory_id(tool_call_id: &ToolCallId) -> MemoryId {
    MemoryId::new(deterministic_tag("memory-proposal", tool_call_id.as_str()))
        .expect("SHA-256 memory proposal IDs are valid")
}

/// Derives the immutable first revision identity owned by one proposal tool call.
#[must_use]
#[allow(
    dead_code,
    reason = "consumed by the following proposal-sink checkpoint"
)]
pub fn memory_proposal_revision_id(tool_call_id: &ToolCallId) -> MemoryRevisionId {
    MemoryRevisionId::new(deterministic_tag(
        "memory-proposal-revision",
        tool_call_id.as_str(),
    ))
    .expect("SHA-256 memory proposal revision IDs are valid")
}

/// Derives the evidence identity owned by one proposal tool call.
#[must_use]
#[allow(
    dead_code,
    reason = "consumed by the following proposal-sink checkpoint"
)]
pub fn memory_proposal_evidence_id(tool_call_id: &ToolCallId) -> MemoryEvidenceId {
    MemoryEvidenceId::new(deterministic_tag(
        "memory-proposal-evidence",
        tool_call_id.as_str(),
    ))
    .expect("SHA-256 memory proposal evidence IDs are valid")
}

/// Constructs an exactly replayable proposal command for one durable tool call.
#[must_use]
#[allow(
    dead_code,
    reason = "consumed by the following proposal-sink checkpoint"
)]
pub fn memory_proposal_command(
    tool_call_id: &ToolCallId,
    payload: MemoryCommandPayload,
) -> MemoryCommandEnvelope {
    MemoryCommandEnvelope::new_v1(
        CommandId::new(deterministic_tag(
            "memory-proposal-command",
            tool_call_id.as_str(),
        ))
        .expect("SHA-256 memory proposal command IDs are valid"),
        memory_proposal_memory_id(tool_call_id),
        None,
        CorrelationId::new(deterministic_tag(
            "memory-proposal-correlation",
            tool_call_id.as_str(),
        ))
        .expect("SHA-256 memory proposal correlation IDs are valid"),
        payload,
    )
    .expect("proposal creation uses first-sequence command semantics")
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

fn deterministic_tag(prefix: &str, identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"autoharness-deterministic-id-v1\0");
    hasher.update(prefix.as_bytes());
    hasher.update(b"\0");
    hasher.update(identity.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    format!("{prefix}-{encoded}")
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

    #[test]
    fn memory_proposal_ids_are_replayable_domain_separated_and_content_free() {
        let first = ToolCallId::new("provider-visible-call-1").expect("tool call ID");
        let second = ToolCallId::new("provider-visible-call-2").expect("tool call ID");

        assert_eq!(
            memory_proposal_memory_id(&first),
            memory_proposal_memory_id(&first)
        );
        assert_eq!(
            memory_proposal_revision_id(&first),
            memory_proposal_revision_id(&first)
        );
        assert_eq!(
            memory_proposal_evidence_id(&first),
            memory_proposal_evidence_id(&first)
        );
        assert_ne!(
            memory_proposal_memory_id(&first),
            memory_proposal_memory_id(&second)
        );
        assert_ne!(
            memory_proposal_memory_id(&first).as_str(),
            memory_proposal_revision_id(&first).as_str()
        );
        assert_ne!(
            memory_proposal_revision_id(&first).as_str(),
            memory_proposal_evidence_id(&first).as_str()
        );
        for identity in [
            memory_proposal_memory_id(&first).as_str().to_owned(),
            memory_proposal_revision_id(&first).as_str().to_owned(),
            memory_proposal_evidence_id(&first).as_str().to_owned(),
        ] {
            assert!(!identity.contains(first.as_str()));
        }
    }
}
