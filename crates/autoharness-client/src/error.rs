use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Validation failure for one public client-contract value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    /// A required value was empty or only whitespace.
    Empty { field: &'static str },
    /// A UTF-8 value exceeded its byte bound.
    TooLong {
        field: &'static str,
        max_bytes: usize,
        actual_bytes: usize,
    },
    /// A collection exceeded its item bound.
    TooMany {
        field: &'static str,
        max_items: usize,
        actual_items: usize,
    },
    /// A value contains characters or syntax forbidden by the contract.
    Invalid { field: &'static str },
    /// A sequence-like value used the reserved zero value.
    Zero { field: &'static str },
    /// A monotonic sequence cannot advance further.
    Overflow { field: &'static str },
    /// Fields that must agree describe different state.
    Inconsistent { field: &'static str },
}

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(formatter, "{field} must not be empty"),
            Self::TooLong {
                field,
                max_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "{field} exceeds {max_bytes} bytes ({actual_bytes} bytes supplied)"
            ),
            Self::TooMany {
                field,
                max_items,
                actual_items,
            } => write!(
                formatter,
                "{field} exceeds {max_items} items ({actual_items} items supplied)"
            ),
            Self::Invalid { field } => write!(formatter, "{field} is invalid"),
            Self::Zero { field } => write!(formatter, "{field} must be greater than zero"),
            Self::Overflow { field } => {
                write!(formatter, "{field} cannot advance past its maximum")
            }
            Self::Inconsistent { field } => write!(formatter, "{field} is inconsistent"),
        }
    }
}

impl Error for ValidationError {}
