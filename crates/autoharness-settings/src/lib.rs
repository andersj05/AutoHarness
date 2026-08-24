//! Versioned typed settings resolution for AutoHarness.
//!
//! The resolver merges five layers in fixed precedence: built-in defaults,
//! user file, workspace file, environment, and command-line overrides.
//! Every effective value records which layer supplied it.

mod error;
mod preferences;
mod profile;
mod resolver;
mod source;

pub use error::SettingsError;
pub use preferences::{
    ColorMode, ComposerSubmitBehavior, Density, DisplayLabel, EffectiveLocalPreferences,
    EffectiveLocalProfile, EffectiveValue, GlyphMode, Layout, LocalPreferences, LocalProfile,
    MAX_DISPLAY_LABEL_CHARS, TerminalTimestampStyle, ThemePreset,
};
pub use profile::{
    CredentialDocument, CredentialRecoveryKind, CredentialRecoveryRecord, CredentialReference,
    ProfileId, ProviderKind, ProviderProfile, SETTINGS_SCHEMA_VERSION, SettingsDocument,
};
pub use resolver::{LayerKind, ResolvedSettings, SettingsBuilder};
pub use source::Source;
