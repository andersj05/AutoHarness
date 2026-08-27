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
/// Legacy placeholder model retained for persisted session compatibility.
///
/// The native adapter has no subscription-scoped model-discovery endpoint, so
/// requests for this identifier resolve to the adapter's verified fallback.
pub const CODEX_DEFAULT_MODEL_ID: &str = "codex/default";
