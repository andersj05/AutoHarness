//! Deterministic application preparation for one durable provider turn.

use std::path::Path;

use autoharness_domain::{
    AgentId, AttemptId, ContextEpochHashes, ContextEpochId, ContextEpochManifest,
    ContextEpochReason, ContextEpochVersions, ContextTokenBudget, ContextTurnId, EstimatedTokens,
    MemoryGeneration, ModelRef, Sensitivity, SessionId, SessionSequence, Sha256Digest,
    TimestampMillis, UserId, WorkspaceId,
};
use autoharness_memory::{
    CONTEXT_BUILDER_VERSION, CONTEXT_RENDERER_VERSION, ContextBuildRequest, ContextBuilder,
    MemoryCandidate, ObservedContextSource, RetrievalScope,
};
use autoharness_provider::{ChatRequest, ContextPrelude, ModelDescriptor};
use autoharness_store::{
    ContextAdmissionContent, ContextTurnCommitRequest, ContextTurnContent, RenderedContextText,
};
use sha2::{Digest, Sha256};

use crate::error::AppError;

const CONTEXT_REGISTRY_VERSION: u16 = 1;
const CONTEXT_RANKER_VERSION: u16 = 1;
const CONTEXT_SIZER_VERSION: u16 = 1;
const CONTEXT_CONFIG_VERSION: &[u8] = b"autoharness-context-config-v1";
const LOCAL_USER_ID: &str = "user:local-v1";
const DEFAULT_AGENT_ID: &str = "agent:default-v1";

/// Stable local scope identities used at one provider-turn boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextScope {
    user_id: UserId,
    workspace_id: WorkspaceId,
    agent_id: AgentId,
}

impl ContextScope {
    /// Resolves local identities around an opaque persisted workspace binding.
    pub fn local(workspace_id: WorkspaceId) -> Self {
        Self {
            user_id: UserId::new(LOCAL_USER_ID).expect("static local user ID is valid"),
            workspace_id,
            agent_id: AgentId::new(DEFAULT_AGENT_ID).expect("static agent ID is valid"),
        }
    }

    /// Returns the opaque local-user identity.
    #[must_use]
    pub const fn user_id(&self) -> &UserId {
        &self.user_id
    }

    /// Returns the opaque canonical-workspace identity.
    #[must_use]
    pub const fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    /// Returns the selected built-in agent identity.
    #[must_use]
    pub const fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    /// Produces the exact deterministic retrieval boundary for one session.
    #[must_use]
    pub fn retrieval_scope(&self, session_id: SessionId, as_of: TimestampMillis) -> RetrievalScope {
        RetrievalScope {
            user_id: self.user_id.clone(),
            workspace_id: self.workspace_id.clone(),
            session_id,
            agent_id: Some(self.agent_id.clone()),
            as_of,
            sensitivity_ceiling: Sensitivity::Internal,
        }
    }
}

/// Hashes a canonical local locator only for the persisted binding lookup.
///
/// The digest is never used as workspace authority. The storage owner maps it
/// to a random opaque [`WorkspaceId`] so distinct workspaces cannot silently
/// share durable memory.
pub fn workspace_locator_digest(workspace: &Path) -> Result<Sha256Digest, AppError> {
    let canonical = workspace.canonicalize().map_err(|_| AppError::FileSystem)?;
    sha256_digest(canonical.to_string_lossy().as_bytes())
}

/// Frozen epoch compatibility inputs derived from provider-neutral state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochCompatibility {
    hashes: ContextEpochHashes,
    token_budget: ContextTokenBudget,
    durable_memory_limit: EstimatedTokens,
}

impl EpochCompatibility {
    /// Computes all epoch hashes without provider credentials or native requests.
    pub fn new(
        request: &ChatRequest,
        descriptor: Option<&ModelDescriptor>,
        retrieval_scope: &RetrievalScope,
        token_budget: ContextTokenBudget,
        durable_memory_limit: EstimatedTokens,
    ) -> Result<Self, AppError> {
        let config_hash = sha256_digest(&serde_json::to_vec(&serde_json::json!({
            "version": String::from_utf8_lossy(CONTEXT_CONFIG_VERSION),
            "user_id": retrieval_scope.user_id.as_str(),
            "workspace_id": retrieval_scope.workspace_id.as_str(),
            "agent_id": retrieval_scope.agent_id.as_ref().map(AgentId::as_str),
            "sensitivity": retrieval_scope.sensitivity_ceiling,
            "token_budget": token_budget.get(),
            "durable_memory_limit": durable_memory_limit.get(),
        }))?)?;
        let catalog_hash = sha256_digest(&serde_json::to_vec(&descriptor.map_or_else(
            || {
                serde_json::json!({
                    "model_id": request.model_id.as_str(),
                    "catalog_state": "descriptor_unavailable",
                })
            },
            |descriptor| {
                serde_json::to_value(descriptor)
                    .expect("provider model descriptors are serializable")
            },
        ))?)?;
        let model_capability_hash = sha256_digest(&serde_json::to_vec(
            &descriptor.map(|descriptor| &descriptor.capabilities),
        )?)?;
        let tool_registry_hash = sha256_digest(&serde_json::to_vec(&request.tools)?)?;
        Ok(Self {
            hashes: ContextEpochHashes::new(
                config_hash,
                catalog_hash,
                model_capability_hash,
                tool_registry_hash,
            ),
            token_budget,
            durable_memory_limit,
        })
    }

    /// Returns the complete mutable-input hash set.
    #[must_use]
    pub const fn hashes(&self) -> &ContextEpochHashes {
        &self.hashes
    }

    /// Returns the total conservative provider-turn budget.
    #[must_use]
    pub const fn token_budget(&self) -> ContextTokenBudget {
        self.token_budget
    }

    /// Returns the durable-memory section limit.
    #[must_use]
    pub const fn durable_memory_limit(&self) -> EstimatedTokens {
        self.durable_memory_limit
    }
}

/// Immutable inputs sampled before constructing a provider turn.
pub struct ContextPreparationInput {
    /// Exact session owning the provider attempt.
    pub session_id: SessionId,
    /// Exact provider attempt.
    pub attempt_id: AttemptId,
    /// One-based provider call within the attempt.
    pub run_turn: u32,
    /// Exact session sequence verified by the context transaction.
    pub expected_session_sequence: SessionSequence,
    /// Immutable generation-bound memory candidate batch.
    pub memory_generation: MemoryGeneration,
    /// Exact selected model snapshot.
    pub model: ModelRef,
    /// Provider-neutral history and tool request before context framing.
    pub request: ChatRequest,
    /// Exact retrieval identities and observation time.
    pub retrieval_scope: RetrievalScope,
    /// Frozen compatibility contract for the attempt epoch.
    pub compatibility: EpochCompatibility,
    /// Existing epoch required for tool-loop continuations.
    pub existing_epoch: Option<ContextEpochManifest>,
    /// Complete registered source observations.
    pub observed_sources: Vec<ObservedContextSource>,
    /// Immutable memory candidates in arbitrary physical order.
    pub memory_candidates: Vec<MemoryCandidate>,
    /// Stable commit time used by every record in this turn.
    pub committed_at: TimestampMillis,
    /// Whether this top-level attempt is an explicit retry.
    pub explicit_retry: bool,
}

/// Provider request paired with the exact durable context commit that authorizes dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedContextTurn {
    request: ChatRequest,
    commit: ContextTurnCommitRequest,
}

impl PreparedContextTurn {
    /// Returns the exact provider-neutral request to dispatch after durable binding.
    #[must_use]
    pub const fn request(&self) -> &ChatRequest {
        &self.request
    }

    /// Returns the exact atomic context-store request that must precede dispatch.
    #[must_use]
    pub const fn commit(&self) -> &ContextTurnCommitRequest {
        &self.commit
    }

    /// Returns the immutable manifest for binding into the session event stream.
    #[must_use]
    pub const fn manifest(&self) -> &autoharness_domain::ContextTurnManifest {
        self.commit.turn()
    }
}

/// Builds and seals one provider turn without observing mutable external state.
pub fn prepare_context_turn(
    input: ContextPreparationInput,
) -> Result<PreparedContextTurn, AppError> {
    if input.run_turn == 0
        || input.model.model_id() != &input.request.model_id
        || input.retrieval_scope.session_id != input.session_id
    {
        return Err(AppError::Configuration);
    }
    let context_turn_id = context_turn_id(&input.attempt_id, input.run_turn);
    let epoch_id = context_epoch_id(&input.attempt_id);
    let reserved_tokens = estimated_request_bytes(&input.request)?;
    let builder = ContextBuilder::default();
    let built = builder.build(ContextBuildRequest {
        context_turn_id,
        epoch_id: epoch_id.clone(),
        session_id: input.session_id.clone(),
        attempt_id: input.attempt_id.clone(),
        run_turn: input.run_turn,
        expected_session_sequence: input.expected_session_sequence,
        memory_generation: input.memory_generation,
        model: input.model.clone(),
        token_budget: input.compatibility.token_budget(),
        reserved_tokens,
        durable_memory_limit: input.compatibility.durable_memory_limit(),
        committed_at: input.committed_at,
        retrieval_scope: input.retrieval_scope.clone(),
        observed_sources: input.observed_sources.clone(),
        memory_candidates: input.memory_candidates.clone(),
    })?;

    let prelude = built.prelude().map(str::to_owned);
    let mut rendered_items = built
        .selected_sources()
        .iter()
        .map(|source| source.rendered.clone())
        .collect::<Vec<_>>();
    rendered_items.extend(
        built
            .selected_memories()
            .iter()
            .map(|memory| memory.rendered.clone()),
    );
    let request = match prelude.as_deref() {
        Some(prelude) => input
            .request
            .clone()
            .with_context(ContextPrelude::new(prelude.to_owned())?),
        None => input.request.clone(),
    };
    let request_hash = provider_request_hash(&request)?;
    let manifest = built.seal(request_hash)?;
    let contents = manifest
        .admissions()
        .iter()
        .zip(rendered_items)
        .map(|(admission, rendered)| {
            Ok(ContextAdmissionContent::new(
                admission.admission_id().clone(),
                RenderedContextText::new(rendered)?,
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    if contents.len() != manifest.admissions().len() {
        return Err(AppError::Configuration);
    }
    let content =
        ContextTurnContent::new(prelude.map(RenderedContextText::new).transpose()?, contents);
    let epoch = prepare_epoch(&input, &manifest, epoch_id)?;
    Ok(PreparedContextTurn {
        request,
        commit: ContextTurnCommitRequest::new(epoch, manifest, content),
    })
}

fn prepare_epoch(
    input: &ContextPreparationInput,
    manifest: &autoharness_domain::ContextTurnManifest,
    epoch_id: ContextEpochId,
) -> Result<Option<ContextEpochManifest>, AppError> {
    let versions = context_versions()?;
    if input.run_turn > 1 {
        let existing = input
            .existing_epoch
            .as_ref()
            .ok_or(AppError::Configuration)?;
        if existing.epoch_id() != &epoch_id
            || existing.session_id() != &input.session_id
            || existing.memory_generation() != input.memory_generation
            || existing.versions() != versions
            || existing.hashes() != input.compatibility.hashes()
            || existing.token_budget() != input.compatibility.token_budget()
        {
            return Err(AppError::Configuration);
        }
        return Ok(None);
    }
    if input.existing_epoch.is_some() {
        return Err(AppError::Configuration);
    }
    let baseline_hash = baseline_hash(manifest, input.compatibility.hashes())?;
    let reason = if input.explicit_retry {
        ContextEpochReason::ExplicitRetry
    } else {
        ContextEpochReason::NewAttempt
    };
    ContextEpochManifest::new(
        epoch_id,
        input.session_id.clone(),
        input.memory_generation,
        reason,
        None,
        baseline_hash,
        versions,
        input.compatibility.hashes().clone(),
        input.compatibility.token_budget(),
        input.committed_at,
    )
    .map(Some)
    .map_err(|_| AppError::Configuration)
}

fn context_versions() -> Result<ContextEpochVersions, AppError> {
    ContextEpochVersions::new(
        CONTEXT_BUILDER_VERSION,
        CONTEXT_REGISTRY_VERSION,
        CONTEXT_RANKER_VERSION,
        CONTEXT_RENDERER_VERSION,
        CONTEXT_SIZER_VERSION,
    )
    .map_err(|_| AppError::Configuration)
}

fn estimated_request_bytes(request: &ChatRequest) -> Result<EstimatedTokens, AppError> {
    let bytes = serde_json::to_vec(request)?.len();
    EstimatedTokens::new(u64::try_from(bytes).map_err(|_| AppError::Configuration)?)
        .map_err(|_| AppError::Configuration)
}

fn provider_request_hash(request: &ChatRequest) -> Result<Sha256Digest, AppError> {
    sha256_digest(&serde_json::to_vec(request)?)
}

fn baseline_hash(
    manifest: &autoharness_domain::ContextTurnManifest,
    hashes: &ContextEpochHashes,
) -> Result<Sha256Digest, AppError> {
    sha256_digest(&serde_json::to_vec(&serde_json::json!({
        "attempt_id": manifest.attempt_id().as_str(),
        "epoch_id": manifest.epoch_id().as_str(),
        "memory_generation": manifest.memory_generation().get(),
        "rendered_hash": manifest.rendered_hash().as_str(),
        "config_hash": hashes.config_hash().as_str(),
        "catalog_hash": hashes.catalog_hash().as_str(),
        "model_capability_hash": hashes.model_capability_hash().as_str(),
        "tool_registry_hash": hashes.tool_registry_hash().as_str(),
    }))?)
}

/// Derives the stable epoch identity owned by one exact provider attempt.
#[must_use]
pub fn context_epoch_id(attempt_id: &AttemptId) -> ContextEpochId {
    let digest = digest_fields(&[
        b"autoharness-context-epoch-v1",
        attempt_id.as_str().as_bytes(),
    ]);
    ContextEpochId::new(format!("context-epoch:{digest}"))
        .expect("SHA-256 context epoch IDs are valid")
}

fn context_turn_id(attempt_id: &AttemptId, run_turn: u32) -> ContextTurnId {
    let turn = run_turn.to_be_bytes();
    let digest = digest_fields(&[
        b"autoharness-context-turn-v1",
        attempt_id.as_str().as_bytes(),
        &turn,
    ]);
    ContextTurnId::new(format!("context-turn:{digest}"))
        .expect("SHA-256 context turn IDs are valid")
}

fn sha256_digest(bytes: &[u8]) -> Result<Sha256Digest, AppError> {
    Sha256Digest::new(hex_digest(bytes)).map_err(|_| AppError::Configuration)
}

fn digest_fields(fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
        hasher.update(field);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        ConfidenceBasisPoints, ContextTokenBudget, MemoryContent, MemoryId, MemoryKind,
        MemoryRevisionId, MemoryRevisionStatus, MemoryScope, MemoryValidity, ModelId, ProviderId,
        Sensitivity, TrustClass,
    };
    use autoharness_memory::{MemoryCandidate, normalized_content_hash};
    use autoharness_provider::{ChatContent, ChatMessage, ChatRole};

    use super::*;

    fn scope() -> ContextScope {
        ContextScope::local(WorkspaceId::new("workspace-fixture").expect("workspace ID"))
    }

    fn model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("fixture-provider").expect("provider ID"),
            ModelId::new("fixture-model").expect("model ID"),
        )
    }

    fn request() -> ChatRequest {
        ChatRequest::new(
            model().model_id().clone(),
            vec![ChatMessage::text(
                ChatRole::User,
                ChatContent::new("Remember the interface preference").expect("content"),
            )],
        )
        .expect("request")
    }

    fn memory(scope: &ContextScope) -> MemoryCandidate {
        let content = MemoryContent::new("Prefer compact status updates.").expect("content");
        MemoryCandidate {
            memory_id: MemoryId::new("memory-1").expect("memory ID"),
            revision_id: MemoryRevisionId::new("revision-1").expect("revision ID"),
            status: MemoryRevisionStatus::Active,
            scope: MemoryScope::Workspace(scope.workspace_id().clone()),
            kind: MemoryKind::Preference,
            trust: TrustClass::UserApproved,
            confidence: ConfidenceBasisPoints::new(9_000).expect("confidence"),
            sensitivity: Sensitivity::Internal,
            validity: MemoryValidity::Indefinite,
            content_hash: normalized_content_hash(content.as_str()).expect("hash"),
            content,
            created_at: TimestampMillis::new(1),
            exact_match: true,
            lexical_basis_points: 10_000,
            conflicted: false,
        }
    }

    fn preparation(
        scope: &ContextScope,
        memory_candidates: Vec<MemoryCandidate>,
    ) -> ContextPreparationInput {
        let request = request();
        let retrieval_scope = scope.retrieval_scope(
            SessionId::new("session-1").expect("session ID"),
            TimestampMillis::new(10),
        );
        let compatibility = EpochCompatibility::new(
            &request,
            None,
            &retrieval_scope,
            ContextTokenBudget::new(16_384).expect("budget"),
            EstimatedTokens::new(4_096).expect("memory limit"),
        )
        .expect("compatibility");
        ContextPreparationInput {
            session_id: retrieval_scope.session_id.clone(),
            attempt_id: AttemptId::new("attempt-1").expect("attempt ID"),
            run_turn: 1,
            expected_session_sequence: SessionSequence::new(4).expect("session sequence"),
            memory_generation: MemoryGeneration::new(1).expect("generation"),
            model: model(),
            request,
            retrieval_scope,
            compatibility,
            existing_epoch: None,
            observed_sources: Vec::new(),
            memory_candidates,
            committed_at: TimestampMillis::new(10),
            explicit_retry: false,
        }
    }

    #[test]
    fn locator_digest_is_only_a_distinct_binding_lookup_key() {
        let first = tempfile::tempdir().expect("first workspace");
        let second = tempfile::tempdir().expect("second workspace");

        assert_ne!(
            workspace_locator_digest(first.path()).expect("first digest"),
            workspace_locator_digest(second.path()).expect("second digest")
        );
        let scope = scope();
        assert!(!scope.workspace_id().as_str().contains("AutoHarness"));
    }

    #[test]
    fn memory_is_framed_as_context_and_every_rendered_byte_has_a_sidecar() {
        let scope = scope();
        let prepared =
            prepare_context_turn(preparation(&scope, vec![memory(&scope)])).expect("prepare turn");

        let prelude = prepared
            .request()
            .context
            .as_ref()
            .expect("context prelude")
            .as_str();
        assert!(prelude.contains("<autoharness-memory-data-v1>"));
        assert!(!prepared.request().messages.iter().any(|message| {
            message
                .content()
                .is_some_and(|content| content.as_str().contains("compact status"))
        }));
        assert_eq!(prepared.manifest().run_turn(), 1);
        assert_eq!(prepared.manifest().admissions().len(), 1);
        assert!(prepared.commit.content().prelude().is_some());
        assert_eq!(prepared.commit.content().admissions().len(), 1);
    }

    #[test]
    fn shuffled_candidates_produce_the_same_request_and_manifest() {
        let scope = scope();
        let mut second = memory(&scope);
        second.memory_id = MemoryId::new("memory-2").expect("memory ID");
        second.revision_id = MemoryRevisionId::new("revision-2").expect("revision ID");
        second.content = MemoryContent::new("Prefer keyboard-first navigation.").expect("content");
        second.content_hash = normalized_content_hash(second.content.as_str()).expect("hash");
        let first_order =
            prepare_context_turn(preparation(&scope, vec![memory(&scope), second.clone()]))
                .expect("first order");
        let second_order = prepare_context_turn(preparation(&scope, vec![second, memory(&scope)]))
            .expect("second order");

        assert_eq!(first_order, second_order);
    }

    #[test]
    fn empty_retrieval_still_produces_an_auditable_turn_without_a_prelude() {
        let scope = scope();
        let prepared = prepare_context_turn(preparation(&scope, Vec::new())).expect("prepare");

        assert!(prepared.request().context.is_none());
        assert!(prepared.manifest().admissions().is_empty());
        assert!(prepared.commit.content().prelude().is_none());
    }

    #[test]
    fn continuation_requires_the_exact_frozen_epoch_generation() {
        let scope = scope();
        let first = prepare_context_turn(preparation(&scope, Vec::new())).expect("first turn");
        let epoch = first.commit.epoch().expect("epoch").clone();
        let mut continuation = preparation(&scope, Vec::new());
        continuation.run_turn = 2;
        continuation.existing_epoch = Some(epoch);
        continuation.memory_generation = MemoryGeneration::new(2).expect("generation");

        assert!(prepare_context_turn(continuation).is_err());
    }
}
