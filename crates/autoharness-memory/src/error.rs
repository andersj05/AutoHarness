use std::fmt::{self, Display, Formatter};

use autoharness_domain::{ContextSourceKey, MemoryRevisionId};

/// Deterministic context or memory policy failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryError {
    /// Two sources attempted to register the same stable key.
    DuplicateSource(ContextSourceKey),
    /// Retained source state contained the same stable key more than once.
    DuplicateRetainedSource(ContextSourceKey),
    /// Retrieval supplied the same immutable memory revision more than once.
    DuplicateMemoryRevision(MemoryRevisionId),
    /// A required source could not provide a usable current or retained value.
    RequiredSourceUnavailable(ContextSourceKey),
    /// A context source returned a section that its authority cannot populate.
    InvalidSourceSection(ContextSourceKey),
    /// A context source value was empty or exceeded its explicit byte bound.
    InvalidSourceValue,
    /// Canonical context rendering exceeded its configured token budget.
    BudgetExceeded,
    /// A bounded arithmetic operation could not be represented durably.
    NumericOverflow,
    /// A domain value produced by deterministic construction was invalid.
    InvalidDomainValue,
}

impl Display for MemoryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DuplicateSource(_) => "a context source key was registered more than once",
            Self::DuplicateRetainedSource(_) => {
                "retained context state contained a duplicate source key"
            }
            Self::DuplicateMemoryRevision(_) => {
                "memory retrieval returned a duplicate immutable revision"
            }
            Self::RequiredSourceUnavailable(_) => {
                "a required context source had no usable current or retained value"
            }
            Self::InvalidSourceSection(_) => {
                "a context source attempted to populate a disallowed section"
            }
            Self::InvalidSourceValue => "a context source value was empty or too large",
            Self::BudgetExceeded => "context content exceeds the configured token budget",
            Self::NumericOverflow => "context sizing exceeded the durable numeric range",
            Self::InvalidDomainValue => "deterministic context construction produced invalid data",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for MemoryError {}
