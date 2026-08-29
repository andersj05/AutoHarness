use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{
    ModelId, ProviderCallId, RetryAdvice, TOOL_SCHEMA_V1, ToolArguments, ToolName,
};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{ProviderError, ProviderErrorKind};

/// Maximum provider-neutral instruction prelude retained in one request.
pub const MAX_CONTEXT_PRELUDE_BYTES: usize = 256 * 1024;

/// Provider-neutral conversational role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// Human-authored input.
    User,
    /// Model-authored output admitted into local history.
    Assistant,
}

/// Conversational content whose debug form never exposes transcript text.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ChatContent(String);

impl ChatContent {
    /// Constructs non-empty message content while preserving exact text.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::InvalidRequest,
                RetryAdvice::Never,
            ));
        }
        Ok(Self(value))
    }

    /// Returns exact message text for adapter request preparation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ChatContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChatContent")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ChatContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Exact provider-neutral context rendered before conversational history.
///
/// The application classifies and delimits authorized instructions and inert
/// memory data before constructing this value.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContextPrelude(String);

impl ContextPrelude {
    /// Constructs a bounded non-empty context prelude.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        if value.trim().is_empty() || value.len() > MAX_CONTEXT_PRELUDE_BYTES {
            return Err(ProviderError::new(
                ProviderErrorKind::InvalidRequest,
                RetryAdvice::Never,
            ));
        }
        Ok(Self(value))
    }

    /// Returns the exact prelude for provider-native request preparation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ContextPrelude {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextPrelude")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ContextPrelude {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// One complete locally admitted conversational or tool-loop item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum ChatMessage {
    /// Human- or model-authored text.
    Text {
        /// Speaker role.
        role: ChatRole,
        /// Exact admitted content.
        content: ChatContent,
    },
    /// A durable model-authored function call.
    ToolCall(ProviderToolCall),
    /// A durable locally produced function result.
    ToolResult {
        /// Provider call identity being answered.
        provider_call_id: ProviderCallId,
        /// Registered tool name.
        tool_name: ToolName,
        /// Bounded result or safe failure text.
        content: ChatContent,
    },
}

impl ChatMessage {
    /// Constructs a text message.
    #[must_use]
    pub const fn text(role: ChatRole, content: ChatContent) -> Self {
        Self::Text { role, content }
    }

    /// Returns text content for text-only compatibility consumers.
    #[must_use]
    pub const fn content(&self) -> Option<&ChatContent> {
        match self {
            Self::Text { content, .. } | Self::ToolResult { content, .. } => Some(content),
            Self::ToolCall(_) => None,
        }
    }
}

/// Provider-neutral complete function call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderToolCall {
    /// Provider identity used by a later result message.
    pub provider_call_id: ProviderCallId,
    /// Registered tool name.
    pub tool_name: ToolName,
    /// Validated bounded JSON-object arguments.
    pub arguments: ToolArguments,
}

/// Versioned function schema exposed to a provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderToolDefinition {
    /// Registered name.
    pub name: ToolName,
    /// Bounded human-readable description.
    pub description: String,
    /// Exact supported schema version.
    pub schema_version: u16,
    /// JSON Schema object.
    pub parameters: serde_json::Value,
}

impl<'de> Deserialize<'de> for ProviderToolDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedDefinition {
            name: ToolName,
            description: String,
            schema_version: u16,
            parameters: serde_json::Value,
        }

        let definition = SerializedDefinition::deserialize(deserializer)?;
        if definition.schema_version != TOOL_SCHEMA_V1 {
            return Err(D::Error::custom("unsupported tool schema version"));
        }
        Self::new_v1(
            definition.name,
            definition.description,
            definition.parameters,
        )
        .map_err(D::Error::custom)
    }
}

impl ProviderToolDefinition {
    /// Constructs a v1 definition with a bounded object schema.
    pub fn new_v1(
        name: ToolName,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Result<Self, ProviderError> {
        let description = description.into();
        if description.trim().is_empty()
            || description.len() > 4_096
            || !parameters.is_object()
            || serde_json::to_vec(&parameters).map_or(true, |bytes| bytes.len() > 64 * 1024)
        {
            return Err(ProviderError::new(
                ProviderErrorKind::InvalidRequest,
                RetryAdvice::Never,
            ));
        }
        Ok(Self {
            name,
            description,
            schema_version: TOOL_SCHEMA_V1,
            parameters,
        })
    }
}

/// A stateless provider request containing the complete admitted local history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChatRequest {
    /// Selected provider-owned model.
    pub model_id: ModelId,
    /// Complete local history, in provider-turn order.
    pub messages: Vec<ChatMessage>,
    /// Exact provider-neutral context rendered outside conversational history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextPrelude>,
    /// Exact registered tools available for this provider turn.
    pub tools: Vec<ProviderToolDefinition>,
}

impl<'de> Deserialize<'de> for ChatRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedRequest {
            model_id: ModelId,
            messages: Vec<ChatMessage>,
            #[serde(default)]
            context: Option<ContextPrelude>,
            #[serde(default)]
            tools: Vec<ProviderToolDefinition>,
        }

        let request = SerializedRequest::deserialize(deserializer)?;
        Self::new(request.model_id, request.messages)
            .map(|request_value| {
                let request_value = match request.context {
                    Some(context) => request_value.with_context(context),
                    None => request_value,
                };
                request_value.with_tools(request.tools)
            })
            .map_err(D::Error::custom)
    }
}

impl ChatRequest {
    /// Validates a request with at least one message.
    pub fn new(model_id: ModelId, messages: Vec<ChatMessage>) -> Result<Self, ProviderError> {
        if messages.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::InvalidRequest,
                RetryAdvice::Never,
            ));
        }
        Ok(Self {
            model_id,
            messages,
            context: None,
            tools: Vec::new(),
        })
    }

    /// Adds the exact classified context prelude for this provider turn.
    #[must_use]
    pub fn with_context(mut self, context: ContextPrelude) -> Self {
        self.context = Some(context);
        self
    }

    /// Adds the exact trusted tool registry for this request.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<ProviderToolDefinition>) -> Self {
        self.tools = tools;
        self
    }
}

/// A streamed text fragment whose debug form never exposes generated content.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TextDelta(String);

impl TextDelta {
    /// Constructs a non-empty normalized text delta.
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                RetryAdvice::Never,
            ));
        }
        Ok(Self(value))
    }

    /// Returns the exact generated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for TextDelta {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextDelta")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for TextDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A cumulative provider usage snapshot. Consumers replace older snapshots and
/// must not sum successive values.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageSnapshot {
    /// Input tokens charged or counted by the provider.
    pub input_tokens: Option<u64>,
    /// Generated output tokens, excluding separately reported thinking tokens.
    pub output_tokens: Option<u64>,
    /// Cached input tokens included in provider usage.
    pub cached_input_tokens: Option<u64>,
    /// Thinking or reasoning tokens reported separately.
    pub reasoning_tokens: Option<u64>,
    /// Tool-use tokens reported separately.
    pub tool_tokens: Option<u64>,
    /// Provider-calculated total. It is never derived by the adapter.
    pub total_tokens: Option<u64>,
}

/// Provider-neutral terminal completion reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionReason {
    /// Normal model stop.
    Stop,
    /// The provider reached a generation limit.
    Length,
    /// Safety policy stopped generation.
    Safety,
    /// Recitation policy stopped generation.
    Recitation,
    /// The provider completed with another non-transient reason.
    Other,
    /// The provider turn ended with one or more function calls.
    ToolCalls,
}

/// Normalized lifecycle events emitted by every streaming adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum ProviderStreamEvent {
    /// The provider accepted the request and returned a stream response.
    Started,
    /// A generated text fragment.
    TextDelta(TextDelta),
    /// The latest cumulative usage snapshot.
    Usage(UsageSnapshot),
    /// One complete normalized function call.
    ToolCall(ProviderToolCall),
    /// Successful or policy-stopped terminal completion.
    Completed {
        /// Provider-neutral completion reason.
        reason: CompletionReason,
    },
    /// User-requested cancellation won the terminal-state race.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_debug_forms_are_redacted() {
        let sentinel = "secret transcript sentinel";
        let content = ChatContent::new(sentinel).expect("valid content");
        let context = ContextPrelude::new(sentinel).expect("valid context");
        let delta = TextDelta::new(sentinel).expect("valid delta");

        assert!(!format!("{content:?}").contains(sentinel));
        assert!(!format!("{context:?}").contains(sentinel));
        assert!(!format!("{delta:?}").contains(sentinel));
    }

    #[test]
    fn context_preludes_are_non_empty_and_bounded() {
        assert!(ContextPrelude::new(" ").is_err());
        assert!(ContextPrelude::new("x".repeat(MAX_CONTEXT_PRELUDE_BYTES)).is_ok());
        assert!(ContextPrelude::new("x".repeat(MAX_CONTEXT_PRELUDE_BYTES + 1)).is_err());
    }

    #[test]
    fn deserialization_cannot_bypass_message_validation() {
        assert!(serde_json::from_str::<ChatContent>(r#""""#).is_err());
        assert!(serde_json::from_str::<ContextPrelude>(r#""""#).is_err());
        assert!(serde_json::from_str::<TextDelta>(r#""""#).is_err());
        assert!(
            serde_json::from_str::<ChatRequest>(
                r#"{"model_id":"models/gemini-test","messages":[]}"#
            )
            .is_err()
        );
    }
}
