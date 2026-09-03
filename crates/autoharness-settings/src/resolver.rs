use std::collections::BTreeMap;
use std::fmt;

use crate::error::SettingsError;
use crate::preferences::{
    ColorMode, ComposerSubmitBehavior, Density, DisplayLabel, EffectiveLocalPreferences,
    EffectiveLocalProfile, EffectiveValue, GlyphMode, GuiFontSize, GuiZoomPercent, Layout,
    LocalProfile, PromptStatusDetail, TerminalTimestampStyle, ThemePreset,
};
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

#[derive(Debug, Default, Clone)]
struct RawLocalProfile {
    display_label: Option<(DisplayLabel, Source)>,
    theme_preset: Option<(ThemePreset, Source)>,
    color_mode: Option<(ColorMode, Source)>,
    glyph_mode: Option<(GlyphMode, Source)>,
    reduced_motion: Option<(bool, Source)>,
    density: Option<(Density, Source)>,
    layout: Option<(Layout, Source)>,
    terminal_timestamp_style: Option<(TerminalTimestampStyle, Source)>,
    composer_submit_behavior: Option<(ComposerSubmitBehavior, Source)>,
    prompt_status_detail: Option<(PromptStatusDetail, Source)>,
    gui_zoom_percent: Option<(GuiZoomPercent, Source)>,
    gui_font_size: Option<(GuiFontSize, Source)>,
}

#[derive(Debug, Default, Clone)]
struct RawLayer {
    provider: Option<(ProviderKind, Source)>,
    active_profile: Option<(String, Source)>,
    profiles: BTreeMap<ProfileId, (ProviderProfile, Source)>,
    local_profile: RawLocalProfile,
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
        let layers = std::mem::take(&mut self.layers);

        for expected_kind in [LayerKind::UserFile, LayerKind::WorkspaceFile] {
            for (kind, json) in &layers {
                if *kind != expected_kind {
                    continue;
                }
                let document = match parse_layer(*kind, json) {
                    Ok(document) => document,
                    Err(error @ SettingsError::UnsupportedSchemaVersion { .. }) => {
                        return Err(error);
                    }
                    Err(error @ SettingsError::DisallowedWorkspaceKey { .. }) => {
                        return Err(error);
                    }
                    Err(error) => {
                        diagnostics.push(error.to_string());
                        continue;
                    }
                };
                merge_document(&mut merged, document, kind.source());
            }
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

        let local_profile = effective_local_profile(&merged.local_profile);
        Ok(ResolvedSettings {
            inner: merged,
            local_profile,
            diagnostics,
        })
    }
}

/// Fully resolved, validated, non-secret settings plus provenance.
#[derive(Debug, Clone)]
pub struct ResolvedSettings {
    inner: RawLayer,
    local_profile: EffectiveLocalProfile,
    diagnostics: Vec<String>,
}

impl Default for ResolvedSettings {
    fn default() -> Self {
        let inner = RawLayer::default();
        Self {
            local_profile: effective_local_profile(&inner.local_profile),
            inner,
            diagnostics: Vec::new(),
        }
    }
}

impl ResolvedSettings {
    /// Returns the effective provider selection and its source.
    #[must_use]
    pub fn provider(&self) -> Option<ProviderKind> {
        self.inner.provider.as_ref().map(|(kind, _)| *kind)
    }

    /// Returns effective local identity and UI preferences with leaf provenance.
    #[must_use]
    pub const fn local_profile(&self) -> &EffectiveLocalProfile {
        &self.local_profile
    }

    /// Returns provenance for every resolved setting leaf.
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
        let local_profile = self.local_profile();
        map.insert(
            "local_profile.display_label".to_owned(),
            local_profile.display_label().source(),
        );
        let preferences = local_profile.preferences();
        map.insert(
            "local_profile.preferences.shared.theme_preset".to_owned(),
            preferences.theme_preset().source(),
        );
        map.insert(
            "local_profile.preferences.shared.color_mode".to_owned(),
            preferences.color_mode().source(),
        );
        map.insert(
            "local_profile.preferences.terminal.glyph_mode".to_owned(),
            preferences.glyph_mode().source(),
        );
        map.insert(
            "local_profile.preferences.shared.reduced_motion".to_owned(),
            preferences.reduced_motion().source(),
        );
        map.insert(
            "local_profile.preferences.shared.density".to_owned(),
            preferences.density().source(),
        );
        map.insert(
            "local_profile.preferences.terminal.layout".to_owned(),
            preferences.layout().source(),
        );
        map.insert(
            "local_profile.preferences.shared.timestamp_style".to_owned(),
            preferences.terminal_timestamp_style().source(),
        );
        map.insert(
            "local_profile.preferences.shared.composer_submit_behavior".to_owned(),
            preferences.composer_submit_behavior().source(),
        );
        map.insert(
            "local_profile.preferences.terminal.prompt_status_detail".to_owned(),
            preferences.prompt_status_detail().source(),
        );
        map.insert(
            "local_profile.preferences.gui.zoom_percent".to_owned(),
            preferences.gui_zoom_percent().source(),
        );
        map.insert(
            "local_profile.preferences.gui.font_size".to_owned(),
            preferences.gui_font_size().source(),
        );
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

fn parse_layer(kind: LayerKind, json: &str) -> Result<SettingsDocument, SettingsError> {
    let layer = kind.label();
    if json.trim().is_empty() {
        return Ok(SettingsDocument::default());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| SettingsError::MalformedLayer { layer })?;
    if !value.is_object() {
        return Err(SettingsError::MalformedLayer { layer });
    }
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::from(SETTINGS_SCHEMA_VERSION));
    if version > u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(SettingsError::UnsupportedSchemaVersion {
            layer,
            found: u32::try_from(version).unwrap_or(u32::MAX),
        });
    }
    if kind == LayerKind::WorkspaceFile {
        validate_workspace_document(&value, version)?;
    }
    serde_json::from_value(value).map_err(|_| SettingsError::MalformedLayer { layer })
}

fn parse_provider_value(value: &str) -> Option<ProviderKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gemini" | "google" => Some(ProviderKind::Gemini),
        "router" | "openai" => Some(ProviderKind::Router),
        _ => None,
    }
}

fn validate_workspace_document(
    value: &serde_json::Value,
    schema_version: u64,
) -> Result<(), SettingsError> {
    let document = value
        .as_object()
        .expect("workspace documents are checked to be objects before validation");
    for (key, value) in document {
        match key.as_str() {
            "schema_version" => {}
            "local_profile" => validate_workspace_local_profile(value, schema_version)?,
            _ => return Err(disallowed_workspace_key(key)),
        }
    }
    Ok(())
}

fn validate_workspace_local_profile(
    value: &serde_json::Value,
    schema_version: u64,
) -> Result<(), SettingsError> {
    let Some(profile) = value.as_object() else {
        return Ok(());
    };
    for (key, value) in profile {
        match key.as_str() {
            "preferences" => validate_workspace_preferences(value, schema_version)?,
            _ => return Err(disallowed_workspace_key(&format!("local_profile.{key}"))),
        }
    }
    Ok(())
}

fn validate_workspace_preferences(
    value: &serde_json::Value,
    schema_version: u64,
) -> Result<(), SettingsError> {
    let Some(preferences) = value.as_object() else {
        return Ok(());
    };
    if schema_version < u64::from(SETTINGS_SCHEMA_VERSION) {
        return validate_legacy_workspace_preferences(preferences);
    }
    for (key, value) in preferences {
        match key.as_str() {
            "shared" => validate_workspace_preference_group(
                value,
                "shared",
                &[
                    "theme_preset",
                    "color_mode",
                    "reduced_motion",
                    "density",
                    "timestamp_style",
                ],
            )?,
            "gui" => {
                validate_workspace_preference_group(value, "gui", &["zoom_percent", "font_size"])?
            }
            "terminal" => validate_workspace_preference_group(
                value,
                "terminal",
                &["glyph_mode", "layout", "prompt_status_detail"],
            )?,
            _ => {
                return Err(disallowed_workspace_key(&format!(
                    "local_profile.preferences.{key}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_legacy_workspace_preferences(
    preferences: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), SettingsError> {
    for key in preferences.keys() {
        match key.as_str() {
            "theme_preset"
            | "color_mode"
            | "glyph_mode"
            | "reduced_motion"
            | "density"
            | "layout"
            | "terminal_timestamp_style"
            | "prompt_status_detail" => {}
            _ => {
                return Err(disallowed_workspace_key(&format!(
                    "local_profile.preferences.{key}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_workspace_preference_group(
    value: &serde_json::Value,
    group: &str,
    permitted: &[&str],
) -> Result<(), SettingsError> {
    let Some(values) = value.as_object() else {
        return Ok(());
    };
    for key in values.keys() {
        if !permitted.contains(&key.as_str()) {
            return Err(disallowed_workspace_key(&format!(
                "local_profile.preferences.{group}.{key}"
            )));
        }
    }
    Ok(())
}

fn disallowed_workspace_key(key: &str) -> SettingsError {
    SettingsError::DisallowedWorkspaceKey {
        key: key.to_owned(),
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
    merge_local_profile(&mut merged.local_profile, document.local_profile, source);
}

fn merge_local_profile(merged: &mut RawLocalProfile, profile: LocalProfile, source: Source) {
    if let Some(label) = profile.display_label() {
        merged.display_label = Some((label.clone(), source));
    }
    let preferences = profile.preferences();
    if let Some(value) = preferences.theme_preset() {
        merged.theme_preset = Some((value, source));
    }
    if let Some(value) = preferences.color_mode() {
        merged.color_mode = Some((value, source));
    }
    if let Some(value) = preferences.glyph_mode() {
        merged.glyph_mode = Some((value, source));
    }
    if let Some(value) = preferences.reduced_motion() {
        merged.reduced_motion = Some((value, source));
    }
    if let Some(value) = preferences.density() {
        merged.density = Some((value, source));
    }
    if let Some(value) = preferences.layout() {
        merged.layout = Some((value, source));
    }
    if let Some(value) = preferences.terminal_timestamp_style() {
        merged.terminal_timestamp_style = Some((value, source));
    }
    if let Some(value) = preferences.composer_submit_behavior() {
        merged.composer_submit_behavior = Some((value, source));
    }
    if let Some(value) = preferences.prompt_status_detail() {
        merged.prompt_status_detail = Some((value, source));
    }
    if let Some(value) = preferences.gui_zoom_percent() {
        merged.gui_zoom_percent = Some((value, source));
    }
    if let Some(value) = preferences.gui_font_size() {
        merged.gui_font_size = Some((value, source));
    }
}

fn effective_local_profile(raw: &RawLocalProfile) -> EffectiveLocalProfile {
    let mut preferences = EffectiveLocalPreferences::default();
    preferences.set_theme_preset(effective_leaf(&raw.theme_preset));
    preferences.set_color_mode(effective_leaf(&raw.color_mode));
    preferences.set_glyph_mode(effective_leaf(&raw.glyph_mode));
    preferences.set_reduced_motion(effective_leaf(&raw.reduced_motion));
    preferences.set_density(effective_leaf(&raw.density));
    preferences.set_layout(effective_leaf(&raw.layout));
    preferences.set_terminal_timestamp_style(effective_leaf(&raw.terminal_timestamp_style));
    preferences.set_composer_submit_behavior(effective_leaf(&raw.composer_submit_behavior));
    preferences.set_prompt_status_detail(effective_leaf(&raw.prompt_status_detail));
    preferences.set_gui_zoom_percent(effective_leaf(&raw.gui_zoom_percent));
    preferences.set_gui_font_size(effective_leaf(&raw.gui_font_size));
    EffectiveLocalProfile::new(effective_optional_leaf(&raw.display_label), preferences)
}

fn effective_leaf<T>(value: &Option<(T, Source)>) -> EffectiveValue<T>
where
    T: Clone + Default,
{
    match value {
        Some((value, source)) => EffectiveValue::new(value.clone(), *source),
        None => EffectiveValue::new(T::default(), Source::Default),
    }
}

fn effective_optional_leaf<T>(value: &Option<(T, Source)>) -> EffectiveValue<Option<T>>
where
    T: Clone,
{
    match value {
        Some((value, source)) => EffectiveValue::new(Some(value.clone()), *source),
        None => EffectiveValue::new(None, Source::Default),
    }
}
