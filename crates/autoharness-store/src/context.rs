use std::fmt::{self, Debug, Formatter};

use autoharness_domain::{
    AttemptId, ContextAdmission, ContextAdmissionId, ContextEpochId, ContextEpochManifest,
    ContextEpochReason, ContextTurnId, ContextTurnManifest, EventEnvelope, MemoryGeneration,
    MemoryRevisionId, SessionId, SessionSequence, Sha256Digest, TimestampMillis,
};

use crate::StoreError;

/// Maximum exact rendered context bytes retained for one turn or admission.
pub const MAX_RENDERED_CONTEXT_BYTES: usize = 256 * 1024;

/// Bounded provider-visible context bytes whose debug output is always redacted.
#[derive(Clone, Eq, PartialEq)]
pub struct RenderedContextText(String);

impl RenderedContextText {
    /// Validates exact rendered UTF-8 before it crosses the storage boundary.
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_RENDERED_CONTEXT_BYTES {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self(value))
    }

    /// Returns the exact provider-visible bytes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for RenderedContextText {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenderedContextText")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// Exact erasable rendering for one immutable admission decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextAdmissionContent {
    admission_id: ContextAdmissionId,
    rendered: RenderedContextText,
}

impl ContextAdmissionContent {
    /// Constructs one admission rendering sidecar.
    #[must_use]
    pub const fn new(admission_id: ContextAdmissionId, rendered: RenderedContextText) -> Self {
        Self {
            admission_id,
            rendered,
        }
    }

    /// Returns the admission metadata identity.
    #[must_use]
    pub const fn admission_id(&self) -> &ContextAdmissionId {
        &self.admission_id
    }

    /// Returns the exact rendered admission bytes.
    #[must_use]
    pub const fn rendered(&self) -> &RenderedContextText {
        &self.rendered
    }
}

/// All erasable provider-visible bytes accompanying one context manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextTurnContent {
    prelude: Option<RenderedContextText>,
    admissions: Vec<ContextAdmissionContent>,
}

impl ContextTurnContent {
    /// Constructs exact turn and per-admission rendering sidecars.
    #[must_use]
    pub const fn new(
        prelude: Option<RenderedContextText>,
        admissions: Vec<ContextAdmissionContent>,
    ) -> Self {
        Self {
            prelude,
            admissions,
        }
    }

    /// Returns the exact provider prelude, when the turn has one.
    #[must_use]
    pub const fn prelude(&self) -> Option<&RenderedContextText> {
        self.prelude.as_ref()
    }

    /// Returns rendered admissions in manifest rank order.
    #[must_use]
    pub fn admissions(&self) -> &[ContextAdmissionContent] {
        &self.admissions
    }
}

/// One atomic context-turn persistence request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextTurnCommitRequest {
    epoch: Option<ContextEpochManifest>,
    turn: ContextTurnManifest,
    content: ContextTurnContent,
}

impl ContextTurnCommitRequest {
    /// Creates an atomic context-turn request.
    #[must_use]
    pub const fn new(
        epoch: Option<ContextEpochManifest>,
        turn: ContextTurnManifest,
        content: ContextTurnContent,
    ) -> Self {
        Self {
            epoch,
            turn,
            content,
        }
    }

    /// Returns the new epoch, when this turn starts one.
    #[must_use]
    pub const fn epoch(&self) -> Option<&ContextEpochManifest> {
        self.epoch.as_ref()
    }

    /// Returns the provider-turn manifest.
    #[must_use]
    pub const fn turn(&self) -> &ContextTurnManifest {
        &self.turn
    }

    /// Returns exact erasable renderings accompanying the metadata manifest.
    #[must_use]
    pub const fn content(&self) -> &ContextTurnContent {
        &self.content
    }
}

/// Whether a context-turn commit was new or an exact retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextCommitDisposition {
    /// This call committed the context turn.
    Committed,
    /// The byte-equivalent context turn already existed.
    AlreadyCommitted,
}

/// Explicit verified durable-fact boundary for a compaction epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextCompactionBoundary {
    epoch_id: ContextEpochId,
    predecessor_epoch_id: ContextEpochId,
    session_id: SessionId,
    expected_session_sequence: SessionSequence,
    memory_generation: MemoryGeneration,
    facts_version: u16,
    facts_hash: Sha256Digest,
    memory_fact_count: u32,
    pending_session_fact_count: u32,
    summary_revision_id: Option<MemoryRevisionId>,
    verified_at: TimestampMillis,
}

impl ContextCompactionBoundary {
    /// Constructs a contentless compaction verification record.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        epoch_id: ContextEpochId,
        predecessor_epoch_id: ContextEpochId,
        session_id: SessionId,
        expected_session_sequence: SessionSequence,
        memory_generation: MemoryGeneration,
        facts_version: u16,
        facts_hash: Sha256Digest,
        memory_fact_count: u32,
        pending_session_fact_count: u32,
        summary_revision_id: Option<MemoryRevisionId>,
        verified_at: TimestampMillis,
    ) -> Self {
        Self {
            epoch_id,
            predecessor_epoch_id,
            session_id,
            expected_session_sequence,
            memory_generation,
            facts_version,
            facts_hash,
            memory_fact_count,
            pending_session_fact_count,
            summary_revision_id,
            verified_at,
        }
    }

    /// Returns the new compaction epoch.
    #[must_use]
    pub const fn epoch_id(&self) -> &ContextEpochId {
        &self.epoch_id
    }

    /// Returns the complete predecessor epoch.
    #[must_use]
    pub const fn predecessor_epoch_id(&self) -> &ContextEpochId {
        &self.predecessor_epoch_id
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the optimistic session version sampled for the fingerprint.
    #[must_use]
    pub const fn expected_session_sequence(&self) -> SessionSequence {
        self.expected_session_sequence
    }

    /// Returns the optimistic memory generation sampled for the fingerprint.
    #[must_use]
    pub const fn memory_generation(&self) -> MemoryGeneration {
        self.memory_generation
    }

    /// Returns the pure durable-facts hashing contract version.
    #[must_use]
    pub const fn facts_version(&self) -> u16 {
        self.facts_version
    }

    /// Returns the independently computed effective durable-facts fingerprint.
    #[must_use]
    pub const fn facts_hash(&self) -> &Sha256Digest {
        &self.facts_hash
    }

    /// Returns the number of active memory facts hashed.
    #[must_use]
    pub const fn memory_fact_count(&self) -> u32 {
        self.memory_fact_count
    }

    /// Returns the number of unsettled session facts hashed.
    #[must_use]
    pub const fn pending_session_fact_count(&self) -> u32 {
        self.pending_session_fact_count
    }

    /// Returns the optional untrusted compaction-summary proposal revision.
    #[must_use]
    pub const fn summary_revision_id(&self) -> Option<&MemoryRevisionId> {
        self.summary_revision_id.as_ref()
    }

    /// Returns when the authoritative snapshot was verified.
    #[must_use]
    pub const fn verified_at(&self) -> TimestampMillis {
        self.verified_at
    }
}

/// One consistent canonical-facts read for a prospective compaction turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionFactsSnapshot {
    epoch_id: ContextEpochId,
    session_id: SessionId,
    expected_session_sequence: SessionSequence,
    memory_generation: MemoryGeneration,
    facts_version: u16,
    facts_hash: Sha256Digest,
    memory_fact_count: u32,
    pending_session_fact_count: u32,
}

impl CompactionFactsSnapshot {
    /// Constructs one contentless optimistic compaction proof snapshot.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        epoch_id: ContextEpochId,
        session_id: SessionId,
        expected_session_sequence: SessionSequence,
        memory_generation: MemoryGeneration,
        facts_version: u16,
        facts_hash: Sha256Digest,
        memory_fact_count: u32,
        pending_session_fact_count: u32,
    ) -> Self {
        Self {
            epoch_id,
            session_id,
            expected_session_sequence,
            memory_generation,
            facts_version,
            facts_hash,
            memory_fact_count,
            pending_session_fact_count,
        }
    }

    /// Returns the prospective compaction epoch.
    #[must_use]
    pub const fn epoch_id(&self) -> &ContextEpochId {
        &self.epoch_id
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the exact optimistic session prefix hashed.
    #[must_use]
    pub const fn expected_session_sequence(&self) -> SessionSequence {
        self.expected_session_sequence
    }

    /// Returns the exact optimistic memory eligibility generation hashed.
    #[must_use]
    pub const fn memory_generation(&self) -> MemoryGeneration {
        self.memory_generation
    }

    /// Returns the canonical facts contract version.
    #[must_use]
    pub const fn facts_version(&self) -> u16 {
        self.facts_version
    }

    /// Returns the canonical effective durable-facts digest.
    #[must_use]
    pub const fn facts_hash(&self) -> &Sha256Digest {
        &self.facts_hash
    }

    /// Returns the number of eligible active retained memory facts hashed.
    #[must_use]
    pub const fn memory_fact_count(&self) -> u32 {
        self.memory_fact_count
    }

    /// Returns the number of unsettled session facts hashed.
    #[must_use]
    pub const fn pending_session_fact_count(&self) -> u32 {
        self.pending_session_fact_count
    }
}

/// Latest verified compaction boundary with its exact immutable baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextCompactionCheckpoint {
    boundary: ContextCompactionBoundary,
    epoch: ContextEpochManifest,
    baseline_turn: ContextTurnManifest,
}

impl ContextCompactionCheckpoint {
    /// Constructs a checkpoint only when all three records name one exact baseline.
    pub fn new(
        boundary: ContextCompactionBoundary,
        epoch: ContextEpochManifest,
        baseline_turn: ContextTurnManifest,
    ) -> Result<Self, StoreError> {
        if epoch.reason() != ContextEpochReason::Compaction
            || boundary.epoch_id() != epoch.epoch_id()
            || Some(boundary.predecessor_epoch_id()) != epoch.predecessor_epoch_id()
            || boundary.session_id() != epoch.session_id()
            || baseline_turn.epoch_id() != epoch.epoch_id()
            || baseline_turn.session_id() != epoch.session_id()
            || boundary.expected_session_sequence() != baseline_turn.expected_session_sequence()
            || boundary.memory_generation() != epoch.memory_generation()
            || boundary.memory_generation() != baseline_turn.memory_generation()
        {
            return Err(StoreError::InvalidContextTransition);
        }
        Ok(Self {
            boundary,
            epoch,
            baseline_turn,
        })
    }

    /// Returns the verified canonical-facts boundary.
    #[must_use]
    pub const fn boundary(&self) -> &ContextCompactionBoundary {
        &self.boundary
    }

    /// Returns the immutable compaction epoch.
    #[must_use]
    pub const fn epoch(&self) -> &ContextEpochManifest {
        &self.epoch
    }

    /// Returns the first bound turn that established the compaction epoch.
    #[must_use]
    pub const fn baseline_turn(&self) -> &ContextTurnManifest {
        &self.baseline_turn
    }
}

/// One atomic commit of context metadata, exact rendered bytes, and its session binding event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundContextTurnCommitRequest {
    context: ContextTurnCommitRequest,
    binding_event: EventEnvelope,
    compaction_boundary: Option<ContextCompactionBoundary>,
}

impl BoundContextTurnCommitRequest {
    /// Constructs a context and session binding request.
    #[must_use]
    pub const fn new(context: ContextTurnCommitRequest, binding_event: EventEnvelope) -> Self {
        Self {
            context,
            binding_event,
            compaction_boundary: None,
        }
    }

    /// Attaches the explicit durable-facts boundary required by a compaction epoch.
    #[must_use]
    pub fn with_compaction_boundary(mut self, boundary: ContextCompactionBoundary) -> Self {
        self.compaction_boundary = Some(boundary);
        self
    }

    /// Returns the immutable context turn and erasable sidecars.
    #[must_use]
    pub const fn context(&self) -> &ContextTurnCommitRequest {
        &self.context
    }

    /// Returns the exact `ContextTurnBound` session event committed with the context turn.
    #[must_use]
    pub const fn binding_event(&self) -> &EventEnvelope {
        &self.binding_event
    }

    /// Returns the compaction verification record, when this starts a compaction epoch.
    #[must_use]
    pub const fn compaction_boundary(&self) -> Option<&ContextCompactionBoundary> {
        self.compaction_boundary.as_ref()
    }
}

/// Result of atomically committing context and its session binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundContextTurnCommitReceipt {
    disposition: ContextCommitDisposition,
    last_sequence: u64,
}

impl BoundContextTurnCommitReceipt {
    /// Constructs an atomic context binding receipt.
    #[must_use]
    pub const fn new(disposition: ContextCommitDisposition, last_sequence: u64) -> Self {
        Self {
            disposition,
            last_sequence,
        }
    }

    /// Returns whether the call committed new rows or reconciled an exact retry.
    #[must_use]
    pub const fn disposition(self) -> ContextCommitDisposition {
        self.disposition
    }

    /// Returns the last durable session sequence after the binding append.
    #[must_use]
    pub const fn last_sequence(self) -> u64 {
        self.last_sequence
    }
}

/// Durable context-turn persistence and inspection.
pub trait ContextStore {
    /// Atomically persists one immutable context turn and all admissions.
    fn commit_context_turn(
        &mut self,
        request: &ContextTurnCommitRequest,
    ) -> Result<ContextCommitDisposition, StoreError>;

    /// Atomically persists context and appends its exact `ContextTurnBound` session event.
    ///
    /// Callers must use this boundary before provider dispatch. A standalone context commit is
    /// useful only for non-dispatch inspection and recovery staging.
    fn commit_context_turn_and_bind(
        &mut self,
        request: &BoundContextTurnCommitRequest,
    ) -> Result<BoundContextTurnCommitReceipt, StoreError>;

    /// Loads the active epoch for one session, when present.
    fn load_context_epoch(
        &self,
        epoch_id: &ContextEpochId,
    ) -> Result<Option<ContextEpochManifest>, StoreError>;

    /// Loads the explicit verified durable-facts boundary for one compaction epoch.
    fn load_compaction_boundary(
        &self,
        epoch_id: &ContextEpochId,
    ) -> Result<Option<ContextCompactionBoundary>, StoreError>;

    /// Loads the newest verified compaction checkpoint for one session.
    fn load_latest_compaction_checkpoint(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ContextCompactionCheckpoint>, StoreError>;

    /// Computes a consistent optimistic canonical-facts snapshot for a prospective epoch.
    ///
    /// The later atomic bind must independently recompute this proof before persisting it.
    fn load_compaction_facts_snapshot(
        &mut self,
        epoch: &ContextEpochManifest,
        turn: &ContextTurnManifest,
    ) -> Result<CompactionFactsSnapshot, StoreError>;

    /// Loads one exact persisted context turn.
    fn load_context_turn(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Option<ContextTurnManifest>, StoreError>;

    /// Loads admissions for one context turn in ordinal order.
    fn load_context_admissions(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Vec<ContextAdmission>, StoreError>;

    /// Loads the retained exact provider-visible prelude, when still available.
    fn load_context_turn_content(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Option<RenderedContextText>, StoreError>;

    /// Loads one retained exact admission rendering, when still available.
    fn load_context_admission_content(
        &self,
        admission_id: &ContextAdmissionId,
    ) -> Result<Option<RenderedContextText>, StoreError>;

    /// Finds the exact durable context manifest for an attempt and turn number.
    fn load_attempt_context_turn(
        &self,
        attempt_id: &AttemptId,
        turn: u32,
    ) -> Result<Option<ContextTurnManifest>, StoreError>;

    /// Loads the first bound provider turn that established one exact epoch.
    fn load_context_epoch_baseline(
        &self,
        epoch_id: &ContextEpochId,
    ) -> Result<Option<ContextTurnManifest>, StoreError>;
}
