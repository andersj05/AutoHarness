use std::time::{SystemTime, UNIX_EPOCH};

use autoharness_domain::{
    AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId, EventId, InputId,
    SessionId, TimestampMillis,
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

fn event_id() -> EventId {
    EventId::new(tagged("event")).expect("UUID event IDs are valid")
}

fn tagged(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn now() -> TimestampMillis {
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
    }
}
