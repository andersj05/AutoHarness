use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

use crate::source::Source;

/// Maximum number of visible characters accepted in a local display label.
pub const MAX_DISPLAY_LABEL_CHARS: usize = 64;

/// A bounded, non-secret label shown for the local user profile.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisplayLabel(Arc<str>);

impl DisplayLabel {
    /// Creates a non-empty label with at most 64 non-control characters.
    pub fn new(value: impl AsRef<str>) -> Result<Self, &'static str> {
        let value = value.as_ref();
        if value.trim().is_empty() {
            return Err("display label must not be empty");
        }
        if value.chars().count() > MAX_DISPLAY_LABEL_CHARS {
            return Err("display label must be at most 64 characters");
        }
        if value.chars().any(char::is_control) {
            return Err("display label must not contain control characters");
        }
        Ok(Self(Arc::from(value)))
    }

    /// Returns the display label exactly as configured.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for DisplayLabel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DisplayLabel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// The named terminal appearance preset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    /// Follow the terminal or operating-system appearance when possible.
    #[default]
    System,
    /// Prefer a light terminal appearance.
    Light,
    /// Prefer a dark terminal appearance.
    Dark,
    /// Cool aurora gradients expressed through terminal-safe cyan and violet.
    Aurora,
    /// Warm ember gradients expressed through amber, coral, and magenta.
    Ember,
    /// Deep navy surfaces with crisp indigo and electric-blue accents.
    Midnight,
    /// Ocean-blue surfaces with bright aqua and sea-glass accents.
    Ocean,
    /// Forest-green surfaces with mint, moss, and amber accents.
    Forest,
    /// Rose-dark surfaces with pink, plum, and coral accents.
    Rose,
}

/// The terminal color treatment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// Render the standard color palette.
    #[default]
    Color,
    /// Soften non-essential color and reduce visual intensity.
    Soft,
    /// Strengthen semantic color and focus emphasis.
    Vivid,
    /// Render without color distinctions.
    NoColor,
    /// Render a high-contrast palette.
    HighContrast,
}

/// The character set used for terminal decoration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlyphMode {
    /// Use Unicode box drawing and symbols.
    #[default]
    Unicode,
    /// Use Nerd Font symbols when the active terminal font provides them.
    NerdFont,
    /// Use ASCII-only terminal decoration.
    Ascii,
}

/// Terminal information density.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    /// Use the standard spacing between terminal elements.
    #[default]
    Comfortable,
    /// Use denser terminal spacing.
    Compact,
}

/// Terminal layout behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Adapt terminal panels to the available width.
    #[default]
    Responsive,
    /// Keep terminal content in one vertical column.
    SingleColumn,
}

/// How terminal timestamps are rendered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTimestampStyle {
    /// Render elapsed, relative timestamps.
    #[default]
    Relative,
    /// Render absolute timestamps.
    Absolute,
    /// Do not render timestamps.
    Hidden,
}

/// The keyboard behavior used to submit a composer prompt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerSubmitBehavior {
    /// Submit with the existing Control-S shortcut and keep Enter for newlines.
    #[default]
    ControlS,
    /// Submit with Enter.
    Enter,
}

/// Amount of trusted runtime context shown in the prompt status bar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStatusDetail {
    /// Show only model, thinking, and context utilization.
    Essential,
    /// Add compact workspace path and Git branch context.
    #[default]
    Workspace,
    /// Add latest-turn input and output token usage when available.
    Detailed,
}

/// Optional local preferences stored by one configuration layer.
///
/// An absent field intentionally means that the layer does not override that
/// preference. Clearing a user-layer field therefore resets it to a lower
/// layer or its built-in default.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme_preset: Option<ThemePreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color_mode: Option<ColorMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glyph_mode: Option<GlyphMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reduced_motion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    density: Option<Density>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal_timestamp_style: Option<TerminalTimestampStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composer_submit_behavior: Option<ComposerSubmitBehavior>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_status_detail: Option<PromptStatusDetail>,
}

impl LocalPreferences {
    /// Creates an empty preference layer that inherits every effective value.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            theme_preset: None,
            color_mode: None,
            glyph_mode: None,
            reduced_motion: None,
            density: None,
            layout: None,
            terminal_timestamp_style: None,
            composer_submit_behavior: None,
            prompt_status_detail: None,
        }
    }

    /// Returns whether this layer overrides no preferences.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.theme_preset.is_none()
            && self.color_mode.is_none()
            && self.glyph_mode.is_none()
            && self.reduced_motion.is_none()
            && self.density.is_none()
            && self.layout.is_none()
            && self.terminal_timestamp_style.is_none()
            && self.composer_submit_behavior.is_none()
            && self.prompt_status_detail.is_none()
    }

    /// Returns this layer's optional theme preset.
    #[must_use]
    pub const fn theme_preset(&self) -> Option<ThemePreset> {
        self.theme_preset
    }

    /// Sets this layer's theme preset, or clears it to inherit.
    pub fn set_theme_preset(&mut self, value: Option<ThemePreset>) {
        self.theme_preset = value;
    }

    /// Returns this layer with a theme preset override.
    #[must_use]
    pub fn with_theme_preset(mut self, value: ThemePreset) -> Self {
        self.set_theme_preset(Some(value));
        self
    }

    /// Returns this layer's optional color mode.
    #[must_use]
    pub const fn color_mode(&self) -> Option<ColorMode> {
        self.color_mode
    }

    /// Sets this layer's color mode, or clears it to inherit.
    pub fn set_color_mode(&mut self, value: Option<ColorMode>) {
        self.color_mode = value;
    }

    /// Returns this layer with a color mode override.
    #[must_use]
    pub fn with_color_mode(mut self, value: ColorMode) -> Self {
        self.set_color_mode(Some(value));
        self
    }

    /// Returns this layer's optional glyph mode.
    #[must_use]
    pub const fn glyph_mode(&self) -> Option<GlyphMode> {
        self.glyph_mode
    }

    /// Sets this layer's glyph mode, or clears it to inherit.
    pub fn set_glyph_mode(&mut self, value: Option<GlyphMode>) {
        self.glyph_mode = value;
    }

    /// Returns this layer with a glyph mode override.
    #[must_use]
    pub fn with_glyph_mode(mut self, value: GlyphMode) -> Self {
        self.set_glyph_mode(Some(value));
        self
    }

    /// Returns whether this layer asks for reduced motion.
    #[must_use]
    pub const fn reduced_motion(&self) -> Option<bool> {
        self.reduced_motion
    }

    /// Sets reduced motion for this layer, or clears it to inherit.
    pub fn set_reduced_motion(&mut self, value: Option<bool>) {
        self.reduced_motion = value;
    }

    /// Returns this layer with a reduced-motion override.
    #[must_use]
    pub fn with_reduced_motion(mut self, value: bool) -> Self {
        self.set_reduced_motion(Some(value));
        self
    }

    /// Returns this layer's optional density.
    #[must_use]
    pub const fn density(&self) -> Option<Density> {
        self.density
    }

    /// Sets this layer's density, or clears it to inherit.
    pub fn set_density(&mut self, value: Option<Density>) {
        self.density = value;
    }

    /// Returns this layer with a density override.
    #[must_use]
    pub fn with_density(mut self, value: Density) -> Self {
        self.set_density(Some(value));
        self
    }

    /// Returns this layer's optional layout.
    #[must_use]
    pub const fn layout(&self) -> Option<Layout> {
        self.layout
    }

    /// Sets this layer's layout, or clears it to inherit.
    pub fn set_layout(&mut self, value: Option<Layout>) {
        self.layout = value;
    }

    /// Returns this layer with a layout override.
    #[must_use]
    pub fn with_layout(mut self, value: Layout) -> Self {
        self.set_layout(Some(value));
        self
    }

    /// Returns this layer's optional terminal timestamp style.
    #[must_use]
    pub const fn terminal_timestamp_style(&self) -> Option<TerminalTimestampStyle> {
        self.terminal_timestamp_style
    }

    /// Sets this layer's timestamp style, or clears it to inherit.
    pub fn set_terminal_timestamp_style(&mut self, value: Option<TerminalTimestampStyle>) {
        self.terminal_timestamp_style = value;
    }

    /// Returns this layer with a terminal timestamp style override.
    #[must_use]
    pub fn with_terminal_timestamp_style(mut self, value: TerminalTimestampStyle) -> Self {
        self.set_terminal_timestamp_style(Some(value));
        self
    }

    /// Returns this layer's optional composer submit behavior.
    #[must_use]
    pub const fn composer_submit_behavior(&self) -> Option<ComposerSubmitBehavior> {
        self.composer_submit_behavior
    }

    /// Sets this layer's composer submit behavior, or clears it to inherit.
    pub fn set_composer_submit_behavior(&mut self, value: Option<ComposerSubmitBehavior>) {
        self.composer_submit_behavior = value;
    }

    /// Returns this layer with a composer submit behavior override.
    #[must_use]
    pub fn with_composer_submit_behavior(mut self, value: ComposerSubmitBehavior) -> Self {
        self.set_composer_submit_behavior(Some(value));
        self
    }

    /// Returns the optional prompt status-bar detail level.
    #[must_use]
    pub const fn prompt_status_detail(&self) -> Option<PromptStatusDetail> {
        self.prompt_status_detail
    }

    /// Sets the prompt status-bar detail level, or clears it to inherit.
    pub fn set_prompt_status_detail(&mut self, value: Option<PromptStatusDetail>) {
        self.prompt_status_detail = value;
    }

    /// Returns this layer with a prompt status-bar detail override.
    #[must_use]
    pub fn with_prompt_status_detail(mut self, value: PromptStatusDetail) -> Self {
        self.set_prompt_status_detail(Some(value));
        self
    }
}

/// Local identity and UI preferences stored in a settings document.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_label: Option<DisplayLabel>,
    #[serde(default, skip_serializing_if = "LocalPreferences::is_empty")]
    preferences: LocalPreferences,
}

impl LocalProfile {
    /// Creates an empty local-profile layer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            display_label: None,
            preferences: LocalPreferences::new(),
        }
    }

    /// Returns whether the profile has no persisted values.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.display_label.is_none() && self.preferences.is_empty()
    }

    /// Returns this layer's optional local display label.
    #[must_use]
    pub fn display_label(&self) -> Option<&DisplayLabel> {
        self.display_label.as_ref()
    }

    /// Sets the local display label, or clears it to inherit no label.
    pub fn set_display_label(&mut self, value: Option<DisplayLabel>) {
        self.display_label = value;
    }

    /// Returns this local profile with a display label.
    #[must_use]
    pub fn with_display_label(mut self, value: DisplayLabel) -> Self {
        self.set_display_label(Some(value));
        self
    }

    /// Returns this layer's optional preferences.
    #[must_use]
    pub const fn preferences(&self) -> &LocalPreferences {
        &self.preferences
    }

    /// Replaces the optional preferences for this layer.
    pub fn set_preferences(&mut self, preferences: LocalPreferences) {
        self.preferences = preferences;
    }

    /// Returns this local profile with the supplied preference layer.
    #[must_use]
    pub fn with_preferences(mut self, preferences: LocalPreferences) -> Self {
        self.set_preferences(preferences);
        self
    }
}

/// One effective leaf value and the layer that supplied it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveValue<T> {
    value: T,
    source: Source,
}

impl<T> EffectiveValue<T> {
    pub(crate) const fn new(value: T, source: Source) -> Self {
        Self { value, source }
    }

    /// Returns the resolved value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Returns the layer that supplied the resolved value.
    #[must_use]
    pub const fn source(&self) -> Source {
        self.source
    }
}

/// Effective local preferences with provenance for each leaf.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveLocalPreferences {
    theme_preset: EffectiveValue<ThemePreset>,
    color_mode: EffectiveValue<ColorMode>,
    glyph_mode: EffectiveValue<GlyphMode>,
    reduced_motion: EffectiveValue<bool>,
    density: EffectiveValue<Density>,
    layout: EffectiveValue<Layout>,
    terminal_timestamp_style: EffectiveValue<TerminalTimestampStyle>,
    composer_submit_behavior: EffectiveValue<ComposerSubmitBehavior>,
    prompt_status_detail: EffectiveValue<PromptStatusDetail>,
}

impl Default for EffectiveLocalPreferences {
    fn default() -> Self {
        Self {
            theme_preset: EffectiveValue::new(ThemePreset::default(), Source::Default),
            color_mode: EffectiveValue::new(ColorMode::default(), Source::Default),
            glyph_mode: EffectiveValue::new(GlyphMode::default(), Source::Default),
            reduced_motion: EffectiveValue::new(false, Source::Default),
            density: EffectiveValue::new(Density::default(), Source::Default),
            layout: EffectiveValue::new(Layout::default(), Source::Default),
            terminal_timestamp_style: EffectiveValue::new(
                TerminalTimestampStyle::default(),
                Source::Default,
            ),
            composer_submit_behavior: EffectiveValue::new(
                ComposerSubmitBehavior::default(),
                Source::Default,
            ),
            prompt_status_detail: EffectiveValue::new(
                PromptStatusDetail::default(),
                Source::Default,
            ),
        }
    }
}

impl EffectiveLocalPreferences {
    pub(crate) fn set_theme_preset(&mut self, value: EffectiveValue<ThemePreset>) {
        self.theme_preset = value;
    }

    pub(crate) fn set_color_mode(&mut self, value: EffectiveValue<ColorMode>) {
        self.color_mode = value;
    }

    pub(crate) fn set_glyph_mode(&mut self, value: EffectiveValue<GlyphMode>) {
        self.glyph_mode = value;
    }

    pub(crate) fn set_reduced_motion(&mut self, value: EffectiveValue<bool>) {
        self.reduced_motion = value;
    }

    pub(crate) fn set_density(&mut self, value: EffectiveValue<Density>) {
        self.density = value;
    }

    pub(crate) fn set_layout(&mut self, value: EffectiveValue<Layout>) {
        self.layout = value;
    }

    pub(crate) fn set_terminal_timestamp_style(
        &mut self,
        value: EffectiveValue<TerminalTimestampStyle>,
    ) {
        self.terminal_timestamp_style = value;
    }

    pub(crate) fn set_composer_submit_behavior(
        &mut self,
        value: EffectiveValue<ComposerSubmitBehavior>,
    ) {
        self.composer_submit_behavior = value;
    }

    pub(crate) fn set_prompt_status_detail(&mut self, value: EffectiveValue<PromptStatusDetail>) {
        self.prompt_status_detail = value;
    }

    /// Returns the effective theme preset.
    #[must_use]
    pub const fn theme_preset(&self) -> &EffectiveValue<ThemePreset> {
        &self.theme_preset
    }

    /// Returns the effective color mode.
    #[must_use]
    pub const fn color_mode(&self) -> &EffectiveValue<ColorMode> {
        &self.color_mode
    }

    /// Returns the effective glyph mode.
    #[must_use]
    pub const fn glyph_mode(&self) -> &EffectiveValue<GlyphMode> {
        &self.glyph_mode
    }

    /// Returns the effective reduced-motion setting.
    #[must_use]
    pub const fn reduced_motion(&self) -> &EffectiveValue<bool> {
        &self.reduced_motion
    }

    /// Returns the effective density.
    #[must_use]
    pub const fn density(&self) -> &EffectiveValue<Density> {
        &self.density
    }

    /// Returns the effective layout.
    #[must_use]
    pub const fn layout(&self) -> &EffectiveValue<Layout> {
        &self.layout
    }

    /// Returns the effective terminal timestamp style.
    #[must_use]
    pub const fn terminal_timestamp_style(&self) -> &EffectiveValue<TerminalTimestampStyle> {
        &self.terminal_timestamp_style
    }

    /// Returns the effective composer submit behavior.
    #[must_use]
    pub const fn composer_submit_behavior(&self) -> &EffectiveValue<ComposerSubmitBehavior> {
        &self.composer_submit_behavior
    }

    /// Returns the effective prompt status-bar detail level.
    #[must_use]
    pub const fn prompt_status_detail(&self) -> &EffectiveValue<PromptStatusDetail> {
        &self.prompt_status_detail
    }
}

/// Effective local profile with provenance for each persisted leaf.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveLocalProfile {
    display_label: EffectiveValue<Option<DisplayLabel>>,
    preferences: EffectiveLocalPreferences,
}

impl Default for EffectiveLocalProfile {
    fn default() -> Self {
        Self::new(
            EffectiveValue::new(None, Source::Default),
            EffectiveLocalPreferences::default(),
        )
    }
}
impl EffectiveLocalProfile {
    pub(crate) const fn new(
        display_label: EffectiveValue<Option<DisplayLabel>>,
        preferences: EffectiveLocalPreferences,
    ) -> Self {
        Self {
            display_label,
            preferences,
        }
    }

    /// Returns the effective local display label and its source.
    #[must_use]
    pub const fn display_label(&self) -> &EffectiveValue<Option<DisplayLabel>> {
        &self.display_label
    }

    /// Returns the effective local preferences and their leaf sources.
    #[must_use]
    pub const fn preferences(&self) -> &EffectiveLocalPreferences {
        &self.preferences
    }
}
