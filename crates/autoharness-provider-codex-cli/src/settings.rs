use std::env;
use std::fmt::{self, Debug, Formatter};
use std::path::{Component, Path, PathBuf};

use autoharness_domain::{ProviderId, RetryAdvice};
use autoharness_provider::{ProviderError, ProviderErrorKind};

use crate::CODEX_PROVIDER_ID;

/// Optional path to the official Codex executable.
pub const CODEX_EXECUTABLE_ENV: &str = "AUTOHARNESS_CODEX_EXECUTABLE";

const MAX_EXECUTABLE_PATH_BYTES: usize = 32 * 1024;
const OFFICIAL_EXECUTABLE_NAMES: [&str; 4] = ["codex", "codex.exe", "codex.cmd", "codex.bat"];

/// Validated non-secret configuration for the official Codex CLI.
#[derive(Clone)]
pub struct CodexCliSettings {
    executable: PathBuf,
    provider_id: ProviderId,
}

impl CodexCliSettings {
    /// Configures an official `codex` executable by path or by its `PATH` name.
    pub fn new(executable: impl Into<PathBuf>) -> Result<Self, ProviderError> {
        let executable = executable.into();
        validate_executable(&executable)?;
        let provider_id =
            ProviderId::new(CODEX_PROVIDER_ID).map_err(|_| invalid_configuration())?;
        Ok(Self {
            executable,
            provider_id,
        })
    }

    /// Reads the optional executable location without inspecting credential variables.
    pub fn from_env() -> Result<Self, ProviderError> {
        let executable = env::var_os(CODEX_EXECUTABLE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("codex"));
        Self::new(executable)
    }

    /// Returns the executable path passed directly to the operating system.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Returns this adapter's stable provider identity.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
}

impl Debug for CodexCliSettings {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexCliSettings")
            .field("executable", &"[CONFIGURED]")
            .field("provider_id", &self.provider_id)
            .finish()
    }
}

fn validate_executable(executable: &Path) -> Result<(), ProviderError> {
    let raw = executable.to_str().ok_or_else(invalid_configuration)?;
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return Err(invalid_configuration());
    };

    if raw.is_empty()
        || raw.len() > MAX_EXECUTABLE_PATH_BYTES
        || raw.chars().any(char::is_control)
        || executable
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !OFFICIAL_EXECUTABLE_NAMES
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
    {
        return Err(invalid_configuration());
    }
    Ok(())
}

fn invalid_configuration() -> ProviderError {
    ProviderError::new(ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_accept_only_lexically_safe_official_executable_names() {
        assert!(CodexCliSettings::new("codex").is_ok());
        assert!(CodexCliSettings::new(PathBuf::from("Codex").join("codex.exe")).is_ok());
        assert!(CodexCliSettings::new("../codex").is_err());
        assert!(CodexCliSettings::new("not-codex").is_err());
        assert!(CodexCliSettings::new("codex\u{0000}").is_err());
    }

    #[test]
    fn debug_output_does_not_reveal_executable_location() {
        let settings =
            CodexCliSettings::new(PathBuf::from("private").join("codex.exe")).expect("settings");
        assert!(!format!("{settings:?}").contains("private"));
    }
}
