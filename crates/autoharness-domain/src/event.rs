use serde::{Deserialize, Serialize};

use crate::{
    CommandId, CorrelationId, DeliveryMode, EventId, InputId, ModelRef, PromptText, SessionId,
    SessionSequence, TimestampMillis,
};

/// The only event schema emitted by the initial engine slice.
pub const EVENT_SCHEMA_V1: u16 = 1;

/// Identifies the command or prior event that directly caused an event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Causation {
    /// The event directly resulted from an accepted single-use command.
    Command(CommandId),
    /// The event directly resulted from an earlier event in the same session.
    Event(EventId),
}

/// A versioned, provider-neutral durable session event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EventEnvelope {
    schema_version: u16,
    event_id: EventId,
    session_id: SessionId,
    sequence: SessionSequence,
    occurred_at: TimestampMillis,
    causation: Causation,
    correlation_id: CorrelationId,
    payload: EventPayload,
}

impl EventEnvelope {
    /// Constructs an event using the current v1 schema.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new_v1(
        event_id: EventId,
        session_id: SessionId,
        sequence: SessionSequence,
        occurred_at: TimestampMillis,
        causation: Causation,
        correlation_id: CorrelationId,
        payload: EventPayload,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_V1,
            event_id,
            session_id,
            sequence,
            occurred_at,
            causation,
            correlation_id,
            payload,
        }
    }

    /// Returns the serialized schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the stable event identity.
    #[must_use]
    pub const fn event_id(&self) -> &EventId {
        &self.event_id
    }

    /// Returns the owning session aggregate.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the event's one-based session ordering key.
    #[must_use]
    pub const fn sequence(&self) -> SessionSequence {
        self.sequence
    }

    /// Returns the observed event time, which is never used for replay ordering.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampMillis {
        self.occurred_at
    }

    /// Returns the direct cause.
    #[must_use]
    pub const fn causation(&self) -> &Causation {
        &self.causation
    }

    /// Returns the logical-operation correlation identity.
    #[must_use]
    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    /// Returns the provider-neutral lifecycle payload.
    #[must_use]
    pub const fn payload(&self) -> &EventPayload {
        &self.payload
    }
}

/// Session lifecycle payloads supported by event schema v1.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum EventPayload {
    /// A session was created and can accept subsequent events.
    SessionCreated,
    /// The session's selected model changed.
    ModelSelected {
        /// Provider-neutral selected model.
        model: ModelRef,
    },
    /// User input became durable and eligible according to its delivery mode.
    InputAdmitted {
        /// Stable input identity.
        input_id: InputId,
        /// Exact admitted content.
        prompt: PromptText,
        /// Eligibility boundary for the input.
        delivery_mode: DeliveryMode,
    },
}
