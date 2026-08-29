use std::fmt::{self, Debug, Formatter};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    AgentId, CommandId, ContextSourceKey, CorrelationId, EventId, InputId, MemoryEvidenceId,
    MemoryId, MemoryOperationId, MemoryRevisionId, MemorySubjectKey, SessionId, TimestampMillis,
    ToolCallId, UserId, ValueError, WorkspaceId,
};

/// The only memory-ledger schema emitted by the Phase 4 domain contract.
pub const MEMORY_SCHEMA_V1: u16 = 1;

/// Maximum evidence records attached to one immutable revision.
pub const MAX_MEMORY_EVIDENCE: usize = 64;

/// Maximum inter-memory relations attached to one immutable revision.
pub const MAX_MEMORY_RELATIONS: usize = 64;

/// Maximum stable validation issues retained for one validation pass.
pub const MAX_MEMORY_VALIDATION_ISSUES: usize = 32;

/// Exact bounded memory content that remains redacted from debug output.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MemoryContent(String);

impl MemoryContent {
    /// Maximum persisted byte length for one memory revision.
    pub const MAX_BYTES: usize = 16_384;

    /// Validates non-empty bounded content while preserving its original bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ValueError::EmptyMemoryContent);
        }
        if value.len() > Self::MAX_BYTES {
            return Err(ValueError::MemoryContentTooLong);
        }

        Ok(Self(value))
    }

    /// Returns the exact memory content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for MemoryContent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryContent")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for MemoryContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// An optional bounded evidence excerpt that remains redacted from debug output.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MemoryEvidenceExcerpt(String);

impl MemoryEvidenceExcerpt {
    /// Maximum persisted byte length for one evidence excerpt.
    pub const MAX_BYTES: usize = 4_096;

    /// Validates non-empty bounded content while preserving its original bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ValueError::EmptyMemoryEvidenceExcerpt);
        }
        if value.len() > Self::MAX_BYTES {
            return Err(ValueError::MemoryEvidenceExcerptTooLong);
        }

        Ok(Self(value))
    }

    /// Returns the exact evidence excerpt.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for MemoryEvidenceExcerpt {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryEvidenceExcerpt")
            .field("content", &"[REDACTED]")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for MemoryEvidenceExcerpt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A canonical lowercase SHA-256 digest without a textual algorithm prefix.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// Validates exactly 64 lowercase hexadecimal ASCII characters.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ValueError::InvalidSha256Digest);
        }

        Ok(Self(value))
    }

    /// Returns the canonical lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Confidence represented without floating-point ambiguity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConfidenceBasisPoints(u16);

impl ConfidenceBasisPoints {
    /// The highest representable confidence value.
    pub const MAX: u16 = 10_000;

    /// Validates a confidence value in the inclusive range from zero to 10,000.
    pub const fn new(value: u16) -> Result<Self, ValueError> {
        if value > Self::MAX {
            return Err(ValueError::ConfidenceOutOfRange);
        }

        Ok(Self(value))
    }

    /// Returns the confidence value in basis points.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ConfidenceBasisPoints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A one-based sequence within one memory item's operation stream.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MemorySequence(u64);

impl MemorySequence {
    /// The first valid sequence in a memory operation stream.
    pub const FIRST: Self = Self(1);

    /// Validates and constructs a memory operation sequence.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::ZeroMemorySequence);
        }
        if value > i64::MAX as u64 {
            return Err(ValueError::MemorySequenceTooLarge);
        }

        Ok(Self(value))
    }

    /// Returns the one-based sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next sequence, or `None` at durable integer exhaustion.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) if value <= i64::MAX as u64 => Some(Self(value)),
            Some(_) | None => None,
        }
    }
}

impl<'de> Deserialize<'de> for MemorySequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A one-based ordinal for an immutable revision of one memory item.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MemoryRevisionNumber(u64);

impl MemoryRevisionNumber {
    /// The first valid memory revision number.
    pub const FIRST: Self = Self(1);

    /// Validates and constructs a memory revision number.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::ZeroMemoryRevision);
        }
        if value > i64::MAX as u64 {
            return Err(ValueError::MemoryRevisionTooLarge);
        }

        Ok(Self(value))
    }

    /// Returns the one-based revision number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision, or `None` at durable integer exhaustion.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) if value <= i64::MAX as u64 => Some(Self(value)),
            Some(_) | None => None,
        }
    }
}

impl<'de> Deserialize<'de> for MemoryRevisionNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// The durable authorization boundary that owns a memory item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum MemoryScope {
    /// Memory available only to the identified user.
    User(UserId),
    /// Memory available only inside the identified workspace.
    Workspace(WorkspaceId),
    /// Memory available only inside the identified session.
    Session(SessionId),
    /// Memory available only to the identified configured agent.
    Agent(AgentId),
}

/// The semantic class of durable knowledge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// A claim about the world or project state.
    Fact,
    /// A user-approved choice among otherwise valid alternatives.
    Preference,
    /// A requirement that constrains future behavior.
    Constraint,
    /// A derived takeaway whose provenance remains explicit.
    Lesson,
    /// A reusable sequence of actions.
    Procedure,
}

/// The source class that authored a revision proposal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOrigin {
    /// The user explicitly requested durable retention.
    ExplicitUser,
    /// A permissioned tool produced structured evidence.
    VerifiedTool,
    /// An authorized external source was imported.
    ImportedDocument,
    /// A model proposed the revision without promotion authority.
    ModelProposal,
    /// A compaction process proposed a condensed representation.
    Compaction,
}

/// The authority class assigned by trusted application policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustClass {
    /// A user explicitly approved the exact immutable revision.
    UserApproved,
    /// Trusted policy verified structured observation evidence.
    VerifiedObservation,
    /// Content came from an authorized but independently mutable source.
    Imported,
    /// Content is only an untrusted proposal and is never context-eligible.
    UntrustedProposal,
}

/// The handling class applied before storage, retrieval, and export.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Content may be presented and exported under ordinary policy.
    Public,
    /// Content is limited to ordinary local application use.
    Internal,
    /// Content requires an explicit sensitivity-compatible context.
    Sensitive,
    /// Content is secret-bearing and must not enter ordinary memory storage.
    Secret,
}

/// The immutable lifecycle state recorded for one memory revision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRevisionStatus {
    /// The revision is retained for review but is not context-eligible.
    Proposed,
    /// The revision may be considered by authorized retrieval.
    Active,
    /// A later immutable revision replaced this one.
    Superseded,
    /// Validation or an authority decision rejected the revision.
    Rejected,
    /// Authority withdrew a previously active revision.
    Retracted,
    /// Application-owned content was deleted and only a tombstone remains.
    Deleted,
}

/// A validity window with both inclusive start and exclusive end timestamps.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryValidityWindow {
    valid_from: TimestampMillis,
    valid_until: TimestampMillis,
}

#[derive(Deserialize)]
struct RawMemoryValidityWindow {
    valid_from: TimestampMillis,
    valid_until: TimestampMillis,
}

impl<'de> Deserialize<'de> for MemoryValidityWindow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryValidityWindow::deserialize(deserializer)?;
        Self::new(raw.valid_from, raw.valid_until).map_err(D::Error::custom)
    }
}

impl MemoryValidityWindow {
    /// Constructs a non-empty ordered validity window.
    pub const fn new(
        valid_from: TimestampMillis,
        valid_until: TimestampMillis,
    ) -> Result<Self, ValueError> {
        if valid_until.get() <= valid_from.get() {
            return Err(ValueError::InvalidMemoryValidity);
        }

        Ok(Self {
            valid_from,
            valid_until,
        })
    }

    /// Returns the inclusive start of the validity window.
    #[must_use]
    pub const fn valid_from(self) -> TimestampMillis {
        self.valid_from
    }

    /// Returns the exclusive end of the validity window.
    #[must_use]
    pub const fn valid_until(self) -> TimestampMillis {
        self.valid_until
    }
}

/// The time interval during which a revision may be admitted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum MemoryValidity {
    /// The revision has no time-derived eligibility boundary.
    Indefinite,
    /// The revision is not eligible before the inclusive timestamp.
    From {
        /// Inclusive eligibility start.
        valid_from: TimestampMillis,
    },
    /// The revision is not eligible at or after the exclusive timestamp.
    Until {
        /// Exclusive eligibility end.
        valid_until: TimestampMillis,
    },
    /// The revision is eligible only inside an ordered bounded window.
    Window(MemoryValidityWindow),
}

/// A typed, stable reference to evidence without provider-native payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum MemoryEvidenceSource {
    /// An explicitly admitted user input supports the claim.
    UserInput {
        /// Session that owns the input.
        session_id: SessionId,
        /// Stable admitted input identity.
        input_id: InputId,
    },
    /// A permissioned tool result supports the claim.
    ToolObservation {
        /// Session that owns the tool call.
        session_id: SessionId,
        /// Durable tool-call identity.
        tool_call_id: ToolCallId,
        /// Hash of the exact bounded or artifact-backed result.
        output_hash: Sha256Digest,
    },
    /// An authorized imported context source supports the claim.
    ImportedDocument {
        /// Stable source registry key.
        source_key: ContextSourceKey,
        /// Hash of the exact imported source revision.
        source_revision: Sha256Digest,
    },
    /// A durable session event supports the claim.
    SessionEvent {
        /// Session that owns the event.
        session_id: SessionId,
        /// Stable event identity.
        event_id: EventId,
    },
    /// Another immutable memory revision supports the claim.
    MemoryRevision {
        /// Related memory item.
        memory_id: MemoryId,
        /// Exact related revision.
        revision_id: MemoryRevisionId,
    },
}

/// The evidentiary relationship to the revision claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEvidenceRelation {
    /// The evidence directly supports the claim.
    Supports,
    /// The evidence contradicts the claim and requires review.
    Contradicts,
    /// The revision was derived from the evidence without copying its authority.
    DerivedFrom,
}

/// Bounded evidence attached to one immutable memory revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryEvidence {
    evidence_id: MemoryEvidenceId,
    source: MemoryEvidenceSource,
    relation: MemoryEvidenceRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    excerpt: Option<MemoryEvidenceExcerpt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    excerpt_hash: Option<Sha256Digest>,
}

#[derive(Deserialize)]
struct RawMemoryEvidence {
    evidence_id: MemoryEvidenceId,
    source: MemoryEvidenceSource,
    relation: MemoryEvidenceRelation,
    excerpt: Option<MemoryEvidenceExcerpt>,
    excerpt_hash: Option<Sha256Digest>,
}

impl<'de> Deserialize<'de> for MemoryEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryEvidence::deserialize(deserializer)?;
        Self::new(
            raw.evidence_id,
            raw.source,
            raw.relation,
            raw.excerpt,
            raw.excerpt_hash,
        )
        .map_err(D::Error::custom)
    }
}

impl MemoryEvidence {
    /// Constructs evidence whose erasable excerpt and hash are either both present or both absent.
    pub fn new(
        evidence_id: MemoryEvidenceId,
        source: MemoryEvidenceSource,
        relation: MemoryEvidenceRelation,
        excerpt: Option<MemoryEvidenceExcerpt>,
        excerpt_hash: Option<Sha256Digest>,
    ) -> Result<Self, ValueError> {
        if excerpt.is_some() != excerpt_hash.is_some() {
            return Err(ValueError::InvalidMemoryEvidence);
        }

        Ok(Self {
            evidence_id,
            source,
            relation,
            excerpt,
            excerpt_hash,
        })
    }

    /// Returns the evidence identity.
    #[must_use]
    pub const fn evidence_id(&self) -> &MemoryEvidenceId {
        &self.evidence_id
    }

    /// Returns the typed evidence source.
    #[must_use]
    pub const fn source(&self) -> &MemoryEvidenceSource {
        &self.source
    }

    /// Returns the relationship to the revision claim.
    #[must_use]
    pub const fn relation(&self) -> MemoryEvidenceRelation {
        self.relation
    }

    /// Returns the optional exact evidence excerpt.
    #[must_use]
    pub const fn excerpt(&self) -> Option<&MemoryEvidenceExcerpt> {
        self.excerpt.as_ref()
    }

    /// Returns the optional evidence excerpt digest.
    #[must_use]
    pub const fn excerpt_hash(&self) -> Option<&Sha256Digest> {
        self.excerpt_hash.as_ref()
    }
}

/// Contentless evidence metadata safe to retain after excerpt deletion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryEvidenceMetadata {
    evidence_id: MemoryEvidenceId,
    source: MemoryEvidenceSource,
    relation: MemoryEvidenceRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    excerpt_hash: Option<Sha256Digest>,
}

impl MemoryEvidenceMetadata {
    /// Derives replayable metadata without copying an erasable excerpt.
    #[must_use]
    pub fn from_evidence(evidence: &MemoryEvidence) -> Self {
        Self {
            evidence_id: evidence.evidence_id.clone(),
            source: evidence.source.clone(),
            relation: evidence.relation,
            excerpt_hash: evidence.excerpt_hash.clone(),
        }
    }

    /// Returns the evidence identity used to key an optional excerpt sidecar.
    #[must_use]
    pub const fn evidence_id(&self) -> &MemoryEvidenceId {
        &self.evidence_id
    }

    /// Returns the typed evidence source.
    #[must_use]
    pub const fn source(&self) -> &MemoryEvidenceSource {
        &self.source
    }

    /// Returns the relationship to the revision claim.
    #[must_use]
    pub const fn relation(&self) -> MemoryEvidenceRelation {
        self.relation
    }

    /// Returns the expected excerpt digest when an erasable sidecar exists.
    #[must_use]
    pub const fn excerpt_hash(&self) -> Option<&Sha256Digest> {
        self.excerpt_hash.as_ref()
    }
}

/// The semantic relationship between two memory items.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationKind {
    /// Both items express the same durable claim.
    DuplicateOf,
    /// The items cannot both be true under the same validity conditions.
    Contradicts,
    /// This item adds specificity without replacing the related item.
    Refines,
    /// This item intentionally replaces the related item.
    Supersedes,
    /// The items are associated without a stronger semantic claim.
    Related,
}

/// A typed relation from the owning revision to another memory item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRelation {
    memory_id: MemoryId,
    kind: MemoryRelationKind,
}

impl MemoryRelation {
    /// Constructs a relation to another memory item.
    #[must_use]
    pub const fn new(memory_id: MemoryId, kind: MemoryRelationKind) -> Self {
        Self { memory_id, kind }
    }

    /// Returns the related memory identity.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the semantic relation kind.
    #[must_use]
    pub const fn kind(&self) -> MemoryRelationKind {
        self.kind
    }
}

/// The stable result category of deterministic revision validation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryValidationStatus {
    /// Deterministic validation found no blocking issue.
    Accepted,
    /// Policy requires an independent authority decision.
    NeedsReview,
    /// Deterministic validation found a blocking issue.
    Rejected,
}

/// A stable machine-readable validation issue.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryValidationIssue {
    /// Secret-bearing content was detected.
    SecretDetected,
    /// The requested scope is not authorized.
    UnsupportedScope,
    /// Content failed its type-specific structural contract.
    MalformedContent,
    /// The revision conflicts with non-overridable policy.
    PolicyConflict,
    /// An exact duplicate already exists.
    Duplicate,
    /// A likely contradictory memory requires review.
    Contradiction,
    /// Content contains a likely instruction-injection pattern.
    InjectionPattern,
    /// Evidence does not ground the proposed claim.
    UngroundedEvidence,
}

/// A versioned deterministic validation result for exact revision content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryValidationResult {
    validator_version: u16,
    content_hash: Sha256Digest,
    status: MemoryValidationStatus,
    issues: Vec<MemoryValidationIssue>,
}

#[derive(Deserialize)]
struct RawMemoryValidationResult {
    validator_version: u16,
    content_hash: Sha256Digest,
    status: MemoryValidationStatus,
    issues: Vec<MemoryValidationIssue>,
}

impl<'de> Deserialize<'de> for MemoryValidationResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryValidationResult::deserialize(deserializer)?;
        Self::new(
            raw.validator_version,
            raw.content_hash,
            raw.status,
            raw.issues,
        )
        .map_err(D::Error::custom)
    }
}

impl MemoryValidationResult {
    /// Constructs a bounded validation result.
    pub fn new(
        validator_version: u16,
        content_hash: Sha256Digest,
        status: MemoryValidationStatus,
        issues: Vec<MemoryValidationIssue>,
    ) -> Result<Self, ValueError> {
        if validator_version == 0 {
            return Err(ValueError::ZeroVersion);
        }
        if issues.len() > MAX_MEMORY_VALIDATION_ISSUES {
            return Err(ValueError::CollectionTooLarge);
        }

        Ok(Self {
            validator_version,
            content_hash,
            status,
            issues,
        })
    }

    /// Returns the deterministic validator contract version.
    #[must_use]
    pub const fn validator_version(&self) -> u16 {
        self.validator_version
    }

    /// Returns the exact validated content digest.
    #[must_use]
    pub const fn content_hash(&self) -> &Sha256Digest {
        &self.content_hash
    }

    /// Returns the validation result category.
    #[must_use]
    pub const fn status(&self) -> MemoryValidationStatus {
        self.status
    }

    /// Returns the stable validation issues in deterministic order.
    #[must_use]
    pub fn issues(&self) -> &[MemoryValidationIssue] {
        &self.issues
    }
}

/// Immutable candidate data from which a memory revision may be committed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryRevisionDraft {
    revision_id: MemoryRevisionId,
    revision: MemoryRevisionNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject_key: Option<MemorySubjectKey>,
    content: MemoryContent,
    content_hash: Sha256Digest,
    origin: MemoryOrigin,
    trust_class: TrustClass,
    confidence: ConfidenceBasisPoints,
    sensitivity: Sensitivity,
    validity: MemoryValidity,
    evidence: Vec<MemoryEvidence>,
    relations: Vec<MemoryRelation>,
}

#[derive(Deserialize)]
struct RawMemoryRevisionDraft {
    revision_id: MemoryRevisionId,
    revision: MemoryRevisionNumber,
    #[serde(default)]
    subject_key: Option<MemorySubjectKey>,
    content: MemoryContent,
    content_hash: Sha256Digest,
    origin: MemoryOrigin,
    trust_class: TrustClass,
    confidence: ConfidenceBasisPoints,
    sensitivity: Sensitivity,
    validity: MemoryValidity,
    evidence: Vec<MemoryEvidence>,
    relations: Vec<MemoryRelation>,
}

impl<'de> Deserialize<'de> for MemoryRevisionDraft {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryRevisionDraft::deserialize(deserializer)?;
        Self::new(
            raw.revision_id,
            raw.revision,
            raw.subject_key,
            raw.content,
            raw.content_hash,
            raw.origin,
            raw.trust_class,
            raw.confidence,
            raw.sensitivity,
            raw.validity,
            raw.evidence,
            raw.relations,
        )
        .map_err(D::Error::custom)
    }
}

impl MemoryRevisionDraft {
    /// Constructs a bounded immutable revision candidate.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision_id: MemoryRevisionId,
        revision: MemoryRevisionNumber,
        subject_key: Option<MemorySubjectKey>,
        content: MemoryContent,
        content_hash: Sha256Digest,
        origin: MemoryOrigin,
        trust_class: TrustClass,
        confidence: ConfidenceBasisPoints,
        sensitivity: Sensitivity,
        validity: MemoryValidity,
        evidence: Vec<MemoryEvidence>,
        relations: Vec<MemoryRelation>,
    ) -> Result<Self, ValueError> {
        if evidence.len() > MAX_MEMORY_EVIDENCE || relations.len() > MAX_MEMORY_RELATIONS {
            return Err(ValueError::CollectionTooLarge);
        }

        Ok(Self {
            revision_id,
            revision,
            subject_key,
            content,
            content_hash,
            origin,
            trust_class,
            confidence,
            sensitivity,
            validity,
            evidence,
            relations,
        })
    }

    /// Returns the immutable revision identity.
    #[must_use]
    pub const fn revision_id(&self) -> &MemoryRevisionId {
        &self.revision_id
    }

    /// Returns the one-based revision number.
    #[must_use]
    pub const fn revision(&self) -> MemoryRevisionNumber {
        self.revision
    }

    /// Returns the optional semantic identity used for deterministic conflict grouping.
    #[must_use]
    pub const fn subject_key(&self) -> Option<&MemorySubjectKey> {
        self.subject_key.as_ref()
    }

    /// Returns the exact bounded memory content.
    #[must_use]
    pub const fn content(&self) -> &MemoryContent {
        &self.content
    }

    /// Returns the exact content digest.
    #[must_use]
    pub const fn content_hash(&self) -> &Sha256Digest {
        &self.content_hash
    }

    /// Returns the immutable proposal origin.
    #[must_use]
    pub const fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    /// Returns the trust class assigned by trusted policy.
    #[must_use]
    pub const fn trust_class(&self) -> TrustClass {
        self.trust_class
    }

    /// Returns confidence in basis points.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceBasisPoints {
        self.confidence
    }

    /// Returns the sensitivity handling class.
    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// Returns the revision validity interval.
    #[must_use]
    pub const fn validity(&self) -> MemoryValidity {
        self.validity
    }

    /// Returns evidence in stable caller-supplied order.
    #[must_use]
    pub fn evidence(&self) -> &[MemoryEvidence] {
        &self.evidence
    }

    /// Returns relations in stable caller-supplied order.
    #[must_use]
    pub fn relations(&self) -> &[MemoryRelation] {
        &self.relations
    }
}

/// Contentless revision metadata safe to retain after content deletion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryRevisionMetadata {
    status: MemoryRevisionStatus,
    revision_id: MemoryRevisionId,
    revision: MemoryRevisionNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subject_key: Option<MemorySubjectKey>,
    content_hash: Sha256Digest,
    origin: MemoryOrigin,
    trust_class: TrustClass,
    confidence: ConfidenceBasisPoints,
    sensitivity: Sensitivity,
    validity: MemoryValidity,
    evidence: Vec<MemoryEvidenceMetadata>,
    relations: Vec<MemoryRelation>,
    created_at: TimestampMillis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supersedes_revision_id: Option<MemoryRevisionId>,
}

impl MemoryRevisionMetadata {
    /// Constructs bounded contentless revision metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: MemoryRevisionStatus,
        revision_id: MemoryRevisionId,
        revision: MemoryRevisionNumber,
        subject_key: Option<MemorySubjectKey>,
        content_hash: Sha256Digest,
        origin: MemoryOrigin,
        trust_class: TrustClass,
        confidence: ConfidenceBasisPoints,
        sensitivity: Sensitivity,
        validity: MemoryValidity,
        evidence: Vec<MemoryEvidenceMetadata>,
        relations: Vec<MemoryRelation>,
        created_at: TimestampMillis,
        supersedes_revision_id: Option<MemoryRevisionId>,
    ) -> Result<Self, ValueError> {
        if evidence.len() > MAX_MEMORY_EVIDENCE || relations.len() > MAX_MEMORY_RELATIONS {
            return Err(ValueError::CollectionTooLarge);
        }

        Ok(Self {
            status,
            revision_id,
            revision,
            subject_key,
            content_hash,
            origin,
            trust_class,
            confidence,
            sensitivity,
            validity,
            evidence,
            relations,
            created_at,
            supersedes_revision_id,
        })
    }

    /// Derives replayable metadata without copying erasable content or excerpts.
    #[must_use]
    pub fn from_draft(
        status: MemoryRevisionStatus,
        draft: &MemoryRevisionDraft,
        created_at: TimestampMillis,
        supersedes_revision_id: Option<MemoryRevisionId>,
    ) -> Self {
        Self {
            status,
            revision_id: draft.revision_id.clone(),
            revision: draft.revision,
            subject_key: draft.subject_key.clone(),
            content_hash: draft.content_hash.clone(),
            origin: draft.origin,
            trust_class: draft.trust_class,
            confidence: draft.confidence,
            sensitivity: draft.sensitivity,
            validity: draft.validity,
            evidence: draft
                .evidence
                .iter()
                .map(MemoryEvidenceMetadata::from_evidence)
                .collect(),
            relations: draft.relations.clone(),
            created_at,
            supersedes_revision_id,
        }
    }

    /// Returns new metadata at a projected lifecycle state without mutating the prior value.
    #[must_use]
    pub fn with_status(mut self, status: MemoryRevisionStatus) -> Self {
        self.status = status;
        self
    }

    /// Returns the immutable lifecycle state.
    #[must_use]
    pub const fn status(&self) -> MemoryRevisionStatus {
        self.status
    }

    /// Returns the immutable revision identity.
    #[must_use]
    pub const fn revision_id(&self) -> &MemoryRevisionId {
        &self.revision_id
    }

    /// Returns the one-based revision number.
    #[must_use]
    pub const fn revision(&self) -> MemoryRevisionNumber {
        self.revision
    }

    /// Returns the optional semantic identity retained after content erasure.
    #[must_use]
    pub const fn subject_key(&self) -> Option<&MemorySubjectKey> {
        self.subject_key.as_ref()
    }

    /// Returns the digest used to verify an erasable content sidecar.
    #[must_use]
    pub const fn content_hash(&self) -> &Sha256Digest {
        &self.content_hash
    }

    /// Returns the immutable proposal origin.
    #[must_use]
    pub const fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    /// Returns the trust class assigned by trusted policy.
    #[must_use]
    pub const fn trust_class(&self) -> TrustClass {
        self.trust_class
    }

    /// Returns confidence in basis points.
    #[must_use]
    pub const fn confidence(&self) -> ConfidenceBasisPoints {
        self.confidence
    }

    /// Returns the sensitivity handling class.
    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    /// Returns the revision validity interval.
    #[must_use]
    pub const fn validity(&self) -> MemoryValidity {
        self.validity
    }

    /// Returns contentless evidence metadata in stable order.
    #[must_use]
    pub fn evidence(&self) -> &[MemoryEvidenceMetadata] {
        &self.evidence
    }

    /// Returns inter-memory relations in stable order.
    #[must_use]
    pub fn relations(&self) -> &[MemoryRelation] {
        &self.relations
    }

    /// Returns when the revision was committed.
    #[must_use]
    pub const fn created_at(&self) -> TimestampMillis {
        self.created_at
    }

    /// Returns the prior revision replaced by this one, when any.
    #[must_use]
    pub const fn supersedes_revision_id(&self) -> Option<&MemoryRevisionId> {
        self.supersedes_revision_id.as_ref()
    }
}

#[derive(Deserialize)]
struct RawMemoryRevisionMetadata {
    status: MemoryRevisionStatus,
    revision_id: MemoryRevisionId,
    revision: MemoryRevisionNumber,
    #[serde(default)]
    subject_key: Option<MemorySubjectKey>,
    content_hash: Sha256Digest,
    origin: MemoryOrigin,
    trust_class: TrustClass,
    confidence: ConfidenceBasisPoints,
    sensitivity: Sensitivity,
    validity: MemoryValidity,
    evidence: Vec<MemoryEvidenceMetadata>,
    relations: Vec<MemoryRelation>,
    created_at: TimestampMillis,
    supersedes_revision_id: Option<MemoryRevisionId>,
}

impl<'de> Deserialize<'de> for MemoryRevisionMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryRevisionMetadata::deserialize(deserializer)?;
        Self::new(
            raw.status,
            raw.revision_id,
            raw.revision,
            raw.subject_key,
            raw.content_hash,
            raw.origin,
            raw.trust_class,
            raw.confidence,
            raw.sensitivity,
            raw.validity,
            raw.evidence,
            raw.relations,
            raw.created_at,
            raw.supersedes_revision_id,
        )
        .map_err(D::Error::custom)
    }
}

/// Backward-friendly short name for contentless durable revision metadata.
pub type MemoryRevision = MemoryRevisionMetadata;

/// A stable policy reason for rejecting a proposed revision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRejectionReason {
    /// Deterministic validation rejected the revision.
    ValidationFailed,
    /// The user or another authority declined promotion.
    AuthorityDeclined,
    /// An exact duplicate made the proposal unnecessary.
    Duplicate,
    /// The proposal conflicts with current durable knowledge.
    Conflict,
}

/// Requested memory intent whose acceptance is decided by a trusted aggregate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum MemoryCommandPayload {
    /// Create a new memory item and its first immutable revision.
    CreateMemory {
        /// Authorization scope for retrieval.
        scope: MemoryScope,
        /// Semantic memory kind.
        memory_kind: MemoryKind,
        /// Candidate first revision.
        revision: MemoryRevisionDraft,
    },
    /// Retain an untrusted candidate revision for review.
    ProposeRevision {
        /// Proposed immutable revision.
        revision: MemoryRevisionDraft,
        /// Current revision that this candidate proposes to replace.
        supersedes_revision_id: MemoryRevisionId,
    },
    /// Commit an explicit trusted revision without mutating its predecessor.
    ReviseMemory {
        /// New immutable revision.
        revision: MemoryRevisionDraft,
        /// Current revision replaced by the new revision.
        supersedes_revision_id: MemoryRevisionId,
    },
    /// Record deterministic validation for exact revision bytes.
    RecordValidation {
        /// Revision that was validated.
        revision_id: MemoryRevisionId,
        /// Versioned validation result.
        validation: MemoryValidationResult,
    },
    /// Approve a proposal by creating a distinct user-approved revision.
    ApproveProposal {
        /// Untrusted proposal being approved.
        proposal_revision_id: MemoryRevisionId,
        /// New immutable approved revision.
        approved_revision: MemoryRevisionDraft,
    },
    /// Activate an already validated revision under trusted policy.
    ActivateRevision {
        /// Revision becoming context-eligible.
        revision_id: MemoryRevisionId,
    },
    /// Reject a proposal without deleting its audit envelope.
    RejectRevision {
        /// Revision being rejected.
        revision_id: MemoryRevisionId,
        /// Stable rejection reason.
        reason: MemoryRejectionReason,
    },
    /// Withdraw the current revision from future context admission.
    RetractMemory {
        /// Current revision being retracted.
        revision_id: MemoryRevisionId,
    },
    /// Delete application-owned content while retaining a minimal tombstone.
    DeleteMemory {
        /// Current revision being deleted.
        revision_id: MemoryRevisionId,
    },
}

impl MemoryCommandPayload {
    const fn creates_item(&self) -> bool {
        matches!(self, Self::CreateMemory { .. })
    }
}

/// A schema-versioned command routed to one memory item's logical writer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MemoryCommandEnvelope {
    schema_version: u16,
    command_id: CommandId,
    memory_id: MemoryId,
    expected_sequence: Option<MemorySequence>,
    correlation_id: CorrelationId,
    payload: MemoryCommandPayload,
}

#[derive(Deserialize)]
struct RawMemoryCommandEnvelope {
    schema_version: u16,
    command_id: CommandId,
    memory_id: MemoryId,
    expected_sequence: Option<MemorySequence>,
    correlation_id: CorrelationId,
    payload: MemoryCommandPayload,
}

impl<'de> Deserialize<'de> for MemoryCommandEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawMemoryCommandEnvelope::deserialize(deserializer)?;
        if raw.payload.creates_item() != raw.expected_sequence.is_none() {
            return Err(D::Error::custom(ValueError::InvalidMemoryExpectedSequence));
        }

        Ok(Self {
            schema_version: raw.schema_version,
            command_id: raw.command_id,
            memory_id: raw.memory_id,
            expected_sequence: raw.expected_sequence,
            correlation_id: raw.correlation_id,
            payload: raw.payload,
        })
    }
}

impl MemoryCommandEnvelope {
    /// Constructs a v1 command and enforces create-versus-update sequence semantics.
    pub fn new_v1(
        command_id: CommandId,
        memory_id: MemoryId,
        expected_sequence: Option<MemorySequence>,
        correlation_id: CorrelationId,
        payload: MemoryCommandPayload,
    ) -> Result<Self, ValueError> {
        if payload.creates_item() != expected_sequence.is_none() {
            return Err(ValueError::InvalidMemoryExpectedSequence);
        }

        Ok(Self {
            schema_version: MEMORY_SCHEMA_V1,
            command_id,
            memory_id,
            expected_sequence,
            correlation_id,
            payload,
        })
    }

    /// Returns the serialized schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the stable command identity.
    #[must_use]
    pub const fn command_id(&self) -> &CommandId {
        &self.command_id
    }

    /// Returns the target memory item.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the optimistic per-item sequence expected by an update.
    #[must_use]
    pub const fn expected_sequence(&self) -> Option<MemorySequence> {
        self.expected_sequence
    }

    /// Returns the logical-operation correlation identity.
    #[must_use]
    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    /// Returns the requested memory intent.
    #[must_use]
    pub const fn payload(&self) -> &MemoryCommandPayload {
        &self.payload
    }
}

/// Identifies the command or prior operation that directly caused an operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum MemoryCausation {
    /// The operation directly resulted from an accepted single-use command.
    Command(CommandId),
    /// The operation directly resulted from an earlier memory operation.
    Operation(MemoryOperationId),
}

/// A durable memory-ledger fact emitted only after aggregate validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum MemoryOperationPayload {
    /// A new item and its first revision became durable.
    MemoryCreated {
        /// Authorization scope for retrieval.
        scope: MemoryScope,
        /// Semantic memory kind.
        memory_kind: MemoryKind,
        /// Persisted first revision.
        revision: MemoryRevision,
    },
    /// An untrusted candidate revision became durable for review.
    RevisionProposed {
        /// Persisted proposal revision.
        revision: MemoryRevision,
    },
    /// A trusted replacement revision became durable.
    MemoryRevised {
        /// Persisted replacement revision.
        revision: MemoryRevision,
    },
    /// Deterministic validation of exact revision bytes became durable.
    RevisionValidated {
        /// Validated revision identity.
        revision_id: MemoryRevisionId,
        /// Versioned validation result.
        validation: MemoryValidationResult,
    },
    /// A distinct approved revision was created from an untrusted proposal.
    ProposalApproved {
        /// Original untrusted proposal.
        proposal_revision_id: MemoryRevisionId,
        /// New approved immutable revision.
        approved_revision: MemoryRevision,
    },
    /// A validated revision became context-eligible.
    RevisionActivated {
        /// Activated revision identity.
        revision_id: MemoryRevisionId,
    },
    /// A prior revision was replaced without mutation.
    RevisionSuperseded {
        /// Replaced revision identity.
        revision_id: MemoryRevisionId,
        /// New current revision identity.
        superseded_by_revision_id: MemoryRevisionId,
    },
    /// A proposal was rejected while retaining its audit envelope.
    RevisionRejected {
        /// Rejected revision identity.
        revision_id: MemoryRevisionId,
        /// Stable rejection reason.
        reason: MemoryRejectionReason,
    },
    /// The current revision became ineligible for future admission.
    MemoryRetracted {
        /// Retracted revision identity.
        revision_id: MemoryRevisionId,
    },
    /// Application-owned content was removed and a tombstone remained.
    MemoryDeleted {
        /// Deleted current revision identity.
        revision_id: MemoryRevisionId,
    },
}

/// A versioned operation in one memory item's independent event stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryOperationEnvelope {
    schema_version: u16,
    operation_id: MemoryOperationId,
    memory_id: MemoryId,
    sequence: MemorySequence,
    occurred_at: TimestampMillis,
    causation: MemoryCausation,
    correlation_id: CorrelationId,
    payload: MemoryOperationPayload,
}

impl MemoryOperationEnvelope {
    /// Constructs a memory operation using the current v1 schema.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new_v1(
        operation_id: MemoryOperationId,
        memory_id: MemoryId,
        sequence: MemorySequence,
        occurred_at: TimestampMillis,
        causation: MemoryCausation,
        correlation_id: CorrelationId,
        payload: MemoryOperationPayload,
    ) -> Self {
        Self {
            schema_version: MEMORY_SCHEMA_V1,
            operation_id,
            memory_id,
            sequence,
            occurred_at,
            causation,
            correlation_id,
            payload,
        }
    }

    /// Returns the serialized schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the stable operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> &MemoryOperationId {
        &self.operation_id
    }

    /// Returns the owning memory item.
    #[must_use]
    pub const fn memory_id(&self) -> &MemoryId {
        &self.memory_id
    }

    /// Returns the one-based per-item ordering key.
    #[must_use]
    pub const fn sequence(&self) -> MemorySequence {
        self.sequence
    }

    /// Returns when the operation became durable.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampMillis {
        self.occurred_at
    }

    /// Returns the direct cause of this operation.
    #[must_use]
    pub const fn causation(&self) -> &MemoryCausation {
        &self.causation
    }

    /// Returns the logical-operation correlation identity.
    #[must_use]
    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    /// Returns the durable memory fact.
    #[must_use]
    pub const fn payload(&self) -> &MemoryOperationPayload {
        &self.payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("valid digest")
    }

    fn draft() -> MemoryRevisionDraft {
        MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-1").expect("valid revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            MemoryContent::new("Use compact terminal explanations.").expect("valid content"),
            digest('a'),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(10_000).expect("valid confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("valid draft")
    }

    #[test]
    fn content_and_evidence_debug_output_are_redacted() {
        let secret = "sensitive memory content";
        let content = MemoryContent::new(secret).expect("valid memory content");
        let excerpt = MemoryEvidenceExcerpt::new(secret).expect("valid evidence excerpt");

        assert!(!format!("{content:?}").contains(secret));
        assert!(!format!("{excerpt:?}").contains(secret));
    }

    #[test]
    fn hashes_confidence_and_sequences_enforce_canonical_bounds() {
        assert!(Sha256Digest::new("a".repeat(64)).is_ok());
        assert_eq!(
            Sha256Digest::new("A".repeat(64)),
            Err(ValueError::InvalidSha256Digest)
        );
        assert_eq!(
            ConfidenceBasisPoints::new(10_001),
            Err(ValueError::ConfidenceOutOfRange)
        );
        assert_eq!(MemorySequence::new(0), Err(ValueError::ZeroMemorySequence));
        assert_eq!(
            MemoryRevisionNumber::new(0),
            Err(ValueError::ZeroMemoryRevision)
        );
    }

    #[test]
    fn validity_windows_cannot_be_empty_or_reversed() {
        assert_eq!(
            MemoryValidityWindow::new(TimestampMillis::new(2), TimestampMillis::new(2)),
            Err(ValueError::InvalidMemoryValidity)
        );
        assert_eq!(
            MemoryValidityWindow::new(TimestampMillis::new(3), TimestampMillis::new(2)),
            Err(ValueError::InvalidMemoryValidity)
        );
    }

    #[test]
    fn command_sequence_semantics_distinguish_create_from_update() {
        let create = MemoryCommandPayload::CreateMemory {
            scope: MemoryScope::User(UserId::new("user-1").expect("valid user ID")),
            memory_kind: MemoryKind::Preference,
            revision: draft(),
        };
        let command_id = CommandId::new("command-1").expect("valid command ID");
        let memory_id = MemoryId::new("memory-1").expect("valid memory ID");
        let correlation_id = CorrelationId::new("correlation-1").expect("valid correlation ID");

        assert!(
            MemoryCommandEnvelope::new_v1(
                command_id.clone(),
                memory_id.clone(),
                None,
                correlation_id.clone(),
                create.clone(),
            )
            .is_ok()
        );
        assert_eq!(
            MemoryCommandEnvelope::new_v1(
                command_id,
                memory_id,
                Some(MemorySequence::FIRST),
                correlation_id,
                create,
            ),
            Err(ValueError::InvalidMemoryExpectedSequence)
        );
    }

    #[test]
    fn deserialization_cannot_bypass_memory_value_validation() {
        assert!(serde_json::from_str::<MemoryContent>(r#""   ""#).is_err());
        assert!(serde_json::from_str::<Sha256Digest>(r#""abc""#).is_err());
        assert!(serde_json::from_str::<ConfidenceBasisPoints>("10001").is_err());
        assert!(serde_json::from_str::<MemorySequence>("0").is_err());
    }

    #[test]
    fn event_id_is_available_to_evidence_without_native_payloads() {
        let source = MemoryEvidenceSource::SessionEvent {
            session_id: SessionId::new("session-1").expect("valid session ID"),
            event_id: EventId::new("event-1").expect("valid event ID"),
        };

        assert!(matches!(source, MemoryEvidenceSource::SessionEvent { .. }));
    }

    #[test]
    fn tool_names_can_identify_future_policy_authors() {
        let name = crate::ToolName::new("memory_propose").expect("valid tool name");

        assert_eq!(name.as_str(), "memory_propose");
    }
}
