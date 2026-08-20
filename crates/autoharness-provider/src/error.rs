use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{ClassifiedError, ErrorClass, RetryAdvice};
use serde::{Deserialize, Serialize};

/// Stable provider failure kinds with no provider response text.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    /// A required credential was not configured.
    MissingCredential,
    /// The provider rejected the credential.
    Authentication,
    /// The credential lacks authority for this operation.
    PermissionDenied,
    /// The request was invalid.
    InvalidRequest,
    /// The requested model does not exist or is unavailable to the caller.
    ModelNotFound,
    /// The selected transport or capability is not implemented.
    Unsupported,
    /// A transient request rate limit was reached.
    RateLimited,
    /// A non-transient account or project quota was exhausted.
    QuotaExceeded,
    /// The request exceeded a provider or local deadline.
    Timeout,
    /// The provider is temporarily unavailable.
    Unavailable,
    /// A request conflicted with current provider state.
    Conflict,
    /// The provider blocked generated content.
    ContentBlocked,
    /// The operation was cancelled.
    Cancelled,
    /// A transport failed without a safe automatic replay guarantee.
    Transport,
    /// A response violated the documented provider protocol.
    Protocol,
    /// A configured response or pagination bound was exceeded.
    LimitExceeded,
    /// An internal adapter invariant failed.
    Internal,
}

/// A safe provider error that structurally excludes raw response text and secrets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderError {
    kind: ProviderErrorKind,
    class: ErrorClass,
    retry_advice: RetryAdvice,
    http_status: Option<u16>,
}

impl ProviderError {
    /// Constructs a safe classified error.
    #[must_use]
    pub const fn new(kind: ProviderErrorKind, retry_advice: RetryAdvice) -> Self {
        Self {
            kind,
            class: class_for_kind(kind),
            retry_advice,
            http_status: None,
        }
    }

    /// Adds a numeric HTTP status, which is safe to expose.
    #[must_use]
    pub const fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    /// Returns the stable failure kind.
    #[must_use]
    pub const fn kind(&self) -> ProviderErrorKind {
        self.kind
    }

    /// Returns the optional HTTP status without a response body.
    #[must_use]
    pub const fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    /// Returns whether the initial Interactions request may use an explicit
    /// Generate Content compatibility fallback before any stream event.
    #[must_use]
    pub const fn allows_transport_fallback(&self) -> bool {
        matches!(
            self.kind,
            ProviderErrorKind::ModelNotFound | ProviderErrorKind::Unsupported
        )
    }
}

const fn class_for_kind(kind: ProviderErrorKind) -> ErrorClass {
    match kind {
        ProviderErrorKind::MissingCredential | ProviderErrorKind::Authentication => {
            ErrorClass::Authentication
        }
        ProviderErrorKind::PermissionDenied => ErrorClass::PermissionDenied,
        ProviderErrorKind::InvalidRequest => ErrorClass::Validation,
        ProviderErrorKind::ModelNotFound => ErrorClass::NotFound,
        ProviderErrorKind::Unsupported
        | ProviderErrorKind::Protocol
        | ProviderErrorKind::LimitExceeded => ErrorClass::Protocol,
        ProviderErrorKind::RateLimited | ProviderErrorKind::QuotaExceeded => {
            ErrorClass::RateLimited
        }
        ProviderErrorKind::Timeout => ErrorClass::Timeout,
        ProviderErrorKind::Unavailable | ProviderErrorKind::Transport => ErrorClass::Unavailable,
        ProviderErrorKind::Conflict => ErrorClass::Conflict,
        ProviderErrorKind::ContentBlocked => ErrorClass::PermissionDenied,
        ProviderErrorKind::Cancelled => ErrorClass::Cancelled,
        ProviderErrorKind::Internal => ErrorClass::Internal,
    }
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            ProviderErrorKind::MissingCredential => "provider credential is not configured",
            ProviderErrorKind::Authentication => "provider authentication failed",
            ProviderErrorKind::PermissionDenied => "provider permission was denied",
            ProviderErrorKind::InvalidRequest => "provider request was invalid",
            ProviderErrorKind::ModelNotFound => "provider model was not found",
            ProviderErrorKind::Unsupported => "provider capability is unsupported",
            ProviderErrorKind::RateLimited => "provider rate limit was reached",
            ProviderErrorKind::QuotaExceeded => "provider quota was exhausted",
            ProviderErrorKind::Timeout => "provider request timed out",
            ProviderErrorKind::Unavailable => "provider is temporarily unavailable",
            ProviderErrorKind::Conflict => "provider request conflicted with current state",
            ProviderErrorKind::ContentBlocked => "provider blocked generated content",
            ProviderErrorKind::Cancelled => "provider operation was cancelled",
            ProviderErrorKind::Transport => "provider transport failed",
            ProviderErrorKind::Protocol => "provider response violated its protocol",
            ProviderErrorKind::LimitExceeded => "provider response exceeded a configured bound",
            ProviderErrorKind::Internal => "provider adapter failed internally",
        };

        formatter.write_str(message)
    }
}

impl Error for ProviderError {}

impl ClassifiedError for ProviderError {
    fn class(&self) -> ErrorClass {
        self.class
    }

    fn retry_advice(&self) -> RetryAdvice {
        self.retry_advice
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_error_contract_contains_no_arbitrary_text_field() {
        let sentinel = "api-key-secret-sentinel";
        let error = ProviderError::new(ProviderErrorKind::Authentication, RetryAdvice::Never)
            .with_http_status(401);
        let serialized = serde_json::to_string(&error).expect("serialize safe error");
        let rendered = format!("{error:?} {error}");

        assert!(!serialized.contains(sentinel));
        assert!(!rendered.contains(sentinel));
        assert_eq!(error.class(), ErrorClass::Authentication);
    }
}
