use autoharness_domain::{EventEnvelope, SessionId};

use crate::{AdmittedInputRecord, AttemptRecord, SessionSummary, StoreError, TranscriptEntry};

/// Default upper bound for one event-log read.
pub const DEFAULT_EVENT_PAGE_SIZE: u32 = 512;

/// An atomic compare-and-append request for one session event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendRequest {
    session_id: SessionId,
    expected_last_sequence: u64,
    events: Vec<EventEnvelope>,
}

impl AppendRequest {
    /// Creates an append request.
    ///
    /// Validation is performed by the store so malformed requests have the
    /// same behavior across implementations.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        expected_last_sequence: u64,
        events: Vec<EventEnvelope>,
    ) -> Self {
        Self {
            session_id,
            expected_last_sequence,
            events,
        }
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the durable version the caller used to decide the event batch.
    #[must_use]
    pub const fn expected_last_sequence(&self) -> u64 {
        self.expected_last_sequence
    }

    /// Returns the complete event batch.
    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        &self.events
    }
}

/// Whether an append performed a new commit or reconciled an exact prior commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendDisposition {
    /// The event batch was committed by this call.
    Committed,
    /// Every event had already been atomically committed with identical bytes.
    AlreadyCommitted,
}

/// Result of an atomic event append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppendReceipt {
    disposition: AppendDisposition,
    last_sequence: u64,
}

impl AppendReceipt {
    /// Constructs an append receipt.
    #[must_use]
    pub const fn new(disposition: AppendDisposition, last_sequence: u64) -> Self {
        Self {
            disposition,
            last_sequence,
        }
    }

    /// Returns whether this call committed or reconciled the batch.
    #[must_use]
    pub const fn disposition(self) -> AppendDisposition {
        self.disposition
    }

    /// Returns the last durable sequence after the append.
    #[must_use]
    pub const fn last_sequence(self) -> u64 {
        self.last_sequence
    }
}

/// Provider-neutral durable session transactions and read projections.
///
/// Implementations are synchronous so a concrete blocking adapter can be owned
/// by an application storage task without introducing an async runtime into the
/// domain-facing port.
pub trait SessionStore {
    /// Atomically appends events and all affected projections.
    fn append(&mut self, request: &AppendRequest) -> Result<AppendReceipt, StoreError>;

    /// Loads at most `limit` events after the exclusive session sequence.
    fn load_events(
        &self,
        session_id: &SessionId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<EventEnvelope>, StoreError>;

    /// Lists durable session projections in deterministic recent-first order.
    fn list_sessions(&self) -> Result<Vec<SessionSummary>, StoreError>;

    /// Loads admitted inputs in their authoritative event order.
    fn load_admitted_inputs(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<AdmittedInputRecord>, StoreError>;

    /// Loads provider attempts in their preparation-event order.
    fn load_attempts(&self, session_id: &SessionId) -> Result<Vec<AttemptRecord>, StoreError>;

    /// Loads the visible transcript in authoritative event order.
    fn load_transcript(&self, session_id: &SessionId) -> Result<Vec<TranscriptEntry>, StoreError>;

    /// Rebuilds every read projection from retained authoritative events.
    fn rebuild_projections(&mut self) -> Result<(), StoreError>;

    /// Removes one session and every dependent row inside a single transaction.
    ///
    /// Implementations must reject deletion of a session that still has
    /// unsettled provider attempts so an in-flight attempt can never lose its
    /// durable history, and must report whether the session existed.
    fn delete_session(
        &mut self,
        session_id: &SessionId,
        expected_last_sequence: u64,
    ) -> Result<DeletionDisposition, StoreError>;
}

/// Whether an explicit deletion removed the session or found nothing to remove.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletionDisposition {
    /// The session and its dependents were removed by this call.
    Deleted,
    /// No session with that identity existed.
    NotFound,
}
