use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelRef, RetryAdvice};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui_textarea::{TextArea, WrapMode};
use zeroize::{Zeroize, Zeroizing};

const MAX_CREDENTIAL_BYTES: usize = 4_096;

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
            message: message.into(),
            retry,
        }
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
            TranscriptItem::User { .. } => None,
        })
    }
}

/// Monotonic milliseconds supplied by the runner or a deterministic test.
pub type UiInstant = u64;

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
    pub open: bool,
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
            .field("open", &self.open)
            .field("has_value", &self.has_value())
            .finish()
    }
}

/// Model-picker local state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PickerState {
    pub open: bool,
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
        editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Prompt ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        editor.set_cursor_line_style(Style::default());
        editor.set_cursor_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        editor.set_placeholder_text("Ask AutoHarness...");
        editor.set_wrap_mode(WrapMode::WordOrGlyph);
        Self { editor }
    }
}

impl ComposerState {
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

/// Kind of request awaiting application acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingKind {
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
}

/// Intent emitted by pure update logic and handled by application composition.
#[derive(Debug)]
pub enum UiIntent {
    /// Configure the provider from an ephemeral API key.
    ConfigureCredential {
        request_id: RequestId,
        credential: ApiCredential,
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
}

impl UiIntent {
    /// Returns the local request identity.
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        match self {
            Self::ConfigureCredential { request_id, .. }
            | Self::RefreshCatalog { request_id }
            | Self::SelectModel { request_id, .. }
            | Self::SubmitPrompt { request_id, .. }
            | Self::CancelAttempt { request_id, .. }
            | Self::RetryAttempt { request_id, .. }
            | Self::AnswerPermission { request_id, .. } => *request_id,
        }
    }
}

/// Effect returned by update logic.
#[derive(Debug)]
pub enum UiEffect {
    /// Dispatch an intent through the bounded application mailbox.
    Dispatch(UiIntent),
    /// Exit the terminal client.
    Quit,
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
}

/// Input to the deterministic update function.
pub enum Message {
    /// Backend-independent keyboard input.
    Input(ratatui_textarea::Input),
    /// Bracketed paste content.
    Paste(String),
    /// Newest session projection.
    SessionChanged(Arc<SessionProjection>),
    /// Newest model catalog projection.
    CatalogChanged(Arc<CatalogProjection>),
    /// Application acknowledgement.
    Notice(UiNotice),
    /// Deterministic monotonic time update.
    Tick(UiInstant),
    /// Terminal resize notification.
    Resize,
    /// Process-level shutdown request.
    ShutdownRequested,
}

impl Debug for Message {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(_) => formatter.write_str("Input([REDACTED])"),
            Self::Paste(_) => formatter.write_str("Paste([REDACTED])"),
            Self::SessionChanged(session) => formatter
                .debug_tuple("SessionChanged")
                .field(session)
                .finish(),
            Self::CatalogChanged(catalog) => formatter
                .debug_tuple("CatalogChanged")
                .field(catalog)
                .finish(),
            Self::Notice(notice) => formatter.debug_tuple("Notice").field(notice).finish(),
            Self::Tick(now) => formatter.debug_tuple("Tick").field(now).finish(),
            Self::Resize => formatter.write_str("Resize"),
            Self::ShutdownRequested => formatter.write_str("ShutdownRequested"),
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
    pub(crate) pending: BTreeMap<RequestId, PendingKind>,
    pub(crate) cancelling: BTreeSet<AttemptKey>,
    pub(crate) retrying: BTreeSet<AttemptKey>,
    pub(crate) answering_permissions: BTreeSet<ToolCallKey>,
    pub(crate) permission_scroll: u16,
    pub(crate) retry_deadlines: BTreeMap<AttemptKey, UiInstant>,
    pub(crate) catalog_retry_deadline: Option<UiInstant>,
    pub(crate) next_request_id: u64,
    pub(crate) now: UiInstant,
}

impl Model {
    /// Creates UI state from application-provided read models.
    #[must_use]
    pub fn new(session: Arc<SessionProjection>, catalog: Arc<CatalogProjection>) -> Self {
        let permission_pending = !session.permission_requests.is_empty();
        let open_credential =
            !permission_pending && matches!(&*catalog, CatalogProjection::CredentialRequired);
        let open_picker = !open_credential
            && session.selected_model.is_none()
            && matches!(&*catalog, CatalogProjection::Ready { models, .. } if !models.is_empty());
        let focus = if permission_pending {
            Focus::Permission
        } else if open_credential {
            Focus::Credential
        } else if open_picker {
            Focus::Picker
        } else {
            Focus::Composer
        };
        let selected = session.selected_model.clone().or_else(|| {
            catalog
                .models()
                .iter()
                .find(|model| model.selectable)
                .map(|model| model.model.clone())
        });

        let mut model = Self {
            session,
            catalog,
            composer: ComposerState::default(),
            transcript: TranscriptState::new(),
            focus,
            notice: None,
            dirty: true,
            should_quit: false,
            picker: PickerState {
                open: open_picker,
                query: String::new(),
                selected,
            },
            credential: CredentialState {
                open: open_credential,
                ..CredentialState::default()
            },
            pending: BTreeMap::new(),
            cancelling: BTreeSet::new(),
            retrying: BTreeSet::new(),
            answering_permissions: BTreeSet::new(),
            permission_scroll: 0,
            retry_deadlines: BTreeMap::new(),
            catalog_retry_deadline: None,
            next_request_id: 1,
            now: 0,
        };
        model.sync_retry_deadline();
        model.sync_catalog_retry_deadline();
        model
    }

    /// Returns whether the picker overlay is open.
    #[must_use]
    pub const fn picker_open(&self) -> bool {
        self.picker.open
    }

    /// Returns whether the API-key overlay is open.
    #[must_use]
    pub const fn credential_open(&self) -> bool {
        self.credential.open
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
