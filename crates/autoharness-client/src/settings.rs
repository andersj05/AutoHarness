use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::ValidationError;

/// Named appearance identity shared by supported renderers.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    #[default]
    System,
    Light,
    Dark,
    Aurora,
    Ember,
    Midnight,
    Ocean,
    Forest,
    Rose,
}

/// Cross-client color and contrast treatment.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    #[default]
    Color,
    Soft,
    Vivid,
    NoColor,
    HighContrast,
}

/// Cross-client information density.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

/// Timestamp presentation shared by renderer session surfaces.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampStyle {
    #[default]
    Relative,
    Absolute,
    Hidden,
}

/// Keyboard chord that submits a multiline prompt.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerSubmitBehavior {
    #[default]
    ControlS,
    Enter,
}

/// Base conversation font size used by the desktop renderer.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiFontSize {
    Small,
    #[default]
    Standard,
    Large,
    ExtraLarge,
}

/// Validated whole-percent desktop zoom.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GuiZoomPercent(u16);

impl GuiZoomPercent {
    pub const MIN: u16 = 75;
    pub const MAX: u16 = 200;

    pub const fn new(value: u16) -> Result<Self, ValidationError> {
        if value < Self::MIN || value > Self::MAX {
            return Err(ValidationError::Inconsistent { field: "gui_zoom" });
        }
        Ok(Self(value))
    }

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
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Configuration layer that supplied an effective setting.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceSource {
    #[default]
    Default,
    UserFile,
    WorkspaceFile,
    Environment,
    CommandLine,
}

/// One effective setting, its provenance, and whether the user layer overrides it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveSetting<T> {
    pub value: T,
    pub source: PreferenceSource,
    pub user_override: bool,
}

impl<T> EffectiveSetting<T> {
    #[must_use]
    pub const fn new(value: T, source: PreferenceSource, user_override: bool) -> Self {
        Self {
            value,
            source,
            user_override,
        }
    }
}

/// Renderer-relevant settings projected without storage authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientSettingsProjection {
    pub theme_preset: EffectiveSetting<ThemePreset>,
    pub color_mode: EffectiveSetting<ColorMode>,
    pub zoom_percent: EffectiveSetting<GuiZoomPercent>,
    pub font_size: EffectiveSetting<GuiFontSize>,
    pub density: EffectiveSetting<Density>,
    pub reduced_motion: EffectiveSetting<bool>,
    pub timestamp_style: EffectiveSetting<TimestampStyle>,
    pub composer_submit_behavior: EffectiveSetting<ComposerSubmitBehavior>,
}

impl Default for ClientSettingsProjection {
    fn default() -> Self {
        Self {
            theme_preset: EffectiveSetting::new(
                ThemePreset::default(),
                PreferenceSource::Default,
                false,
            ),
            color_mode: EffectiveSetting::new(
                ColorMode::default(),
                PreferenceSource::Default,
                false,
            ),
            zoom_percent: EffectiveSetting::new(
                GuiZoomPercent::default(),
                PreferenceSource::Default,
                false,
            ),
            font_size: EffectiveSetting::new(
                GuiFontSize::default(),
                PreferenceSource::Default,
                false,
            ),
            density: EffectiveSetting::new(Density::default(), PreferenceSource::Default, false),
            reduced_motion: EffectiveSetting::new(false, PreferenceSource::Default, false),
            timestamp_style: EffectiveSetting::new(
                TimestampStyle::default(),
                PreferenceSource::Default,
                false,
            ),
            composer_submit_behavior: EffectiveSetting::new(
                ComposerSubmitBehavior::default(),
                PreferenceSource::Default,
                false,
            ),
        }
    }
}

/// One typed user-layer preference mutation.
///
/// A missing value clears the user override so the next permitted layer wins.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ClientPreferenceChange {
    ThemePreset {
        value: Option<ThemePreset>,
    },
    ColorMode {
        value: Option<ColorMode>,
    },
    ZoomPercent {
        value: Option<GuiZoomPercent>,
    },
    FontSize {
        value: Option<GuiFontSize>,
    },
    Density {
        value: Option<Density>,
    },
    ReducedMotion {
        value: Option<bool>,
    },
    TimestampStyle {
        value: Option<TimestampStyle>,
    },
    ComposerSubmitBehavior {
        value: Option<ComposerSubmitBehavior>,
    },
}
