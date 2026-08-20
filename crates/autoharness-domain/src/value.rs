use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{ErrorClass, ErrorCode, ModelId, ProviderId, RetryAdvice, ValueError};

/// A provider and model selection kept independent of provider-native payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelRef {
    provider_id: ProviderId,
    model_id: ModelId,
}

impl ModelRef {
    /// Creates a provider-neutral model reference.
    #[must_use]
    pub const fn new(provider_id: ProviderId, model_id: ModelId) -> Self {
        Self {
            provider_id,
            model_id,
        }
    }

    /// Returns the selected provider.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Returns the provider-owned model identity.
    #[must_use]
    pub const fn model_id(&self) -> &ModelId {
        &self.model_id
    }
}

/// Exact user-authored prompt content.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PromptText(String);

impl PromptText {
    /// Validates a non-empty prompt while preserving its original bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ValueError::EmptyPrompt);
        }

        Ok(Self(value))
    }

    /// Returns the exact prompt content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for PromptText {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PromptText")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for PromptText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Exact provider-authored response content.
///
/// Whitespace-only deltas are valid because they can be meaningful when
/// concatenated with adjacent deltas.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ResponseText(String);

impl ResponseText {
    /// Validates a non-empty response delta while preserving its original bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ValueError::EmptyResponseText);
        }

        Ok(Self(value))
    }

    /// Returns the exact response content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ResponseText {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseText")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for ResponseText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A bounded message explicitly classified as safe for user presentation.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PublicMessage(String);

impl PublicMessage {
    /// Maximum persisted byte length for a public failure message.
    pub const MAX_BYTES: usize = 4_096;

    /// Constructs a non-empty bounded message.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ValueError::EmptyPublicMessage);
        }
        if value.len() > Self::MAX_BYTES {
            return Err(ValueError::PublicMessageTooLong);
        }

        Ok(Self(value))
    }

    /// Returns the user-presentable message.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for PublicMessage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublicMessage")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for PublicMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Provider-neutral, cumulative token accounting for one attempt.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageSnapshot {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_tokens: Option<u64>,
}

impl UsageSnapshot {
    /// Creates a cumulative usage snapshot.
    #[must_use]
    pub const fn new(
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens,
            cached_input_tokens: None,
            reasoning_tokens: None,
            tool_tokens: None,
        }
    }

    /// Adds provider-reported cumulative cached, reasoning, and tool-use tokens.
    #[must_use]
    pub const fn with_breakdown(
        mut self,
        cached_input_tokens: Option<u64>,
        reasoning_tokens: Option<u64>,
        tool_tokens: Option<u64>,
    ) -> Self {
        self.cached_input_tokens = cached_input_tokens;
        self.reasoning_tokens = reasoning_tokens;
        self.tool_tokens = tool_tokens;
        self
    }

    /// Returns cumulative input tokens when reported.
    #[must_use]
    pub const fn input_tokens(self) -> Option<u64> {
        self.input_tokens
    }

    /// Returns cumulative model-output tokens when reported.
    #[must_use]
    pub const fn output_tokens(self) -> Option<u64> {
        self.output_tokens
    }

    /// Returns cumulative total tokens when reported.
    #[must_use]
    pub const fn total_tokens(self) -> Option<u64> {
        self.total_tokens
    }

    /// Returns cumulative cached input tokens when reported.
    #[must_use]
    pub const fn cached_input_tokens(self) -> Option<u64> {
        self.cached_input_tokens
    }

    /// Returns cumulative reasoning tokens when reported.
    #[must_use]
    pub const fn reasoning_tokens(self) -> Option<u64> {
        self.reasoning_tokens
    }

    /// Returns cumulative tool-use tokens when reported.
    #[must_use]
    pub const fn tool_tokens(self) -> Option<u64> {
        self.tool_tokens
    }
}

/// Sanitized failure data that may cross the durable event boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttemptFailure {
    class: ErrorClass,
    code: ErrorCode,
    message: PublicMessage,
    retry_advice: RetryAdvice,
}

impl AttemptFailure {
    /// Constructs a provider-neutral safe failure projection.
    #[must_use]
    pub const fn new(
        class: ErrorClass,
        code: ErrorCode,
        message: PublicMessage,
        retry_advice: RetryAdvice,
    ) -> Self {
        Self {
            class,
            code,
            message,
            retry_advice,
        }
    }

    /// Returns the stable failure category.
    #[must_use]
    pub const fn class(&self) -> ErrorClass {
        self.class
    }

    /// Returns the stable public code.
    #[must_use]
    pub const fn code(&self) -> &ErrorCode {
        &self.code
    }

    /// Returns the safe user-presentable message.
    #[must_use]
    pub const fn message(&self) -> &PublicMessage {
        &self.message
    }

    /// Returns the retry policy captured at failure time.
    #[must_use]
    pub const fn retry_advice(&self) -> RetryAdvice {
        self.retry_advice
    }
}

/// Determines when durable input becomes eligible for a provider turn.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Make the input eligible at the next provider-turn boundary.
    #[default]
    NextTurn,
}

/// A one-based sequence within a session event stream.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SessionSequence(u64);

impl SessionSequence {
    /// The first valid sequence in a session event stream.
    pub const FIRST: Self = Self(1);

    /// Validates and constructs a sequence.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::ZeroSequence);
        }
        if value > i64::MAX as u64 {
            return Err(ValueError::SequenceTooLarge);
        }

        Ok(Self(value))
    }

    /// Returns the underlying one-based value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next sequence, or `None` at integer exhaustion.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) if value <= i64::MAX as u64 => Some(Self(value)),
            Some(_) | None => None,
        }
    }
}

impl<'de> Deserialize<'de> for SessionSequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Milliseconds relative to the Unix epoch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TimestampMillis(i64);

impl TimestampMillis {
    /// Constructs a timestamp from Unix epoch milliseconds.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns Unix epoch milliseconds.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_preserves_multiline_unicode_exactly() {
        let original = "  first line\nsecond line: こんにちは  ";
        let prompt = PromptText::new(original).expect("non-empty prompt");

        assert_eq!(prompt.as_str(), original);
    }

    #[test]
    fn prompt_debug_does_not_reveal_content() {
        let prompt = PromptText::new("sensitive transcript content").expect("non-empty prompt");
        let rendered = format!("{prompt:?}");

        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("sensitive transcript content"));
    }

    #[test]
    fn prompt_rejects_whitespace_only() {
        assert_eq!(PromptText::new(" \n\t "), Err(ValueError::EmptyPrompt));
    }

    #[test]
    fn response_text_preserves_whitespace_but_rejects_empty_content() {
        let response = ResponseText::new(" \n").expect("whitespace delta is meaningful");

        assert_eq!(response.as_str(), " \n");
        assert_eq!(ResponseText::new(""), Err(ValueError::EmptyResponseText));
        assert!(!format!("{response:?}").contains(" \n"));
    }

    #[test]
    fn public_messages_are_bounded_and_redacted_in_debug_output() {
        let message = PublicMessage::new("Safe explanation").expect("valid message");

        assert_eq!(message.as_str(), "Safe explanation");
        assert!(!format!("{message:?}").contains("Safe explanation"));
        assert_eq!(
            PublicMessage::new("x".repeat(PublicMessage::MAX_BYTES + 1)),
            Err(ValueError::PublicMessageTooLong)
        );
    }

    #[test]
    fn sequence_stays_within_signed_storage_range() {
        assert!(SessionSequence::new(i64::MAX as u64).is_ok());
        assert_eq!(
            SessionSequence::new(i64::MAX as u64 + 1),
            Err(ValueError::SequenceTooLarge)
        );
        assert_eq!(
            SessionSequence::new(i64::MAX as u64)
                .expect("maximum sequence")
                .checked_next(),
            None
        );
    }

    #[test]
    fn deserialization_cannot_bypass_value_validation() {
        assert!(serde_json::from_str::<PromptText>(r#""   ""#).is_err());
        assert!(serde_json::from_str::<SessionSequence>("0").is_err());
    }
}
