use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{ModelId, ProviderId, ValueError};

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
            Some(value) => Some(Self(value)),
            None => None,
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
    fn deserialization_cannot_bypass_value_validation() {
        assert!(serde_json::from_str::<PromptText>(r#""   ""#).is_err());
        assert!(serde_json::from_str::<SessionSequence>("0").is_err());
    }
}
