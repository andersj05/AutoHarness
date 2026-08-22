use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{
    AttemptFailure, AttemptId, DeliveryMode, InputId, ModelRef, PromptText, SessionId,
    SessionSequence, SessionTitle, TimestampMillis, UsageSnapshot, ValueError,
};

/// Current durable session lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    /// The session accepts commands.
    Active,
    /// The session is retained but does not accept ordinary commands.
    Archived,
}

/// A read-optimized session list item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSummary {
    session_id: SessionId,
    status: SessionStatus,
    title: Option<SessionTitle>,
    selected_model: Option<ModelRef>,
    last_sequence: SessionSequence,
    created_at: TimestampMillis,
    updated_at: TimestampMillis,
}

impl SessionSummary {
    /// Constructs a validated session summary from a store implementation.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        status: SessionStatus,
        title: Option<SessionTitle>,
        selected_model: Option<ModelRef>,
        last_sequence: SessionSequence,
        created_at: TimestampMillis,
        updated_at: TimestampMillis,
    ) -> Self {
        Self {
            session_id,
            status,
            title,
            selected_model,
            last_sequence,
            created_at,
            updated_at,
        }
    }

    /// Returns the stable session identity.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the session lifecycle state.
    #[must_use]
    pub const fn status(&self) -> SessionStatus {
        self.status
    }

    /// Returns the latest user-facing title, when one was set.
    #[must_use]
    pub const fn title(&self) -> Option<&SessionTitle> {
        self.title.as_ref()
    }

    /// Returns a deterministic browser label: the explicit title when set,
    /// otherwise the fixed fallback derived from durable identity.
    ///
    /// The fallback is intentionally not transcript-derived so offline
    /// browsing never leaks provider content into unattended surfaces.
    #[must_use]
    pub fn display_title(&self) -> String {
        match &self.title {
            Some(title) => title.as_str().to_owned(),
            None => format!("Untitled session {}", self.session_id.as_str()),
        }
    }

    /// Returns the latest selected model projection.
    #[must_use]
    pub const fn selected_model(&self) -> Option<&ModelRef> {
        self.selected_model.as_ref()
    }

    /// Returns the last authoritative event sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> SessionSequence {
        self.last_sequence
    }

    /// Returns the creation event's observed time.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMillis {
        self.created_at
    }

    /// Returns the latest event's observed time.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMillis {
        self.updated_at
    }
}

/// Current durable input lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputState {
    /// The input is durable and eligible under its delivery mode.
    Admitted,
    /// The input has been bound to a provider turn.
    Promoted,
}

/// One exact durable user input projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedInputRecord {
    session_id: SessionId,
    input_id: InputId,
    sequence: SessionSequence,
    prompt: PromptText,
    delivery_mode: DeliveryMode,
    state: InputState,
    admitted_at: TimestampMillis,
}

impl AdmittedInputRecord {
    /// Constructs an admitted-input read record.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        session_id: SessionId,
        input_id: InputId,
        sequence: SessionSequence,
        prompt: PromptText,
        delivery_mode: DeliveryMode,
        state: InputState,
        admitted_at: TimestampMillis,
    ) -> Self {
        Self {
            session_id,
            input_id,
            sequence,
            prompt,
            delivery_mode,
            state,
            admitted_at,
        }
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the input identity.
    #[must_use]
    pub const fn input_id(&self) -> &InputId {
        &self.input_id
    }

    /// Returns the admitting event sequence.
    #[must_use]
    pub const fn sequence(&self) -> SessionSequence {
        self.sequence
    }

    /// Returns the exact user-authored prompt.
    #[must_use]
    pub const fn prompt(&self) -> &PromptText {
        &self.prompt
    }

    /// Returns the eligibility boundary.
    #[must_use]
    pub const fn delivery_mode(&self) -> DeliveryMode {
        self.delivery_mode
    }

    /// Returns the durable input lifecycle state.
    #[must_use]
    pub const fn state(&self) -> InputState {
        self.state
    }

    /// Returns the admitting event's observed time.
    #[must_use]
    pub const fn admitted_at(&self) -> TimestampMillis {
        self.admitted_at
    }
}

/// Stable source of a visible transcript entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptSource {
    /// A user message admitted under this input identity.
    Input(InputId),
    /// An assistant message emitted by this provider attempt.
    Attempt(AttemptId),
}

/// Provider-neutral transcript speaker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptRole {
    /// User-authored content.
    User,
    /// Provider-authored assistant content.
    Assistant,
}

/// Visible lifecycle state of a transcript message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptState {
    /// The complete message is durable.
    Complete,
    /// More provider segments may arrive.
    Streaming,
    /// Generation failed after any retained partial content.
    Failed,
    /// Generation was cancelled after any retained partial content.
    Cancelled,
    /// Recovery cannot determine whether the external attempt settled.
    Unknown,
}

/// Exact assembled transcript text with redacted debug output.
#[derive(Clone, Eq, PartialEq)]
pub struct TranscriptText(String);

impl TranscriptText {
    /// Constructs non-empty transcript content without normalization.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ValueError::EmptyResponseText);
        }
        Ok(Self(value))
    }

    /// Returns the exact transcript content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for TranscriptText {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranscriptText")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// One assembled transcript message projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptEntry {
    session_id: SessionId,
    source: TranscriptSource,
    role: TranscriptRole,
    state: TranscriptState,
    first_sequence: SessionSequence,
    last_sequence: SessionSequence,
    content: TranscriptText,
}

impl TranscriptEntry {
    /// Constructs one transcript entry from validated projection data.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        session_id: SessionId,
        source: TranscriptSource,
        role: TranscriptRole,
        state: TranscriptState,
        first_sequence: SessionSequence,
        last_sequence: SessionSequence,
        content: TranscriptText,
    ) -> Self {
        Self {
            session_id,
            source,
            role,
            state,
            first_sequence,
            last_sequence,
            content,
        }
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the stable message source.
    #[must_use]
    pub const fn source(&self) -> &TranscriptSource {
        &self.source
    }

    /// Returns the transcript speaker.
    #[must_use]
    pub const fn role(&self) -> TranscriptRole {
        self.role
    }

    /// Returns the visible message lifecycle state.
    #[must_use]
    pub const fn state(&self) -> TranscriptState {
        self.state
    }

    /// Returns the first contributing event sequence.
    #[must_use]
    pub const fn first_sequence(&self) -> SessionSequence {
        self.first_sequence
    }

    /// Returns the last contributing event sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> SessionSequence {
        self.last_sequence
    }

    /// Returns the exact assembled visible content.
    #[must_use]
    pub const fn content(&self) -> &TranscriptText {
        &self.content
    }
}

/// Durable provider-attempt lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptState {
    /// Input and model were durably bound before dispatch.
    Prepared,
    /// Dispatch became possible and settlement has not been observed.
    InFlight,
    /// The provider response completed successfully.
    Completed,
    /// The provider attempt settled with a safe failure.
    Failed,
    /// Cooperative cancellation settled the provider attempt.
    Cancelled,
    /// Recovery could not determine the external provider outcome.
    Unknown,
}

/// Read projection for one durable provider attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptRecord {
    session_id: SessionId,
    attempt_id: AttemptId,
    input_id: InputId,
    model: ModelRef,
    retry_of: Option<AttemptId>,
    state: AttemptState,
    prepared_sequence: SessionSequence,
    prepared_at: TimestampMillis,
    started_at: Option<TimestampMillis>,
    settled_at: Option<TimestampMillis>,
    cancellation_requested_at: Option<TimestampMillis>,
    usage: Option<UsageSnapshot>,
    failure: Option<AttemptFailure>,
}

impl AttemptRecord {
    /// Constructs a validated attempt read record.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        session_id: SessionId,
        attempt_id: AttemptId,
        input_id: InputId,
        model: ModelRef,
        retry_of: Option<AttemptId>,
        state: AttemptState,
        prepared_sequence: SessionSequence,
        prepared_at: TimestampMillis,
        started_at: Option<TimestampMillis>,
        settled_at: Option<TimestampMillis>,
        cancellation_requested_at: Option<TimestampMillis>,
        usage: Option<UsageSnapshot>,
        failure: Option<AttemptFailure>,
    ) -> Self {
        Self {
            session_id,
            attempt_id,
            input_id,
            model,
            retry_of,
            state,
            prepared_sequence,
            prepared_at,
            started_at,
            settled_at,
            cancellation_requested_at,
            usage,
            failure,
        }
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the stable attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    /// Returns the exact promoted input identity.
    #[must_use]
    pub const fn input_id(&self) -> &InputId {
        &self.input_id
    }

    /// Returns the provider-neutral model snapshot.
    #[must_use]
    pub const fn model(&self) -> &ModelRef {
        &self.model
    }

    /// Returns the prior settled attempt when this is an explicit retry.
    #[must_use]
    pub const fn retry_of(&self) -> Option<&AttemptId> {
        self.retry_of.as_ref()
    }

    /// Returns the durable attempt state.
    #[must_use]
    pub const fn state(&self) -> AttemptState {
        self.state
    }

    /// Returns the preparation event sequence.
    #[must_use]
    pub const fn prepared_sequence(&self) -> SessionSequence {
        self.prepared_sequence
    }

    /// Returns when the attempt was prepared.
    #[must_use]
    pub const fn prepared_at(&self) -> TimestampMillis {
        self.prepared_at
    }

    /// Returns when dispatch became possible.
    #[must_use]
    pub const fn started_at(&self) -> Option<TimestampMillis> {
        self.started_at
    }

    /// Returns when a terminal outcome was recorded.
    #[must_use]
    pub const fn settled_at(&self) -> Option<TimestampMillis> {
        self.settled_at
    }

    /// Returns when cancellation was durably requested.
    #[must_use]
    pub const fn cancellation_requested_at(&self) -> Option<TimestampMillis> {
        self.cancellation_requested_at
    }

    /// Returns the latest cumulative usage snapshot.
    #[must_use]
    pub const fn usage(&self) -> Option<UsageSnapshot> {
        self.usage
    }

    /// Returns the sanitized terminal failure when present.
    #[must_use]
    pub const fn failure(&self) -> Option<&AttemptFailure> {
        self.failure.as_ref()
    }
}
