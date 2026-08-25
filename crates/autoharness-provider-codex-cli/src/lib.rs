//! Official Codex CLI adapter.
//!
//! The adapter delegates authentication to the installed `codex` executable,
//! invokes only documented non-interactive commands, and keeps Codex-owned tool
//! activity outside AutoHarness's provider-neutral tool authority.

mod jsonl;
mod provider;
mod settings;

pub use provider::CodexCliProvider;
pub use settings::{CODEX_EXECUTABLE_ENV, CodexCliSettings};

/// Stable provider identity for the official Codex CLI adapter.
pub const CODEX_PROVIDER_ID: &str = "codex-cli";
/// Placeholder model representing the authenticated CLI's configured default.
pub const CODEX_DEFAULT_MODEL_ID: &str = "codex/default";
