use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelRef, RetryAdvice};
use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, EffectiveLocalProfile, GlyphMode, Layout,
    TerminalTimestampStyle, ThemePreset,
};
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
}

impl Route {
    /// Stable ordered route table used by navigation and rendering.
    pub const ALL: [Self; 5] = [
        Self::Chat,
        Self::Sessions,
        Self::Profiles,
        Self::Settings,
        Self::Help,
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
}

/// Runner-side side effects the pure update layer cannot perform itself.
#[derive(Debug)]
pub enum UiEffect {
    /// Dispatch an intent through the bounded application mailbox.
    Dispatch(UiIntent),
    /// Start the official Codex CLI browser-login flow without handling credentials.
    LaunchCodexLogin,
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
}

impl UiIntent {
    /// Returns the local request identity.
    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        match self {
            Self::CreateSession { request_id }
            | Self::ConfigureCredential { request_id, .. }
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
            | Self::ExportTranscript { request_id, .. } => *request_id,
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
}
/// Semantic mouse actions produced by terminal hit testing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MouseAction {
    /// Switch to one of the primary shell routes.
    Route(Route),
    /// Switch to one of the nested Settings tabs.
    SettingsTab(usize),
    /// Open the local user-profile dialog.
    OpenUserProfile,
    /// Chat footer actions.
    ChatSend,
    ChatModels,
    ChatNewSession,
    ChatSessions,
    ChatCredential,
    ChatHelp,
    /// Profile-center actions.
    ProfileNew,
    ProfileCredential,
    ProfileTest,
    ProfileDefaultModel,
    ProfileDisconnect,
    ProfileDelete,
    /// Select a provider setup choice.
    SelectProviderChoice(usize),
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
    PermissionAllow,
    PermissionDeny,
}

/// Input to the deterministic update function.
pub enum Message {
    /// Backend-independent keyboard input.
    Input(ratatui_textarea::Input),
    /// Semantic click action derived from a visible terminal hit region.
    Mouse(MouseAction),
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
            Self::Mouse(action) => formatter.debug_tuple("Mouse").field(action).finish(),
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
            Self::Notice(notice) => formatter.debug_tuple("Notice").field(notice).finish(),
            Self::Tick(now) => formatter.debug_tuple("Tick").field(now).finish(),
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

/// Provider-choice, authentication-page, and account-editor state.
#[derive(Debug, Default)]
pub(crate) struct ProfileCenterState {
    pub focus: ProfileCenterFocus,
    pub choice_selected: usize,
    pub auth_page: Option<ProviderChoice>,
    pub auth_selected: usize,
    pub open_credential_after_save: Option<String>,
    pub query: String,
    pub selected: Option<String>,
    pub confirming_disconnect: Option<String>,
    pub editor: Option<ProfileEditorState>,
    pub credential: Option<ProfileCredentialEditor>,
    pub confirming_delete: Option<String>,
}

/// Step in the keyboard-first default-agent selection sequence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AgentDefaultStep {
    /// Select a connected provider account.
    #[default]
    Provider,
    /// Select one compatible model from the active provider catalog.
    Model,
    /// Confirm the only currently portable thinking choice.
    Thinking,
}

/// Local state for selecting the default agent provider, model, and thinking mode.
#[derive(Debug, Default)]
pub(crate) struct AgentDefaultsState {
    pub step: AgentDefaultStep,
    pub profile_selected: usize,
    pub model_selected: usize,
    pub profile_id: Option<String>,
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
/// The selectable rows in deterministic Settings workspace order.
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
    ReducedMotion,
    Density,
    Approvals,
    Retention,
    Logging,
    Layout,
    TerminalTimestampStyle,
    ComposerSubmitBehavior,
}

impl SettingsPreference {
    pub(crate) const ALL: [Self; 18] = [
        Self::DisplayLabel,
        Self::Provider,
        Self::Profile,
        Self::Credential,
        Self::Source,
        Self::Model,
        Self::Mode,
        Self::ThemePreset,
        Self::ColorMode,
        Self::GlyphMode,
        Self::ReducedMotion,
        Self::Density,
        Self::Approvals,
        Self::Retention,
        Self::Logging,
        Self::Layout,
        Self::TerminalTimestampStyle,
        Self::ComposerSubmitBehavior,
    ];

    #[must_use]
    pub(crate) fn at(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len().saturating_sub(1))]
    }
}

/// Inline state owned exclusively by the Settings route.
#[derive(Debug, Default)]
pub(crate) struct SettingsState {
    /// Whether the top-level Settings navigation owns arrow-key focus.
    pub nav_focus: bool,
    /// Index into the top-level Settings navigation.
    pub nav_selected: usize,
    /// Index into `SettingsPreference::ALL`.
    pub selected: usize,
    /// First rendered Settings line kept visible while selecting preferences.
    pub scroll: u16,
    /// Buffered local-label edit; no value is persisted until Enter.
    pub display_label_editor: Option<String>,
}

pub(crate) const SETTINGS_NAV_COUNT: usize = 4;

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
                "Alt+1..5",
                "switch Chat, Sessions, Profiles, Settings, Help",
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
            ("Alt+Up / Alt+Down", "scroll the transcript"),
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
            ("Alt+Up / Alt+Down", "scroll the transcript"),
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
            ("Left/Right", "move between Settings pages"),
            ("Down", "enter Settings preferences"),
            ("Up/Down", "choose a preference"),
            ("PageUp/PageDown", "move through settings"),
            ("Home/End", "jump to the first or last preference"),
            ("Enter", "activate a page or edit the display label"),
            ("R / D", "inherit or restore the user default"),
            ("Esc", "return to Chat"),
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
        id: "agents",
        label: "Agents settings",
        description: "Open the Agents tab in Settings",
        key_hint: None,
    },
    CommandEntry {
        id: "user-profile",
        label: "User profile",
        description: "Edit the local display name and profile summary",
        key_hint: Some("Alt+U"),
    },
    CommandEntry {
        id: "new-session",
        label: "New session",
        description: "Create and activate a fresh durable session",
        key_hint: Some("Ctrl+N"),
    },
    CommandEntry {
        id: "models",
        label: "Models",
        description: "Choose a model or save it as the active provider default",
        key_hint: Some("Ctrl+P"),
    },
    CommandEntry {
        id: "refresh-models",
        label: "Refresh models",
        description: "Reload the model catalog from the provider",
        key_hint: Some("Ctrl+R in the picker"),
    },
    CommandEntry {
        id: "connect-api-key",
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
        id: "toggle-tools",
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
    /// User-owned official Codex CLI subscription session.
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
    /// Keyboard-first connected-provider default agent selection state.
    pub(crate) agent_defaults: AgentDefaultsState,
    /// Local user-profile dialog interaction state.
    pub(crate) user_profile: UserProfileState,
    /// Composer text saved while working in another session.
    pub(crate) drafts: SessionDrafts,
    /// In-run submitted-prompt history for recall.
    pub(crate) history: ComposerHistory,
    /// Active transcript search state.
    pub(crate) search: SearchState,
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
}

const STARTUP_ANIMATION_MS: UiInstant = 1_800;

impl Model {
    pub(crate) fn startup_active(&self) -> bool {
        !self.startup_complete && self.now < STARTUP_ANIMATION_MS
    }

    pub(crate) fn advance_startup(&mut self, now: UiInstant) {
        self.now = now;
        if now >= STARTUP_ANIMATION_MS {
            self.startup_complete = true;
        }
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
            agent_defaults: AgentDefaultsState::default(),
            drafts: SessionDrafts::default(),
            user_profile: UserProfileState::default(),
            history: ComposerHistory::default(),
            search: SearchState::default(),
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
        };
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
    pub const fn profile_center_open(&self) -> bool {
        matches!(self.navigation.route, Route::Profiles)
            || (matches!(self.navigation.route, Route::Settings)
                && self.settings_workspace.nav_selected == 1)
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
        let query = self.palette.query.to_lowercase();
        COMMANDS
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.id.contains(&query)
                    || entry.label.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query)
            })
            .copied()
            .collect()
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
