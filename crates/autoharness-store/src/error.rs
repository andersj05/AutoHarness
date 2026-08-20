use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{ClassifiedError, ErrorClass, RetryAdvice, SessionId};

/// The durable identity whose uniqueness was violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityKind {
    /// A stable event identity was reused.
    Event,
    /// A single-use command identity was reused.
    Command,
    /// An admitted input identity was reused within a session.
    Input,
    /// A provider-attempt identity was reused.
    Attempt,
    /// A backend constraint failed without a safely attributable identity.
    Constraint,
}

/// The safe category of persisted data that failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorruptionArea {
    /// A serialized event envelope could not be decoded or did not match its indexed columns.
    Event,
    /// Session event ordering was not contiguous.
    EventSequence,
    /// A session projection contained invalid or inconsistent data.
    SessionProjection,
    /// An admitted-input projection contained invalid or inconsistent data.
    InputProjection,
    /// A provider-attempt projection contained invalid or inconsistent data.
    AttemptProjection,
    /// A transcript projection contained invalid or inconsistent data.
    TranscriptProjection,
    /// A migration record did not match the embedded migration set.
    MigrationHistory,
}

/// A sanitized durable-store failure.
///
/// This error intentionally excludes SQL text, event JSON, prompts, paths, and
/// backend error messages so it is safe to present or record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    /// The request did not contain any events.
    EmptyAppend,
    /// An event did not belong to the request's session.
    MixedSessions,
    /// The requested sequence cannot be represented by the current store.
    SequenceOutOfRange,
    /// The event batch did not begin at the expected next sequence.
    NonContiguousBatch,
    /// An event cited a missing, future, or cross-session cause.
    InvalidCausation,
    /// An event payload was not valid for the durable session version.
    InvalidSessionTransition,
    /// A schema version is not supported by this store implementation.
    UnsupportedEventSchema {
        /// Unsupported serialized schema value.
        found: u16,
    },
    /// The current durable session version did not match the caller's expectation.
    VersionConflict {
        /// Session whose logical writer was stale.
        session_id: SessionId,
        /// Version supplied by the caller.
        expected: u64,
        /// Version currently stored, with zero representing an absent session.
        actual: u64,
    },
    /// A stable identity conflicted with an existing durable record.
    IdentityConflict {
        /// Kind of identity involved.
        kind: IdentityKind,
    },
    /// Persisted authoritative or projected data failed closed validation.
    CorruptData {
        /// Safe location of the invalid data.
        area: CorruptionArea,
    },
    /// The database is temporarily busy or locked.
    Busy,
    /// The database uses a schema newer than this binary understands.
    NewerSchema {
        /// Highest schema version found in the database.
        found: u32,
        /// Highest schema version understood by this binary.
        supported: u32,
    },
    /// Database configuration could not meet the required durability policy.
    Configuration,
    /// Schema migration failed without exposing backend details.
    Migration,
    /// A backend operation failed without a safe more-specific classification.
    Backend,
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAppend => formatter.write_str("event append must not be empty"),
            Self::MixedSessions => {
                formatter.write_str("event append contains more than one session")
            }
            Self::SequenceOutOfRange => {
                formatter.write_str("session sequence exceeds the store's supported range")
            }
            Self::NonContiguousBatch => {
                formatter.write_str("event append is not contiguous from the expected version")
            }
            Self::InvalidCausation => {
                formatter.write_str("event append contains invalid causation")
            }
            Self::InvalidSessionTransition => {
                formatter.write_str("event append contains an invalid session transition")
            }
            Self::UnsupportedEventSchema { found } => {
                write!(formatter, "event schema {found} is not supported")
            }
            Self::VersionConflict {
                session_id,
                expected,
                actual,
            } => write!(
                formatter,
                "session {session_id} is at version {actual}, expected {expected}"
            ),
            Self::IdentityConflict { kind } => {
                write!(formatter, "a durable {kind:?} identity already exists")
            }
            Self::CorruptData { area } => {
                write!(
                    formatter,
                    "stored {area:?} data failed integrity validation"
                )
            }
            Self::Busy => formatter.write_str("durable storage is temporarily busy"),
            Self::NewerSchema { found, supported } => write!(
                formatter,
                "database schema {found} is newer than supported schema {supported}"
            ),
            Self::Configuration => {
                formatter.write_str("durable storage configuration requirements were not met")
            }
            Self::Migration => formatter.write_str("durable storage migration failed"),
            Self::Backend => formatter.write_str("durable storage operation failed"),
        }
    }
}

impl Error for StoreError {}

impl ClassifiedError for StoreError {
    fn class(&self) -> ErrorClass {
        match self {
            Self::EmptyAppend
            | Self::MixedSessions
            | Self::SequenceOutOfRange
            | Self::NonContiguousBatch
            | Self::InvalidCausation
            | Self::InvalidSessionTransition
            | Self::UnsupportedEventSchema { .. } => ErrorClass::Validation,
            Self::VersionConflict { .. } | Self::IdentityConflict { .. } => ErrorClass::Conflict,
            Self::Busy => ErrorClass::Unavailable,
            Self::CorruptData { .. }
            | Self::NewerSchema { .. }
            | Self::Configuration
            | Self::Migration
            | Self::Backend => ErrorClass::Storage,
        }
    }

    fn retry_advice(&self) -> RetryAdvice {
        match self {
            Self::VersionConflict { .. } => RetryAdvice::Immediate,
            Self::Busy => RetryAdvice::Backoff,
            Self::EmptyAppend
            | Self::MixedSessions
            | Self::SequenceOutOfRange
            | Self::NonContiguousBatch
            | Self::InvalidCausation
            | Self::InvalidSessionTransition
            | Self::UnsupportedEventSchema { .. }
            | Self::IdentityConflict { .. }
            | Self::CorruptData { .. }
            | Self::NewerSchema { .. }
            | Self::Configuration
            | Self::Migration
            | Self::Backend => RetryAdvice::Never,
        }
    }
}
