use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

/// A stable, provider-neutral error category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// A supplied value or command is invalid.
    Validation,
    /// A requested resource does not exist.
    NotFound,
    /// Current state conflicts with the requested operation.
    Conflict,
    /// Authentication failed or is unavailable.
    Authentication,
    /// The caller lacks required authority.
    PermissionDenied,
    /// A configured or provider-enforced rate limit was reached.
    RateLimited,
    /// An operation exceeded its deadline.
    Timeout,
    /// A dependency is temporarily unavailable.
    Unavailable,
    /// An operation was cancelled.
    Cancelled,
    /// A protocol response violated its contract.
    Protocol,
    /// Durable storage failed.
    Storage,
    /// An internal invariant failed.
    Internal,
}

/// Advice used by policy code when deciding whether to retry a failure.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RetryAdvice {
    /// Retrying the same operation is not safe or useful.
    Never,
    /// The same operation may be retried immediately.
    Immediate,
    /// Retry after policy-controlled exponential backoff.
    Backoff,
    /// Retry no earlier than the supplied provider or policy delay.
    After {
        /// Minimum delay before another attempt.
        delay_ms: u64,
    },
}

/// Exposes stable classification without leaking an implementation-specific source error.
pub trait ClassifiedError: Error {
    /// Returns the stable category for this failure.
    fn class(&self) -> ErrorClass;

    /// Returns the retry policy for this failure.
    fn retry_advice(&self) -> RetryAdvice;
}

/// A validation error that never includes the rejected value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueError {
    /// An identifier was empty.
    EmptyIdentifier,
    /// An identifier contained whitespace.
    IdentifierContainsWhitespace,
    /// An identifier contained a control character.
    IdentifierContainsControlCharacter,
    /// An identifier contained a character outside the stable log-safe alphabet.
    IdentifierContainsUnsafeCharacter,
    /// An identifier exceeded the supported byte length.
    IdentifierTooLong,
    /// A prompt was empty or contained only whitespace.
    EmptyPrompt,
    /// A session title was empty or contained only whitespace.
    EmptySessionTitle,
    /// A session title exceeded its durable byte bound.
    SessionTitleTooLong,
    /// A session title contained a character unsafe for terminal display.
    InvalidSessionTitle,
    /// A provider response delta contained no bytes.
    EmptyResponseText,
    /// A public message was empty or contained only whitespace.
    EmptyPublicMessage,
    /// A public message exceeded its durable byte bound.
    PublicMessageTooLong,
    /// Tool arguments were not a JSON object or exceeded their durable bound.
    InvalidToolArguments,
    /// A capability resource was empty, unsafe for logs, or exceeded its durable bound.
    InvalidResource,
    /// A run limit was zero or internally inconsistent.
    InvalidRunLimits,
    /// Tool output exceeded its durable inline bound.
    ToolOutputTooLong,
    /// Artifact metadata was invalid.
    InvalidArtifact,
    /// A session event sequence was zero.
    ZeroSequence,
    /// A session event sequence exceeded the signed durable-store range.
    SequenceTooLarge,
    /// Memory content was empty or contained only whitespace.
    EmptyMemoryContent,
    /// Memory content exceeded its durable byte bound.
    MemoryContentTooLong,
    /// A memory evidence excerpt was empty or contained only whitespace.
    EmptyMemoryEvidenceExcerpt,
    /// A memory evidence excerpt exceeded its durable byte bound.
    MemoryEvidenceExcerptTooLong,
    /// A SHA-256 digest was not exactly 64 lowercase hexadecimal characters.
    InvalidSha256Digest,
    /// Confidence exceeded the inclusive basis-point range from zero to 10,000.
    ConfidenceOutOfRange,
    /// A memory-ledger sequence was zero.
    ZeroMemorySequence,
    /// A memory-ledger sequence exceeded the signed durable-store range.
    MemorySequenceTooLarge,
    /// A memory revision number was zero.
    ZeroMemoryRevision,
    /// A memory revision number exceeded the signed durable-store range.
    MemoryRevisionTooLarge,
    /// A memory validity window ended before it began.
    InvalidMemoryValidity,
    /// Memory evidence excerpt fields were not both present or both absent.
    InvalidMemoryEvidence,
    /// Memory validation candidates were inconsistent with reported issues.
    InvalidMemoryValidation,
    /// A create command supplied an expected sequence or an update omitted one.
    InvalidMemoryExpectedSequence,
    /// A bounded collection exceeded its durable item limit.
    CollectionTooLarge,
    /// A schema or algorithm version was zero.
    ZeroVersion,
    /// A context token budget was zero or exceeded the durable-store range.
    InvalidContextTokenBudget,
    /// An estimated token count exceeded the durable-store range.
    EstimatedTokensTooLarge,
    /// A context turn number was zero.
    ZeroContextRunTurn,
    /// A context source observation had inconsistent revision or hash fields.
    InvalidContextObservation,
    /// A memory-store generation exceeded the signed durable-store range.
    MemoryGenerationTooLarge,
    /// A context admission rank or reason ordinal was zero.
    ZeroContextOrdinal,
    /// A context epoch referred to itself as its predecessor.
    InvalidContextEpoch,
    /// A context admission or turn manifest violated a deterministic ordering invariant.
    InvalidContextManifest,
}

impl Display for ValueError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyIdentifier => "identifier must not be empty",
            Self::IdentifierContainsWhitespace => "identifier must not contain whitespace",
            Self::IdentifierContainsControlCharacter => {
                "identifier must not contain control characters"
            }
            Self::IdentifierContainsUnsafeCharacter => {
                "identifier contains a character outside the supported alphabet"
            }
            Self::IdentifierTooLong => "identifier exceeds the supported length",
            Self::EmptyPrompt => "prompt must contain non-whitespace text",
            Self::EmptySessionTitle => "session title must contain non-whitespace text",
            Self::SessionTitleTooLong => "session title exceeds the supported length",
            Self::InvalidSessionTitle => "session title must not contain control characters",
            Self::EmptyResponseText => "response text must not be empty",
            Self::EmptyPublicMessage => "public message must contain non-whitespace text",
            Self::PublicMessageTooLong => "public message exceeds the supported length",
            Self::InvalidToolArguments => "tool arguments must be a bounded JSON object",
            Self::InvalidResource => "capability resource is invalid",
            Self::InvalidRunLimits => "run limits must be non-zero and internally consistent",
            Self::ToolOutputTooLong => "tool output exceeds the durable inline bound",
            Self::InvalidArtifact => "artifact metadata is invalid",
            Self::ZeroSequence => "session sequence must be greater than zero",
            Self::SequenceTooLarge => "session sequence exceeds the durable storage range",
            Self::EmptyMemoryContent => "memory content must contain non-whitespace text",
            Self::MemoryContentTooLong => "memory content exceeds the supported length",
            Self::EmptyMemoryEvidenceExcerpt => {
                "memory evidence excerpt must contain non-whitespace text"
            }
            Self::MemoryEvidenceExcerptTooLong => {
                "memory evidence excerpt exceeds the supported length"
            }
            Self::InvalidSha256Digest => {
                "SHA-256 digest must be 64 lowercase hexadecimal characters"
            }
            Self::ConfidenceOutOfRange => "confidence basis points must be between zero and 10,000",
            Self::ZeroMemorySequence => "memory sequence must be greater than zero",
            Self::MemorySequenceTooLarge => "memory sequence exceeds the durable storage range",
            Self::ZeroMemoryRevision => "memory revision must be greater than zero",
            Self::MemoryRevisionTooLarge => "memory revision exceeds the durable storage range",
            Self::InvalidMemoryValidity => "memory validity window ends before it begins",
            Self::InvalidMemoryEvidence => {
                "memory evidence excerpt and digest must be present together"
            }
            Self::InvalidMemoryValidation => {
                "memory validation candidates require their matching issue"
            }
            Self::InvalidMemoryExpectedSequence => {
                "memory command expected sequence does not match its operation"
            }
            Self::CollectionTooLarge => "collection exceeds the supported item count",
            Self::ZeroVersion => "schema and algorithm versions must be greater than zero",
            Self::InvalidContextTokenBudget => {
                "context token budget must be non-zero and fit durable storage"
            }
            Self::EstimatedTokensTooLarge => {
                "estimated token count exceeds the durable storage range"
            }
            Self::ZeroContextRunTurn => "context run turn must be greater than zero",
            Self::InvalidContextObservation => {
                "context observation fields are inconsistent with its state"
            }
            Self::MemoryGenerationTooLarge => "memory generation exceeds the durable storage range",
            Self::ZeroContextOrdinal => "context ordinals must be greater than zero",
            Self::InvalidContextEpoch => "context epoch cannot be its own predecessor",
            Self::InvalidContextManifest => {
                "context manifest violates a deterministic ordering invariant"
            }
        };

        formatter.write_str(message)
    }
}

impl Error for ValueError {}

impl ClassifiedError for ValueError {
    fn class(&self) -> ErrorClass {
        ErrorClass::Validation
    }

    fn retry_advice(&self) -> RetryAdvice {
        RetryAdvice::Never
    }
}
