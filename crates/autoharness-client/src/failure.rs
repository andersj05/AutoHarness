use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::bounds::{
    MAX_FAILURE_CODE_BYTES, MAX_FAILURE_MESSAGE_BYTES, MAX_RETRY_DELAY_MS, validate_non_empty_text,
};
use crate::{DecimalU64, ValidationError};

/// Stable renderer-neutral failure class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    Validation,
    NotFound,
    Conflict,
    Authentication,
    PermissionDenied,
    RateLimited,
    Timeout,
    Unavailable,
    Cancelled,
    Protocol,
    Storage,
    Internal,
}

/// Safe retry guidance supplied by the Rust authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum RetryDirective {
    Never,
    Immediate,
    After { delay_ms: DecimalU64 },
}

impl RetryDirective {
    /// Constructs bounded delayed-retry guidance.
    pub fn after(delay_ms: u64) -> Result<Self, ValidationError> {
        if delay_ms == 0 || delay_ms > MAX_RETRY_DELAY_MS {
            return Err(ValidationError::Invalid {
                field: "retry_delay_ms",
            });
        }
        Ok(Self::After {
            delay_ms: DecimalU64::new(delay_ms),
        })
    }
}

impl<'de> Deserialize<'de> for RetryDirective {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(
            deny_unknown_fields,
            rename_all = "snake_case",
            tag = "kind",
            content = "payload"
        )]
        enum WireRetry {
            Never,
            Immediate,
            After { delay_ms: DecimalU64 },
        }

        match WireRetry::deserialize(deserializer)? {
            WireRetry::Never => Ok(Self::Never),
            WireRetry::Immediate => Ok(Self::Immediate),
            WireRetry::After { delay_ms } => Self::after(delay_ms.get()).map_err(D::Error::custom),
        }
    }
}

/// Sanitized public failure that contains no provider-native payload.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct SafeFailure {
    pub class: FailureClass,
    pub code: String,
    pub message: String,
    pub retry: RetryDirective,
}

impl SafeFailure {
    /// Constructs and validates a public failure.
    pub fn new(
        class: FailureClass,
        code: impl Into<String>,
        message: impl Into<String>,
        retry: RetryDirective,
    ) -> Result<Self, ValidationError> {
        let code = code.into();
        let message = message.into();
        validate_non_empty_text("failure_code", &code, MAX_FAILURE_CODE_BYTES)?;
        if !code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        }) {
            return Err(ValidationError::Invalid {
                field: "failure_code",
            });
        }
        validate_non_empty_text("failure_message", &message, MAX_FAILURE_MESSAGE_BYTES)?;
        if message
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        {
            return Err(ValidationError::Invalid {
                field: "failure_message",
            });
        }
        if matches!(retry, RetryDirective::After { delay_ms } if delay_ms.get() == 0 || delay_ms.get() > MAX_RETRY_DELAY_MS)
        {
            return Err(ValidationError::Invalid {
                field: "retry_delay_ms",
            });
        }
        Ok(Self {
            class,
            code,
            message,
            retry,
        })
    }
}

impl Debug for SafeFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFailure")
            .field("class", &self.class)
            .field("code", &self.code)
            .field("message", &"[REDACTED]")
            .field("message_bytes", &self.message.len())
            .field("retry", &self.retry)
            .finish()
    }
}

impl<'de> Deserialize<'de> for SafeFailure {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireFailure {
            class: FailureClass,
            code: String,
            message: String,
            retry: RetryDirective,
        }

        let wire = WireFailure::deserialize(deserializer)?;
        Self::new(wire.class, wire.code, wire.message, wire.retry).map_err(D::Error::custom)
    }
}
