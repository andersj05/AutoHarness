use serde::{Deserialize, Serialize};

use crate::{
    AttemptFailure, AttemptId, CommandId, CorrelationId, DeliveryMode, EventId, InputId, ModelRef,
    PermissionAnswer, PermissionDecisionId, PermissionOutcome, PromptText, ResponseText, RunLimits,
    SessionId, SessionSequence, SessionTitle, TimestampMillis, ToolCallId, ToolCallSpec,
    ToolOutput, UsageSnapshot,
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
    /// The session's user-facing title changed.
    SessionRenamed {
        /// Validated replacement title.
        title: SessionTitle,
    },
    /// The session stopped accepting ordinary commands but remains readable.
    SessionArchived,
    /// An archived session returned to ordinary command eligibility.
    SessionUnarchived,
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
    /// An attempt was durably bound to exact input and a model snapshot.
    AttemptPrepared {
        /// Stable attempt identity.
        attempt_id: AttemptId,
        /// Exact input promoted into this attempt.
        input_id: InputId,
        /// Provider-neutral model snapshot selected for the attempt.
        model: ModelRef,
        /// Prior settled attempt when this attempt is an explicit retry.
        retry_of: Option<AttemptId>,
    },
    /// Provider dispatch became possible immediately after this event committed.
    AttemptStarted {
        /// Attempt entering the in-flight state.
        attempt_id: AttemptId,
    },
    /// Exact assistant response content was durably appended.
    AttemptTextAppended {
        /// Attempt receiving the content.
        attempt_id: AttemptId,
        /// Exact provider-neutral text delta.
        text: ResponseText,
    },
    /// A cumulative usage snapshot replaced the previous snapshot.
    AttemptUsageRecorded {
        /// Attempt receiving usage.
        attempt_id: AttemptId,
        /// Provider-neutral cumulative usage.
        usage: UsageSnapshot,
    },
    /// Cancellation was durably requested before signalling the provider task.
    AttemptCancellationRequested {
        /// Attempt targeted by cancellation.
        attempt_id: AttemptId,
    },
    /// An attempt settled successfully.
    AttemptCompleted {
        /// Settled attempt.
        attempt_id: AttemptId,
    },
    /// An attempt settled with a sanitized failure.
    AttemptFailed {
        /// Settled attempt.
        attempt_id: AttemptId,
        /// Safe failure projection.
        failure: AttemptFailure,
    },
    /// An attempt settled after cooperative cancellation.
    AttemptCancelled {
        /// Settled attempt.
        attempt_id: AttemptId,
    },
    /// Recovery found a dispatched attempt with an ambiguous provider outcome.
    AttemptMarkedUnknown {
        /// Ambiguous attempt.
        attempt_id: AttemptId,
    },
    /// Immutable run limits were frozen before provider dispatch.
    RunBudgetConfigured {
        /// Owning attempt.
        attempt_id: AttemptId,
        /// Bounded authority dimensions.
        limits: RunLimits,
    },
    /// A provider turn crossed its durable pre-dispatch boundary.
    RunTurnStarted {
        /// Owning attempt.
        attempt_id: AttemptId,
        /// One-based turn number derived and checked by the engine.
        turn: u32,
    },
    /// A model-authored call and trusted derived capability became durable.
    ToolCallProposed {
        /// Owning provider attempt.
        attempt_id: AttemptId,
        /// Frozen call specification.
        call: ToolCallSpec,
    },
    /// Trusted policy evaluated the exact frozen call and resource.
    ToolPermissionRecorded {
        /// Tool call evaluated.
        tool_call_id: ToolCallId,
        /// Stable policy-decision identity.
        decision_id: PermissionDecisionId,
        /// Deny, ask, or allow result.
        outcome: PermissionOutcome,
    },
    /// A human resolved a prior `ask` decision.
    ToolPermissionAnswered {
        /// Tool call resolved.
        tool_call_id: ToolCallId,
        /// Stable answer identity.
        decision_id: PermissionDecisionId,
        /// Allow once or deny answer.
        answer: PermissionAnswer,
    },
    /// An authorized tool call may perform its external effect after this commit.
    ToolCallStarted {
        /// Tool call entering execution.
        tool_call_id: ToolCallId,
    },
    /// A tool call settled successfully with bounded output.
    ToolCallCompleted {
        /// Settled tool call.
        tool_call_id: ToolCallId,
        /// Bounded result projection.
        output: ToolOutput,
    },
    /// A tool call settled with a sanitized failure.
    ToolCallFailed {
        /// Settled tool call.
        tool_call_id: ToolCallId,
        /// Safe durable failure.
        failure: AttemptFailure,
    },
    /// Policy or a human denied the call before execution.
    ToolCallDenied {
        /// Settled tool call.
        tool_call_id: ToolCallId,
    },
    /// Cooperative cancellation settled the tool call.
    ToolCallCancelled {
        /// Settled tool call.
        tool_call_id: ToolCallId,
    },
    /// Recovery could not reconcile a previously started external effect.
    ToolCallMarkedUnknown {
        /// Ambiguous tool call.
        tool_call_id: ToolCallId,
    },
    /// A provider turn completed and the attempt is durably waiting for tools.
    AttemptPausedForTools {
        /// Waiting attempt.
        attempt_id: AttemptId,
    },
    /// Settled tools were admitted before another provider dispatch.
    AttemptResumedAfterTools {
        /// Resuming attempt.
        attempt_id: AttemptId,
    },
}
