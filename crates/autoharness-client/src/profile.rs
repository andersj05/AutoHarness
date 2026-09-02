use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::bounds::{
    MAX_PROFILE_FIELD_BYTES, MAX_ROUTER_URL_BYTES, validate_non_empty_text, validate_text,
};
use crate::{ConnectionId, ValidationError};

/// Provider adapter selected by one named connection profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Gemini,
    Router,
    CodexSubscription,
}

/// Provider-native reasoning effort saved beside a profile default model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// Durable operating-system vault linkage state for one named profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialState {
    Disconnected,
    Stored,
    RecoveryPending,
}

/// Whether a provider row is a durable named profile or the temporary session default.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfileScope {
    Named,
    SessionDefault,
}

impl ReasoningEffort {
    /// Returns the validated provider-native value expected by settings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// Non-secret connection configuration for one provider profile.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ProviderConfiguration {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub project: Option<String>,
    pub auth_header: Option<String>,
}

impl ProviderConfiguration {
    /// Constructs and validates provider-specific non-secret fields.
    pub fn new(
        kind: ProviderKind,
        base_url: Option<String>,
        project: Option<String>,
        auth_header: Option<String>,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            kind,
            base_url,
            project,
            auth_header,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn validate(&self) -> Result<(), ValidationError> {
        match self.kind {
            ProviderKind::Router => {
                if let Some(base_url) = &self.base_url {
                    validate_non_empty_text("provider_base_url", base_url, MAX_ROUTER_URL_BYTES)?;
                    if base_url.chars().any(char::is_control) {
                        return Err(ValidationError::Invalid {
                            field: "provider_base_url",
                        });
                    }
                }
                for (field, value) in [
                    ("provider_project", &self.project),
                    ("provider_auth_header", &self.auth_header),
                ] {
                    if let Some(value) = value {
                        validate_text(field, value, MAX_PROFILE_FIELD_BYTES)?;
                        if value.is_empty() || value.chars().any(char::is_control) {
                            return Err(ValidationError::Invalid { field });
                        }
                    }
                }
            }
            ProviderKind::Gemini | ProviderKind::CodexSubscription => {
                if self.base_url.is_some() || self.project.is_some() || self.auth_header.is_some() {
                    return Err(ValidationError::Inconsistent {
                        field: "provider_configuration",
                    });
                }
            }
        }
        Ok(())
    }
}

impl Debug for ProviderConfiguration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderConfiguration")
            .field("kind", &self.kind)
            .field("has_base_url", &self.base_url.is_some())
            .field("has_project", &self.project.is_some())
            .field("has_auth_header", &self.auth_header.is_some())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ProviderConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireConfiguration {
            kind: ProviderKind,
            base_url: Option<String>,
            project: Option<String>,
            auth_header: Option<String>,
        }
        let wire = WireConfiguration::deserialize(deserializer)?;
        Self::new(wire.kind, wire.base_url, wire.project, wire.auth_header)
            .map_err(D::Error::custom)
    }
}

/// Exact named profile and non-secret configuration submitted by a client.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ProviderProfileInput {
    pub connection_id: ConnectionId,
    pub configuration: ProviderConfiguration,
}

impl ProviderProfileInput {
    pub fn new(
        connection_id: ConnectionId,
        configuration: ProviderConfiguration,
    ) -> Result<Self, ValidationError> {
        if configuration.kind == ProviderKind::Router && configuration.base_url.is_none() {
            return Err(ValidationError::Empty {
                field: "provider_base_url",
            });
        }
        Ok(Self {
            connection_id,
            configuration,
        })
    }
}

impl Debug for ProviderProfileInput {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderProfileInput")
            .field("connection_id", &self.connection_id)
            .field("configuration", &self.configuration)
            .finish()
    }
}

impl<'de> Deserialize<'de> for ProviderProfileInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireProfileInput {
            connection_id: ConnectionId,
            configuration: ProviderConfiguration,
        }
        let wire = WireProfileInput::deserialize(deserializer)?;
        Self::new(wire.connection_id, wire.configuration).map_err(D::Error::custom)
    }
}
