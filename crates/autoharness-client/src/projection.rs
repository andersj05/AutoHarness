use std::collections::BTreeSet;
use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::bounds::{
    MAX_CATALOG_MODELS, MAX_DETAIL_BYTES, MAX_LABEL_BYTES, MAX_PERMISSION_DETAILS,
    MAX_PERMISSION_REQUESTS, MAX_PROVIDERS, MAX_SESSIONS, MAX_TRANSCRIPT_ITEMS, validate_count,
    validate_non_empty_text, validate_text,
};
use crate::{
    AttemptId, CLIENT_SCHEMA_VERSION, ConnectionId, DecimalU64, InputId, ModelId, ProviderId,
    SafeFailure, SessionId, SessionRevision, SessionTitle, ToolCallId, TranscriptContent,
    UnixMillis, ValidationError,
};

/// Stable provider and provider-owned model identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    pub provider_id: ProviderId,
    pub model_id: ModelId,
}

impl ModelRef {
    #[must_use]
    pub const fn new(provider_id: ProviderId, model_id: ModelId) -> Self {
        Self {
            provider_id,
            model_id,
        }
    }
}

/// Cumulative usage for one provider attempt.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsageProjection {
    pub input_tokens: Option<DecimalU64>,
    pub output_tokens: Option<DecimalU64>,
    pub cached_input_tokens: Option<DecimalU64>,
    pub reasoning_tokens: Option<DecimalU64>,
    pub tool_tokens: Option<DecimalU64>,
    pub total_tokens: Option<DecimalU64>,
}

/// Current lifecycle of one durable model attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum AttemptState {
    Streaming,
    Cancelling,
    Completed,
    Cancelled,
    Failed { failure: SafeFailure },
}

/// Current lifecycle of one durable tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ToolCallState {
    Proposed,
    PermissionPending,
    Authorized,
    Denying,
    Running,
    Completed,
    Failed { failure: SafeFailure },
    Denied,
    Cancelled,
    Unknown,
}

/// One durable tool-call row projected without executable authority.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ToolCallProjection {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub capability: String,
    pub resource: String,
    pub state: ToolCallState,
    pub summary: Option<String>,
}

impl ToolCallProjection {
    pub fn new(
        tool_call_id: ToolCallId,
        tool_name: impl Into<String>,
        capability: impl Into<String>,
        resource: impl Into<String>,
        state: ToolCallState,
        summary: Option<String>,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            tool_call_id,
            tool_name: tool_name.into(),
            capability: capability.into(),
            resource: resource.into(),
            state,
            summary,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty_text("tool_name", &self.tool_name, MAX_LABEL_BYTES)?;
        validate_non_empty_text("tool_capability", &self.capability, MAX_LABEL_BYTES)?;
        validate_non_empty_text("tool_resource", &self.resource, MAX_DETAIL_BYTES)?;
        if let Some(summary) = &self.summary {
            validate_text("tool_summary", summary, MAX_DETAIL_BYTES)?;
        }
        Ok(())
    }
}

impl Debug for ToolCallProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolCallProjection")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("capability", &self.capability)
            .field("resource", &"[REDACTED]")
            .field("state", &self.state)
            .field("summary", &self.summary.as_ref().map(|value| value.len()))
            .finish()
    }
}

impl<'de> Deserialize<'de> for ToolCallProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireToolCall {
            tool_call_id: ToolCallId,
            tool_name: String,
            capability: String,
            resource: String,
            state: ToolCallState,
            summary: Option<String>,
        }
        let wire = WireToolCall::deserialize(deserializer)?;
        Self::new(
            wire.tool_call_id,
            wire.tool_name,
            wire.capability,
            wire.resource,
            wire.state,
            wire.summary,
        )
        .map_err(D::Error::custom)
    }
}

/// One provider-neutral durable transcript row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum TranscriptItem {
    User {
        input_id: InputId,
        content: TranscriptContent,
    },
    Assistant {
        attempt_id: AttemptId,
        content: TranscriptContent,
        state: AttemptState,
        usage: Option<UsageProjection>,
        retry_of: Option<AttemptId>,
    },
    Tool(ToolCallProjection),
}

/// One exact trusted field shown in a permission decision surface.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct PermissionDetail {
    pub label: String,
    pub value: String,
}

impl PermissionDetail {
    pub fn new(
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            label: label.into(),
            value: value.into(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty_text("permission_detail_label", &self.label, MAX_LABEL_BYTES)?;
        validate_text("permission_detail_value", &self.value, MAX_DETAIL_BYTES)
    }
}

impl Debug for PermissionDetail {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("PermissionDetail([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for PermissionDetail {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireDetail {
            label: String,
            value: String,
        }
        let wire = WireDetail::deserialize(deserializer)?;
        Self::new(wire.label, wire.value).map_err(D::Error::custom)
    }
}

/// One unresolved durable permission request that must preempt ordinary UI.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct PermissionRequest {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub capability: String,
    pub resource: String,
    pub details: Vec<PermissionDetail>,
}

impl PermissionRequest {
    pub fn new(
        tool_call_id: ToolCallId,
        tool_name: impl Into<String>,
        capability: impl Into<String>,
        resource: impl Into<String>,
        details: Vec<PermissionDetail>,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            tool_call_id,
            tool_name: tool_name.into(),
            capability: capability.into(),
            resource: resource.into(),
            details,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty_text("permission_tool_name", &self.tool_name, MAX_LABEL_BYTES)?;
        validate_non_empty_text("permission_capability", &self.capability, MAX_LABEL_BYTES)?;
        validate_non_empty_text("permission_resource", &self.resource, MAX_DETAIL_BYTES)?;
        validate_count(
            "permission_details",
            self.details.len(),
            MAX_PERMISSION_DETAILS,
        )?;
        self.details.iter().try_for_each(PermissionDetail::validate)
    }
}

impl Debug for PermissionRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionRequest")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("capability", &self.capability)
            .field("resource", &"[REDACTED]")
            .field("details", &"[REDACTED]")
            .finish()
    }
}

impl<'de> Deserialize<'de> for PermissionRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePermission {
            tool_call_id: ToolCallId,
            tool_name: String,
            capability: String,
            resource: String,
            details: Vec<PermissionDetail>,
        }
        let wire = WirePermission::deserialize(deserializer)?;
        Self::new(
            wire.tool_call_id,
            wire.tool_name,
            wire.capability,
            wire.resource,
            wire.details,
        )
        .map_err(D::Error::custom)
    }
}

/// Complete active-session projection derived from durable events.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionProjection {
    pub session_id: SessionId,
    pub revision: SessionRevision,
    pub selected_model: Option<ModelRef>,
    pub transcript: Vec<TranscriptItem>,
    pub permission_requests: Vec<PermissionRequest>,
}

impl SessionProjection {
    pub fn new(
        session_id: SessionId,
        revision: SessionRevision,
        selected_model: Option<ModelRef>,
        transcript: Vec<TranscriptItem>,
        permission_requests: Vec<PermissionRequest>,
    ) -> Result<Self, ValidationError> {
        validate_count("transcript", transcript.len(), MAX_TRANSCRIPT_ITEMS)?;
        validate_count(
            "permission_requests",
            permission_requests.len(),
            MAX_PERMISSION_REQUESTS,
        )?;
        let mut pending = BTreeSet::new();
        for permission in &permission_requests {
            permission.validate()?;
            if !pending.insert(permission.tool_call_id.as_str()) {
                return Err(ValidationError::Inconsistent {
                    field: "permission_requests",
                });
            }
        }
        Ok(Self {
            session_id,
            revision,
            selected_model,
            transcript,
            permission_requests,
        })
    }
}

impl<'de> Deserialize<'de> for SessionProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSession {
            session_id: SessionId,
            revision: SessionRevision,
            selected_model: Option<ModelRef>,
            transcript: Vec<TranscriptItem>,
            permission_requests: Vec<PermissionRequest>,
        }
        let wire = WireSession::deserialize(deserializer)?;
        Self::new(
            wire.session_id,
            wire.revision,
            wire.selected_model,
            wire.transcript,
            wire.permission_requests,
        )
        .map_err(D::Error::custom)
    }
}

/// One row in the complete durable session list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub title: SessionTitle,
    /// Durable projection revision, or `None` when the source row omits it.
    pub revision: Option<SessionRevision>,
    pub selected_model: Option<ModelRef>,
    /// Durable last-update time, or `None` when the row is synthetic.
    pub updated_at_ms: Option<UnixMillis>,
    /// Durable message count, or `None` when the row is synthetic.
    pub message_count: Option<DecimalU64>,
    pub archived: bool,
}

impl SessionSummary {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        title: SessionTitle,
        revision: Option<u64>,
        selected_model: Option<ModelRef>,
        updated_at_ms: Option<i64>,
        message_count: Option<u64>,
        archived: bool,
    ) -> Self {
        Self {
            session_id,
            title,
            revision: match revision {
                Some(value) => Some(SessionRevision::new(value)),
                None => None,
            },
            selected_model,
            updated_at_ms: match updated_at_ms {
                Some(value) => Some(UnixMillis::new(value)),
                None => None,
            },
            message_count: match message_count {
                Some(value) => Some(DecimalU64::new(value)),
                None => None,
            },
            archived,
        }
    }
}

/// Provider-reported capability support.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
}

/// Provider-neutral model row suitable for any renderer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ModelSummary {
    pub model: ModelRef,
    pub display_name: String,
    pub detail: String,
    pub context_window_tokens: Option<DecimalU64>,
    pub selectable: bool,
    pub chat: CapabilitySupport,
    pub streaming: CapabilitySupport,
    pub thinking: CapabilitySupport,
    pub tool_calling: CapabilitySupport,
}

impl ModelSummary {
    /// Constructs a bounded provider-neutral model row.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model: ModelRef,
        display_name: impl Into<String>,
        detail: impl Into<String>,
        context_window_tokens: Option<u64>,
        selectable: bool,
        chat: CapabilitySupport,
        streaming: CapabilitySupport,
        thinking: CapabilitySupport,
        tool_calling: CapabilitySupport,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            model,
            display_name: display_name.into(),
            detail: detail.into(),
            context_window_tokens: context_window_tokens.map(DecimalU64::new),
            selectable,
            chat,
            streaming,
            thinking,
            tool_calling,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty_text("model_display_name", &self.display_name, MAX_LABEL_BYTES)?;
        validate_text("model_detail", &self.detail, MAX_DETAIL_BYTES)
    }
}

impl<'de> Deserialize<'de> for ModelSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireModelSummary {
            model: ModelRef,
            display_name: String,
            detail: String,
            context_window_tokens: Option<DecimalU64>,
            selectable: bool,
            chat: CapabilitySupport,
            streaming: CapabilitySupport,
            thinking: CapabilitySupport,
            tool_calling: CapabilitySupport,
        }
        let wire = WireModelSummary::deserialize(deserializer)?;
        Self::new(
            wire.model,
            wire.display_name,
            wire.detail,
            wire.context_window_tokens.map(DecimalU64::get),
            wire.selectable,
            wire.chat,
            wire.streaming,
            wire.thinking,
            wire.tool_calling,
        )
        .map_err(D::Error::custom)
    }
}

/// Current provider-neutral model-catalog state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum CatalogProjection {
    CredentialRequired,
    Loading,
    Ready {
        generation: DecimalU64,
        models: Vec<ModelSummary>,
        stale: bool,
    },
    Failed {
        failure: SafeFailure,
    },
}

impl CatalogProjection {
    pub fn ready(
        generation: u64,
        models: Vec<ModelSummary>,
        stale: bool,
    ) -> Result<Self, ValidationError> {
        let value = Self::Ready {
            generation: DecimalU64::new(generation),
            models,
            stale,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Ready { models, .. } = self {
            validate_count("catalog_models", models.len(), MAX_CATALOG_MODELS)?;
            models.iter().try_for_each(ModelSummary::validate)?;
            let mut identities = BTreeSet::new();
            for model in models {
                if !identities.insert((
                    model.model.provider_id.as_str(),
                    model.model.model_id.as_str(),
                )) {
                    return Err(ValidationError::Inconsistent {
                        field: "catalog_models",
                    });
                }
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for CatalogProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(
            deny_unknown_fields,
            rename_all = "snake_case",
            tag = "kind",
            content = "payload"
        )]
        enum WireCatalog {
            CredentialRequired,
            Loading,
            Ready {
                generation: DecimalU64,
                models: Vec<ModelSummary>,
                stale: bool,
            },
            Failed {
                failure: SafeFailure,
            },
        }
        match WireCatalog::deserialize(deserializer)? {
            WireCatalog::CredentialRequired => Ok(Self::CredentialRequired),
            WireCatalog::Loading => Ok(Self::Loading),
            WireCatalog::Ready {
                generation,
                models,
                stale,
            } => Self::ready(generation.get(), models, stale).map_err(D::Error::custom),
            WireCatalog::Failed { failure } => Ok(Self::Failed { failure }),
        }
    }
}

/// Non-secret origin of the active provider credential.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    None,
    Environment,
    Vault,
    SessionOnly,
}

/// Current safe provider connection state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ProviderStatus {
    Disconnected,
    CredentialRequired,
    Connecting,
    Ready,
    Offline,
    Failed { failure: SafeFailure },
}

/// One provider connection projection without credential material.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderProjection {
    pub connection_id: ConnectionId,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub active: bool,
    pub status: ProviderStatus,
    pub credential_source: CredentialSource,
    pub default_model: Option<ModelRef>,
}

impl ProviderProjection {
    /// Constructs one bounded non-secret provider connection projection.
    pub fn new(
        connection_id: ConnectionId,
        provider_id: ProviderId,
        display_name: impl Into<String>,
        active: bool,
        status: ProviderStatus,
        credential_source: CredentialSource,
        default_model: Option<ModelRef>,
    ) -> Result<Self, ValidationError> {
        let value = Self {
            connection_id,
            provider_id,
            display_name: display_name.into(),
            active,
            status,
            credential_source,
            default_model,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_non_empty_text("provider_display_name", &self.display_name, MAX_LABEL_BYTES)?;
        if self.credential_source == CredentialSource::None
            && matches!(self.status, ProviderStatus::Ready)
        {
            return Err(ValidationError::Inconsistent {
                field: "provider_status",
            });
        }
        if self
            .default_model
            .as_ref()
            .is_some_and(|model| model.provider_id != self.provider_id)
        {
            return Err(ValidationError::Inconsistent {
                field: "provider_default_model",
            });
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ProviderProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireProvider {
            connection_id: ConnectionId,
            provider_id: ProviderId,
            display_name: String,
            active: bool,
            status: ProviderStatus,
            credential_source: CredentialSource,
            default_model: Option<ModelRef>,
        }
        let wire = WireProvider::deserialize(deserializer)?;
        Self::new(
            wire.connection_id,
            wire.provider_id,
            wire.display_name,
            wire.active,
            wire.status,
            wire.credential_source,
            wire.default_model,
        )
        .map_err(D::Error::custom)
    }
}

/// Coarse host lifecycle visible to renderer-neutral clients.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    tag = "kind",
    content = "payload"
)]
pub enum ClientLifecycle {
    Starting,
    Ready,
    Offline,
    ShuttingDown,
    Failed { failure: SafeFailure },
}

/// Complete authoritative GUI baseline at protocol schema v1.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClientSnapshot {
    pub schema_version: u16,
    pub lifecycle: ClientLifecycle,
    pub active_session_id: Option<SessionId>,
    pub sessions: Vec<SessionSummary>,
    pub active_session: Option<SessionProjection>,
    pub catalog: CatalogProjection,
    pub providers: Vec<ProviderProjection>,
}

impl ClientSnapshot {
    pub fn new(
        lifecycle: ClientLifecycle,
        active_session_id: Option<SessionId>,
        sessions: Vec<SessionSummary>,
        active_session: Option<SessionProjection>,
        catalog: CatalogProjection,
        providers: Vec<ProviderProjection>,
    ) -> Result<Self, ValidationError> {
        validate_count("sessions", sessions.len(), MAX_SESSIONS)?;
        validate_count("providers", providers.len(), MAX_PROVIDERS)?;
        catalog.validate()?;
        providers
            .iter()
            .try_for_each(ProviderProjection::validate)?;

        let session_ids = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<BTreeSet<_>>();
        if session_ids.len() != sessions.len() {
            return Err(ValidationError::Inconsistent { field: "sessions" });
        }
        let connection_ids = providers
            .iter()
            .map(|provider| provider.connection_id.as_str())
            .collect::<BTreeSet<_>>();
        if connection_ids.len() != providers.len()
            || providers.iter().filter(|provider| provider.active).count() > 1
        {
            return Err(ValidationError::Inconsistent { field: "providers" });
        }

        match (&active_session_id, &active_session) {
            (None, None) => {}
            (Some(identity), Some(session))
                if identity == &session.session_id && session_ids.contains(identity.as_str()) => {}
            _ => {
                return Err(ValidationError::Inconsistent {
                    field: "active_session",
                });
            }
        }

        Ok(Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            lifecycle,
            active_session_id,
            sessions,
            active_session,
            catalog,
            providers,
        })
    }
}

impl<'de> Deserialize<'de> for ClientSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSnapshot {
            schema_version: u16,
            lifecycle: ClientLifecycle,
            active_session_id: Option<SessionId>,
            sessions: Vec<SessionSummary>,
            active_session: Option<SessionProjection>,
            catalog: CatalogProjection,
            providers: Vec<ProviderProjection>,
        }
        let wire = WireSnapshot::deserialize(deserializer)?;
        if wire.schema_version != CLIENT_SCHEMA_VERSION {
            return Err(D::Error::custom(
                "unsupported client snapshot schema version",
            ));
        }
        Self::new(
            wire.lifecycle,
            wire.active_session_id,
            wire.sessions,
            wire.active_session,
            wire.catalog,
            wire.providers,
        )
        .map_err(D::Error::custom)
    }
}
