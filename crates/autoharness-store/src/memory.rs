use autoharness_domain::{
    AttemptId, ContextAdmissionId, ContextAdmissionReason, ContextEpochId, ContextTurnId,
    EstimatedTokens, MemoryContent, MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryGeneration,
    MemoryId, MemoryKind, MemoryOperationEnvelope, MemoryRevision, MemoryRevisionId,
    MemoryRevisionStatus, MemoryScope, MemorySubjectKey, MemoryValidationResult, ModelRef,
    Sensitivity, SessionId, TimestampMillis,
};

use crate::StoreError;

/// Default upper bound for one memory-ledger read.
pub const DEFAULT_MEMORY_PAGE_SIZE: u32 = 256;

/// Maximum number of FTS candidates returned by one store query.
pub const MAX_MEMORY_SEARCH_CANDIDATES: u32 = 256;

/// Maximum rows returned by one workspace or admission-history inspection.
pub const MAX_MEMORY_INSPECTION_PAGE_SIZE: u32 = 128;

/// An optimistic append to one durable memory-item ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAppendRequest {
    expected_last_sequence: u64,
    operation: MemoryOperationEnvelope,
    content: Option<MemoryRevisionContent>,
}

/// One operation and its optional erasable content inside a logical command batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAppendOperation {
    operation: MemoryOperationEnvelope,
    content: Option<MemoryRevisionContent>,
}

impl MemoryAppendOperation {
    /// Constructs one atomic batch member.
    #[must_use]
    pub const fn new(
        operation: MemoryOperationEnvelope,
        content: Option<MemoryRevisionContent>,
    ) -> Self {
        Self { operation, content }
    }

    /// Returns the exact durable operation.
    #[must_use]
    pub const fn operation(&self) -> &MemoryOperationEnvelope {
        &self.operation
    }

    /// Returns erasable revision content for a content-introducing operation.
    #[must_use]
    pub const fn content(&self) -> Option<&MemoryRevisionContent> {
        self.content.as_ref()
    }
}

/// A contiguous compare-and-append batch produced by one logical memory command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAppendBatchRequest {
    expected_last_sequence: u64,
    operations: Vec<MemoryAppendOperation>,
}

impl MemoryAppendBatchRequest {
    /// Constructs one atomic logical-command batch.
    #[must_use]
    pub const fn new(expected_last_sequence: u64, operations: Vec<MemoryAppendOperation>) -> Self {
        Self {
            expected_last_sequence,
            operations,
        }
    }

    /// Returns the item sequence observed by the caller.
    #[must_use]
    pub const fn expected_last_sequence(&self) -> u64 {
        self.expected_last_sequence
    }

    /// Returns contiguous operations in durable sequence order.
    #[must_use]
    pub fn operations(&self) -> &[MemoryAppendOperation] {
        &self.operations
    }
}

impl MemoryAppendRequest {
    /// Creates one compare-and-append request.
    #[must_use]
    pub const fn new(
        expected_last_sequence: u64,
        operation: MemoryOperationEnvelope,
        content: Option<MemoryRevisionContent>,
    ) -> Self {
        Self {
            expected_last_sequence,
            operation,
            content,
        }
    }

    /// Returns the item sequence observed by the caller.
    #[must_use]
    pub const fn expected_last_sequence(&self) -> u64 {
        self.expected_last_sequence
    }

    /// Returns the exact operation being appended.
    #[must_use]
    pub const fn operation(&self) -> &MemoryOperationEnvelope {
        &self.operation
    }

    /// Returns erasable content accompanying a content-introducing operation.
    #[must_use]
    pub const fn content(&self) -> Option<&MemoryRevisionContent> {
        self.content.as_ref()
    }
}

/// One erasable evidence excerpt keyed independently from durable metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryEvidenceContent {
    evidence_id: MemoryEvidenceId,
    excerpt: MemoryEvidenceExcerpt,
}

impl MemoryEvidenceContent {
    /// Constructs an evidence excerpt sidecar.
    #[must_use]
    pub const fn new(evidence_id: MemoryEvidenceId, excerpt: MemoryEvidenceExcerpt) -> Self {
        Self {
            evidence_id,
            excerpt,
        }
    }

    /// Returns the evidence identity named by revision metadata.
    #[must_use]
    pub const fn evidence_id(&self) -> &MemoryEvidenceId {
        &self.evidence_id
    }

    /// Returns the exact bounded excerpt bytes.
    #[must_use]
    pub const fn excerpt(&self) -> &MemoryEvidenceExcerpt {
        &self.excerpt
    }
}

/// Exact availability of one independently erasable evidence excerpt.
///
/// This type deliberately does not implement serialization. Callers must make an explicit
/// policy decision before copying retained evidence bytes into another authority such as export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryEvidenceExcerptState {
    /// The immutable evidence metadata never named an excerpt.
    Absent,
    /// The exact hash-verified excerpt sidecar remains available.
    Retained(MemoryEvidenceExcerpt),
    /// Metadata proves an excerpt once existed, but its sidecar was logically erased.
    Erased,
}

impl MemoryEvidenceExcerptState {
    /// Returns retained exact bytes without conflating absent and erased states.
    #[must_use]
    pub const fn retained(&self) -> Option<&MemoryEvidenceExcerpt> {
        match self {
            Self::Retained(excerpt) => Some(excerpt),
            Self::Absent | Self::Erased => None,
        }
    }
}

/// One evidence identity and the exact availability of its own excerpt sidecar.
///
/// Loading this record never follows the typed evidence source into another session, event,
/// tool result, document, or memory revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMemoryEvidenceContent {
    evidence_id: MemoryEvidenceId,
    excerpt: MemoryEvidenceExcerptState,
}

impl StoredMemoryEvidenceContent {
    /// Constructs one integrity-checked evidence sidecar read model.
    #[must_use]
    pub const fn new(evidence_id: MemoryEvidenceId, excerpt: MemoryEvidenceExcerptState) -> Self {
        Self {
            evidence_id,
            excerpt,
        }
    }

    /// Returns the identity declared by immutable revision metadata.
    #[must_use]
    pub const fn evidence_id(&self) -> &MemoryEvidenceId {
        &self.evidence_id
    }

    /// Returns whether the exact excerpt is absent, retained, or erased.
    #[must_use]
    pub const fn excerpt(&self) -> &MemoryEvidenceExcerptState {
        &self.excerpt
    }
}

/// Erasable content sidecar for one immutable revision metadata record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRevisionContent {
    revision_id: MemoryRevisionId,
    content: MemoryContent,
    evidence: Vec<MemoryEvidenceContent>,
}

impl MemoryRevisionContent {
    /// Constructs revision content and its independently erasable excerpts.
    #[must_use]
    pub const fn new(
        revision_id: MemoryRevisionId,
        content: MemoryContent,
        evidence: Vec<MemoryEvidenceContent>,
    ) -> Self {
        Self {
            revision_id,
            content,
            evidence,
        }
    }

    /// Returns the owning immutable revision identity.
    #[must_use]
    pub const fn revision_id(&self) -> &MemoryRevisionId {
        &self.revision_id
    }

    /// Returns the exact bounded revision content.
    #[must_use]
    pub const fn content(&self) -> &MemoryContent {
        &self.content
    }

    /// Returns evidence excerpts keyed by evidence identity.
    #[must_use]
    pub fn evidence(&self) -> &[MemoryEvidenceContent] {
        &self.evidence
    }
}

/// Whether a memory append performed a commit or reconciled an exact retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryAppendDisposition {
    /// The operation committed in this call.
    Committed,
    /// The exact operation was already committed.
    AlreadyCommitted,
}

/// Global monotonic version of every successfully committed logical memory mutation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryMutationGeneration(u64);

impl MemoryMutationGeneration {
    /// Constructs a persisted mutation generation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the monotonic numeric generation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Result of one atomic memory-ledger append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryAppendReceipt {
    disposition: MemoryAppendDisposition,
    last_sequence: u64,
    generation: MemoryGeneration,
    mutation_generation: MemoryMutationGeneration,
}

impl MemoryAppendReceipt {
    /// Constructs a durable memory receipt.
    #[must_use]
    pub const fn new(
        disposition: MemoryAppendDisposition,
        last_sequence: u64,
        generation: MemoryGeneration,
        mutation_generation: MemoryMutationGeneration,
    ) -> Self {
        Self {
            disposition,
            last_sequence,
            generation,
            mutation_generation,
        }
    }

    /// Returns the commit disposition.
    #[must_use]
    pub const fn disposition(self) -> MemoryAppendDisposition {
        self.disposition
    }

    /// Returns the final item sequence.
    #[must_use]
    pub const fn last_sequence(self) -> u64 {
        self.last_sequence
    }

    /// Returns the global memory generation after the operation.
    #[must_use]
    pub const fn generation(self) -> MemoryGeneration {
        self.generation
    }

    /// Returns the global projection mutation generation after the append.
    #[must_use]
    pub const fn mutation_generation(self) -> MemoryMutationGeneration {
        self.mutation_generation
    }
}

/// Exact scope eligibility and time boundary for deterministic FTS retrieval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemorySearchQuery {
    query: MemoryContent,
    eligible_scopes: Vec<MemoryScope>,
    sensitivity_ceiling: Sensitivity,
    as_of: TimestampMillis,
    limit: u32,
}

impl MemorySearchQuery {
    /// Creates a bounded candidate query.
    pub fn new(
        query: MemoryContent,
        eligible_scopes: Vec<MemoryScope>,
        sensitivity_ceiling: Sensitivity,
        as_of: TimestampMillis,
        limit: u32,
    ) -> Result<Self, StoreError> {
        if eligible_scopes.is_empty() || limit == 0 || limit > MAX_MEMORY_SEARCH_CANDIDATES {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self {
            query,
            eligible_scopes,
            sensitivity_ceiling,
            as_of,
            limit,
        })
    }

    /// Returns literal user query content.
    #[must_use]
    pub const fn query(&self) -> &MemoryContent {
        &self.query
    }

    /// Returns the exact eligible scopes.
    #[must_use]
    pub fn eligible_scopes(&self) -> &[MemoryScope] {
        &self.eligible_scopes
    }

    /// Returns the maximum authorized sensitivity applied before the SQL limit.
    #[must_use]
    pub const fn sensitivity_ceiling(&self) -> Sensitivity {
        self.sensitivity_ceiling
    }

    /// Returns the explicit validity observation time.
    #[must_use]
    pub const fn as_of(&self) -> TimestampMillis {
        self.as_of
    }

    /// Returns the candidate bound.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }
}

/// Bounded structured query for active heads without lexical candidate generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMemoryHeadQuery {
    eligible_scopes: Vec<MemoryScope>,
    memory_kind: MemoryKind,
    subject_key: Option<MemorySubjectKey>,
    content_hash: Option<autoharness_domain::Sha256Digest>,
    limit: u32,
}

impl ActiveMemoryHeadQuery {
    /// Constructs an exact scope-and-kind query with an optional subject filter.
    pub fn new(
        eligible_scopes: Vec<MemoryScope>,
        memory_kind: MemoryKind,
        subject_key: Option<MemorySubjectKey>,
        limit: u32,
    ) -> Result<Self, StoreError> {
        if eligible_scopes.is_empty() || limit == 0 || limit > MAX_MEMORY_SEARCH_CANDIDATES {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self {
            eligible_scopes,
            memory_kind,
            subject_key,
            content_hash: None,
            limit,
        })
    }

    /// Restricts the query to one exact normalized content hash.
    #[must_use]
    pub fn with_content_hash(mut self, content_hash: autoharness_domain::Sha256Digest) -> Self {
        self.content_hash = Some(content_hash);
        self
    }

    /// Returns exact authorized scopes.
    #[must_use]
    pub fn eligible_scopes(&self) -> &[MemoryScope] {
        &self.eligible_scopes
    }

    /// Returns the exact semantic kind.
    #[must_use]
    pub const fn memory_kind(&self) -> MemoryKind {
        self.memory_kind
    }

    /// Returns the exact subject identity, where absence means SQL `NULL`.
    #[must_use]
    pub const fn subject_key(&self) -> Option<&MemorySubjectKey> {
        self.subject_key.as_ref()
    }

    /// Returns the optional exact normalized content hash predicate.
    #[must_use]
    pub const fn content_hash(&self) -> Option<&autoharness_domain::Sha256Digest> {
        self.content_hash.as_ref()
    }

    /// Returns the deterministic row bound.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }
}

/// One active memory head returned by a structured identity query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMemoryHead {
    memory_id: MemoryId,
    scope: MemoryScope,
    memory_kind: MemoryKind,
    revision: MemoryRevision,
}

/// One stable cursor over globally unique memory identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMemoryHeadCursor {
    memory_id: MemoryId,
}

impl ActiveMemoryHeadCursor {
    /// Constructs an exclusive ascending active-head page boundary.
    #[must_use]
    pub const fn new(memory_id: MemoryId) -> Self {
        Self { memory_id }
    }

    /// Returns the globally unique memory identity boundary.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }
}

/// Bounded deterministic page over every active head in authorized scopes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMemoryHeadPageQuery {
    eligible_scopes: Vec<MemoryScope>,
    after: Option<ActiveMemoryHeadCursor>,
    limit: u32,
}

impl ActiveMemoryHeadPageQuery {
    /// Constructs an ascending page that can be exhausted without a fixed-set blind spot.
    pub fn new(
        eligible_scopes: Vec<MemoryScope>,
        after: Option<ActiveMemoryHeadCursor>,
        limit: u32,
    ) -> Result<Self, StoreError> {
        if eligible_scopes.is_empty() || limit == 0 || limit > MAX_MEMORY_SEARCH_CANDIDATES {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self {
            eligible_scopes,
            after,
            limit,
        })
    }

    /// Returns exact authorized scopes.
    #[must_use]
    pub fn eligible_scopes(&self) -> &[MemoryScope] {
        &self.eligible_scopes
    }

    /// Returns the exclusive ascending boundary.
    #[must_use]
    pub const fn after(&self) -> Option<&ActiveMemoryHeadCursor> {
        self.after.as_ref()
    }

    /// Returns the maximum page size.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }
}

impl ActiveMemoryHead {
    /// Constructs one active head read model.
    #[must_use]
    pub const fn new(
        memory_id: MemoryId,
        scope: MemoryScope,
        memory_kind: MemoryKind,
        revision: MemoryRevision,
    ) -> Self {
        Self {
            memory_id,
            scope,
            memory_kind,
            revision,
        }
    }

    /// Returns the durable memory identity.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the exact authorization scope.
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// Returns the semantic memory kind.
    #[must_use]
    pub const fn memory_kind(&self) -> MemoryKind {
        self.memory_kind
    }

    /// Returns active contentless revision metadata, including content and subject hashes.
    #[must_use]
    pub const fn revision(&self) -> &MemoryRevision {
        &self.revision
    }
}

/// One active revision produced by deterministic candidate generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemorySearchCandidate {
    memory_id: MemoryId,
    scope: MemoryScope,
    memory_kind: MemoryKind,
    revision: MemoryRevision,
    content: MemoryContent,
    fts_rank: u32,
}

/// Availability of exact content retained for an immutable memory revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryContentState {
    /// Exact bounded UTF-8 remains available and hash verified.
    Retained(MemoryContent),
    /// Application-owned bytes were intentionally erased.
    Erased,
}

/// Exact revision-by-ID read model for frozen-epoch continuation reconstruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMemoryCandidate {
    memory_id: MemoryId,
    scope: MemoryScope,
    memory_kind: MemoryKind,
    revision: MemoryRevision,
    content: MemoryContentState,
}

impl StoredMemoryCandidate {
    /// Constructs one integrity-checked immutable revision read model.
    #[must_use]
    pub const fn new(
        memory_id: MemoryId,
        scope: MemoryScope,
        memory_kind: MemoryKind,
        revision: MemoryRevision,
        content: MemoryContentState,
    ) -> Self {
        Self {
            memory_id,
            scope,
            memory_kind,
            revision,
            content,
        }
    }

    /// Returns the owning durable memory identity.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the immutable authorization scope recorded on the item.
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// Returns the semantic memory kind.
    #[must_use]
    pub const fn memory_kind(&self) -> MemoryKind {
        self.memory_kind
    }

    /// Returns projected metadata for the exact immutable revision.
    #[must_use]
    pub const fn revision(&self) -> &MemoryRevision {
        &self.revision
    }

    /// Returns retained exact bytes or an explicit erasure state.
    #[must_use]
    pub const fn content(&self) -> &MemoryContentState {
        &self.content
    }
}

impl MemorySearchCandidate {
    /// Constructs a candidate from one validated revision and stable FTS ordinal.
    #[must_use]
    pub const fn new(
        memory_id: MemoryId,
        scope: MemoryScope,
        memory_kind: MemoryKind,
        revision: MemoryRevision,
        content: MemoryContent,
        fts_rank: u32,
    ) -> Self {
        Self {
            memory_id,
            scope,
            memory_kind,
            revision,
            content,
            fts_rank,
        }
    }

    /// Returns the owning durable memory identity.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the authorization scope already checked by the store query.
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// Returns the semantic memory kind.
    #[must_use]
    pub const fn memory_kind(&self) -> MemoryKind {
        self.memory_kind
    }

    /// Returns the optional semantic conflict-grouping key.
    #[must_use]
    pub const fn subject_key(&self) -> Option<&MemorySubjectKey> {
        self.revision.subject_key()
    }

    /// Returns the complete validated revision.
    #[must_use]
    pub const fn revision(&self) -> &MemoryRevision {
        &self.revision
    }

    /// Returns the exact erasable revision content.
    #[must_use]
    pub const fn content(&self) -> &MemoryContent {
        &self.content
    }

    /// Returns the zero-based candidate-source rank.
    #[must_use]
    pub const fn fts_rank(&self) -> u32 {
        self.fts_rank
    }
}

/// Candidate batch tied to one global memory generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCandidateBatch {
    generation: MemoryGeneration,
    candidates: Vec<MemorySearchCandidate>,
}

/// Stable newest-first cursor for memory-workspace inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryInspectionCursor {
    updated_at: TimestampMillis,
    memory_id: MemoryId,
}

impl MemoryInspectionCursor {
    /// Constructs an exclusive newest-first page boundary.
    #[must_use]
    pub const fn new(updated_at: TimestampMillis, memory_id: MemoryId) -> Self {
        Self {
            updated_at,
            memory_id,
        }
    }

    /// Returns the cursor timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMillis {
        self.updated_at
    }

    /// Returns the stable identity tie-breaker.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }
}

/// Authorized filters for one bounded memory-workspace page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryInspectionQuery {
    eligible_scopes: Vec<MemoryScope>,
    statuses: Vec<MemoryRevisionStatus>,
    memory_kind: Option<MemoryKind>,
    subject_key: Option<MemorySubjectKey>,
    literal_search: Option<MemoryContent>,
    sensitivity_ceiling: Sensitivity,
    before: Option<MemoryInspectionCursor>,
    limit: u32,
}

impl MemoryInspectionQuery {
    /// Constructs a bounded inspection query, with an empty status list meaning all states.
    pub fn new(
        eligible_scopes: Vec<MemoryScope>,
        statuses: Vec<MemoryRevisionStatus>,
        before: Option<MemoryInspectionCursor>,
        limit: u32,
    ) -> Result<Self, StoreError> {
        if eligible_scopes.is_empty() || limit == 0 || limit > MAX_MEMORY_INSPECTION_PAGE_SIZE {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self {
            eligible_scopes,
            statuses,
            memory_kind: None,
            subject_key: None,
            literal_search: None,
            sensitivity_ceiling: Sensitivity::Sensitive,
            before,
            limit,
        })
    }

    /// Restricts inspection to one semantic memory kind.
    #[must_use]
    pub fn with_memory_kind(mut self, memory_kind: MemoryKind) -> Self {
        self.memory_kind = Some(memory_kind);
        self
    }

    /// Restricts inspection to one exact semantic subject key.
    #[must_use]
    pub fn with_subject_key(mut self, subject_key: MemorySubjectKey) -> Self {
        self.subject_key = Some(subject_key);
        self
    }

    /// Restricts inspection to a literal retained-content or memory-ID substring.
    ///
    /// The store binds this value as data and never interprets FTS, SQL, or control syntax.
    #[must_use]
    pub fn with_literal_search(mut self, literal_search: MemoryContent) -> Self {
        self.literal_search = Some(literal_search);
        self
    }

    /// Restricts inspection before pagination to authorized sensitivity classes.
    #[must_use]
    pub fn with_sensitivity_ceiling(mut self, sensitivity_ceiling: Sensitivity) -> Self {
        self.sensitivity_ceiling = sensitivity_ceiling;
        self
    }

    /// Returns exact authorized scopes.
    #[must_use]
    pub fn eligible_scopes(&self) -> &[MemoryScope] {
        &self.eligible_scopes
    }

    /// Returns requested lifecycle states, or an empty slice for every state.
    #[must_use]
    pub fn statuses(&self) -> &[MemoryRevisionStatus] {
        &self.statuses
    }

    /// Returns the optional semantic-kind filter.
    #[must_use]
    pub const fn memory_kind(&self) -> Option<MemoryKind> {
        self.memory_kind
    }

    /// Returns the optional exact subject filter.
    #[must_use]
    pub const fn subject_key(&self) -> Option<&MemorySubjectKey> {
        self.subject_key.as_ref()
    }

    /// Returns the optional literal retained-content or durable-ID substring.
    #[must_use]
    pub const fn literal_search(&self) -> Option<&MemoryContent> {
        self.literal_search.as_ref()
    }

    /// Returns the maximum authorized sensitivity applied before the row limit.
    #[must_use]
    pub const fn sensitivity_ceiling(&self) -> Sensitivity {
        self.sensitivity_ceiling
    }

    /// Returns the exclusive newest-first boundary.
    #[must_use]
    pub const fn before(&self) -> Option<&MemoryInspectionCursor> {
        self.before.as_ref()
    }

    /// Returns the maximum page size.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }
}

/// One bounded newest-first Memory workspace page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryInspectionPage {
    records: Vec<MemoryInspectionRecord>,
    has_more: bool,
}

impl MemoryInspectionPage {
    /// Constructs one deterministic inspection page.
    #[must_use]
    pub const fn new(records: Vec<MemoryInspectionRecord>, has_more: bool) -> Self {
        Self { records, has_more }
    }

    /// Returns records in stable newest-first order.
    #[must_use]
    pub fn records(&self) -> &[MemoryInspectionRecord] {
        &self.records
    }

    /// Returns whether at least one additional authorized matching row exists.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    /// Consumes the page and returns its records.
    #[must_use]
    pub fn into_records(self) -> Vec<MemoryInspectionRecord> {
        self.records
    }
}

/// One memory-workspace row covering every durable lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryInspectionRecord {
    memory_id: MemoryId,
    scope: MemoryScope,
    memory_kind: MemoryKind,
    lifecycle: MemoryRevisionStatus,
    latest_revision: MemoryRevision,
    content: Option<MemoryContent>,
    evidence_content: Vec<StoredMemoryEvidenceContent>,
    latest_validation: Option<MemoryValidationResult>,
    active_revision_id: Option<MemoryRevisionId>,
    last_sequence: u64,
    created_at: TimestampMillis,
    updated_at: TimestampMillis,
}

impl MemoryInspectionRecord {
    /// Constructs one integrity-checked workspace row.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        memory_id: MemoryId,
        scope: MemoryScope,
        memory_kind: MemoryKind,
        lifecycle: MemoryRevisionStatus,
        latest_revision: MemoryRevision,
        content: Option<MemoryContent>,
        evidence_content: Vec<StoredMemoryEvidenceContent>,
        latest_validation: Option<MemoryValidationResult>,
        active_revision_id: Option<MemoryRevisionId>,
        last_sequence: u64,
        created_at: TimestampMillis,
        updated_at: TimestampMillis,
    ) -> Self {
        Self {
            memory_id,
            scope,
            memory_kind,
            lifecycle,
            latest_revision,
            content,
            evidence_content,
            latest_validation,
            active_revision_id,
            last_sequence,
            created_at,
            updated_at,
        }
    }

    /// Returns the durable memory identity.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the authorization scope.
    #[must_use]
    pub const fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// Returns the semantic memory kind.
    #[must_use]
    pub const fn memory_kind(&self) -> MemoryKind {
        self.memory_kind
    }

    /// Returns the item-level current lifecycle.
    #[must_use]
    pub const fn lifecycle(&self) -> MemoryRevisionStatus {
        self.lifecycle
    }

    /// Returns latest immutable revision metadata at its projected state.
    #[must_use]
    pub const fn latest_revision(&self) -> &MemoryRevision {
        &self.latest_revision
    }

    /// Returns latest erasable content when it remains available.
    #[must_use]
    pub const fn content(&self) -> Option<&MemoryContent> {
        self.content.as_ref()
    }

    /// Returns exact excerpt availability for the latest revision in metadata order.
    ///
    /// Retained bytes remain redacted from this record's debug representation.
    #[must_use]
    pub fn evidence_content(&self) -> &[StoredMemoryEvidenceContent] {
        &self.evidence_content
    }

    /// Returns the latest durable deterministic validation for the latest revision.
    #[must_use]
    pub const fn latest_validation(&self) -> Option<&MemoryValidationResult> {
        self.latest_validation.as_ref()
    }

    /// Returns the currently eligible revision, when any.
    #[must_use]
    pub const fn active_revision_id(&self) -> Option<&MemoryRevisionId> {
        self.active_revision_id.as_ref()
    }

    /// Returns the last durable item operation sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    /// Returns when the item was created.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMillis {
        self.created_at
    }

    /// Returns when the item was last changed.
    #[must_use]
    pub const fn updated_at(&self) -> TimestampMillis {
        self.updated_at
    }
}

/// Selects admission history for an entire item or one immutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryAdmissionKey {
    /// Every admitted revision belonging to one memory item.
    Memory(MemoryId),
    /// Admissions of one exact immutable revision.
    Revision(MemoryRevisionId),
}

/// Stable newest-first cursor for memory admission history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAdmissionCursor {
    admitted_at: TimestampMillis,
    admission_id: ContextAdmissionId,
}

impl MemoryAdmissionCursor {
    /// Constructs an exclusive history boundary.
    #[must_use]
    pub const fn new(admitted_at: TimestampMillis, admission_id: ContextAdmissionId) -> Self {
        Self {
            admitted_at,
            admission_id,
        }
    }

    /// Returns the admission timestamp boundary.
    #[must_use]
    pub const fn admitted_at(&self) -> TimestampMillis {
        self.admitted_at
    }

    /// Returns the stable identity tie-breaker.
    #[must_use]
    pub const fn admission_id(&self) -> &ContextAdmissionId {
        &self.admission_id
    }
}

/// Bounded newest-first memory admission-history query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAdmissionQuery {
    key: MemoryAdmissionKey,
    before: Option<MemoryAdmissionCursor>,
    limit: u32,
}

impl MemoryAdmissionQuery {
    /// Constructs a bounded admission-history query.
    pub fn new(
        key: MemoryAdmissionKey,
        before: Option<MemoryAdmissionCursor>,
        limit: u32,
    ) -> Result<Self, StoreError> {
        if limit == 0 || limit > MAX_MEMORY_INSPECTION_PAGE_SIZE {
            return Err(StoreError::LimitExceeded);
        }
        Ok(Self { key, before, limit })
    }

    /// Returns the selected memory identity boundary.
    #[must_use]
    pub const fn key(&self) -> &MemoryAdmissionKey {
        &self.key
    }

    /// Returns the exclusive newest-first boundary.
    #[must_use]
    pub const fn before(&self) -> Option<&MemoryAdmissionCursor> {
        self.before.as_ref()
    }

    /// Returns the maximum page size.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }
}

/// One exact durable record of a memory revision entering provider context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryAdmissionRecord {
    admission_id: ContextAdmissionId,
    memory_revision_id: MemoryRevisionId,
    context_turn_id: ContextTurnId,
    epoch_id: ContextEpochId,
    session_id: SessionId,
    attempt_id: AttemptId,
    run_turn: u32,
    model: ModelRef,
    admitted_at: TimestampMillis,
    rank: u32,
    rank_score: i64,
    token_count: EstimatedTokens,
    renderer_version: u16,
    reasons: Vec<ContextAdmissionReason>,
    rendered_content_available: bool,
}

impl MemoryAdmissionRecord {
    /// Constructs one admission-history row.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        admission_id: ContextAdmissionId,
        memory_revision_id: MemoryRevisionId,
        context_turn_id: ContextTurnId,
        epoch_id: ContextEpochId,
        session_id: SessionId,
        attempt_id: AttemptId,
        run_turn: u32,
        model: ModelRef,
        admitted_at: TimestampMillis,
        rank: u32,
        rank_score: i64,
        token_count: EstimatedTokens,
        renderer_version: u16,
        reasons: Vec<ContextAdmissionReason>,
        rendered_content_available: bool,
    ) -> Self {
        Self {
            admission_id,
            memory_revision_id,
            context_turn_id,
            epoch_id,
            session_id,
            attempt_id,
            run_turn,
            model,
            admitted_at,
            rank,
            rank_score,
            token_count,
            renderer_version,
            reasons,
            rendered_content_available,
        }
    }

    /// Returns the durable admission identity.
    #[must_use]
    pub const fn admission_id(&self) -> &ContextAdmissionId {
        &self.admission_id
    }

    /// Returns the exact admitted revision.
    #[must_use]
    pub const fn memory_revision_id(&self) -> &MemoryRevisionId {
        &self.memory_revision_id
    }

    /// Returns the provider-turn context identity.
    #[must_use]
    pub const fn context_turn_id(&self) -> &ContextTurnId {
        &self.context_turn_id
    }

    /// Returns the frozen epoch identity.
    #[must_use]
    pub const fn epoch_id(&self) -> &ContextEpochId {
        &self.epoch_id
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the exact provider attempt.
    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    /// Returns the one-based provider run turn.
    #[must_use]
    pub const fn run_turn(&self) -> u32 {
        self.run_turn
    }

    /// Returns the selected provider-neutral model snapshot.
    #[must_use]
    pub const fn model(&self) -> &ModelRef {
        &self.model
    }

    /// Returns when the admission became durable.
    #[must_use]
    pub const fn admitted_at(&self) -> TimestampMillis {
        self.admitted_at
    }

    /// Returns the one-based deterministic rank.
    #[must_use]
    pub const fn rank(&self) -> u32 {
        self.rank
    }

    /// Returns the final integer rank score.
    #[must_use]
    pub const fn rank_score(&self) -> i64 {
        self.rank_score
    }

    /// Returns the rendered token estimate.
    #[must_use]
    pub const fn token_count(&self) -> EstimatedTokens {
        self.token_count
    }

    /// Returns the renderer contract version.
    #[must_use]
    pub const fn renderer_version(&self) -> u16 {
        self.renderer_version
    }

    /// Returns stable deterministic score factors.
    #[must_use]
    pub fn reasons(&self) -> &[ContextAdmissionReason] {
        &self.reasons
    }

    /// Returns whether exact rendered bytes have not been erased.
    #[must_use]
    pub const fn rendered_content_available(&self) -> bool {
        self.rendered_content_available
    }
}

impl MemoryCandidateBatch {
    /// Constructs a candidate batch.
    #[must_use]
    pub const fn new(generation: MemoryGeneration, candidates: Vec<MemorySearchCandidate>) -> Self {
        Self {
            generation,
            candidates,
        }
    }

    /// Returns the generation sampled with the candidates.
    #[must_use]
    pub const fn generation(&self) -> MemoryGeneration {
        self.generation
    }

    /// Returns candidates in stable candidate-source order.
    #[must_use]
    pub fn candidates(&self) -> &[MemorySearchCandidate] {
        &self.candidates
    }
}

/// Durable memory-ledger writes and read models.
pub trait MemoryStore {
    /// Atomically appends one lifecycle operation and updates projections and FTS.
    fn append_memory(
        &mut self,
        request: &MemoryAppendRequest,
    ) -> Result<MemoryAppendReceipt, StoreError>;

    /// Atomically appends one contiguous logical-command operation batch.
    fn append_memory_batch(
        &mut self,
        request: &MemoryAppendBatchRequest,
    ) -> Result<MemoryAppendReceipt, StoreError>;

    /// Loads at most `limit` operations after an exclusive item sequence.
    fn load_memory_operations(
        &self,
        memory_id: &MemoryId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<MemoryOperationEnvelope>, StoreError>;

    /// Loads all retained revisions for one memory item in revision order.
    fn load_memory_revisions(
        &self,
        memory_id: &MemoryId,
    ) -> Result<Vec<MemoryRevision>, StoreError>;

    /// Loads erasable content for one retained revision, when present.
    fn load_memory_content(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<MemoryContent>, StoreError>;

    /// Loads exact evidence excerpt availability for one immutable revision.
    ///
    /// The read is bounded by the domain evidence limit and never follows evidence source
    /// references into content owned by another scope.
    fn load_memory_evidence_content(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<Vec<StoredMemoryEvidenceContent>>, StoreError>;

    /// Loads one exact revision and owning identity for frozen context reconstruction.
    fn load_memory_candidate(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<StoredMemoryCandidate>, StoreError>;

    /// Loads an authorized Memory workspace page and an exact continuation indicator.
    fn inspect_memory_page(
        &self,
        query: &MemoryInspectionQuery,
    ) -> Result<MemoryInspectionPage, StoreError>;

    /// Lists authorized memory items across every lifecycle with stable pagination.
    ///
    /// Callers that need a continuation indicator should use [`Self::inspect_memory_page`].
    fn inspect_memories(
        &self,
        query: &MemoryInspectionQuery,
    ) -> Result<Vec<MemoryInspectionRecord>, StoreError> {
        self.inspect_memory_page(query)
            .map(MemoryInspectionPage::into_records)
    }

    /// Loads bounded newest-first provider-context admission history.
    fn load_memory_admissions(
        &self,
        query: &MemoryAdmissionQuery,
    ) -> Result<Vec<MemoryAdmissionRecord>, StoreError>;

    /// Loads deterministic active heads by exact authorization scope and semantic identity.
    fn load_active_memory_heads(
        &self,
        query: &ActiveMemoryHeadQuery,
    ) -> Result<Vec<ActiveMemoryHead>, StoreError>;

    /// Pages every authorized active head by globally unique memory identity.
    fn page_active_memory_heads(
        &self,
        query: &ActiveMemoryHeadPageQuery,
    ) -> Result<Vec<ActiveMemoryHead>, StoreError>;

    /// Retrieves active FTS candidates and the exact sampled generation.
    fn search_memory(&self, query: &MemorySearchQuery) -> Result<MemoryCandidateBatch, StoreError>;

    /// Returns the current global eligibility generation.
    fn memory_generation(&self) -> Result<MemoryGeneration, StoreError>;

    /// Returns the monotonic generation of every committed logical memory mutation.
    fn memory_mutation_generation(&self) -> Result<MemoryMutationGeneration, StoreError>;

    /// Rebuilds lifecycle projections and FTS from the authoritative ledger.
    fn rebuild_memory_projections(&mut self) -> Result<(), StoreError>;
}
