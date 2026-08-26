//! Native Codex subscription authentication and Responses transport.
//!
//! The adapter owns the browser PKCE callback, stores opaque OAuth credentials
//! through the application vault boundary, and does not require a separate CLI.

mod oauth;
mod provider;
mod settings;

pub use oauth::{CodexAuthProgress, CodexOAuthCredential, login_with_browser};
pub use provider::{CodexCredentialPersistence, CodexProvider};
pub use settings::CodexSettings;

/// Stable provider identity retained for settings compatibility.
pub const CODEX_PROVIDER_ID: &str = "codex-cli";
/// Placeholder model representing the subscription's current default.
pub const CODEX_DEFAULT_MODEL_ID: &str = "codex/default";
