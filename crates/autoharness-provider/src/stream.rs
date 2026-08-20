use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{ModelId, RetryAdvice};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{ProviderError, ProviderErrorKind};

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

/// One complete locally admitted conversational message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChatMessage {
    /// Speaker role.
    pub role: ChatRole,
    /// Exact admitted content.
    pub content: ChatContent,
}

/// A stateless provider request containing the complete admitted local history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChatRequest {
    /// Selected provider-owned model.
    pub model_id: ModelId,
    /// Complete local history, in provider-turn order.
    pub messages: Vec<ChatMessage>,
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
        }

        let request = SerializedRequest::deserialize(deserializer)?;
        Self::new(request.model_id, request.messages).map_err(D::Error::custom)
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
        Ok(Self { model_id, messages })
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
        let delta = TextDelta::new(sentinel).expect("valid delta");

        assert!(!format!("{content:?}").contains(sentinel));
        assert!(!format!("{delta:?}").contains(sentinel));
    }

    #[test]
    fn deserialization_cannot_bypass_message_validation() {
        assert!(serde_json::from_str::<ChatContent>(r#""""#).is_err());
        assert!(serde_json::from_str::<TextDelta>(r#""""#).is_err());
        assert!(
            serde_json::from_str::<ChatRequest>(
                r#"{"model_id":"models/gemini-test","messages":[]}"#
            )
            .is_err()
        );
    }
}
