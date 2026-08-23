use std::collections::BTreeMap;
use std::fmt;

use crate::error::SettingsError;
use crate::profile::{
    ProfileId, ProviderKind, ProviderProfile, SETTINGS_SCHEMA_VERSION, SettingsDocument,
};
use crate::source::Source;

const LAYER_USER: &str = "user settings file";
const LAYER_WORKSPACE: &str = "workspace settings file";

/// Ordered configuration layers the resolver understands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerKind {
    /// The per-user settings file.
    UserFile,
    /// The workspace-local settings file.
    WorkspaceFile,
}

impl LayerKind {
    fn label(self) -> &'static str {
        match self {
            Self::UserFile => LAYER_USER,
            Self::WorkspaceFile => LAYER_WORKSPACE,
        }
    }

    fn source(self) -> Source {
        match self {
            Self::UserFile => Source::UserFile,
            Self::WorkspaceFile => Source::WorkspaceFile,
        }
    }
}

/// Keys a workspace settings document may never override.
const WORKSPACE_FORBIDDEN_KEYS: &[&str] = &[
    "provider",
    "profiles",
    "active_profile",
    "credential_recovery",
];

#[derive(Debug, Default, Clone)]
struct RawLayer {
    provider: Option<(ProviderKind, Source)>,
    active_profile: Option<(String, Source)>,
    profiles: BTreeMap<ProfileId, (ProviderProfile, Source)>,
}

/// Accumulates configuration layers and resolves them into typed settings.
#[derive(Debug, Default)]
pub struct SettingsBuilder {
    layers: Vec<(LayerKind, String)>,
    environment: Vec<(String, String)>,
}

impl SettingsBuilder {
    /// Creates an empty builder containing only built-in defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one JSON layer parsed in resolution order.
    #[must_use]
    pub fn with_layer(mut self, kind: LayerKind, json: impl Into<String>) -> Self {
        self.layers.push((kind, json.into()));
        self
    }

    /// Adds environment variables from any ordered name-value source.
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

    /// Resolves every added layer into validated effective settings.
    pub fn resolve(mut self) -> Result<ResolvedSettings, SettingsError> {
        let mut merged = RawLayer::default();
        let mut diagnostics = Vec::new();

        for (kind, json) in std::mem::take(&mut self.layers) {
            let document = match parse_layer(kind.label(), &json) {
                Ok(document) => document,
                Err(SettingsError::UnsupportedSchemaVersion { layer, found }) => {
                    return Err(SettingsError::UnsupportedSchemaVersion { layer, found });
                }
                Err(error) => {
                    diagnostics.push(error.to_string());
                    continue;
                }
            };
            if kind == LayerKind::WorkspaceFile {
                for key in WORKSPACE_FORBIDDEN_KEYS {
                    if document_has_key(&document, key) {
                        return Err(SettingsError::DisallowedWorkspaceKey {
                            key: (*key).to_owned(),
                        });
                    }
                }
            }
            merge_document(&mut merged, document, kind.source());
        }

        for (name, value) in self.environment {
            if name == "AUTOHARNESS_PROVIDER" {
                match parse_provider_value(&value) {
                    Some(kind) => merged.provider = Some((kind, Source::Environment)),
                    None => diagnostics.push(format!("unknown provider '{value}'")),
                }
            }
        }

        let missing = merged.active_profile.as_ref().filter(|(active, _)| {
            !merged
                .profiles
                .keys()
                .any(|id| id.as_str() == active.as_str())
        });
        if let Some((active, _)) = missing {
            return Err(SettingsError::InvalidMerge {
                reason: format!("active profile '{active}' is not declared"),
            });
        }

        Ok(ResolvedSettings {
            inner: merged,
            diagnostics,
        })
    }
}

/// Fully resolved, validated, non-secret settings plus provenance.
#[derive(Debug, Clone, Default)]
pub struct ResolvedSettings {
    inner: RawLayer,
    diagnostics: Vec<String>,
}

impl ResolvedSettings {
    /// Returns the effective provider selection and its source.
    #[must_use]
    pub fn provider(&self) -> Option<ProviderKind> {
        self.inner.provider.as_ref().map(|(kind, _)| *kind)
    }

    /// Returns provenance for keys that have an effective value.
    #[must_use]
    pub fn provenance(&self) -> BTreeMap<String, Source> {
        let mut map = BTreeMap::new();
        if let Some((_, source)) = &self.inner.provider {
            map.insert("provider".to_owned(), *source);
        }
        if let Some((_, source)) = &self.inner.active_profile {
            map.insert("active_profile".to_owned(), *source);
        }
        for (id, (_, source)) in &self.inner.profiles {
            map.insert(format!("profiles.{id}"), *source);
        }
        map
    }

    /// Iterates declared profiles in stable name order.
    pub fn profiles(&self) -> impl Iterator<Item = (&ProfileId, &ProviderProfile)> + use<'_> {
        self.inner
            .profiles
            .iter()
            .map(|(id, (profile, _))| (id, profile))
    }

    /// Returns one declared profile by identity.
    #[must_use]
    pub fn profile(&self, id: &ProfileId) -> Option<&ProviderProfile> {
        self.inner.profiles.get(id).map(|(profile, _)| profile)
    }

    /// Returns the active profile name, when configured.
    #[must_use]
    pub fn active_profile(&self) -> Option<&str> {
        self.inner
            .active_profile
            .as_ref()
            .map(|(id, _)| id.as_str())
    }

    /// Returns safe diagnostics gathered while skipping unusable input.
    #[must_use]
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

impl fmt::Display for ResolvedSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "settings(")?;
        match self.provider() {
            Some(kind) => write!(formatter, "provider={kind:?}, ")?,
            None => write!(formatter, "provider=default, ")?,
        }
        write!(
            formatter,
            "active_profile={:?})",
            self.active_profile().unwrap_or_default()
        )
    }
}

fn parse_layer(label: &'static str, json: &str) -> Result<SettingsDocument, SettingsError> {
    if json.trim().is_empty() {
        return Ok(SettingsDocument {
            schema_version: SETTINGS_SCHEMA_VERSION,
            ..SettingsDocument::default()
        });
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| SettingsError::MalformedLayer { layer: label })?;
    if !value.is_object() && !value.is_null() {
        return Err(SettingsError::MalformedLayer { layer: label });
    }
    let version = value
        .get("schema_version")
        .and_then(|version| version.as_u64())
        .unwrap_or(u64::from(SETTINGS_SCHEMA_VERSION));
    if version > u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(SettingsError::UnsupportedSchemaVersion {
            layer: label,
            found: version as u32,
        });
    }
    serde_json::from_value(value).map_err(|_| SettingsError::MalformedLayer { layer: label })
}

fn parse_provider_value(value: &str) -> Option<ProviderKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gemini" | "google" => Some(ProviderKind::Gemini),
        "router" | "openai" => Some(ProviderKind::Router),
        _ => None,
    }
}

fn document_has_key(document: &SettingsDocument, key: &str) -> bool {
    match key {
        "provider" => document.provider.is_some(),
        "active_profile" => document.active_profile.is_some(),
        "profiles" => !document.profiles.is_empty(),
        "credential_recovery" => !document.credential_recovery.is_empty(),
        _ => false,
    }
}

fn merge_document(merged: &mut RawLayer, document: SettingsDocument, source: Source) {
    if let Some(provider) = document.provider {
        merged.provider = Some((provider, source));
    }
    if let Some(active) = document.active_profile {
        merged.active_profile = Some((active.to_string(), source));
    }
    for (id, profile) in document.profiles {
        merged.profiles.insert(id, (profile, source));
    }
}
