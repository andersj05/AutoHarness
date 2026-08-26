use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{ProviderId, RetryAdvice};
use autoharness_provider::{ProviderError, ProviderErrorKind};

use crate::CODEX_PROVIDER_ID;

/// Validated non-secret configuration for native Codex subscription requests.
#[derive(Clone)]
pub struct CodexSettings {
    provider_id: ProviderId,
    reasoning_effort: Option<String>,
}

impl CodexSettings {
    /// Creates the fixed first-party Codex subscription configuration.
    pub fn new() -> Result<Self, ProviderError> {
        let provider_id =
            ProviderId::new(CODEX_PROVIDER_ID).map_err(|_| invalid_configuration())?;
        Ok(Self {
            provider_id,
            reasoning_effort: None,
        })
    }

    /// Returns this adapter's stable provider identity.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Applies a validated Codex reasoning-effort override.
    pub fn with_reasoning_effort(mut self, effort: Option<&str>) -> Result<Self, ProviderError> {
        const SUPPORTED: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
        if effort.is_some_and(|value| !SUPPORTED.contains(&value)) {
            return Err(invalid_configuration());
        }
        self.reasoning_effort = effort.map(str::to_owned);
        Ok(self)
    }

    /// Returns the configured reasoning effort.
    #[must_use]
    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self::new().expect("the fixed Codex provider identifier is valid")
    }
}

impl Debug for CodexSettings {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSettings")
            .field("provider_id", &self.provider_id)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

fn invalid_configuration() -> ProviderError {
    ProviderError::new(ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_accept_only_supported_reasoning_efforts() {
        assert!(CodexSettings::new().is_ok());
        assert!(CodexSettings::new()
            .and_then(|settings| settings.with_reasoning_effort(Some("high")))
            .is_ok());
        assert!(CodexSettings::new()
            .and_then(|settings| settings.with_reasoning_effort(Some("extreme")))
            .is_err());
    }
}
