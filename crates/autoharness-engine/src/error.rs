use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    ClassifiedError, CommandId, ErrorClass, EventId, InputId, RetryAdvice, SessionId,
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
    /// An input identity was already admitted to the session.
    DuplicateInput {
        /// Owning session identity.
        session_id: SessionId,
        /// Conflicting input identity.
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
            Self::DuplicateInput {
                session_id,
                input_id,
            } => write!(
                formatter,
                "input {input_id} is already admitted to session {session_id}"
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
            Self::SessionNotFound { .. } => ErrorClass::NotFound,
            Self::DuplicateCommand { .. }
            | Self::SessionAlreadyExists { .. }
            | Self::DuplicateInput { .. } => ErrorClass::Conflict,
            Self::WrongSession { .. } => ErrorClass::Validation,
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
            Self::DuplicateInput {
                session_id,
                input_id,
                event_id,
            } => write!(
                formatter,
                "event {event_id} admits duplicate input {input_id} to session {session_id}"
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
