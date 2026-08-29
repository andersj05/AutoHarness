use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelRef, RetryAdvice};
use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, EffectiveLocalProfile, GlyphMode, Layout,
    PromptStatusDetail, TerminalTimestampStyle, ThemePreset,
};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui_textarea::{TextArea, WrapMode};
use zeroize::{Zeroize, Zeroizing};

use crate::ui::{ColorDepth, Theme, Token};

const MAX_CREDENTIAL_BYTES: usize = 4_096;
const MAX_MEMORY_ID_CHARS: usize = 128;
const MAX_MEMORY_PREVIEW_CHARS: usize = 240;
const MAX_MEMORY_CONTENT_CHARS: usize = 16_384;
const MAX_MEMORY_SOURCE_CHARS: usize = 512;
const MAX_MEMORY_ADMISSION_TEXT_CHARS: usize = 256;
const MAX_MEMORY_CONTEXT_TEXT_CHARS: usize = 512;
const MAX_MEMORY_SUMMARIES: usize = 100;
const MAX_MEMORY_DETAILS: usize = 100;
const MAX_MEMORY_ADMISSIONS: usize = 64;
const MAX_MEMORY_EVIDENCE: usize = 16;
const MAX_MEMORY_RELATIONS: usize = 16;
const MAX_MEMORY_FINDINGS: usize = 16;
const MAX_MEMORY_REASON_FACTORS: usize = 16;

/// Monotonic, process-local identity used to correlate a UI request.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    /// Creates a process-local request identity for application composition or tests.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying request sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A provider-neutral attempt identity supplied by application composition.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AttemptKey(String);

impl AttemptKey {
    /// Creates a non-empty UI attempt identity.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("attempt identity must not be empty");
        }
        Ok(Self(value))
    }

    /// Returns the stable string form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A provider-neutral tool-call identity supplied by application composition.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ToolCallKey(String);

impl ToolCallKey {
    /// Creates a non-empty UI tool-call identity.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("tool-call identity must not be empty");
        }
        Ok(Self(value))
    }

    /// Returns the stable string form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A model row suitable for the picker without provider-native payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSummary {
    /// Stable provider and model identity.
    pub model: ModelRef,
    /// Human-oriented display label.
    pub display_name: String,
    /// Short provider-neutral capability summary.
    pub detail: String,
    /// Maximum provider-advertised context window in tokens, when known.
    pub context_window_tokens: Option<u64>,
    /// Whether this model can be selected for chat.
    pub selectable: bool,
}

/// Current model-catalog read state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogProjection {
    /// No provider credential is available for model discovery.
    CredentialRequired,
    /// The catalog has not completed its first load.
    Loading,
    /// The latest successfully projected catalog.
    Ready {
        /// Compatible and visible model rows.
        models: Vec<ModelSummary>,
        /// Whether the rows came from a stale cache.
        stale: bool,
    },
    /// Catalog discovery failed with a safe error.
    Failed(UiFailure),
}

impl CatalogProjection {
    /// Returns ready model rows, or an empty slice for non-ready states.
    #[must_use]
    pub fn models(&self) -> &[ModelSummary] {
        match self {
            Self::Ready { models, .. } => models,
            Self::CredentialRequired | Self::Loading | Self::Failed(_) => &[],
        }
    }
}

/// Stable retry information used by presentation logic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryPolicy {
    /// This operation cannot safely be retried.
    Never,
    /// Retry is available now.
    Now,
    /// Retry becomes available this many milliseconds after first UI observation.
    After {
        /// Relative delay supplied by durable provider-neutral retry advice.
        delay_ms: u64,
    },
    /// Retry becomes available at this UI monotonic time.
    At(UiInstant),
}

impl RetryPolicy {
    const DEFAULT_BACKOFF_DELAY_MS: u64 = 1_000;

    /// Converts domain retry advice into presentation state.
    #[must_use]
    pub const fn from_advice(advice: RetryAdvice, _now: UiInstant) -> Self {
        match advice {
            RetryAdvice::Never => Self::Never,
            RetryAdvice::Immediate => Self::Now,
            RetryAdvice::Backoff => Self::After {
                delay_ms: Self::DEFAULT_BACKOFF_DELAY_MS,
            },
            RetryAdvice::After { delay_ms } => Self::After { delay_ms },
        }
    }
}

/// A safe, provider-neutral error rendered by the terminal client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiFailure {
    /// Stable failure class.
    pub class: ErrorClass,
    /// Stable machine-readable failure code.
    pub code: String,
    /// Sanitized public message. Rendering still escapes terminal controls.
    pub message: String,
    /// User-visible retry policy.
    pub retry: RetryPolicy,
}

impl UiFailure {
    /// Constructs a safe UI failure.
    #[must_use]
    pub fn new(class: ErrorClass, message: impl Into<String>, retry: RetryPolicy) -> Self {
        Self {
            class,
            code: error_class_code(class).to_owned(),
            message: message.into(),
            retry,
        }
    }

    /// Replaces the class fallback with a more specific stable code.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }
}

const fn error_class_code(class: ErrorClass) -> &'static str {
    match class {
        ErrorClass::Validation => "validation",
        ErrorClass::NotFound => "not_found",
        ErrorClass::Conflict => "conflict",
        ErrorClass::Authentication => "authentication",
        ErrorClass::PermissionDenied => "permission_denied",
        ErrorClass::RateLimited => "rate_limited",
        ErrorClass::Timeout => "timeout",
        ErrorClass::Unavailable => "unavailable",
        ErrorClass::Cancelled => "cancelled",
        ErrorClass::Protocol => "protocol",
        ErrorClass::Storage => "storage",
        ErrorClass::Internal => "internal",
    }
}

/// Usage values displayed after or during an attempt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UsageView {
    /// Input tokens reported by the provider.
    pub input_tokens: u64,
    /// Output tokens reported by the provider.
    pub output_tokens: u64,
}

/// Visible lifecycle of an assistant attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttemptStatus {
    /// Provider output is still arriving.
    Streaming,
    /// Cancellation was durably requested but the attempt has not settled.
    Cancelling,
    /// The attempt completed normally.
    Completed,
    /// The attempt settled as cancelled.
    Cancelled,
    /// The attempt settled with a safe failure.
    Failed(UiFailure),
}

/// One provider-neutral item in the visible transcript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptItem {
    /// A durably admitted user input.
    User {
        /// Stable input identity.
        input_id: String,
        /// Exact user-authored text.
        text: String,
    },
    /// One durable tool call rendered as a structured row.
    Tool(
        /// Provider-neutral presentation row for one tool call.
        ToolRowView,
    ),
    /// One model attempt, including retry lineage and settlement.
    Assistant {
        /// Stable attempt identity.
        attempt_id: AttemptKey,
        /// Exact accumulated provider text.
        text: String,
        /// Current attempt state.
        status: AttemptStatus,
        /// Optional provider-reported usage.
        usage: Option<UsageView>,
        /// Prior attempt when this is a retry.
        retry_of: Option<AttemptKey>,
    },
}

/// One durable tool-call row suitable for collapsed transcript display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRowView {
    /// Stable tool-call identity.
    pub tool_call_id: ToolCallKey,
    /// Registered versioned tool name.
    pub tool_name: String,
    /// Canonical scoped resource, shown only when expanded.
    pub resource: String,
    /// Safe settled or running state label.
    pub status: String,
    /// One-line bounded summary of the outcome, when any exists.
    pub summary: Option<String>,
}

/// One durable human permission request.
#[derive(Clone, Eq, PartialEq)]
pub struct PermissionRequestView {
    /// Stable tool-call identity.
    pub tool_call_id: ToolCallKey,
    /// Registered versioned tool name.
    pub tool_name: String,
    /// Trusted capability class.
    pub capability: String,
    /// Canonical scoped resource.
    pub resource: String,
    /// Trusted operation-specific fields required for an informed decision.
    pub details: Vec<PermissionDetailView>,
}

impl Debug for PermissionRequestView {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionRequestView")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("capability", &self.capability)
            .field("resource", &self.resource)
            .field("details", &"[REDACTED]")
            .finish()
    }
}

/// One trusted permission field visible only in the decision overlay.
#[derive(Clone, Eq, PartialEq)]
pub struct PermissionDetailView {
    /// Human-readable field label.
    pub label: String,
    /// Exact or conservatively summarized field value.
    pub value: String,
}

impl Debug for PermissionDetailView {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("PermissionDetailView([REDACTED])")
    }
}

/// Read model derived from durable session events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProjection {
    /// Stable durable identity of the projected session.
    pub session_id: String,
    /// Monotonic projection revision.
    pub revision: u64,
    /// Current selected model.
    pub selected_model: Option<ModelRef>,
    /// Visible transcript in durable order.
    pub transcript: Vec<TranscriptItem>,
    /// Durable unanswered permission requests in proposal order.
    pub permission_requests: Vec<PermissionRequestView>,
}

impl SessionProjection {
    /// Creates an empty session projection.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            session_id: String::new(),
            revision: 0,
            selected_model: None,
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }
    }

    /// Returns the newest actively streaming attempt, if any.
    #[must_use]
    pub fn streaming_attempt(&self) -> Option<&AttemptKey> {
        self.latest_assistant().and_then(|(attempt_id, status)| {
            matches!(status, AttemptStatus::Streaming).then_some(attempt_id)
        })
    }

    /// Returns the newest unsettled attempt and its active state, if any.
    #[must_use]
    pub fn active_attempt(&self) -> Option<(&AttemptKey, &AttemptStatus)> {
        self.latest_assistant().filter(|(_, status)| {
            matches!(status, AttemptStatus::Streaming | AttemptStatus::Cancelling)
        })
    }

    /// Returns the newest failed attempt, if any.
    #[must_use]
    pub fn failed_attempt(&self) -> Option<(&AttemptKey, &UiFailure)> {
        self.latest_assistant()
            .and_then(|(attempt_id, status)| match status {
                AttemptStatus::Failed(failure) => Some((attempt_id, failure)),
                AttemptStatus::Streaming
                | AttemptStatus::Cancelling
                | AttemptStatus::Completed
                | AttemptStatus::Cancelled => None,
            })
    }

    /// Returns the newest failed or cancelled attempt and its retry policy.
    #[must_use]
    pub fn retryable_attempt(&self) -> Option<(&AttemptKey, RetryPolicy)> {
        self.latest_assistant()
            .and_then(|(attempt_id, status)| match status {
                AttemptStatus::Failed(failure) => Some((attempt_id, failure.retry)),
                AttemptStatus::Cancelled => Some((attempt_id, RetryPolicy::Now)),
                AttemptStatus::Streaming | AttemptStatus::Cancelling | AttemptStatus::Completed => {
                    None
                }
            })
    }

    fn latest_assistant(&self) -> Option<(&AttemptKey, &AttemptStatus)> {
        self.transcript.iter().rev().find_map(|item| match item {
            TranscriptItem::Assistant {
                attempt_id, status, ..
            } => Some((attempt_id, status)),
            TranscriptItem::User { .. } | TranscriptItem::Tool(_) => None,
        })
    }
}

/// Monotonic milliseconds supplied by the runner or a deterministic test.
pub type UiInstant = u64;

/// One clock sample published by the runner or a deterministic test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiClock {
    /// Monotonic milliseconds since the runner started, used for animation and deadlines.
    pub now: UiInstant,
    /// Unix epoch milliseconds, used for relative session timestamps.
    pub wall_ms: i64,
}

impl UiClock {
    /// Creates a clock sample from monotonic and wall-clock milliseconds.
    #[must_use]
    pub const fn new(now: UiInstant, wall_ms: i64) -> Self {
        Self { now, wall_ms }
    }
}

/// Current keyboard focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Focus {
    /// The multiline prompt editor owns ordinary key input.
    #[default]
    Composer,
    /// The model-picker overlay owns key input.
    Picker,
    /// The ephemeral provider-credential overlay owns key input.
    Credential,
    /// A durable tool permission request owns key input.
    Permission,
    /// The session-browser overlay owns key input.
    Browser,
    /// The Profiles and Providers surface owns key input.
    Profiles,
    /// The command-palette overlay owns key input.
    Palette,
    /// The contextual help overlay owns key input.
    Help,
    /// The settings route owns key input.
    Settings,
    /// The local user-profile dialog owns key input.
    UserProfile,
    /// An exact destructive action awaits Y or N.
    Confirmation,
    /// The transcript search bar owns key input.
    Search,
    /// The Memory inspection workspace owns key input.
    Memory,
    /// The single Memory lifecycle dialog owns key input.
    MemoryLifecycle,
}
/// One primary destination in the terminal application shell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Route {
    /// Streaming conversation, transcript, tools, and composer.
    #[default]
    Chat,
    /// Durable session discovery and lifecycle management.
    Sessions,
    /// Local profile and provider connection management.
    Profiles,
    /// Resolved settings and provenance.
    Settings,
    /// Contextual keyboard and workflow guidance.
    Help,
    /// Searchable, provenance-aware memory inspection.
    Memory,
}

impl Route {
    /// Stable ordered route table used by navigation and rendering.
    pub const ALL: [Self; 6] = [
        Self::Chat,
        Self::Sessions,
        Self::Profiles,
        Self::Settings,
        Self::Help,
        Self::Memory,
    ];

    /// Returns the visible route label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Sessions => "Sessions",
            Self::Profiles => "Profiles",
            Self::Settings => "Settings",
            Self::Help => "Help",
            Self::Memory => "Memory",
        }
    }

    /// Returns the direct keyboard chord shown in the shell.
    #[must_use]
    pub const fn key_hint(self) -> &'static str {
        match self {
            Self::Chat => "Alt+1",
            Self::Sessions => "Alt+2",
            Self::Profiles => "Alt+3",
            Self::Settings => "Alt+4",
            Self::Help => "Alt+5",
            Self::Memory => "Alt+6",
        }
    }

    /// Returns the route's normal keyboard owner.
    #[must_use]
    pub const fn focus(self) -> Focus {
        match self {
            Self::Chat => Focus::Composer,
            Self::Sessions => Focus::Browser,
            Self::Profiles => Focus::Profiles,
            Self::Settings => Focus::Settings,
            Self::Help => Focus::Help,
            Self::Memory => Focus::Memory,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayKind {
    ModelPicker,
    SessionCredential,
    CommandPalette,
    TranscriptSearch,
    Permission,
    ProfileCredential,
    UserProfile,
    Confirmation,
    MemoryLifecycle,
}

impl OverlayKind {
    /// Returns the keyboard owner for this modal layer.
    #[must_use]
    pub const fn focus(self) -> Focus {
        match self {
            Self::ModelPicker => Focus::Picker,
            Self::SessionCredential | Self::ProfileCredential => Focus::Credential,
            Self::CommandPalette => Focus::Palette,
            Self::TranscriptSearch => Focus::Search,
            Self::Permission => Focus::Permission,
            Self::UserProfile => Focus::UserProfile,
            Self::Confirmation => Focus::Confirmation,
            Self::MemoryLifecycle => Focus::MemoryLifecycle,
        }
    }
}

/// Captured base state restored when one modal overlay closes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlayFrame {
    pub kind: OverlayKind,
    pub return_route: Route,
    pub return_focus: Focus,
}

/// Single authority for route selection and modal ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NavigationState {
    pub route: Route,
    pub previous_route: Route,
    pub overlay: Option<OverlayFrame>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            route: Route::Chat,
            previous_route: Route::Chat,
            overlay: None,
        }
    }
}

/// One searchable row in the session browser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionBrowserEntry {
    /// Stable durable session identity.
    pub session_id: String,
    /// Deterministic browser label derived from durable state.
    pub title: String,
    /// Durable lifecycle state of the session.
    pub archived: bool,
    /// Latest selected provider-neutral model identity, when any.
    pub selected_model: Option<ModelRef>,
    /// Number of provider-neutral transcript messages in durable storage.
    pub message_count: u64,
    /// Last event's observed time in epoch milliseconds.
    pub updated_at_ms: i64,
    /// Whether this row is the currently active session.
    pub active: bool,
}
/// Non-secret provider form transferred from the TUI to application composition.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderProfileDraft {
    /// Stable profile identity entered by the user.
    pub id: String,
    /// Selected provider adapter.
    pub kind: ProviderKindLabel,
    /// Router base URL; empty for Gemini.
    pub base_url: String,
    /// Optional router project identity.
    pub project: String,
    /// Optional router authentication header name.
    pub auth_header: String,
}

impl Debug for ProviderProfileDraft {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderProfileDraft")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("has_base_url", &!self.base_url.is_empty())
            .field("has_project", &!self.project.is_empty())
            .field("has_auth_header", &!self.auth_header.is_empty())
            .finish()
    }
}

/// Read model for every durable session known to the application.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionsProjection {
    /// Sessions in deterministic recent-first order.
    pub sessions: Vec<SessionBrowserEntry>,
}

/// Durable memory lifecycle shown by the inspection workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryStatus {
    Active,
    Proposed,
    Superseded,
    Rejected,
    Retracted,
    Deleted,
}

impl MemoryStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Proposed => "proposed",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
            Self::Retracted => "retracted",
            Self::Deleted => "deleted",
        }
    }
}

/// Boundary at which a memory may be admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryScope {
    User,
    Workspace,
    Session,
    Agent,
}

impl MemoryScope {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Session => "session",
            Self::Agent => "agent",
        }
    }
}

/// Provenance class attached to one admitted memory revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryTrust {
    UserApproved,
    VerifiedObservation,
    Imported,
    UntrustedProposal,
}

impl MemoryTrust {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UserApproved => "user approved",
            Self::VerifiedObservation => "verified observation",
            Self::Imported => "imported",
            Self::UntrustedProposal => "untrusted proposal",
        }
    }
}

/// Bounded memory content transferred without appearing in diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryContent {
    raw: Zeroizing<String>,
}

impl MemoryContent {
    /// Creates exact, bounded memory content with safe terminal controls only.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = bounded_text(value.into(), MAX_MEMORY_CONTENT_CHARS, "memory content")?;
        if value.trim().is_empty() {
            return Err("memory content must not be blank");
        }
        Ok(Self {
            raw: Zeroizing::new(value),
        })
    }

    /// Borrows the exact content for application-owned validation and dispatch.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Moves the exact content into application composition.
    #[must_use]
    pub fn into_string(mut self) -> String {
        std::mem::take(&mut *self.raw)
    }
}

impl Debug for MemoryContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryContent([REDACTED])")
    }
}

/// Durable origin class shown during inspection and proposal review.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryOrigin {
    ExplicitUser,
    VerifiedTool,
    ImportedDocument,
    ModelProposal,
    Compaction,
}

impl MemoryOrigin {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExplicitUser => "explicit user",
            Self::VerifiedTool => "verified tool",
            Self::ImportedDocument => "imported document",
            Self::ModelProposal => "model proposal",
            Self::Compaction => "compaction",
        }
    }
}

/// Sensitivity classification used by memory admission policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemorySensitivity {
    Public,
    Internal,
    Sensitive,
    Secret,
}

impl MemorySensitivity {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Sensitive => "sensitive",
            Self::Secret => "secret",
        }
    }
}

/// One exact, bounded evidence excerpt supporting a memory revision.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryEvidence {
    label: String,
    source: String,
    excerpt: String,
}

impl MemoryEvidence {
    pub fn new(
        label: impl Into<String>,
        source: impl Into<String>,
        excerpt: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            label: bounded_single_line(
                label.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory evidence label",
            )?,
            source: bounded_single_line(
                source.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory evidence source",
            )?,
            excerpt: bounded_text(
                excerpt.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory evidence excerpt",
            )?,
        })
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn excerpt(&self) -> &str {
        &self.excerpt
    }
}

impl Debug for MemoryEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryEvidence")
            .field("label", &"[REDACTED]")
            .field("source", &"[REDACTED]")
            .field("excerpt", &"[REDACTED]")
            .finish()
    }
}

/// Typed relation between one memory and another ledger identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRelationKind {
    DuplicateOf,
    Contradicts,
    Supersedes,
    DerivedFrom,
}

impl MemoryRelationKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DuplicateOf => "duplicate of",
            Self::Contradicts => "contradicts",
            Self::Supersedes => "supersedes",
            Self::DerivedFrom => "derived from",
        }
    }
}

/// One bounded relation shown without exposing its identity to diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRelation {
    kind: MemoryRelationKind,
    memory_id: String,
}

impl MemoryRelation {
    pub fn new(
        kind: MemoryRelationKind,
        memory_id: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            kind,
            memory_id: bounded_single_line(
                memory_id.into(),
                MAX_MEMORY_ID_CHARS,
                "memory identity",
            )?,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> MemoryRelationKind {
        self.kind
    }

    #[must_use]
    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }
}

impl Debug for MemoryRelation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRelation")
            .field("kind", &self.kind)
            .field("memory_id", &"[REDACTED]")
            .finish()
    }
}

/// Validation finding that needs deliberate review before proposal approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryFindingKind {
    Duplicate,
    Contradiction,
}

impl MemoryFindingKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Duplicate => "possible duplicate",
            Self::Contradiction => "possible contradiction",
        }
    }
}

/// One bounded duplicate or contradiction finding.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryValidationFinding {
    kind: MemoryFindingKind,
    related_memory_id: String,
    summary: String,
}

impl MemoryValidationFinding {
    pub fn new(
        kind: MemoryFindingKind,
        related_memory_id: impl Into<String>,
        summary: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            kind,
            related_memory_id: bounded_single_line(
                related_memory_id.into(),
                MAX_MEMORY_ID_CHARS,
                "memory identity",
            )?,
            summary: bounded_single_line(
                summary.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory validation finding",
            )?,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> MemoryFindingKind {
        self.kind
    }

    #[must_use]
    pub fn related_memory_id(&self) -> &str {
        &self.related_memory_id
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

impl Debug for MemoryValidationFinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryValidationFinding")
            .field("kind", &self.kind)
            .field("related_memory_id", &"[REDACTED]")
            .field("summary", &"[REDACTED]")
            .finish()
    }
}

/// Exact command and review metadata for a loaded memory revision.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRevisionContext {
    expected_last_sequence: u64,
    revision_id: String,
    proposal_revision_id: Option<String>,
    scope_identity: String,
    origin: MemoryOrigin,
    sensitivity: MemorySensitivity,
    evidence: Vec<MemoryEvidence>,
    relations: Vec<MemoryRelation>,
    findings: Vec<MemoryValidationFinding>,
}

impl MemoryRevisionContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        expected_last_sequence: u64,
        revision_id: impl Into<String>,
        proposal_revision_id: Option<String>,
        scope_identity: impl Into<String>,
        origin: MemoryOrigin,
        sensitivity: MemorySensitivity,
        evidence: Vec<MemoryEvidence>,
        relations: Vec<MemoryRelation>,
        findings: Vec<MemoryValidationFinding>,
    ) -> Result<Self, &'static str> {
        if evidence.len() > MAX_MEMORY_EVIDENCE {
            return Err("too many memory evidence records");
        }
        if relations.len() > MAX_MEMORY_RELATIONS {
            return Err("too many memory relations");
        }
        if findings.len() > MAX_MEMORY_FINDINGS {
            return Err("too many memory validation findings");
        }
        let proposal_revision_id = proposal_revision_id
            .map(|value| {
                bounded_single_line(value, MAX_MEMORY_ID_CHARS, "memory proposal identity")
            })
            .transpose()?;
        Ok(Self {
            expected_last_sequence,
            revision_id: bounded_single_line(
                revision_id.into(),
                MAX_MEMORY_ID_CHARS,
                "memory revision identity",
            )?,
            proposal_revision_id,
            scope_identity: bounded_single_line(
                scope_identity.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory scope identity",
            )?,
            origin,
            sensitivity,
            evidence,
            relations,
            findings,
        })
    }

    #[must_use]
    pub const fn expected_last_sequence(&self) -> u64 {
        self.expected_last_sequence
    }

    #[must_use]
    pub fn revision_id(&self) -> &str {
        &self.revision_id
    }

    #[must_use]
    pub fn proposal_revision_id(&self) -> Option<&str> {
        self.proposal_revision_id.as_deref()
    }

    #[must_use]
    pub fn scope_identity(&self) -> &str {
        &self.scope_identity
    }

    #[must_use]
    pub const fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    #[must_use]
    pub const fn sensitivity(&self) -> MemorySensitivity {
        self.sensitivity
    }

    #[must_use]
    pub fn evidence(&self) -> &[MemoryEvidence] {
        &self.evidence
    }

    #[must_use]
    pub fn relations(&self) -> &[MemoryRelation] {
        &self.relations
    }

    #[must_use]
    pub fn findings(&self) -> &[MemoryValidationFinding] {
        &self.findings
    }
}

impl Debug for MemoryRevisionContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRevisionContext")
            .field("expected_last_sequence", &self.expected_last_sequence)
            .field("revision_id", &"[REDACTED]")
            .field(
                "has_proposal_revision",
                &self.proposal_revision_id.is_some(),
            )
            .field("scope_identity", &"[REDACTED]")
            .field("origin", &self.origin)
            .field("sensitivity", &self.sensitivity)
            .field("evidence_count", &self.evidence.len())
            .field("relation_count", &self.relations.len())
            .field("finding_count", &self.findings.len())
            .finish()
    }
}

/// One bounded, display-safe row in the Memory index.
#[derive(Clone, Eq, PartialEq)]
pub struct MemorySummary {
    id: String,
    preview: String,
    status: MemoryStatus,
    scope: MemoryScope,
    updated_at_ms: i64,
    confidence_bps: Option<u16>,
    admission_count: u32,
}

impl MemorySummary {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        preview: impl Into<String>,
        status: MemoryStatus,
        scope: MemoryScope,
        updated_at_ms: i64,
        confidence_bps: Option<u16>,
        admission_count: u32,
    ) -> Result<Self, &'static str> {
        let id = bounded_single_line(id.into(), MAX_MEMORY_ID_CHARS, "memory identity")?;
        let preview =
            bounded_single_line(preview.into(), MAX_MEMORY_PREVIEW_CHARS, "memory preview")?;
        if confidence_bps.is_some_and(|value| value > 10_000) {
            return Err("memory confidence must be at most 10000 basis points");
        }
        Ok(Self {
            id,
            preview,
            status,
            scope,
            updated_at_ms,
            confidence_bps,
            admission_count,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn preview(&self) -> &str {
        &self.preview
    }

    #[must_use]
    pub const fn status(&self) -> MemoryStatus {
        self.status
    }

    #[must_use]
    pub const fn scope(&self) -> MemoryScope {
        self.scope
    }

    #[must_use]
    pub const fn updated_at_ms(&self) -> i64 {
        self.updated_at_ms
    }

    #[must_use]
    pub const fn confidence_bps(&self) -> Option<u16> {
        self.confidence_bps
    }

    #[must_use]
    pub const fn admission_count(&self) -> u32 {
        self.admission_count
    }
}

impl Debug for MemorySummary {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemorySummary")
            .field("id", &"[REDACTED]")
            .field("preview", &"[REDACTED]")
            .field("status", &self.status)
            .field("scope", &self.scope)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("confidence_bps", &self.confidence_bps)
            .field("admission_count", &self.admission_count)
            .finish()
    }
}

/// One bounded provenance record explaining a memory admission.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryAdmission {
    session: String,
    model: String,
    reason: String,
    admitted_at_ms: i64,
    rank: u32,
    context: Option<MemoryAdmissionContext>,
}

impl MemoryAdmission {
    pub fn new(
        session: impl Into<String>,
        model: impl Into<String>,
        reason: impl Into<String>,
        admitted_at_ms: i64,
        rank: u32,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            session: bounded_single_line(
                session.into(),
                MAX_MEMORY_ADMISSION_TEXT_CHARS,
                "admission session",
            )?,
            model: bounded_single_line(
                model.into(),
                MAX_MEMORY_ADMISSION_TEXT_CHARS,
                "admission model",
            )?,
            reason: bounded_single_line(
                reason.into(),
                MAX_MEMORY_ADMISSION_TEXT_CHARS,
                "admission reason",
            )?,
            admitted_at_ms,
            rank,
            context: None,
        })
    }

    /// Attaches exact provider-turn admission context when the projection loaded it.
    #[must_use]
    pub fn with_context(mut self, context: MemoryAdmissionContext) -> Self {
        self.context = Some(context);
        self
    }

    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub const fn admitted_at_ms(&self) -> i64 {
        self.admitted_at_ms
    }

    #[must_use]
    pub const fn rank(&self) -> u32 {
        self.rank
    }

    #[must_use]
    pub fn context(&self) -> Option<&MemoryAdmissionContext> {
        self.context.as_ref()
    }
}

impl Debug for MemoryAdmission {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryAdmission")
            .field("session", &"[REDACTED]")
            .field("model", &"[REDACTED]")
            .field("reason", &"[REDACTED]")
            .field("admitted_at_ms", &self.admitted_at_ms)
            .field("rank", &self.rank)
            .field("context", &self.context)
            .finish()
    }
}

/// Exact, bounded coordinates explaining one provider-turn admission.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryAdmissionContext {
    provider_attempt: String,
    run_turn: u32,
    epoch: String,
    token_count: u32,
    source_revision: String,
    renderer_version: String,
    reason_factors: Vec<String>,
}

impl MemoryAdmissionContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_attempt: impl Into<String>,
        run_turn: u32,
        epoch: impl Into<String>,
        token_count: u32,
        source_revision: impl Into<String>,
        renderer_version: impl Into<String>,
        reason_factors: Vec<String>,
    ) -> Result<Self, &'static str> {
        if reason_factors.len() > MAX_MEMORY_REASON_FACTORS {
            return Err("too many memory admission reason factors");
        }
        let reason_factors = reason_factors
            .into_iter()
            .map(|factor| {
                bounded_single_line(
                    factor,
                    MAX_MEMORY_ADMISSION_TEXT_CHARS,
                    "memory admission factor",
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            provider_attempt: bounded_single_line(
                provider_attempt.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory provider attempt",
            )?,
            run_turn,
            epoch: bounded_single_line(
                epoch.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory context epoch",
            )?,
            token_count,
            source_revision: bounded_single_line(
                source_revision.into(),
                MAX_MEMORY_ID_CHARS,
                "memory revision identity",
            )?,
            renderer_version: bounded_single_line(
                renderer_version.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory renderer version",
            )?,
            reason_factors,
        })
    }

    #[must_use]
    pub fn provider_attempt(&self) -> &str {
        &self.provider_attempt
    }

    #[must_use]
    pub const fn run_turn(&self) -> u32 {
        self.run_turn
    }

    #[must_use]
    pub fn epoch(&self) -> &str {
        &self.epoch
    }

    #[must_use]
    pub const fn token_count(&self) -> u32 {
        self.token_count
    }

    #[must_use]
    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    #[must_use]
    pub fn renderer_version(&self) -> &str {
        &self.renderer_version
    }

    #[must_use]
    pub fn reason_factors(&self) -> &[String] {
        &self.reason_factors
    }
}

impl Debug for MemoryAdmissionContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryAdmissionContext")
            .field("provider_attempt", &"[REDACTED]")
            .field("run_turn", &self.run_turn)
            .field("epoch", &"[REDACTED]")
            .field("token_count", &self.token_count)
            .field("source_revision", &"[REDACTED]")
            .field("renderer_version", &"[REDACTED]")
            .field("reason_factor_count", &self.reason_factors.len())
            .finish()
    }
}

/// Bounded content and provenance for one memory revision.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryDetail {
    memory_id: String,
    revision: u32,
    content: String,
    source: String,
    trust: MemoryTrust,
    created_at_ms: i64,
    valid_until_ms: Option<i64>,
    admissions: Vec<MemoryAdmission>,
    revision_context: Option<MemoryRevisionContext>,
}

impl MemoryDetail {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        memory_id: impl Into<String>,
        revision: u32,
        content: impl Into<String>,
        source: impl Into<String>,
        trust: MemoryTrust,
        created_at_ms: i64,
        valid_until_ms: Option<i64>,
        admissions: Vec<MemoryAdmission>,
    ) -> Result<Self, &'static str> {
        let memory_id =
            bounded_single_line(memory_id.into(), MAX_MEMORY_ID_CHARS, "memory identity")?;
        let content = bounded_text(content.into(), MAX_MEMORY_CONTENT_CHARS, "memory content")?;
        let source = bounded_single_line(source.into(), MAX_MEMORY_SOURCE_CHARS, "memory source")?;
        if admissions.len() > MAX_MEMORY_ADMISSIONS {
            return Err("too many memory admissions");
        }
        Ok(Self {
            memory_id,
            revision,
            content,
            source,
            trust,
            created_at_ms,
            valid_until_ms,
            admissions,
            revision_context: None,
        })
    }

    /// Creates lifecycle metadata for a revision whose erasable content sidecar is unavailable.
    #[allow(clippy::too_many_arguments)]
    pub fn metadata_only(
        memory_id: impl Into<String>,
        revision: u32,
        source: impl Into<String>,
        trust: MemoryTrust,
        created_at_ms: i64,
        valid_until_ms: Option<i64>,
        admissions: Vec<MemoryAdmission>,
    ) -> Result<Self, &'static str> {
        let memory_id =
            bounded_single_line(memory_id.into(), MAX_MEMORY_ID_CHARS, "memory identity")?;
        let source = bounded_single_line(source.into(), MAX_MEMORY_SOURCE_CHARS, "memory source")?;
        if admissions.len() > MAX_MEMORY_ADMISSIONS {
            return Err("too many memory admissions");
        }
        Ok(Self {
            memory_id,
            revision,
            content: String::new(),
            source,
            trust,
            created_at_ms,
            valid_until_ms,
            admissions,
            revision_context: None,
        })
    }

    /// Attaches exact lifecycle and review metadata when loaded by the projection.
    #[must_use]
    pub fn with_revision_context(mut self, context: MemoryRevisionContext) -> Self {
        self.revision_context = Some(context);
        self
    }

    #[must_use]
    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }

    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns whether the erasable exact-content sidecar is loaded.
    #[must_use]
    pub fn has_content(&self) -> bool {
        !self.content.is_empty()
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub const fn trust(&self) -> MemoryTrust {
        self.trust
    }

    #[must_use]
    pub const fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }

    #[must_use]
    pub const fn valid_until_ms(&self) -> Option<i64> {
        self.valid_until_ms
    }

    #[must_use]
    pub fn admissions(&self) -> &[MemoryAdmission] {
        &self.admissions
    }

    #[must_use]
    pub fn revision_context(&self) -> Option<&MemoryRevisionContext> {
        self.revision_context.as_ref()
    }
}

impl Debug for MemoryDetail {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryDetail")
            .field("memory_id", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("content", &"[REDACTED]")
            .field("source", &"[REDACTED]")
            .field("trust", &self.trust)
            .field("created_at_ms", &self.created_at_ms)
            .field("valid_until_ms", &self.valid_until_ms)
            .field("admission_count", &self.admissions.len())
            .field("revision_context", &self.revision_context)
            .finish()
    }
}

/// Loading state for the bounded Memory projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryLoadState {
    Loading,
    Ready,
    Failed(UiFailure),
}

/// Bounded read model consumed by the read-only Memory workspace.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryProjection {
    generation: u64,
    state: MemoryLoadState,
    summaries: Vec<MemorySummary>,
    details: Vec<MemoryDetail>,
    total: u32,
    stale: bool,
}

impl MemoryProjection {
    #[must_use]
    pub const fn loading(generation: u64) -> Self {
        Self {
            generation,
            state: MemoryLoadState::Loading,
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
        }
    }

    #[must_use]
    pub fn failed(generation: u64, failure: UiFailure) -> Self {
        Self {
            generation,
            state: MemoryLoadState::Failed(failure),
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
        }
    }

    pub fn ready(
        generation: u64,
        summaries: Vec<MemorySummary>,
        details: Vec<MemoryDetail>,
        total: u32,
        stale: bool,
    ) -> Result<Self, &'static str> {
        if summaries.len() > MAX_MEMORY_SUMMARIES {
            return Err("too many memory summaries");
        }
        if details.len() > MAX_MEMORY_DETAILS {
            return Err("too many memory details");
        }
        if usize::try_from(total).unwrap_or(usize::MAX) < summaries.len() {
            return Err("memory total cannot be smaller than the bounded page");
        }
        let summary_ids = summaries
            .iter()
            .map(MemorySummary::id)
            .collect::<BTreeSet<_>>();
        if summary_ids.len() != summaries.len() {
            return Err("memory summary identities must be unique");
        }
        let detail_ids = details
            .iter()
            .map(MemoryDetail::memory_id)
            .collect::<BTreeSet<_>>();
        if detail_ids.len() != details.len() {
            return Err("memory detail identities must be unique");
        }
        if details
            .iter()
            .any(|detail| !summary_ids.contains(detail.memory_id()))
        {
            return Err("memory detail has no matching summary");
        }
        Ok(Self {
            generation,
            state: MemoryLoadState::Ready,
            summaries,
            details,
            total,
            stale,
        })
    }

    #[must_use]
    pub fn summaries(&self) -> &[MemorySummary] {
        &self.summaries
    }

    #[must_use]
    pub fn detail(&self, memory_id: &str) -> Option<&MemoryDetail> {
        self.details
            .iter()
            .find(|detail| detail.memory_id() == memory_id)
    }

    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Monotonic ledger generation used to reject stale projections.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn stale(&self) -> bool {
        self.stale
    }

    #[must_use]
    pub const fn state(&self) -> &MemoryLoadState {
        &self.state
    }

    #[must_use]
    pub fn failure(&self) -> Option<&UiFailure> {
        match &self.state {
            MemoryLoadState::Failed(failure) => Some(failure),
            MemoryLoadState::Loading | MemoryLoadState::Ready => None,
        }
    }
}

impl Default for MemoryProjection {
    fn default() -> Self {
        Self {
            generation: 0,
            state: MemoryLoadState::Ready,
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
        }
    }
}

impl Debug for MemoryProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryProjection")
            .field("generation", &self.generation)
            .field("state", &self.state)
            .field("summary_count", &self.summaries.len())
            .field("detail_count", &self.details.len())
            .field("total", &self.total)
            .field("stale", &self.stale)
            .finish()
    }
}

fn bounded_single_line(
    value: String,
    max_chars: usize,
    field: &'static str,
) -> Result<String, &'static str> {
    if value.is_empty() {
        return Err(match field {
            "memory identity" => "memory identity must not be empty",
            "memory preview" => "memory preview must not be empty",
            "memory source" => "memory source must not be empty",
            "admission session" => "admission session must not be empty",
            "admission model" => "admission model must not be empty",
            _ => "admission reason must not be empty",
        });
    }
    if value.chars().count() > max_chars {
        return Err("memory presentation text is too long");
    }
    if value.chars().any(char::is_control) {
        return Err("memory presentation text must be one safe line");
    }
    Ok(value)
}

fn bounded_text(
    value: String,
    max_chars: usize,
    _field: &'static str,
) -> Result<String, &'static str> {
    if value.is_empty() {
        return Err("memory content must not be empty");
    }
    if value.chars().count() > max_chars {
        return Err("memory content is too long");
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("memory content contains unsafe control characters");
    }
    Ok(value)
}

/// A provider credential transferred from the TUI without persistence or serialization.
pub struct ApiCredential {
    raw: Zeroizing<String>,
}

impl ApiCredential {
    /// Creates a bounded visible-ASCII credential and keeps it in zeroizing memory.
    pub fn new(value: String) -> Result<Self, &'static str> {
        let raw = Zeroizing::new(value);
        if raw.is_empty() {
            return Err("API key must not be empty");
        }
        if raw.len() > MAX_CREDENTIAL_BYTES {
            return Err("API key is too long");
        }
        if !raw.chars().all(|character| character.is_ascii_graphic()) {
            return Err("API keys must contain visible ASCII characters only");
        }
        Ok(Self { raw })
    }

    /// Moves the credential into application composition.
    ///
    /// The caller must immediately transfer ownership into a redacting, zeroizing provider type.
    #[must_use]
    pub fn into_string(mut self) -> String {
        std::mem::take(&mut *self.raw)
    }
}

impl Debug for ApiCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiCredential([REDACTED])")
    }
}

/// Ephemeral, masked API-key editor state.
#[derive(Default)]
pub(crate) struct CredentialState {
    raw: Zeroizing<String>,
}

impl CredentialState {
    pub fn has_value(&self) -> bool {
        !self.raw.is_empty()
    }

    pub fn append_character(&mut self, character: char) -> Result<(), &'static str> {
        if !character.is_ascii_graphic() {
            return Err("API keys must contain visible ASCII characters only");
        }
        if self.raw.len().saturating_add(character.len_utf8()) > MAX_CREDENTIAL_BYTES {
            return Err("API key is too long");
        }
        self.raw.push(character);
        Ok(())
    }

    pub fn append_paste(&mut self, value: &str) -> Result<(), &'static str> {
        let value = value.trim_matches(char::is_whitespace);
        if value.is_empty() {
            return Err("Paste a non-empty API key");
        }
        if !value.chars().all(|character| character.is_ascii_graphic()) {
            return Err("API keys must contain visible ASCII characters only");
        }
        if self.raw.len().saturating_add(value.len()) > MAX_CREDENTIAL_BYTES {
            return Err("API key is too long");
        }
        self.raw.push_str(value);
        Ok(())
    }

    pub fn pop(&mut self) {
        self.raw.pop();
    }

    pub fn clear(&mut self) {
        self.raw.zeroize();
    }

    pub fn take(&mut self) -> ApiCredential {
        ApiCredential {
            raw: std::mem::take(&mut self.raw),
        }
    }
}

impl Debug for CredentialState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialState")
            .field("has_value", &self.has_value())
            .finish()
    }
}

/// Model-picker local state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PickerState {
    pub query: String,
    pub selected: Option<ModelRef>,
}

/// Transcript scrolling state measured upward from the tail.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TranscriptState {
    /// Whether new output keeps the bottom visible.
    pub follow_tail: bool,
    /// Wrapped rows held below the viewport while manually scrolled.
    pub rows_from_bottom: usize,
}

impl TranscriptState {
    pub(crate) fn new() -> Self {
        Self {
            follow_tail: true,
            rows_from_bottom: 0,
        }
    }
}

/// Unicode multiline composer state.
#[derive(Clone, Debug)]
pub struct ComposerState {
    pub(crate) editor: TextArea<'static>,
}

impl Default for ComposerState {
    fn default() -> Self {
        let mut editor = TextArea::default();
        editor.set_placeholder_text("Ask Agent...");
        editor.set_wrap_mode(WrapMode::WordOrGlyph);
        editor.set_cursor_line_style(Style::default());
        let mut state = Self { editor };
        state.apply_theme(&Theme::from_preset(
            ThemePreset::System,
            ColorMode::Color,
            ColorDepth::TrueColor,
        ));
        state
    }
}

impl ComposerState {
    fn apply_theme(&mut self, theme: &Theme) {
        self.editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Prompt ")
                .border_style(theme.style(Token::BorderSubtle)),
        );
        self.editor.set_cursor_style(theme.filled(Token::Accent));
    }
    /// Returns exact composer content using line-feed separators.
    #[must_use]
    pub fn text(&self) -> String {
        self.editor.lines().join("\n")
    }

    /// Returns whether the editor contains only whitespace.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    /// Returns editor lines for tests and composition.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.editor.lines()
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// An informational or safe failure banner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// Short non-failure information.
    Info(String),
    /// A safe failure.
    Failure(UiFailure),
}

/// One committed reversible lifecycle action awaiting possible undo.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UndoableLifecycle {
    /// Session the action applied to.
    pub session_id: String,
    /// Whether the committed action archived (true) or unarchived (false).
    pub archived: bool,
}

/// One user-layer local preference update requested by the Settings route.
///
/// `None` clears the user-layer leaf so the resolver inherits the next
/// applicable layer. The application validates and persists these values;
/// the TUI never accesses settings storage directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalPreferenceChange {
    /// Local display label; `None` inherits no local label.
    DisplayLabel(Option<String>),
    /// Terminal theme preset.
    ThemePreset(Option<ThemePreset>),
    /// Terminal color treatment.
    ColorMode(Option<ColorMode>),
    /// Terminal decoration character set.
    GlyphMode(Option<GlyphMode>),
    /// Whether animation is suppressed.
    ReducedMotion(Option<bool>),
    /// Terminal information density.
    Density(Option<Density>),
    /// Terminal panel layout.
    Layout(Option<Layout>),
    /// Transcript timestamp display.
    TerminalTimestampStyle(Option<TerminalTimestampStyle>),
    /// Composer submission chord.
    ComposerSubmitBehavior(Option<ComposerSubmitBehavior>),
    /// Prompt status-bar information density.
    PromptStatusDetail(Option<PromptStatusDetail>),
}

/// Kind of request awaiting application acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingKind {
    /// Durable creation and activation of a fresh session.
    CreateSession,
    /// Runtime provider credential configuration and catalog validation.
    ConfigureCredential,
    /// Model catalog refresh.
    RefreshCatalog,
    /// Model selection.
    SelectModel(ModelRef),
    /// Durable input admission.
    SubmitPrompt(String),
    /// Attempt cancellation.
    CancelAttempt(AttemptKey),
    /// Attempt retry.
    RetryAttempt(AttemptKey),
    /// Human response to one durable permission request.
    AnswerPermission(ToolCallKey),
    /// Durable rename of one session.
    RenameSession(String),
    /// Durable archive of one session.
    ArchiveSession(String),
    /// Durable unarchive of one session.
    UnarchiveSession(String),
    /// Confirmed deletion of one session.
    DeleteSession(String),
    /// Opening one durable session as the active session.
    OpenSession(String),
    /// Markdown export of the active session transcript.
    ExportTranscript,
    /// One profile create or edit request.
    UpsertProfile(ProviderProfileDraft),
    /// One profile duplication request.
    DuplicateProfile { source: String, destination: String },
    /// Active profile selection.
    ActivateProfile(String),
    /// First save of a profile credential.
    SaveProfileCredential(String),
    /// Replacement of a profile credential.
    ReplaceProfileCredential(String),
    /// Safe provider connection test.
    TestProfile(String),
    /// Selection of the active session model as this profile's default.
    SetProfileDefaultModel(String),
    /// One explicit profile and compatible model default.
    SetProfileDefault { profile_id: String, model: ModelRef },
    /// Stored credential disconnection.
    DisconnectProfile(String),
    /// One user-layer preference update from the Settings route.
    UpdateLocalPreference(LocalPreferenceChange),
    /// Confirmed profile deletion.
    DeleteProfile(String),
    /// Native Codex browser authentication.
    CodexLogin,
    /// Explicit user-authored memory creation.
    RememberMemory(MemoryContent),
    /// Explicit correction of one exact memory revision.
    ReviseMemory {
        memory_id: String,
        content: MemoryContent,
    },
    /// Deliberate approval of one exact proposed revision.
    ApproveMemoryProposal(String),
    /// Deliberate rejection of one exact proposed revision.
    RejectMemoryProposal(String),
    /// Stop admitting one exact active revision into future turns.
    RetractMemory(String),
    /// Logical deletion of one memory ledger identity.
    DeleteMemory(String),
    /// Export one exact loaded memory as a user-owned artifact.
    ExportMemory(String),
}

/// Runner-side side effects the pure update layer cannot perform itself.
#[derive(Debug)]
pub enum UiEffect {
    /// Dispatch an intent through the bounded application mailbox.
    Dispatch(UiIntent),
    /// Copy exact text to the system clipboard through OSC 52.
    CopyTranscript(String),
    /// Exit the terminal client.
    Quit,
}

/// Intent emitted by pure update logic and handled by application composition.
#[derive(Debug)]
pub enum UiIntent {
    /// Create and activate a fresh durable session.
    CreateSession { request_id: RequestId },
    /// Configure the provider from an ephemeral API key.
    ConfigureCredential {
        request_id: RequestId,
        credential: ApiCredential,
    },
    /// Start application-owned Codex subscription authentication.
    StartCodexLogin { request_id: RequestId },
    /// Cancel application-owned Codex subscription authentication.
    CancelCodexLogin { request_id: RequestId },
    /// Creates or edits one non-secret provider profile.
    UpsertProfile {
        request_id: RequestId,
        profile: ProviderProfileDraft,
    },
    /// Duplicates non-secret configuration without sharing a credential.
    DuplicateProfile {
        request_id: RequestId,
        source: String,
        destination: String,
    },
    /// Selects one profile as the active runtime provider.
    ActivateProfile {
        request_id: RequestId,
        profile_id: String,
    },
    /// Saves a first credential into the operating-system vault.
    SaveProfileCredential {
        request_id: RequestId,
        profile_id: String,
        credential: ApiCredential,
    },
    /// Replaces one exact stored profile credential.
    ReplaceProfileCredential {
        request_id: RequestId,
        profile_id: String,
        credential: ApiCredential,
    },
    /// Tests one profile without retaining provider content.
    TestProfile {
        request_id: RequestId,
        profile_id: String,
    },
    /// Uses the active session's selected model as the profile default.
    SetProfileDefaultModel {
        request_id: RequestId,
        profile_id: String,
    },
    /// Persists one explicit connected-provider model as that profile's default.
    SetProfileDefault {
        request_id: RequestId,
        profile_id: String,
        model: ModelRef,
        reasoning_effort: Option<String>,
    },
    /// Disconnects one stored profile credential.
    DisconnectProfile {
        request_id: RequestId,
        profile_id: String,
    },
    /// Deletes one confirmed provider profile and its vault entry.
    DeleteProfile {
        request_id: RequestId,
        profile_id: String,
    },
    /// Refresh the model catalog.
    RefreshCatalog { request_id: RequestId },
    /// Select a model for later turns.
    SelectModel {
        request_id: RequestId,
        model: ModelRef,
    },
    /// Admit a user prompt durably.
    SubmitPrompt {
        request_id: RequestId,
        prompt: String,
    },
    /// Request cancellation without assuming settlement.
    CancelAttempt {
        request_id: RequestId,
        attempt_id: AttemptKey,
    },
    /// Retry a settled attempt by identity.
    RetryAttempt {
        request_id: RequestId,
        attempt_id: AttemptKey,
    },
    /// Resolve one exact durable tool permission request.
    AnswerPermission {
        request_id: RequestId,
        tool_call_id: ToolCallKey,
        allow: bool,
    },
    /// Open one durable session by identity.
    OpenSession {
        request_id: RequestId,
        session_id: String,
    },
    /// Replace the title of one durable session.
    RenameSession {
        request_id: RequestId,
        session_id: String,
        title: String,
    },
    /// Retain a session but stop it from accepting ordinary commands.
    ArchiveSession {
        request_id: RequestId,
        session_id: String,
    },
    /// Return an archived session to ordinary command eligibility.
    UnarchiveSession {
        request_id: RequestId,
        session_id: String,
    },
    /// Permanently delete a confirmed session and its history.
    DeleteSession {
        request_id: RequestId,
        session_id: String,
    },
    /// Updates or clears one persisted user-layer local preference.
    UpdateLocalPreference {
        request_id: RequestId,
        change: LocalPreferenceChange,
    },
    /// Write the active session transcript to a Markdown file.
    ExportTranscript {
        request_id: RequestId,
        session_id: String,
    },
    /// Create an explicit user-authored memory.
    RememberMemory {
        request_id: RequestId,
        content: MemoryContent,
    },
    /// Correct one exact memory with optimistic sequence protection.
    ReviseMemory {
        request_id: RequestId,
        memory_id: String,
        expected_last_sequence: u64,
        content: MemoryContent,
    },
    /// Deliberately approve one exact proposed revision.
    ApproveMemoryProposal {
        request_id: RequestId,
        memory_id: String,
        expected_last_sequence: u64,
        proposal_revision_id: String,
    },
    /// Deliberately reject one exact proposed revision.
    RejectMemoryProposal {
        request_id: RequestId,
        memory_id: String,
        expected_last_sequence: u64,
        proposal_revision_id: String,
    },
    /// Stop one exact revision from admission into future turns.
    RetractMemory {
        request_id: RequestId,
        memory_id: String,
        expected_last_sequence: u64,
        revision_id: String,
    },
    /// Logically delete one exact memory identity.
    DeleteMemory {
        request_id: RequestId,
        memory_id: String,
        expected_last_sequence: u64,
    },
    /// Export one exact memory as a user-owned artifact.
    ExportMemory {
        request_id: RequestId,
        memory_id: String,
    },
}

impl UiIntent {
    /// Returns the local request identity.
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        match self {
            Self::CreateSession { request_id }
            | Self::ConfigureCredential { request_id, .. }
            | Self::StartCodexLogin { request_id }
            | Self::CancelCodexLogin { request_id }
            | Self::UpsertProfile { request_id, .. }
            | Self::DuplicateProfile { request_id, .. }
            | Self::ActivateProfile { request_id, .. }
            | Self::SaveProfileCredential { request_id, .. }
            | Self::ReplaceProfileCredential { request_id, .. }
            | Self::TestProfile { request_id, .. }
            | Self::SetProfileDefaultModel { request_id, .. }
            | Self::SetProfileDefault { request_id, .. }
            | Self::DisconnectProfile { request_id, .. }
            | Self::DeleteProfile { request_id, .. }
            | Self::RefreshCatalog { request_id }
            | Self::SelectModel { request_id, .. }
            | Self::SubmitPrompt { request_id, .. }
            | Self::CancelAttempt { request_id, .. }
            | Self::RetryAttempt { request_id, .. }
            | Self::AnswerPermission { request_id, .. }
            | Self::OpenSession { request_id, .. }
            | Self::RenameSession { request_id, .. }
            | Self::ArchiveSession { request_id, .. }
            | Self::UnarchiveSession { request_id, .. }
            | Self::DeleteSession { request_id, .. }
            | Self::UpdateLocalPreference { request_id, .. }
            | Self::ExportTranscript { request_id, .. }
            | Self::RememberMemory { request_id, .. }
            | Self::ReviseMemory { request_id, .. }
            | Self::ApproveMemoryProposal { request_id, .. }
            | Self::RejectMemoryProposal { request_id, .. }
            | Self::RetractMemory { request_id, .. }
            | Self::DeleteMemory { request_id, .. }
            | Self::ExportMemory { request_id, .. } => *request_id,
        }
    }
}

/// Application acknowledgement for a UI request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiNotice {
    /// The request crossed its durable commit boundary.
    IntentCommitted { request_id: RequestId },
    /// The request was rejected and is safe to present.
    IntentRejected {
        request_id: RequestId,
        failure: UiFailure,
    },
    /// The Codex authorization page was handed to the default browser.
    CodexLoginBrowserOpened { request_id: RequestId },
    /// Codex authentication completed and the connected profile is active.
    CodexLoginCompleted { request_id: RequestId },
}
/// Semantic mouse actions produced by terminal hit testing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MouseAction {
    /// Switch to one of the primary shell routes.
    Route(Route),
    /// Switch to one of the nested Settings tabs.
    SettingsTab(usize),
    /// Focus the Chat composer by clicking it.
    FocusComposer,
    /// Click the Chat transcript to inspect it without tail-follow.
    FocusTranscript,
    /// Open the model picker from the Chat status line.
    ChatModels,
    /// Retry the failed Chat attempt from a callout button.
    ChatRetry,
    /// Create a fresh session from a Chat recovery callout.
    ChatFreshSession,
    /// Select a General Settings preference row.
    SettingsRow(usize),
    /// Profile-center actions.
    ProfileCredential,
    ProfileTest,
    ProfileDefaultModel,
    ProfileDisconnect,
    ProfileDelete,
    /// Select a provider setup choice.
    SelectProviderChoice(usize),
    /// Start or retry the visible Codex browser sign-in action.
    CodexLogin,
    /// Cancel the visible Codex browser sign-in flow.
    CodexLoginCancel,
    /// Save or cancel the provider profile editor.
    ProfileEditorSubmit,
    ProfileEditorCancel,
    /// Select a connected provider profile.
    SelectProfile(String),
    UserProfileSave,
    /// Session-browser action bar controls.
    SessionOpen,
    SessionRename,
    SessionArchive,
    SessionDelete,
    /// Destructive confirmation controls.
    Confirm,
    Cancel,
    UserProfileCancel,
    /// Select a visible model-picker row.
    PickerSelect(ModelRef),
    /// Execute a visible command-palette row.
    PaletteRun(String),
    /// Credential and permission modal controls.
    CredentialSubmit,
    CredentialCancel,
    ProfileCredentialSubmit,
    ProfileCredentialCancel,
    /// Close a non-authority picker or palette modal.
    OverlayCancel,
    PermissionAllow,
    PermissionDeny,
    /// Focus the Memory search field.
    MemoryFocusSearch,
    /// Select one visible memory by stable identity.
    MemorySelect(String),
    /// Select one visible admission by bounded index.
    MemorySelectAdmission(usize),
    /// Cycle the status filter.
    MemoryCycleStatus,
    /// Cycle the scope filter.
    MemoryCycleScope,
    /// Open the selected memory detail on a compact layout.
    MemoryOpen,
    /// Return to the preceding compact Memory pane.
    MemoryBack,
    /// Inspect admissions for the selected memory.
    MemoryAdmissions,
    /// Open explicit memory creation.
    MemoryRemember,
    /// Open correction for the selected revision.
    MemoryRevise,
    /// Open deliberate review for the selected proposal.
    MemoryReview,
    /// Open the compact lifecycle action chooser.
    MemoryActions,
    /// Open retraction confirmation for the selected revision.
    MemoryRetract,
    /// Open logical-delete confirmation for the selected identity.
    MemoryDelete,
    /// Export the selected memory.
    MemoryExport,
    /// Choose one row in the Memory action overlay.
    MemoryActionSelect(usize),
    /// Submit the primary action in the Memory lifecycle overlay.
    MemoryLifecycleSubmit,
    /// Deliberately reject the proposal in review.
    MemoryProposalReject,
    /// Close the Memory lifecycle overlay and restore focus.
    MemoryLifecycleCancel,
}

/// Input to the deterministic update function.
pub enum Message {
    /// Backend-independent keyboard input.
    Input(ratatui_textarea::Input),
    /// Semantic click action derived from a visible terminal hit region.
    Mouse(MouseAction),
    /// Mouse-wheel movement over the Chat conversation, positive away from the tail.
    TranscriptScroll(i16),
    /// Bracketed paste content.
    Paste(String),
    /// Newest session projection.
    SessionChanged(Arc<SessionProjection>),
    /// Newest read model of every durable session.
    SessionsChanged(Arc<SessionsProjection>),
    /// Newest model catalog projection.
    CatalogChanged(Arc<CatalogProjection>),
    /// Newest local profile and provider connection projection.
    ProfilesChanged(Arc<ProfilesProjection>),
    /// Newest resolved settings and provenance projection.
    SettingsChanged(Arc<SettingsProjection>),
    /// Newest bounded memory inspection projection.
    MemoryChanged(Arc<MemoryProjection>),
    /// Application acknowledgement.
    Notice(UiNotice),
    /// Clock sample with monotonic time for animation and wall time for display.
    Tick(UiClock),
    /// Terminal resize notification.
    Resize,
    /// Process-level shutdown request.
    ShutdownRequested,
}

impl Debug for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(_) => formatter.write_str("Input([REDACTED])"),
            Self::Mouse(action) => formatter.debug_tuple("Mouse").field(action).finish(),
            Self::TranscriptScroll(rows) => formatter
                .debug_tuple("TranscriptScroll")
                .field(rows)
                .finish(),
            Self::Paste(_) => formatter.write_str("Paste([REDACTED])"),
            Self::SessionChanged(session) => formatter
                .debug_tuple("SessionChanged")
                .field(session)
                .finish(),
            Self::SessionsChanged(sessions) => formatter
                .debug_tuple("SessionsChanged")
                .field(sessions)
                .finish(),
            Self::ProfilesChanged(profiles) => formatter
                .debug_tuple("ProfilesChanged")
                .field(profiles)
                .finish(),
            Self::CatalogChanged(catalog) => formatter
                .debug_tuple("CatalogChanged")
                .field(catalog)
                .finish(),
            Self::SettingsChanged(settings) => formatter
                .debug_tuple("SettingsChanged")
                .field(settings)
                .finish(),
            Self::MemoryChanged(memory) => formatter
                .debug_tuple("MemoryChanged")
                .field(memory)
                .finish(),
            Self::Notice(notice) => formatter.debug_tuple("Notice").field(notice).finish(),
            Self::Tick(clock) => formatter.debug_tuple("Tick").field(clock).finish(),
            Self::Resize => formatter.write_str("Resize"),
            Self::ShutdownRequested => formatter.write_str("ShutdownRequested"),
        }
    }
}

/// Session-browser local state.
#[derive(Debug, Default)]
pub(crate) struct BrowserState {
    pub query: String,
    /// Stable identity of the highlighted row.
    pub selected: Option<String>,
    /// When set, the highlighted row awaits a typed replacement title.
    pub renaming: bool,
    pub rename_buffer: String,
    /// When set, deletion of this identity is awaiting explicit confirmation.
    pub confirming_delete: Option<String>,
    /// When set, archiving this identity is awaiting explicit confirmation.
    pub confirming_archive: Option<String>,
}

/// Coarse status filter applied locally to the bounded Memory page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MemoryStatusFilter {
    #[default]
    Eligible,
    All,
    Active,
    Proposed,
    Inactive,
}

impl MemoryStatusFilter {
    pub const ALL: [Self; 5] = [
        Self::Eligible,
        Self::All,
        Self::Active,
        Self::Proposed,
        Self::Inactive,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::All => "all states",
            Self::Active => "active",
            Self::Proposed => "proposed",
            Self::Inactive => "inactive",
        }
    }

    #[must_use]
    pub fn includes(self, status: MemoryStatus) -> bool {
        match self {
            Self::Eligible => status == MemoryStatus::Active,
            Self::All => true,
            Self::Active => status == MemoryStatus::Active,
            Self::Proposed => status == MemoryStatus::Proposed,
            Self::Inactive => {
                matches!(
                    status,
                    MemoryStatus::Superseded
                        | MemoryStatus::Rejected
                        | MemoryStatus::Retracted
                        | MemoryStatus::Deleted
                )
            }
        }
    }
}

/// Scope filter applied locally to the bounded Memory page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MemoryScopeFilter {
    #[default]
    All,
    User,
    Workspace,
    Session,
    Agent,
}

impl MemoryScopeFilter {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::User,
        Self::Workspace,
        Self::Session,
        Self::Agent,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "all scopes",
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Session => "session",
            Self::Agent => "agent",
        }
    }

    #[must_use]
    pub fn includes(self, scope: MemoryScope) -> bool {
        match self {
            Self::All => true,
            Self::User => scope == MemoryScope::User,
            Self::Workspace => scope == MemoryScope::Workspace,
            Self::Session => scope == MemoryScope::Session,
            Self::Agent => scope == MemoryScope::Agent,
        }
    }
}

/// Compact-page drill-down destination.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MemoryPane {
    #[default]
    List,
    Detail,
    Admissions,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MemoryWorkspaceFocus {
    Search,
    Status,
    Scope,
    #[default]
    List,
    Detail,
    Admissions,
}

/// Deterministic local state for Memory search, filters, selection, and drill-down.
#[derive(Debug, Default)]
pub(crate) struct MemoryState {
    pub query: String,
    pub selected: Option<String>,
    pub status: MemoryStatusFilter,
    pub scope: MemoryScopeFilter,
    pub pane: MemoryPane,
    pub focus: MemoryWorkspaceFocus,
    pub admission_selected: usize,
}

/// Workflow shown by the single Memory lifecycle overlay owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryLifecycleMode {
    Remember,
    Revise,
    Review,
    Actions,
    Retract,
    Delete,
    Export,
}

impl MemoryLifecycleMode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Remember => "Remember",
            Self::Revise => "Correct memory",
            Self::Review => "Review proposal",
            Self::Actions => "Memory actions",
            Self::Retract => "Retract memory",
            Self::Delete => "Delete memory",
            Self::Export => "Export memory",
        }
    }
}

/// Exact selected revision captured when a lifecycle workflow opens.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct MemoryTargetSnapshot {
    pub memory_id: String,
    pub status: MemoryStatus,
    pub scope: MemoryScope,
    pub revision: u32,
    pub content: Option<MemoryContent>,
    pub source: String,
    pub trust: MemoryTrust,
    pub revision_context: Option<MemoryRevisionContext>,
}

impl Debug for MemoryTargetSnapshot {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryTargetSnapshot")
            .field("memory_id", &"[REDACTED]")
            .field("status", &self.status)
            .field("scope", &self.scope)
            .field("revision", &self.revision)
            .field("content", &self.content)
            .field("source", &"[REDACTED]")
            .field("trust", &self.trust)
            .field("revision_context", &self.revision_context)
            .finish()
    }
}

/// Bounded append-only editor state that redacts exact memory content from Debug.
#[derive(Default)]
pub(crate) struct MemoryDraftEditor {
    raw: Zeroizing<String>,
}

impl MemoryDraftEditor {
    pub fn from_content(content: &MemoryContent) -> Self {
        Self {
            raw: Zeroizing::new(content.as_str().to_owned()),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.raw
    }

    #[must_use]
    pub fn char_count(&self) -> usize {
        self.raw.chars().count()
    }

    pub fn append_character(&mut self, character: char) -> Result<(), &'static str> {
        if character.is_control() && character != '\n' {
            return Err("memory content contains unsafe control characters");
        }
        if self.char_count() >= MAX_MEMORY_CONTENT_CHARS {
            return Err("memory content is too long");
        }
        self.raw.push(character);
        Ok(())
    }

    pub fn append_text(&mut self, value: &str) -> Result<(), &'static str> {
        let value = value.replace("\r\n", "\n").replace('\r', "\n");
        if value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        {
            return Err("memory content contains unsafe control characters");
        }
        if self.char_count().saturating_add(value.chars().count()) > MAX_MEMORY_CONTENT_CHARS {
            return Err("memory content is too long");
        }
        self.raw.push_str(&value);
        Ok(())
    }

    pub fn pop(&mut self) {
        self.raw.pop();
    }

    pub fn content(&self) -> Result<MemoryContent, &'static str> {
        MemoryContent::new(self.raw.as_str())
    }
}

impl Debug for MemoryDraftEditor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryDraftEditor")
            .field("content", &"[REDACTED]")
            .field("character_count", &self.char_count())
            .finish()
    }
}

/// Local, mutually exclusive Memory lifecycle dialog state.
#[derive(Debug)]
pub(crate) struct MemoryLifecycleState {
    pub mode: MemoryLifecycleMode,
    pub target: Option<MemoryTargetSnapshot>,
    pub editor: Option<MemoryDraftEditor>,
    pub action_selected: usize,
    pub pending_request: Option<RequestId>,
    pub scroll: u16,
}

/// Profile editor operation currently shown inside the profile center.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileEditorMode {
    Create,
    Edit,
    Duplicate,
}

/// Non-secret profile form state.
#[derive(Debug)]
pub(crate) struct ProfileEditorState {
    pub mode: ProfileEditorMode,
    pub source_id: Option<String>,
    pub field: usize,
    pub id: String,
    pub kind: ProviderKindLabel,
    pub base_url: String,
    pub project: String,
    pub auth_header: String,
}

impl ProfileEditorState {
    pub fn field_count(&self) -> usize {
        match self.kind {
            ProviderKindLabel::Gemini | ProviderKindLabel::CodexCli => 2,
            ProviderKindLabel::Router => 5,
        }
    }
}

/// Whether credential entry saves a first value or replaces an existing one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileCredentialAction {
    Save,
    Replace,
}

/// Masked profile credential editor state.
pub(crate) struct ProfileCredentialEditor {
    pub profile_id: String,
    pub action: ProfileCredentialAction,
    raw: Zeroizing<String>,
}

impl Debug for ProfileCredentialEditor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProfileCredentialEditor")
            .field("profile_id", &self.profile_id)
            .field("action", &self.action)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl ProfileCredentialEditor {
    pub fn new(profile_id: String, action: ProfileCredentialAction) -> Self {
        Self {
            profile_id,
            action,
            raw: Zeroizing::new(String::new()),
        }
    }

    pub fn has_value(&self) -> bool {
        !self.raw.is_empty()
    }

    pub fn append_character(&mut self, character: char) -> Result<(), &'static str> {
        if !character.is_ascii_graphic() {
            return Err("API keys must contain visible ASCII characters only");
        }
        if self.raw.len().saturating_add(character.len_utf8()) > MAX_CREDENTIAL_BYTES {
            return Err("API key is too long");
        }
        self.raw.push(character);
        Ok(())
    }

    pub fn append_paste(&mut self, value: &str) -> Result<(), &'static str> {
        let value = value.trim_matches(char::is_whitespace);
        if value.is_empty() {
            return Err("Paste a non-empty API key");
        }
        if !value.chars().all(|character| character.is_ascii_graphic()) {
            return Err("API keys must contain visible ASCII characters only");
        }
        if self.raw.len().saturating_add(value.len()) > MAX_CREDENTIAL_BYTES {
            return Err("API key is too long");
        }
        self.raw.push_str(value);
        Ok(())
    }

    pub fn pop(&mut self) {
        self.raw.pop();
    }

    pub fn take(&mut self) -> Result<ApiCredential, &'static str> {
        let raw = std::mem::take(&mut *self.raw);
        ApiCredential::new(raw)
    }
}

impl Drop for ProfileCredentialEditor {
    fn drop(&mut self) {
        self.raw.zeroize();
    }
}

/// One provider choice exposed by the terminal connection catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderChoice {
    Gemini,
    GoogleAiStudio,
    Cursor,
    Codex,
    ClaudeCode,
    OpenAiCompatible,
}

pub(crate) const PROVIDER_CHOICES: [ProviderChoice; 6] = [
    ProviderChoice::Gemini,
    ProviderChoice::GoogleAiStudio,
    ProviderChoice::Cursor,
    ProviderChoice::Codex,
    ProviderChoice::ClaudeCode,
    ProviderChoice::OpenAiCompatible,
];

impl ProviderChoice {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Gemini => "Gemini",
            Self::GoogleAiStudio => "Google AI Studio API",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::OpenAiCompatible => "OpenAI-compatible API",
        }
    }
}

/// Which list owns arrow-key focus in the Providers workspace.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ProfileCenterFocus {
    /// Add or configure a provider connection.
    #[default]
    ProviderChoices,
    /// Inspect or manage an existing provider profile.
    ConnectedProfiles,
}

/// Observable state of the native Codex browser authentication handoff.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum CodexLoginState {
    /// No login process is running.
    #[default]
    Idle,
    /// AutoHarness is checking the existing CLI session or starting login.
    Starting,
    /// The official authentication page was opened in the default browser.
    BrowserOpened,
    /// The last launch failed and may be retried.
    Failed,
}

/// Provider-choice, authentication-page, and account-editor state.
#[derive(Debug, Default)]
pub(crate) struct ProfileCenterState {
    pub focus: ProfileCenterFocus,
    pub choice_selected: usize,
    pub auth_page: Option<ProviderChoice>,
    pub codex_login: CodexLoginState,
    pub open_credential_after_save: Option<String>,
    pub query: String,
    pub selected: Option<String>,
    pub confirming_disconnect: Option<String>,
    pub editor: Option<ProfileEditorState>,
    pub credential: Option<ProfileCredentialEditor>,
    pub confirming_delete: Option<String>,
}

/// Step in the keyboard-first default-model selection sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ModelDefaultStep {
    /// Select one compatible model from the active provider catalog.
    #[default]
    Model,
    /// Select the provider-native thinking effort saved with the model.
    Thinking,
}

/// Provider-neutral thinking choices persisted with a profile default.
pub(crate) const MODEL_THINKING_LEVELS: [&str; 7] = [
    "provider default",
    "none",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
];

/// Local state for selecting the active profile's default model and thinking mode.
#[derive(Debug, Default)]
pub(crate) struct ModelDefaultsState {
    pub step: ModelDefaultStep,
    pub model_selected: usize,
    pub model: Option<ModelRef>,
    pub thinking_selected: usize,
}
/// Command-palette local state.
#[derive(Debug, Default)]
pub(crate) struct PaletteState {
    pub query: String,
    /// Stable identity of the highlighted command.
    pub selected: Option<&'static str>,
}

/// Contextual help local state.
#[derive(Debug, Default)]
pub(crate) struct HelpState {
    /// Rows scrolled from the top of the help content.
    pub scroll: u16,
}
/// One category in the two-pane Settings workspace.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SettingsCategory {
    #[default]
    Appearance,
    ChatComposer,
    Accessibility,
    Providers,
    ModelsThinking,
    Profile,
    SessionsData,
    Shortcuts,
    About,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 9] = [
        Self::Appearance,
        Self::ChatComposer,
        Self::Accessibility,
        Self::Providers,
        Self::ModelsThinking,
        Self::Profile,
        Self::SessionsData,
        Self::Shortcuts,
        Self::About,
    ];

    #[must_use]
    pub(crate) fn at(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len().saturating_sub(1))]
    }

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::ChatComposer => "Chat & Composer",
            Self::Accessibility => "Accessibility",
            Self::Providers => "Providers",
            Self::ModelsThinking => "Models & Thinking",
            Self::Profile => "Profile",
            Self::SessionsData => "Sessions & Data",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }
}

/// Typed rows in deterministic Settings workspace order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsPreference {
    DisplayLabel,
    Provider,
    Profile,
    Credential,
    Source,
    Model,
    Mode,
    ThemePreset,
    ColorMode,
    GlyphMode,
    PromptStatusDetail,
    ReducedMotion,
    Density,
    Approvals,
    Retention,
    Logging,
    Layout,
    TerminalTimestampStyle,
    ComposerSubmitBehavior,
    GlyphCheck,
    KeyboardNavigation,
    StateIndicators,
    Workspace,
    ColorDepth,
    Version,
    ManageProviders,
    ConnectCredential,
    ConfigureModels,
    OpenSessions,
    OpenMemory,
}

impl SettingsPreference {
    const APPEARANCE: [Self; 4] = [
        Self::ThemePreset,
        Self::Density,
        Self::GlyphMode,
        Self::GlyphCheck,
    ];
    const CHAT_COMPOSER: [Self; 4] = [
        Self::PromptStatusDetail,
        Self::Layout,
        Self::TerminalTimestampStyle,
        Self::ComposerSubmitBehavior,
    ];
    const ACCESSIBILITY: [Self; 4] = [
        Self::ReducedMotion,
        Self::ColorMode,
        Self::KeyboardNavigation,
        Self::StateIndicators,
    ];
    const PROVIDERS: [Self; 5] = [
        Self::Provider,
        Self::Credential,
        Self::Source,
        Self::ConnectCredential,
        Self::ManageProviders,
    ];
    const MODELS_THINKING: [Self; 3] = [Self::Model, Self::Mode, Self::ConfigureModels];
    const PROFILE: [Self; 4] = [
        Self::Profile,
        Self::Workspace,
        Self::DisplayLabel,
        Self::ManageProviders,
    ];
    const SESSIONS_DATA: [Self; 4] = [
        Self::Retention,
        Self::Logging,
        Self::OpenSessions,
        Self::OpenMemory,
    ];
    const SHORTCUTS: [Self; 0] = [];
    const ABOUT: [Self; 3] = [Self::Approvals, Self::ColorDepth, Self::Version];

    #[must_use]
    pub(crate) const fn rows(category: SettingsCategory) -> &'static [Self] {
        match category {
            SettingsCategory::Appearance => &Self::APPEARANCE,
            SettingsCategory::ChatComposer => &Self::CHAT_COMPOSER,
            SettingsCategory::Accessibility => &Self::ACCESSIBILITY,
            SettingsCategory::Providers => &Self::PROVIDERS,
            SettingsCategory::ModelsThinking => &Self::MODELS_THINKING,
            SettingsCategory::Profile => &Self::PROFILE,
            SettingsCategory::SessionsData => &Self::SESSIONS_DATA,
            SettingsCategory::Shortcuts => &Self::SHORTCUTS,
            SettingsCategory::About => &Self::ABOUT,
        }
    }

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DisplayLabel => "Display label",
            Self::Provider => "Provider",
            Self::Profile => "Active profile",
            Self::Credential => "Connection",
            Self::Source => "Credential source",
            Self::Model => "Default model",
            Self::Mode => "Thinking",
            Self::ThemePreset => "Theme",
            Self::ColorMode => "Color mode",
            Self::GlyphMode => "Glyph mode",
            Self::PromptStatusDetail => "Prompt detail",
            Self::ReducedMotion => "Reduced motion",
            Self::Density => "Density",
            Self::Approvals => "Approvals",
            Self::Retention => "Retention",
            Self::Logging => "Logging",
            Self::Layout => "Layout",
            Self::TerminalTimestampStyle => "Timestamps",
            Self::ComposerSubmitBehavior => "Submit prompt",
            Self::GlyphCheck => "Glyph check",
            Self::KeyboardNavigation => "Keyboard navigation",
            Self::StateIndicators => "State indicators",
            Self::Workspace => "Workspace",
            Self::ColorDepth => "Terminal colors",
            Self::Version => "Version",
            Self::ManageProviders => "Provider profiles",
            Self::ConnectCredential => "API credential",
            Self::ConfigureModels => "Model defaults",
            Self::OpenSessions => "Session browser",
            Self::OpenMemory => "Memory workspace",
        }
    }

    #[must_use]
    pub(crate) const fn editable(self) -> bool {
        !matches!(
            self,
            Self::Provider
                | Self::Profile
                | Self::Credential
                | Self::Source
                | Self::Model
                | Self::Mode
                | Self::Approvals
                | Self::Retention
                | Self::Logging
                | Self::GlyphCheck
                | Self::KeyboardNavigation
                | Self::StateIndicators
                | Self::Workspace
                | Self::ColorDepth
                | Self::Version
        )
    }
}

/// Inline state owned exclusively by the Settings route.
#[derive(Debug, Default)]
pub(crate) struct SettingsState {
    /// Whether the category rail owns arrow-key focus.
    pub nav_focus: bool,
    /// Index into `SettingsCategory::ALL`.
    pub nav_selected: usize,
    /// Index into the selected category's row slice.
    pub selected: usize,
    /// First typed row kept visible while selecting preferences.
    pub scroll: u16,
    /// Buffered local-label edit; no value is persisted until Enter.
    pub display_label_editor: Option<String>,
    /// Inline Settings search query.
    pub search_query: String,
    /// Whether cross-category Settings search owns text input.
    pub search_active: bool,
    /// Highlighted result in the filtered Settings search.
    pub search_selected: usize,
    /// Whether the existing model-default workflow is open inside its category.
    pub detail_open: bool,
    /// Whether the focused long choice is showing its inline picker.
    pub choice_picker_open: bool,
    /// Highlighted option in the inline long-choice picker.
    pub choice_picker_selected: usize,
}

pub(crate) const SETTINGS_NAV_COUNT: usize = SettingsCategory::ALL.len();

/// Local user-profile dialog state.
#[derive(Debug, Default)]
pub(crate) struct UserProfileState {
    /// Buffered display-label edit; persisted only after Save.
    pub display_label_editor: Option<String>,
}

/// One contextual help section shown in the help overlay.
#[derive(Clone, Copy)]
pub(crate) struct HelpSection {
    /// Section heading, such as `Global` or `Composer`.
    pub title: &'static str,
    /// Ordered key-and-description rows.
    pub rows: &'static [(&'static str, &'static str)],
}

/// The complete static help content.
///
/// Sections are rendered in order; the focused surface's section is
/// highlighted by presentation logic.
pub(crate) const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Global",
        rows: &[
            (
                "Alt+1..6",
                "switch Chat, Sessions, Profiles, Settings, Help, Memory",
            ),
            (
                "Ctrl+S",
                "send the prompt by default; configurable in Settings",
            ),
            ("Ctrl+N", "create a fresh session"),
            ("Ctrl+L", "open Sessions"),
            ("Ctrl+G", "open Providers"),
            ("Alt+U", "edit the local user profile"),
            ("Ctrl+P", "choose a model"),
            ("Ctrl+K", "connect or replace the API key"),
            ("Ctrl+,", "show settings provenance"),
            ("Ctrl+R", "retry the failed attempt"),
            ("Ctrl+F", "search the transcript"),
            ("Ctrl+X", "expand or collapse tool rows"),
            ("Ctrl+Y", "copy the visible transcript"),
            ("Ctrl+Z", "undo the latest archive or unarchive"),
            ("Esc", "cancel streaming output"),
            ("PgUp/PgDn", "scroll the transcript; mouse wheel also works"),
            ("Ctrl+End", "follow live output again"),
            ("Ctrl+/", "command palette"),
            ("F1", "this help"),
            ("Ctrl+C", "quit when idle; cancel an active response"),
        ],
    },
    HelpSection {
        title: "Composer",
        rows: &[
            ("Enter", "new line"),
            ("Type", "write the prompt"),
            ("Paste", "insert multiline text"),
            ("Esc", "cancel streaming output"),
            ("PgUp/PgDn", "scroll the transcript; mouse wheel also works"),
        ],
    },
    HelpSection {
        title: "Browser",
        rows: &[
            ("Type", "filter sessions"),
            ("Up/Down", "choose a session"),
            ("Enter", "open the session"),
            ("Ctrl+R", "rename the session"),
            ("Ctrl+A", "archive or unarchive"),
            ("Ctrl+D", "delete with confirmation"),
            ("Esc", "close the browser"),
        ],
    },
    HelpSection {
        title: "Profiles",
        rows: &[
            ("Type", "filter provider profiles"),
            ("Up/Down", "choose a profile"),
            ("Enter", "activate the profile"),
            ("Alt+N / Alt+E", "create or edit"),
            ("Alt+D", "duplicate without a credential"),
            ("Alt+K / Alt+T", "manage credential or test"),
            ("Alt+M", "use the selected model as profile default"),
            ("Alt+X / Delete", "disconnect or delete"),
            ("Esc", "close or cancel the current form"),
        ],
    },
    HelpSection {
        title: "Models",
        rows: &[
            ("Type", "filter models"),
            ("Up/Down", "choose a model"),
            ("Enter", "select the model"),
            ("Ctrl+R", "refresh the catalog"),
            ("Esc", "close the picker"),
        ],
    },
    HelpSection {
        title: "Settings",
        rows: &[
            ("Tab / Shift+Tab", "move between categories and rows"),
            ("Up/Down", "choose a category or editable row"),
            ("Left/Right", "change the focused row value"),
            ("Enter / Space", "edit, toggle, or activate the focused row"),
            ("Ctrl+F", "search every category by label or value"),
            ("Backspace", "inherit when a user override exists"),
            ("Shift+Backspace", "restore the built-in default"),
            ("Esc", "step back one Settings level"),
        ],
    },
    HelpSection {
        title: "Memory",
        rows: &[
            ("/", "focus memory search"),
            ("Tab / Shift+Tab", "move between search, filters, and panes"),
            ("Up/Down", "choose a memory or admission"),
            ("Left/Right", "change the focused filter"),
            ("Enter", "open details or admission history"),
            (
                "Alt+N",
                "remember a Workspace Fact with Internal sensitivity",
            ),
            (
                "Alt+E / Alt+V",
                "correct an active revision or review a proposal",
            ),
            (
                "Alt+A",
                "open every available action for the exact revision",
            ),
            (
                "Alt+X / Alt+D",
                "retract future admission or logically delete",
            ),
            ("Alt+S", "export the selected loaded memory"),
            ("Ctrl+S", "save an open Remember or Correct editor"),
            ("Esc", "step back, clear search, or return to Chat"),
        ],
    },
    HelpSection {
        title: "User profile",
        rows: &[
            ("Type", "edit the local display name"),
            ("Enter / Ctrl+S", "save the local profile"),
            ("Esc", "cancel without saving"),
        ],
    },
    HelpSection {
        title: "Permission",
        rows: &[
            ("Y", "allow this exact call once"),
            ("N or Esc", "deny"),
            ("Up/Down", "inspect request details"),
        ],
    },
];

impl HelpSection {
    /// Returns whether this section describes the given focus surface.
    #[must_use]
    pub(crate) fn matches_focus(&self, focus: Focus) -> bool {
        match self.title {
            "Global" => true,
            "Composer" => matches!(focus, Focus::Composer | Focus::Credential | Focus::Help),
            "Browser" => focus == Focus::Browser,
            "Profiles" => focus == Focus::Profiles,
            "Models" => focus == Focus::Picker,
            "Settings" => focus == Focus::Settings,
            "Memory" => matches!(focus, Focus::Memory | Focus::MemoryLifecycle),
            "User profile" => focus == Focus::UserProfile,
            "Permission" => focus == Focus::Permission,
            _ => false,
        }
    }
}

/// Transcript search local state.
#[derive(Debug, Default)]
pub(crate) struct SearchState {
    pub query: String,
    /// Wrapped row indexes (in renderer coordinates) of matches.
    pub matches: Vec<usize>,
    /// Position within `matches`; `None` before the first Enter.
    pub current: Option<usize>,
}

/// One executable command shared by the palette, slash commands, and keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandEntry {
    /// Stable command identity, also the slash spelling without the slash.
    pub id: &'static str,
    /// Human-oriented palette label.
    pub label: &'static str,
    /// One-line description shown in the palette.
    pub description: &'static str,
    /// Primary key chord that runs the same action, when one exists.
    pub key_hint: Option<&'static str>,
}

/// The complete, ordered command table.
///
/// Every user-facing action path (key, palette row, slash command) resolves to
/// one entry here so all paths converge on the same behavior and intents.
pub const COMMANDS: &[CommandEntry] = &[
    CommandEntry {
        id: "chat",
        label: "Chat",
        description: "Return to the conversation workspace",
        key_hint: Some("Alt+1"),
    },
    CommandEntry {
        id: "sessions",
        label: "Sessions",
        description: "Browse, rename, archive, or delete sessions",
        key_hint: Some("Alt+2"),
    },
    CommandEntry {
        id: "profile",
        label: "Profile settings",
        description: "Open the Profile tab in Settings",
        key_hint: Some("Alt+3"),
    },
    CommandEntry {
        id: "provider",
        label: "Provider settings",
        description: "Open the Providers tab in Settings",
        key_hint: None,
    },
    CommandEntry {
        id: "models",
        label: "Default model",
        description: "Choose the model and thinking mode for new sessions",
        key_hint: None,
    },
    CommandEntry {
        id: "user",
        label: "User profile",
        description: "Edit the local display name and profile summary",
        key_hint: Some("Alt+U"),
    },
    CommandEntry {
        id: "new",
        label: "New session",
        description: "Create and activate a fresh durable session",
        key_hint: Some("Ctrl+N"),
    },
    CommandEntry {
        id: "session-model",
        label: "Session model",
        description: "Choose the model used by the current session",
        key_hint: Some("Ctrl+P"),
    },
    CommandEntry {
        id: "refresh",
        label: "Refresh models",
        description: "Reload the model catalog from the provider",
        key_hint: Some("Ctrl+R in the picker"),
    },
    CommandEntry {
        id: "connect",
        label: "Connect API key",
        description: "Enter or replace the provider API key",
        key_hint: Some("Ctrl+K"),
    },
    CommandEntry {
        id: "retry",
        label: "Retry response",
        description: "Retry the latest failed or cancelled response",
        key_hint: Some("Ctrl+R"),
    },
    CommandEntry {
        id: "cancel",
        label: "Cancel response",
        description: "Cancel the active response",
        key_hint: Some("Esc"),
    },
    CommandEntry {
        id: "search",
        label: "Search transcript",
        description: "Search the current conversation",
        key_hint: Some("Ctrl+F"),
    },
    CommandEntry {
        id: "tools",
        label: "Toggle tool details",
        description: "Expand or collapse tool resources",
        key_hint: Some("Ctrl+X"),
    },
    CommandEntry {
        id: "settings",
        label: "Settings",
        description: "Show effective provider, profile, and credential source",
        key_hint: Some("Alt+4"),
    },
    CommandEntry {
        id: "help",
        label: "Help",
        description: "Show keybindings and guidance for the current focus",
        key_hint: Some("Alt+5"),
    },
    CommandEntry {
        id: "memory",
        label: "Memory",
        description: "Inspect admitted memories, provenance, and usage",
        key_hint: Some("Alt+6"),
    },
    CommandEntry {
        id: "remember",
        label: "Remember",
        description: "Create an explicit workspace fact in Memory",
        key_hint: Some("Alt+N in Memory"),
    },
    CommandEntry {
        id: "memory-actions",
        label: "Memory actions",
        description: "Review, correct, retract, delete, or export the selected memory",
        key_hint: Some("Alt+A in Memory"),
    },
    CommandEntry {
        id: "memory-export",
        label: "Export memory",
        description: "Export the selected exact loaded memory revision",
        key_hint: Some("Alt+S in Memory"),
    },
    CommandEntry {
        id: "copy",
        label: "Copy transcript",
        description: "Copy the visible transcript to the system clipboard",
        key_hint: Some("Ctrl+Y"),
    },
    CommandEntry {
        id: "export",
        label: "Export transcript",
        description: "Save this session as Markdown beside the database",
        key_hint: None,
    },
    CommandEntry {
        id: "commands",
        label: "Commands",
        description: "Open this command palette",
        key_hint: Some("Ctrl+/"),
    },
];

/// Safe provider-kind label surfaced by application composition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProviderKindLabel {
    /// Google AI Studio Gemini.
    #[default]
    Gemini,
    /// Configurable OpenAI-compatible router.
    Router,
    /// User-owned Codex subscription session.
    CodexCli,
}

impl ProviderKindLabel {
    /// Returns the stable lowercase label shown to users.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Router => "router",
            Self::CodexCli => "codex subscription",
        }
    }
}

/// Safe credential-source label surfaced by application composition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CredentialSourceLabel {
    /// A process environment variable supplied the credential.
    Environment,
    /// The operating-system credential vault resolved a profile reference.
    CredentialVault,
    /// Nothing persisted; session-only entry applies.
    #[default]
    SessionOnly,
}

impl CredentialSourceLabel {
    /// Returns the stable lowercase label shown to users.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::CredentialVault => "credential vault",
            Self::SessionOnly => "session only",
        }
    }
}

/// Read-only view of effective provider and credential status.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderStatusProjection {
    /// Active profile name, when one is configured.
    pub active_profile: Option<String>,
    /// Provider adapter selected by the active profile.
    pub provider_kind: Option<ProviderKindLabel>,
    /// Effective credential source in safe terms.
    pub credential_source: CredentialSourceLabel,
    /// Whether a usable credential is currently connected.
    pub credential_connected: bool,
}

/// Read model of resolved settings for display and provenance.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsProjection {
    /// Effective provider and credential status.
    pub provider_status: ProviderStatusProjection,
    /// Effective local profile preferences and provenance for every leaf.
    pub local_profile: EffectiveLocalProfile,
    /// Safe current Git branch for the workspace, when the workspace is a checkout.
    pub git_branch: Option<String>,
}

impl SettingsProjection {
    /// Returns the effective provider label in safe provenance terms.
    #[must_use]
    pub fn provider_label(&self) -> String {
        let status = &self.provider_status;
        match (&status.active_profile, status.provider_kind) {
            (Some(profile), Some(kind)) => format!("{} via '{}'", kind.as_str(), profile),
            (Some(profile), None) => format!("profile '{profile}'"),
            (None, Some(kind)) => kind.as_str().to_owned(),
            (None, None) => "gemini (default)".to_owned(),
        }
    }

    /// Returns the effective credential label in safe provenance terms.
    #[must_use]
    pub fn credential_label(&self) -> String {
        let status = &self.provider_status;
        match (status.credential_source, status.credential_connected) {
            (_, false) if status.active_profile.is_some() => {
                "session only; press Ctrl+K to connect".to_owned()
            }
            (source, _) => source.as_str().to_owned(),
        }
    }
}
/// Stored credential status for one named provider profile.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProfileCredentialStateLabel {
    /// No operating-system vault entry is linked.
    #[default]
    Disconnected,
    /// One operating-system vault entry is linked.
    Stored,
    /// A restart-safe cross-store operation still needs cleanup.
    RecoveryPending,
}

impl ProfileCredentialStateLabel {
    /// Returns the stable user-facing status label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Stored => "stored",
            Self::RecoveryPending => "repair pending",
        }
    }
}

/// Safe result of the latest connection test for one profile.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProfileConnectionState {
    /// This profile has not been tested in the current process.
    #[default]
    Untested,
    /// A content-free catalog test is in flight.
    Testing,
    /// The selected provider accepted the profile and credential.
    Ready,
    /// The test failed with one bounded safe reason.
    Failed(String),
}

impl ProfileConnectionState {
    /// Returns a compact connection label.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Untested => "not tested",
            Self::Testing => "testing",
            Self::Ready => "connected",
            Self::Failed(_) => "test failed",
        }
    }
}

/// One safe named provider profile projected into the terminal.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderProfileProjection {
    /// Validated profile identity.
    pub id: String,
    /// Selected provider adapter.
    pub kind: ProviderKindLabel,
    /// Whether this profile selects the runtime provider.
    pub active: bool,
    /// Router base URL, or empty for Gemini.
    pub base_url: String,
    /// Optional router project identity.
    pub project: String,
    /// Optional router sensitive-header name.
    pub auth_header: String,
    /// Whether a vault credential is linked or needs repair.
    pub credential_state: ProfileCredentialStateLabel,
    /// Effective credential source for this provider in safe terms.
    pub credential_source: CredentialSourceLabel,
    /// Latest content-free connection test result.
    pub connection: ProfileConnectionState,
    /// Optional default model label.
    pub default_model: Option<String>,
    /// Default interaction mode label.
    pub default_mode: String,
}

impl Debug for ProviderProfileProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderProfileProjection")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("active", &self.active)
            .field("has_base_url", &!self.base_url.is_empty())
            .field("has_project", &!self.project.is_empty())
            .field("has_auth_header", &!self.auth_header.is_empty())
            .field("credential_state", &self.credential_state)
            .field("credential_source", &self.credential_source)
            .field("connection", &self.connection)
            .field("default_model", &self.default_model)
            .field("default_mode", &self.default_mode)
            .finish()
    }
}

/// Local-only user profile summary; this is not a hosted identity.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct LocalUserProfileProjection {
    /// Optional local display label from typed user settings.
    pub display_label: Option<String>,
    /// Active workspace shown to the local user.
    pub workspace: String,
    /// Default provider profile when configured.
    pub default_profile: Option<String>,
    /// Default model when configured.
    pub default_model: Option<String>,
    /// Default interaction mode.
    pub default_mode: String,
}

impl Debug for LocalUserProfileProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalUserProfileProjection")
            .field("display_label", &self.display_label)
            .field("has_workspace", &!self.workspace.is_empty())
            .field("default_profile", &self.default_profile)
            .field("default_model", &self.default_model)
            .field("default_mode", &self.default_mode)
            .finish()
    }
}

/// Full local profile and provider-connection read model.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfilesProjection {
    /// Local preferences summary.
    pub user: LocalUserProfileProjection,
    /// Named provider connections in stable profile order.
    pub profiles: Vec<ProviderProfileProjection>,
    /// Number of restart-safe recovery operations awaiting cleanup.
    pub pending_recovery: usize,
}

/// In-run composer history for prompt recall.
///
/// Entries are ordered oldest to newest and never persisted; the walk
/// position is an offset from the end, where `None` means the live draft.
#[derive(Clone, Debug, Default)]
pub(crate) struct ComposerHistory {
    entries: Vec<String>,
    /// Offset back into history: 0 = newest entry, None = not walking.
    walk: Option<usize>,
    /// Live composer content stashed while walking, restored at the end.
    stashed_draft: Option<String>,
}

impl ComposerHistory {
    const MAX_ENTRIES: usize = 100;

    pub(crate) fn record(&mut self, prompt: &str) {
        if prompt.is_empty() {
            return;
        }
        if self.entries.last().is_some_and(|last| last == prompt) {
            self.walk = None;
            return;
        }
        self.entries.push(prompt.to_owned());
        if self.entries.len() > Self::MAX_ENTRIES {
            let overflow = self.entries.len() - Self::MAX_ENTRIES;
            self.entries.drain(0..overflow);
        }
        self.walk = None;
    }

    /// Steps back (negative) or forward (positive) through history.
    ///
    /// Returns the recalled text, or `None` when the step would leave the
    /// history range. Stepping forward past the newest entry returns the
    /// stashed draft and ends the walk.
    pub(crate) fn step(&mut self, direction: isize, draft: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        match direction.cmp(&0) {
            std::cmp::Ordering::Less => {
                if self.walk.is_none() {
                    // Starting a walk stashes the live draft so it can be
                    // restored when the walk returns past the newest entry.
                    self.stashed_draft = Some(draft.to_owned());
                }
                let next = self
                    .walk
                    .map_or(0, |walk| walk.saturating_add((-direction) as usize));
                if next >= self.entries.len() {
                    // Saturate at the oldest entry.
                    self.walk = Some(self.entries.len() - 1);
                    return Some(self.entries[0].clone());
                }
                self.walk = Some(next);
                Some(self.entries[self.entries.len() - 1 - next].clone())
            }
            std::cmp::Ordering::Greater => {
                let walk = self.walk?;
                if walk == 0 {
                    self.walk = None;
                    return Some(self.stashed_draft.take().unwrap_or_default());
                }
                self.walk = Some(walk - 1);
                Some(self.entries[self.entries.len() - 1 - (walk - 1)].clone())
            }
            std::cmp::Ordering::Equal => None,
        }
    }

    pub(crate) fn reset_walk(&mut self) {
        self.walk = None;
    }
}

/// Per-session composer draft keyed by stable session identity.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub(crate) struct SessionDrafts {
    drafts: BTreeMap<String, String>,
}

impl SessionDrafts {
    pub(crate) fn take_for(&mut self, session_id: &str) -> Option<String> {
        self.drafts.remove(session_id)
    }

    pub(crate) fn stash(&mut self, session_id: &str, draft: String) {
        if draft.is_empty() {
            self.drafts.remove(session_id);
        } else {
            self.drafts.insert(session_id.to_owned(), draft);
        }
    }
}

/// Complete local state rendered by the terminal client.
#[derive(Debug)]
pub struct Model {
    /// Newest session read model.
    pub session: Arc<SessionProjection>,
    /// Newest catalog read model.
    pub catalog: Arc<CatalogProjection>,
    /// Newest read model of every durable session.
    pub(crate) sessions: Arc<SessionsProjection>,
    /// Newest resolved-settings read model.
    pub(crate) settings: Arc<SettingsProjection>,
    /// Newest safe local profile and provider-connection read model.
    pub(crate) profiles: Arc<ProfilesProjection>,
    /// Newest bounded memory inspection read model.
    pub(crate) memory: Arc<MemoryProjection>,
    /// Single source of truth for the active shell route and modal layer.
    pub(crate) navigation: NavigationState,
    /// Multiline prompt composer.
    pub composer: ComposerState,
    /// Transcript tail-follow and scroll state.
    pub transcript: TranscriptState,
    /// Current keyboard focus.
    pub focus: Focus,
    /// Current banner, if any.
    pub notice: Option<Notice>,
    /// Whether the next frame is materially different.
    pub dirty: bool,
    /// Whether the runner should exit.
    pub should_quit: bool,
    pub(crate) picker: PickerState,
    pub(crate) credential: CredentialState,
    pub(crate) browser: BrowserState,
    pub(crate) profile_center: ProfileCenterState,
    pub(crate) palette: PaletteState,
    pub(crate) help: HelpState,
    /// Deterministic inline Settings workspace interaction state.
    pub(crate) settings_workspace: SettingsState,
    /// Keyboard-first active-profile model-default selection state.
    pub(crate) model_defaults: ModelDefaultsState,
    /// Local user-profile dialog interaction state.
    pub(crate) user_profile: UserProfileState,
    /// Composer text saved while working in another session.
    pub(crate) drafts: SessionDrafts,
    /// In-run submitted-prompt history for recall.
    pub(crate) history: ComposerHistory,
    /// Active transcript search state.
    pub(crate) search: SearchState,
    /// Local Memory workspace search, filter, and drill-down state.
    pub(crate) memory_workspace: MemoryState,
    /// State owned by the one Memory lifecycle overlay.
    pub(crate) memory_lifecycle: Option<MemoryLifecycleState>,
    /// Whether transcript tool rows render expanded with resources.
    pub(crate) tools_expanded: bool,
    /// Wrapped transcript row pinned into view by an active search jump.
    pub(crate) search_pinned_row: Option<usize>,
    /// Most recent committed archive or unarchive available for undo.
    pub(crate) undoable: Option<UndoableLifecycle>,
    pub(crate) pending: BTreeMap<RequestId, PendingKind>,
    pub(crate) cancelling: BTreeSet<AttemptKey>,
    pub(crate) retrying: BTreeSet<AttemptKey>,
    pub(crate) answering_permissions: BTreeSet<ToolCallKey>,
    pub(crate) permission_scroll: u16,
    pub(crate) startup_complete: bool,
    pub(crate) retry_deadlines: BTreeMap<AttemptKey, UiInstant>,
    pub(crate) catalog_retry_deadline: Option<UiInstant>,
    pub(crate) next_request_id: u64,
    pub(crate) now: UiInstant,
    pub(crate) last_activity_ms: UiInstant,
    pub(crate) wall_ms: i64,
    pub(crate) color_depth: ColorDepth,
    pub(crate) theme: Theme,
}

const STARTUP_ANIMATION_MS: UiInstant = 400;

impl Model {
    pub(crate) fn startup_active(&self) -> bool {
        !self.startup_complete
            && self.now < STARTUP_ANIMATION_MS
            && matches!(&*self.catalog, CatalogProjection::Loading)
    }

    pub(crate) fn mark_activity(&mut self) {
        self.last_activity_ms = self.now;
    }

    pub(crate) fn advance_clock(&mut self, clock: UiClock) {
        self.now = clock.now;
        self.wall_ms = clock.wall_ms;
        if clock.now >= STARTUP_ANIMATION_MS {
            self.startup_complete = true;
        }
    }

    /// Motion sample for the current clock and accessibility flags.
    #[must_use]
    pub fn motion(&self) -> crate::ui::Motion {
        let preferences = self.settings.local_profile.preferences();
        crate::ui::Motion::new(
            self.now,
            self.last_activity_ms,
            *preferences.reduced_motion().value(),
            *preferences.glyph_mode().value(),
        )
    }
}

impl Model {
    /// Creates UI state from application-provided read models.
    #[must_use]
    pub fn new(
        session: Arc<SessionProjection>,
        sessions: Arc<SessionsProjection>,
        catalog: Arc<CatalogProjection>,
    ) -> Self {
        let permission_pending = !session.permission_requests.is_empty();
        let open_picker = session.selected_model.is_none()
            && matches!(&*catalog, CatalogProjection::Ready { models, .. } if !models.is_empty());
        let focus = if permission_pending {
            Focus::Permission
        } else if open_picker {
            Focus::Picker
        } else {
            Focus::Composer
        };
        let overlay = if permission_pending {
            Some(OverlayKind::Permission)
        } else if open_picker {
            Some(OverlayKind::ModelPicker)
        } else {
            None
        };
        let navigation = NavigationState {
            route: Route::Chat,
            previous_route: Route::Chat,
            overlay: overlay.map(|kind| OverlayFrame {
                kind,
                return_route: Route::Chat,
                return_focus: Focus::Composer,
            }),
        };
        let selected = session.selected_model.clone().or_else(|| {
            catalog
                .models()
                .iter()
                .find(|model| model.selectable)
                .map(|model| model.model.clone())
        });
        let startup_complete = matches!(
            &*catalog,
            CatalogProjection::Ready { .. } | CatalogProjection::Failed(_)
        );

        let mut model = Self {
            session,
            catalog,
            sessions,
            settings: Arc::new(SettingsProjection::default()),
            profiles: Arc::new(ProfilesProjection::default()),
            memory: Arc::new(MemoryProjection::default()),
            navigation,
            composer: ComposerState::default(),
            transcript: TranscriptState::new(),
            focus,
            notice: None,
            dirty: true,
            should_quit: false,
            picker: PickerState {
                query: String::new(),
                selected,
            },
            credential: CredentialState::default(),
            browser: BrowserState::default(),
            profile_center: ProfileCenterState::default(),
            palette: PaletteState::default(),
            startup_complete,
            help: HelpState::default(),
            settings_workspace: SettingsState::default(),
            model_defaults: ModelDefaultsState::default(),
            drafts: SessionDrafts::default(),
            user_profile: UserProfileState::default(),
            history: ComposerHistory::default(),
            search: SearchState::default(),
            memory_workspace: MemoryState::default(),
            memory_lifecycle: None,
            tools_expanded: false,
            search_pinned_row: None,
            undoable: None,
            pending: BTreeMap::new(),
            cancelling: BTreeSet::new(),
            retrying: BTreeSet::new(),
            answering_permissions: BTreeSet::new(),
            permission_scroll: 0,
            retry_deadlines: BTreeMap::new(),
            catalog_retry_deadline: None,
            next_request_id: 1,
            now: 0,
            last_activity_ms: 0,
            wall_ms: 0,
            color_depth: ColorDepth::TrueColor,
            theme: Theme::from_preset(ThemePreset::System, ColorMode::Color, ColorDepth::TrueColor),
        };
        model.refresh_theme();
        model.sync_retry_deadline();
        model.sync_catalog_retry_deadline();
        model.sync_browser_selection();
        model
    }
    /// Returns the active primary shell route.
    #[must_use]
    pub const fn route(&self) -> Route {
        self.navigation.route
    }

    /// Returns the one active modal layer, when any.
    #[must_use]
    pub fn overlay(&self) -> Option<OverlayKind> {
        self.navigation.overlay.map(|frame| frame.kind)
    }

    /// Changes the primary route unless a security decision owns input.
    pub(crate) fn navigate(&mut self, route: Route) -> bool {
        if self.overlay() == Some(OverlayKind::Permission) {
            return false;
        }
        if self.navigation.route != route {
            self.navigation.previous_route = self.navigation.route;
            self.navigation.route = route;
        }
        self.navigation.overlay = None;
        if route == Route::Settings {
            self.settings_workspace.nav_focus = true;
        }
        self.focus = route.focus();
        self.dirty = true;
        true
    }

    /// Returns to the route visited before the current primary route.
    pub(crate) fn navigate_back(&mut self) -> bool {
        let route = self.navigation.previous_route;
        self.navigate(route)
    }

    /// Opens one mutually exclusive modal layer over the current route.
    pub(crate) fn open_overlay(&mut self, kind: OverlayKind) -> bool {
        if self.overlay() == Some(OverlayKind::Permission) && kind != OverlayKind::Permission {
            return false;
        }
        let (return_route, return_focus) = self
            .navigation
            .overlay
            .map(|frame| (frame.return_route, frame.return_focus))
            .unwrap_or((self.navigation.route, self.navigation.route.focus()));
        self.navigation.overlay = Some(OverlayFrame {
            kind,
            return_route,
            return_focus,
        });
        self.focus = kind.focus();
        self.dirty = true;
        true
    }

    /// Closes the active modal layer and restores its captured base state.
    pub(crate) fn close_overlay(&mut self, expected: OverlayKind) -> bool {
        let Some(frame) = self.navigation.overlay else {
            return false;
        };
        if frame.kind != expected {
            return false;
        }
        self.navigation.overlay = None;
        self.navigation.route = frame.return_route;
        self.focus = frame.return_focus;
        self.dirty = true;
        true
    }

    /// Returns whether the Sessions route is active.
    #[must_use]
    pub const fn browser_open(&self) -> bool {
        matches!(self.navigation.route, Route::Sessions)
    }

    /// Replaces the resolved-settings read model.
    pub fn apply_settings(&mut self, settings: Arc<SettingsProjection>) {
        self.settings = settings;
        self.refresh_theme();
        self.dirty = true;
    }

    /// Replaces the bounded read-only Memory projection.
    pub fn apply_memory(&mut self, memory: Arc<MemoryProjection>) {
        if memory.generation() < self.memory.generation() {
            return;
        }
        self.memory = memory;
        self.sync_memory_selection();
        self.dirty = true;
    }

    /// Returns the latest bounded Memory projection.
    #[must_use]
    pub fn memory(&self) -> &MemoryProjection {
        &self.memory
    }

    /// Returns whether the Memory route is active.
    #[must_use]
    pub const fn memory_open(&self) -> bool {
        matches!(self.navigation.route, Route::Memory)
    }

    /// Returns the local Memory search query.
    #[must_use]
    pub fn memory_query(&self) -> &str {
        &self.memory_workspace.query
    }

    /// Returns the selected stable memory identity.
    #[must_use]
    pub fn memory_selection(&self) -> Option<&str> {
        self.memory_workspace.selected.as_deref()
    }

    /// Returns the active local Memory status filter.
    #[must_use]
    pub const fn memory_status_filter(&self) -> MemoryStatusFilter {
        self.memory_workspace.status
    }

    /// Returns the active local Memory scope filter.
    #[must_use]
    pub const fn memory_scope_filter(&self) -> MemoryScopeFilter {
        self.memory_workspace.scope
    }

    /// Returns the active compact Memory drill-down pane.
    #[must_use]
    pub const fn memory_pane(&self) -> MemoryPane {
        self.memory_workspace.pane
    }

    /// Returns the active Memory lifecycle workflow, when its overlay owns input.
    #[must_use]
    pub fn memory_lifecycle_mode(&self) -> Option<MemoryLifecycleMode> {
        self.memory_lifecycle.as_ref().map(|state| state.mode)
    }

    /// Returns whether a Memory lifecycle request is awaiting acknowledgement.
    #[must_use]
    pub fn memory_lifecycle_pending(&self) -> bool {
        self.memory_lifecycle
            .as_ref()
            .is_some_and(|state| state.pending_request.is_some())
    }

    /// Returns lifecycle actions supported by the selected loaded projection row.
    #[must_use]
    pub(crate) fn memory_actions(&self) -> Vec<MemoryLifecycleMode> {
        let Some((summary, detail)) = self.selected_memory() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if detail.is_some_and(|detail| detail.revision_context().is_some()) {
            match summary.status() {
                MemoryStatus::Proposed if detail.is_some_and(MemoryDetail::has_content) => {
                    actions.push(MemoryLifecycleMode::Review);
                }
                MemoryStatus::Proposed => {}
                MemoryStatus::Active => {
                    if detail.is_some_and(MemoryDetail::has_content) {
                        actions.push(MemoryLifecycleMode::Revise);
                    }
                    actions.push(MemoryLifecycleMode::Retract);
                }
                MemoryStatus::Superseded
                | MemoryStatus::Rejected
                | MemoryStatus::Retracted
                | MemoryStatus::Deleted => {}
            }
            if summary.status() != MemoryStatus::Deleted {
                actions.push(MemoryLifecycleMode::Delete);
            }
        }
        if detail.is_some() {
            actions.push(MemoryLifecycleMode::Export);
        }
        actions
    }

    /// Returns locally filtered memories in projection order.
    #[must_use]
    pub fn memory_entries(&self) -> Vec<&MemorySummary> {
        let query = self.memory_workspace.query.to_lowercase();
        self.memory
            .summaries()
            .iter()
            .filter(|summary| {
                self.memory_workspace.status.includes(summary.status())
                    && self.memory_workspace.scope.includes(summary.scope())
                    && (query.is_empty()
                        || summary.preview().to_lowercase().contains(&query)
                        || summary.id().to_lowercase().contains(&query)
                        || summary.status().label().contains(&query)
                        || summary.scope().label().contains(&query))
            })
            .collect()
    }

    pub(crate) fn selected_memory(&self) -> Option<(&MemorySummary, Option<&MemoryDetail>)> {
        let selected = self.memory_workspace.selected.as_deref()?;
        let summary = self
            .memory
            .summaries()
            .iter()
            .find(|summary| summary.id() == selected)?;
        Some((summary, self.memory.detail(selected)))
    }

    pub(crate) fn sync_memory_selection(&mut self) {
        let selected = self.memory_workspace.selected.clone();
        let valid = self
            .memory_entries()
            .iter()
            .any(|summary| Some(summary.id()) == selected.as_deref());
        if !valid {
            self.memory_workspace.selected = self
                .memory_entries()
                .first()
                .map(|summary| summary.id().to_owned());
            self.memory_workspace.pane = MemoryPane::List;
            self.memory_workspace.admission_selected = 0;
        }
    }

    fn refresh_theme(&mut self) {
        self.theme = Theme::resolve(self.settings.local_profile.preferences(), self.color_depth);
        self.composer.apply_theme(&self.theme);
    }

    /// Returns the resolved presentation theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Updates terminal color depth and re-resolves the theme.
    pub fn set_color_depth(&mut self, depth: ColorDepth) {
        self.color_depth = depth;
        self.refresh_theme();
        self.dirty = true;
    }
    /// Replaces the safe local profile and provider-connection read model.
    pub fn apply_profiles(&mut self, profiles: Arc<ProfilesProjection>) {
        self.profiles = profiles;
        self.sync_profile_selection();
        self.dirty = true;
    }

    /// Returns the latest safe profile projection.
    #[must_use]
    pub fn profiles(&self) -> &ProfilesProjection {
        &self.profiles
    }

    /// Returns whether the Profiles route is active.
    #[must_use]
    pub fn profile_center_open(&self) -> bool {
        matches!(self.navigation.route, Route::Profiles)
            || (matches!(self.navigation.route, Route::Settings)
                && SettingsCategory::at(self.settings_workspace.nav_selected)
                    == SettingsCategory::Providers)
    }

    /// Returns the highlighted provider profile identity.
    #[must_use]
    pub fn profile_selection(&self) -> Option<&str> {
        self.profile_center.selected.as_deref()
    }

    pub(crate) fn selected_profile(&self) -> Option<&ProviderProfileProjection> {
        let selected = self.profile_center.selected.as_deref()?;
        self.profiles
            .profiles
            .iter()
            .find(|profile| profile.id == selected)
    }

    pub(crate) fn filtered_profiles(
        &self,
    ) -> impl Iterator<Item = &ProviderProfileProjection> + use<'_> {
        let query = self.profile_center.query.to_ascii_lowercase();
        self.profiles.profiles.iter().filter(move |profile| {
            query.is_empty()
                || profile.id.to_ascii_lowercase().contains(&query)
                || profile.kind.as_str().contains(&query)
        })
    }

    /// Returns the newest resolved-settings read model.
    #[must_use]
    pub fn settings(&self) -> &SettingsProjection {
        &self.settings
    }

    /// Returns whether the Settings route is active.
    #[must_use]
    pub const fn settings_open(&self) -> bool {
        matches!(self.navigation.route, Route::Settings)
    }

    /// Returns display-safe row data shared by Settings rendering and search.
    #[must_use]
    pub(crate) fn settings_row_value(&self, preference: SettingsPreference) -> String {
        let profile = &self.settings.local_profile;
        let preferences = profile.preferences();
        match preference {
            SettingsPreference::DisplayLabel => self
                .settings_workspace
                .display_label_editor
                .as_ref()
                .cloned()
                .or_else(|| {
                    profile
                        .display_label()
                        .value()
                        .as_ref()
                        .map(|label| label.as_str().to_owned())
                })
                .unwrap_or_else(|| "not set".to_owned()),
            SettingsPreference::Provider => self.settings.provider_label(),
            SettingsPreference::Profile => self
                .settings
                .provider_status
                .active_profile
                .clone()
                .unwrap_or_else(|| "none".to_owned()),
            SettingsPreference::Credential => if self.settings.provider_status.credential_connected
            {
                "connected"
            } else {
                "disconnected"
            }
            .to_owned(),
            SettingsPreference::Source => self
                .settings
                .provider_status
                .credential_source
                .as_str()
                .to_owned(),
            SettingsPreference::Model => self.session.selected_model.as_ref().map_or_else(
                || "no model".to_owned(),
                |model| model.model_id().as_str().to_owned(),
            ),
            SettingsPreference::Mode => if self.profiles.user.default_mode.is_empty() {
                "provider default"
            } else {
                self.profiles.user.default_mode.as_str()
            }
            .to_owned(),
            SettingsPreference::ThemePreset => match preferences.theme_preset().value() {
                ThemePreset::System => "system",
                ThemePreset::Light => "light",
                ThemePreset::Dark => "dark",
                ThemePreset::Aurora => "aurora",
                ThemePreset::Ember => "ember",
                ThemePreset::Midnight => "midnight",
                ThemePreset::Ocean => "ocean",
                ThemePreset::Forest => "forest",
                ThemePreset::Rose => "rose",
            }
            .to_owned(),
            SettingsPreference::ColorMode => match preferences.color_mode().value() {
                ColorMode::Color => "color",
                ColorMode::Soft => "soft",
                ColorMode::Vivid => "vivid",
                ColorMode::NoColor => "no color",
                ColorMode::HighContrast => "high contrast",
            }
            .to_owned(),
            SettingsPreference::GlyphMode => match preferences.glyph_mode().value() {
                GlyphMode::Unicode => "unicode",
                GlyphMode::NerdFont => "Nerd Font",
                GlyphMode::Ascii => "ASCII",
            }
            .to_owned(),
            SettingsPreference::PromptStatusDetail => {
                match preferences.prompt_status_detail().value() {
                    PromptStatusDetail::Essential => "essential",
                    PromptStatusDetail::Workspace => "workspace",
                    PromptStatusDetail::Detailed => "detailed",
                }
                .to_owned()
            }
            SettingsPreference::ReducedMotion => if *preferences.reduced_motion().value() {
                "on"
            } else {
                "off"
            }
            .to_owned(),
            SettingsPreference::Density => match preferences.density().value() {
                Density::Comfortable => "comfortable",
                Density::Compact => "compact",
            }
            .to_owned(),
            SettingsPreference::Layout => match preferences.layout().value() {
                Layout::Responsive => "responsive",
                Layout::SingleColumn => "single column",
            }
            .to_owned(),
            SettingsPreference::TerminalTimestampStyle => {
                match preferences.terminal_timestamp_style().value() {
                    TerminalTimestampStyle::Relative => "relative",
                    TerminalTimestampStyle::Absolute => "absolute",
                    TerminalTimestampStyle::Hidden => "hidden",
                }
                .to_owned()
            }
            SettingsPreference::ComposerSubmitBehavior => {
                match preferences.composer_submit_behavior().value() {
                    ComposerSubmitBehavior::ControlS => "Ctrl+S",
                    ComposerSubmitBehavior::Enter => "Enter",
                }
                .to_owned()
            }
            SettingsPreference::Approvals => "per-call capability decisions".to_owned(),
            SettingsPreference::Retention => "durable until explicitly deleted".to_owned(),
            SettingsPreference::Logging => "redacted; credentials excluded".to_owned(),
            SettingsPreference::GlyphCheck => self.theme.icons().glyph_check_line(),
            SettingsPreference::KeyboardNavigation => {
                "all controls are keyboard reachable".to_owned()
            }
            SettingsPreference::StateIndicators => "fill and glyph, never color alone".to_owned(),
            SettingsPreference::Workspace => if self.profiles.user.workspace.is_empty() {
                "not set"
            } else {
                self.profiles.user.workspace.as_str()
            }
            .to_owned(),
            SettingsPreference::ColorDepth => match self.color_depth {
                ColorDepth::TrueColor => "truecolor",
                ColorDepth::Indexed256 => "256 colors",
                ColorDepth::Basic16 => "16 colors",
            }
            .to_owned(),
            SettingsPreference::Version => env!("CARGO_PKG_VERSION").to_owned(),
            SettingsPreference::ManageProviders => "Manage providers".to_owned(),
            SettingsPreference::ConnectCredential => "Connect API key".to_owned(),
            SettingsPreference::ConfigureModels => "Choose model and thinking".to_owned(),
            SettingsPreference::OpenSessions => "Open sessions".to_owned(),
            SettingsPreference::OpenMemory => "Inspect and manage memory".to_owned(),
        }
    }

    /// Finds typed Settings rows by label or current rendered value.
    #[must_use]
    pub(crate) fn settings_search_results(
        &self,
    ) -> Vec<(SettingsCategory, usize, SettingsPreference)> {
        let query = self
            .settings_workspace
            .search_query
            .trim()
            .to_ascii_lowercase();
        let mut results = Vec::new();
        for category in SettingsCategory::ALL {
            for (index, preference) in SettingsPreference::rows(category)
                .iter()
                .copied()
                .enumerate()
            {
                let matches = query.is_empty()
                    || preference.label().to_ascii_lowercase().contains(&query)
                    || self
                        .settings_row_value(preference)
                        .to_ascii_lowercase()
                        .contains(&query);
                if matches {
                    results.push((category, index, preference));
                }
            }
        }
        results
    }
    /// Returns whether the local user-profile dialog owns the modal slot.
    #[must_use]
    pub fn user_profile_open(&self) -> bool {
        self.overlay() == Some(OverlayKind::UserProfile)
    }

    /// Returns the effective provider label in safe provenance terms.
    #[must_use]
    pub fn settings_provider_label(&self) -> String {
        let status = &self.settings.provider_status;
        match (&status.active_profile, status.provider_kind) {
            (Some(profile), Some(kind)) => format!("{} via '{}'", kind.as_str(), profile),
            (Some(profile), None) => format!("profile '{profile}'"),
            (None, Some(kind)) => kind.as_str().to_owned(),
            (None, None) => "gemini (default)".to_owned(),
        }
    }

    /// Returns the effective credential label in safe provenance terms.
    #[must_use]
    pub fn settings_credential_label(&self) -> String {
        let status = &self.settings.provider_status;
        match (status.credential_source, status.credential_connected) {
            (CredentialSourceLabel::CredentialVault, true) => "credential vault".to_owned(),
            (CredentialSourceLabel::Environment, true) => "environment".to_owned(),
            (source, true) => source.as_str().to_owned(),
            (_, false) => "session only; press Ctrl+K to connect".to_owned(),
        }
    }

    /// Returns the highlighted browser session identity.
    #[must_use]
    pub fn browser_selection(&self) -> Option<&str> {
        self.browser.selected.as_deref()
    }

    /// Returns whether a rename buffer is active in the browser.
    #[must_use]
    pub const fn browser_renaming(&self) -> bool {
        self.browser.renaming
    }

    /// Returns the pending rename buffer content.
    #[must_use]
    pub fn browser_rename_buffer(&self) -> &str {
        &self.browser.rename_buffer
    }

    /// Returns the current browser filter query.
    #[must_use]
    pub fn browser_query(&self) -> &str {
        &self.browser.query
    }

    /// Returns the identity awaiting confirmed deletion, if any.
    #[must_use]
    pub fn browser_delete_confirmation(&self) -> Option<&str> {
        self.browser.confirming_delete.as_deref()
    }

    /// Returns whether the model picker owns the overlay slot.
    #[must_use]
    pub fn picker_open(&self) -> bool {
        self.overlay() == Some(OverlayKind::ModelPicker)
    }

    /// Returns whether the command palette owns the overlay slot.
    #[must_use]
    pub fn palette_open(&self) -> bool {
        self.overlay() == Some(OverlayKind::CommandPalette)
    }

    /// Returns the current palette filter query.
    #[must_use]
    pub fn palette_query(&self) -> &str {
        &self.palette.query
    }

    /// Returns palette rows matching the query in table order.
    #[must_use]
    pub fn palette_entries(&self) -> Vec<CommandEntry> {
        ranked_command_entries(&self.palette.query)
    }

    /// Returns the highlighted palette command identity.
    #[must_use]
    pub const fn palette_selection(&self) -> Option<&'static str> {
        self.palette.selected
    }

    /// Returns whether the Help route is active.
    #[must_use]
    pub const fn help_open(&self) -> bool {
        matches!(self.navigation.route, Route::Help)
    }

    /// Returns the current help scroll offset in rows.
    #[must_use]
    pub const fn help_scroll(&self) -> u16 {
        self.help.scroll
    }

    /// Returns whether tool rows currently render expanded.
    #[must_use]
    pub const fn tools_expanded(&self) -> bool {
        self.tools_expanded
    }

    /// Returns the identity awaiting confirmed archiving, if any.
    #[must_use]
    pub fn browser_archive_confirmation(&self) -> Option<&str> {
        self.browser.confirming_archive.as_deref()
    }

    /// Returns whether transcript search owns the overlay slot.
    #[must_use]
    pub fn search_open(&self) -> bool {
        self.overlay() == Some(OverlayKind::TranscriptSearch)
    }

    /// Returns the active search query.
    #[must_use]
    pub fn search_query(&self) -> &str {
        &self.search.query
    }

    /// Returns the number of transcript matches for the query.
    #[must_use]
    pub fn search_match_count(&self) -> usize {
        self.search.matches.len()
    }

    /// Returns the position of the currently selected match.
    #[must_use]
    pub fn search_current_index(&self) -> usize {
        self.search.current.unwrap_or(0)
    }

    /// Returns a safe one-line status label such as `2/7 matches`.
    #[must_use]
    pub fn search_status_label(&self) -> String {
        if self.search.matches.is_empty() {
            return "no matches".to_owned();
        }
        let current = self.search.current.map_or(1, |index| index + 1);
        format!("{}/{} matches", current, self.search.matches.len())
    }

    /// Returns the filtered session-browser rows in durable order.
    #[must_use]
    pub fn browser_entries(&self) -> Vec<&SessionBrowserEntry> {
        let query = self.browser.query.to_lowercase();
        self.sessions
            .sessions
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.session_id.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub(crate) fn sync_browser_selection(&mut self) {
        let entries = self.browser_entries();
        if entries.is_empty() {
            self.browser.selected = None;
            return;
        }
        let valid = self
            .browser
            .selected
            .as_ref()
            .is_some_and(|selected| entries.iter().any(|entry| &entry.session_id == selected));
        if !valid {
            self.browser.selected = entries
                .iter()
                .find(|entry| entry.active)
                .or_else(|| entries.first())
                .map(|entry| entry.session_id.clone());
        }
    }

    /// Returns whether session-only credential entry owns the overlay slot.
    #[must_use]
    pub fn credential_open(&self) -> bool {
        self.overlay() == Some(OverlayKind::SessionCredential)
    }

    /// Returns whether the masked credential editor contains any input.
    #[must_use]
    pub fn credential_has_value(&self) -> bool {
        self.credential.has_value()
    }

    /// Returns the picker search query.
    #[must_use]
    pub fn picker_query(&self) -> &str {
        &self.picker.query
    }

    /// Returns the selected picker model.
    #[must_use]
    pub const fn picker_selection(&self) -> Option<&ModelRef> {
        self.picker.selected.as_ref()
    }

    /// Returns requests awaiting acknowledgement.
    #[must_use]
    pub const fn pending(&self) -> &BTreeMap<RequestId, PendingKind> {
        &self.pending
    }

    /// Returns whether a cancellation request is durably acknowledged but unsettled.
    #[must_use]
    pub fn cancellation_requested(&self, attempt_id: &AttemptKey) -> bool {
        self.cancelling.contains(attempt_id)
            || self
                .session
                .active_attempt()
                .is_some_and(|(candidate, status)| {
                    candidate == attempt_id && matches!(status, AttemptStatus::Cancelling)
                })
    }

    /// Returns whether a retry request is durably acknowledged but not yet projected.
    #[must_use]
    pub fn retry_requested(&self, attempt_id: &AttemptKey) -> bool {
        self.retrying.contains(attempt_id)
    }

    /// Returns whether the newest failed attempt may be retried now.
    #[must_use]
    pub fn retry_available(&self, attempt_id: &AttemptKey, policy: RetryPolicy) -> bool {
        match policy {
            RetryPolicy::Never => false,
            RetryPolicy::Now => true,
            RetryPolicy::After { .. } => self
                .retry_deadlines
                .get(attempt_id)
                .is_some_and(|ready_at| self.now >= *ready_at),
            RetryPolicy::At(ready_at) => self.now >= ready_at,
        }
    }

    /// Returns remaining delay for a retry policy, when it has a deadline.
    #[must_use]
    pub fn retry_remaining_ms(&self, attempt_id: &AttemptKey, policy: RetryPolicy) -> Option<u64> {
        match policy {
            RetryPolicy::After { .. } => self
                .retry_deadlines
                .get(attempt_id)
                .map(|ready_at| ready_at.saturating_sub(self.now)),
            RetryPolicy::At(ready_at) => Some(ready_at.saturating_sub(self.now)),
            RetryPolicy::Never | RetryPolicy::Now => None,
        }
    }

    /// Returns the current monotonic UI time.
    #[must_use]
    pub const fn now(&self) -> UiInstant {
        self.now
    }

    /// Returns the current Unix-epoch wall-clock milliseconds.
    #[must_use]
    pub const fn wall_ms(&self) -> i64 {
        self.wall_ms
    }

    /// Returns whether the failed catalog may be refreshed under its retry policy.
    #[must_use]
    pub fn catalog_retry_available(&self, policy: RetryPolicy) -> bool {
        match policy {
            RetryPolicy::Never => false,
            RetryPolicy::Now => true,
            RetryPolicy::After { .. } => self
                .catalog_retry_deadline
                .is_some_and(|ready_at| self.now >= ready_at),
            RetryPolicy::At(ready_at) => self.now >= ready_at,
        }
    }

    /// Returns the remaining catalog refresh delay, when one exists.
    #[must_use]
    pub fn catalog_retry_remaining_ms(&self, policy: RetryPolicy) -> Option<u64> {
        match policy {
            RetryPolicy::After { .. } => self
                .catalog_retry_deadline
                .map(|ready_at| ready_at.saturating_sub(self.now)),
            RetryPolicy::At(ready_at) => Some(ready_at.saturating_sub(self.now)),
            RetryPolicy::Never | RetryPolicy::Now => None,
        }
    }

    pub(crate) fn allocate_request(&mut self) -> RequestId {
        let request_id = RequestId(self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }

    pub(crate) fn sync_profile_selection(&mut self) {
        let selected_visible = self
            .profile_center
            .selected
            .as_deref()
            .is_some_and(|selected| {
                self.filtered_profiles()
                    .any(|profile| profile.id == selected)
            });
        if selected_visible {
            return;
        }
        let next = self
            .filtered_profiles()
            .find(|profile| profile.active)
            .or_else(|| self.filtered_profiles().next())
            .map(|profile| profile.id.clone());
        self.profile_center.selected = next;
    }

    pub(crate) fn sync_retry_deadline(&mut self) {
        let relative_retry = self
            .session
            .retryable_attempt()
            .and_then(|(attempt_id, retry)| match retry {
                RetryPolicy::After { delay_ms } => Some((attempt_id.clone(), delay_ms)),
                RetryPolicy::Never | RetryPolicy::Now | RetryPolicy::At(_) => None,
            });
        self.retry_deadlines.retain(|attempt_id, _| {
            relative_retry
                .as_ref()
                .is_some_and(|(current, _)| current == attempt_id)
        });
        if let Some((attempt_id, delay_ms)) = relative_retry {
            let ready_at = self.now.saturating_add(delay_ms);
            self.retry_deadlines.entry(attempt_id).or_insert(ready_at);
        }
    }

    pub(crate) fn sync_catalog_retry_deadline(&mut self) {
        self.catalog_retry_deadline = match &*self.catalog {
            CatalogProjection::Failed(UiFailure {
                retry: RetryPolicy::After { delay_ms },
                ..
            }) => Some(self.now.saturating_add(*delay_ms)),
            CatalogProjection::CredentialRequired
            | CatalogProjection::Loading
            | CatalogProjection::Ready { .. }
            | CatalogProjection::Failed(_) => None,
        };
    }
}

fn ranked_command_entries(query: &str) -> Vec<CommandEntry> {
    let query = query.trim_start_matches('/').trim().to_ascii_lowercase();
    let mut entries = COMMANDS
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, entry)| {
            command_match_score(entry, &query).map(|score| (score, index, entry))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(score, index, _)| (*score, *index));
    entries.into_iter().map(|(_, _, entry)| entry).collect()
}

fn command_match_score(entry: CommandEntry, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }
    let label = entry.label.to_ascii_lowercase();
    let description = entry.description.to_ascii_lowercase();
    if entry.id == query {
        return Some((0, 0));
    }
    if entry.id.starts_with(query) {
        return Some((1, entry.id.len().saturating_sub(query.len())));
    }
    if label.starts_with(query) {
        return Some((2, label.len().saturating_sub(query.len())));
    }
    if entry.id.split('-').any(|word| word.starts_with(query))
        || label.split_whitespace().any(|word| word.starts_with(query))
    {
        return Some((3, 0));
    }
    if let Some(position) = entry.id.find(query) {
        return Some((4, position));
    }
    if let Some(position) = label.find(query) {
        return Some((5, position));
    }
    let fuzzy_limit = 1.max(query.chars().count() / 3);
    let id_distance = edit_distance(entry.id, query);
    if id_distance <= fuzzy_limit {
        return Some((6, id_distance));
    }
    let label_distance = label
        .split_whitespace()
        .map(|candidate| edit_distance(candidate, query))
        .min()
        .unwrap_or(usize::MAX);
    if label_distance <= fuzzy_limit {
        return Some((7, label_distance));
    }
    description.find(query).map(|position| (8, position))
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_character) in right.iter().enumerate() {
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution =
                previous[right_index] + usize::from(left_character != *right_character);
            current.push(insertion.min(deletion).min(substitution));
        }
        previous = current;
    }
    previous[right.len()]
}
