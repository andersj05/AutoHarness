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
    /// A provider response delta contained no bytes.
    EmptyResponseText,
    /// A public message was empty or contained only whitespace.
    EmptyPublicMessage,
    /// A public message exceeded its durable byte bound.
    PublicMessageTooLong,
    /// A session event sequence was zero.
    ZeroSequence,
    /// A session event sequence exceeded the signed durable-store range.
    SequenceTooLarge,
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
            Self::EmptyResponseText => "response text must not be empty",
            Self::EmptyPublicMessage => "public message must contain non-whitespace text",
            Self::PublicMessageTooLong => "public message exceeds the supported length",
            Self::ZeroSequence => "session sequence must be greater than zero",
            Self::SequenceTooLarge => "session sequence exceeds the durable storage range",
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
