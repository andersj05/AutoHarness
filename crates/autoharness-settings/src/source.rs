use std::fmt;

/// Which configuration layer supplied an effective value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// Built-in default when no layer supplies the key.
    Default,
    /// The per-user settings file.
    UserFile,
    /// The workspace-local settings file.
    WorkspaceFile,
    /// A process environment variable.
    Environment,
    /// An explicit command-line or in-app override.
    CommandLine,
}

impl Source {
    /// Returns the stable lowercase label shown to users and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::UserFile => "user file",
            Self::WorkspaceFile => "workspace file",
            Self::Environment => "environment",
            Self::CommandLine => "command line",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
