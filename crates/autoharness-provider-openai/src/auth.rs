use std::env;
use std::fmt::{self, Debug, Formatter};

use autoharness_domain::RetryAdvice;
use autoharness_provider::{ProviderError, ProviderErrorKind};
use zeroize::Zeroizing;

/// Environment variable containing the configured router credential.
pub const ROUTER_API_KEY_ENV: &str = "AUTOHARNESS_ROUTER_API_KEY";

const MAX_CREDENTIAL_BYTES: usize = 4096;

/// An in-memory router credential that cannot be displayed or serialized.
pub struct RouterCredential {
    raw: Zeroizing<String>,
    percent_encoded: Zeroizing<String>,
    percent_encoded_lower_hex: Zeroizing<String>,
}

impl RouterCredential {
    /// Validates a credential without including rejected content in errors.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = Zeroizing::new(value.into());
        if value.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::MissingCredential,
                RetryAdvice::Never,
            ));
        }
        if !value.is_ascii()
            || value.len() > MAX_CREDENTIAL_BYTES
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ));
        }
        Ok(Self {
            percent_encoded: Zeroizing::new(percent_encode(&value, b"0123456789ABCDEF")),
            percent_encoded_lower_hex: Zeroizing::new(percent_encode(&value, b"0123456789abcdef")),
            raw: value,
        })
    }

    /// Reads the configured router credential from the environment.
    pub fn from_env() -> Result<Self, ProviderError> {
        let value = env::var(ROUTER_API_KEY_ENV).map_err(|_| {
            ProviderError::new(ProviderErrorKind::MissingCredential, RetryAdvice::Never)
        })?;
        Self::new(value)
    }

    pub(crate) fn expose(&self) -> &str {
        self.raw.as_str()
    }

    pub(crate) fn contains(&self, value: &str) -> bool {
        value.contains(self.expose())
            || value.contains(self.percent_encoded.as_str())
            || value.contains(self.percent_encoded_lower_hex.as_str())
    }

    pub(crate) fn redact(&self, value: &str) -> String {
        value
            .replace(self.expose(), "[REDACTED]")
            .replace(self.percent_encoded.as_str(), "[REDACTED]")
            .replace(self.percent_encoded_lower_hex.as_str(), "[REDACTED]")
    }
}

impl Clone for RouterCredential {
    fn clone(&self) -> Self {
        Self {
            raw: Zeroizing::new(self.raw.to_string()),
            percent_encoded: Zeroizing::new(self.percent_encoded.to_string()),
            percent_encoded_lower_hex: Zeroizing::new(self.percent_encoded_lower_hex.to_string()),
        }
    }
}

impl Debug for RouterCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("RouterCredential([REDACTED])")
    }
}

fn percent_encode(value: &str, hex: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(hex[usize::from(byte >> 4)]));
            encoded.push(char::from(hex[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_redaction_never_expose_credential() {
        let credential = RouterCredential::new("router+/=secret").expect("credential");

        assert_eq!(format!("{credential:?}"), "RouterCredential([REDACTED])");
        assert_eq!(
            credential.redact("router%2B%2F%3Dsecret router%2b%2f%3dsecret"),
            "[REDACTED] [REDACTED]"
        );
    }
}
