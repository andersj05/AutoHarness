use std::fmt;

/// Safe settings-resolution failures surfaced to users.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsError {
    /// A layer could not be parsed; the offending layer is skipped safely.
    MalformedLayer {
        /// Human-oriented layer name for diagnostics.
        layer: &'static str,
    },
    /// The document declares a schema version this build cannot read.
    UnsupportedSchemaVersion {
        /// Layer name for diagnostics.
        layer: &'static str,
        /// The declared version that was refused.
        found: u32,
    },
    /// A workspace document tried to override a protected key.
    DisallowedWorkspaceKey {
        /// The rejected key path.
        key: String,
    },
    /// Merged output failed validation, such as an unknown active profile.
    InvalidMerge {
        /// Safe explanation of the violated rule.
        reason: String,
    },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLayer { layer } => {
                write!(formatter, "the {layer} could not be parsed and was ignored")
            }
            Self::UnsupportedSchemaVersion { layer, found } => {
                write!(
                    formatter,
                    "the {layer} declares unsupported schema version {found}; it was ignored"
                )
            }
            Self::DisallowedWorkspaceKey { key } => write!(
                formatter,
                "the workspace settings file may not override '{key}'"
            ),
            Self::InvalidMerge { reason } => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for SettingsError {}
