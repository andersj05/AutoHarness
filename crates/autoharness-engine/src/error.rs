use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    AttemptId, ClassifiedError, CommandId, ContextTurnId, ErrorClass, EventId, InputId,
    RetryAdvice, SessionId, ToolCallId,
};

/// Expected rejection of a command that is invalid for current session state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandRejection {
    /// A stable command identity was already accepted.
    DuplicateCommand {
        /// Reused command identity.
        command_id: CommandId,
    },
    /// A create command targeted an active session.
    SessionAlreadyExists {
        /// Conflicting session identity.
        session_id: SessionId,
    },
    /// A command required a session that has not been created.
    SessionNotFound {
        /// Missing session identity.
        session_id: SessionId,
    },
    /// An ordinary command targeted an archived session.
    SessionArchived {
        /// Archived session identity.
        session_id: SessionId,
    },
    /// A lifecycle command contradicted the session's current state.
    InvalidSessionState {
        /// Target session identity.
        session_id: SessionId,
    },
    /// Archive was requested while provider or tool work remained unsettled.
    SessionHasUnsettledWork {
        /// Target session identity.
        session_id: SessionId,
    },
    /// An input identity was already admitted to the session.
    DuplicateInput {
        /// Owning session identity.
        session_id: SessionId,
        /// Conflicting input identity.
        input_id: InputId,
    },
    /// Attempt preparation requires a selected model.
    ModelNotSelected {
        /// Target session identity.
        session_id: SessionId,
    },
    /// An attempt referenced an input that was never admitted.
    InputNotFound {
        /// Target session identity.
        session_id: SessionId,
        /// Missing input identity.
        input_id: InputId,
    },
    /// An ordinary attempt tried to promote an already promoted input.
    InputAlreadyPromoted {
        /// Target session identity.
        session_id: SessionId,
        /// Already promoted input identity.
        input_id: InputId,
    },
    /// An attempt identity already exists in the session.
    DuplicateAttempt {
        /// Target session identity.
        session_id: SessionId,
        /// Conflicting attempt identity.
        attempt_id: AttemptId,
    },
    /// A command referenced an unknown attempt.
    AttemptNotFound {
        /// Target session identity.
        session_id: SessionId,
        /// Missing attempt identity.
        attempt_id: AttemptId,
    },
    /// A tool-call identity already exists in the session.
    DuplicateToolCall {
        /// Target session identity.
        session_id: SessionId,
        /// Conflicting tool-call identity.
        tool_call_id: ToolCallId,
    },
    /// A command referenced an unknown tool call.
    ToolCallNotFound {
        /// Target session identity.
        session_id: SessionId,
        /// Missing tool-call identity.
        tool_call_id: ToolCallId,
    },
    /// A tool call does not permit the requested lifecycle transition.
    InvalidToolCallState {
        /// Target session identity.
        session_id: SessionId,
        /// Tool call in incompatible state.
        tool_call_id: ToolCallId,
    },
    /// An attempt does not permit the requested lifecycle transition.
    InvalidAttemptState {
        /// Target session identity.
        session_id: SessionId,
        /// Attempt in incompatible state.
        attempt_id: AttemptId,
    },
    /// A context-turn identity was already bound in this session.
    DuplicateContextTurn {
        /// Target session identity.
        session_id: SessionId,
        /// Reused context-turn identity.
        context_turn_id: ContextTurnId,
    },
    /// A context binding does not exactly match the next dispatchable turn.
    InvalidContextTurnBinding {
        /// Target session identity.
        session_id: SessionId,
        /// Attempt whose next turn is not exactly bound.
        attempt_id: AttemptId,
    },
    /// A settled attempt's policy does not permit retry.
    RetryNotAllowed {
        /// Target session identity.
        session_id: SessionId,
        /// Prior attempt identity.
        attempt_id: AttemptId,
    },
    /// A retry attempted to bind a different admitted input.
    RetryInputMismatch {
        /// Target session identity.
        session_id: SessionId,
        /// Prior attempt identity.
        attempt_id: AttemptId,
        /// Supplied input identity.
        input_id: InputId,
    },
    /// A command was routed to a different session aggregate.
    WrongSession {
        /// Session owned by the aggregate.
        expected: SessionId,
        /// Session targeted by the command.
        found: SessionId,
    },
}

impl Display for CommandRejection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCommand { command_id } => {
                write!(formatter, "command {command_id} was already accepted")
            }
            Self::SessionAlreadyExists { session_id } => {
                write!(formatter, "session {session_id} already exists")
            }
            Self::SessionNotFound { session_id } => {
                write!(formatter, "session {session_id} does not exist")
            }
            Self::SessionArchived { session_id } => write!(
                formatter,
                "session {session_id} is archived and accepts no ordinary commands"
            ),
            Self::InvalidSessionState { session_id } => write!(
                formatter,
                "session {session_id} is not in a state that permits that lifecycle command"
            ),
            Self::SessionHasUnsettledWork { session_id } => write!(
                formatter,
                "session {session_id} still has unsettled provider or tool work"
            ),
            Self::DuplicateInput {
                session_id,
                input_id,
            } => write!(
                formatter,
                "input {input_id} is already admitted to session {session_id}"
            ),
            Self::ModelNotSelected { session_id } => {
                write!(formatter, "session {session_id} has no selected model")
            }
            Self::InputNotFound {
                session_id,
                input_id,
            } => write!(
                formatter,
                "input {input_id} does not exist in session {session_id}"
            ),
            Self::InputAlreadyPromoted {
                session_id,
                input_id,
            } => write!(
                formatter,
                "input {input_id} is already promoted in session {session_id}"
            ),
            Self::DuplicateAttempt {
                session_id,
                attempt_id,
            } => write!(
                formatter,
                "attempt {attempt_id} already exists in session {session_id}"
            ),
            Self::AttemptNotFound {
                session_id,
                attempt_id,
            } => write!(
                formatter,
                "attempt {attempt_id} does not exist in session {session_id}"
            ),
            Self::InvalidAttemptState {
                session_id,
                attempt_id,
            } => write!(
                formatter,
                "attempt {attempt_id} cannot make that transition in session {session_id}"
            ),
            Self::DuplicateContextTurn {
                session_id,
                context_turn_id,
            } => write!(
                formatter,
                "context turn {context_turn_id} is already bound in session {session_id}"
            ),
            Self::InvalidContextTurnBinding {
                session_id,
                attempt_id,
            } => write!(
                formatter,
                "attempt {attempt_id} has no exact adjacent context binding for its next turn in session {session_id}"
            ),
            Self::DuplicateToolCall {
                session_id,
                tool_call_id,
            } => write!(
                formatter,
                "tool call {tool_call_id} already exists in session {session_id}"
            ),
            Self::ToolCallNotFound {
                session_id,
                tool_call_id,
            } => write!(
                formatter,
                "tool call {tool_call_id} does not exist in session {session_id}"
            ),
            Self::InvalidToolCallState {
                session_id,
                tool_call_id,
            } => write!(
                formatter,
                "tool call {tool_call_id} cannot make that transition in session {session_id}"
            ),
            Self::RetryNotAllowed {
                session_id,
                attempt_id,
            } => write!(
                formatter,
                "attempt {attempt_id} cannot be retried in session {session_id}"
            ),
            Self::RetryInputMismatch {
                session_id,
                attempt_id,
                input_id,
            } => write!(
                formatter,
                "retry of attempt {attempt_id} cannot use input {input_id} in session {session_id}"
            ),
            Self::WrongSession { expected, found } => write!(
                formatter,
                "command targets session {found}, but aggregate owns {expected}"
            ),
        }
    }
}

impl Error for CommandRejection {}

impl ClassifiedError for CommandRejection {
    fn class(&self) -> ErrorClass {
        match self {
            Self::SessionNotFound { .. }
            | Self::InputNotFound { .. }
            | Self::AttemptNotFound { .. } => ErrorClass::NotFound,
            Self::ToolCallNotFound { .. } => ErrorClass::NotFound,
            Self::DuplicateCommand { .. }
            | Self::SessionAlreadyExists { .. }
            | Self::SessionArchived { .. }
            | Self::InvalidSessionState { .. }
            | Self::SessionHasUnsettledWork { .. }
            | Self::DuplicateInput { .. }
            | Self::InputAlreadyPromoted { .. }
            | Self::DuplicateAttempt { .. }
            | Self::InvalidAttemptState { .. }
            | Self::DuplicateContextTurn { .. }
            | Self::InvalidContextTurnBinding { .. }
            | Self::DuplicateToolCall { .. }
            | Self::InvalidToolCallState { .. }
            | Self::RetryNotAllowed { .. } => ErrorClass::Conflict,
            Self::WrongSession { .. }
            | Self::ModelNotSelected { .. }
            | Self::RetryInputMismatch { .. } => ErrorClass::Validation,
        }
    }

    fn retry_advice(&self) -> RetryAdvice {
        RetryAdvice::Never
    }
}

/// Integrity failure found while applying or replaying durable events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayError {
    /// The event uses a schema this engine does not understand.
    UnsupportedSchema {
        /// Event being validated.
        event_id: EventId,
        /// Unsupported schema value.
        found: u16,
    },
    /// An event belongs to a different session aggregate.
    WrongSession {
        /// Session owned by the aggregate.
        expected: SessionId,
        /// Session carried by the event.
        found: SessionId,
        /// Event being validated.
        event_id: EventId,
    },
    /// An attempt identity appears more than once in session history.
    DuplicateAttempt {
        /// Owning session identity.
        session_id: SessionId,
        /// Repeated attempt identity.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A tool-call identity appears more than once in session history.
    DuplicateToolCall {
        /// Owning session identity.
        session_id: SessionId,
        /// Repeated tool-call identity.
        tool_call_id: ToolCallId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A tool lifecycle event referenced an unknown call.
    UnknownToolCall {
        /// Owning session identity.
        session_id: SessionId,
        /// Missing tool-call identity.
        tool_call_id: ToolCallId,
        /// Event being validated.
        event_id: EventId,
    },
    /// Tool-call history contains an invalid lifecycle transition.
    IllegalToolCallTransition {
        /// Owning session identity.
        session_id: SessionId,
        /// Tool call in incompatible state.
        tool_call_id: ToolCallId,
        /// Event being validated.
        event_id: EventId,
    },
    /// Attempt preparation referenced an input absent from history.
    UnknownInput {
        /// Owning session identity.
        session_id: SessionId,
        /// Missing input identity.
        input_id: InputId,
        /// Event being validated.
        event_id: EventId,
    },
    /// An attempt lifecycle event referenced an unknown attempt.
    UnknownAttempt {
        /// Owning session identity.
        session_id: SessionId,
        /// Missing attempt identity.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// Attempt history contains an invalid lifecycle transition.
    IllegalAttemptTransition {
        /// Owning session identity.
        session_id: SessionId,
        /// Attempt in incompatible state.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A context-turn identity appears more than once in session history.
    DuplicateContextTurn {
        /// Owning session identity.
        session_id: SessionId,
        /// Repeated context-turn identity.
        context_turn_id: ContextTurnId,
        /// Event being validated.
        event_id: EventId,
    },
    /// Context binding history does not match a dispatchable next turn.
    IllegalContextTurnBinding {
        /// Owning session identity.
        session_id: SessionId,
        /// Attempt with an invalid or missing binding.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// Retry history disagrees with the prior settled attempt.
    InvalidRetry {
        /// Owning session identity.
        session_id: SessionId,
        /// New retry attempt identity.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// An ordinary attempt reused an already promoted input.
    InputAlreadyPromoted {
        /// Owning session identity.
        session_id: SessionId,
        /// Reused input identity.
        input_id: InputId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A prepared attempt's model does not match the selected model at that sequence.
    ModelSnapshotMismatch {
        /// Owning session identity.
        session_id: SessionId,
        /// Attempt with the invalid snapshot.
        attempt_id: AttemptId,
        /// Event being validated.
        event_id: EventId,
    },
    /// The supplied event order is not exactly contiguous.
    NonContiguousSequence {
        /// Owning session identity.
        session_id: SessionId,
        /// Next required one-based sequence.
        expected: u64,
        /// Sequence carried by the event.
        found: u64,
        /// Event being validated.
        event_id: EventId,
    },
    /// An event identity appears more than once in history.
    DuplicateEventId {
        /// Repeated event identity.
        event_id: EventId,
    },
    /// More than one direct event cites the same single-use command identity.
    DuplicateCommandCausation {
        /// Event being validated.
        event_id: EventId,
        /// Reused direct-cause command identity.
        command_id: CommandId,
    },
    /// Event causation points to an event that has not already been applied.
    UnknownCausation {
        /// Event being validated.
        event_id: EventId,
        /// Missing or future direct-cause event identity.
        cause_event_id: EventId,
    },
    /// A create event appeared after the session became active.
    SessionAlreadyCreated {
        /// Owning session identity.
        session_id: SessionId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A non-create event appeared before session creation.
    SessionNotCreated {
        /// Owning session identity.
        session_id: SessionId,
        /// Event being validated.
        event_id: EventId,
    },
    /// A lifecycle event contradicted the session's replayed state.
    IllegalSessionTransition {
        /// Owning session identity.
        session_id: SessionId,
        /// Event being validated.
        event_id: EventId,
    },
    /// An input identity appears more than once in a session's history.
    DuplicateInput {
        /// Owning session identity.
        session_id: SessionId,
        /// Repeated input identity.
        input_id: InputId,
        /// Event being validated.
        event_id: EventId,
    },
    /// The session sequence reached its integer limit.
    SequenceExhausted {
        /// Owning session identity.
        session_id: SessionId,
    },
}

impl Display for ReplayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { event_id, found } => {
                write!(
                    formatter,
                    "event {event_id} uses unsupported schema {found}"
                )
            }
            Self::WrongSession {
                expected,
                found,
                event_id,
            } => write!(
                formatter,
                "event {event_id} belongs to session {found}, expected {expected}"
            ),
            Self::NonContiguousSequence {
                session_id,
                expected,
                found,
                event_id,
            } => write!(
                formatter,
                "event {event_id} has sequence {found} for session {session_id}, expected {expected}"
            ),
            Self::DuplicateEventId { event_id } => {
                write!(formatter, "event ID {event_id} appears more than once")
            }
            Self::DuplicateCommandCausation {
                event_id,
                command_id,
            } => write!(
                formatter,
                "event {event_id} reuses direct-cause command {command_id}"
            ),
            Self::UnknownCausation {
                event_id,
                cause_event_id,
            } => write!(
                formatter,
                "event {event_id} cites unapplied cause event {cause_event_id}"
            ),
            Self::SessionAlreadyCreated {
                session_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} creates active session {session_id} again"
            ),
            Self::SessionNotCreated {
                session_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} precedes creation of session {session_id}"
            ),
            Self::IllegalSessionTransition {
                session_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} contradicts the replayed lifecycle state of session {session_id}"
            ),
            Self::DuplicateInput {
                session_id,
                input_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} admits duplicate input {input_id} to session {session_id}"
            ),
            Self::DuplicateAttempt {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} creates duplicate attempt {attempt_id} in session {session_id}"
            ),
            Self::UnknownInput {
                session_id,
                input_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} references unknown input {input_id} in session {session_id}"
            ),
            Self::DuplicateToolCall {
                session_id,
                tool_call_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} creates duplicate tool call {tool_call_id} in session {session_id}"
            ),
            Self::UnknownToolCall {
                session_id,
                tool_call_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} references unknown tool call {tool_call_id} in session {session_id}"
            ),
            Self::IllegalToolCallTransition {
                session_id,
                tool_call_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} makes an illegal transition for tool call {tool_call_id} in session {session_id}"
            ),
            Self::UnknownAttempt {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} references unknown attempt {attempt_id} in session {session_id}"
            ),
            Self::IllegalAttemptTransition {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} makes an illegal transition for attempt {attempt_id} in session {session_id}"
            ),
            Self::DuplicateContextTurn {
                session_id,
                context_turn_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} reuses context turn {context_turn_id} in session {session_id}"
            ),
            Self::IllegalContextTurnBinding {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} records an invalid context boundary for attempt {attempt_id} in session {session_id}"
            ),
            Self::InvalidRetry {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} defines invalid retry attempt {attempt_id} in session {session_id}"
            ),
            Self::InputAlreadyPromoted {
                session_id,
                input_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} re-promotes input {input_id} in session {session_id}"
            ),
            Self::ModelSnapshotMismatch {
                session_id,
                attempt_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} records a mismatched model for attempt {attempt_id} in session {session_id}"
            ),
            Self::SequenceExhausted { session_id } => {
                write!(
                    formatter,
                    "event sequence is exhausted for session {session_id}"
                )
            }
        }
    }
}

impl Error for ReplayError {}

impl ClassifiedError for ReplayError {
    fn class(&self) -> ErrorClass {
        ErrorClass::Storage
    }

    fn retry_advice(&self) -> RetryAdvice {
        RetryAdvice::Never
    }
}

/// Failure to execute a command through the in-memory engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineError {
    /// Current state rejected the requested command.
    CommandRejected(CommandRejection),
    /// The injected metadata source produced an existing event identity.
    EventIdCollision {
        /// Conflicting event identity.
        event_id: EventId,
    },
    /// The session sequence reached its integer limit.
    SequenceExhausted {
        /// Owning session identity.
        session_id: SessionId,
    },
    /// The engine generated an event batch that failed replay validation.
    InvariantViolation {
        /// Safe replay-integrity error.
        source: ReplayError,
    },
}

impl Display for EngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandRejected(source) => Display::fmt(source, formatter),
            Self::EventIdCollision { event_id } => {
                write!(formatter, "metadata source repeated event ID {event_id}")
            }
            Self::SequenceExhausted { session_id } => {
                write!(
                    formatter,
                    "event sequence is exhausted for session {session_id}"
                )
            }
            Self::InvariantViolation { .. } => {
                formatter.write_str("engine-generated event batch violated replay invariants")
            }
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CommandRejected(source) => Some(source),
            Self::InvariantViolation { source } => Some(source),
            Self::EventIdCollision { .. } | Self::SequenceExhausted { .. } => None,
        }
    }
}

impl ClassifiedError for EngineError {
    fn class(&self) -> ErrorClass {
        match self {
            Self::CommandRejected(source) => source.class(),
            Self::EventIdCollision { .. }
            | Self::SequenceExhausted { .. }
            | Self::InvariantViolation { .. } => ErrorClass::Internal,
        }
    }

    fn retry_advice(&self) -> RetryAdvice {
        match self {
            Self::CommandRejected(source) => source.retry_advice(),
            Self::EventIdCollision { .. }
            | Self::SequenceExhausted { .. }
            | Self::InvariantViolation { .. } => RetryAdvice::Never,
        }
    }
}

impl From<CommandRejection> for EngineError {
    fn from(value: CommandRejection) -> Self {
        Self::CommandRejected(value)
    }
}
