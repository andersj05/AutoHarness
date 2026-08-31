use std::fmt::{self, Display, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ValidationError;
use crate::bounds::validate_id;

macro_rules! string_id {
    ($name:ident, $field:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Constructs a validated stable identity.
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                let value = value.into();
                validate_id($field, &value)?;
                Ok(Self(value))
            }

            /// Returns the exact stable string identity.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identity and returns its string form.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

string_id!(SessionId, "session_id", "Stable durable session identity.");
string_id!(InputId, "input_id", "Stable durable user-input identity.");
string_id!(
    AttemptId,
    "attempt_id",
    "Stable durable model-attempt identity."
);
string_id!(
    ToolCallId,
    "tool_call_id",
    "Stable durable tool-call identity."
);
string_id!(
    ProviderId,
    "provider_id",
    "Stable provider adapter identity."
);
string_id!(
    ConnectionId,
    "connection_id",
    "Stable named provider-connection identity."
);
string_id!(ModelId, "model_id", "Stable provider-owned model identity.");

fn deserialize_decimal_u64<'de, D>(
    deserializer: D,
    field: &'static str,
    allow_zero: bool,
) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(D::Error::custom(ValidationError::Invalid { field }));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| D::Error::custom(ValidationError::Invalid { field }))?;
    if parsed == 0 && !allow_zero {
        return Err(D::Error::custom(ValidationError::Zero { field }));
    }
    Ok(parsed)
}

fn serialize_decimal_u64<S>(value: u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Exact unsigned wire integer serialized as a canonical decimal string.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DecimalU64(u64);

impl DecimalU64 {
    /// Constructs an exact unsigned wire integer.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying Rust integer.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for DecimalU64 {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl Serialize for DecimalU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_decimal_u64(self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for DecimalU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(deserialize_decimal_u64(
            deserializer,
            "decimal_u64",
            true,
        )?))
    }
}

/// Exact Unix-epoch millisecond value serialized as a canonical decimal string.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixMillis(i64);

impl UnixMillis {
    /// Constructs an exact signed Unix-epoch millisecond value.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the underlying Rust integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl From<i64> for UnixMillis {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl Serialize for UnixMillis {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for UnixMillis {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let parsed = value
            .parse::<i64>()
            .map_err(|_| D::Error::custom(ValidationError::Invalid { field: "unix_ms" }))?;
        if parsed.to_string() != value {
            return Err(D::Error::custom(ValidationError::Invalid {
                field: "unix_ms",
            }));
        }
        Ok(Self(parsed))
    }
}

/// Rust-issued process-local request identity.
///
/// The wire form is a decimal string so JavaScript never rounds the identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    /// Issues an identity from a positive host-owned sequence.
    pub const fn new(value: u64) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::Zero {
                field: "request_id",
            });
        }
        Ok(Self(value))
    }

    /// Returns the host sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Display for RequestId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_decimal_u64(self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(deserialize_decimal_u64(deserializer, "request_id", false)?)
            .map_err(D::Error::custom)
    }
}

/// Monotonic carrier revision shared by every frame type.
///
/// Revisions start at one and serialize as decimal strings.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransportRevision(u64);

impl TransportRevision {
    /// First valid transport revision.
    pub const INITIAL: Self = Self(1);

    /// Constructs a positive transport revision.
    pub const fn new(value: u64) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::Zero {
                field: "transport_revision",
            });
        }
        Ok(Self(value))
    }

    /// Returns the underlying monotonic value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision, or fails closed at `u64::MAX`.
    pub const fn next(self) -> Result<Self, ValidationError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(ValidationError::Overflow {
                field: "transport_revision",
            }),
        }
    }
}

impl Serialize for TransportRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_decimal_u64(self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for TransportRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(deserialize_decimal_u64(
            deserializer,
            "transport_revision",
            false,
        )?)
        .map_err(D::Error::custom)
    }
}

/// Durable session projection revision.
///
/// Zero represents a durable session with no committed events yet.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionRevision(u64);

impl SessionRevision {
    /// Constructs a durable projection revision.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying durable sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Serialize for SessionRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_decimal_u64(self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for SessionRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(deserialize_decimal_u64(
            deserializer,
            "session_revision",
            true,
        )?))
    }
}
