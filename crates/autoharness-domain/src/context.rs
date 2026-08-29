use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    AgentId, AttemptId, ContextAdmissionId, ContextEpochId, ContextSourceKey, ContextTurnId,
    MemoryRevisionId, MemoryScope, ModelRef, Sensitivity, SessionId, SessionSequence, Sha256Digest,
    TimestampMillis, UserId, ValueError, WorkspaceId,
};

/// Maximum source observations retained in one provider-turn manifest.
pub const MAX_CONTEXT_SOURCES: usize = 256;

/// Maximum admissions retained in one provider-turn manifest.
pub const MAX_CONTEXT_ADMISSIONS: usize = 256;

/// Maximum deterministic reason factors retained for one admission.
pub const MAX_CONTEXT_ADMISSION_REASONS: usize = 16;

/// A non-zero token budget constrained to the signed durable-store range.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ContextTokenBudget(u64);

impl ContextTokenBudget {
    /// Validates and constructs a token budget.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 || value > i64::MAX as u64 {
            return Err(ValueError::InvalidContextTokenBudget);
        }

        Ok(Self(value))
    }

    /// Returns the token budget.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ContextTokenBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A deterministic conservative token estimate without floating-point state.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EstimatedTokens(u64);

impl EstimatedTokens {
    /// Validates and constructs an estimated token count.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value > i64::MAX as u64 {
            return Err(ValueError::EstimatedTokensTooLarge);
        }

        Ok(Self(value))
    }

    /// Returns the estimated token count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for EstimatedTokens {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A monotonic global generation for eligibility-changing memory operations.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MemoryGeneration(u64);

impl MemoryGeneration {
    /// The generation before any eligibility-changing memory operation.
    pub const INITIAL: Self = Self(0);

    /// Validates and constructs a memory-store generation.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value > i64::MAX as u64 {
            return Err(ValueError::MemoryGenerationTooLarge);
        }

        Ok(Self(value))
    }

    /// Returns the generation value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation, or `None` at durable integer exhaustion.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) if value <= i64::MAX as u64 => Some(Self(value)),
            Some(_) | None => None,
        }
    }
}

impl<'de> Deserialize<'de> for MemoryGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Exact scope authority and sensitivity ceiling frozen for one provider turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEligibility {
    user_id: UserId,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_id: Option<AgentId>,
    sensitivity_ceiling: Sensitivity,
}

impl ContextEligibility {
    /// Constructs an exact provider-turn eligibility boundary.
    #[must_use]
    pub const fn new(
        user_id: UserId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        agent_id: Option<AgentId>,
        sensitivity_ceiling: Sensitivity,
    ) -> Self {
        Self {
            user_id,
            workspace_id,
            session_id,
            agent_id,
            sensitivity_ceiling,
        }
    }

    /// Returns the authorized user scope identity.
    #[must_use]
    pub const fn user_id(&self) -> &UserId {
        &self.user_id
    }

    /// Returns the authorized workspace scope identity.
    #[must_use]
    pub const fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    /// Returns the authorized session scope identity.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the selected agent scope identity, when present.
    #[must_use]
    pub const fn agent_id(&self) -> Option<&AgentId> {
        self.agent_id.as_ref()
    }

    /// Returns the highest sensitivity admitted to this provider turn.
    #[must_use]
    pub const fn sensitivity_ceiling(&self) -> Sensitivity {
        self.sensitivity_ceiling
    }

    /// Returns whether an exact memory scope is authorized.
    #[must_use]
    pub fn permits_scope(&self, scope: &MemoryScope) -> bool {
        match scope {
            MemoryScope::User(id) => id == &self.user_id,
            MemoryScope::Workspace(id) => id == &self.workspace_id,
            MemoryScope::Session(id) => id == &self.session_id,
            MemoryScope::Agent(id) => self.agent_id.as_ref() == Some(id),
        }
    }

    /// Returns whether a sensitivity class is inside the frozen ceiling.
    #[must_use]
    pub const fn permits_sensitivity(&self, sensitivity: Sensitivity) -> bool {
        sensitivity_rank(sensitivity) <= sensitivity_rank(self.sensitivity_ceiling)
    }
}

#[derive(Deserialize)]
struct RawContextEligibility {
    user_id: UserId,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    agent_id: Option<AgentId>,
    sensitivity_ceiling: Sensitivity,
}

impl<'de> Deserialize<'de> for ContextEligibility {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextEligibility::deserialize(deserializer)?;
        Ok(Self::new(
            raw.user_id,
            raw.workspace_id,
            raw.session_id,
            raw.agent_id,
            raw.sensitivity_ceiling,
        ))
    }
}

/// Explicit allocation of the fixed provider-turn context budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ContextBudgetAllocation {
    token_budget: ContextTokenBudget,
    reserved_tokens: EstimatedTokens,
    durable_memory_limit: EstimatedTokens,
}

impl ContextBudgetAllocation {
    /// Constructs an allocation whose sections cannot exceed the total.
    pub const fn new(
        token_budget: ContextTokenBudget,
        reserved_tokens: EstimatedTokens,
        durable_memory_limit: EstimatedTokens,
    ) -> Result<Self, ValueError> {
        if reserved_tokens.get() > token_budget.get()
            || durable_memory_limit.get() > token_budget.get() - reserved_tokens.get()
        {
            return Err(ValueError::InvalidContextManifest);
        }
        Ok(Self {
            token_budget,
            reserved_tokens,
            durable_memory_limit,
        })
    }

    /// Returns the complete provider-turn context budget.
    #[must_use]
    pub const fn token_budget(self) -> ContextTokenBudget {
        self.token_budget
    }

    /// Returns bytes or tokens reserved before registered context and memory.
    #[must_use]
    pub const fn reserved_tokens(self) -> EstimatedTokens {
        self.reserved_tokens
    }

    /// Returns the maximum allocation for durable memory records.
    #[must_use]
    pub const fn durable_memory_limit(self) -> EstimatedTokens {
        self.durable_memory_limit
    }

    /// Returns the remaining allocation for all rendered prelude sections.
    #[must_use]
    pub const fn rendered_limit(self) -> u64 {
        self.token_budget.get() - self.reserved_tokens.get()
    }
}

#[derive(Deserialize)]
struct RawContextBudgetAllocation {
    token_budget: ContextTokenBudget,
    reserved_tokens: EstimatedTokens,
    durable_memory_limit: EstimatedTokens,
}

impl<'de> Deserialize<'de> for ContextBudgetAllocation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextBudgetAllocation::deserialize(deserializer)?;
        Self::new(
            raw.token_budget,
            raw.reserved_tokens,
            raw.durable_memory_limit,
        )
        .map_err(D::Error::custom)
    }
}

const fn sensitivity_rank(sensitivity: Sensitivity) -> u8 {
    match sensitivity {
        Sensitivity::Public => 0,
        Sensitivity::Internal => 1,
        Sensitivity::Sensitive => 2,
        Sensitivity::Secret => 3,
    }
}

/// The stable reason that started a new immutable context epoch.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEpochReason {
    /// A new top-level provider attempt started.
    NewAttempt,
    /// The user explicitly retried a settled provider attempt.
    ExplicitRetry,
    /// Deterministic compaction replaced the prior baseline.
    Compaction,
    /// A source became incompatible with the prior epoch contract.
    SourceIncompatibility,
    /// Trusted policy intentionally changed context behavior.
    PolicyChange,
    /// Recovery established a new safe dispatch baseline.
    Recovery,
}

/// Version identities for every deterministic context algorithm boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEpochVersions {
    builder_version: u16,
    registry_version: u16,
    ranker_version: u16,
    renderer_version: u16,
    sizer_version: u16,
}

impl ContextEpochVersions {
    /// Constructs non-zero algorithm contract versions.
    pub const fn new(
        builder_version: u16,
        registry_version: u16,
        ranker_version: u16,
        renderer_version: u16,
        sizer_version: u16,
    ) -> Result<Self, ValueError> {
        if builder_version == 0
            || registry_version == 0
            || ranker_version == 0
            || renderer_version == 0
            || sizer_version == 0
        {
            return Err(ValueError::ZeroVersion);
        }

        Ok(Self {
            builder_version,
            registry_version,
            ranker_version,
            renderer_version,
            sizer_version,
        })
    }

    /// Returns the context builder contract version.
    #[must_use]
    pub const fn builder_version(self) -> u16 {
        self.builder_version
    }

    /// Returns the source registry contract version.
    #[must_use]
    pub const fn registry_version(self) -> u16 {
        self.registry_version
    }

    /// Returns the deterministic ranker contract version.
    #[must_use]
    pub const fn ranker_version(self) -> u16 {
        self.ranker_version
    }

    /// Returns the canonical renderer contract version.
    #[must_use]
    pub const fn renderer_version(self) -> u16 {
        self.renderer_version
    }

    /// Returns the conservative token sizer contract version.
    #[must_use]
    pub const fn sizer_version(self) -> u16 {
        self.sizer_version
    }
}

#[derive(Deserialize)]
struct RawContextEpochVersions {
    builder_version: u16,
    registry_version: u16,
    ranker_version: u16,
    renderer_version: u16,
    sizer_version: u16,
}

impl<'de> Deserialize<'de> for ContextEpochVersions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextEpochVersions::deserialize(deserializer)?;
        Self::new(
            raw.builder_version,
            raw.registry_version,
            raw.ranker_version,
            raw.renderer_version,
            raw.sizer_version,
        )
        .map_err(D::Error::custom)
    }
}

/// Exact hashes for mutable inputs frozen at an epoch boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextEpochHashes {
    config_hash: Sha256Digest,
    catalog_hash: Sha256Digest,
    model_capability_hash: Sha256Digest,
    tool_registry_hash: Sha256Digest,
}

impl ContextEpochHashes {
    /// Constructs the complete set of mutable-input hashes.
    #[must_use]
    pub const fn new(
        config_hash: Sha256Digest,
        catalog_hash: Sha256Digest,
        model_capability_hash: Sha256Digest,
        tool_registry_hash: Sha256Digest,
    ) -> Self {
        Self {
            config_hash,
            catalog_hash,
            model_capability_hash,
            tool_registry_hash,
        }
    }

    /// Returns the effective configuration hash.
    #[must_use]
    pub const fn config_hash(&self) -> &Sha256Digest {
        &self.config_hash
    }

    /// Returns the model catalog snapshot hash.
    #[must_use]
    pub const fn catalog_hash(&self) -> &Sha256Digest {
        &self.catalog_hash
    }

    /// Returns the selected model capability snapshot hash.
    #[must_use]
    pub const fn model_capability_hash(&self) -> &Sha256Digest {
        &self.model_capability_hash
    }

    /// Returns the advertised tool registry hash.
    #[must_use]
    pub const fn tool_registry_hash(&self) -> &Sha256Digest {
        &self.tool_registry_hash
    }
}

/// Immutable compatibility metadata shared by every turn in one attempt epoch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEpochManifest {
    epoch_id: ContextEpochId,
    session_id: SessionId,
    memory_generation: MemoryGeneration,
    reason: ContextEpochReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    predecessor_epoch_id: Option<ContextEpochId>,
    baseline_hash: Sha256Digest,
    versions: ContextEpochVersions,
    hashes: ContextEpochHashes,
    token_budget: ContextTokenBudget,
    started_at: TimestampMillis,
}

impl ContextEpochManifest {
    /// Constructs an epoch and rejects a self-referential predecessor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch_id: ContextEpochId,
        session_id: SessionId,
        memory_generation: MemoryGeneration,
        reason: ContextEpochReason,
        predecessor_epoch_id: Option<ContextEpochId>,
        baseline_hash: Sha256Digest,
        versions: ContextEpochVersions,
        hashes: ContextEpochHashes,
        token_budget: ContextTokenBudget,
        started_at: TimestampMillis,
    ) -> Result<Self, ValueError> {
        if predecessor_epoch_id.as_ref() == Some(&epoch_id) {
            return Err(ValueError::InvalidContextEpoch);
        }

        Ok(Self {
            epoch_id,
            session_id,
            memory_generation,
            reason,
            predecessor_epoch_id,
            baseline_hash,
            versions,
            hashes,
            token_budget,
            started_at,
        })
    }

    /// Returns the stable epoch identity.
    #[must_use]
    pub const fn epoch_id(&self) -> &ContextEpochId {
        &self.epoch_id
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the frozen memory-store generation.
    #[must_use]
    pub const fn memory_generation(&self) -> MemoryGeneration {
        self.memory_generation
    }

    /// Returns why the epoch began.
    #[must_use]
    pub const fn reason(&self) -> ContextEpochReason {
        self.reason
    }

    /// Returns the preceding epoch when this epoch replaced one.
    #[must_use]
    pub const fn predecessor_epoch_id(&self) -> Option<&ContextEpochId> {
        self.predecessor_epoch_id.as_ref()
    }

    /// Returns the exact frozen baseline hash.
    #[must_use]
    pub const fn baseline_hash(&self) -> &Sha256Digest {
        &self.baseline_hash
    }

    /// Returns all deterministic algorithm versions.
    #[must_use]
    pub const fn versions(&self) -> ContextEpochVersions {
        self.versions
    }

    /// Returns all mutable-input hashes.
    #[must_use]
    pub const fn hashes(&self) -> &ContextEpochHashes {
        &self.hashes
    }

    /// Returns the fixed epoch token budget.
    #[must_use]
    pub const fn token_budget(&self) -> ContextTokenBudget {
        self.token_budget
    }

    /// Returns when the epoch became durable.
    #[must_use]
    pub const fn started_at(&self) -> TimestampMillis {
        self.started_at
    }
}

#[derive(Deserialize)]
struct RawContextEpochManifest {
    epoch_id: ContextEpochId,
    session_id: SessionId,
    memory_generation: MemoryGeneration,
    reason: ContextEpochReason,
    predecessor_epoch_id: Option<ContextEpochId>,
    baseline_hash: Sha256Digest,
    versions: ContextEpochVersions,
    hashes: ContextEpochHashes,
    token_budget: ContextTokenBudget,
    started_at: TimestampMillis,
}

impl<'de> Deserialize<'de> for ContextEpochManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextEpochManifest::deserialize(deserializer)?;
        Self::new(
            raw.epoch_id,
            raw.session_id,
            raw.memory_generation,
            raw.reason,
            raw.predecessor_epoch_id,
            raw.baseline_hash,
            raw.versions,
            raw.hashes,
            raw.token_budget,
            raw.started_at,
        )
        .map_err(D::Error::custom)
    }
}

/// The explicit result of observing one registered context source.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextObservationState {
    /// The source was read successfully at the recorded revision.
    Available,
    /// A prior verified revision was retained after a temporary read failure.
    RetainedStale,
    /// A successful observation confirmed that the source does not exist.
    ObservedAbsent,
    /// The source could not be observed and no stale value was retained.
    Unavailable,
}

/// One immutable source observation frozen for a provider turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextSourceSnapshot {
    source_key: ContextSourceKey,
    observation_state: ContextObservationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_revision: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value_hash: Option<Sha256Digest>,
    observed_at: TimestampMillis,
}

impl ContextSourceSnapshot {
    /// Constructs an observation with revision fields consistent with its state.
    pub fn new(
        source_key: ContextSourceKey,
        observation_state: ContextObservationState,
        source_revision: Option<Sha256Digest>,
        value_hash: Option<Sha256Digest>,
        observed_at: TimestampMillis,
    ) -> Result<Self, ValueError> {
        let has_value = source_revision.is_some() && value_hash.is_some();
        let has_no_value = source_revision.is_none() && value_hash.is_none();
        let valid = match observation_state {
            ContextObservationState::Available | ContextObservationState::RetainedStale => {
                has_value
            }
            ContextObservationState::ObservedAbsent | ContextObservationState::Unavailable => {
                has_no_value
            }
        };
        if !valid {
            return Err(ValueError::InvalidContextObservation);
        }

        Ok(Self {
            source_key,
            observation_state,
            source_revision,
            value_hash,
            observed_at,
        })
    }

    /// Returns the stable source key.
    #[must_use]
    pub const fn source_key(&self) -> &ContextSourceKey {
        &self.source_key
    }

    /// Returns the explicit observation state.
    #[must_use]
    pub const fn observation_state(&self) -> ContextObservationState {
        self.observation_state
    }

    /// Returns the observed or retained source revision.
    #[must_use]
    pub const fn source_revision(&self) -> Option<&Sha256Digest> {
        self.source_revision.as_ref()
    }

    /// Returns the hash of the exact observed or retained value.
    #[must_use]
    pub const fn value_hash(&self) -> Option<&Sha256Digest> {
        self.value_hash.as_ref()
    }

    /// Returns when the observation attempt occurred.
    #[must_use]
    pub const fn observed_at(&self) -> TimestampMillis {
        self.observed_at
    }
}

#[derive(Deserialize)]
struct RawContextSourceSnapshot {
    source_key: ContextSourceKey,
    observation_state: ContextObservationState,
    source_revision: Option<Sha256Digest>,
    value_hash: Option<Sha256Digest>,
    observed_at: TimestampMillis,
}

impl<'de> Deserialize<'de> for ContextSourceSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextSourceSnapshot::deserialize(deserializer)?;
        Self::new(
            raw.source_key,
            raw.observation_state,
            raw.source_revision,
            raw.value_hash,
            raw.observed_at,
        )
        .map_err(D::Error::custom)
    }
}

/// The canonical context section receiving an admitted value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSection {
    /// Non-overridable product safety policy.
    SafetyPolicy,
    /// Exact current user instruction.
    CurrentInstruction,
    /// Authorized workspace or agent instruction.
    AuthorizedInstruction,
    /// Frozen provider-neutral tool contract.
    ToolContract,
    /// Prior conversation and settled tool history.
    ConversationHistory,
    /// Eligible durable memory rendered as untrusted data.
    DurableMemory,
}

/// A stable integer-only factor used to explain deterministic ranking.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextAdmissionFactor {
    /// Policy marked the candidate as mandatory or pinned.
    Pin,
    /// Source authority affected ranking.
    Authority,
    /// An exact structured key matched.
    ExactMatch,
    /// A narrower authorized scope matched.
    ScopeSpecificity,
    /// Deterministic lexical overlap affected ranking.
    LexicalOverlap,
    /// Validity and observation freshness affected ranking.
    Freshness,
    /// Integer confidence basis points affected ranking.
    Confidence,
    /// Prior measured utility affected ranking.
    PriorUtility,
    /// Diversity policy affected ranking.
    Diversity,
    /// The complete candidate fit its section budget.
    BudgetFit,
}

/// One stable contribution to an integer-only admission score.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ContextAdmissionReason {
    ordinal: u16,
    factor: ContextAdmissionFactor,
    contribution: i64,
}

#[derive(Deserialize)]
struct RawContextAdmissionReason {
    ordinal: u16,
    factor: ContextAdmissionFactor,
    contribution: i64,
}

impl<'de> Deserialize<'de> for ContextAdmissionReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextAdmissionReason::deserialize(deserializer)?;
        Self::new(raw.ordinal, raw.factor, raw.contribution).map_err(D::Error::custom)
    }
}

impl ContextAdmissionReason {
    /// Constructs a one-based ordered reason factor.
    pub const fn new(
        ordinal: u16,
        factor: ContextAdmissionFactor,
        contribution: i64,
    ) -> Result<Self, ValueError> {
        if ordinal == 0 {
            return Err(ValueError::ZeroContextOrdinal);
        }

        Ok(Self {
            ordinal,
            factor,
            contribution,
        })
    }

    /// Returns the one-based reason order.
    #[must_use]
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }

    /// Returns the stable factor kind.
    #[must_use]
    pub const fn factor(self) -> ContextAdmissionFactor {
        self.factor
    }

    /// Returns the signed integer score contribution.
    #[must_use]
    pub const fn contribution(self) -> i64 {
        self.contribution
    }
}

/// One immutable decision to render a complete value into a provider turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextAdmission {
    admission_id: ContextAdmissionId,
    context_turn_id: ContextTurnId,
    section: ContextSection,
    source_key: ContextSourceKey,
    source_revision: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory_revision_id: Option<MemoryRevisionId>,
    renderer_version: u16,
    rendered_hash: Sha256Digest,
    rank: u32,
    rank_score: i64,
    token_count: EstimatedTokens,
    admitted_at: TimestampMillis,
    reasons: Vec<ContextAdmissionReason>,
}

impl ContextAdmission {
    /// Constructs a bounded admission with contiguous one-based reason ordinals.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        admission_id: ContextAdmissionId,
        context_turn_id: ContextTurnId,
        section: ContextSection,
        source_key: ContextSourceKey,
        source_revision: Sha256Digest,
        memory_revision_id: Option<MemoryRevisionId>,
        renderer_version: u16,
        rendered_hash: Sha256Digest,
        rank: u32,
        rank_score: i64,
        token_count: EstimatedTokens,
        admitted_at: TimestampMillis,
        reasons: Vec<ContextAdmissionReason>,
    ) -> Result<Self, ValueError> {
        if renderer_version == 0 {
            return Err(ValueError::ZeroVersion);
        }
        if rank == 0 {
            return Err(ValueError::ZeroContextOrdinal);
        }
        if reasons.len() > MAX_CONTEXT_ADMISSION_REASONS
            || reasons
                .iter()
                .enumerate()
                .any(|(index, reason)| usize::from(reason.ordinal) != index + 1)
        {
            return Err(ValueError::InvalidContextManifest);
        }

        Ok(Self {
            admission_id,
            context_turn_id,
            section,
            source_key,
            source_revision,
            memory_revision_id,
            renderer_version,
            rendered_hash,
            rank,
            rank_score,
            token_count,
            admitted_at,
            reasons,
        })
    }

    /// Returns the stable admission identity.
    #[must_use]
    pub const fn admission_id(&self) -> &ContextAdmissionId {
        &self.admission_id
    }

    /// Returns the exact provider turn receiving the admission.
    #[must_use]
    pub const fn context_turn_id(&self) -> &ContextTurnId {
        &self.context_turn_id
    }

    /// Returns the canonical rendered section.
    #[must_use]
    pub const fn section(&self) -> ContextSection {
        self.section
    }

    /// Returns the stable source key.
    #[must_use]
    pub const fn source_key(&self) -> &ContextSourceKey {
        &self.source_key
    }

    /// Returns the exact admitted source revision.
    #[must_use]
    pub const fn source_revision(&self) -> &Sha256Digest {
        &self.source_revision
    }

    /// Returns the admitted memory revision when this is durable memory.
    #[must_use]
    pub const fn memory_revision_id(&self) -> Option<&MemoryRevisionId> {
        self.memory_revision_id.as_ref()
    }

    /// Returns the canonical renderer contract version.
    #[must_use]
    pub const fn renderer_version(&self) -> u16 {
        self.renderer_version
    }

    /// Returns the exact rendered value hash.
    #[must_use]
    pub const fn rendered_hash(&self) -> &Sha256Digest {
        &self.rendered_hash
    }

    /// Returns the one-based deterministic rank.
    #[must_use]
    pub const fn rank(&self) -> u32 {
        self.rank
    }

    /// Returns the final integer-only rank score.
    #[must_use]
    pub const fn rank_score(&self) -> i64 {
        self.rank_score
    }

    /// Returns the conservative rendered token estimate.
    #[must_use]
    pub const fn token_count(&self) -> EstimatedTokens {
        self.token_count
    }

    /// Returns when the admission became durable.
    #[must_use]
    pub const fn admitted_at(&self) -> TimestampMillis {
        self.admitted_at
    }

    /// Returns deterministic rank factors in one-based order.
    #[must_use]
    pub fn reasons(&self) -> &[ContextAdmissionReason] {
        &self.reasons
    }
}

#[derive(Deserialize)]
struct RawContextAdmission {
    admission_id: ContextAdmissionId,
    context_turn_id: ContextTurnId,
    section: ContextSection,
    source_key: ContextSourceKey,
    source_revision: Sha256Digest,
    memory_revision_id: Option<MemoryRevisionId>,
    renderer_version: u16,
    rendered_hash: Sha256Digest,
    rank: u32,
    rank_score: i64,
    token_count: EstimatedTokens,
    admitted_at: TimestampMillis,
    reasons: Vec<ContextAdmissionReason>,
}

impl<'de> Deserialize<'de> for ContextAdmission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextAdmission::deserialize(deserializer)?;
        Self::new(
            raw.admission_id,
            raw.context_turn_id,
            raw.section,
            raw.source_key,
            raw.source_revision,
            raw.memory_revision_id,
            raw.renderer_version,
            raw.rendered_hash,
            raw.rank,
            raw.rank_score,
            raw.token_count,
            raw.admitted_at,
            raw.reasons,
        )
        .map_err(D::Error::custom)
    }
}

/// The exact committed context identity for one provider call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextTurnManifest {
    context_turn_id: ContextTurnId,
    epoch_id: ContextEpochId,
    session_id: SessionId,
    attempt_id: AttemptId,
    run_turn: u32,
    expected_session_sequence: SessionSequence,
    memory_generation: MemoryGeneration,
    model: ModelRef,
    request_hash: Sha256Digest,
    rendered_hash: Sha256Digest,
    manifest_hash: Sha256Digest,
    eligibility: ContextEligibility,
    budget: ContextBudgetAllocation,
    rendered_token_count: EstimatedTokens,
    committed_at: TimestampMillis,
    sources: Vec<ContextSourceSnapshot>,
    admissions: Vec<ContextAdmission>,
}

impl ContextTurnManifest {
    /// Constructs a bounded turn manifest keyed by attempt and non-zero run turn.
    ///
    /// Sources must be strictly ordered by source key.
    /// Admissions must use this turn identity with contiguous one-based ranks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context_turn_id: ContextTurnId,
        epoch_id: ContextEpochId,
        session_id: SessionId,
        attempt_id: AttemptId,
        run_turn: u32,
        expected_session_sequence: SessionSequence,
        memory_generation: MemoryGeneration,
        model: ModelRef,
        request_hash: Sha256Digest,
        rendered_hash: Sha256Digest,
        manifest_hash: Sha256Digest,
        eligibility: ContextEligibility,
        budget: ContextBudgetAllocation,
        rendered_token_count: EstimatedTokens,
        committed_at: TimestampMillis,
        sources: Vec<ContextSourceSnapshot>,
        admissions: Vec<ContextAdmission>,
    ) -> Result<Self, ValueError> {
        if run_turn == 0 {
            return Err(ValueError::ZeroContextRunTurn);
        }
        if sources.len() > MAX_CONTEXT_SOURCES || admissions.len() > MAX_CONTEXT_ADMISSIONS {
            return Err(ValueError::CollectionTooLarge);
        }
        let admission_token_count = admissions.iter().try_fold(0_u64, |total, admission| {
            total.checked_add(admission.token_count.get())
        });
        let durable_memory_token_count = admissions
            .iter()
            .filter(|admission| admission.section == ContextSection::DurableMemory)
            .try_fold(0_u64, |total, admission| {
                total.checked_add(admission.token_count.get())
            });
        if eligibility.session_id() != &session_id
            || rendered_token_count.get() > budget.rendered_limit()
            || admission_token_count.is_none_or(|total| total > rendered_token_count.get())
            || durable_memory_token_count
                .is_none_or(|total| total > budget.durable_memory_limit().get())
            || !sources
                .windows(2)
                .all(|pair| pair[0].source_key < pair[1].source_key)
            || admissions.iter().enumerate().any(|(index, admission)| {
                admission.context_turn_id != context_turn_id
                    || usize::try_from(admission.rank).ok() != Some(index + 1)
            })
        {
            return Err(ValueError::InvalidContextManifest);
        }

        Ok(Self {
            context_turn_id,
            epoch_id,
            session_id,
            attempt_id,
            run_turn,
            expected_session_sequence,
            memory_generation,
            model,
            request_hash,
            rendered_hash,
            manifest_hash,
            eligibility,
            budget,
            rendered_token_count,
            committed_at,
            sources,
            admissions,
        })
    }

    /// Returns the stable context turn identity.
    #[must_use]
    pub const fn context_turn_id(&self) -> &ContextTurnId {
        &self.context_turn_id
    }

    /// Returns the immutable epoch identity.
    #[must_use]
    pub const fn epoch_id(&self) -> &ContextEpochId {
        &self.epoch_id
    }

    /// Returns the owning session.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns the owning provider attempt.
    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    /// Returns the one-based provider turn within the attempt.
    #[must_use]
    pub const fn run_turn(&self) -> u32 {
        self.run_turn
    }

    /// Returns the session sequence checked by the atomic context commit.
    #[must_use]
    pub const fn expected_session_sequence(&self) -> SessionSequence {
        self.expected_session_sequence
    }

    /// Returns the memory generation checked by the atomic context commit.
    #[must_use]
    pub const fn memory_generation(&self) -> MemoryGeneration {
        self.memory_generation
    }

    /// Returns the exact selected provider-neutral model snapshot.
    #[must_use]
    pub const fn model(&self) -> &ModelRef {
        &self.model
    }

    /// Returns the exact canonical provider request hash.
    #[must_use]
    pub const fn request_hash(&self) -> &Sha256Digest {
        &self.request_hash
    }

    /// Returns the hash of all rendered context bytes.
    #[must_use]
    pub const fn rendered_hash(&self) -> &Sha256Digest {
        &self.rendered_hash
    }

    /// Returns the hash of the canonical manifest representation.
    #[must_use]
    pub const fn manifest_hash(&self) -> &Sha256Digest {
        &self.manifest_hash
    }

    /// Returns the fixed provider-turn token budget.
    #[must_use]
    pub const fn token_budget(&self) -> ContextTokenBudget {
        self.budget.token_budget()
    }

    /// Returns the exact scope and sensitivity authority for this turn.
    #[must_use]
    pub const fn eligibility(&self) -> &ContextEligibility {
        &self.eligibility
    }

    /// Returns the complete explicit context budget allocation.
    #[must_use]
    pub const fn budget(&self) -> ContextBudgetAllocation {
        self.budget
    }

    /// Returns the conservative total rendered token estimate.
    #[must_use]
    pub const fn rendered_token_count(&self) -> EstimatedTokens {
        self.rendered_token_count
    }

    /// Returns when the manifest became durable.
    #[must_use]
    pub const fn committed_at(&self) -> TimestampMillis {
        self.committed_at
    }

    /// Returns source observations in strict source-key order.
    #[must_use]
    pub fn sources(&self) -> &[ContextSourceSnapshot] {
        &self.sources
    }

    /// Returns admissions in contiguous rank order.
    #[must_use]
    pub fn admissions(&self) -> &[ContextAdmission] {
        &self.admissions
    }
}

#[derive(Deserialize)]
struct RawContextTurnManifest {
    context_turn_id: ContextTurnId,
    epoch_id: ContextEpochId,
    session_id: SessionId,
    attempt_id: AttemptId,
    run_turn: u32,
    expected_session_sequence: SessionSequence,
    memory_generation: MemoryGeneration,
    model: ModelRef,
    request_hash: Sha256Digest,
    rendered_hash: Sha256Digest,
    manifest_hash: Sha256Digest,
    eligibility: ContextEligibility,
    budget: ContextBudgetAllocation,
    rendered_token_count: EstimatedTokens,
    committed_at: TimestampMillis,
    sources: Vec<ContextSourceSnapshot>,
    admissions: Vec<ContextAdmission>,
}

impl<'de> Deserialize<'de> for ContextTurnManifest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawContextTurnManifest::deserialize(deserializer)?;
        Self::new(
            raw.context_turn_id,
            raw.epoch_id,
            raw.session_id,
            raw.attempt_id,
            raw.run_turn,
            raw.expected_session_sequence,
            raw.memory_generation,
            raw.model,
            raw.request_hash,
            raw.rendered_hash,
            raw.manifest_hash,
            raw.eligibility,
            raw.budget,
            raw.rendered_token_count,
            raw.committed_at,
            raw.sources,
            raw.admissions,
        )
        .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelId, ProviderId};

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("valid digest")
    }

    fn source(key: &str) -> ContextSourceSnapshot {
        ContextSourceSnapshot::new(
            ContextSourceKey::new(key).expect("valid source key"),
            ContextObservationState::Available,
            Some(digest('a')),
            Some(digest('b')),
            TimestampMillis::new(1),
        )
        .expect("valid source")
    }

    #[test]
    fn source_observation_states_cannot_confuse_absence_with_failure() {
        assert!(
            ContextSourceSnapshot::new(
                ContextSourceKey::new("workspace:agents").expect("valid source key"),
                ContextObservationState::Unavailable,
                None,
                None,
                TimestampMillis::new(1),
            )
            .is_ok()
        );
        assert_eq!(
            ContextSourceSnapshot::new(
                ContextSourceKey::new("workspace:agents").expect("valid source key"),
                ContextObservationState::ObservedAbsent,
                Some(digest('a')),
                Some(digest('b')),
                TimestampMillis::new(1),
            ),
            Err(ValueError::InvalidContextObservation)
        );
    }

    #[test]
    fn epoch_versions_and_token_counts_reject_invalid_bounds() {
        assert_eq!(
            ContextEpochVersions::new(1, 1, 0, 1, 1),
            Err(ValueError::ZeroVersion)
        );
        assert_eq!(
            ContextTokenBudget::new(0),
            Err(ValueError::InvalidContextTokenBudget)
        );
        assert_eq!(
            EstimatedTokens::new(i64::MAX as u64 + 1),
            Err(ValueError::EstimatedTokensTooLarge)
        );
    }

    #[test]
    fn turn_manifest_requires_sorted_sources_and_a_nonzero_turn() {
        let build = |run_turn, sources| {
            let session_id = SessionId::new("session-1").expect("valid session ID");
            ContextTurnManifest::new(
                ContextTurnId::new("turn-1").expect("valid turn ID"),
                ContextEpochId::new("epoch-1").expect("valid epoch ID"),
                session_id.clone(),
                AttemptId::new("attempt-1").expect("valid attempt ID"),
                run_turn,
                SessionSequence::FIRST,
                MemoryGeneration::INITIAL,
                ModelRef::new(
                    ProviderId::new("google-ai-studio").expect("valid provider ID"),
                    ModelId::new("models/gemini-test").expect("valid model ID"),
                ),
                digest('c'),
                digest('d'),
                digest('e'),
                ContextEligibility::new(
                    UserId::new("user-1").expect("valid user ID"),
                    WorkspaceId::new("workspace-1").expect("valid workspace ID"),
                    session_id,
                    None,
                    Sensitivity::Internal,
                ),
                ContextBudgetAllocation::new(
                    ContextTokenBudget::new(1_000).expect("valid budget"),
                    EstimatedTokens::new(100).expect("valid reservation"),
                    EstimatedTokens::new(500).expect("valid memory limit"),
                )
                .expect("valid allocation"),
                EstimatedTokens::new(10).expect("valid estimate"),
                TimestampMillis::new(2),
                sources,
                Vec::new(),
            )
        };

        assert_eq!(build(0, Vec::new()), Err(ValueError::ZeroContextRunTurn));
        assert_eq!(
            build(1, vec![source("z"), source("a")]),
            Err(ValueError::InvalidContextManifest)
        );
        assert!(build(1, vec![source("a"), source("z")]).is_ok());
    }

    #[test]
    fn deserialization_cannot_bypass_context_invariants() {
        assert!(serde_json::from_str::<ContextTokenBudget>("0").is_err());
        assert!(serde_json::from_str::<ContextEpochVersions>(
            r#"{"builder_version":1,"registry_version":1,"ranker_version":0,"renderer_version":1,"sizer_version":1}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ContextSourceSnapshot>(
            r#"{"source_key":"source","observation_state":"unavailable","source_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","value_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","observed_at":1}"#
        )
        .is_err());
        assert_eq!(
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(100).expect("budget"),
                EstimatedTokens::new(80).expect("reservation"),
                EstimatedTokens::new(21).expect("memory limit"),
            ),
            Err(ValueError::InvalidContextManifest)
        );
    }

    #[test]
    fn eligibility_is_exact_and_sensitivity_is_fail_closed() {
        let eligibility = ContextEligibility::new(
            UserId::new("user-1").expect("user ID"),
            WorkspaceId::new("workspace-1").expect("workspace ID"),
            SessionId::new("session-1").expect("session ID"),
            Some(AgentId::new("agent-1").expect("agent ID")),
            Sensitivity::Internal,
        );

        assert!(
            eligibility.permits_scope(&MemoryScope::User(UserId::new("user-1").expect("user ID")))
        );
        assert!(!eligibility.permits_scope(&MemoryScope::Workspace(
            WorkspaceId::new("workspace-2").expect("workspace ID")
        )));
        assert!(eligibility.permits_sensitivity(Sensitivity::Internal));
        assert!(!eligibility.permits_sensitivity(Sensitivity::Sensitive));
    }
}
