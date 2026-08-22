use serde::{Deserialize, Serialize};

use crate::{
    AttemptFailure, AttemptId, CommandId, CorrelationId, DeliveryMode, InputId, ModelRef,
    PermissionAnswer, PermissionDecisionId, PermissionOutcome, PromptText, ResponseText, RunLimits,
    SessionId, SessionTitle, ToolCallId, ToolCallSpec, ToolOutput, UsageSnapshot,
};

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
    /// Replace the user-facing title of an existing session.
    RenameSession {
        /// Target session.
        session_id: SessionId,
        /// Validated replacement title.
        title: SessionTitle,
    },
    /// Retain a session but stop it from accepting ordinary commands.
    ArchiveSession {
        /// Target session.
        session_id: SessionId,
    },
    /// Return an archived session to ordinary command eligibility.
    UnarchiveSession {
        /// Target session.
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
    /// Atomically admit exact input and prepare its first provider attempt.
    AdmitPromptAndPrepareAttempt {
        /// Target session.
        session_id: SessionId,
        /// Stable identity used to reject duplicate admission.
        input_id: InputId,
        /// Exact user-authored content.
        prompt: PromptText,
        /// Boundary at which the input becomes eligible.
        delivery_mode: DeliveryMode,
        /// Stable attempt identity selected before any provider effect.
        attempt_id: AttemptId,
    },
    /// Prepare an attempt and bind its exact input and model before dispatch.
    PrepareAttempt {
        /// Target session.
        session_id: SessionId,
        /// Stable attempt identity selected before any provider effect.
        attempt_id: AttemptId,
        /// Exact admitted input promoted into the attempt.
        input_id: InputId,
        /// Prior settled attempt when this is an explicit retry.
        retry_of: Option<AttemptId>,
    },
    /// Record the durable dispatch boundary immediately before provider I/O.
    StartAttempt {
        /// Target session.
        session_id: SessionId,
        /// Prepared attempt being dispatched.
        attempt_id: AttemptId,
    },
    /// Append an exact provider-neutral response delta.
    AppendAttemptText {
        /// Target session.
        session_id: SessionId,
        /// Active attempt receiving content.
        attempt_id: AttemptId,
        /// Exact response bytes represented as UTF-8 text.
        text: ResponseText,
    },
    /// Replace the cumulative usage snapshot for an active attempt.
    RecordAttemptUsage {
        /// Target session.
        session_id: SessionId,
        /// Active attempt receiving usage.
        attempt_id: AttemptId,
        /// Provider-neutral cumulative usage.
        usage: UsageSnapshot,
    },
    /// Request cancellation before signalling the in-memory provider task.
    RequestAttemptCancellation {
        /// Target session.
        session_id: SessionId,
        /// Active attempt to cancel.
        attempt_id: AttemptId,
    },
    /// Settle an attempt successfully.
    CompleteAttempt {
        /// Target session.
        session_id: SessionId,
        /// Active attempt that completed.
        attempt_id: AttemptId,
    },
    /// Settle an attempt with a sanitized provider-neutral failure.
    FailAttempt {
        /// Target session.
        session_id: SessionId,
        /// Attempt that failed.
        attempt_id: AttemptId,
        /// Safe failure data suitable for persistence and display.
        failure: AttemptFailure,
    },
    /// Settle an attempt after cooperative cancellation.
    CancelAttempt {
        /// Target session.
        session_id: SessionId,
        /// Attempt that observed cancellation.
        attempt_id: AttemptId,
    },
    /// Mark a previously dispatched attempt as ambiguous after recovery.
    MarkAttemptUnknown {
        /// Target session.
        session_id: SessionId,
        /// In-flight attempt whose provider outcome is unknown.
        attempt_id: AttemptId,
    },
    /// Freeze all authority limits before the first provider turn.
    ConfigureRunBudget {
        /// Target session.
        session_id: SessionId,
        /// Prepared attempt that owns the agent run.
        attempt_id: AttemptId,
        /// Immutable bounded limits.
        limits: RunLimits,
    },
    /// Record a provider-turn dispatch boundary within an active agent run.
    StartRunTurn {
        /// Target session.
        session_id: SessionId,
        /// Active attempt receiving the turn.
        attempt_id: AttemptId,
    },
    /// Admit a provider-requested tool call only after trusted planning derived authority.
    ProposeToolCall {
        /// Target session.
        session_id: SessionId,
        /// Active attempt that emitted the call.
        attempt_id: AttemptId,
        /// Frozen call and capability details.
        call: ToolCallSpec,
    },
    /// Record the policy result for an exact frozen call.
    RecordToolPermission {
        /// Target session.
        session_id: SessionId,
        /// Tool call being evaluated.
        tool_call_id: ToolCallId,
        /// Stable policy-decision identity.
        decision_id: PermissionDecisionId,
        /// Deny, ask, or allow result.
        outcome: PermissionOutcome,
    },
    /// Resolve a durable human permission request.
    AnswerToolPermission {
        /// Target session.
        session_id: SessionId,
        /// Tool call whose ask decision is pending.
        tool_call_id: ToolCallId,
        /// Stable human-decision identity.
        decision_id: PermissionDecisionId,
        /// Allow once or deny answer.
        answer: PermissionAnswer,
    },
    /// Record the last durable boundary before a tool may perform an external effect.
    StartToolCall {
        /// Target session.
        session_id: SessionId,
        /// Authorized tool call entering execution.
        tool_call_id: ToolCallId,
    },
    /// Settle a tool call with bounded captured output.
    CompleteToolCall {
        /// Target session.
        session_id: SessionId,
        /// Executed tool call.
        tool_call_id: ToolCallId,
        /// Bounded output and optional full artifact reference.
        output: ToolOutput,
    },
    /// Settle a tool call with a sanitized failure.
    FailToolCall {
        /// Target session.
        session_id: SessionId,
        /// Tool call that failed.
        tool_call_id: ToolCallId,
        /// Safe durable failure.
        failure: AttemptFailure,
    },
    /// Settle a call that was denied before execution.
    DenyToolCall {
        /// Target session.
        session_id: SessionId,
        /// Denied tool call.
        tool_call_id: ToolCallId,
    },
    /// Settle a tool call after cooperative cancellation.
    CancelToolCall {
        /// Target session.
        session_id: SessionId,
        /// Cancelled tool call.
        tool_call_id: ToolCallId,
    },
    /// Mark a started tool effect ambiguous during crash recovery.
    MarkToolCallUnknown {
        /// Target session.
        session_id: SessionId,
        /// Tool call whose external outcome cannot be reconciled.
        tool_call_id: ToolCallId,
    },
    /// Record that a provider turn ended with admitted tool calls.
    PauseAttemptForTools {
        /// Target session.
        session_id: SessionId,
        /// Attempt waiting for its calls to settle.
        attempt_id: AttemptId,
    },
    /// Record the next provider dispatch boundary after every tool call settled.
    ResumeAttemptAfterTools {
        /// Target session.
        session_id: SessionId,
        /// Attempt resuming its model loop.
        attempt_id: AttemptId,
    },
}

impl CommandPayload {
    /// Returns the target session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        match self {
            Self::CreateSession { session_id }
            | Self::RenameSession { session_id, .. }
            | Self::ArchiveSession { session_id }
            | Self::UnarchiveSession { session_id }
            | Self::SelectModel { session_id, .. }
            | Self::AdmitPrompt { session_id, .. }
            | Self::AdmitPromptAndPrepareAttempt { session_id, .. }
            | Self::PrepareAttempt { session_id, .. }
            | Self::StartAttempt { session_id, .. }
            | Self::AppendAttemptText { session_id, .. }
            | Self::RecordAttemptUsage { session_id, .. }
            | Self::RequestAttemptCancellation { session_id, .. }
            | Self::CompleteAttempt { session_id, .. }
            | Self::FailAttempt { session_id, .. }
            | Self::CancelAttempt { session_id, .. }
            | Self::MarkAttemptUnknown { session_id, .. }
            | Self::ConfigureRunBudget { session_id, .. }
            | Self::StartRunTurn { session_id, .. }
            | Self::ProposeToolCall { session_id, .. }
            | Self::RecordToolPermission { session_id, .. }
            | Self::AnswerToolPermission { session_id, .. }
            | Self::StartToolCall { session_id, .. }
            | Self::CompleteToolCall { session_id, .. }
            | Self::FailToolCall { session_id, .. }
            | Self::DenyToolCall { session_id, .. }
            | Self::CancelToolCall { session_id, .. }
            | Self::MarkToolCallUnknown { session_id, .. }
            | Self::PauseAttemptForTools { session_id, .. }
            | Self::ResumeAttemptAfterTools { session_id, .. } => session_id,
        }
    }
}
