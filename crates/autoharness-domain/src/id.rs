use std::fmt::{self, Display, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::ValueError;

const MAX_IDENTIFIER_BYTES: usize = 512;

macro_rules! identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Constructs an identifier of at most 512 bytes using `A-Z`, `a-z`,
            /// `0-9`, or one of `-_.:/@+%~`.
            pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier's stable string representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ValueError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

identifier!(SessionId, "Stable identity for a conversation session.");
identifier!(CommandId, "Stable identity for a requested command.");
identifier!(EventId, "Stable identity for one durable event.");
identifier!(InputId, "Stable identity for one admitted user input.");
identifier!(AttemptId, "Stable identity for one provider attempt.");
identifier!(
    ToolCallId,
    "Stable local identity for one durable tool call."
);
identifier!(
    ProviderCallId,
    "Provider-owned tool-call identity retained for continuation."
);
identifier!(
    PermissionDecisionId,
    "Stable identity for one durable capability-policy decision."
);
identifier!(
    ArtifactId,
    "Content-addressed identity for one retained artifact."
);
identifier!(ToolName, "Stable versioned tool name exposed to models.");
identifier!(
    CorrelationId,
    "Identity shared by commands and events in one logical operation."
);
identifier!(ProviderId, "Stable provider adapter identity.");
identifier!(ModelId, "Provider-owned model identity.");
identifier!(MemoryId, "Stable identity for one durable memory item.");
identifier!(
    MemorySubjectKey,
    "Optional stable semantic key used for exact memory contradiction grouping."
);
identifier!(
    MemoryRevisionId,
    "Stable identity for one immutable memory revision."
);
identifier!(
    MemoryOperationId,
    "Stable identity for one durable memory-ledger operation."
);
identifier!(
    MemoryEvidenceId,
    "Stable identity for one evidence record attached to memory."
);
identifier!(
    ContextEpochId,
    "Stable identity for one immutable context epoch."
);
identifier!(
    ContextTurnId,
    "Stable identity for one provider-turn context manifest."
);
identifier!(
    ContextAdmissionId,
    "Stable identity for one context admission decision."
);
identifier!(
    ContextSourceKey,
    "Stable key for one deterministic context source."
);
identifier!(UserId, "Stable opaque identity for one user scope.");
identifier!(
    WorkspaceId,
    "Stable opaque identity for one workspace scope."
);
identifier!(AgentId, "Stable identity for one configured agent scope.");
identifier!(
    ErrorCode,
    "Stable public error code safe for logs and storage."
);

fn validate_identifier(value: &str) -> Result<(), ValueError> {
    if value.is_empty() {
        return Err(ValueError::EmptyIdentifier);
    }
    if value.chars().any(char::is_control) {
        return Err(ValueError::IdentifierContainsControlCharacter);
    }
    if value.chars().any(char::is_whitespace) {
        return Err(ValueError::IdentifierContainsWhitespace);
    }
    if !value.chars().all(is_safe_identifier_character) {
        return Err(ValueError::IdentifierContainsUnsafeCharacter);
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(ValueError::IdentifierTooLong);
    }

    Ok(())
}

fn is_safe_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '-' | '_' | '.' | ':' | '/' | '@' | '+' | '%' | '~'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_log_unsafe_values() {
        assert_eq!(SessionId::new(""), Err(ValueError::EmptyIdentifier));
        assert_eq!(
            SessionId::new(" session-1"),
            Err(ValueError::IdentifierContainsWhitespace)
        );
        assert_eq!(
            SessionId::new("session\n1"),
            Err(ValueError::IdentifierContainsControlCharacter)
        );
        assert_eq!(
            SessionId::new("session\u{2028}forged"),
            Err(ValueError::IdentifierContainsWhitespace)
        );
        assert_eq!(
            SessionId::new("session\u{202e}forged"),
            Err(ValueError::IdentifierContainsUnsafeCharacter)
        );
        assert_eq!(
            SessionId::new("session\u{2066}forged"),
            Err(ValueError::IdentifierContainsUnsafeCharacter)
        );
        assert_eq!(
            SessionId::new("session\u{fff9}forged"),
            Err(ValueError::IdentifierContainsUnsafeCharacter)
        );
        assert_eq!(
            SessionId::new("session\u{e0001}forged"),
            Err(ValueError::IdentifierContainsUnsafeCharacter)
        );
    }

    #[test]
    fn model_ids_allow_provider_paths() {
        let model = ModelId::new("models/gemini-2.5-pro").expect("valid model ID");

        assert_eq!(model.as_str(), "models/gemini-2.5-pro");
    }

    #[test]
    fn deserialization_cannot_bypass_identifier_validation() {
        let result = serde_json::from_str::<EventId>(r#""""#);

        assert!(result.is_err());
    }

    #[test]
    fn identifier_length_limit_is_an_explicit_utf8_byte_boundary() {
        let maximum = "a".repeat(MAX_IDENTIFIER_BYTES);
        let too_long = "a".repeat(MAX_IDENTIFIER_BYTES + 1);

        assert!(EventId::new(maximum).is_ok());
        assert_eq!(EventId::new(too_long), Err(ValueError::IdentifierTooLong));
    }
}
