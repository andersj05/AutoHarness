//! Versioned typed settings resolution for AutoHarness.
//!
//! The resolver merges five layers in fixed precedence: built-in defaults,
//! user file, workspace file, environment, and command-line overrides.
//! Every effective value records which layer supplied it.

mod error;
mod profile;
mod resolver;
mod source;

pub use error::SettingsError;
pub use profile::{CredentialReference, ProfileId, ProviderKind, ProviderProfile};
pub use resolver::{LayerKind, ResolvedSettings, SettingsBuilder};
pub use source::Source;
