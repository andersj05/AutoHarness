//! Credential-source resolution across environment, vault, and session-only
//! fallbacks.
//!
//! The resolver answers one question safely: which credential should a
//! provider adapter use this launch? Precedence is environment first,
//! then an active profile's vault reference, then session-only entry.

use std::collections::BTreeMap;
use std::fmt;

use autoharness_settings::{ProviderKind, ResolvedSettings};
use zeroize::Zeroizing;

use crate::vault::VaultPort;

/// Safe labels for the effective credential source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialSourceName {
    /// A process environment variable supplied the credential.
    Environment,
    /// The operating-system credential vault resolved a profile reference.
    CredentialVault,
    /// Nothing persisted; the user may enter a session-only key in-app.
    SessionOnly,
}

impl CredentialSourceName {
    /// Returns the stable lowercase label shown to users.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::CredentialVault => "credential vault",
            Self::SessionOnly => "session only",
        }
    }
}

impl fmt::Display for CredentialSourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Effective credential resolution with provenance and provider context.
pub struct CredentialSource {
    source_name: CredentialSourceName,
    credential: Zeroizing<String>,
    profile: Option<String>,
    provider_kind: Option<ProviderKind>,
    description: String,
}

impl CredentialSource {
    /// Returns the safe source label.
    #[must_use]
    pub const fn source_name(&self) -> CredentialSourceName {
        self.source_name
    }

    /// Returns the credential bytes when one resolved, or empty for
    /// session-only mode.
    #[must_use]
    pub fn credential(&self) -> &str {
        &self.credential
    }

    /// Takes the credential bytes out of the resolution result.
    pub fn into_credential(self) -> Zeroizing<String> {
        self.credential
    }

    /// Returns the active profile identity, when one applied.
    #[must_use]
    pub fn profile_id(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// Returns the effective provider selected by profile, settings, or default.
    #[must_use]
    pub fn provider_kind(&self) -> Option<ProviderKind> {
        self.provider_kind
    }

    /// Returns a safe human description that names no secret metadata.
    #[must_use]
    pub fn source_description(&self) -> &str {
        &self.description
    }
}

impl fmt::Debug for CredentialSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialSource")
            .field("source", &self.source_name)
            .field("profile", &self.profile)
            .field("has_credential", &!self.credential.is_empty())
            .finish_non_exhaustive()
    }
}

/// Resolves the effective credential from layered settings plus a vault.
pub struct ProfileCredentialResolver<'a> {
    vault: &'a dyn VaultPort,
    environment: BTreeMap<String, String>,
}

impl<'a> ProfileCredentialResolver<'a> {
    /// Creates a resolver over any credential-vault implementation.
    pub fn new(vault: &'a dyn VaultPort) -> Self {
        Self {
            vault,
            environment: BTreeMap::new(),
        }
    }

    /// Supplies ordered environment variables for precedence checks.
    #[must_use]
    pub fn with_environment<I, K, V>(mut self, variables: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.environment.extend(
            variables
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    /// Resolves credentials using fully merged typed settings.
    ///
    /// The active profile selects its provider when present; otherwise the
    /// resolved provider setting applies, with Gemini as the product default.
    pub fn resolve(&self, settings: &ResolvedSettings) -> Result<CredentialSource, ResolverError> {
        let document = serde_json::json!({
            "active_profile": settings.active_profile(),
            "profiles": settings
                .profiles()
                .map(|(id, profile)| {
                    (
                        id.as_str().to_owned(),
                        serde_json::json!({
                            "kind": match profile.kind() {
                                ProviderKind::Gemini => "gemini",
                                ProviderKind::Router => "router",
                                ProviderKind::CodexCli => "codex_cli",
                            },
                            "credential": profile.credential_reference().map(|reference| {
                                serde_json::json!({ "reference": reference })
                            }),
                        }),
                    )
                })
                .collect::<serde_json::Map<String, serde_json::Value>>(),
        });
        self.resolve_document_for_provider(&document, settings.provider())
    }

    /// Resolves credentials directly from a raw settings document.
    ///
    /// This entry point exists for degraded launches where full typed
    /// resolution failed; it applies the same precedence rules.
    pub fn resolve_document(
        &self,
        document: &serde_json::Value,
    ) -> Result<CredentialSource, ResolverError> {
        let selected_provider = document
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .and_then(kind_from_str);
        self.resolve_document_for_provider(document, selected_provider)
    }

    fn resolve_document_for_provider(
        &self,
        document: &serde_json::Value,
        selected_provider: Option<ProviderKind>,
    ) -> Result<CredentialSource, ResolverError> {
        let active = document
            .get("active_profile")
            .and_then(serde_json::Value::as_str);
        let profile = active.and_then(|profile_name| {
            document
                .get("profiles")
                .and_then(|profiles| profiles.get(profile_name))
        });
        let profile_kind = profile
            .and_then(|profile| profile.get("kind"))
            .and_then(serde_json::Value::as_str)
            .and_then(kind_from_str);
        let provider_kind = profile_kind
            .or(selected_provider)
            .unwrap_or(ProviderKind::Gemini);
        let environment_key = match provider_kind {
            ProviderKind::Gemini => Some("GEMINI_API_KEY"),
            ProviderKind::Router => Some("AUTOHARNESS_ROUTER_API_KEY"),
            // Subscription authentication remains exclusively inside Codex CLI.
            ProviderKind::CodexCli => None,
        };

        // 1. The credential matching the effective provider wins.
        if let Some(value) = environment_key
            .and_then(|key| self.environment.get(key))
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(CredentialSource {
                source_name: CredentialSourceName::Environment,
                credential: Zeroizing::new(value.clone()),
                profile: profile.and(active).map(str::to_owned),
                provider_kind: Some(provider_kind),
                description: "environment".to_owned(),
            });
        }

        // 2. An active profile with a stored reference resolves via the vault.
        if let (Some(profile_name), Some(profile)) = (active, profile) {
            let reference = profile
                .get("credential")
                .and_then(|credential| credential.get("reference"))
                .and_then(serde_json::Value::as_str);
            if let Some(reference) = reference {
                let parsed = autoharness_settings::CredentialReference::new(reference)
                    .map_err(ResolverError::InvalidReference)?;
                // A missing or locked vault entry degrades to session-only
                // instead of blocking offline use (ADR-0009).
                let secret = match self.vault.load(&parsed) {
                    Ok(secret) => secret,
                    Err(
                        crate::vault::VaultError::MissingEntry
                        | crate::vault::VaultError::Unavailable,
                    ) => return Ok(session_only_for(profile_name, Some(provider_kind))),
                    Err(error) => return Err(ResolverError::Vault(error)),
                };
                if secret.is_empty() {
                    return Ok(session_only_for(profile_name, Some(provider_kind)));
                }
                return Ok(CredentialSource {
                    source_name: CredentialSourceName::CredentialVault,
                    credential: secret,
                    profile: Some(profile_name.to_owned()),
                    provider_kind: Some(provider_kind),
                    description: format!("credential vault profile '{profile_name}'"),
                });
            }
            return Ok(session_only_for(profile_name, Some(provider_kind)));
        }

        Ok(session_only(Some(provider_kind)))
    }
}

impl fmt::Debug for ProfileCredentialResolver<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ProfileCredentialResolver").finish()
    }
}

/// Errors surfaced while resolving a credential reference.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ResolverError {
    /// A stored reference string failed validation.
    InvalidReference(&'static str),
    /// The vault rejected the read.
    Vault(crate::vault::VaultError),
}

impl fmt::Display for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReference(reason) => write!(formatter, "{reason}"),
            Self::Vault(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ResolverError {}

impl From<crate::vault::VaultError> for ResolverError {
    fn from(error: crate::vault::VaultError) -> Self {
        Self::Vault(error)
    }
}

fn session_only(kind: Option<ProviderKind>) -> CredentialSource {
    CredentialSource {
        source_name: CredentialSourceName::SessionOnly,
        credential: Zeroizing::new(String::new()),
        profile: None,
        provider_kind: kind,
        description: "session only".to_owned(),
    }
}

fn session_only_for(profile_name: &str, kind: Option<ProviderKind>) -> CredentialSource {
    CredentialSource {
        source_name: CredentialSourceName::SessionOnly,
        credential: Zeroizing::new(String::new()),
        profile: Some(profile_name.to_owned()),
        provider_kind: kind,
        description: format!("session only for profile '{profile_name}'"),
    }
}

fn kind_from_str(value: &str) -> Option<ProviderKind> {
    match value {
        "gemini" => Some(ProviderKind::Gemini),
        "router" => Some(ProviderKind::Router),
        "codex_cli" => Some(ProviderKind::CodexCli),
        _ => None,
    }
}
