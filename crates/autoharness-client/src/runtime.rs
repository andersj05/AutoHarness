//! In-process application messages and projections, independent of any renderer.
//!
//! These types carry Rust-owned channels and zeroizing ingress. The crate root
//! separately defines the serializable, versioned transport contract.

use autoharness_domain::{
    ErrorClass, MAX_MEMORY_VALIDATION_CANDIDATES, MAX_MEMORY_VALIDATION_ISSUES, ModelRef,
    RetryAdvice,
};
use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, EffectiveLocalProfile, GlyphMode, GuiFontSize,
    GuiZoomPercent, Layout, LocalProfile, PromptStatusDetail, TerminalTimestampStyle, ThemePreset,
};
use std::collections::BTreeSet;
use std::fmt::{self, Debug, Formatter};
use zeroize::Zeroizing;

/// Monotonic milliseconds supplied by application composition.
pub type UiInstant = u64;

mod ports;
pub use ports::*;

const MAX_CREDENTIAL_BYTES: usize = 4_096;
const MAX_MEMORY_ID_CHARS: usize = 512;
const MAX_MEMORY_PREVIEW_CHARS: usize = 240;
const MAX_MEMORY_CONTENT_CHARS: usize = 16_384;
const MAX_MEMORY_IMPORT_PATH_CHARS: usize = 1_024;
const MAX_MEMORY_SOURCE_CHARS: usize = 512;
const MAX_MEMORY_ADMISSION_TEXT_CHARS: usize = 256;
const MAX_MEMORY_CONTEXT_TEXT_CHARS: usize = 4_096;
const MAX_MEMORY_SUMMARIES: usize = 100;
const MAX_MEMORY_DETAILS: usize = 100;
const MAX_MEMORY_ADMISSIONS: usize = 64;
const MAX_MEMORY_EVIDENCE: usize = 64;
const MAX_MEMORY_RELATIONS: usize = 64;
const MAX_MEMORY_FINDINGS: usize =
    MAX_MEMORY_RELATIONS + MAX_MEMORY_VALIDATION_CANDIDATES + MAX_MEMORY_VALIDATION_ISSUES;
const MAX_MEMORY_REASON_FACTORS: usize = 16;
const MAX_MEMORY_VIEW_CURSOR_CHARS: usize = 512;
/// Maximum literal characters accepted by one Memory workspace query.
pub const MAX_MEMORY_VIEW_QUERY_CHARS: usize = 256;
/// Fixed bounded page size requested by the Memory workspace.
pub const MEMORY_VIEW_PAGE_SIZE: u16 = 100;

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
/// Non-secret provider form transferred from a client to application composition.
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
    Conflicting,
    Superseded,
    Rejected,
    Retracted,
    Expired,
    Deleted,
}

impl MemoryStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Proposed => "proposed",
            Self::Conflicting => "conflicting",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
            Self::Retracted => "retracted",
            Self::Expired => "expired",
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

/// Bounded workspace-relative document path transferred without diagnostic disclosure.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryImportPath {
    raw: Zeroizing<String>,
}

impl MemoryImportPath {
    /// Creates one normalized workspace-relative path with no traversal or platform prefix.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("import path must not be blank");
        }
        if value.chars().count() > MAX_MEMORY_IMPORT_PATH_CHARS {
            return Err("import path is too long");
        }
        if value.chars().any(char::is_control) {
            return Err("import path must be one safe line");
        }

        let normalized = value.replace('\\', "/");
        let mut relative = normalized.as_str();
        while let Some(stripped) = relative.strip_prefix("./") {
            relative = stripped;
        }
        if relative.is_empty() || relative.starts_with('/') {
            return Err("import path must be workspace-relative");
        }
        let first = relative.split('/').next().unwrap_or_default().as_bytes();
        if first.len() >= 2 && first[0].is_ascii_alphabetic() && first[1] == b':' {
            return Err("import path must not use a platform prefix");
        }
        if relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        {
            return Err("import path must not contain traversal components");
        }

        Ok(Self {
            raw: Zeroizing::new(relative.to_owned()),
        })
    }

    /// Borrows the normalized relative path for application-owned resolution.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Moves the normalized relative path into application composition.
    #[must_use]
    pub fn into_string(mut self) -> String {
        std::mem::take(&mut *self.raw)
    }
}

impl Debug for MemoryImportPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryImportPath([REDACTED])")
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
    excerpt: Option<Zeroizing<String>>,
    availability: MemoryEvidenceAvailability,
}

impl MemoryEvidence {
    /// Constructs evidence whose exact retained excerpt may be displayed deliberately.
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
            excerpt: Some(Zeroizing::new(bounded_text(
                excerpt.into(),
                MAX_MEMORY_CONTEXT_TEXT_CHARS,
                "memory evidence excerpt",
            )?)),
            availability: MemoryEvidenceAvailability::Retained,
        })
    }

    /// Constructs evidence whose immutable metadata never named an excerpt.
    pub fn absent(
        label: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Self::without_excerpt(label, source, MemoryEvidenceAvailability::Absent)
    }

    /// Constructs evidence whose formerly retained excerpt was logically erased.
    pub fn erased(
        label: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Self::without_excerpt(label, source, MemoryEvidenceAvailability::Erased)
    }

    fn without_excerpt(
        label: impl Into<String>,
        source: impl Into<String>,
        availability: MemoryEvidenceAvailability,
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
            excerpt: None,
            availability,
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
        match self.availability {
            MemoryEvidenceAvailability::Retained => self
                .excerpt
                .as_ref()
                .map(|excerpt| excerpt.as_str())
                .expect("retained evidence owns an excerpt"),
            MemoryEvidenceAvailability::Absent => "No excerpt was recorded.",
            MemoryEvidenceAvailability::Erased => "Excerpt erased by logical deletion.",
        }
    }

    /// Returns retained exact evidence bytes without conflating absent and erased states.
    #[must_use]
    pub fn retained_excerpt(&self) -> Option<&str> {
        self.excerpt.as_ref().map(|excerpt| excerpt.as_str())
    }

    /// Returns the explicit excerpt availability state.
    #[must_use]
    pub const fn availability(&self) -> MemoryEvidenceAvailability {
        self.availability
    }
}

impl Debug for MemoryEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryEvidence")
            .field("label", &"[REDACTED]")
            .field("source", &"[REDACTED]")
            .field("excerpt", &"[REDACTED]")
            .field("availability", &self.availability)
            .finish()
    }
}

/// Whether exact evidence bytes remain available to the inspection workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryEvidenceAvailability {
    Retained,
    Absent,
    Erased,
}

/// Typed relation between one memory and another ledger identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRelationKind {
    DuplicateOf,
    Contradicts,
    Refines,
    Supersedes,
    Related,
    DerivedFrom,
}

impl MemoryRelationKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DuplicateOf => "duplicate of",
            Self::Contradicts => "contradicts",
            Self::Refines => "refines",
            Self::Supersedes => "supersedes",
            Self::Related => "related to",
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
    SecretDetected,
    UnsupportedScope,
    MalformedContent,
    PolicyConflict,
    InjectionPattern,
    UngroundedEvidence,
}

impl MemoryFindingKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Duplicate => "possible duplicate",
            Self::Contradiction => "possible contradiction",
            Self::SecretDetected => "secret detected",
            Self::UnsupportedScope => "unsupported scope",
            Self::MalformedContent => "malformed content",
            Self::PolicyConflict => "policy conflict",
            Self::InjectionPattern => "injection pattern",
            Self::UngroundedEvidence => "ungrounded evidence",
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

/// Opaque stable boundary returned by application-owned Memory inspection.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryViewCursor(String);

impl MemoryViewCursor {
    /// Creates a bounded non-empty cursor without interpreting its contents.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("memory view cursor must not be empty");
        }
        if value.chars().count() > MAX_MEMORY_VIEW_CURSOR_CHARS {
            return Err("memory view cursor is too long");
        }
        if value.chars().any(char::is_control) {
            return Err("memory view cursor contains control characters");
        }
        Ok(Self(value))
    }

    /// Returns the opaque cursor for application-owned decoding.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for MemoryViewCursor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("MemoryViewCursor([REDACTED])")
    }
}

/// User navigation that selected the requested stable Memory page boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MemoryPageDirection {
    /// Start from the newest matching row.
    #[default]
    First,
    /// Advance to the next older page.
    Next,
    /// Return to a previously visited newer page.
    Previous,
}

/// One bounded authoritative Memory workspace request.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryViewQuery {
    literal: String,
    status: MemoryStatusFilter,
    scope: MemoryScopeFilter,
    direction: MemoryPageDirection,
    before: Option<MemoryViewCursor>,
    limit: u16,
}

impl MemoryViewQuery {
    /// Constructs a literal query over one stable page boundary.
    pub fn new(
        literal: impl Into<String>,
        status: MemoryStatusFilter,
        scope: MemoryScopeFilter,
        direction: MemoryPageDirection,
        before: Option<MemoryViewCursor>,
        limit: u16,
    ) -> Result<Self, &'static str> {
        let literal = literal.into();
        if literal.chars().count() > MAX_MEMORY_VIEW_QUERY_CHARS {
            return Err("memory view query is too long");
        }
        if literal.chars().any(char::is_control) {
            return Err("memory view query contains control characters");
        }
        if limit == 0 || limit > MEMORY_VIEW_PAGE_SIZE {
            return Err("memory view page limit is out of range");
        }
        if direction == MemoryPageDirection::First && before.is_some() {
            return Err("first memory page cannot have a cursor");
        }
        if direction == MemoryPageDirection::Next && before.is_none() {
            return Err("next memory page requires a cursor");
        }
        Ok(Self {
            literal,
            status,
            scope,
            direction,
            before,
            limit,
        })
    }

    /// Returns exact literal search text without query-language interpretation.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }

    /// Returns the requested status filter.
    #[must_use]
    pub const fn status(&self) -> MemoryStatusFilter {
        self.status
    }

    /// Returns the requested scope filter.
    #[must_use]
    pub const fn scope(&self) -> MemoryScopeFilter {
        self.scope
    }

    /// Returns how the user reached this page boundary.
    #[must_use]
    pub const fn direction(&self) -> MemoryPageDirection {
        self.direction
    }

    /// Returns the exclusive newest-first boundary, when not on the first page.
    #[must_use]
    pub const fn before(&self) -> Option<&MemoryViewCursor> {
        self.before.as_ref()
    }

    /// Returns the fixed bounded row limit.
    #[must_use]
    pub const fn limit(&self) -> u16 {
        self.limit
    }
}

impl Debug for MemoryViewQuery {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryViewQuery")
            .field("literal", &"[REDACTED]")
            .field("status", &self.status)
            .field("scope", &self.scope)
            .field("direction", &self.direction)
            .field("has_cursor", &self.before.is_some())
            .field("limit", &self.limit)
            .finish()
    }
}

/// Bounded read model consumed by the read-only Memory workspace.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryProjection {
    view_generation: u64,
    generation: u64,
    state: MemoryLoadState,
    summaries: Vec<MemorySummary>,
    details: Vec<MemoryDetail>,
    total: u32,
    stale: bool,
    next_cursor: Option<MemoryViewCursor>,
}

impl MemoryProjection {
    #[must_use]
    pub const fn loading(generation: u64) -> Self {
        Self {
            view_generation: 0,
            generation,
            state: MemoryLoadState::Loading,
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
            next_cursor: None,
        }
    }

    #[must_use]
    pub fn failed(generation: u64, failure: UiFailure) -> Self {
        Self {
            view_generation: 0,
            generation,
            state: MemoryLoadState::Failed(failure),
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
            next_cursor: None,
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
            view_generation: 0,
            generation,
            state: MemoryLoadState::Ready,
            summaries,
            details,
            total,
            stale,
            next_cursor: None,
        })
    }

    /// Binds this response to one exact UI view request and its next-page boundary.
    #[must_use]
    pub fn with_view_page(
        mut self,
        view_generation: u64,
        next_cursor: Option<MemoryViewCursor>,
    ) -> Self {
        self.view_generation = view_generation;
        self.next_cursor = next_cursor;
        self
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

    /// Monotonic UI query generation, independent from durable ledger generation.
    #[must_use]
    pub const fn view_generation(&self) -> u64 {
        self.view_generation
    }

    /// Returns the stable boundary for the next older page, when one exists.
    #[must_use]
    pub const fn next_cursor(&self) -> Option<&MemoryViewCursor> {
        self.next_cursor.as_ref()
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
            view_generation: 0,
            generation: 0,
            state: MemoryLoadState::Ready,
            summaries: Vec::new(),
            details: Vec::new(),
            total: 0,
            stale: false,
            next_cursor: None,
        }
    }
}

impl Debug for MemoryProjection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryProjection")
            .field("view_generation", &self.view_generation)
            .field("generation", &self.generation)
            .field("state", &self.state)
            .field("summary_count", &self.summaries.len())
            .field("detail_count", &self.details.len())
            .field("total", &self.total)
            .field("stale", &self.stale)
            .field("has_next_page", &self.next_cursor.is_some())
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

/// A provider credential transferred from a client without persistence or serialization.
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

/// One user-layer local preference update requested by the Settings route.
///
/// `None` clears the user-layer leaf so the resolver inherits the next
/// applicable layer. The application validates and persists these values;
/// the renderer never accesses settings storage directly.
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
    /// Desktop renderer zoom percentage.
    GuiZoomPercent(Option<GuiZoomPercent>),
    /// Desktop renderer base font size.
    GuiFontSize(Option<GuiFontSize>),
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
    /// Query one bounded authoritative Memory workspace page.
    QueryMemory {
        request_id: RequestId,
        view_generation: u64,
        query: MemoryViewQuery,
    },
    /// Create an explicit user-authored memory.
    RememberMemory {
        request_id: RequestId,
        content: MemoryContent,
    },
    /// Copy one workspace-relative UTF-8 document into a review-only proposal.
    ImportMemory {
        request_id: RequestId,
        path: MemoryImportPath,
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
            | Self::QueryMemory { request_id, .. }
            | Self::RememberMemory { request_id, .. }
            | Self::ImportMemory { request_id, .. }
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
                        | MemoryStatus::Conflicting
                        | MemoryStatus::Expired
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
    /// Persisted user-layer values used to expose exact reset availability.
    pub user_local_profile: LocalProfile,
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
