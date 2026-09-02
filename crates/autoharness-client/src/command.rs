use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroizing;

use crate::bounds::validate_credential;
use crate::{
    AttemptId, CLIENT_SCHEMA_VERSION, ConnectionId, ModelRef, PromptContent, RequestId,
    SafeFailure, SessionId, SessionTitle, ToolCallId, TransportRevision, ValidationError,
};

/// Exact user decision for one frozen durable permission request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    Deny,
}

/// Serializable renderer-neutral command intent.
///
/// Request identities are issued by Rust after bounded mailbox admission.
/// Credential ingress intentionally has no variant in this enum.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ClientCommand {
    CreateSession,
    OpenSession {
        session_id: SessionId,
    },
    RenameSession {
        session_id: SessionId,
        title: SessionTitle,
    },
    ArchiveSession {
        session_id: SessionId,
    },
    UnarchiveSession {
        session_id: SessionId,
    },
    ExportTranscript {
        session_id: SessionId,
    },
    DeleteSession {
        session_id: SessionId,
    },
    RefreshCatalog,
    SelectModel {
        session_id: SessionId,
        model: ModelRef,
    },
    SubmitPrompt {
        session_id: SessionId,
        prompt: PromptContent,
    },
    CancelAttempt {
        session_id: SessionId,
        attempt_id: AttemptId,
    },
    RetryAttempt {
        session_id: SessionId,
        attempt_id: AttemptId,
    },
    AnswerPermission {
        session_id: SessionId,
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
    },
    RequestResynchronization {
        last_applied_revision: Option<TransportRevision>,
    },
    RequestShutdown,
}

/// Versioned command envelope accepted by any physical carrier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandEnvelope {
    pub schema_version: u16,
    pub command: ClientCommand,
}

impl CommandEnvelope {
    #[must_use]
    pub const fn new(command: ClientCommand) -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            command,
        }
    }
}

impl<'de> Deserialize<'de> for CommandEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireCommand {
            schema_version: u16,
            command: ClientCommand,
        }
        let wire = WireCommand::deserialize(deserializer)?;
        if wire.schema_version != CLIENT_SCHEMA_VERSION {
            return Err(D::Error::custom(
                "unsupported client command schema version",
            ));
        }
        Ok(Self::new(wire.command))
    }
}

/// Immediate response proving bounded host-mailbox admission.
///
/// This is not proof of a durable commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CommandReceipt {
    pub schema_version: u16,
    pub request_id: RequestId,
}

impl CommandReceipt {
    #[must_use]
    pub const fn new(request_id: RequestId) -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            request_id,
        }
    }
}

impl<'de> Deserialize<'de> for CommandReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireReceipt {
            schema_version: u16,
            request_id: RequestId,
        }
        let wire = WireReceipt::deserialize(deserializer)?;
        if wire.schema_version != CLIENT_SCHEMA_VERSION {
            return Err(D::Error::custom(
                "unsupported command receipt schema version",
            ));
        }
        Ok(Self::new(wire.request_id))
    }
}

/// Dedicated one-way credential ingress.
///
/// This type deliberately implements neither `Serialize` nor `Deserialize`.
pub struct SecretIngress {
    connection_id: ConnectionId,
    credential: Zeroizing<String>,
}

impl SecretIngress {
    /// Takes ownership of one nonempty visible-ASCII credential of at most 4096 bytes.
    pub fn new(
        connection_id: ConnectionId,
        credential: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let credential = Zeroizing::new(credential.into());
        validate_credential(credential.as_str())?;
        Ok(Self {
            connection_id,
            credential,
        })
    }

    /// Returns the named connection that will receive the credential.
    #[must_use]
    pub const fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    /// Borrows credential text for immediate transfer into the Rust runtime.
    #[must_use]
    pub fn credential(&self) -> &str {
        self.credential.as_str()
    }

    /// Consumes ingress and transfers owned zeroizing storage.
    #[must_use]
    pub fn into_credential(self) -> Zeroizing<String> {
        self.credential
    }
}

impl Debug for SecretIngress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretIngress")
            .field("connection_id", &self.connection_id)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

/// Safe authentication lifecycle notice.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationState {
    BrowserOpened,
    Completed,
    Cancelled,
}

/// Safe shutdown lifecycle notice.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownState {
    Requested,
    Ready,
}

/// Correlated application notice or process lifecycle signal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ClientNotice {
    CommandCommitted {
        request_id: RequestId,
    },
    CommandRejected {
        request_id: RequestId,
        failure: SafeFailure,
    },
    Authentication {
        request_id: RequestId,
        state: AuthenticationState,
    },
    Shutdown {
        state: ShutdownState,
    },
}

impl ClientNotice {
    /// Returns the correlated request identity when the notice is request-scoped.
    #[must_use]
    pub const fn request_id(&self) -> Option<RequestId> {
        match self {
            Self::CommandCommitted { request_id }
            | Self::CommandRejected { request_id, .. }
            | Self::Authentication { request_id, .. } => Some(*request_id),
            Self::Shutdown { .. } => None,
        }
    }
}
