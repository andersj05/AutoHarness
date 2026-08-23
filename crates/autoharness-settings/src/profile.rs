use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Supported settings schema version.
pub const SETTINGS_SCHEMA_VERSION: u32 = 2;

/// Bounded profile-name value type.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProfileId(Arc<str>);

impl ProfileId {
    /// Creates a non-empty profile name of at most 64 visible characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, &'static str> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err("profile name must not be empty");
        }
        if value.len() > 64 {
            return Err("profile name must be at most 64 characters");
        }
        if !value
            .chars()
            .all(|character| character.is_ascii_graphic() && character != '"')
        {
            return Err("profile name must be visible ASCII without quotes");
        }
        Ok(Self(Arc::from(value)))
    }

    /// Returns the stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Opaque credential reference retained in non-secret settings.
///
/// The string names where a credential can be resolved from; it never
/// contains credential material itself.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CredentialReference(Arc<str>);

impl CredentialReference {
    /// Creates a bounded opaque reference of visible ASCII characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, &'static str> {
        let value = value.as_ref();
        let value = value.trim();
        if value.is_empty() {
            return Err("credential reference must not be empty");
        }
        if value.len() > 256 {
            return Err("credential reference is too long");
        }
        if !value.chars().all(|character| character.is_ascii_graphic()) {
            return Err("credential references must contain visible ASCII only");
        }
        Ok(Self(Arc::from(value)))
    }

    /// Returns the opaque reference string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CredentialReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for CredentialReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CredentialReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Which provider adapter a profile selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Google AI Studio Gemini.
    Gemini,
    /// Configurable OpenAI-compatible router.
    Router,
}

/// Non-secret connection fields for one named provider configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
    pub(crate) kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) auth_header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) models_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chat_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) credential: Option<CredentialDocument>,
}

impl ProviderProfile {
    /// Creates a Gemini profile with no stored credential.
    #[must_use]
    pub const fn gemini() -> Self {
        Self {
            kind: ProviderKind::Gemini,
            base_url: None,
            project: None,
            auth_header: None,
            models_path: None,
            chat_path: None,
            credential: None,
        }
    }

    /// Creates a router profile with validated non-secret connection fields.
    pub fn router(
        base_url: impl Into<String>,
        project: Option<String>,
        auth_header: Option<String>,
    ) -> Result<Self, &'static str> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err("router base URL must not be empty");
        }
        if base_url.len() > 2_048 || base_url.chars().any(char::is_control) {
            return Err("router base URL is invalid");
        }
        for value in project.iter().chain(auth_header.iter()) {
            if value.len() > 256 || value.chars().any(char::is_control) {
                return Err("router profile field is invalid");
            }
        }
        Ok(Self {
            kind: ProviderKind::Router,
            base_url: Some(base_url),
            project,
            auth_header,
            models_path: None,
            chat_path: None,
            credential: None,
        })
    }

    /// Returns a copy without credential linkage for safe duplication.
    #[must_use]
    pub fn without_credential(mut self) -> Self {
        self.credential = None;
        self
    }

    /// Returns the provider adapter this profile selects.
    #[must_use]
    pub const fn kind(&self) -> ProviderKind {
        self.kind
    }

    /// Returns the configured router base URL, when present.
    #[must_use]
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    /// Returns the configured router project identity, when present.
    #[must_use]
    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    /// Returns the configured authentication header name, when present.
    #[must_use]
    pub fn auth_header(&self) -> Option<&str> {
        self.auth_header.as_deref()
    }

    /// Returns the configured model-discovery path, when present.
    #[must_use]
    pub fn models_path(&self) -> Option<&str> {
        self.models_path.as_deref()
    }

    /// Returns the configured streamed-chat path, when present.
    #[must_use]
    pub fn chat_path(&self) -> Option<&str> {
        self.chat_path.as_deref()
    }

    /// Returns the opaque credential reference, when a credential is linked.
    #[must_use]
    pub fn credential_reference(&self) -> Option<&str> {
        self.credential
            .as_ref()
            .map(|document| document.reference.as_str())
    }
}

/// Serialized credential linkage inside one profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialDocument {
    pub(crate) reference: CredentialReference,
}
/// Durable non-secret recovery action for a cross-system credential mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRecoveryKind {
    /// A vault save has not yet committed its profile-document link.
    UncommittedSave,
    /// A removed profile link still requires idempotent vault cleanup.
    Delete,
}

/// Bounded recovery record containing identities but never credential material.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRecoveryRecord {
    profile: ProfileId,
    reference: CredentialReference,
    kind: CredentialRecoveryKind,
}

impl CredentialRecoveryRecord {
    /// Creates one non-secret recovery record.
    #[must_use]
    pub const fn new(
        profile: ProfileId,
        reference: CredentialReference,
        kind: CredentialRecoveryKind,
    ) -> Self {
        Self {
            profile,
            reference,
            kind,
        }
    }

    /// Returns the exact profile involved in the mutation.
    #[must_use]
    pub const fn profile(&self) -> &ProfileId {
        &self.profile
    }

    /// Returns the deterministic opaque vault reference.
    #[must_use]
    pub const fn reference(&self) -> &CredentialReference {
        &self.reference
    }

    /// Returns the recovery action kind.
    #[must_use]
    pub const fn kind(&self) -> CredentialRecoveryKind {
        self.kind
    }
}

/// Validated top-level settings document for one layer or merged output.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDocument {
    pub(crate) schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<ProviderKind>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) profiles: BTreeMap<ProfileId, ProviderProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_profile: Option<ProfileId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) credential_recovery: Vec<CredentialRecoveryRecord>,
}
