use std::env;
use std::fmt::{self, Debug, Formatter};

use autoharness_domain::RetryAdvice;
use autoharness_provider::{ProviderError, ProviderErrorKind};
use zeroize::Zeroizing;

/// Environment variable read by [`GeminiApiKey::from_env`].
pub const GEMINI_API_KEY_ENV: &str = "GEMINI_API_KEY";

const MAX_API_KEY_BYTES: usize = 4096;

/// An in-memory API key that cannot be displayed or serialized.
pub struct GeminiApiKey {
    raw: Zeroizing<String>,
    percent_encoded: Zeroizing<String>,
    percent_encoded_lower_hex: Zeroizing<String>,
}

impl GeminiApiKey {
    /// Validates an API key without ever including the rejected value in an error.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::MissingCredential,
                RetryAdvice::Never,
            ));
        }
        if !value.is_ascii()
            || value.len() > MAX_API_KEY_BYTES
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ));
        }
        let percent_encoded = percent_encode(&value, b"0123456789ABCDEF");
        let percent_encoded_lower_hex = percent_encode(&value, b"0123456789abcdef");
        Ok(Self {
            raw: Zeroizing::new(value),
            percent_encoded: Zeroizing::new(percent_encoded),
            percent_encoded_lower_hex: Zeroizing::new(percent_encoded_lower_hex),
        })
    }

    /// Reads `GEMINI_API_KEY` without writing it to configuration or disk.
    pub fn from_env() -> Result<Self, ProviderError> {
        let value = env::var(GEMINI_API_KEY_ENV).map_err(|_| {
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

impl Clone for GeminiApiKey {
    fn clone(&self) -> Self {
        Self {
            raw: Zeroizing::new(self.raw.to_string()),
            percent_encoded: Zeroizing::new(self.percent_encoded.to_string()),
            percent_encoded_lower_hex: Zeroizing::new(self.percent_encoded_lower_hex.to_string()),
        }
    }
}

impl Debug for GeminiApiKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeminiApiKey([REDACTED])")
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
    fn debug_never_exposes_key() {
        let sentinel = "gemini-secret-sentinel";
        let key = GeminiApiKey::new(sentinel).expect("valid key");

        assert!(!format!("{key:?}").contains(sentinel));
        assert_eq!(key.redact(&format!("echo:{sentinel}")), "echo:[REDACTED]");
    }

    #[test]
    fn redacts_raw_and_percent_encoded_forms() {
        let key = GeminiApiKey::new("sensitive+/=sentinel").expect("key");

        assert_eq!(
            key.redact("sensitive%2B%2F%3Dsentinel sensitive%2b%2f%3dsentinel"),
            "[REDACTED] [REDACTED]"
        );
        assert!(key.contains("prefix-sensitive%2B%2F%3Dsentinel-suffix"));
    }
}
