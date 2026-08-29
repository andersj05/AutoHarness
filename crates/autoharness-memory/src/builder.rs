use std::collections::BTreeSet;

use autoharness_domain::{
    AttemptId, ContextAdmission, ContextAdmissionFactor, ContextAdmissionId,
    ContextAdmissionReason, ContextBudgetAllocation, ContextEligibility, ContextEpochId,
    ContextSection, ContextSourceKey, ContextSourceSnapshot, ContextTokenBudget, ContextTurnId,
    ContextTurnManifest, EstimatedTokens, MemoryGeneration, MemoryRevisionId, ModelRef, SessionId,
    SessionSequence, Sha256Digest, TimestampMillis,
};

use crate::{
    CanonicalEncoder, ContextSizer, ContextSourcePolicy, DeterministicRankerV1, MemoryCandidate,
    MemoryError, MemoryRanker, ObservedContextSource, RankReason, RankedMemory, RenderedMemory,
    RenderedSource, RetrievalScope, Utf8ByteSizerV1, render_context_prelude, render_memory,
    render_source,
};

/// Stable pure builder contract persisted with each context epoch.
pub const CONTEXT_BUILDER_VERSION: u16 = 1;

/// Stable numeric renderer contract persisted with domain admissions.
pub const CONTEXT_RENDERER_VERSION: u16 = 1;

/// Maximum provider-neutral prelude size shared with provider request validation.
pub const MAX_RENDERED_CONTEXT_BYTES: usize = 256 * 1024;

/// Fixed identity and optimistic state used to construct one provider turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextBuildRequest {
    /// Stable turn identity allocated before construction.
    pub context_turn_id: ContextTurnId,
    /// Frozen epoch identity.
    pub epoch_id: ContextEpochId,
    /// Owning session.
    pub session_id: SessionId,
    /// Exact provider attempt.
    pub attempt_id: AttemptId,
    /// One-based provider call within the attempt.
    pub run_turn: u32,
    /// Session sequence checked by the eventual atomic commit.
    pub expected_session_sequence: SessionSequence,
    /// Memory generation read with the immutable candidate batch.
    pub memory_generation: MemoryGeneration,
    /// Exact selected model snapshot.
    pub model: ModelRef,
    /// Total conservative provider-turn context budget.
    pub token_budget: ContextTokenBudget,
    /// Budget already reserved for safety, input, history, and tool protocol.
    pub reserved_tokens: EstimatedTokens,
    /// Maximum conservative size available to durable memory records.
    pub durable_memory_limit: EstimatedTokens,
    /// Explicit observation time used for all immutable turn metadata.
    pub committed_at: TimestampMillis,
    /// Eligible scope and deterministic ranking time.
    pub retrieval_scope: RetrievalScope,
    /// Complete registered source observations.
    pub observed_sources: Vec<ObservedContextSource>,
    /// Immutable bounded retrieval candidates in arbitrary physical order.
    pub memory_candidates: Vec<MemoryCandidate>,
}

/// Pure deterministic context builder with replaceable rank and sizing policies.
#[derive(Clone, Copy, Debug)]
pub struct ContextBuilder<R = DeterministicRankerV1, S = Utf8ByteSizerV1> {
    ranker: R,
    sizer: S,
}

impl Default for ContextBuilder<DeterministicRankerV1, Utf8ByteSizerV1> {
    fn default() -> Self {
        Self::new(DeterministicRankerV1, Utf8ByteSizerV1)
    }
}

impl<R, S> ContextBuilder<R, S>
where
    R: MemoryRanker,
    S: ContextSizer,
{
    /// Creates a builder with explicit deterministic policies.
    #[must_use]
    pub const fn new(ranker: R, sizer: S) -> Self {
        Self { ranker, sizer }
    }

    /// Observes no external state and constructs an immutable context draft.
    pub fn build(&self, mut request: ContextBuildRequest) -> Result<BuiltContext, MemoryError> {
        validate_request(&request)?;
        request.observed_sources.sort_by(|left, right| {
            left.snapshot()
                .source_key()
                .cmp(right.snapshot().source_key())
        });
        if let Some(duplicate) = request
            .observed_sources
            .windows(2)
            .find(|pair| pair[0].snapshot().source_key() == pair[1].snapshot().source_key())
        {
            return Err(MemoryError::DuplicateSource(
                duplicate[0].snapshot().source_key().clone(),
            ));
        }

        let snapshots: Vec<_> = request
            .observed_sources
            .iter()
            .map(|source| source.snapshot().clone())
            .collect();
        let mut rendered_sources = Vec::new();
        for source in &request.observed_sources {
            if let Some(rendered) = render_source(source, &self.sizer)? {
                rendered_sources.push((source.policy(), rendered));
            }
        }
        rendered_sources.sort_by(|left, right| {
            source_priority(left.0, left.1.section, &left.1.source_key).cmp(&source_priority(
                right.0,
                right.1.section,
                &right.1.source_key,
            ))
        });

        let ranked = self
            .ranker
            .rank(&request.retrieval_scope, request.memory_candidates.clone());
        validate_unique_revisions(&ranked)?;
        let rendered_memories: Vec<_> = ranked
            .into_iter()
            .map(|ranked| {
                let rendered = render_memory(&ranked, &self.sizer)?;
                Ok(RankedRenderedMemory { ranked, rendered })
            })
            .collect::<Result<_, MemoryError>>()?;

        let mut selected_sources = Vec::new();
        let mut selected_memories = Vec::new();
        for (policy, source) in rendered_sources {
            let mut trial_sources = selected_sources.clone();
            trial_sources.push(source.clone());
            if self.fits_total(&request, &trial_sources, &selected_memories)? {
                selected_sources.push(source);
            } else if policy == ContextSourcePolicy::Required {
                return Err(MemoryError::BudgetExceeded);
            }
        }
        for memory in rendered_memories {
            let mut trial_memories = selected_memories.clone();
            trial_memories.push(memory);
            if self.fits_memory_limit(&request, &trial_memories)?
                && self.fits_total(&request, &selected_sources, &trial_memories)?
            {
                selected_memories = trial_memories;
            }
        }

        let selected_memory_renderings = memory_renderings(&selected_memories);
        let prelude = render_context_prelude(&selected_sources, &selected_memory_renderings);
        let rendered_token_count = match prelude.as_deref() {
            Some(value) => self.sizer.estimate(value)?,
            None => EstimatedTokens::new(0).map_err(|_| MemoryError::InvalidDomainValue)?,
        };
        let rendered_hash = rendered_context_hash(prelude.as_deref().unwrap_or(""))?;
        let admissions = build_admissions(
            &request,
            &request.observed_sources,
            &selected_sources,
            &selected_memories,
        )?;

        Ok(BuiltContext {
            request,
            prelude,
            rendered_hash,
            rendered_token_count,
            snapshots,
            admissions,
            selected_sources,
            selected_memories: selected_memory_renderings,
        })
    }

    fn fits_total(
        &self,
        request: &ContextBuildRequest,
        sources: &[RenderedSource],
        memories: &[RankedRenderedMemory],
    ) -> Result<bool, MemoryError> {
        let rendered_memories = memory_renderings(memories);
        let Some(prelude) = render_context_prelude(sources, &rendered_memories) else {
            return Ok(request.reserved_tokens.get() <= request.token_budget.get());
        };
        if prelude.len() > MAX_RENDERED_CONTEXT_BYTES {
            return Ok(false);
        }
        let rendered = self.sizer.estimate(&prelude)?.get();
        let total = request
            .reserved_tokens
            .get()
            .checked_add(rendered)
            .ok_or(MemoryError::NumericOverflow)?;
        Ok(total <= request.token_budget.get())
    }

    fn fits_memory_limit(
        &self,
        request: &ContextBuildRequest,
        memories: &[RankedRenderedMemory],
    ) -> Result<bool, MemoryError> {
        let mut rendered = String::new();
        for memory in memories {
            rendered.push_str(&memory.rendered.rendered);
            rendered.push('\n');
        }
        Ok(self.sizer.estimate(&rendered)?.get() <= request.durable_memory_limit.get())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RankedRenderedMemory {
    ranked: RankedMemory,
    rendered: RenderedMemory,
}

fn memory_renderings(memories: &[RankedRenderedMemory]) -> Vec<RenderedMemory> {
    memories
        .iter()
        .map(|memory| memory.rendered.clone())
        .collect()
}

/// Provider-neutral context that must be request-hashed before durable commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltContext {
    request: ContextBuildRequest,
    prelude: Option<String>,
    rendered_hash: Sha256Digest,
    rendered_token_count: EstimatedTokens,
    snapshots: Vec<ContextSourceSnapshot>,
    admissions: Vec<ContextAdmission>,
    selected_sources: Vec<RenderedSource>,
    selected_memories: Vec<RenderedMemory>,
}

impl BuiltContext {
    /// Returns the exact provider-neutral prelude, if any item was admitted.
    #[must_use]
    pub fn prelude(&self) -> Option<&str> {
        self.prelude.as_deref()
    }

    /// Returns the hash of exact rendered provider-visible bytes.
    #[must_use]
    pub const fn rendered_hash(&self) -> &Sha256Digest {
        &self.rendered_hash
    }

    /// Returns selected registered source records.
    #[must_use]
    pub fn selected_sources(&self) -> &[RenderedSource] {
        &self.selected_sources
    }

    /// Returns selected memory records in admission order.
    #[must_use]
    pub fn selected_memories(&self) -> &[RenderedMemory] {
        &self.selected_memories
    }

    /// Seals the draft with the exact final provider-neutral request hash.
    pub fn seal(self, request_hash: Sha256Digest) -> Result<ContextTurnManifest, MemoryError> {
        let eligibility = ContextEligibility::new(
            self.request.retrieval_scope.user_id.clone(),
            self.request.retrieval_scope.workspace_id.clone(),
            self.request.retrieval_scope.session_id.clone(),
            self.request.retrieval_scope.agent_id.clone(),
            self.request.retrieval_scope.sensitivity_ceiling,
        );
        let budget = ContextBudgetAllocation::new(
            self.request.token_budget,
            self.request.reserved_tokens,
            self.request.durable_memory_limit,
        )
        .map_err(|_| MemoryError::InvalidDomainValue)?;
        let manifest_hash = hash_manifest_material(ManifestHashMaterial {
            context_turn_id: &self.request.context_turn_id,
            epoch_id: &self.request.epoch_id,
            session_id: &self.request.session_id,
            attempt_id: &self.request.attempt_id,
            run_turn: self.request.run_turn,
            expected_session_sequence: self.request.expected_session_sequence,
            memory_generation: self.request.memory_generation,
            model: &self.request.model,
            request_hash: &request_hash,
            rendered_hash: &self.rendered_hash,
            eligibility: &eligibility,
            budget,
            rendered_token_count: self.rendered_token_count,
            committed_at: self.request.committed_at,
            snapshots: &self.snapshots,
            admissions: &self.admissions,
        })?;
        ContextTurnManifest::new(
            self.request.context_turn_id,
            self.request.epoch_id,
            self.request.session_id,
            self.request.attempt_id,
            self.request.run_turn,
            self.request.expected_session_sequence,
            self.request.memory_generation,
            self.request.model,
            request_hash,
            self.rendered_hash,
            manifest_hash,
            eligibility,
            budget,
            self.rendered_token_count,
            self.request.committed_at,
            self.snapshots,
            self.admissions,
        )
        .map_err(|_| MemoryError::InvalidDomainValue)
    }
}

/// Recomputes the canonical hash of persisted manifest material.
pub fn context_manifest_hash(manifest: &ContextTurnManifest) -> Result<Sha256Digest, MemoryError> {
    hash_manifest_material(ManifestHashMaterial {
        context_turn_id: manifest.context_turn_id(),
        epoch_id: manifest.epoch_id(),
        session_id: manifest.session_id(),
        attempt_id: manifest.attempt_id(),
        run_turn: manifest.run_turn(),
        expected_session_sequence: manifest.expected_session_sequence(),
        memory_generation: manifest.memory_generation(),
        model: manifest.model(),
        request_hash: manifest.request_hash(),
        rendered_hash: manifest.rendered_hash(),
        eligibility: manifest.eligibility(),
        budget: manifest.budget(),
        rendered_token_count: manifest.rendered_token_count(),
        committed_at: manifest.committed_at(),
        snapshots: manifest.sources(),
        admissions: manifest.admissions(),
    })
}

/// Verifies persisted manifest integrity without consulting mutable state.
pub fn verify_context_manifest_hash(manifest: &ContextTurnManifest) -> Result<bool, MemoryError> {
    Ok(context_manifest_hash(manifest)? == *manifest.manifest_hash())
}

/// Computes the canonical digest of the complete provider-neutral context prelude.
pub fn rendered_context_hash(rendered: &str) -> Result<Sha256Digest, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.field("context_renderer_v1", rendered.as_bytes())?;
    encoder.finish()
}

/// Verifies exact retained prelude bytes against a durable turn manifest digest.
pub fn verify_rendered_context_hash(
    rendered: &str,
    expected: &Sha256Digest,
) -> Result<bool, MemoryError> {
    Ok(rendered_context_hash(rendered)? == *expected)
}

fn validate_request(request: &ContextBuildRequest) -> Result<(), MemoryError> {
    if request.run_turn == 0 || request.reserved_tokens.get() > request.token_budget.get() {
        return Err(MemoryError::BudgetExceeded);
    }
    Ok(())
}

fn validate_unique_revisions(ranked: &[RankedMemory]) -> Result<(), MemoryError> {
    let mut revisions = BTreeSet::new();
    for memory in ranked {
        if !revisions.insert(memory.candidate.revision_id.clone()) {
            return Err(MemoryError::DuplicateMemoryRevision(
                memory.candidate.revision_id.clone(),
            ));
        }
    }
    Ok(())
}

fn source_priority(
    policy: ContextSourcePolicy,
    section: ContextSection,
    key: &ContextSourceKey,
) -> (u8, u8, ContextSourceKey) {
    let required = if policy == ContextSourcePolicy::Required {
        0
    } else {
        1
    };
    let section = match section {
        ContextSection::AuthorizedInstruction => 0,
        ContextSection::ToolContract => 1,
        ContextSection::ConversationHistory => 2,
        ContextSection::SafetyPolicy | ContextSection::CurrentInstruction => 3,
        ContextSection::DurableMemory => 4,
    };
    (required, section, key.clone())
}

fn build_admissions(
    request: &ContextBuildRequest,
    observed: &[ObservedContextSource],
    sources: &[RenderedSource],
    memories: &[RankedRenderedMemory],
) -> Result<Vec<ContextAdmission>, MemoryError> {
    let mut admissions = Vec::new();
    for source in sources {
        let policy = observed
            .iter()
            .find(|candidate| candidate.snapshot().source_key() == &source.source_key)
            .map(ObservedContextSource::policy)
            .ok_or(MemoryError::InvalidDomainValue)?;
        let mut reasons = Vec::new();
        if policy == ContextSourcePolicy::Required {
            reasons.push(admission_reason(1, ContextAdmissionFactor::Pin, 0)?);
        }
        reasons.push(admission_reason(
            reasons.len() + 1,
            ContextAdmissionFactor::BudgetFit,
            0,
        )?);
        let rank = admissions.len() + 1;
        admissions.push(
            ContextAdmission::new(
                admission_id(&request.context_turn_id, rank)?,
                request.context_turn_id.clone(),
                source.section,
                source.source_key.clone(),
                source.source_revision.clone(),
                None,
                CONTEXT_RENDERER_VERSION,
                source.rendered_hash.clone(),
                u32::try_from(rank).map_err(|_| MemoryError::NumericOverflow)?,
                0,
                source.estimated_tokens,
                request.committed_at,
                reasons,
            )
            .map_err(|_| MemoryError::InvalidDomainValue)?,
        );
    }

    for memory in memories {
        let mut reasons: Vec<_> = memory
            .ranked
            .factors
            .iter()
            .enumerate()
            .map(|(index, factor)| {
                admission_reason(index + 1, rank_factor(factor.reason), factor.contribution)
            })
            .collect::<Result<_, _>>()?;
        reasons.push(admission_reason(
            reasons.len() + 1,
            ContextAdmissionFactor::BudgetFit,
            0,
        )?);
        let rank = admissions.len() + 1;
        admissions.push(
            ContextAdmission::new(
                admission_id(&request.context_turn_id, rank)?,
                request.context_turn_id.clone(),
                ContextSection::DurableMemory,
                memory_source_key(&memory.rendered.revision_id)?,
                memory.ranked.candidate.content_hash.clone(),
                Some(memory.rendered.revision_id.clone()),
                CONTEXT_RENDERER_VERSION,
                memory.rendered.rendered_hash.clone(),
                u32::try_from(rank).map_err(|_| MemoryError::NumericOverflow)?,
                memory.ranked.score,
                memory.rendered.estimated_tokens,
                request.committed_at,
                reasons,
            )
            .map_err(|_| MemoryError::InvalidDomainValue)?,
        );
    }
    Ok(admissions)
}

fn admission_reason(
    ordinal: usize,
    factor: ContextAdmissionFactor,
    contribution: i64,
) -> Result<ContextAdmissionReason, MemoryError> {
    ContextAdmissionReason::new(
        u16::try_from(ordinal).map_err(|_| MemoryError::NumericOverflow)?,
        factor,
        contribution,
    )
    .map_err(|_| MemoryError::InvalidDomainValue)
}

const fn rank_factor(reason: RankReason) -> ContextAdmissionFactor {
    match reason {
        RankReason::SourceAuthority => ContextAdmissionFactor::Authority,
        RankReason::ExactMatch => ContextAdmissionFactor::ExactMatch,
        RankReason::LexicalOverlap => ContextAdmissionFactor::LexicalOverlap,
        RankReason::ScopeSpecificity => ContextAdmissionFactor::ScopeSpecificity,
        RankReason::Freshness => ContextAdmissionFactor::Freshness,
        RankReason::Confidence => ContextAdmissionFactor::Confidence,
    }
}

fn admission_id(
    context_turn_id: &ContextTurnId,
    rank: usize,
) -> Result<ContextAdmissionId, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.field("context_turn_id", context_turn_id.as_str().as_bytes())?;
    encoder.integer(
        "admission_rank",
        u64::try_from(rank).map_err(|_| MemoryError::NumericOverflow)?,
    )?;
    let digest = encoder.finish()?;
    ContextAdmissionId::new(format!("ca:{}", digest.as_str()))
        .map_err(|_| MemoryError::InvalidDomainValue)
}

fn memory_source_key(revision_id: &MemoryRevisionId) -> Result<ContextSourceKey, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.field("memory_revision_id", revision_id.as_str().as_bytes())?;
    let digest = encoder.finish()?;
    ContextSourceKey::new(format!("memory:{}", digest.as_str()))
        .map_err(|_| MemoryError::InvalidDomainValue)
}

struct ManifestHashMaterial<'a> {
    context_turn_id: &'a ContextTurnId,
    epoch_id: &'a ContextEpochId,
    session_id: &'a SessionId,
    attempt_id: &'a AttemptId,
    run_turn: u32,
    expected_session_sequence: SessionSequence,
    memory_generation: MemoryGeneration,
    model: &'a ModelRef,
    request_hash: &'a Sha256Digest,
    rendered_hash: &'a Sha256Digest,
    eligibility: &'a ContextEligibility,
    budget: ContextBudgetAllocation,
    rendered_token_count: EstimatedTokens,
    committed_at: TimestampMillis,
    snapshots: &'a [ContextSourceSnapshot],
    admissions: &'a [ContextAdmission],
}

fn hash_manifest_material(material: ManifestHashMaterial<'_>) -> Result<Sha256Digest, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.integer("manifest_schema", u64::from(CONTEXT_BUILDER_VERSION))?;
    encoder.field(
        "context_turn_id",
        material.context_turn_id.as_str().as_bytes(),
    )?;
    encoder.field("epoch_id", material.epoch_id.as_str().as_bytes())?;
    encoder.field("session_id", material.session_id.as_str().as_bytes())?;
    encoder.field("attempt_id", material.attempt_id.as_str().as_bytes())?;
    encoder.integer("run_turn", u64::from(material.run_turn))?;
    encoder.integer(
        "expected_session_sequence",
        material.expected_session_sequence.get(),
    )?;
    encoder.integer("memory_generation", material.memory_generation.get())?;
    encoder.field(
        "provider_id",
        material.model.provider_id().as_str().as_bytes(),
    )?;
    encoder.field("model_id", material.model.model_id().as_str().as_bytes())?;
    encoder.field("request_hash", material.request_hash.as_str().as_bytes())?;
    encoder.field("rendered_hash", material.rendered_hash.as_str().as_bytes())?;
    encoder.field(
        "eligibility",
        &serde_json::to_vec(material.eligibility).map_err(|_| MemoryError::InvalidDomainValue)?,
    )?;
    encoder.integer("token_budget", material.budget.token_budget().get())?;
    encoder.integer("reserved_tokens", material.budget.reserved_tokens().get())?;
    encoder.integer(
        "durable_memory_limit",
        material.budget.durable_memory_limit().get(),
    )?;
    encoder.integer("rendered_token_count", material.rendered_token_count.get())?;
    encoder.field("committed_at", &material.committed_at.get().to_be_bytes())?;
    encoder.field(
        "sources",
        &serde_json::to_vec(material.snapshots).map_err(|_| MemoryError::InvalidDomainValue)?,
    )?;
    encoder.field(
        "admissions",
        &serde_json::to_vec(material.admissions).map_err(|_| MemoryError::InvalidDomainValue)?,
    )?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        ConfidenceBasisPoints, ContextSection, MemoryContent, MemoryId, MemoryKind,
        MemoryRevisionStatus, MemoryScope, MemoryValidity, ModelId, ProviderId, Sensitivity,
        SessionId, TrustClass, UserId, WorkspaceId,
    };

    use super::*;
    use crate::{
        ContextSource, ContextSourceRead, ContextSourceRegistry, ContextSourceValue,
        normalized_content_hash,
    };

    #[derive(Clone)]
    struct FixedSource {
        key: ContextSourceKey,
        policy: ContextSourcePolicy,
        read: ContextSourceRead,
    }

    impl ContextSource for FixedSource {
        fn key(&self) -> &ContextSourceKey {
            &self.key
        }

        fn policy(&self) -> ContextSourcePolicy {
            self.policy
        }

        fn observe(&self) -> ContextSourceRead {
            self.read.clone()
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
    }

    fn candidate(id: &str) -> MemoryCandidate {
        let content = format!("Remember {id} with Unicode 雪.");
        MemoryCandidate {
            memory_id: MemoryId::new(format!("memory-{id}")).expect("memory ID"),
            revision_id: MemoryRevisionId::new(format!("revision-{id}")).expect("revision ID"),
            status: MemoryRevisionStatus::Active,
            scope: MemoryScope::Workspace(WorkspaceId::new("workspace-1").expect("workspace ID")),
            kind: MemoryKind::Fact,
            trust: TrustClass::UserApproved,
            confidence: ConfidenceBasisPoints::new(8_000).expect("confidence"),
            sensitivity: Sensitivity::Internal,
            validity: MemoryValidity::Indefinite,
            content: MemoryContent::new(content.clone()).expect("content"),
            content_hash: normalized_content_hash(&content).expect("hash"),
            created_at: TimestampMillis::new(10),
            exact_match: false,
            lexical_basis_points: 5_000,
            conflicted: false,
        }
    }

    fn request(memory_candidates: Vec<MemoryCandidate>) -> ContextBuildRequest {
        ContextBuildRequest {
            context_turn_id: ContextTurnId::new("context-turn-1").expect("turn ID"),
            epoch_id: ContextEpochId::new("epoch-1").expect("epoch ID"),
            session_id: SessionId::new("session-1").expect("session ID"),
            attempt_id: AttemptId::new("attempt-1").expect("attempt ID"),
            run_turn: 2,
            expected_session_sequence: SessionSequence::FIRST,
            memory_generation: MemoryGeneration::new(3).expect("generation"),
            model: ModelRef::new(
                ProviderId::new("google-ai-studio").expect("provider ID"),
                ModelId::new("models/gemini-test").expect("model ID"),
            ),
            token_budget: ContextTokenBudget::new(100_000).expect("budget"),
            reserved_tokens: EstimatedTokens::new(1_000).expect("reserved"),
            durable_memory_limit: EstimatedTokens::new(50_000).expect("memory budget"),
            committed_at: TimestampMillis::new(20),
            retrieval_scope: RetrievalScope {
                user_id: UserId::new("user-1").expect("user ID"),
                workspace_id: WorkspaceId::new("workspace-1").expect("workspace ID"),
                session_id: SessionId::new("session-1").expect("session ID"),
                agent_id: None,
                as_of: TimestampMillis::new(20),
                sensitivity_ceiling: Sensitivity::Internal,
            },
            observed_sources: Vec::new(),
            memory_candidates,
        }
    }

    #[test]
    fn shuffled_candidates_produce_byte_identical_context_and_manifest() {
        let builder = ContextBuilder::default();
        let first = builder
            .build(request(vec![candidate("z"), candidate("a")]))
            .expect("build");
        let second = builder
            .build(request(vec![candidate("a"), candidate("z")]))
            .expect("build");

        assert_eq!(first.prelude(), second.prelude());
        assert_eq!(first.rendered_hash(), second.rendered_hash());
        let first = first.seal(digest('f')).expect("seal");
        let second = second.seal(digest('f')).expect("seal");
        assert_eq!(
            serde_json::to_vec(&first).expect("serialize"),
            serde_json::to_vec(&second).expect("serialize")
        );
        assert!(verify_context_manifest_hash(&first).expect("verify"));
        assert_eq!(first.attempt_id().as_str(), "attempt-1");
        assert_eq!(first.run_turn(), 2);
    }

    #[test]
    fn exact_budget_edge_admits_or_skips_a_complete_memory_item() {
        let builder = ContextBuilder::default();
        let mut generous = request(vec![candidate("edge")]);
        generous.reserved_tokens = EstimatedTokens::new(0).expect("reserved");
        let complete = builder.build(generous.clone()).expect("build");
        let exact_bytes = u64::try_from(complete.prelude().expect("prelude").len()).expect("size");

        generous.token_budget = ContextTokenBudget::new(exact_bytes).expect("exact budget");
        let exact = builder.build(generous.clone()).expect("exact build");
        assert_eq!(exact.selected_memories().len(), 1);

        generous.token_budget = ContextTokenBudget::new(exact_bytes - 1).expect("short budget");
        let short = builder.build(generous).expect("short build");
        assert!(short.selected_memories().is_empty());
        assert_eq!(short.prelude(), None);
    }

    #[test]
    fn required_source_cannot_be_silently_dropped_for_budget() {
        let mut registry = ContextSourceRegistry::new();
        registry
            .register(FixedSource {
                key: ContextSourceKey::new("workspace:agents").expect("source key"),
                policy: ContextSourcePolicy::Required,
                read: ContextSourceRead::Available {
                    section: ContextSection::AuthorizedInstruction,
                    source_revision: digest('a'),
                    value: ContextSourceValue::new("required workspace instructions")
                        .expect("value"),
                },
            })
            .expect("register");
        let mut build_request = request(Vec::new());
        build_request.reserved_tokens = EstimatedTokens::new(0).expect("reserved");
        build_request.token_budget = ContextTokenBudget::new(10).expect("budget");
        build_request.observed_sources = registry
            .observe_all(TimestampMillis::new(20), Vec::new())
            .expect("observe");

        assert_eq!(
            ContextBuilder::default().build(build_request),
            Err(MemoryError::BudgetExceeded)
        );
    }

    #[test]
    fn persisted_hash_detects_manifest_tampering() {
        let manifest = ContextBuilder::default()
            .build(request(vec![candidate("one")]))
            .expect("build")
            .seal(digest('f'))
            .expect("seal");
        let mut json = serde_json::to_value(&manifest).expect("serialize");
        json["request_hash"] = serde_json::Value::String("e".repeat(64));
        let tampered: ContextTurnManifest = serde_json::from_value(json).expect("manifest shape");

        assert!(!verify_context_manifest_hash(&tampered).expect("verify"));
    }

    #[test]
    fn retained_context_hash_verification_binds_exact_utf8_bytes() {
        let rendered = "provider context with snow: 雪";
        let hash = rendered_context_hash(rendered).expect("hash");

        assert!(verify_rendered_context_hash(rendered, &hash).expect("verify"));
        assert!(
            !verify_rendered_context_hash("provider context with snow", &hash).expect("verify")
        );
    }

    #[test]
    fn duplicate_revision_candidates_are_rejected() {
        let duplicate = candidate("duplicate");
        let result = ContextBuilder::default().build(request(vec![duplicate.clone(), duplicate]));

        assert_eq!(
            result,
            Err(MemoryError::DuplicateMemoryRevision(
                MemoryRevisionId::new("revision-duplicate").expect("revision ID")
            ))
        );
    }
}
