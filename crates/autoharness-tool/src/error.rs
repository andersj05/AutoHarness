use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    AttemptFailure, ClassifiedError, ErrorClass, ErrorCode, PublicMessage, RetryAdvice,
};

/// Stable tool-runtime failure category without secret-bearing source details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolErrorKind {
    /// The tool name, schema, or arguments are invalid.
    InvalidCall,
    /// An internal memory proposal reached the external capability runtime.
    MemoryProposalSinkRequired,
    /// The exact call lacks execution authority.
    PermissionDenied,
    /// A path escaped or could not be accessed through the workspace capability.
    Filesystem,
    /// A shell-free child process failed.
    Process,
    /// An HTTP capability failed.
    Http,
    /// Execution exceeded its deadline.
    Timeout,
    /// Cooperative cancellation won the terminal race.
    Cancelled,
    /// Captured output exceeded the hard run bound.
    OutputLimit,
    /// Provider repair or tool continuation exceeded the hard turn bound.
    TurnLimit,
    /// Full output could not be retained as an artifact.
    Artifact,
    /// An internal invariant failed.
    Internal,
}

/// Sanitized error safe for events, logs, and the terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolError {
    kind: ToolErrorKind,
    retry: RetryAdvice,
}

impl ToolError {
    /// Constructs a classified sanitized failure.
    #[must_use]
    pub const fn new(kind: ToolErrorKind, retry: RetryAdvice) -> Self {
        Self { kind, retry }
    }

    /// Returns the stable runtime category.
    #[must_use]
    pub const fn kind(&self) -> ToolErrorKind {
        self.kind
    }

    /// Converts the error into the existing provider-neutral durable failure value.
    #[must_use]
    pub fn durable_failure(&self) -> AttemptFailure {
        AttemptFailure::new(
            self.class(),
            ErrorCode::new(error_code(self.kind)).expect("static tool error code is valid"),
            PublicMessage::new(self.to_string()).expect("static tool message is valid"),
            self.retry,
        )
    }
}

impl Display for ToolError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            ToolErrorKind::InvalidCall => "The model requested an invalid or unknown tool call",
            ToolErrorKind::MemoryProposalSinkRequired => {
                "The memory proposal requires the application review sink"
            }
            ToolErrorKind::PermissionDenied => "The exact tool capability was not authorized",
            ToolErrorKind::Filesystem => "The workspace filesystem operation failed",
            ToolErrorKind::Process => "The process operation failed",
            ToolErrorKind::Http => "The HTTP operation failed",
            ToolErrorKind::Timeout => "The tool operation exceeded its deadline",
            ToolErrorKind::Cancelled => "The tool operation was cancelled",
            ToolErrorKind::OutputLimit => "The tool output exceeded the run limit",
            ToolErrorKind::TurnLimit => "The provider turn limit was reached",
            ToolErrorKind::Artifact => "The full tool output could not be retained",
            ToolErrorKind::Internal => "The tool runtime encountered an internal error",
        })
    }
}

impl Error for ToolError {}

impl ClassifiedError for ToolError {
    fn class(&self) -> ErrorClass {
        match self.kind {
            ToolErrorKind::InvalidCall | ToolErrorKind::OutputLimit | ToolErrorKind::TurnLimit => {
                ErrorClass::Validation
            }
            ToolErrorKind::MemoryProposalSinkRequired => ErrorClass::Internal,
            ToolErrorKind::PermissionDenied => ErrorClass::PermissionDenied,
            ToolErrorKind::Timeout => ErrorClass::Timeout,
            ToolErrorKind::Cancelled => ErrorClass::Cancelled,
            ToolErrorKind::Filesystem | ToolErrorKind::Artifact => ErrorClass::Storage,
            ToolErrorKind::Process | ToolErrorKind::Http => ErrorClass::Unavailable,
            ToolErrorKind::Internal => ErrorClass::Internal,
        }
    }

    fn retry_advice(&self) -> RetryAdvice {
        self.retry
    }
}

const fn error_code(kind: ToolErrorKind) -> &'static str {
    match kind {
        ToolErrorKind::InvalidCall => "invalid_tool_call",
        ToolErrorKind::MemoryProposalSinkRequired => "memory_proposal_sink_required",
        ToolErrorKind::PermissionDenied => "tool_permission_denied",
        ToolErrorKind::Filesystem => "tool_filesystem_failed",
        ToolErrorKind::Process => "tool_process_failed",
        ToolErrorKind::Http => "tool_http_failed",
        ToolErrorKind::Timeout => "tool_timeout",
        ToolErrorKind::Cancelled => "tool_cancelled",
        ToolErrorKind::OutputLimit => "tool_output_limit",
        ToolErrorKind::TurnLimit => "tool_turn_limit",
        ToolErrorKind::Artifact => "tool_artifact_failed",
        ToolErrorKind::Internal => "tool_internal",
    }
}
