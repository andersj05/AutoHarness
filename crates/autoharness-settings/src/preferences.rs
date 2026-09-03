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

/// The named cross-client appearance preset.
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

/// The cross-client color treatment.
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

/// Cross-client information density.
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

/// How renderer timestamps are displayed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampStyle {
    /// Render elapsed, relative timestamps.
    #[default]
    Relative,
    /// Render absolute timestamps.
    Absolute,
    /// Do not render timestamps.
    Hidden,
}

/// Compatibility name retained while the terminal renderer migrates to the
/// cross-client timestamp preference.
pub type TerminalTimestampStyle = TimestampStyle;

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

/// Base prose size used by the desktop renderer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiFontSize {
    /// Use a compact 14-pixel conversation base.
    Small,
    /// Use the balanced 16-pixel conversation base.
    #[default]
    Standard,
    /// Use an 18-pixel conversation base.
    Large,
    /// Use a 20-pixel conversation base.
    ExtraLarge,
}

/// Validated whole-percent desktop zoom from 75 through 200 percent.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GuiZoomPercent(u16);

impl GuiZoomPercent {
    /// Minimum supported desktop zoom.
    pub const MIN: u16 = 75;
    /// Maximum supported desktop zoom, matching the Stage 6 accessibility gate.
    pub const MAX: u16 = 200;

    /// Creates a supported whole-percent desktop zoom.
    pub const fn new(value: u16) -> Result<Self, &'static str> {
        if value < Self::MIN || value > Self::MAX {
            return Err("GUI zoom must be between 75 and 200 percent");
        }
        Ok(Self(value))
    }

    /// Returns the whole-percent zoom value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Default for GuiZoomPercent {
    fn default() -> Self {
        Self(100)
    }
}

impl<'de> Deserialize<'de> for GuiZoomPercent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
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

/// Cross-client preferences stored by one configuration layer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme_preset: Option<ThemePreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color_mode: Option<ColorMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reduced_motion: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    density: Option<Density>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timestamp_style: Option<TimestampStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composer_submit_behavior: Option<ComposerSubmitBehavior>,
}

impl SharedPreferences {
    const fn is_empty(&self) -> bool {
        self.theme_preset.is_none()
            && self.color_mode.is_none()
            && self.reduced_motion.is_none()
            && self.density.is_none()
            && self.timestamp_style.is_none()
            && self.composer_submit_behavior.is_none()
    }
}

/// Desktop-only presentation preferences stored by one configuration layer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuiPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    zoom_percent: Option<GuiZoomPercent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_size: Option<GuiFontSize>,
}

impl GuiPreferences {
    const fn is_empty(&self) -> bool {
        self.zoom_percent.is_none() && self.font_size.is_none()
    }
}

/// Terminal-only presentation preferences stored by one configuration layer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glyph_mode: Option<GlyphMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_status_detail: Option<PromptStatusDetail>,
}

impl TerminalPreferences {
    const fn is_empty(&self) -> bool {
        self.glyph_mode.is_none() && self.layout.is_none() && self.prompt_status_detail.is_none()
    }
}

/// Optional local preferences stored by one configuration layer.
///
/// An absent field intentionally means that the layer does not override that
/// preference. Clearing a user-layer field therefore resets it to a lower
/// layer or its built-in default.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPreferences {
    #[serde(default, skip_serializing_if = "SharedPreferences::is_empty")]
    shared: SharedPreferences,
    #[serde(default, skip_serializing_if = "GuiPreferences::is_empty")]
    gui: GuiPreferences,
    #[serde(default, skip_serializing_if = "TerminalPreferences::is_empty")]
    terminal: TerminalPreferences,
}

impl LocalPreferences {
    /// Creates an empty preference layer that inherits every effective value.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            shared: SharedPreferences {
                theme_preset: None,
                color_mode: None,
                reduced_motion: None,
                density: None,
                timestamp_style: None,
                composer_submit_behavior: None,
            },
            gui: GuiPreferences {
                zoom_percent: None,
                font_size: None,
            },
            terminal: TerminalPreferences {
                glyph_mode: None,
                layout: None,
                prompt_status_detail: None,
            },
        }
    }

    /// Returns whether this layer overrides no preferences.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.shared.is_empty() && self.gui.is_empty() && self.terminal.is_empty()
    }

    /// Returns the cross-client preference layer.
    #[must_use]
    pub const fn shared(&self) -> &SharedPreferences {
        &self.shared
    }

    /// Returns the desktop-only preference layer.
    #[must_use]
    pub const fn gui(&self) -> &GuiPreferences {
        &self.gui
    }

    /// Returns the terminal-only preference layer.
    #[must_use]
    pub const fn terminal(&self) -> &TerminalPreferences {
        &self.terminal
    }

    /// Returns this layer's optional theme preset.
    #[must_use]
    pub const fn theme_preset(&self) -> Option<ThemePreset> {
        self.shared.theme_preset
    }

    /// Sets this layer's theme preset, or clears it to inherit.
    pub fn set_theme_preset(&mut self, value: Option<ThemePreset>) {
        self.shared.theme_preset = value;
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
        self.shared.color_mode
    }

    /// Sets this layer's color mode, or clears it to inherit.
    pub fn set_color_mode(&mut self, value: Option<ColorMode>) {
        self.shared.color_mode = value;
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
        self.terminal.glyph_mode
    }

    /// Sets this layer's glyph mode, or clears it to inherit.
    pub fn set_glyph_mode(&mut self, value: Option<GlyphMode>) {
        self.terminal.glyph_mode = value;
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
        self.shared.reduced_motion
    }

    /// Sets reduced motion for this layer, or clears it to inherit.
    pub fn set_reduced_motion(&mut self, value: Option<bool>) {
        self.shared.reduced_motion = value;
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
        self.shared.density
    }

    /// Sets this layer's density, or clears it to inherit.
    pub fn set_density(&mut self, value: Option<Density>) {
        self.shared.density = value;
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
        self.terminal.layout
    }

    /// Sets this layer's layout, or clears it to inherit.
    pub fn set_layout(&mut self, value: Option<Layout>) {
        self.terminal.layout = value;
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
        self.shared.timestamp_style
    }

    /// Sets this layer's timestamp style, or clears it to inherit.
    pub fn set_terminal_timestamp_style(&mut self, value: Option<TerminalTimestampStyle>) {
        self.shared.timestamp_style = value;
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
        self.shared.composer_submit_behavior
    }

    /// Sets this layer's composer submit behavior, or clears it to inherit.
    pub fn set_composer_submit_behavior(&mut self, value: Option<ComposerSubmitBehavior>) {
        self.shared.composer_submit_behavior = value;
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
        self.terminal.prompt_status_detail
    }

    /// Sets the prompt status-bar detail level, or clears it to inherit.
    pub fn set_prompt_status_detail(&mut self, value: Option<PromptStatusDetail>) {
        self.terminal.prompt_status_detail = value;
    }

    /// Returns this layer with a prompt status-bar detail override.
    #[must_use]
    pub fn with_prompt_status_detail(mut self, value: PromptStatusDetail) -> Self {
        self.set_prompt_status_detail(Some(value));
        self
    }

    /// Returns this layer's optional desktop zoom percentage.
    #[must_use]
    pub const fn gui_zoom_percent(&self) -> Option<GuiZoomPercent> {
        self.gui.zoom_percent
    }

    /// Sets desktop zoom for this layer, or clears it to inherit.
    pub fn set_gui_zoom_percent(&mut self, value: Option<GuiZoomPercent>) {
        self.gui.zoom_percent = value;
    }

    /// Returns this layer with a desktop zoom override.
    #[must_use]
    pub fn with_gui_zoom_percent(mut self, value: GuiZoomPercent) -> Self {
        self.set_gui_zoom_percent(Some(value));
        self
    }

    /// Returns this layer's optional desktop font size.
    #[must_use]
    pub const fn gui_font_size(&self) -> Option<GuiFontSize> {
        self.gui.font_size
    }

    /// Sets desktop font size for this layer, or clears it to inherit.
    pub fn set_gui_font_size(&mut self, value: Option<GuiFontSize>) {
        self.gui.font_size = value;
    }

    /// Returns this layer with a desktop font-size override.
    #[must_use]
    pub fn with_gui_font_size(mut self, value: GuiFontSize) -> Self {
        self.set_gui_font_size(Some(value));
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
    gui_zoom_percent: EffectiveValue<GuiZoomPercent>,
    gui_font_size: EffectiveValue<GuiFontSize>,
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
            gui_zoom_percent: EffectiveValue::new(GuiZoomPercent::default(), Source::Default),
            gui_font_size: EffectiveValue::new(GuiFontSize::default(), Source::Default),
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

    pub(crate) fn set_gui_zoom_percent(&mut self, value: EffectiveValue<GuiZoomPercent>) {
        self.gui_zoom_percent = value;
    }

    pub(crate) fn set_gui_font_size(&mut self, value: EffectiveValue<GuiFontSize>) {
        self.gui_font_size = value;
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

    /// Returns the effective desktop zoom percentage.
    #[must_use]
    pub const fn gui_zoom_percent(&self) -> &EffectiveValue<GuiZoomPercent> {
        &self.gui_zoom_percent
    }

    /// Returns the effective desktop font size.
    #[must_use]
    pub const fn gui_font_size(&self) -> &EffectiveValue<GuiFontSize> {
        &self.gui_font_size
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
