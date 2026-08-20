use serde::{Deserialize, Serialize};

use crate::{CommandId, CorrelationId, DeliveryMode, InputId, ModelRef, PromptText, SessionId};

/// A command and the metadata used to correlate its resulting events.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandEnvelope {
    command_id: CommandId,
    correlation_id: CorrelationId,
    payload: CommandPayload,
}

impl CommandEnvelope {
    /// Constructs a command envelope.
    #[must_use]
    pub const fn new(
        command_id: CommandId,
        correlation_id: CorrelationId,
        payload: CommandPayload,
    ) -> Self {
        Self {
            command_id,
            correlation_id,
            payload,
        }
    }

    /// Returns the command identity used for event causation.
    #[must_use]
    pub const fn command_id(&self) -> &CommandId {
        &self.command_id
    }

    /// Returns the logical-operation correlation identity.
    #[must_use]
    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    /// Returns the requested intent.
    #[must_use]
    pub const fn payload(&self) -> &CommandPayload {
        &self.payload
    }

    /// Returns the target session for routing to its single logical writer.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        self.payload.session_id()
    }
}

/// Requested session intent. A variant does not imply that it succeeded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum CommandPayload {
    /// Create a new session aggregate.
    CreateSession {
        /// Stable identity selected before durable creation.
        session_id: SessionId,
    },
    /// Select a provider-neutral model for subsequent turns.
    SelectModel {
        /// Target session.
        session_id: SessionId,
        /// Selected provider and model.
        model: ModelRef,
    },
    /// Admit exact user input durably before provider execution.
    AdmitPrompt {
        /// Target session.
        session_id: SessionId,
        /// Stable identity used to reject duplicate admission.
        input_id: InputId,
        /// Exact user-authored content.
        prompt: PromptText,
        /// Boundary at which the input becomes eligible.
        delivery_mode: DeliveryMode,
    },
}

impl CommandPayload {
    /// Returns the target session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        match self {
            Self::CreateSession { session_id }
            | Self::SelectModel { session_id, .. }
            | Self::AdmitPrompt { session_id, .. } => session_id,
        }
    }
}
