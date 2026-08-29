use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{ClassifiedError, ErrorClass, MemoryId, RetryAdvice, SessionId};

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
    /// A durable memory-item identity was reused.
    Memory,
    /// A durable memory-revision identity was reused.
    MemoryRevision,
    /// A durable memory-operation identity was reused.
    MemoryOperation,
    /// A context-epoch identity was reused.
    ContextEpoch,
    /// A provider-turn context identity was reused.
    ContextTurn,
    /// A context-admission identity was reused.
    ContextAdmission,
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
    /// A durable model-catalog cache record failed integrity validation.
    CatalogCache,
    /// A migration record did not match the embedded migration set.
    MigrationHistory,
    /// The authoritative memory ledger failed validation.
    MemoryLedger,
    /// A memory lifecycle projection failed validation.
    MemoryProjection,
    /// A context epoch, snapshot, turn, or admission failed validation.
    ContextLedger,
    /// The SQLite FTS memory candidate index failed validation.
    MemorySearch,
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
    /// A memory mutation was not valid for the current durable lifecycle.
    InvalidMemoryTransition,
    /// A context manifest or admission violated its durable boundary.
    InvalidContextTransition,
    /// A bounded memory or context request exceeded a supported store limit.
    LimitExceeded,
    /// A schema version is not supported by this store implementation.
    UnsupportedEventSchema {
        /// Unsupported serialized schema value.
        found: u16,
    },
    /// A memory-ledger schema version is not supported by this store implementation.
    UnsupportedMemorySchema {
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
    /// The current memory-item version did not match the caller's expectation.
    MemoryVersionConflict {
        /// Memory item whose logical writer was stale.
        memory_id: MemoryId,
        /// Version supplied by the caller.
        expected: u64,
        /// Version currently stored, with zero representing an absent item.
        actual: u64,
    },
    /// The global memory generation changed while deterministic context was being built.
    ContextGenerationConflict {
        /// Generation frozen into the context manifest.
        expected: u64,
        /// Generation currently stored.
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
            Self::InvalidMemoryTransition => {
                formatter.write_str("memory mutation contains an invalid lifecycle transition")
            }
            Self::InvalidContextTransition => {
                formatter.write_str("context mutation contains an invalid lifecycle transition")
            }
            Self::LimitExceeded => {
                formatter.write_str("durable storage request exceeds a supported limit")
            }
            Self::UnsupportedEventSchema { found } => {
                write!(formatter, "event schema {found} is not supported")
            }
            Self::UnsupportedMemorySchema { found } => {
                write!(formatter, "memory schema {found} is not supported")
            }
            Self::VersionConflict {
                session_id,
                expected,
                actual,
            } => write!(
                formatter,
                "session {session_id} is at version {actual}, expected {expected}"
            ),
            Self::MemoryVersionConflict {
                memory_id,
                expected,
                actual,
            } => write!(
                formatter,
                "memory {memory_id} is at version {actual}, expected {expected}"
            ),
            Self::ContextGenerationConflict { expected, actual } => write!(
                formatter,
                "memory generation is {actual}, expected {expected} for context commit"
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
            | Self::InvalidMemoryTransition
            | Self::InvalidContextTransition
            | Self::LimitExceeded
            | Self::UnsupportedEventSchema { .. }
            | Self::UnsupportedMemorySchema { .. } => ErrorClass::Validation,
            Self::VersionConflict { .. }
            | Self::MemoryVersionConflict { .. }
            | Self::ContextGenerationConflict { .. }
            | Self::IdentityConflict { .. } => ErrorClass::Conflict,
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
            Self::VersionConflict { .. }
            | Self::MemoryVersionConflict { .. }
            | Self::ContextGenerationConflict { .. } => RetryAdvice::Immediate,
            Self::Busy => RetryAdvice::Backoff,
            Self::EmptyAppend
            | Self::MixedSessions
            | Self::SequenceOutOfRange
            | Self::NonContiguousBatch
            | Self::InvalidCausation
            | Self::InvalidSessionTransition
            | Self::InvalidMemoryTransition
            | Self::InvalidContextTransition
            | Self::LimitExceeded
            | Self::UnsupportedEventSchema { .. }
            | Self::UnsupportedMemorySchema { .. }
            | Self::IdentityConflict { .. }
            | Self::CorruptData { .. }
            | Self::NewerSchema { .. }
            | Self::Configuration
            | Self::Migration
            | Self::Backend => RetryAdvice::Never,
        }
    }
}
