use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::ValidationError;
use crate::bounds::{
    MAX_PROMPT_BYTES, MAX_SESSION_TITLE_BYTES, MAX_TRANSCRIPT_CONTENT_BYTES,
    validate_non_empty_text, validate_text,
};

/// Exact user-authored prompt admitted through a public command.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PromptContent(String);

impl PromptContent {
    /// Constructs a non-blank bounded prompt while preserving exact text.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_non_empty_text("prompt", &value, MAX_PROMPT_BYTES)?;
        Ok(Self(value))
    }

    /// Returns exact prompt text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns exact prompt text.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Debug for PromptContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PromptContent")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for PromptContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Exact durable transcript content supplied by the Rust authority.
///
/// Empty assistant content is valid while an attempt starts or settles without text.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TranscriptContent(String);

impl TranscriptContent {
    /// Constructs bounded transcript content while preserving exact text.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_text("transcript_content", &value, MAX_TRANSCRIPT_CONTENT_BYTES)?;
        Ok(Self(value))
    }

    /// Returns exact transcript content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for TranscriptContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranscriptContent")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for TranscriptContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Bounded non-empty human-readable durable session title.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SessionTitle(String);

impl SessionTitle {
    /// Constructs a bounded title while preserving exact text.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_non_empty_text("session_title", &value, MAX_SESSION_TITLE_BYTES)?;
        Ok(Self(value))
    }

    /// Returns exact title text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SessionTitle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}
