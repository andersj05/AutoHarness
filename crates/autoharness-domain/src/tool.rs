use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{ArtifactId, ProviderCallId, ToolCallId, ToolName, ValueError};

/// The first provider-neutral schema for built-in tool definitions and calls.
pub const TOOL_SCHEMA_V1: u16 = 1;

/// Maximum serialized argument size admitted from a provider.
const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
/// Maximum durable inline output. Larger results use a content-addressed artifact.
pub const MAX_INLINE_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_RESOURCE_BYTES: usize = 4 * 1024;
const MAX_MEDIA_TYPE_BYTES: usize = 255;

/// A validated JSON object containing model-authored tool arguments.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolArguments(Map<String, Value>);

impl ToolArguments {
    /// Validates and admits one bounded JSON object.
    pub fn new(value: Value) -> Result<Self, ValueError> {
        let Value::Object(object) = value else {
            return Err(ValueError::InvalidToolArguments);
        };
        let encoded = serde_json::to_vec(&object).map_err(|_| ValueError::InvalidToolArguments)?;
        if encoded.len() > MAX_ARGUMENT_BYTES {
            return Err(ValueError::InvalidToolArguments);
        }
        Ok(Self(object))
    }

    /// Returns the admitted object for strict trusted-schema parsing.
    #[must_use]
    pub const fn as_object(&self) -> &Map<String, Value> {
        &self.0
    }

    /// Returns a JSON value suitable for provider request preparation.
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::Object(self.0.clone())
    }
}

impl Debug for ToolArguments {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolArguments")
            .field("content", &"[REDACTED]")
            .field(
                "bytes",
                &serde_json::to_vec(&self.0).map_or(0, |bytes| bytes.len()),
            )
            .finish()
    }
}

impl<'de> Deserialize<'de> for ToolArguments {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Capability classes understood by the policy engine and sandbox adapters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// No external authority because trusted planning rejected the model call.
    InvalidToolCall,
    /// Submit an untrusted memory candidate to the application-owned review sink.
    MemoryProposal,
    /// Read bytes from a workspace-confined path.
    FilesystemRead,
    /// Create or replace bytes at a workspace-confined path.
    FilesystemWrite,
    /// Spawn one executable without a shell.
    ProcessExecute,
    /// Issue one HTTP request to an exact origin.
    HttpRequest,
}

/// A bounded log-safe canonical resource selected by trusted tool planning.
#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ResourceRef(String);

impl ResourceRef {
    /// Creates a stable resource reference without retaining control characters.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.trim().is_empty()
            || value.len() > MAX_RESOURCE_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(ValueError::InvalidResource);
        }
        Ok(Self(value))
    }

    /// Returns the canonical policy resource.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ResourceRef {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Exact capability authority frozen before a permission decision is recorded.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityRequest {
    /// Capability class selected by trusted code.
    pub kind: CapabilityKind,
    /// Canonical resource within that class.
    pub resource: ResourceRef,
}

/// Policy result for an exact tool and resource.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOutcome {
    /// The call may not execute.
    Deny,
    /// A human answer is required before execution.
    Ask,
    /// The exact frozen call may execute once.
    Allow,
}

/// Human resolution of a prior `ask` outcome.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAnswer {
    /// Allow this exact call once.
    AllowOnce,
    /// Deny this exact call.
    Deny,
}

/// Model-authored call data paired with trusted derived authority or explicit no-authority rejection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolCallSpec {
    /// Stable local identity allocated before admission.
    pub tool_call_id: ToolCallId,
    /// Provider identity used when returning the result.
    pub provider_call_id: ProviderCallId,
    /// Bounded model-selected tool name.
    pub tool_name: ToolName,
    /// Exact supported schema version.
    pub schema_version: u16,
    /// Validated but still untrusted model arguments.
    pub arguments: ToolArguments,
    /// Exact capability or no-authority rejection derived by the trusted registry.
    pub capability: CapabilityRequest,
}

/// Immutable per-attempt agent-run limits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunLimits {
    /// Maximum provider turns, including the initial turn.
    pub max_turns: u32,
    /// Maximum elapsed monotonic runtime.
    pub max_time_ms: u64,
    /// Maximum cumulative provider tokens when reported.
    pub max_tokens: u64,
    /// Maximum provider text plus tool-output bytes.
    pub max_output_bytes: u64,
    /// Maximum concurrent external tool effects.
    pub max_concurrency: u32,
}

impl RunLimits {
    /// Creates limits only when every authority dimension is explicitly bounded.
    pub const fn new(
        max_turns: u32,
        max_time_ms: u64,
        max_tokens: u64,
        max_output_bytes: u64,
        max_concurrency: u32,
    ) -> Result<Self, ValueError> {
        if max_turns == 0
            || max_time_ms == 0
            || max_tokens == 0
            || max_output_bytes == 0
            || max_concurrency == 0
        {
            return Err(ValueError::InvalidRunLimits);
        }
        Ok(Self {
            max_turns,
            max_time_ms,
            max_tokens,
            max_output_bytes,
            max_concurrency,
        })
    }
}

impl Default for RunLimits {
    fn default() -> Self {
        Self::new(8, 10 * 60_000, 100_000, 1024 * 1024, 2).expect("static run limits are valid")
    }
}

/// Content-addressed metadata for output retained outside the event log.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactRef {
    /// Digest-derived artifact identity.
    pub artifact_id: ArtifactId,
    /// Full original byte length.
    pub byte_len: u64,
    /// Validated media type.
    pub media_type: String,
}

impl ArtifactRef {
    /// Constructs bounded artifact metadata.
    pub fn new(
        artifact_id: ArtifactId,
        byte_len: u64,
        media_type: impl Into<String>,
    ) -> Result<Self, ValueError> {
        let media_type = media_type.into();
        if byte_len == 0
            || media_type.is_empty()
            || media_type.len() > MAX_MEDIA_TYPE_BYTES
            || media_type.chars().any(char::is_control)
        {
            return Err(ValueError::InvalidArtifact);
        }
        Ok(Self {
            artifact_id,
            byte_len,
            media_type,
        })
    }
}

/// Bounded inline tool output with optional reference to the full artifact.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ToolOutput {
    content: String,
    artifact: Option<ArtifactRef>,
    original_bytes: u64,
    truncated: bool,
}

impl ToolOutput {
    /// Creates a validated output projection.
    pub fn new(
        content: impl Into<String>,
        artifact: Option<ArtifactRef>,
        original_bytes: u64,
        truncated: bool,
    ) -> Result<Self, ValueError> {
        let content = content.into();
        if content.len() > MAX_INLINE_TOOL_OUTPUT_BYTES
            || original_bytes < u64::try_from(content.len()).unwrap_or(u64::MAX)
            || truncated != artifact.is_some()
        {
            return Err(ValueError::ToolOutputTooLong);
        }
        Ok(Self {
            content,
            artifact,
            original_bytes,
            truncated,
        })
    }

    /// Returns content admitted into the next model turn.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns retained full-output metadata when inline content was truncated.
    #[must_use]
    pub const fn artifact(&self) -> Option<&ArtifactRef> {
        self.artifact.as_ref()
    }

    /// Returns the full pre-truncation byte count.
    #[must_use]
    pub const fn original_bytes(&self) -> u64 {
        self.original_bytes
    }

    /// Returns whether inline content is a prefix of a retained artifact.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

impl Debug for ToolOutput {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolOutput")
            .field("content", &"[REDACTED]")
            .field("inline_bytes", &self.content.len())
            .field("original_bytes", &self.original_bytes)
            .field("truncated", &self.truncated)
            .field("artifact", &self.artifact)
            .finish()
    }
}

impl<'de> Deserialize<'de> for ToolOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Serialized {
            content: String,
            artifact: Option<ArtifactRef>,
            original_bytes: u64,
            truncated: bool,
        }

        let value = Serialized::deserialize(deserializer)?;
        Self::new(
            value.content,
            value.artifact,
            value.original_bytes,
            value.truncated,
        )
        .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_require_a_bounded_object_and_redact_debug() {
        assert_eq!(
            ToolArguments::new(Value::String("no".to_owned())),
            Err(ValueError::InvalidToolArguments)
        );
        let arguments =
            ToolArguments::new(serde_json::json!({"secret": "sentinel"})).expect("valid arguments");
        assert!(!format!("{arguments:?}").contains("sentinel"));
    }

    #[test]
    fn every_run_dimension_must_be_bounded() {
        assert_eq!(
            RunLimits::new(0, 1, 1, 1, 1),
            Err(ValueError::InvalidRunLimits)
        );
    }
}
