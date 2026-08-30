use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Arc;

use autoharness_domain::{
    AttemptFailure, AttemptId, ClassifiedError, CommandId, CommandPayload, ConfidenceBasisPoints,
    ContextEpochId, ContextTokenBudget, CorrelationId, DeliveryMode, ErrorClass, ErrorCode,
    EstimatedTokens, EventPayload, MemoryCommandEnvelope, MemoryCommandPayload, MemoryContent,
    MemoryEvidence, MemoryEvidenceId, MemoryEvidenceRelation, MemoryEvidenceSource, MemoryId,
    MemoryKind, MemoryOrigin, MemoryRejectionReason, MemoryRelationKind, MemoryRevision,
    MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionNumber, MemoryRevisionStatus,
    MemoryScope as DomainMemoryScope, MemorySequence, MemoryValidity, PermissionAnswer,
    PermissionOutcome, PromptText, PublicMessage, ResponseText, RetryAdvice, RunLimits,
    Sensitivity, SessionId, SessionSequence, SessionTitle, Sha256Digest, TimestampMillis,
    ToolCallId, ToolOutput, TrustClass, UsageSnapshot as DomainUsage,
};
use autoharness_engine::{
    AttemptStatus as EngineAttemptStatus, DurableEngineError, SessionAggregate,
};
use autoharness_memory::{
    MAX_CONTEXT_SOURCE_VALUE_BYTES, MemoryCandidate, RetainedContextSource,
    normalized_content_hash, verify_admission_rendered_hash, verify_rendered_context_hash,
};
use autoharness_provider::{
    CancellationToken, CatalogFreshness, CatalogRequest, ChatContent, ChatMessage, ChatRequest,
    ChatRole, ContextPrelude, ModelCatalog, ModelDescriptor, Provider, ProviderError,
    ProviderErrorKind, ProviderStreamEvent, ProviderToolCall, ProviderToolDefinition,
    SecretAccumulator,
};
use autoharness_provider_codex_cli::{CodexAuthProgress, login_with_browser};
#[cfg(test)]
use autoharness_provider_gemini::{GeminiApiKey, GeminiProvider};
use autoharness_settings::{DisplayLabel, ProfileId, ProviderKind, ProviderProfile};
use autoharness_store::{
    ContextAdmissionContent, ContextCompactionBoundary, ContextCompactionCheckpoint,
    ContextTurnContent, MemoryAdmissionKey, MemoryAdmissionQuery, MemoryContentState,
    MemoryInspectionCursor, MemoryInspectionQuery, MemoryInspectionStatus, MemorySearchQuery,
    SessionStatus,
};
use autoharness_tool::{
    IncomingToolCall, MemoryProposal, RunBudget, ToolError, ToolRuntime, definitions, plan, replan,
};
use autoharness_tui::{
    ApiCredential, AppPorts, AttemptKey, CatalogProjection, CredentialSourceLabel,
    LocalPreferenceChange, LocalUserProfileProjection, MEMORY_VIEW_PAGE_SIZE, MemoryPageDirection,
    MemoryProjection, MemoryScopeFilter, MemoryStatusFilter, MemoryViewCursor, MemoryViewQuery,
    ProfileConnectionState, ProfileCredentialStateLabel, ProfilesProjection, ProviderKindLabel,
    ProviderProfileDraft, ProviderProfileProjection, RequestId, RetryPolicy, SessionBrowserEntry,
    SessionsProjection, SettingsProjection, ToolCallKey, UiFailure, UiIntent, UiNotice,
};
use futures_util::StreamExt as _;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::context_runtime::{
    CompactedHistoryGroup, CompactedHistoryV1, ContextEpochMode, ContextPreparationInput,
    ContextScope, EpochCompatibility, FrozenContinuationInput, PreparedContextTurn,
    compact_history, compaction_epoch_id, is_compacted_history_admission,
    is_workspace_agents_admission, observe_compacted_history, observe_workspace_agents,
    prepare_context_turn, prepare_frozen_continuation, retained_compacted_history,
    retained_workspace_agents, workspace_locator_digest,
};
use crate::engine_actor::EngineHandle;
use crate::error::AppError;
use crate::import_runtime::{ImportDocumentError, build_workspace_document_import};
use crate::{ids, projection, telemetry};
use autoharness_app::profiles::{
    ProfileManagementError, ProfileManager, ProfileStoreError, StoredCredentialState,
};
use autoharness_app::vault::VaultError;

const PROVIDER_MESSAGE_CAPACITY: usize = 128;
const DEFAULT_CONTEXT_TOKEN_BUDGET: u64 = 65_536;
const CONTEXT_SIZER_BYTES_PER_TOKEN: u64 = 4;
const MEMORY_CONTEXT_CANDIDATE_LIMIT: u32 = 64;
const CONTEXT_SNAPSHOT_ATTEMPTS: usize = 3;

pub(crate) type ProviderFactory =
    Arc<dyn Fn(ApiCredential) -> Result<Arc<dyn Provider>, ProviderError> + Send + Sync + 'static>;

pub(crate) struct ProviderComposition {
    pub(crate) initial: Option<Arc<dyn Provider>>,
    pub(crate) factory: ProviderFactory,
}
pub(crate) type ProfileProviderFactory = Arc<
    dyn Fn(
            &ProfileId,
            &ProviderProfile,
            Zeroizing<String>,
        ) -> Result<Arc<dyn Provider>, ProviderError>
        + Send
        + Sync
        + 'static,
>;

/// Environment credentials retained only in zeroizing process memory.
pub(crate) struct EnvironmentCredentials {
    pub(crate) gemini: Option<Zeroizing<String>>,
    pub(crate) router: Option<Zeroizing<String>>,
}

impl EnvironmentCredentials {
    fn credential(&self, kind: ProviderKind) -> Option<Zeroizing<String>> {
        match kind {
            ProviderKind::Gemini => self.gemini.clone(),
            ProviderKind::Router => self.router.clone(),
            ProviderKind::CodexCli => None,
        }
    }

    fn has(&self, kind: ProviderKind) -> bool {
        match kind {
            ProviderKind::Gemini => self.gemini.is_some(),
            ProviderKind::Router => self.router.is_some(),
            ProviderKind::CodexCli => false,
        }
    }
}

pub(crate) struct ProfileRuntime {
    pub(crate) manager: Arc<ProfileManager>,
    pub(crate) factory: ProfileProviderFactory,
    pub(crate) environment: EnvironmentCredentials,
    pub(crate) workspace: String,
    pub(crate) git_branch: Option<String>,
    connection: BTreeMap<String, ProfileConnectionState>,
}

impl ProfileRuntime {
    pub(crate) fn new(
        manager: Arc<ProfileManager>,
        factory: ProfileProviderFactory,
        environment: EnvironmentCredentials,
        workspace: String,
    ) -> Self {
        let git_branch = workspace_git_branch(&workspace);
        Self {
            manager,
            factory,
            environment,
            workspace,
            git_branch,
            connection: BTreeMap::new(),
        }
    }
}
fn workspace_git_branch(workspace: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?;
    let branch = branch.trim();
    (!branch.is_empty()
        && branch.chars().count() <= 128
        && branch.chars().all(|character| !character.is_control()))
    .then(|| branch.to_owned())
}
pub(crate) struct RuntimeComposition {
    pub(crate) provider: ProviderComposition,
    pub(crate) profiles: Option<ProfileRuntime>,
    pub(crate) tool_runtime: Arc<ToolRuntime>,
    pub(crate) artifact_root: Option<std::path::PathBuf>,
}

#[cfg(test)]
fn gemini_provider(credential: ApiCredential) -> Result<Arc<dyn Provider>, ProviderError> {
    let api_key = GeminiApiKey::new(credential.into_string())?;
    Ok(Arc::new(GeminiProvider::new(api_key)?))
}

enum AsyncMessage {
    Catalog {
        generation: u64,
        request_id: Option<RequestId>,
        result: Result<ModelCatalog, ProviderError>,
    },
    Stream {
        attempt_id: AttemptId,
        result: Result<ProviderStreamEvent, ProviderError>,
        benchmark_chunk_sequence: Option<u64>,
    },
    Tool {
        tool_call_id: ToolCallId,
        result: Result<autoharness_domain::ToolOutput, ToolError>,
    },
    ProfileTest {
        profile_id: String,
        request_id: RequestId,
        result: Result<ModelCatalog, ProviderError>,
    },
    CodexLoginBrowserOpened {
        request_id: RequestId,
    },
    CodexLoginFinished {
        request_id: RequestId,
        result: Result<Zeroizing<String>, ProviderError>,
    },
}

struct ActiveAttempt {
    attempt_id: AttemptId,
    cancellation: CancellationToken,
    budget: RunBudget,
    usage_base: DomainUsage,
    credential_ingress: Option<CredentialIngressGuard>,
}

struct CredentialIngressGuard {
    credentials: Vec<Zeroizing<String>>,
    all_provider_values: SecretAccumulator,
    provider_text: SecretAccumulator,
    tool_values: SecretAccumulator,
}

impl CredentialIngressGuard {
    fn new(credentials: Vec<Zeroizing<String>>) -> Self {
        Self {
            credentials,
            all_provider_values: SecretAccumulator::new(),
            provider_text: SecretAccumulator::new(),
            tool_values: SecretAccumulator::new(),
        }
    }

    fn observe_provider_text(&mut self, value: &str) -> bool {
        let credentials = self
            .credentials
            .iter()
            .map(|credential| credential.as_str())
            .collect::<Vec<_>>();
        self.all_provider_values.observe_text(value, &credentials)
            || self.provider_text.observe_text(value, &credentials)
    }

    fn observe_tool_value(&mut self, value: &str) -> bool {
        let credentials = self
            .credentials
            .iter()
            .map(|credential| credential.as_str())
            .collect::<Vec<_>>();
        self.all_provider_values.observe_text(value, &credentials)
            || self.tool_values.observe_text(value, &credentials)
    }

    fn observe_tool_arguments(&mut self, value: &serde_json::Value) -> bool {
        if json_value_contains_exact_credential(value, &self.credentials) {
            return true;
        }
        let credentials = self
            .credentials
            .iter()
            .map(|credential| credential.as_str())
            .collect::<Vec<_>>();
        self.all_provider_values
            .observe_structured(value, &credentials)
            || self.tool_values.observe_structured(value, &credentials)
    }

    fn observe_tool_call(
        &mut self,
        provider_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> bool {
        self.observe_tool_value(provider_call_id)
            || self.observe_tool_value(tool_name)
            || self.observe_tool_arguments(arguments)
    }

    fn contains_tool_output(&self, value: &str) -> bool {
        value_contains_exact_credential(value, &self.credentials)
    }
}

enum StartAttemptError {
    Engine(DurableEngineError),
    Provider(ProviderError),
    Context(AppError),
}

#[derive(Debug)]
enum MemoryProposalPersistenceError {
    Safe(ToolError),
    Ambiguous(AppError),
}

struct FrozenContextBaseline {
    epoch: ContextEpochMode,
    turn: autoharness_domain::ContextTurnManifest,
    content: ContextTurnContent,
}

impl FrozenContextBaseline {
    fn epoch(&self) -> &autoharness_domain::ContextEpochManifest {
        match &self.epoch {
            ContextEpochMode::Existing(epoch) => epoch,
            ContextEpochMode::NewAttempt { .. } | ContextEpochMode::Compaction { .. } => {
                unreachable!("a frozen baseline always names an existing epoch")
            }
        }
    }
}

struct PreparedContextSnapshot {
    turn: PreparedContextTurn,
    compaction_boundary: Option<ContextCompactionBoundary>,
}

struct CompactionCandidate {
    history: CompactedHistoryV1,
    request: ChatRequest,
    predecessor_epoch_id: ContextEpochId,
    epoch_id: ContextEpochId,
}

#[derive(Clone)]
struct MemoryViewState {
    generation: u64,
    query: MemoryViewQuery,
}

/// Owns application orchestration while the terminal runner owns UI state.
pub struct Coordinator {
    session_id: SessionId,
    session: SessionAggregate,
    engine: EngineHandle,
    provider: Option<Arc<dyn Provider>>,
    session_credential_connected: bool,
    provider_factory: ProviderFactory,
    profiles: Option<ProfileRuntime>,
    tool_runtime: Arc<ToolRuntime>,
    artifact_root: Option<std::path::PathBuf>,
    ports: AppPorts,
    messages: mpsc::Sender<AsyncMessage>,
    message_rx: mpsc::Receiver<AsyncMessage>,
    shutdown: CancellationToken,
    active: Option<ActiveAttempt>,
    catalog_models: Vec<ModelDescriptor>,
    catalog_generation: u64,
    catalog_cancellation: Option<CancellationToken>,
    codex_login: Option<(RequestId, CancellationToken)>,
    context_scope: Option<ContextScope>,
    workspace: std::path::PathBuf,
    memory_view: Option<MemoryViewState>,
}

impl Coordinator {
    /// Creates application composition around replayed state and bounded ports.
    #[must_use]
    #[cfg(test)]
    pub fn new(
        session_id: SessionId,
        session: SessionAggregate,
        engine: EngineHandle,
        provider: Option<Arc<dyn Provider>>,
        ports: AppPorts,
        shutdown: CancellationToken,
    ) -> Self {
        Self::with_provider_factory(
            session_id,
            session,
            engine,
            ProviderComposition {
                initial: provider,
                factory: Arc::new(gemini_provider),
            },
            test_tool_runtime(),
            ports,
            shutdown,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_provider_factory(
        session_id: SessionId,
        session: SessionAggregate,
        engine: EngineHandle,
        provider: ProviderComposition,
        tool_runtime: Arc<ToolRuntime>,
        ports: AppPorts,
        shutdown: CancellationToken,
    ) -> Self {
        Self::with_runtime(
            session_id,
            session,
            engine,
            RuntimeComposition {
                provider,
                profiles: None,
                tool_runtime,
                artifact_root: None,
            },
            ports,
            shutdown,
        )
    }

    pub(crate) fn with_runtime(
        session_id: SessionId,
        session: SessionAggregate,
        engine: EngineHandle,
        runtime: RuntimeComposition,
        ports: AppPorts,
        shutdown: CancellationToken,
    ) -> Self {
        let (messages, message_rx) = mpsc::channel(PROVIDER_MESSAGE_CAPACITY);
        let active = recover_active_attempt(&session, &shutdown);
        let workspace = runtime.profiles.as_ref().map_or_else(
            || std::path::PathBuf::from("."),
            |profiles| std::path::PathBuf::from(&profiles.workspace),
        );
        Self {
            session_id,
            session,
            engine,
            provider: runtime.provider.initial,
            session_credential_connected: false,
            provider_factory: runtime.provider.factory,
            profiles: runtime.profiles,
            tool_runtime: runtime.tool_runtime,
            artifact_root: runtime.artifact_root,
            ports,
            messages,
            message_rx,
            shutdown,
            active,
            catalog_models: Vec::new(),
            catalog_generation: 0,
            catalog_cancellation: None,
            codex_login: None,
            context_scope: None,
            workspace,
            memory_view: None,
        }
    }

    /// Runs until terminal shutdown or application-channel closure.
    pub async fn run(mut self) -> Result<(), AppError> {
        self.initialize_context_scope().await?;
        self.publish_sessions().await?;
        self.publish_memories().await?;
        if let Some(profiles) = &self.profiles {
            let _ = profiles.manager.recover_pending();
        }
        self.publish_profiles();
        if self.provider.is_some() {
            self.refresh_catalog(None);
            self.maybe_resume_after_tools().await?;
        }

        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    if let Some(active) = &self.active {
                        active.cancellation.cancel();
                    }
                    if let Some(cancellation) = &self.catalog_cancellation {
                        cancellation.cancel();
                    }
                    if let Some((_, cancellation)) = &self.codex_login {
                        cancellation.cancel();
                    }
                    return Ok(());
                }
                intent = self.ports.intents.recv() => {
                    let Some(intent) = intent else {
                        return Ok(());
                    };
                    self.handle_intent(intent).await?;
                }
                message = self.message_rx.recv() => {
                    let Some(message) = message else {
                        return Err(AppError::WorkerStopped);
                    };
                    self.handle_async(message).await?;
                }
            }
        }
    }

    async fn initialize_context_scope(&mut self) -> Result<(), AppError> {
        let locator = workspace_locator_digest(&self.workspace)?;
        let workspace_id = self.engine.resolve_workspace_id(locator).await?;
        self.context_scope = Some(ContextScope::local(workspace_id));
        Ok(())
    }

    fn context_scope(&self) -> Result<&ContextScope, AppError> {
        self.context_scope.as_ref().ok_or(AppError::Configuration)
    }

    async fn handle_intent(&mut self, intent: UiIntent) -> Result<(), AppError> {
        match intent {
            UiIntent::CreateSession { request_id } => {
                self.create_session(request_id).await?;
            }
            UiIntent::ConfigureCredential {
                request_id,
                credential,
            } => {
                self.configure_credential(request_id, credential).await?;
            }
            UiIntent::StartCodexLogin { request_id } => {
                self.start_codex_login(request_id).await?;
            }
            UiIntent::CancelCodexLogin { request_id } => {
                self.cancel_codex_login(request_id).await?;
            }
            UiIntent::UpsertProfile {
                request_id,
                profile,
            } => {
                self.upsert_profile(request_id, profile).await?;
            }
            UiIntent::DuplicateProfile {
                request_id,
                source,
                destination,
            } => {
                self.duplicate_profile(request_id, source, destination)
                    .await?;
            }
            UiIntent::ActivateProfile {
                request_id,
                profile_id,
            } => {
                self.activate_profile(request_id, profile_id).await?;
            }
            UiIntent::SaveProfileCredential {
                request_id,
                profile_id,
                credential,
            } => {
                self.save_profile_credential(request_id, profile_id, credential, false)
                    .await?;
            }
            UiIntent::ReplaceProfileCredential {
                request_id,
                profile_id,
                credential,
            } => {
                self.save_profile_credential(request_id, profile_id, credential, true)
                    .await?;
            }
            UiIntent::TestProfile {
                request_id,
                profile_id,
            } => {
                self.test_profile(request_id, profile_id).await?;
            }
            UiIntent::SetProfileDefaultModel {
                request_id,
                profile_id,
            } => {
                self.set_profile_default_model(request_id, profile_id)
                    .await?;
            }
            UiIntent::SetProfileDefault {
                request_id,
                profile_id,
                model,
                reasoning_effort,
            } => {
                self.set_profile_default(request_id, profile_id, model, reasoning_effort)
                    .await?;
            }
            UiIntent::DisconnectProfile {
                request_id,
                profile_id,
            } => {
                self.disconnect_profile(request_id, profile_id).await?;
            }
            UiIntent::DeleteProfile {
                request_id,
                profile_id,
            } => {
                self.delete_profile(request_id, profile_id).await?;
            }
            UiIntent::UpdateLocalPreference { request_id, change } => {
                self.update_local_preference(request_id, change).await?;
            }
            UiIntent::RefreshCatalog { request_id } => {
                if self.provider.is_some() {
                    self.ports
                        .catalogs
                        .send_replace(Arc::new(CatalogProjection::Loading));
                    self.refresh_catalog(Some(request_id));
                } else {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Authentication,
                            "A provider API key is not configured",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                }
            }
            UiIntent::SelectModel { request_id, model } => {
                if !self.model_is_available(&model) {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Validation,
                            "The selected model is not in the current compatible catalog",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                    return Ok(());
                }
                match self
                    .execute(CommandPayload::SelectModel {
                        session_id: self.session_id.clone(),
                        model,
                    })
                    .await
                {
                    Ok(()) => self.commit(request_id).await?,
                    Err(error) => self.reject(request_id, engine_failure(&error)).await?,
                }
            }
            UiIntent::SubmitPrompt { request_id, prompt } => {
                self.submit_prompt(request_id, prompt).await?;
            }
            UiIntent::CancelAttempt {
                request_id,
                attempt_id,
            } => {
                self.cancel_attempt(request_id, attempt_id).await?;
            }
            UiIntent::RetryAttempt {
                request_id,
                attempt_id,
            } => {
                self.retry_attempt(request_id, attempt_id).await?;
            }
            UiIntent::AnswerPermission {
                request_id,
                tool_call_id,
                allow,
            } => {
                self.answer_permission(request_id, tool_call_id, allow)
                    .await?;
            }
            UiIntent::OpenSession {
                request_id,
                session_id,
            } => {
                self.open_session(request_id, session_id).await?;
            }
            UiIntent::RenameSession {
                request_id,
                session_id,
                title,
            } => {
                self.rename_session(request_id, session_id, title).await?;
            }
            UiIntent::ArchiveSession {
                request_id,
                session_id,
            } => {
                self.archive_session(request_id, session_id, true).await?;
            }
            UiIntent::UnarchiveSession {
                request_id,
                session_id,
            } => {
                self.archive_session(request_id, session_id, false).await?;
            }
            UiIntent::DeleteSession {
                request_id,
                session_id,
            } => {
                self.delete_session(request_id, session_id).await?;
            }
            UiIntent::ExportTranscript {
                request_id,
                session_id,
            } => {
                self.export_transcript(request_id, session_id).await?;
            }
            UiIntent::QueryMemory {
                request_id,
                view_generation,
                query,
            } => {
                self.query_memories(request_id, view_generation, query)
                    .await?;
            }
            UiIntent::RememberMemory {
                request_id,
                content,
            } => {
                self.remember_memory(request_id, content.into_string())
                    .await?;
            }
            UiIntent::ImportMemory { request_id, path } => {
                self.import_memory(request_id, path.into_string()).await?;
            }
            UiIntent::ReviseMemory {
                request_id,
                memory_id,
                expected_last_sequence,
                content,
            } => {
                self.revise_memory(
                    request_id,
                    memory_id,
                    expected_last_sequence,
                    content.into_string(),
                )
                .await?;
            }
            UiIntent::ApproveMemoryProposal {
                request_id,
                memory_id,
                expected_last_sequence,
                proposal_revision_id,
            } => {
                self.approve_memory_proposal(
                    request_id,
                    memory_id,
                    expected_last_sequence,
                    proposal_revision_id,
                )
                .await?;
            }
            UiIntent::RejectMemoryProposal {
                request_id,
                memory_id,
                expected_last_sequence,
                proposal_revision_id,
            } => {
                self.reject_memory_proposal(
                    request_id,
                    memory_id,
                    expected_last_sequence,
                    proposal_revision_id,
                )
                .await?;
            }
            UiIntent::RetractMemory {
                request_id,
                memory_id,
                expected_last_sequence,
                revision_id,
            } => {
                self.retract_memory(request_id, memory_id, expected_last_sequence, revision_id)
                    .await?;
            }
            UiIntent::DeleteMemory {
                request_id,
                memory_id,
                expected_last_sequence,
            } => {
                self.delete_memory(request_id, memory_id, expected_last_sequence)
                    .await?;
            }
            UiIntent::ExportMemory {
                request_id,
                memory_id,
            } => {
                self.export_memory(request_id, memory_id).await?;
            }
        }
        Ok(())
    }

    /// Rebuilds and publishes the all-sessions read model from durable state.
    async fn publish_sessions(&self) -> Result<(), AppError> {
        let active = self.session_id.as_str().to_owned();
        let summaries = match self.engine.list_sessions().await {
            Ok(summaries) => summaries,
            Err(error) => {
                tracing::warn!(error = %error, "session listing failed");
                return Ok(());
            }
        };
        let sessions = summaries
            .iter()
            .map(|summary| SessionBrowserEntry {
                session_id: summary.session_id().as_str().to_owned(),
                title: summary.display_title(),
                archived: summary.status() == SessionStatus::Archived,
                selected_model: summary.selected_model().cloned(),
                message_count: summary.message_count(),
                updated_at_ms: summary.updated_at().get(),
                active: summary.session_id().as_str() == active,
            })
            .collect();
        self.ports
            .session_lists
            .send_replace(Arc::new(SessionsProjection { sessions }));
        Ok(())
    }

    /// Rebuilds the latest accepted bounded Memory workspace view from durable projections.
    async fn publish_memories(&mut self) -> Result<(), AppError> {
        let view = match self.memory_view.clone() {
            Some(view) => view,
            None => {
                let view = MemoryViewState {
                    generation: 0,
                    query: default_memory_view_query()?,
                };
                self.memory_view = Some(view.clone());
                view
            }
        };
        let projection = self.load_memory_view(&view).await?;
        self.ports.memories.send_replace(Arc::new(projection));
        Ok(())
    }

    async fn query_memories(
        &mut self,
        request_id: RequestId,
        view_generation: u64,
        query: MemoryViewQuery,
    ) -> Result<(), AppError> {
        self.memory_view = Some(MemoryViewState {
            generation: view_generation,
            query,
        });
        match self.publish_memories().await {
            Ok(()) => self.commit(request_id).await,
            Err(error) => {
                tracing::warn!(error = %error, "authoritative Memory view query failed");
                let failure = memory_view_failure();
                let ledger_generation = self.ports.memories.borrow().generation();
                self.ports.memories.send_replace(Arc::new(
                    MemoryProjection::failed(ledger_generation, failure.clone())
                        .with_view_page(view_generation, None),
                ));
                self.reject(request_id, failure).await
            }
        }
    }

    async fn load_memory_view(&self, view: &MemoryViewState) -> Result<MemoryProjection, AppError> {
        let generation = self.engine.memory_mutation_generation().await?;
        let as_of = ids::now();
        let before = view
            .query
            .before()
            .map(decode_memory_view_cursor)
            .transpose()?;
        let mut query = MemoryInspectionQuery::new(
            self.memory_view_scopes(view.query.scope())?,
            Vec::new(),
            before,
            u32::from(view.query.limit()),
        )?
        .with_effective_statuses(memory_view_statuses(view.query.status()), as_of);
        if !view.query.literal().trim().is_empty() {
            query = query.with_literal_search(
                MemoryContent::new(view.query.literal().to_owned())
                    .map_err(|_| AppError::Configuration)?,
            );
        }
        let page = self.engine.inspect_memories(query).await?;
        let has_more = page.has_more();
        let next_cursor = if has_more {
            Some(encode_memory_view_cursor(
                page.records().last().ok_or(AppError::Configuration)?,
            )?)
        } else {
            None
        };
        let mut rows = Vec::with_capacity(page.records().len());
        for record in page.into_records() {
            let query = MemoryAdmissionQuery::new(
                MemoryAdmissionKey::Memory(record.memory_id().clone()),
                None,
                projection::memory_admission_page_size(),
            )?;
            let admissions = self.engine.load_memory_admissions(query).await?;
            rows.push((record, admissions));
        }
        projection::memory_at(generation.get(), rows, has_more, as_of)
            .map(|projection| projection.with_view_page(view.generation, next_cursor))
            .map_err(|_| AppError::Configuration)
    }

    fn memory_view_scopes(
        &self,
        filter: MemoryScopeFilter,
    ) -> Result<Vec<DomainMemoryScope>, AppError> {
        let scope = self.context_scope()?;
        let scopes = match filter {
            MemoryScopeFilter::All => self.authorized_memory_scopes()?,
            MemoryScopeFilter::User => {
                vec![DomainMemoryScope::User(scope.user_id().clone())]
            }
            MemoryScopeFilter::Workspace => {
                vec![DomainMemoryScope::Workspace(scope.workspace_id().clone())]
            }
            MemoryScopeFilter::Session => {
                vec![DomainMemoryScope::Session(self.session_id.clone())]
            }
            MemoryScopeFilter::Agent => {
                vec![DomainMemoryScope::Agent(scope.agent_id().clone())]
            }
        };
        Ok(scopes)
    }

    fn authorized_memory_scopes(&self) -> Result<Vec<DomainMemoryScope>, AppError> {
        let scope = self.context_scope()?;
        Ok(vec![
            DomainMemoryScope::User(scope.user_id().clone()),
            DomainMemoryScope::Workspace(scope.workspace_id().clone()),
            DomainMemoryScope::Session(self.session_id.clone()),
            DomainMemoryScope::Agent(scope.agent_id().clone()),
        ])
    }

    async fn remember_memory(
        &mut self,
        request_id: RequestId,
        content: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let memory_id = ids::memory_id();
        let revision = user_memory_draft(
            MemoryRevisionNumber::FIRST,
            None,
            content,
            ConfidenceBasisPoints::new(10_000).expect("maximum confidence is valid"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
        )?;
        let command = ids::memory_command(
            memory_id,
            None,
            MemoryCommandPayload::CreateMemory {
                scope: DomainMemoryScope::Workspace(self.context_scope()?.workspace_id().clone()),
                memory_kind: MemoryKind::Fact,
                revision,
            },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn import_memory(
        &mut self,
        request_id: RequestId,
        relative_path: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let workspace_id = self.context_scope()?.workspace_id().clone();
        let command = match build_workspace_document_import(
            &self.workspace,
            std::path::Path::new(&relative_path),
            workspace_id,
        ) {
            Ok(command) => command,
            Err(error) => {
                self.reject(request_id, memory_import_failure(error))
                    .await?;
                return Ok(());
            }
        };
        self.commit_memory_command(request_id, command).await
    }

    async fn revise_memory(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
        content: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let Some((memory_id, expected)) = self
            .parse_memory_target(request_id, raw_memory_id, expected_last_sequence)
            .await?
        else {
            return Ok(());
        };
        let revisions = self.engine.load_memory_revisions(memory_id.clone()).await?;
        let Some(active) = revisions
            .iter()
            .find(|revision| revision.status() == MemoryRevisionStatus::Active)
        else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let revision_number = next_memory_revision(&revisions)?;
        let revision = user_memory_draft(
            revision_number,
            active.subject_key().cloned(),
            content,
            active.confidence(),
            active.sensitivity(),
            active.validity(),
            active.relations().to_vec(),
        )?;
        let command = ids::memory_command(
            memory_id,
            Some(expected),
            MemoryCommandPayload::ReviseMemory {
                revision,
                supersedes_revision_id: active.revision_id().clone(),
            },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn approve_memory_proposal(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
        raw_proposal_revision_id: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let Some((memory_id, expected)) = self
            .parse_memory_target(request_id, raw_memory_id, expected_last_sequence)
            .await?
        else {
            return Ok(());
        };
        let Ok(proposal_revision_id) = MemoryRevisionId::new(raw_proposal_revision_id) else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let revisions = self.engine.load_memory_revisions(memory_id.clone()).await?;
        let Some(proposal) = revisions.iter().find(|revision| {
            revision.revision_id() == &proposal_revision_id
                && revision.status() == MemoryRevisionStatus::Proposed
        }) else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let Some(content) = self
            .engine
            .load_memory_content(proposal_revision_id.clone())
            .await?
        else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let revision = user_memory_draft(
            next_memory_revision(&revisions)?,
            proposal.subject_key().cloned(),
            content.as_str().to_owned(),
            proposal.confidence(),
            proposal.sensitivity(),
            proposal.validity(),
            proposal.relations().to_vec(),
        )?;
        let command = ids::memory_command(
            memory_id,
            Some(expected),
            MemoryCommandPayload::ApproveProposal {
                proposal_revision_id,
                approved_revision: revision,
            },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn reject_memory_proposal(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
        raw_proposal_revision_id: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let Some((memory_id, expected)) = self
            .parse_memory_target(request_id, raw_memory_id, expected_last_sequence)
            .await?
        else {
            return Ok(());
        };
        let Ok(revision_id) = MemoryRevisionId::new(raw_proposal_revision_id) else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let command = ids::memory_command(
            memory_id,
            Some(expected),
            MemoryCommandPayload::RejectRevision {
                revision_id,
                reason: MemoryRejectionReason::AuthorityDeclined,
            },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn retract_memory(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
        raw_revision_id: String,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let Some((memory_id, expected)) = self
            .parse_memory_target(request_id, raw_memory_id, expected_last_sequence)
            .await?
        else {
            return Ok(());
        };
        let Ok(revision_id) = MemoryRevisionId::new(raw_revision_id) else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let command = ids::memory_command(
            memory_id,
            Some(expected),
            MemoryCommandPayload::RetractMemory { revision_id },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn delete_memory(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
    ) -> Result<(), AppError> {
        if !self.memory_mutation_ready(request_id).await? {
            return Ok(());
        }
        let Some((memory_id, expected)) = self
            .parse_memory_target(request_id, raw_memory_id, expected_last_sequence)
            .await?
        else {
            return Ok(());
        };
        let revisions = self.engine.load_memory_revisions(memory_id.clone()).await?;
        let Some(revision_id) = revisions
            .last()
            .map(|revision| revision.revision_id().clone())
        else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        let command = ids::memory_command(
            memory_id,
            Some(expected),
            MemoryCommandPayload::DeleteMemory { revision_id },
        );
        self.commit_memory_command(request_id, command).await
    }

    async fn export_memory(
        &mut self,
        request_id: RequestId,
        raw_memory_id: String,
    ) -> Result<(), AppError> {
        let Ok(memory_id) = MemoryId::new(raw_memory_id) else {
            self.reject(request_id, stale_memory_failure()).await?;
            return Ok(());
        };
        match self
            .engine
            .export_memory(memory_id, self.authorized_memory_scopes()?)
            .await
        {
            Ok(_) => self.commit(request_id).await,
            Err(error) => self.reject(request_id, memory_failure(&error)).await,
        }
    }

    async fn memory_mutation_ready(&self, request_id: RequestId) -> Result<bool, AppError> {
        if self.active.is_none() {
            return Ok(true);
        }
        self.reject(
            request_id,
            UiFailure::new(
                ErrorClass::Conflict,
                "Finish or cancel the active response before changing durable memory",
                RetryPolicy::Now,
            ),
        )
        .await?;
        Ok(false)
    }

    async fn provider_configuration_mutation_ready(
        &self,
        request_id: RequestId,
    ) -> Result<bool, AppError> {
        if self.active.is_none() {
            return Ok(true);
        }
        self.reject(
            request_id,
            UiFailure::new(
                ErrorClass::Conflict,
                "Finish or cancel the active response before changing provider settings or saved credentials",
                RetryPolicy::Now,
            )
            .with_code("credential_change_active"),
        )
        .await?;
        Ok(false)
    }

    async fn parse_memory_target(
        &self,
        request_id: RequestId,
        raw_memory_id: String,
        expected_last_sequence: u64,
    ) -> Result<Option<(MemoryId, MemorySequence)>, AppError> {
        let target = MemoryId::new(raw_memory_id)
            .ok()
            .zip(MemorySequence::new(expected_last_sequence).ok());
        if target.is_none() {
            self.reject(request_id, stale_memory_failure()).await?;
        }
        Ok(target)
    }

    async fn commit_memory_command(
        &mut self,
        request_id: RequestId,
        command: autoharness_domain::MemoryCommandEnvelope,
    ) -> Result<(), AppError> {
        match self.memory_command_contains_configured_secret(&command) {
            Ok(false) => {}
            Ok(true) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Validation,
                        "Configured provider credentials cannot be stored as durable memory",
                        RetryPolicy::Never,
                    )
                    .with_code("memory_secret"),
                )
                .await?;
                return Ok(());
            }
            Err(_) => {
                self.reject(request_id, memory_redaction_unavailable_failure())
                    .await?;
                return Ok(());
            }
        }
        match self.engine.execute_memory_command(command).await {
            Ok(commit) => {
                tracing::debug!(
                    generation = commit.receipt().generation().get(),
                    duplicate = commit.duplicate_memory_id().is_some(),
                    contradictions = commit.contradiction_candidates().len(),
                    validated = commit.validation().is_some(),
                    "memory lifecycle command committed"
                );
                self.publish_memories().await?;
                self.commit(request_id).await
            }
            Err(error) => self.reject(request_id, memory_failure(&error)).await,
        }
    }

    fn memory_command_contains_configured_secret(
        &self,
        command: &autoharness_domain::MemoryCommandEnvelope,
    ) -> Result<bool, ProfileManagementError> {
        let Some(draft) = memory_command_draft(command.payload()) else {
            return Ok(false);
        };
        let credentials = self.configured_credential_sentinels()?;
        Ok(
            self.value_contains_configured_secret(draft.content().as_str(), &credentials)
                || draft.evidence().iter().any(|evidence| {
                    evidence.excerpt().is_some_and(|excerpt| {
                        self.value_contains_configured_secret(excerpt.as_str(), &credentials)
                    })
                }),
        )
    }

    async fn update_local_preference(
        &mut self,
        request_id: RequestId,
        change: LocalPreferenceChange,
    ) -> Result<(), AppError> {
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let mut local_profile = match manager.local_profile() {
            Ok(local_profile) => local_profile,
            Err(error) => {
                self.reject(request_id, profile_failure(&error)).await?;
                return Ok(());
            }
        };
        let mut preferences = local_profile.preferences().clone();
        let display_label = match change {
            LocalPreferenceChange::DisplayLabel(value) => match value {
                Some(value) => match DisplayLabel::new(value) {
                    Ok(label) => Some(label),
                    Err(_) => {
                        self.reject(
                            request_id,
                            profile_validation_failure(
                                "the local display label must be visible, bounded text",
                            ),
                        )
                        .await?;
                        return Ok(());
                    }
                },
                None => None,
            },
            LocalPreferenceChange::ThemePreset(value) => {
                preferences.set_theme_preset(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::ColorMode(value) => {
                preferences.set_color_mode(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::GlyphMode(value) => {
                preferences.set_glyph_mode(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::ReducedMotion(value) => {
                preferences.set_reduced_motion(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::Density(value) => {
                preferences.set_density(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::Layout(value) => {
                preferences.set_layout(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::TerminalTimestampStyle(value) => {
                preferences.set_terminal_timestamp_style(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::ComposerSubmitBehavior(value) => {
                preferences.set_composer_submit_behavior(value);
                local_profile.display_label().cloned()
            }
            LocalPreferenceChange::PromptStatusDetail(value) => {
                preferences.set_prompt_status_detail(value);
                local_profile.display_label().cloned()
            }
        };
        local_profile.set_display_label(display_label);
        local_profile.set_preferences(preferences);
        match manager.set_local_profile(local_profile) {
            Ok(()) => {
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => {
                self.reject(request_id, profile_failure(&error)).await?;
            }
        }
        Ok(())
    }

    fn publish_profiles(&self) {
        let Some(runtime) = &self.profiles else {
            return;
        };
        let Ok(snapshot) = runtime.manager.snapshot() else {
            return;
        };
        let local_profile = runtime
            .manager
            .resolved_settings()
            .map(|settings| settings.local_profile().clone())
            .unwrap_or_default();
        let active_profile = snapshot.profiles.iter().find(|profile| profile.active);
        let active_default_model = active_profile
            .and_then(|profile| profile.profile.default_model())
            .map(str::to_owned);
        let active_default_effort = active_profile
            .and_then(|profile| profile.profile.default_reasoning_effort())
            .map(str::to_owned);
        let profiles = snapshot
            .profiles
            .iter()
            .map(|managed| {
                let kind = provider_kind_label(managed.profile.kind());
                let credential_source = if managed.active && self.session_credential_connected {
                    CredentialSourceLabel::SessionOnly
                } else if runtime.environment.has(managed.profile.kind()) {
                    CredentialSourceLabel::Environment
                } else if managed.credential_state == StoredCredentialState::Stored {
                    CredentialSourceLabel::CredentialVault
                } else {
                    CredentialSourceLabel::SessionOnly
                };
                ProviderProfileProjection {
                    id: managed.id.as_str().to_owned(),
                    kind,
                    active: managed.active,
                    base_url: managed.profile.base_url().unwrap_or_default().to_owned(),
                    project: managed.profile.project().unwrap_or_default().to_owned(),
                    auth_header: managed.profile.auth_header().unwrap_or_default().to_owned(),
                    credential_state: match managed.credential_state {
                        StoredCredentialState::Disconnected => {
                            ProfileCredentialStateLabel::Disconnected
                        }
                        StoredCredentialState::Stored => ProfileCredentialStateLabel::Stored,
                        StoredCredentialState::RecoveryPending => {
                            ProfileCredentialStateLabel::RecoveryPending
                        }
                    },
                    credential_source,
                    connection: runtime
                        .connection
                        .get(managed.id.as_str())
                        .cloned()
                        .unwrap_or_default(),
                    default_model: managed.profile.default_model().map(str::to_owned),
                    default_mode: managed
                        .profile
                        .default_reasoning_effort()
                        .unwrap_or("provider default")
                        .to_owned(),
                }
            })
            .collect();
        let active_id = active_profile.map(|profile| profile.id.as_str().to_owned());
        self.ports
            .profiles
            .send_replace(Arc::new(ProfilesProjection {
                user: LocalUserProfileProjection {
                    display_label: local_profile
                        .display_label()
                        .value()
                        .as_ref()
                        .map(ToString::to_string),
                    workspace: runtime.workspace.clone(),
                    default_profile: active_id.clone(),
                    default_model: active_default_model,
                    default_mode: active_default_effort
                        .unwrap_or_else(|| "provider default".to_owned()),
                },
                profiles,
                pending_recovery: snapshot.pending_recovery,
            }));
        let active_kind = active_profile.map(|profile| profile.profile.kind());
        let credential_source = if self.session_credential_connected {
            CredentialSourceLabel::SessionOnly
        } else {
            active_profile.map_or(CredentialSourceLabel::SessionOnly, |profile| {
                if runtime.environment.has(profile.profile.kind()) {
                    CredentialSourceLabel::Environment
                } else if profile.credential_state == StoredCredentialState::Stored {
                    CredentialSourceLabel::CredentialVault
                } else {
                    CredentialSourceLabel::SessionOnly
                }
            })
        };
        let credential_connected = self.session_credential_connected
            || active_profile.is_some_and(|profile| {
                runtime.environment.has(profile.profile.kind())
                    || profile.credential_state == StoredCredentialState::Stored
            });
        self.ports
            .settings
            .send_replace(Arc::new(SettingsProjection {
                provider_status: autoharness_tui::ProviderStatusProjection {
                    active_profile: active_id,
                    provider_kind: active_kind.map(provider_kind_label),
                    credential_source,
                    credential_connected,
                },
                local_profile,
                git_branch: runtime.git_branch.clone(),
            }));
    }

    async fn upsert_profile(
        &mut self,
        request_id: RequestId,
        draft: ProviderProfileDraft,
    ) -> Result<(), AppError> {
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let id = match ProfileId::new(&draft.id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let profile = match provider_profile_from_draft(draft) {
            Ok(profile) => profile,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        match manager.upsert(&id, &profile) {
            Ok(()) => {
                let activated_codex =
                    profile.kind() == autoharness_settings::ProviderKind::CodexCli;
                if activated_codex {
                    if let Err(error) = manager.activate(Some(&id)) {
                        self.reject(request_id, profile_failure(&error)).await?;
                        return Ok(());
                    }
                    self.configure_active_profile(&id);
                }
                if !activated_codex
                    && manager
                        .snapshot()
                        .ok()
                        .and_then(|snapshot| {
                            snapshot
                                .profiles
                                .into_iter()
                                .find(|profile| profile.id == id)
                        })
                        .is_some_and(|profile| profile.active)
                {
                    self.configure_active_profile(&id);
                }
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => {
                self.publish_profiles();
                self.reject(request_id, profile_failure(&error)).await?;
            }
        }
        Ok(())
    }

    async fn duplicate_profile(
        &mut self,
        request_id: RequestId,
        source: String,
        destination: String,
    ) -> Result<(), AppError> {
        let source = match ProfileId::new(source) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let destination = match ProfileId::new(destination) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        match manager.duplicate(&source, &destination) {
            Ok(()) => {
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, profile_failure(&error)).await?,
        }
        Ok(())
    }

    async fn activate_profile(
        &mut self,
        request_id: RequestId,
        profile_id: String,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Cancel or finish the active response before switching provider profiles",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        let id = match ProfileId::new(profile_id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        match manager.activate(Some(&id)) {
            Ok(()) => {
                self.configure_active_profile(&id);
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, profile_failure(&error)).await?,
        }
        Ok(())
    }

    async fn save_profile_credential(
        &mut self,
        request_id: RequestId,
        profile_id: String,
        credential: ApiCredential,
        replace: bool,
    ) -> Result<(), AppError> {
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let id = match ProfileId::new(profile_id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let secret = Zeroizing::new(credential.into_string());
        let result = if replace {
            manager.replace_credential(&id, &secret)
        } else {
            manager.save_credential(&id, &secret).map(|_| ())
        };
        match result {
            Ok(()) => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime
                        .connection
                        .insert(id.as_str().to_owned(), ProfileConnectionState::Untested);
                }
                if manager
                    .snapshot()
                    .ok()
                    .and_then(|snapshot| {
                        snapshot
                            .profiles
                            .into_iter()
                            .find(|profile| profile.id == id)
                    })
                    .is_some_and(|profile| profile.active)
                {
                    self.configure_active_profile(&id);
                }
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => {
                self.publish_profiles();
                self.reject(request_id, profile_failure(&error)).await?;
            }
        }
        Ok(())
    }

    async fn test_profile(
        &mut self,
        request_id: RequestId,
        profile_id: String,
    ) -> Result<(), AppError> {
        let id = match ProfileId::new(profile_id.clone()) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let provider = match self.provider_for_profile(&id) {
            Ok(Some(provider)) => provider,
            Ok(None) => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.insert(
                        profile_id,
                        ProfileConnectionState::Failed(
                            "No effective credential is available".to_owned(),
                        ),
                    );
                }
                self.publish_profiles();
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Authentication,
                        "No effective credential is available for this profile",
                        RetryPolicy::Never,
                    ),
                )
                .await?;
                return Ok(());
            }
            Err(failure) => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.insert(
                        profile_id,
                        ProfileConnectionState::Failed(failure.message.clone()),
                    );
                }
                self.publish_profiles();
                self.reject(request_id, failure).await?;
                return Ok(());
            }
        };
        if let Some(runtime) = self.profiles.as_mut() {
            runtime
                .connection
                .insert(profile_id.clone(), ProfileConnectionState::Testing);
        }
        self.publish_profiles();
        let messages = self.messages.clone();
        let cancellation = self.shutdown.child_token();
        tokio::spawn(async move {
            let result = provider
                .list_models(CatalogRequest::Refresh, cancellation)
                .await;
            let _ = messages
                .send(AsyncMessage::ProfileTest {
                    profile_id,
                    request_id,
                    result,
                })
                .await;
        });
        Ok(())
    }
    async fn set_profile_default_model(
        &mut self,
        request_id: RequestId,
        profile_id: String,
    ) -> Result<(), AppError> {
        let Some(selected_model) = self.session.selected_model().cloned() else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Select a model before saving a profile default",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        self.set_profile_default(request_id, profile_id, selected_model, None)
            .await
    }

    async fn set_profile_default(
        &mut self,
        request_id: RequestId,
        profile_id: String,
        selected_model: autoharness_domain::ModelRef,
        reasoning_effort: Option<String>,
    ) -> Result<(), AppError> {
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let id = match ProfileId::new(profile_id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        if !self.model_is_available(&selected_model) {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Choose a model from the active profile's current catalog first",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let model_id = selected_model.model_id().as_str().to_owned();
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let active = manager
            .snapshot()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .profiles
                    .into_iter()
                    .find(|profile| profile.id == id)
            })
            .is_some_and(|profile| profile.active);
        if !active {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Activate this profile before assigning the selected model",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        match manager.set_agent_defaults(&id, model_id, reasoning_effort) {
            Ok(()) => {
                self.configure_active_profile(&id);
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, profile_failure(&error)).await?,
        }
        Ok(())
    }

    async fn disconnect_profile(
        &mut self,
        request_id: RequestId,
        profile_id: String,
    ) -> Result<(), AppError> {
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let id = match ProfileId::new(profile_id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let was_active = manager
            .snapshot()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .profiles
                    .into_iter()
                    .find(|profile| profile.id == id)
            })
            .is_some_and(|profile| profile.active);
        match manager.disconnect(&id) {
            Ok(()) => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime
                        .connection
                        .insert(id.as_str().to_owned(), ProfileConnectionState::Untested);
                }
                if was_active {
                    self.configure_active_profile(&id);
                }
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => {
                self.publish_profiles();
                self.reject(request_id, profile_failure(&error)).await?;
            }
        }
        Ok(())
    }

    async fn delete_profile(
        &mut self,
        request_id: RequestId,
        profile_id: String,
    ) -> Result<(), AppError> {
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let id = match ProfileId::new(profile_id) {
            Ok(id) => id,
            Err(reason) => {
                self.reject(request_id, profile_validation_failure(reason))
                    .await?;
                return Ok(());
            }
        };
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let was_active = manager
            .snapshot()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .profiles
                    .into_iter()
                    .find(|profile| profile.id == id)
            })
            .is_some_and(|profile| profile.active);
        match manager.delete(&id) {
            Ok(()) | Err(ProfileManagementError::RecoveryPending) => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.remove(id.as_str());
                }
                if was_active {
                    self.provider = None;
                    self.session_credential_connected = false;
                    self.catalog_models.clear();
                    self.ports
                        .catalogs
                        .send_replace(Arc::new(CatalogProjection::CredentialRequired));
                }
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Err(error) => {
                self.publish_profiles();
                self.reject(request_id, profile_failure(&error)).await?;
            }
        }
        Ok(())
    }

    fn configure_active_profile(&mut self, id: &ProfileId) {
        self.session_credential_connected = false;
        match self.provider_for_profile(id) {
            Ok(Some(provider)) => {
                self.provider = Some(provider);
                self.catalog_models.clear();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Loading));
                self.refresh_catalog(None);
            }
            Ok(None) => {
                self.provider = None;
                self.catalog_models.clear();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::CredentialRequired));
            }
            Err(failure) => {
                self.provider = None;
                self.catalog_models.clear();
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.insert(
                        id.as_str().to_owned(),
                        ProfileConnectionState::Failed(failure.message),
                    );
                }
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::CredentialRequired));
            }
        }
    }

    fn provider_for_profile(&self, id: &ProfileId) -> Result<Option<Arc<dyn Provider>>, UiFailure> {
        let runtime = self
            .profiles
            .as_ref()
            .ok_or_else(profile_unavailable_failure)?;
        let snapshot = runtime
            .manager
            .snapshot()
            .map_err(|error| profile_failure(&error))?;
        let managed = snapshot
            .profiles
            .into_iter()
            .find(|profile| profile.id == *id)
            .ok_or_else(|| profile_validation_failure("that profile does not exist"))?;
        let secret = match runtime.environment.credential(managed.profile.kind()) {
            Some(secret) => secret,
            None => match runtime.manager.credential_for_test(id) {
                Ok(secret) => secret,
                Err(ProfileManagementError::CredentialNotStored) => return Ok(None),
                Err(error) => return Err(profile_failure(&error)),
            },
        };
        (runtime.factory)(id, &managed.profile, secret)
            .map(Some)
            .map_err(|error| provider_failure(&error))
    }

    /// Opens a different durable session after verifying it is switch-safe.
    ///
    /// The switch is rejected while an attempt is in flight in the current
    /// session so no provider task or pending permission can be orphaned.
    /// The target aggregate is rebuilt from its authoritative history before
    /// any projection swap, so an unknown identity fails closed.
    async fn open_session(
        &mut self,
        request_id: RequestId,
        raw_session_id: String,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Cancel or wait for the active response before switching sessions",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if raw_session_id == self.session_id.as_str() {
            // Selecting the already-active row just closes the browser.
            self.commit(request_id).await?;
            return Ok(());
        }
        let target = match SessionId::new(raw_session_id.clone()) {
            Ok(target) => target,
            Err(_) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Validation,
                        "That session identity is invalid",
                        RetryPolicy::Never,
                    ),
                )
                .await?;
                return Ok(());
            }
        };
        let events = match self.engine.load_events(target.clone()).await {
            Ok(events) => events,
            Err(_) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Storage,
                        "That session could not be loaded from local storage",
                        RetryPolicy::Never,
                    ),
                )
                .await?;
                return Ok(());
            }
        };
        let aggregate =
            match autoharness_engine::SessionAggregate::rehydrate(target.clone(), &events) {
                Ok(aggregate) => aggregate,
                Err(_) => {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Storage,
                            "That session's history failed validation",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                    return Ok(());
                }
            };

        self.session_id = target;
        self.session = aggregate;
        self.active = None;
        self.catalog_cancellation = None;
        self.ports
            .sessions
            .send_replace(Arc::new(projection::session(&self.session)));
        self.publish_sessions().await?;
        self.commit(request_id).await?;
        self.maybe_resume_after_tools().await
    }

    async fn rename_session(
        &mut self,
        request_id: RequestId,
        raw_session_id: String,
        title: String,
    ) -> Result<(), AppError> {
        let Ok(session_id) = SessionId::new(raw_session_id.clone()) else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "That session identity is invalid",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let title = match SessionTitle::new(title) {
            Ok(title) => title,
            Err(_) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Validation,
                        "Session titles must be 1-128 visible characters",
                        RetryPolicy::Never,
                    ),
                )
                .await?;
                return Ok(());
            }
        };
        let payload = CommandPayload::RenameSession {
            session_id: session_id.clone(),
            title,
        };
        match self.execute(payload).await {
            Ok(()) => {
                self.publish_sessions().await?;
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, engine_failure(&error)).await?,
        }
        Ok(())
    }

    async fn archive_session(
        &mut self,
        request_id: RequestId,
        raw_session_id: String,
        archive: bool,
    ) -> Result<(), AppError> {
        let Ok(session_id) = SessionId::new(raw_session_id) else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "That session identity is invalid",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let payload = if archive {
            CommandPayload::ArchiveSession {
                session_id: session_id.clone(),
            }
        } else {
            CommandPayload::UnarchiveSession {
                session_id: session_id.clone(),
            }
        };
        match self.execute(payload).await {
            Ok(()) => {
                self.publish_sessions().await?;
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, engine_failure(&error)).await?,
        }
        Ok(())
    }

    /// Permanently deletes a settled session and keeps one open conversation.
    ///
    /// Deleting the current session first prepares the newest other open
    /// session, then swaps to it only after export and deletion succeed.
    async fn delete_session(
        &mut self,
        request_id: RequestId,
        raw_session_id: String,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Cancel or finish the active response before deleting a session",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        let Ok(session_id) = SessionId::new(raw_session_id) else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "That session identity is invalid",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let deleting_current = session_id == self.session_id;
        let summaries = self.engine.list_sessions().await?;
        let Some(summary) = summaries
            .iter()
            .find(|summary| summary.session_id() == &session_id)
            .cloned()
        else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::NotFound,
                    "That session no longer exists",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let replacement = if deleting_current {
            let Some(replacement_summary) = summaries.iter().find(|candidate| {
                candidate.session_id() != &session_id && candidate.status() == SessionStatus::Active
            }) else {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Conflict,
                        "Create another session before deleting your only open session",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
                return Ok(());
            };
            let replacement_id = replacement_summary.session_id().clone();
            let events = match self.engine.load_events(replacement_id.clone()).await {
                Ok(events) => events,
                Err(_) => {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Storage,
                            "The next session could not be loaded, so nothing was deleted",
                            RetryPolicy::Now,
                        ),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let aggregate = match SessionAggregate::rehydrate(replacement_id.clone(), &events) {
                Ok(aggregate) => aggregate,
                Err(_) => {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Storage,
                            "The next session failed validation, so nothing was deleted",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                    return Ok(());
                }
            };
            Some((replacement_id, aggregate))
        } else {
            None
        };
        let expected_last_sequence = summary.last_sequence().get();
        let delete_result = self
            .engine
            .export_and_delete_session(session_id, expected_last_sequence)
            .await;
        match delete_result {
            Ok(_) => {
                if let Some((replacement_id, aggregate)) = replacement {
                    self.session_id = replacement_id;
                    self.session = aggregate;
                    self.ports
                        .sessions
                        .send_replace(Arc::new(projection::session(&self.session)));
                }
                self.publish_sessions().await?;
                self.commit(request_id).await?;
                if deleting_current {
                    self.maybe_resume_after_tools().await?;
                }
            }
            Err(AppError::Store(autoharness_store::StoreError::InvalidSessionTransition)) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Conflict,
                        "The session still has unsettled work and cannot be deleted yet",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
            }
            Err(_) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Storage,
                        "The session could not be deleted from local storage",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Writes the active session transcript as Markdown beside the database.
    ///
    /// The session is read-only for this operation, so it is allowed even
    /// while other work is active; a concurrent export is deduplicated by
    /// the terminal's pending-request tracking.
    async fn export_transcript(
        &mut self,
        request_id: RequestId,
        raw_session_id: String,
    ) -> Result<(), AppError> {
        let Ok(session_id) = SessionId::new(raw_session_id) else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "That session identity is invalid",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        if session_id != self.session_id {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "Only the active session can be exported from here",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        match self.engine.export_transcript_markdown(session_id).await {
            Ok(path) => {
                let _ = path;
                self.commit(request_id).await?;
            }
            Err(_) => {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Storage,
                        "The transcript could not be written to local storage",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn create_session(&mut self, request_id: RequestId) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Cancel or finish the active response before starting a new session",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }

        let session_id = ids::session_id();
        match self
            .engine
            .execute(ids::command(CommandPayload::CreateSession {
                session_id: session_id.clone(),
            }))
            .await
        {
            Ok(reply) => {
                self.session_id = session_id;
                self.session = reply.session;
                if let Some(model) = self.active_profile_default_model() {
                    self.execute(CommandPayload::SelectModel {
                        session_id: self.session_id.clone(),
                        model,
                    })
                    .await?;
                }
                self.ports
                    .sessions
                    .send_replace(Arc::new(projection::session(&self.session)));
                self.publish_sessions().await?;
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, engine_failure(&error)).await?,
        }
        Ok(())
    }

    fn active_profile_default_model(&self) -> Option<autoharness_domain::ModelRef> {
        let default_model = self.profiles.as_ref().and_then(|runtime| {
            runtime.manager.snapshot().ok().and_then(|snapshot| {
                snapshot
                    .profiles
                    .into_iter()
                    .find(|profile| profile.active)
                    .and_then(|profile| profile.profile.default_model().map(str::to_owned))
            })
        })?;
        self.catalog_models
            .iter()
            .find(|model| model.model_id.as_str() == default_model)
            .map(|model| {
                autoharness_domain::ModelRef::new(model.provider_id.clone(), model.model_id.clone())
            })
    }

    fn set_active_profile_connection(&mut self, connection: ProfileConnectionState) {
        let active_profile_id = self.profiles.as_ref().and_then(|runtime| {
            runtime.manager.snapshot().ok().and_then(|snapshot| {
                snapshot
                    .profiles
                    .into_iter()
                    .find(|profile| profile.active)
                    .map(|profile| profile.id.as_str().to_owned())
            })
        });
        if let (Some(runtime), Some(profile_id)) = (self.profiles.as_mut(), active_profile_id) {
            runtime.connection.insert(profile_id, connection);
        }
    }

    async fn configure_credential(
        &mut self,
        request_id: RequestId,
        credential: ApiCredential,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Wait for or cancel the active response before changing the API key",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }

        telemetry::credential_submitted();
        match (self.provider_factory)(credential) {
            Ok(provider) => {
                telemetry::provider_ready();
                self.provider = Some(provider);
                self.session_credential_connected = true;
                self.catalog_models.clear();
                self.publish_profiles();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Loading));
                self.refresh_catalog(Some(request_id));
                self.maybe_resume_after_tools().await?;
            }
            Err(error) => {
                telemetry::provider_unavailable(&error);
                self.reject(request_id, provider_failure(&error)).await?;
            }
        }
        Ok(())
    }

    async fn start_codex_login(&mut self, request_id: RequestId) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Wait for or cancel the active response before signing in to Codex",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if self.codex_login.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A Codex sign-in is already in progress",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }

        let cancellation = self.shutdown.child_token();
        self.codex_login = Some((request_id, cancellation.clone()));
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let progress_messages = messages.clone();
            let result = login_with_browser(cancellation, move |progress| {
                if progress == CodexAuthProgress::BrowserOpened {
                    let _ = progress_messages
                        .try_send(AsyncMessage::CodexLoginBrowserOpened { request_id });
                }
            })
            .await;
            let _ = messages
                .send(AsyncMessage::CodexLoginFinished { request_id, result })
                .await;
        });
        Ok(())
    }

    async fn cancel_codex_login(&mut self, request_id: RequestId) -> Result<(), AppError> {
        if self
            .codex_login
            .as_ref()
            .is_some_and(|(active_request, _)| *active_request == request_id)
            && let Some((_, cancellation)) = self.codex_login.take()
        {
            cancellation.cancel();
        }
        self.commit(request_id).await
    }

    async fn handle_codex_login_finished(
        &mut self,
        request_id: RequestId,
        result: Result<Zeroizing<String>, ProviderError>,
    ) -> Result<(), AppError> {
        if !self
            .codex_login
            .as_ref()
            .is_some_and(|(active_request, _)| *active_request == request_id)
        {
            return Ok(());
        }
        self.codex_login = None;
        let credential = match result {
            Ok(credential) => credential,
            Err(error) => {
                self.reject(request_id, provider_failure(&error)).await?;
                return Ok(());
            }
        };
        if !self
            .provider_configuration_mutation_ready(request_id)
            .await?
        {
            return Ok(());
        }
        let Some(manager) = self
            .profiles
            .as_ref()
            .map(|runtime| Arc::clone(&runtime.manager))
        else {
            self.reject(request_id, profile_unavailable_failure())
                .await?;
            return Ok(());
        };
        let snapshot = match manager.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.reject(request_id, profile_failure(&error)).await?;
                return Ok(());
            }
        };
        let existing = snapshot
            .profiles
            .iter()
            .find(|managed| managed.profile.kind() == ProviderKind::CodexCli);
        let id = match existing {
            Some(managed) => managed.id.clone(),
            None => available_codex_profile_id(&snapshot.profiles),
        };
        if existing.is_none()
            && let Err(error) = manager.upsert(&id, &ProviderProfile::codex_cli())
        {
            self.reject(request_id, profile_failure(&error)).await?;
            return Ok(());
        }
        let credential_result = if existing
            .is_some_and(|managed| managed.credential_state == StoredCredentialState::Stored)
        {
            manager.replace_credential(&id, &credential)
        } else {
            manager.save_credential(&id, &credential).map(|_| ())
        };
        if let Err(error) = credential_result {
            self.publish_profiles();
            self.reject(request_id, profile_failure(&error)).await?;
            return Ok(());
        }
        if let Err(error) = manager.activate(Some(&id)) {
            self.publish_profiles();
            self.reject(request_id, profile_failure(&error)).await?;
            return Ok(());
        }
        if let Some(runtime) = self.profiles.as_mut() {
            runtime
                .connection
                .insert(id.as_str().to_owned(), ProfileConnectionState::Ready);
        }
        self.configure_active_profile(&id);
        self.publish_profiles();
        self.ports
            .notices
            .send(UiNotice::CodexLoginCompleted { request_id })
            .await
            .map_err(|_| AppError::WorkerStopped)
    }

    async fn submit_prompt(
        &mut self,
        request_id: RequestId,
        prompt: String,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A provider attempt is already active",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if self.provider.is_none() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Authentication,
                    "A provider API key is not configured",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let credentials = match self.configured_credential_sentinels() {
            Ok(credentials) => credentials,
            Err(_) => {
                self.reject(request_id, prompt_redaction_unavailable_failure())
                    .await?;
                return Ok(());
            }
        };
        let redacted_prompt = self.redact_configured_secrets(&prompt, &credentials);
        let prompt = match PromptText::new(redacted_prompt) {
            Ok(prompt) => prompt,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        if !self
            .session
            .selected_model()
            .is_some_and(|model| self.model_is_available(model))
        {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "Choose a model from the current catalog before sending",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let title_was_absent = self.session.title().is_none();

        let input_id = ids::input_id();
        let attempt_id = ids::attempt_id();
        if let Err(error) = self
            .execute(CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                input_id,
                prompt,
                delivery_mode: DeliveryMode::NextTurn,
            })
            .await
        {
            self.reject(request_id, engine_failure(&error)).await?;
            return Ok(());
        }
        if title_was_absent && self.session.title().is_some() {
            self.publish_sessions().await?;
        }
        telemetry::attempt_prepared();
        if let Err(error) = self.start_attempt(attempt_id, Some(request_id)).await {
            self.reject(request_id, start_attempt_failure(&error))
                .await?;
            return Ok(());
        }
        self.commit(request_id).await
    }

    async fn retry_attempt(
        &mut self,
        request_id: RequestId,
        attempt_key: AttemptKey,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A provider attempt is already active",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if self.provider.is_none() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Authentication,
                    "A provider API key is not configured",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        if !self
            .session
            .selected_model()
            .is_some_and(|model| self.model_is_available(model))
        {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "Choose a model from the current catalog before retrying",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let prior = match AttemptId::new(attempt_key.as_str()) {
            Ok(attempt_id) => attempt_id,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        let Some(input_id) = self
            .session
            .attempt(&prior)
            .map(|attempt| attempt.input_id().clone())
        else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::NotFound,
                    "The prior attempt no longer exists",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let retry = ids::attempt_id();
        if let Err(error) = self
            .execute(CommandPayload::PrepareAttempt {
                session_id: self.session_id.clone(),
                attempt_id: retry.clone(),
                input_id,
                retry_of: Some(prior),
            })
            .await
        {
            self.reject(request_id, engine_failure(&error)).await?;
            return Ok(());
        }
        telemetry::attempt_prepared();
        if let Err(error) = self.start_attempt(retry, None).await {
            self.reject(request_id, start_attempt_failure(&error))
                .await?;
            return Ok(());
        }
        self.commit(request_id).await
    }

    async fn cancel_attempt(
        &mut self,
        request_id: RequestId,
        attempt_key: AttemptKey,
    ) -> Result<(), AppError> {
        let attempt_id = match AttemptId::new(attempt_key.as_str()) {
            Ok(attempt_id) => attempt_id,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        let Some(active) = &self.active else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "The attempt is no longer active",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        if active.attempt_id != attempt_id {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A different provider attempt is active",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let was_awaiting_tools = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| attempt.status() == EngineAttemptStatus::AwaitingTools);
        let cancellation = active.cancellation.clone();
        match self
            .execute(CommandPayload::RequestAttemptCancellation {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .await
        {
            Ok(()) => {
                telemetry::cancellation_requested();
                cancellation.cancel();
                if was_awaiting_tools {
                    let pending: Vec<_> = self
                        .session
                        .tool_calls()
                        .iter()
                        .filter(|call| {
                            call.attempt_id() == &attempt_id
                                && !call.status().is_settled()
                                && call.status() != autoharness_engine::ToolCallStatus::Running
                        })
                        .map(|call| call.call().tool_call_id.clone())
                        .collect();
                    for tool_call_id in pending {
                        self.execute(CommandPayload::CancelToolCall {
                            session_id: self.session_id.clone(),
                            tool_call_id,
                        })
                        .await?;
                    }
                    if self.active_tool_calls_settled() {
                        self.execute(CommandPayload::CancelAttempt {
                            session_id: self.session_id.clone(),
                            attempt_id: attempt_id.clone(),
                        })
                        .await?;
                        self.active = None;
                    }
                }
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, engine_failure(&error)).await?,
        }
        Ok(())
    }

    async fn start_attempt(
        &mut self,
        attempt_id: AttemptId,
        benchmark_request_id: Option<RequestId>,
    ) -> Result<(), StartAttemptError> {
        let limits = RunLimits::default();
        let advertise_tools = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| self.model_supports_tools(attempt.model()));
        if let Err(error) = build_request(&self.session, &attempt_id, advertise_tools) {
            self.execute(CommandPayload::FailAttempt {
                session_id: self.session_id.clone(),
                attempt_id,
                failure: attempt_failure(&error),
            })
            .await
            .map_err(StartAttemptError::Engine)?;
            telemetry::attempt_settled("failed", Some(&error));
            return Err(StartAttemptError::Provider(error));
        }
        self.execute(CommandPayload::ConfigureRunBudget {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
            limits,
        })
        .await
        .map_err(StartAttemptError::Engine)?;
        if let Err(error) = self
            .execute(CommandPayload::StartAttempt {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .await
        {
            self.fail_context_preparation(&attempt_id)
                .await
                .map_err(StartAttemptError::Engine)?;
            return Err(StartAttemptError::Engine(error));
        }
        let mut budget = RunBudget::new(limits);
        if let Err(error) = budget.start_turn() {
            let provider_error = tool_provider_error(&error);
            self.execute(CommandPayload::FailAttempt {
                session_id: self.session_id.clone(),
                attempt_id,
                failure: attempt_failure(&provider_error),
            })
            .await
            .map_err(StartAttemptError::Engine)?;
            return Err(StartAttemptError::Provider(provider_error));
        }
        let prepared = match self
            .prepare_and_bind_context(&attempt_id, advertise_tools)
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail_context_preparation(&attempt_id)
                    .await
                    .map_err(StartAttemptError::Engine)?;
                return Err(StartAttemptError::Context(error));
            }
        };
        let credential_ingress = match self.credential_ingress_guard(&attempt_id).await {
            Ok(guard) => guard,
            Err(error) => {
                self.fail_context_preparation(&attempt_id)
                    .await
                    .map_err(StartAttemptError::Engine)?;
                return Err(StartAttemptError::Context(error));
            }
        };
        if let Err(error) = self
            .execute(CommandPayload::StartRunTurn {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .await
        {
            self.fail_context_preparation(&attempt_id)
                .await
                .map_err(StartAttemptError::Engine)?;
            return Err(StartAttemptError::Engine(error));
        }
        telemetry::attempt_started();
        let cancellation = self.shutdown.child_token();
        self.spawn_stream(
            attempt_id.clone(),
            prepared.request().clone(),
            cancellation.clone(),
            benchmark_request_id,
        );
        self.active = Some(ActiveAttempt {
            attempt_id,
            cancellation,
            budget,
            usage_base: DomainUsage::default(),
            credential_ingress: Some(credential_ingress),
        });
        Ok(())
    }

    async fn prepare_and_bind_context(
        &mut self,
        attempt_id: &AttemptId,
        advertise_tools: bool,
    ) -> Result<PreparedContextTurn, AppError> {
        for attempt_index in 0..CONTEXT_SNAPSHOT_ATTEMPTS {
            let snapshot = self
                .prepare_context_snapshot(attempt_id, advertise_tools)
                .await?;
            let prepared = &snapshot.turn;
            self.ensure_provider_request_is_credential_free(prepared.request())?;
            let binding = ids::command(CommandPayload::BindContextTurn {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                run_turn: prepared.manifest().run_turn(),
                context_turn_id: prepared.manifest().context_turn_id().clone(),
                manifest_hash: prepared.manifest().manifest_hash().clone(),
            });
            let result = match snapshot.compaction_boundary.clone() {
                Some(boundary) => {
                    self.engine
                        .commit_compaction_context_turn_and_bind(
                            prepared.commit().clone(),
                            boundary,
                            binding,
                        )
                        .await
                }
                None => {
                    self.engine
                        .commit_context_turn_and_bind(prepared.commit().clone(), binding)
                        .await
                }
            };
            match result {
                Ok(reply) => {
                    if reply.session.session_id() != &self.session_id {
                        return Err(AppError::Configuration);
                    }
                    self.session = reply.session;
                    self.ports
                        .sessions
                        .send_replace(Arc::new(projection::session(&self.session)));
                    let persisted = self
                        .engine
                        .load_attempt_context_turn(
                            attempt_id.clone(),
                            prepared.manifest().run_turn(),
                        )
                        .await?
                        .ok_or(AppError::Configuration)?;
                    if &persisted != prepared.manifest() {
                        return Err(AppError::Configuration);
                    }
                    return Ok(snapshot.turn);
                }
                Err(error)
                    if attempt_index + 1 < CONTEXT_SNAPSHOT_ATTEMPTS
                        && context_snapshot_conflict(&error) =>
                {
                    let fresh = self
                        .engine
                        .load_session(self.session_id.clone())
                        .await?
                        .ok_or(AppError::Configuration)?;
                    if fresh.session_id() != &self.session_id {
                        return Err(AppError::Configuration);
                    }
                    self.session = fresh;
                    self.ports
                        .sessions
                        .send_replace(Arc::new(projection::session(&self.session)));
                    continue;
                }
                Err(error) => return Err(AppError::Engine(error)),
            }
        }
        Err(AppError::Configuration)
    }

    async fn prepare_context_snapshot(
        &mut self,
        attempt_id: &AttemptId,
        advertise_tools: bool,
    ) -> Result<PreparedContextSnapshot, AppError> {
        let (run_turn, model, explicit_retry) = {
            let attempt = self
                .session
                .attempt(attempt_id)
                .ok_or(AppError::Configuration)?;
            (
                attempt
                    .turns_started()
                    .checked_add(1)
                    .ok_or(AppError::Configuration)?,
                attempt.model().clone(),
                attempt.retry_of().is_some(),
            )
        };
        let expected_session_sequence = self
            .session
            .last_sequence()
            .ok_or(AppError::Configuration)?;
        let descriptor = self
            .catalog_models
            .iter()
            .find(|descriptor| {
                descriptor.provider_id == *model.provider_id()
                    && descriptor.model_id == *model.model_id()
            })
            .cloned();
        let checkpoint = self
            .engine
            .load_latest_compaction_checkpoint(self.session_id.clone())
            .await?;
        let retained_history = match checkpoint.as_ref() {
            Some(checkpoint) => Some(self.load_compacted_history(checkpoint).await?),
            None => None,
        };
        let compacted_through = retained_history
            .as_ref()
            .map(CompactedHistoryV1::cutoff_sequence)
            .map(SessionSequence::new)
            .transpose()
            .map_err(|_| AppError::Configuration)?;
        let request = build_request_after_cutoff(
            &self.session,
            attempt_id,
            advertise_tools,
            compacted_through,
        )?;
        self.ensure_provider_request_is_credential_free(&request)?;
        let frozen = if run_turn > 1 {
            Some(
                self.load_frozen_context_baseline(attempt_id, run_turn)
                    .await?,
            )
        } else {
            None
        };
        let token_budget = frozen.as_ref().map_or_else(
            || context_token_budget(descriptor.as_ref()),
            |baseline| Ok(baseline.epoch().token_budget()),
        )?;
        let durable_memory_limit = frozen.as_ref().map_or_else(
            || EstimatedTokens::new(token_budget.get() / 4).map_err(|_| AppError::Configuration),
            |baseline| Ok(baseline.turn.budget().durable_memory_limit()),
        )?;
        let ordinary = if !exact_request_fits(&request, token_budget)? {
            None
        } else if let Some(baseline) = frozen.as_ref() {
            let retrieval_scope = self
                .context_scope()?
                .retrieval_scope(self.session_id.clone(), baseline.epoch().started_at());
            let compatibility = EpochCompatibility::new(
                &request,
                descriptor.as_ref(),
                &retrieval_scope,
                token_budget,
                durable_memory_limit,
            )?;
            Some(prepare_frozen_continuation(FrozenContinuationInput {
                request: request.clone(),
                expected_session_sequence,
                run_turn,
                committed_at: ids::now(),
                epoch: baseline.epoch().clone(),
                baseline_turn: baseline.turn.clone(),
                baseline_content: baseline.content.clone(),
                compatibility,
            }))
        } else {
            Some(
                self.prepare_new_context_epoch(
                    attempt_id,
                    run_turn,
                    expected_session_sequence,
                    &model,
                    request.clone(),
                    descriptor.as_ref(),
                    token_budget,
                    durable_memory_limit,
                    ContextEpochMode::NewAttempt { explicit_retry },
                    retained_history.as_ref(),
                    ids::now(),
                )
                .await,
            )
        };
        match ordinary {
            Some(Ok(turn)) if exact_request_fits(turn.request(), token_budget)? => {
                return Ok(PreparedContextSnapshot {
                    turn,
                    compaction_boundary: None,
                });
            }
            None
            | Some(Ok(_))
            | Some(Err(AppError::Memory(autoharness_memory::MemoryError::BudgetExceeded))) => {}
            Some(Err(error)) => return Err(error),
        }

        let candidate = self
            .select_compaction_candidate(
                attempt_id,
                run_turn,
                expected_session_sequence,
                &model,
                advertise_tools,
                descriptor.as_ref(),
                token_budget,
                durable_memory_limit,
                checkpoint.as_ref(),
                retained_history.as_ref(),
            )
            .await?
            .ok_or(AppError::Memory(
                autoharness_memory::MemoryError::BudgetExceeded,
            ))?;
        let proposal = self.build_compaction_proposal_command(&candidate).await?;
        let summary_revision_id = compaction_proposal_revision(&proposal)?.clone();
        self.ensure_compaction_proposal(&proposal).await?;

        let committed_at = ids::now();
        let turn = self
            .prepare_new_context_epoch(
                attempt_id,
                run_turn,
                expected_session_sequence,
                &model,
                candidate.request,
                descriptor.as_ref(),
                token_budget,
                durable_memory_limit,
                ContextEpochMode::Compaction {
                    epoch_id: candidate.epoch_id.clone(),
                    predecessor_epoch_id: candidate.predecessor_epoch_id.clone(),
                },
                Some(&candidate.history),
                committed_at,
            )
            .await?;
        if !exact_request_fits(turn.request(), token_budget)? {
            return Err(AppError::Memory(
                autoharness_memory::MemoryError::BudgetExceeded,
            ));
        }
        let epoch = turn
            .commit()
            .epoch()
            .cloned()
            .ok_or(AppError::Configuration)?;
        let facts = self
            .engine
            .load_compaction_facts_snapshot(epoch, turn.manifest().clone())
            .await?;
        if facts.epoch_id() != &candidate.epoch_id
            || facts.session_id() != &self.session_id
            || facts.expected_session_sequence() != expected_session_sequence
        {
            return Err(AppError::Configuration);
        }
        let boundary = ContextCompactionBoundary::new(
            candidate.epoch_id,
            candidate.predecessor_epoch_id,
            self.session_id.clone(),
            facts.expected_session_sequence(),
            facts.memory_generation(),
            facts.facts_version(),
            facts.facts_hash().clone(),
            facts.memory_fact_count(),
            facts.pending_session_fact_count(),
            Some(summary_revision_id),
            committed_at,
        );
        Ok(PreparedContextSnapshot {
            turn,
            compaction_boundary: Some(boundary),
        })
    }

    async fn load_frozen_context_baseline(
        &self,
        attempt_id: &AttemptId,
        run_turn: u32,
    ) -> Result<FrozenContextBaseline, AppError> {
        let prior_turn = run_turn.checked_sub(1).ok_or(AppError::Configuration)?;
        let prior = self
            .engine
            .load_attempt_context_turn(attempt_id.clone(), prior_turn)
            .await?
            .ok_or(AppError::Configuration)?;
        let epoch = self
            .engine
            .load_context_epoch(prior.epoch_id().clone())
            .await?
            .ok_or(AppError::Configuration)?;
        let turn = self
            .engine
            .load_context_epoch_baseline(epoch.epoch_id().clone())
            .await?
            .ok_or(AppError::Configuration)?;
        let content = self.load_frozen_context_content(&turn).await?;
        Ok(FrozenContextBaseline {
            epoch: ContextEpochMode::Existing(epoch),
            turn,
            content,
        })
    }

    async fn load_compacted_history(
        &self,
        checkpoint: &ContextCompactionCheckpoint,
    ) -> Result<CompactedHistoryV1, AppError> {
        let mut retained = None;
        for admission in checkpoint.baseline_turn().admissions() {
            if !is_compacted_history_admission(admission) {
                continue;
            }
            if retained.is_some() {
                return Err(AppError::Configuration);
            }
            let rendered = self
                .engine
                .load_context_admission_content(admission.admission_id().clone())
                .await?
                .ok_or(AppError::Configuration)?;
            retained = retained_compacted_history(admission, &rendered)?;
        }
        let retained = retained.ok_or(AppError::Configuration)?;
        if retained.cutoff_sequence() > checkpoint.boundary().expected_session_sequence().get() {
            return Err(AppError::Configuration);
        }
        Ok(retained)
    }

    #[allow(clippy::too_many_arguments)]
    async fn prepare_new_context_epoch(
        &self,
        attempt_id: &AttemptId,
        run_turn: u32,
        expected_session_sequence: SessionSequence,
        model: &autoharness_domain::ModelRef,
        request: ChatRequest,
        descriptor: Option<&ModelDescriptor>,
        token_budget: ContextTokenBudget,
        durable_memory_limit: EstimatedTokens,
        epoch: ContextEpochMode,
        compacted_history: Option<&CompactedHistoryV1>,
        committed_at: autoharness_domain::TimestampMillis,
    ) -> Result<PreparedContextTurn, AppError> {
        let retrieval_scope = self
            .context_scope()?
            .retrieval_scope(self.session_id.clone(), committed_at);
        let memory_query = attempt_memory_query(&self.session, attempt_id)?;
        let candidate_batch = self
            .engine
            .search_memory(MemorySearchQuery::new(
                memory_query.clone(),
                self.authorized_memory_scopes()?,
                retrieval_scope.sensitivity_ceiling,
                committed_at,
                MEMORY_CONTEXT_CANDIDATE_LIMIT,
            )?)
            .await?;
        let candidates = candidate_batch
            .candidates()
            .iter()
            .map(|candidate| memory_candidate(candidate, &memory_query))
            .collect::<Vec<_>>();
        let credential_sentinels = self
            .configured_credential_sentinels()
            .map_err(|_| AppError::CredentialRedactionUnavailable)?;
        if candidates.iter().any(|candidate| {
            self.value_contains_configured_secret(candidate.content.as_str(), &credential_sentinels)
        }) {
            return Err(AppError::Configuration);
        }
        let compatibility = EpochCompatibility::new(
            &request,
            descriptor,
            &retrieval_scope,
            token_budget,
            durable_memory_limit,
        )?;
        let retained_sources = self.retained_workspace_agents(attempt_id).await?;
        let configured_secrets = credential_sentinels
            .iter()
            .map(|secret| secret.as_str())
            .collect::<Vec<_>>();
        let mut observed_sources = observe_workspace_agents(
            &self.workspace,
            self.provider.as_deref(),
            &configured_secrets,
            committed_at,
            retained_sources,
        )?;
        if let Some(history) = compacted_history {
            observed_sources.push(observe_compacted_history(
                history,
                self.provider.as_deref(),
                &configured_secrets,
                committed_at,
            )?);
        }
        prepare_context_turn(ContextPreparationInput {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
            run_turn,
            expected_session_sequence,
            memory_generation: candidate_batch.generation(),
            model: model.clone(),
            request,
            retrieval_scope,
            compatibility,
            epoch,
            observed_sources,
            memory_candidates: candidates,
            committed_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn select_compaction_candidate(
        &self,
        attempt_id: &AttemptId,
        run_turn: u32,
        expected_session_sequence: SessionSequence,
        model: &autoharness_domain::ModelRef,
        advertise_tools: bool,
        descriptor: Option<&ModelDescriptor>,
        token_budget: ContextTokenBudget,
        durable_memory_limit: EstimatedTokens,
        checkpoint: Option<&ContextCompactionCheckpoint>,
        prior_history: Option<&CompactedHistoryV1>,
    ) -> Result<Option<CompactionCandidate>, AppError> {
        let prior_cutoff = prior_history.map(CompactedHistoryV1::cutoff_sequence);
        for cutoff in compaction_cutoffs(&self.session, prior_cutoff) {
            let request = build_request_after_cutoff(
                &self.session,
                attempt_id,
                advertise_tools,
                Some(cutoff),
            )?;
            let Some(predecessor_epoch_id) = self
                .compaction_predecessor_epoch(attempt_id, run_turn, cutoff, checkpoint)
                .await?
            else {
                continue;
            };
            let groups = compaction_groups(&self.session, prior_cutoff, cutoff)?;
            let history = match compact_history(
                &self.session_id,
                cutoff,
                prior_history,
                groups,
                MemoryContent::MAX_BYTES.min(MAX_CONTEXT_SOURCE_VALUE_BYTES),
            ) {
                Ok(history) => history,
                Err(AppError::Configuration) => continue,
                Err(error) => return Err(error),
            };
            let epoch_id = compaction_epoch_id(attempt_id, run_turn, &predecessor_epoch_id, cutoff);
            let preflight = self
                .prepare_new_context_epoch(
                    attempt_id,
                    run_turn,
                    expected_session_sequence,
                    model,
                    request.clone(),
                    descriptor,
                    token_budget,
                    durable_memory_limit,
                    ContextEpochMode::Compaction {
                        epoch_id: epoch_id.clone(),
                        predecessor_epoch_id: predecessor_epoch_id.clone(),
                    },
                    Some(&history),
                    ids::now(),
                )
                .await;
            match preflight {
                Ok(turn) if exact_request_fits(turn.request(), token_budget)? => {
                    return Ok(Some(CompactionCandidate {
                        history,
                        request,
                        predecessor_epoch_id,
                        epoch_id,
                    }));
                }
                Ok(_) | Err(AppError::Memory(autoharness_memory::MemoryError::BudgetExceeded)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    async fn compaction_predecessor_epoch(
        &self,
        attempt_id: &AttemptId,
        run_turn: u32,
        cutoff: SessionSequence,
        checkpoint: Option<&ContextCompactionCheckpoint>,
    ) -> Result<Option<ContextEpochId>, AppError> {
        if run_turn > 1 {
            let prior = self
                .engine
                .load_attempt_context_turn(
                    attempt_id.clone(),
                    run_turn.checked_sub(1).ok_or(AppError::Configuration)?,
                )
                .await?
                .ok_or(AppError::Configuration)?;
            return Ok(Some(prior.epoch_id().clone()));
        }
        if let Some(checkpoint) = checkpoint {
            return Ok(Some(checkpoint.epoch().epoch_id().clone()));
        }
        let mut attempts = self
            .session
            .attempts()
            .iter()
            .filter_map(|attempt| {
                attempt
                    .completed_sequence()
                    .filter(|completed| *completed <= cutoff)
                    .map(|completed| (completed, attempt))
            })
            .collect::<Vec<_>>();
        attempts.sort_by_key(|(completed, _)| std::cmp::Reverse(*completed));
        for (_, attempt) in attempts {
            let Some(binding) = attempt.context_turn_bindings().last() else {
                continue;
            };
            let Some(turn) = self
                .engine
                .load_attempt_context_turn(attempt.attempt_id().clone(), binding.run_turn())
                .await?
            else {
                continue;
            };
            if self
                .engine
                .load_context_epoch(turn.epoch_id().clone())
                .await?
                .is_some()
            {
                return Ok(Some(turn.epoch_id().clone()));
            }
        }
        Ok(None)
    }

    async fn build_compaction_proposal_command(
        &self,
        candidate: &CompactionCandidate,
    ) -> Result<MemoryCommandEnvelope, AppError> {
        let content = MemoryContent::new(candidate.history.content()?)
            .map_err(|_| AppError::Configuration)?;
        let content_hash = normalized_content_hash(content.as_str())?;
        let identity = format!(
            "{}\0{}\0{}\0{}",
            self.session_id.as_str(),
            candidate.predecessor_epoch_id.as_str(),
            candidate.history.cutoff_sequence(),
            content_hash.as_str(),
        );
        let cutoff = SessionSequence::new(candidate.history.cutoff_sequence())
            .map_err(|_| AppError::Configuration)?;
        let event = self
            .engine
            .load_events(self.session_id.clone())
            .await?
            .into_iter()
            .find(|event| event.sequence() == cutoff)
            .filter(|event| {
                matches!(
                    event.payload(),
                    autoharness_domain::EventPayload::AttemptCompleted { .. }
                )
            })
            .ok_or(AppError::Configuration)?;
        let evidence = MemoryEvidence::new(
            MemoryEvidenceId::new(deterministic_compaction_tag(
                "memory-compaction-evidence",
                &identity,
            ))
            .map_err(|_| AppError::Configuration)?,
            MemoryEvidenceSource::SessionEvent {
                session_id: self.session_id.clone(),
                event_id: event.event_id().clone(),
            },
            MemoryEvidenceRelation::DerivedFrom,
            None,
            None,
        )
        .map_err(|_| AppError::Configuration)?;
        let revision_id = MemoryRevisionId::new(deterministic_compaction_tag(
            "memory-compaction-revision",
            &identity,
        ))
        .map_err(|_| AppError::Configuration)?;
        let revision = MemoryRevisionDraft::new(
            revision_id,
            MemoryRevisionNumber::FIRST,
            None,
            content,
            content_hash,
            MemoryOrigin::Compaction,
            TrustClass::UntrustedProposal,
            ConfidenceBasisPoints::new(7_500).expect("static compaction confidence is valid"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            vec![evidence],
            Vec::new(),
        )
        .map_err(|_| AppError::Configuration)?;
        MemoryCommandEnvelope::new_v1(
            CommandId::new(deterministic_compaction_tag(
                "memory-compaction-command",
                &identity,
            ))
            .map_err(|_| AppError::Configuration)?,
            MemoryId::new(deterministic_compaction_tag("memory-compaction", &identity))
                .map_err(|_| AppError::Configuration)?,
            None,
            CorrelationId::new(deterministic_compaction_tag(
                "memory-compaction-correlation",
                &identity,
            ))
            .map_err(|_| AppError::Configuration)?,
            MemoryCommandPayload::CreateMemory {
                scope: DomainMemoryScope::Session(self.session_id.clone()),
                memory_kind: MemoryKind::Lesson,
                revision,
            },
        )
        .map_err(|_| AppError::Configuration)
    }

    async fn ensure_compaction_proposal(
        &self,
        command: &MemoryCommandEnvelope,
    ) -> Result<(), AppError> {
        if self
            .memory_command_contains_configured_secret(command)
            .map_err(|_| AppError::CredentialRedactionUnavailable)?
        {
            return Err(AppError::Configuration);
        }
        if let Err(error) = self.engine.execute_memory_command(command.clone()).await
            && !self.exact_memory_proposal_committed(command).await?
        {
            return Err(error);
        }
        if !self.exact_memory_proposal_committed(command).await? {
            return Err(AppError::Configuration);
        }
        Ok(())
    }

    async fn load_frozen_context_content(
        &self,
        baseline_turn: &autoharness_domain::ContextTurnManifest,
    ) -> Result<ContextTurnContent, AppError> {
        let prelude = self
            .engine
            .load_context_turn_content(baseline_turn.context_turn_id().clone())
            .await?;
        if baseline_turn.admissions().is_empty() != prelude.is_none() {
            return Err(AppError::Configuration);
        }
        let mut contents = Vec::with_capacity(baseline_turn.admissions().len());
        for admission in baseline_turn.admissions() {
            let rendered = self
                .engine
                .load_context_admission_content(admission.admission_id().clone())
                .await?
                .ok_or(AppError::Configuration)?;
            let memory_id = if let Some(revision_id) = admission.memory_revision_id() {
                let candidate = self
                    .engine
                    .load_memory_candidate(revision_id.clone())
                    .await?
                    .ok_or(AppError::Configuration)?;
                if !matches!(candidate.content(), MemoryContentState::Retained(_)) {
                    return Err(AppError::Configuration);
                }
                Some(candidate.memory_id().clone())
            } else {
                None
            };
            if !verify_admission_rendered_hash(admission, memory_id.as_ref(), rendered.as_str())? {
                return Err(AppError::Configuration);
            }
            contents.push(ContextAdmissionContent::new(
                admission.admission_id().clone(),
                rendered,
            ));
        }
        let content = ContextTurnContent::new(prelude, contents);
        let credentials = self
            .configured_credential_sentinels()
            .map_err(|_| AppError::CredentialRedactionUnavailable)?;
        if content.prelude().is_some_and(|prelude| {
            self.value_contains_configured_secret(prelude.as_str(), &credentials)
        }) || content.admissions().iter().any(|admission| {
            self.value_contains_configured_secret(admission.rendered().as_str(), &credentials)
        }) {
            return Err(AppError::Configuration);
        }
        Ok(content)
    }

    async fn retained_workspace_agents(
        &self,
        current_attempt_id: &AttemptId,
    ) -> Result<Vec<RetainedContextSource>, AppError> {
        for attempt in self.session.attempts().iter().rev() {
            let last_turn = if attempt.attempt_id() == current_attempt_id {
                attempt.turns_started()
            } else {
                attempt
                    .context_turn_bindings()
                    .last()
                    .map_or(0, autoharness_engine::ContextTurnBinding::run_turn)
            };
            if last_turn == 0 {
                continue;
            }
            for run_turn in (1..=last_turn).rev() {
                let Some(turn) = self
                    .engine
                    .load_attempt_context_turn(attempt.attempt_id().clone(), run_turn)
                    .await?
                else {
                    continue;
                };
                for admission in turn.admissions() {
                    if !is_workspace_agents_admission(admission) {
                        continue;
                    }
                    let rendered = self
                        .engine
                        .load_context_admission_content(admission.admission_id().clone())
                        .await?
                        .ok_or(AppError::Configuration)?;
                    let source = retained_workspace_agents(admission, &rendered)?
                        .ok_or(AppError::Configuration)?;
                    return Ok(vec![source]);
                }
            }
            // A newer attempt may have observed the optional source as absent.
            // Keep searching for the latest retained value so an unavailable
            // read never silently discards previously verified instructions.
        }
        Ok(Vec::new())
    }

    fn configured_credential_sentinels(
        &self,
    ) -> Result<Vec<Zeroizing<String>>, ProfileManagementError> {
        let Some(profiles) = &self.profiles else {
            return Ok(Vec::new());
        };
        let mut credentials = [
            profiles.environment.gemini.clone(),
            profiles.environment.router.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        credentials.extend(profiles.manager.configured_credentials_for_redaction()?);
        Ok(credentials)
    }

    async fn credential_ingress_guard(
        &self,
        attempt_id: &AttemptId,
    ) -> Result<CredentialIngressGuard, AppError> {
        let credentials = self
            .configured_credential_sentinels()
            .map_err(|_| AppError::CredentialRedactionUnavailable)?;
        let mut guard = CredentialIngressGuard::new(credentials);
        let events = self.engine.load_events(self.session_id.clone()).await?;
        let mut attempt_prepared = false;
        for event in events {
            match event.payload() {
                EventPayload::AttemptPrepared {
                    attempt_id: prepared,
                    ..
                } if prepared == attempt_id => attempt_prepared = true,
                EventPayload::AttemptTextAppended {
                    attempt_id: appended,
                    text,
                } if appended == attempt_id => {
                    if self.provider_value_contains_secret(text.as_str())
                        || guard.observe_provider_text(text.as_str())
                    {
                        return Err(AppError::Configuration);
                    }
                }
                EventPayload::ToolCallProposed {
                    attempt_id: proposed,
                    call,
                } if proposed == attempt_id => {
                    let arguments = call.arguments.to_value();
                    if self.provider_value_contains_secret(call.provider_call_id.as_str())
                        || self.provider_value_contains_secret(call.tool_name.as_str())
                        || self.json_value_contains_provider_secret(&arguments)
                        || guard.observe_tool_call(
                            call.provider_call_id.as_str(),
                            call.tool_name.as_str(),
                            &arguments,
                        )
                    {
                        return Err(AppError::Configuration);
                    }
                }
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    output,
                } if self
                    .session
                    .tool_call(tool_call_id)
                    .is_some_and(|call| call.attempt_id() == attempt_id)
                    && (self.provider_value_contains_secret(output.content())
                        || guard.contains_tool_output(output.content())) =>
                {
                    return Err(AppError::Configuration);
                }
                _ => {}
            }
        }
        if !attempt_prepared {
            return Err(AppError::Configuration);
        }
        Ok(guard)
    }

    fn value_contains_configured_secret(
        &self,
        value: &str,
        credentials: &[Zeroizing<String>],
    ) -> bool {
        value_contains_exact_credential(value, credentials)
            || self.provider_value_contains_secret(value)
    }

    fn provider_value_contains_secret(&self, value: &str) -> bool {
        self.provider
            .as_ref()
            .is_some_and(|provider| provider.redact_secrets(value) != value)
    }

    fn json_value_contains_provider_secret(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(value) => self.provider_value_contains_secret(value),
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| self.json_value_contains_provider_secret(value)),
            serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
                self.provider_value_contains_secret(key)
                    || self.json_value_contains_provider_secret(value)
            }),
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                false
            }
        }
    }

    fn redact_configured_secrets(&self, value: &str, credentials: &[Zeroizing<String>]) -> String {
        let mut ranges = credentials
            .iter()
            .filter(|secret| !secret.is_empty())
            .flat_map(|secret| {
                value
                    .match_indices(secret.as_str())
                    .map(move |(start, _)| (start, start + secret.len()))
            })
            .collect::<Vec<_>>();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            match merged.last_mut() {
                Some((_, prior_end)) if start <= *prior_end => {
                    *prior_end = (*prior_end).max(end);
                }
                _ => merged.push((start, end)),
            }
        }
        let mut redacted = String::with_capacity(value.len());
        let mut cursor = 0;
        for (start, end) in merged {
            redacted.push_str(&value[cursor..start]);
            redacted.push_str("[REDACTED]");
            cursor = end;
        }
        redacted.push_str(&value[cursor..]);
        match &self.provider {
            Some(provider) => provider.redact_secrets(&redacted),
            None => redacted,
        }
    }

    fn ensure_provider_request_is_credential_free(
        &self,
        request: &ChatRequest,
    ) -> Result<(), AppError> {
        let credentials = self
            .configured_credential_sentinels()
            .map_err(|_| AppError::CredentialRedactionUnavailable)?;
        let request = serde_json::to_value(request)?;
        if self.json_value_contains_configured_secret(&request, &credentials) {
            return Err(AppError::Configuration);
        }
        Ok(())
    }

    fn json_value_contains_configured_secret(
        &self,
        value: &serde_json::Value,
        credentials: &[Zeroizing<String>],
    ) -> bool {
        match value {
            serde_json::Value::String(value) => {
                self.value_contains_configured_secret(value, credentials)
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| self.json_value_contains_configured_secret(value, credentials)),
            serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
                self.value_contains_configured_secret(key, credentials)
                    || self.json_value_contains_configured_secret(value, credentials)
            }),
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                false
            }
        }
    }

    async fn fail_context_preparation(
        &mut self,
        attempt_id: &AttemptId,
    ) -> Result<(), DurableEngineError> {
        self.execute(CommandPayload::FailAttempt {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
            failure: context_preparation_failure(),
        })
        .await
    }

    async fn handle_async(&mut self, message: AsyncMessage) -> Result<(), AppError> {
        match message {
            AsyncMessage::Catalog {
                generation,
                request_id,
                result,
            } => self.handle_catalog(generation, request_id, result).await,
            AsyncMessage::Stream {
                attempt_id,
                result,
                benchmark_chunk_sequence,
            } => {
                self.handle_stream(attempt_id, result, benchmark_chunk_sequence)
                    .await
            }
            AsyncMessage::Tool {
                tool_call_id,
                result,
            } => self.handle_tool_result(tool_call_id, result).await,
            AsyncMessage::ProfileTest {
                profile_id,
                request_id,
                result,
            } => {
                self.handle_profile_test(profile_id, request_id, result)
                    .await
            }
            AsyncMessage::CodexLoginBrowserOpened { request_id } => {
                if self
                    .codex_login
                    .as_ref()
                    .is_some_and(|(active_request, _)| *active_request == request_id)
                {
                    self.ports
                        .notices
                        .send(UiNotice::CodexLoginBrowserOpened { request_id })
                        .await
                        .map_err(|_| AppError::WorkerStopped)?;
                }
                Ok(())
            }
            AsyncMessage::CodexLoginFinished { request_id, result } => {
                self.handle_codex_login_finished(request_id, result).await
            }
        }
    }

    async fn handle_profile_test(
        &mut self,
        profile_id: String,
        request_id: RequestId,
        result: Result<ModelCatalog, ProviderError>,
    ) -> Result<(), AppError> {
        let exists = self
            .profiles
            .as_ref()
            .and_then(|runtime| runtime.manager.snapshot().ok())
            .is_some_and(|snapshot| {
                snapshot
                    .profiles
                    .iter()
                    .any(|profile| profile.id.as_str() == profile_id)
            });
        if !exists {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Cancelled,
                    "The tested profile no longer exists",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        match result {
            Ok(catalog) if !catalog.is_stale() && !catalog.models().is_empty() => {
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime
                        .connection
                        .insert(profile_id, ProfileConnectionState::Ready);
                }
                self.publish_profiles();
                self.commit(request_id).await?;
            }
            Ok(_) => {
                let failure = UiFailure::new(
                    ErrorClass::Unavailable,
                    "The provider test did not return a live compatible model catalog",
                    RetryPolicy::Now,
                );
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.insert(
                        profile_id,
                        ProfileConnectionState::Failed(failure.message.clone()),
                    );
                }
                self.publish_profiles();
                self.reject(request_id, failure).await?;
            }
            Err(error) => {
                let failure = provider_failure(&error);
                if let Some(runtime) = self.profiles.as_mut() {
                    runtime.connection.insert(
                        profile_id,
                        ProfileConnectionState::Failed(failure.message.clone()),
                    );
                }
                self.publish_profiles();
                self.reject(request_id, failure).await?;
            }
        }
        Ok(())
    }

    async fn handle_catalog(
        &mut self,
        generation: u64,
        request_id: Option<RequestId>,
        result: Result<ModelCatalog, ProviderError>,
    ) -> Result<(), AppError> {
        if generation != self.catalog_generation {
            if let Some(request_id) = request_id {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Cancelled,
                        "The catalog refresh was superseded by a newer request",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
            }
            return Ok(());
        }
        self.catalog_cancellation = None;
        match result {
            Ok(catalog) => {
                let freshness = catalog.freshness();
                let stale = catalog.is_stale();
                let models = catalog.into_models();
                telemetry::catalog_ready(models.len());
                self.catalog_models = models.clone();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(projection::catalog(models, stale)));
                if self.session.selected_model().is_none()
                    && let Some(model) = self.active_profile_default_model()
                {
                    let _ = self
                        .execute(CommandPayload::SelectModel {
                            session_id: self.session_id.clone(),
                            model,
                        })
                        .await;
                }
                if freshness == CatalogFreshness::Live {
                    self.set_active_profile_connection(ProfileConnectionState::Ready);
                    self.maybe_resume_bound_turn_after_live_catalog().await;
                }
                self.publish_profiles();
                if let Some(request_id) = request_id {
                    self.commit(request_id).await?;
                }
            }
            Err(error) => {
                telemetry::catalog_failed(&error);
                if matches!(
                    error.kind(),
                    ProviderErrorKind::MissingCredential
                        | ProviderErrorKind::Authentication
                        | ProviderErrorKind::PermissionDenied
                ) {
                    self.provider = None;
                    self.session_credential_connected = false;
                }
                let failure = provider_failure(&error);
                self.set_active_profile_connection(ProfileConnectionState::Failed(
                    failure.message.clone(),
                ));
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Failed(failure.clone())));
                self.publish_profiles();
                if let Some(request_id) = request_id {
                    self.reject(request_id, failure).await?;
                }
            }
        }
        Ok(())
    }

    async fn maybe_resume_bound_turn_after_live_catalog(&mut self) {
        if self.active.is_some() {
            return;
        }
        if let Err(error) = self.resume_bound_turn_after_live_catalog().await {
            tracing::warn!(
                error = %error,
                "recovered provider turn did not match durable dispatch authority"
            );
        }
    }

    async fn resume_bound_turn_after_live_catalog(&mut self) -> Result<(), AppError> {
        let Some((attempt_id, model, run_turn)) = self.pending_bound_turn() else {
            return Ok(());
        };
        let provider = self.provider.as_ref().ok_or(AppError::Configuration)?;
        if provider.provider_id() != model.provider_id() {
            return Err(AppError::Configuration);
        }
        let descriptor = self
            .catalog_models
            .iter()
            .find(|descriptor| {
                descriptor.provider_id == *model.provider_id()
                    && descriptor.model_id == *model.model_id()
                    && descriptor.capabilities.supports_streamed_chat()
            })
            .ok_or(AppError::Configuration)?;
        let manifest = self
            .engine
            .load_attempt_context_turn(attempt_id.clone(), run_turn)
            .await?
            .ok_or(AppError::Configuration)?;
        if manifest.session_id() != &self.session_id
            || manifest.attempt_id() != &attempt_id
            || manifest.run_turn() != run_turn
            || manifest.model() != &model
        {
            return Err(AppError::Configuration);
        }
        let epoch = self
            .engine
            .load_context_epoch(manifest.epoch_id().clone())
            .await?
            .ok_or(AppError::Configuration)?;
        if epoch.session_id() != &self.session_id
            || epoch.epoch_id() != manifest.epoch_id()
            || epoch.memory_generation() != manifest.memory_generation()
        {
            return Err(AppError::Configuration);
        }

        let checkpoint = self
            .engine
            .load_latest_compaction_checkpoint(self.session_id.clone())
            .await?;
        let retained_history = match checkpoint.as_ref() {
            Some(checkpoint) => Some(self.load_compacted_history(checkpoint).await?),
            None => None,
        };
        let compacted_through = retained_history
            .as_ref()
            .map(CompactedHistoryV1::cutoff_sequence)
            .map(SessionSequence::new)
            .transpose()
            .map_err(|_| AppError::Configuration)?;
        let advertise_tools = descriptor.capabilities.supports_tool_calling();
        let request = build_request_after_cutoff(
            &self.session,
            &attempt_id,
            advertise_tools,
            compacted_through,
        )?;
        self.ensure_provider_request_is_credential_free(&request)?;
        let retrieval_scope = self
            .context_scope()?
            .retrieval_scope(self.session_id.clone(), epoch.started_at());
        let compatibility = EpochCompatibility::new(
            &request,
            Some(descriptor),
            &retrieval_scope,
            epoch.token_budget(),
            manifest.budget().durable_memory_limit(),
        )?;
        if compatibility.hashes() != epoch.hashes() {
            return Err(AppError::Configuration);
        }
        let content = self.load_frozen_context_content(&manifest).await?;
        let prelude = content.prelude().cloned();
        if !verify_rendered_context_hash(
            prelude.as_ref().map_or("", |value| value.as_str()),
            manifest.rendered_hash(),
        )? {
            return Err(AppError::Configuration);
        }
        let request = match prelude {
            Some(prelude) => {
                request.with_context(ContextPrelude::new(prelude.as_str().to_owned())?)
            }
            None => request,
        };
        self.ensure_provider_request_is_credential_free(&request)?;
        if exact_provider_request_hash(&request)? != *manifest.request_hash() {
            return Err(AppError::Configuration);
        }

        let attempt = self
            .session
            .attempt(&attempt_id)
            .ok_or(AppError::Configuration)?;
        let mut budget = restore_run_budget(&self.session, attempt)
            .map_err(|error| AppError::Provider(tool_provider_error(&error)))?;
        budget
            .start_turn()
            .map_err(|error| AppError::Provider(tool_provider_error(&error)))?;
        let usage_base = attempt.usage().unwrap_or_default();
        let credential_ingress = self.credential_ingress_guard(&attempt_id).await?;
        let reply = self
            .engine
            .start_recovered_run_turn(request.clone(), manifest)
            .await?;
        if reply.session.session_id() != &self.session_id {
            return Err(AppError::Configuration);
        }
        self.session = reply.session;
        self.ports
            .sessions
            .send_replace(Arc::new(projection::session(&self.session)));
        let cancellation = self.shutdown.child_token();
        self.spawn_stream(attempt_id.clone(), request, cancellation.clone(), None);
        self.active = Some(ActiveAttempt {
            attempt_id,
            cancellation,
            budget,
            usage_base,
            credential_ingress: Some(credential_ingress),
        });
        Ok(())
    }

    fn pending_bound_turn(&self) -> Option<(AttemptId, autoharness_domain::ModelRef, u32)> {
        self.session.attempts().iter().rev().find_map(|attempt| {
            if attempt.status() != EngineAttemptStatus::InFlight
                || !attempt.is_provider_dispatch_ready()
            {
                return None;
            }
            let binding = attempt.pending_context_turn()?;
            Some((
                attempt.attempt_id().clone(),
                attempt.model().clone(),
                binding.run_turn(),
            ))
        })
    }

    async fn fail_provider_credential_ingress(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), AppError> {
        if let Some(active) = &self.active
            && active.attempt_id == attempt_id
        {
            active.cancellation.cancel();
        }
        self.settle_attempt_tools_conservatively(&attempt_id)
            .await?;
        self.execute(CommandPayload::FailAttempt {
            session_id: self.session_id.clone(),
            attempt_id,
            failure: credential_ingress_failure(),
        })
        .await?;
        self.active = None;
        telemetry::attempt_settled("failed", None);
        Ok(())
    }

    async fn handle_stream(
        &mut self,
        attempt_id: AttemptId,
        result: Result<ProviderStreamEvent, ProviderError>,
        _benchmark_chunk_sequence: Option<u64>,
    ) -> Result<(), AppError> {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.attempt_id != attempt_id)
        {
            return Ok(());
        }
        let cancellation_requested = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| attempt.status() == EngineAttemptStatus::CancellationRequested);

        match result {
            Ok(ProviderStreamEvent::Started) => {}
            Ok(ProviderStreamEvent::TextDelta(delta)) => {
                let blocked = self.provider_value_contains_secret(delta.as_str())
                    || self
                        .active
                        .as_mut()
                        .and_then(|active| active.credential_ingress.as_mut())
                        .is_none_or(|guard| guard.observe_provider_text(delta.as_str()));
                if blocked {
                    self.fail_provider_credential_ingress(attempt_id).await?;
                    return Ok(());
                }
                let bytes = delta.as_str().len();
                if let Err(error) = self.add_run_output(bytes) {
                    self.fail_run_budget(attempt_id, error).await?;
                    return Ok(());
                }
                self.execute_and_publish(
                    CommandPayload::AppendAttemptText {
                        session_id: self.session_id.clone(),
                        attempt_id: attempt_id.clone(),
                        text: ResponseText::new(delta.as_str())
                            .expect("provider contract excludes empty deltas"),
                    },
                    _benchmark_chunk_sequence.map(|sequence| (attempt_id, sequence)),
                )
                .await?;
                telemetry::response_segment_committed(bytes);
            }
            Ok(ProviderStreamEvent::Usage(usage)) => {
                let input_tokens = usage.input_tokens;
                let output_tokens = usage.output_tokens;
                let cumulative = self.cumulative_usage(usage);
                let total_tokens = cumulative.total_tokens();
                if let Some(total) = total_tokens
                    && let Err(error) = self.record_run_tokens(total)
                {
                    self.fail_run_budget(attempt_id, error).await?;
                    return Ok(());
                }
                self.execute(CommandPayload::RecordAttemptUsage {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    usage: cumulative,
                })
                .await?;
                telemetry::usage_committed(input_tokens, output_tokens, total_tokens);
            }
            Ok(ProviderStreamEvent::ToolCall(call)) => {
                if !cancellation_requested {
                    let arguments = call.arguments.to_value();
                    let provider_blocked = self
                        .provider_value_contains_secret(call.provider_call_id.as_str())
                        || self.provider_value_contains_secret(call.tool_name.as_str())
                        || self.json_value_contains_provider_secret(&arguments);
                    let blocked = provider_blocked
                        || self
                            .active
                            .as_mut()
                            .and_then(|active| active.credential_ingress.as_mut())
                            .is_none_or(|guard| {
                                guard.observe_tool_call(
                                    call.provider_call_id.as_str(),
                                    call.tool_name.as_str(),
                                    &arguments,
                                )
                            });
                    if blocked {
                        self.fail_provider_credential_ingress(attempt_id).await?;
                        return Ok(());
                    }
                    self.handle_provider_tool_call(attempt_id, call).await?;
                }
            }
            Ok(ProviderStreamEvent::Completed {
                reason: autoharness_provider::CompletionReason::ToolCalls,
            }) => {
                if cancellation_requested {
                    if self.active_tool_calls_settled() {
                        self.execute(CommandPayload::CancelAttempt {
                            session_id: self.session_id.clone(),
                            attempt_id,
                        })
                        .await?;
                        self.active = None;
                    }
                    return Ok(());
                }
                self.execute(CommandPayload::PauseAttemptForTools {
                    session_id: self.session_id.clone(),
                    attempt_id,
                })
                .await?;
                self.maybe_resume_after_tools().await?;
            }
            Ok(ProviderStreamEvent::Completed { reason }) => {
                if self.attempt_has_unsettled_tool_calls(&attempt_id) {
                    self.settle_attempt_tools_conservatively(&attempt_id)
                        .await?;
                    self.execute(CommandPayload::FailAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id,
                        failure: completion_failure(
                            ErrorClass::Protocol,
                            "orphaned_tool_calls",
                            "The provider ended while tool calls were still unsettled",
                            RetryAdvice::Never,
                        ),
                    })
                    .await?;
                    self.active = None;
                    telemetry::attempt_settled("failed", None);
                    return Ok(());
                }
                telemetry::completion_observed(reason);
                let outcome = completion_outcome(reason);
                let payload = completion_payload(&self.session_id, attempt_id, reason);
                self.execute(payload).await?;
                self.active = None;
                telemetry::attempt_settled(outcome, None);
            }
            Ok(ProviderStreamEvent::Cancelled) if cancellation_requested => {
                if self.active_tool_calls_settled() {
                    self.execute(CommandPayload::CancelAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id,
                    })
                    .await?;
                    self.active = None;
                    telemetry::attempt_settled("cancelled", None);
                }
            }
            Ok(ProviderStreamEvent::Cancelled) => {
                self.settle_attempt_tools_conservatively(&attempt_id)
                    .await?;
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure: unsolicited_cancellation_failure(),
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled("provider_cancelled", None);
            }
            Err(error) if cancellation_requested => {
                if self.active_tool_calls_settled() {
                    self.execute(CommandPayload::CancelAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id,
                    })
                    .await?;
                    self.active = None;
                    telemetry::attempt_settled("cancelled", Some(&error));
                }
            }
            Err(error) => {
                self.settle_attempt_tools_conservatively(&attempt_id)
                    .await?;
                let (failure, outcome) = if error.kind() == ProviderErrorKind::Cancelled {
                    (unsolicited_cancellation_failure(), "provider_cancelled")
                } else {
                    (attempt_failure(&error), "failed")
                };
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure,
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled(outcome, Some(&error));
            }
        }
        Ok(())
    }

    async fn handle_provider_tool_call(
        &mut self,
        attempt_id: AttemptId,
        call: ProviderToolCall,
    ) -> Result<(), AppError> {
        let planned = match plan(IncomingToolCall {
            tool_call_id: ids::tool_call_id(),
            provider_call_id: call.provider_call_id,
            tool_name: call.tool_name,
            arguments: call.arguments,
        }) {
            Ok(planned) => planned,
            Err(error) => {
                self.settle_attempt_tools_conservatively(&attempt_id)
                    .await?;
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure: error.durable_failure(),
                })
                .await?;
                if let Some(active) = self.active.take() {
                    active.cancellation.cancel();
                }
                return Ok(());
            }
        };
        let outcome = self.tool_runtime.evaluate(&planned);
        if planned.spec().capability.kind == autoharness_domain::CapabilityKind::InvalidToolCall {
            telemetry::invalid_tool_call_rejected();
        }
        let tool_call_id = planned.spec().tool_call_id.clone();
        self.execute(CommandPayload::ProposeToolCall {
            session_id: self.session_id.clone(),
            attempt_id,
            call: planned.spec().clone(),
        })
        .await?;
        self.execute(CommandPayload::RecordToolPermission {
            session_id: self.session_id.clone(),
            tool_call_id: tool_call_id.clone(),
            decision_id: ids::permission_decision_id(),
            outcome,
        })
        .await?;
        match outcome {
            PermissionOutcome::Allow => self.start_tool_call(tool_call_id).await?,
            PermissionOutcome::Deny => {
                self.execute(CommandPayload::DenyToolCall {
                    session_id: self.session_id.clone(),
                    tool_call_id,
                })
                .await?;
            }
            PermissionOutcome::Ask => {}
        }
        Ok(())
    }

    async fn start_tool_call(&mut self, tool_call_id: ToolCallId) -> Result<(), AppError> {
        let call = self
            .session
            .tool_call(&tool_call_id)
            .cloned()
            .ok_or(AppError::Configuration)?;
        let replanned = replan(call.call().clone()).map_err(|_| AppError::Configuration)?;
        let memory_proposal = replanned.memory_proposal().cloned();
        let outcome = call
            .policy_decision()
            .map(|(_, outcome)| *outcome)
            .ok_or(AppError::Configuration)?;
        let answer = call.human_answer().map(|(_, answer)| *answer);
        let (authorized, _) = self
            .tool_runtime
            .authorize_replayed(call.call().clone(), outcome, answer)
            .map_err(|_| AppError::Configuration)?;
        self.execute(CommandPayload::StartToolCall {
            session_id: self.session_id.clone(),
            tool_call_id: tool_call_id.clone(),
        })
        .await?;
        let budget_result = self
            .active
            .as_mut()
            .ok_or(AppError::Configuration)?
            .budget
            .start_tool();
        if let Err(error) = budget_result {
            self.execute(CommandPayload::FailToolCall {
                session_id: self.session_id.clone(),
                tool_call_id,
                failure: error.durable_failure(),
            })
            .await?;
            return Ok(());
        }
        if let Some(proposal) = memory_proposal {
            self.finish_memory_proposal_tool(call, proposal).await?;
            return Ok(());
        }
        let runtime = Arc::clone(&self.tool_runtime);
        let messages = self.messages.clone();
        let cancellation = self
            .active
            .as_ref()
            .ok_or(AppError::Configuration)?
            .cancellation
            .clone();
        tokio::spawn(async move {
            let result = runtime.execute(authorized, cancellation).await;
            let _ = messages
                .send(AsyncMessage::Tool {
                    tool_call_id,
                    result,
                })
                .await;
        });
        Ok(())
    }

    async fn finish_memory_proposal_tool(
        &mut self,
        call: autoharness_engine::ToolCallProjection,
        proposal: MemoryProposal,
    ) -> Result<(), AppError> {
        let tool_call_id = call.call().tool_call_id.clone();
        let result = self.persist_memory_proposal(&call, &proposal).await;
        match result {
            Ok(()) => {
                self.finish_active_tool_budget(call.attempt_id());
                self.execute(CommandPayload::CompleteToolCall {
                    session_id: self.session_id.clone(),
                    tool_call_id,
                    output: ToolOutput::new(String::new(), None, 0, false)
                        .expect("empty proposal output is valid"),
                })
                .await?;
            }
            Err(MemoryProposalPersistenceError::Safe(error)) => {
                self.finish_active_tool_budget(call.attempt_id());
                self.execute(CommandPayload::FailToolCall {
                    session_id: self.session_id.clone(),
                    tool_call_id,
                    failure: error.durable_failure(),
                })
                .await?;
            }
            Err(MemoryProposalPersistenceError::Ambiguous(error)) => {
                return Err(error);
            }
        }
        self.maybe_resume_after_tools().await
    }

    fn finish_active_tool_budget(&mut self, attempt_id: &AttemptId) {
        if let Some(active) = &mut self.active
            && active.attempt_id == *attempt_id
        {
            active.budget.finish_tool();
        }
    }

    async fn persist_memory_proposal(
        &mut self,
        call: &autoharness_engine::ToolCallProjection,
        proposal: &MemoryProposal,
    ) -> Result<(), MemoryProposalPersistenceError> {
        let command = self
            .build_memory_proposal_command(call, proposal)
            .map_err(|error| MemoryProposalPersistenceError::Safe(error.into_tool_error()))?;
        match self.memory_command_contains_configured_secret(&command) {
            Ok(false) => {}
            Ok(true) => {
                return Err(MemoryProposalPersistenceError::Safe(
                    memory_proposal_invalid(),
                ));
            }
            Err(_) => {
                return Err(MemoryProposalPersistenceError::Safe(
                    memory_proposal_internal(),
                ));
            }
        }
        if let Err(error) = self.engine.execute_memory_command(command.clone()).await {
            match self.exact_memory_proposal_committed(&command).await {
                Ok(true) => {}
                Ok(false) => {
                    return Err(match error {
                        AppError::MemoryCommand(
                            crate::memory_runtime::MemoryCommandError::ValidationRejected
                            | crate::memory_runtime::MemoryCommandError::InvalidTransition
                            | crate::memory_runtime::MemoryCommandError::UnsupportedCommand
                            | crate::memory_runtime::MemoryCommandError::Policy(_),
                        ) => MemoryProposalPersistenceError::Safe(memory_proposal_invalid()),
                        AppError::MemoryCommand(
                            crate::memory_runtime::MemoryCommandError::VersionConflict,
                        ) => MemoryProposalPersistenceError::Safe(memory_proposal_internal()),
                        other => MemoryProposalPersistenceError::Ambiguous(other),
                    });
                }
                Err(_) => return Err(MemoryProposalPersistenceError::Ambiguous(error)),
            }
        }
        if let Err(error) = self.publish_memories().await {
            tracing::warn!(error = %error, "memory proposal committed but projection refresh failed");
        }
        Ok(())
    }

    fn build_memory_proposal_command(
        &self,
        call: &autoharness_engine::ToolCallProjection,
        proposal: &MemoryProposal,
    ) -> Result<
        autoharness_domain::MemoryCommandEnvelope,
        crate::proposal_runtime::ProposalBuildError,
    > {
        let workspace_id = self
            .context_scope
            .as_ref()
            .ok_or(crate::proposal_runtime::ProposalBuildError::Internal)?
            .workspace_id();
        crate::proposal_runtime::build_memory_proposal_command(
            &self.session,
            &self.session_id,
            workspace_id,
            self.artifact_root.as_deref(),
            call,
            proposal,
        )
    }

    async fn exact_memory_proposal_committed(
        &self,
        command: &autoharness_domain::MemoryCommandEnvelope,
    ) -> Result<bool, AppError> {
        let MemoryCommandPayload::CreateMemory { revision, .. } = command.payload() else {
            return Ok(false);
        };
        let operations = self
            .engine
            .load_memory_operations(command.memory_id().clone(), 0, 16)
            .await?;
        let content = self
            .engine
            .load_memory_content(revision.revision_id().clone())
            .await?;
        Ok(crate::proposal_runtime::exact_memory_proposal_committed(
            command,
            &operations,
            content.as_ref(),
        ))
    }

    async fn handle_tool_result(
        &mut self,
        tool_call_id: ToolCallId,
        result: Result<autoharness_domain::ToolOutput, ToolError>,
    ) -> Result<(), AppError> {
        let Some(call) = self.session.tool_call(&tool_call_id) else {
            return Ok(());
        };
        if call.status() != autoharness_engine::ToolCallStatus::Running {
            return Ok(());
        }
        let attempt_id = call.attempt_id().clone();
        if let Some(active) = &mut self.active
            && active.attempt_id == attempt_id
        {
            active.budget.finish_tool();
        }
        match result {
            Ok(output) => {
                let blocked = self.provider_value_contains_secret(output.content())
                    || self
                        .active
                        .as_ref()
                        .and_then(|active| active.credential_ingress.as_ref())
                        .is_none_or(|guard| guard.contains_tool_output(output.content()));
                if blocked {
                    tracing::warn!(
                        tool_call_id = %tool_call_id,
                        "local tool output was suppressed by the credential ingress boundary"
                    );
                    self.execute(CommandPayload::MarkToolCallUnknown {
                        session_id: self.session_id.clone(),
                        tool_call_id,
                    })
                    .await?;
                    self.maybe_resume_after_tools().await?;
                    return Ok(());
                }
                let bytes = usize::try_from(output.original_bytes()).unwrap_or(usize::MAX);
                if let Err(error) = self.add_run_output(bytes) {
                    self.execute(CommandPayload::MarkToolCallUnknown {
                        session_id: self.session_id.clone(),
                        tool_call_id,
                    })
                    .await?;
                    self.fail_run_budget(attempt_id, error).await?;
                    return Ok(());
                } else {
                    self.execute(CommandPayload::CompleteToolCall {
                        session_id: self.session_id.clone(),
                        tool_call_id,
                        output,
                    })
                    .await?;
                }
            }
            Err(_) => {
                self.execute(CommandPayload::MarkToolCallUnknown {
                    session_id: self.session_id.clone(),
                    tool_call_id,
                })
                .await?;
            }
        }
        if self.active.as_ref().is_some_and(|active| {
            self.session
                .attempt(&active.attempt_id)
                .is_some_and(|attempt| {
                    attempt.status() == EngineAttemptStatus::CancellationRequested
                })
        }) && self.active_tool_calls_settled()
        {
            let attempt_id = self
                .active
                .as_ref()
                .expect("active checked")
                .attempt_id
                .clone();
            self.execute(CommandPayload::CancelAttempt {
                session_id: self.session_id.clone(),
                attempt_id,
            })
            .await?;
            self.active = None;
            return Ok(());
        }
        self.maybe_resume_after_tools().await
    }

    async fn answer_permission(
        &mut self,
        request_id: RequestId,
        tool_call_key: ToolCallKey,
        allow: bool,
    ) -> Result<(), AppError> {
        let tool_call_id = match ToolCallId::new(tool_call_key.as_str()) {
            Ok(id) => id,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        if self.session.tool_call(&tool_call_id).is_none_or(|call| {
            call.status() != autoharness_engine::ToolCallStatus::PermissionPending
        }) {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "The permission request is no longer pending",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let answer = if allow {
            PermissionAnswer::AllowOnce
        } else {
            PermissionAnswer::Deny
        };
        self.execute(CommandPayload::AnswerToolPermission {
            session_id: self.session_id.clone(),
            tool_call_id: tool_call_id.clone(),
            decision_id: ids::permission_decision_id(),
            answer,
        })
        .await?;
        if allow {
            self.start_tool_call(tool_call_id).await?;
        } else {
            self.execute(CommandPayload::DenyToolCall {
                session_id: self.session_id.clone(),
                tool_call_id,
            })
            .await?;
        }
        self.commit(request_id).await?;
        self.maybe_resume_after_tools().await
    }

    async fn maybe_resume_after_tools(&mut self) -> Result<(), AppError> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        let attempt_id = active.attempt_id.clone();
        if self
            .session
            .attempt(&attempt_id)
            .is_none_or(|attempt| attempt.status() != EngineAttemptStatus::AwaitingTools)
        {
            return Ok(());
        }
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.credential_ingress.is_none())
        {
            let guard = match self.credential_ingress_guard(&attempt_id).await {
                Ok(guard) => guard,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "provider continuation credential ingress preparation failed closed"
                    );
                    self.settle_attempt_tools_conservatively(&attempt_id)
                        .await?;
                    self.fail_context_preparation(&attempt_id).await?;
                    self.active = None;
                    return Ok(());
                }
            };
            self.active
                .as_mut()
                .expect("active checked")
                .credential_ingress = Some(guard);
        }
        if self.provider.is_none() || !self.active_tool_calls_settled() {
            return Ok(());
        }
        if let Err(error) = self
            .active
            .as_mut()
            .expect("active checked")
            .budget
            .start_turn()
        {
            self.fail_run_budget(attempt_id, error).await?;
            return Ok(());
        }
        let usage_base = self
            .session
            .attempt(&attempt_id)
            .and_then(|attempt| attempt.usage())
            .unwrap_or_default();
        self.active.as_mut().expect("active checked").usage_base = usage_base;
        self.execute(CommandPayload::ResumeAttemptAfterTools {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
        })
        .await?;
        let advertise_tools = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| self.model_supports_tools(attempt.model()));
        let prepared = match self
            .prepare_and_bind_context(&attempt_id, advertise_tools)
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!(error = %error, "provider continuation context preparation failed closed");
                self.fail_context_preparation(&attempt_id).await?;
                self.active = None;
                return Ok(());
            }
        };
        if let Err(error) = self
            .execute(CommandPayload::StartRunTurn {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .await
        {
            tracing::warn!(error = %error, "provider continuation turn start failed closed");
            self.fail_context_preparation(&attempt_id).await?;
            self.active = None;
            return Ok(());
        }
        let cancellation = self
            .active
            .as_ref()
            .expect("active checked")
            .cancellation
            .clone();
        self.spawn_stream(attempt_id, prepared.request().clone(), cancellation, None);
        Ok(())
    }

    fn active_tool_calls_settled(&self) -> bool {
        let Some(active) = &self.active else {
            return true;
        };
        self.session
            .tool_calls()
            .iter()
            .filter(|call| call.attempt_id() == &active.attempt_id)
            .all(|call| call.status().is_settled())
    }

    fn attempt_has_unsettled_tool_calls(&self, attempt_id: &AttemptId) -> bool {
        self.session
            .tool_calls()
            .iter()
            .any(|call| call.attempt_id() == attempt_id && !call.status().is_settled())
    }

    async fn settle_attempt_tools_conservatively(
        &mut self,
        attempt_id: &AttemptId,
    ) -> Result<(), AppError> {
        if let Some(active) = &self.active
            && &active.attempt_id == attempt_id
        {
            active.cancellation.cancel();
        }
        let calls = self
            .session
            .tool_calls()
            .iter()
            .filter(|call| call.attempt_id() == attempt_id && !call.status().is_settled())
            .map(|call| (call.call().tool_call_id.clone(), call.status()))
            .collect::<Vec<_>>();
        for (tool_call_id, status) in calls {
            let payload = match status {
                autoharness_engine::ToolCallStatus::Running => {
                    CommandPayload::MarkToolCallUnknown {
                        session_id: self.session_id.clone(),
                        tool_call_id,
                    }
                }
                autoharness_engine::ToolCallStatus::DeniedPending => CommandPayload::DenyToolCall {
                    session_id: self.session_id.clone(),
                    tool_call_id,
                },
                autoharness_engine::ToolCallStatus::Proposed
                | autoharness_engine::ToolCallStatus::PermissionPending
                | autoharness_engine::ToolCallStatus::Authorized => {
                    CommandPayload::CancelToolCall {
                        session_id: self.session_id.clone(),
                        tool_call_id,
                    }
                }
                autoharness_engine::ToolCallStatus::Completed
                | autoharness_engine::ToolCallStatus::Failed
                | autoharness_engine::ToolCallStatus::Denied
                | autoharness_engine::ToolCallStatus::Cancelled
                | autoharness_engine::ToolCallStatus::Unknown => continue,
            };
            self.execute(payload).await?;
        }
        Ok(())
    }

    fn add_run_output(&mut self, bytes: usize) -> Result<(), ToolError> {
        self.active
            .as_mut()
            .ok_or_else(|| {
                ToolError::new(
                    autoharness_tool::ToolErrorKind::Internal,
                    RetryAdvice::Never,
                )
            })?
            .budget
            .add_output(u64::try_from(bytes).unwrap_or(u64::MAX))
    }

    fn record_run_tokens(&mut self, total: u64) -> Result<(), ToolError> {
        self.active
            .as_mut()
            .ok_or_else(|| {
                ToolError::new(
                    autoharness_tool::ToolErrorKind::Internal,
                    RetryAdvice::Never,
                )
            })?
            .budget
            .record_tokens(total)
    }

    fn cumulative_usage(&self, usage: autoharness_provider::UsageSnapshot) -> DomainUsage {
        let base = self
            .active
            .as_ref()
            .map_or_else(DomainUsage::default, |active| active.usage_base);
        DomainUsage::new(
            add_optional(base.input_tokens(), usage.input_tokens),
            add_optional(base.output_tokens(), usage.output_tokens),
            add_optional(base.total_tokens(), usage.total_tokens),
        )
        .with_breakdown(
            add_optional(base.cached_input_tokens(), usage.cached_input_tokens),
            add_optional(base.reasoning_tokens(), usage.reasoning_tokens),
            add_optional(base.tool_tokens(), usage.tool_tokens),
        )
    }

    async fn fail_run_budget(
        &mut self,
        attempt_id: AttemptId,
        error: ToolError,
    ) -> Result<(), AppError> {
        self.settle_attempt_tools_conservatively(&attempt_id)
            .await?;
        self.execute(CommandPayload::FailAttempt {
            session_id: self.session_id.clone(),
            attempt_id,
            failure: error.durable_failure(),
        })
        .await?;
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
        }
        Ok(())
    }

    async fn execute(&mut self, payload: CommandPayload) -> Result<(), DurableEngineError> {
        self.execute_and_publish(payload, None).await
    }

    async fn execute_and_publish(
        &mut self,
        payload: CommandPayload,
        _benchmark_chunk: Option<(AttemptId, u64)>,
    ) -> Result<(), DurableEngineError> {
        let reply = self.engine.execute(ids::command(payload)).await?;
        if reply.session.session_id() != &self.session_id {
            return Ok(());
        }
        self.session = reply.session;
        #[cfg(feature = "benchmark-instrumentation")]
        if let Some((attempt_id, chunk_sequence)) = _benchmark_chunk {
            let revision = self
                .session
                .last_sequence()
                .map_or(0, autoharness_domain::SessionSequence::get);
            autoharness_tui::benchmark::projection_committed(
                self.session_id.as_str(),
                revision,
                attempt_id.as_str(),
                chunk_sequence,
            );
        }
        self.ports
            .sessions
            .send_replace(Arc::new(projection::session(&self.session)));
        Ok(())
    }

    fn refresh_catalog(&mut self, request_id: Option<RequestId>) {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        if let Some(cancellation) = self.catalog_cancellation.take() {
            cancellation.cancel();
        }
        self.catalog_generation = self.catalog_generation.saturating_add(1);
        let generation = self.catalog_generation;
        let cancellation = self.shutdown.child_token();
        self.catalog_cancellation = Some(cancellation.clone());
        telemetry::catalog_refresh_started(generation, request_id.is_some());
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let request = if request_id.is_some() {
                CatalogRequest::Refresh
            } else {
                CatalogRequest::PreferCache
            };
            let result = provider.list_models(request, cancellation).await;
            let _ = messages
                .send(AsyncMessage::Catalog {
                    generation,
                    request_id,
                    result,
                })
                .await;
        });
    }

    fn model_is_available(&self, model: &autoharness_domain::ModelRef) -> bool {
        self.catalog_models.iter().any(|descriptor| {
            descriptor.provider_id == *model.provider_id()
                && descriptor.model_id == *model.model_id()
                && descriptor.capabilities.supports_streamed_chat()
        })
    }

    fn model_supports_tools(&self, model: &autoharness_domain::ModelRef) -> bool {
        self.catalog_models.iter().any(|descriptor| {
            descriptor.provider_id == *model.provider_id()
                && descriptor.model_id == *model.model_id()
                && descriptor.capabilities.supports_tool_calling()
        })
    }

    fn spawn_stream(
        &self,
        attempt_id: AttemptId,
        request: ChatRequest,
        cancellation: CancellationToken,
        _benchmark_request_id: Option<RequestId>,
    ) {
        let provider = Arc::clone(
            self.provider
                .as_ref()
                .expect("provider checked before attempt"),
        );
        let messages = self.messages.clone();
        tokio::spawn(async move {
            #[cfg(feature = "benchmark-instrumentation")]
            if let Some(request_id) = _benchmark_request_id {
                autoharness_tui::benchmark::provider_dispatch_started(
                    request_id,
                    attempt_id.as_str(),
                );
            }
            match provider.stream_chat(request, cancellation).await {
                Ok(mut stream) => {
                    while let Some(result) = stream.next().await {
                        #[cfg(feature = "benchmark-instrumentation")]
                        let benchmark_chunk_sequence = match &result {
                            Ok(ProviderStreamEvent::TextDelta(delta)) => {
                                autoharness_tui::benchmark::provider_chunk_received(
                                    attempt_id.as_str(),
                                    delta.as_str().len(),
                                )
                            }
                            _ => None,
                        };
                        #[cfg(not(feature = "benchmark-instrumentation"))]
                        let benchmark_chunk_sequence = None;
                        let terminal = match &result {
                            Err(_) => true,
                            Ok(event) => matches!(
                                event,
                                ProviderStreamEvent::Completed { .. }
                                    | ProviderStreamEvent::Cancelled
                            ),
                        };
                        if messages
                            .send(AsyncMessage::Stream {
                                attempt_id: attempt_id.clone(),
                                result,
                                benchmark_chunk_sequence,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        if terminal {
                            return;
                        }
                    }
                    let _ = messages
                        .send(AsyncMessage::Stream {
                            attempt_id,
                            result: Err(ProviderError::new(
                                ProviderErrorKind::Protocol,
                                RetryAdvice::Never,
                            )),
                            benchmark_chunk_sequence: None,
                        })
                        .await;
                }
                Err(error) => {
                    let _ = messages
                        .send(AsyncMessage::Stream {
                            attempt_id,
                            result: Err(error),
                            benchmark_chunk_sequence: None,
                        })
                        .await;
                }
            }
        });
    }

    async fn commit(&self, request_id: RequestId) -> Result<(), AppError> {
        self.ports
            .notices
            .send(UiNotice::IntentCommitted { request_id })
            .await
            .map_err(|_| AppError::WorkerStopped)
    }

    async fn reject(&self, request_id: RequestId, failure: UiFailure) -> Result<(), AppError> {
        self.ports
            .notices
            .send(UiNotice::IntentRejected {
                request_id,
                failure,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)
    }
}

fn value_contains_exact_credential(value: &str, credentials: &[Zeroizing<String>]) -> bool {
    credentials
        .iter()
        .any(|credential| !credential.is_empty() && value.contains(credential.as_str()))
}

fn json_value_contains_exact_credential(
    value: &serde_json::Value,
    credentials: &[Zeroizing<String>],
) -> bool {
    match value {
        serde_json::Value::String(value) => value_contains_exact_credential(value, credentials),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_value_contains_exact_credential(value, credentials)),
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            value_contains_exact_credential(key, credentials)
                || json_value_contains_exact_credential(value, credentials)
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

fn attempt_memory_query(
    session: &SessionAggregate,
    attempt_id: &AttemptId,
) -> Result<MemoryContent, AppError> {
    let attempt = session.attempt(attempt_id).ok_or(AppError::Configuration)?;
    let prompt = session
        .admitted_inputs()
        .iter()
        .find(|input| input.input_id() == attempt.input_id())
        .ok_or(AppError::Configuration)?
        .prompt()
        .as_str();
    let mut end = prompt.len().min(MemoryContent::MAX_BYTES);
    while !prompt.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    MemoryContent::new(prompt[..end].to_owned()).map_err(|_| AppError::Configuration)
}

const fn memory_command_draft(payload: &MemoryCommandPayload) -> Option<&MemoryRevisionDraft> {
    match payload {
        MemoryCommandPayload::CreateMemory { revision, .. }
        | MemoryCommandPayload::ProposeRevision { revision, .. }
        | MemoryCommandPayload::ReviseMemory { revision, .. } => Some(revision),
        MemoryCommandPayload::ApproveProposal {
            approved_revision, ..
        } => Some(approved_revision),
        MemoryCommandPayload::RecordValidation { .. }
        | MemoryCommandPayload::ActivateRevision { .. }
        | MemoryCommandPayload::RejectRevision { .. }
        | MemoryCommandPayload::RetractMemory { .. }
        | MemoryCommandPayload::DeleteMemory { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn user_memory_draft(
    revision: MemoryRevisionNumber,
    subject_key: Option<autoharness_domain::MemorySubjectKey>,
    content: String,
    confidence: ConfidenceBasisPoints,
    sensitivity: Sensitivity,
    validity: MemoryValidity,
    relations: Vec<autoharness_domain::MemoryRelation>,
) -> Result<MemoryRevisionDraft, AppError> {
    let content = MemoryContent::new(content).map_err(|_| AppError::Configuration)?;
    let content_hash = normalized_content_hash(content.as_str())?;
    MemoryRevisionDraft::new(
        ids::memory_revision_id(),
        revision,
        subject_key,
        content,
        content_hash,
        MemoryOrigin::ExplicitUser,
        TrustClass::UserApproved,
        confidence,
        sensitivity,
        validity,
        Vec::new(),
        relations,
    )
    .map_err(|_| AppError::Configuration)
}

fn next_memory_revision(revisions: &[MemoryRevision]) -> Result<MemoryRevisionNumber, AppError> {
    let next = revisions
        .last()
        .map_or(1, |revision| revision.revision().get().saturating_add(1));
    MemoryRevisionNumber::new(next).map_err(|_| AppError::Configuration)
}

fn context_snapshot_conflict(error: &DurableEngineError) -> bool {
    matches!(
        error,
        DurableEngineError::Store(
            autoharness_store::StoreError::VersionConflict { .. }
                | autoharness_store::StoreError::ContextGenerationConflict { .. }
        )
    )
}

fn context_token_budget(
    descriptor: Option<&ModelDescriptor>,
) -> Result<ContextTokenBudget, AppError> {
    let reported_token_limit = descriptor
        .and_then(|descriptor| descriptor.input_token_limit)
        .unwrap_or(DEFAULT_CONTEXT_TOKEN_BUDGET);
    ContextTokenBudget::new(reported_token_limit.saturating_mul(CONTEXT_SIZER_BYTES_PER_TOKEN))
        .map_err(|_| AppError::Configuration)
}

fn exact_request_fits(
    request: &ChatRequest,
    token_budget: ContextTokenBudget,
) -> Result<bool, AppError> {
    let bytes =
        u64::try_from(serde_json::to_vec(request)?.len()).map_err(|_| AppError::Configuration)?;
    Ok(bytes <= token_budget.get())
}

fn compaction_cutoffs(
    session: &SessionAggregate,
    compacted_through: Option<u64>,
) -> Vec<SessionSequence> {
    let mut cutoffs = session
        .attempts()
        .iter()
        .filter_map(autoharness_engine::AttemptProjection::completed_sequence)
        .filter(|completed| compacted_through.is_none_or(|cutoff| completed.get() > cutoff))
        .collect::<Vec<_>>();
    cutoffs.sort();
    cutoffs.dedup();
    cutoffs
}

fn compaction_groups(
    session: &SessionAggregate,
    compacted_through: Option<u64>,
    cutoff: SessionSequence,
) -> Result<Vec<CompactedHistoryGroup>, AppError> {
    let mut attempts = session
        .attempts()
        .iter()
        .filter_map(|attempt| {
            attempt
                .completed_sequence()
                .filter(|completed| {
                    *completed <= cutoff
                        && compacted_through.is_none_or(|prior| completed.get() > prior)
                })
                .map(|completed| (completed, attempt))
        })
        .collect::<Vec<_>>();
    attempts.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.attempt_id().cmp(right.1.attempt_id()))
    });
    attempts
        .into_iter()
        .map(|(completed, attempt)| {
            let messages = complete_attempt_messages(session, attempt)?;
            let group = CompactedHistoryGroup::new(attempt.attempt_id(), completed, &messages)?;
            if group.completed_sequence() != completed.get() {
                return Err(AppError::Configuration);
            }
            Ok(group)
        })
        .collect()
}

fn deterministic_compaction_tag(prefix: &str, identity: &str) -> String {
    use sha2::{Digest as _, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(b"autoharness-compaction-id-v1\0");
    hasher.update(prefix.as_bytes());
    hasher.update(b"\0");
    hasher.update(identity.as_bytes());
    let encoded = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{encoded}")
}

fn compaction_proposal_revision(
    command: &MemoryCommandEnvelope,
) -> Result<&MemoryRevisionId, AppError> {
    match command.payload() {
        MemoryCommandPayload::CreateMemory { revision, .. }
            if revision.origin() == MemoryOrigin::Compaction
                && revision.trust_class() == TrustClass::UntrustedProposal =>
        {
            Ok(revision.revision_id())
        }
        _ => Err(AppError::Configuration),
    }
}

fn memory_candidate(
    candidate: &autoharness_store::MemorySearchCandidate,
    query: &MemoryContent,
) -> MemoryCandidate {
    let revision = candidate.revision();
    let conflicted = revision
        .relations()
        .iter()
        .any(|relation| relation.kind() == MemoryRelationKind::Contradicts);
    let exact_match = candidate
        .content()
        .as_str()
        .trim()
        .eq_ignore_ascii_case(query.as_str().trim());
    MemoryCandidate {
        memory_id: candidate.memory_id().clone(),
        revision_id: revision.revision_id().clone(),
        status: revision.status(),
        scope: candidate.scope().clone(),
        kind: candidate.memory_kind(),
        trust: revision.trust_class(),
        confidence: revision.confidence(),
        sensitivity: revision.sensitivity(),
        validity: revision.validity(),
        content: candidate.content().clone(),
        content_hash: revision.content_hash().clone(),
        created_at: revision.created_at(),
        exact_match,
        lexical_basis_points: 10_000_u16.saturating_sub(
            u16::try_from(candidate.fts_rank())
                .unwrap_or(u16::MAX)
                .saturating_mul(250),
        ),
        conflicted,
    }
}

fn build_request(
    session: &SessionAggregate,
    attempt_id: &AttemptId,
    advertise_tools: bool,
) -> Result<ChatRequest, ProviderError> {
    build_request_after_cutoff(session, attempt_id, advertise_tools, None)
}

fn build_request_after_cutoff(
    session: &SessionAggregate,
    attempt_id: &AttemptId,
    advertise_tools: bool,
    compacted_through: Option<autoharness_domain::SessionSequence>,
) -> Result<ChatRequest, ProviderError> {
    let attempt = session
        .attempt(attempt_id)
        .ok_or_else(|| ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never))?;
    let is_retained_completed_attempt = |candidate: &autoharness_engine::AttemptProjection| {
        candidate.status() == EngineAttemptStatus::Completed
            && candidate
                .completed_sequence()
                .is_some_and(|completed| compacted_through.is_none_or(|cutoff| completed > cutoff))
    };
    let mut messages = Vec::new();
    for input in session.admitted_inputs().iter().filter(|input| {
        input.promoted_by().is_some()
            && (input.input_id() == attempt.input_id()
                || session.attempts().iter().any(|candidate| {
                    candidate.input_id() == input.input_id()
                        && is_retained_completed_attempt(candidate)
                }))
    }) {
        messages.push(ChatMessage::text(
            ChatRole::User,
            ChatContent::new(input.prompt().as_str())?,
        ));
        for response in session.attempts().iter().filter(|candidate| {
            candidate.input_id() == input.input_id()
                && (is_retained_completed_attempt(candidate)
                    || (candidate.attempt_id() == attempt_id
                        && session
                            .tool_calls()
                            .iter()
                            .any(|call| call.attempt_id() == attempt_id)))
        }) {
            append_attempt_output(session, response, &mut messages)?;
        }
    }
    let tools = definitions()
        .into_iter()
        .map(|definition| {
            ProviderToolDefinition::new_v1(
                definition.name,
                definition.description,
                definition.parameters,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    ChatRequest::new(attempt.model().model_id().clone(), messages).map(|request| {
        if advertise_tools {
            request.with_tools(tools)
        } else {
            request
        }
    })
}

fn append_attempt_output(
    session: &SessionAggregate,
    attempt: &autoharness_engine::AttemptProjection,
    messages: &mut Vec<ChatMessage>,
) -> Result<(), ProviderError> {
    let text = attempt.response_text();
    if !text.trim().is_empty() {
        messages.push(ChatMessage::text(
            ChatRole::Assistant,
            ChatContent::new(text)?,
        ));
    }
    for tool_call in session
        .tool_calls()
        .iter()
        .filter(|call| call.attempt_id() == attempt.attempt_id())
    {
        messages.push(ChatMessage::ToolCall(ProviderToolCall {
            provider_call_id: tool_call.call().provider_call_id.clone(),
            tool_name: tool_call.call().tool_name.clone(),
            arguments: tool_call.call().arguments.clone(),
        }));
        if tool_call.status().is_settled() {
            messages.push(ChatMessage::ToolResult {
                provider_call_id: tool_call.call().provider_call_id.clone(),
                tool_name: tool_call.call().tool_name.clone(),
                content: ChatContent::new(tool_result_content(tool_call))?,
            });
        }
    }
    Ok(())
}

fn complete_attempt_messages(
    session: &SessionAggregate,
    attempt: &autoharness_engine::AttemptProjection,
) -> Result<Vec<ChatMessage>, AppError> {
    if attempt.status() != EngineAttemptStatus::Completed || attempt.completed_sequence().is_none()
    {
        return Err(AppError::Configuration);
    }
    let input = session
        .admitted_inputs()
        .iter()
        .find(|input| input.input_id() == attempt.input_id() && input.promoted_by().is_some())
        .ok_or(AppError::Configuration)?;
    let mut messages = vec![ChatMessage::text(
        ChatRole::User,
        ChatContent::new(input.prompt().as_str())?,
    )];
    append_attempt_output(session, attempt, &mut messages)?;
    Ok(messages)
}

fn recover_active_attempt(
    session: &SessionAggregate,
    shutdown: &CancellationToken,
) -> Option<ActiveAttempt> {
    let attempt = session
        .attempts()
        .iter()
        .rev()
        .find(|attempt| attempt.status() == EngineAttemptStatus::AwaitingTools)?;
    let budget = restore_run_budget(session, attempt).ok()?;
    Some(ActiveAttempt {
        attempt_id: attempt.attempt_id().clone(),
        cancellation: shutdown.child_token(),
        budget,
        usage_base: attempt.usage().unwrap_or_default(),
        credential_ingress: None,
    })
}

fn restore_run_budget(
    session: &SessionAggregate,
    attempt: &autoharness_engine::AttemptProjection,
) -> Result<RunBudget, ToolError> {
    let limits = attempt.run_limits().unwrap_or_default();
    let tokens = attempt
        .usage()
        .and_then(|usage| usage.total_tokens())
        .unwrap_or(0);
    let mut output_bytes = u64::try_from(attempt.response_text().len()).unwrap_or(u64::MAX);
    for output in session
        .tool_calls()
        .iter()
        .filter(|call| call.attempt_id() == attempt.attempt_id())
        .filter_map(autoharness_engine::ToolCallProjection::output)
    {
        output_bytes = output_bytes.saturating_add(output.original_bytes());
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        });
    let started_ms = attempt
        .started_at()
        .and_then(|timestamp| u64::try_from(timestamp.get()).ok())
        .unwrap_or(now_ms);
    let elapsed = std::time::Duration::from_millis(now_ms.saturating_sub(started_ms));
    RunBudget::restore(
        limits,
        elapsed,
        attempt.turns_started(),
        tokens,
        output_bytes,
        0,
    )
}

fn exact_provider_request_hash(request: &ChatRequest) -> Result<Sha256Digest, AppError> {
    use sha2::{Digest as _, Sha256};

    let digest = Sha256::digest(serde_json::to_vec(request)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Sha256Digest::new(digest).map_err(|_| AppError::Configuration)
}

fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn tool_provider_error(error: &ToolError) -> ProviderError {
    let kind = match error.kind() {
        autoharness_tool::ToolErrorKind::Timeout => ProviderErrorKind::Timeout,
        autoharness_tool::ToolErrorKind::Cancelled => ProviderErrorKind::Cancelled,
        autoharness_tool::ToolErrorKind::PermissionDenied => ProviderErrorKind::PermissionDenied,
        autoharness_tool::ToolErrorKind::InvalidCall
        | autoharness_tool::ToolErrorKind::OutputLimit
        | autoharness_tool::ToolErrorKind::TurnLimit => ProviderErrorKind::LimitExceeded,
        autoharness_tool::ToolErrorKind::Filesystem
        | autoharness_tool::ToolErrorKind::Process
        | autoharness_tool::ToolErrorKind::Http
        | autoharness_tool::ToolErrorKind::Artifact => ProviderErrorKind::Unavailable,
        autoharness_tool::ToolErrorKind::MemoryProposalSinkRequired
        | autoharness_tool::ToolErrorKind::Internal => ProviderErrorKind::Internal,
    };
    ProviderError::new(kind, error.retry_advice())
}

fn memory_proposal_invalid() -> ToolError {
    ToolError::new(
        autoharness_tool::ToolErrorKind::InvalidCall,
        RetryAdvice::Never,
    )
}

fn memory_proposal_internal() -> ToolError {
    ToolError::new(
        autoharness_tool::ToolErrorKind::Internal,
        RetryAdvice::Never,
    )
}

#[cfg(test)]
fn raw_sha256(bytes: &[u8]) -> Result<autoharness_domain::Sha256Digest, ToolError> {
    use sha2::{Digest as _, Sha256};

    let digest = Sha256::digest(bytes);
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    autoharness_domain::Sha256Digest::new(encoded).map_err(|_| memory_proposal_internal())
}

#[cfg(test)]
fn test_tool_runtime() -> Arc<ToolRuntime> {
    let root = tempfile::tempdir().expect("tool test directory").keep();
    test_tool_runtime_at(&root)
}

#[cfg(test)]
fn test_tool_runtime_at(root: &std::path::Path) -> Arc<ToolRuntime> {
    use autoharness_tool::{
        FileArtifactStore, LocalFilesystem, LocalHttp, LocalProcess, PermissionPolicy,
    };

    let artifacts = root.join("artifacts");
    Arc::new(
        ToolRuntime::new(
            Arc::new(LocalFilesystem::new(root, 1024 * 1024).expect("filesystem")),
            Arc::new(LocalProcess::new(root, 1024 * 1024).expect("process")),
            Arc::new(LocalHttp::new(1024 * 1024).expect("HTTP")),
            Arc::new(FileArtifactStore::new(artifacts).expect("artifacts")),
            PermissionPolicy::local_default(),
            2,
            std::time::Duration::from_secs(10),
            64 * 1024,
        )
        .expect("tool runtime"),
    )
}

fn tool_result_content(call: &autoharness_engine::ToolCallProjection) -> String {
    match call.status() {
        autoharness_engine::ToolCallStatus::Completed
            if call.call().capability.kind
                == autoharness_domain::CapabilityKind::MemoryProposal =>
        {
            "Memory proposal recorded for review".to_owned()
        }
        autoharness_engine::ToolCallStatus::Completed => call.output().map_or_else(
            || "Tool completed without output".to_owned(),
            |output| output.content().to_owned(),
        ),
        autoharness_engine::ToolCallStatus::Failed => call.failure().map_or_else(
            || "Tool failed".to_owned(),
            |failure| format!("Tool failed: {}", failure.message().as_str()),
        ),
        autoharness_engine::ToolCallStatus::Denied
            if call.call().capability.kind
                == autoharness_domain::CapabilityKind::InvalidToolCall =>
        {
            "Tool call rejected: use only an advertised tool with its exact argument schema"
                .to_owned()
        }
        autoharness_engine::ToolCallStatus::Denied => "Tool permission was denied".to_owned(),
        autoharness_engine::ToolCallStatus::Cancelled => "Tool execution was cancelled".to_owned(),
        autoharness_engine::ToolCallStatus::Unknown => {
            "Tool outcome is unknown after interruption and was not retried".to_owned()
        }
        autoharness_engine::ToolCallStatus::Proposed
        | autoharness_engine::ToolCallStatus::PermissionPending
        | autoharness_engine::ToolCallStatus::Authorized
        | autoharness_engine::ToolCallStatus::DeniedPending
        | autoharness_engine::ToolCallStatus::Running => "Tool result is not settled".to_owned(),
    }
}

fn attempt_failure(error: &ProviderError) -> AttemptFailure {
    AttemptFailure::new(
        error.class(),
        ErrorCode::new(provider_code(error.kind())).expect("provider error codes are valid"),
        PublicMessage::new(error.to_string()).expect("provider errors have safe messages"),
        error.retry_advice(),
    )
}

fn context_preparation_failure() -> AttemptFailure {
    AttemptFailure::new(
        ErrorClass::Storage,
        ErrorCode::new("context_not_committed").expect("static context failure code is valid"),
        PublicMessage::new(
            "The provider request was not sent because its context was not durably committed",
        )
        .expect("static context failure message is valid"),
        RetryAdvice::Immediate,
    )
}

fn credential_ingress_failure() -> AttemptFailure {
    AttemptFailure::new(
        ErrorClass::Protocol,
        ErrorCode::new("credential_in_provider_data")
            .expect("static credential ingress code is valid"),
        PublicMessage::new(
            "The provider response was stopped before protected credential material was saved",
        )
        .expect("static credential ingress message is valid"),
        RetryAdvice::Never,
    )
}

fn completion_payload(
    session_id: &SessionId,
    attempt_id: AttemptId,
    reason: autoharness_provider::CompletionReason,
) -> CommandPayload {
    use autoharness_provider::CompletionReason;

    match reason {
        CompletionReason::Stop => CommandPayload::CompleteAttempt {
            session_id: session_id.clone(),
            attempt_id,
        },
        CompletionReason::Length => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::Validation,
                "generation_limit",
                "The response stopped at the provider generation limit",
                RetryAdvice::Immediate,
            ),
        },
        CompletionReason::Safety => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::PermissionDenied,
                "safety_stop",
                "The provider stopped the response for safety policy",
                RetryAdvice::Never,
            ),
        },
        CompletionReason::Recitation => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::PermissionDenied,
                "recitation_stop",
                "The provider stopped the response for recitation policy",
                RetryAdvice::Never,
            ),
        },
        CompletionReason::Other => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::Protocol,
                "provider_stop",
                "The provider stopped the response without a normal completion",
                RetryAdvice::Never,
            ),
        },
        CompletionReason::ToolCalls => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::Internal,
                "unhandled_tool_completion",
                "The provider tool turn could not be continued",
                RetryAdvice::Never,
            ),
        },
    }
}

const fn completion_outcome(reason: autoharness_provider::CompletionReason) -> &'static str {
    use autoharness_provider::CompletionReason;

    match reason {
        CompletionReason::Stop => "completed",
        CompletionReason::Length => "generation_limit",
        CompletionReason::Safety => "safety_stop",
        CompletionReason::Recitation => "recitation_stop",
        CompletionReason::Other => "provider_stop",
        CompletionReason::ToolCalls => "tool_calls",
    }
}

fn completion_failure(
    class: ErrorClass,
    code: &'static str,
    message: &'static str,
    retry_advice: RetryAdvice,
) -> AttemptFailure {
    AttemptFailure::new(
        class,
        ErrorCode::new(code).expect("static completion code is valid"),
        PublicMessage::new(message).expect("static completion message is valid"),
        retry_advice,
    )
}

fn unsolicited_cancellation_failure() -> AttemptFailure {
    completion_failure(
        ErrorClass::Cancelled,
        "provider_cancelled",
        "The provider cancelled the attempt before completion",
        RetryAdvice::Immediate,
    )
}

fn provider_code(kind: ProviderErrorKind) -> &'static str {
    match kind {
        ProviderErrorKind::MissingCredential => "missing_credential",
        ProviderErrorKind::Authentication => "authentication",
        ProviderErrorKind::PermissionDenied => "permission_denied",
        ProviderErrorKind::InvalidRequest => "invalid_request",
        ProviderErrorKind::ModelNotFound => "model_not_found",
        ProviderErrorKind::Unsupported => "unsupported",
        ProviderErrorKind::RateLimited => "rate_limited",
        ProviderErrorKind::QuotaExceeded => "quota_exceeded",
        ProviderErrorKind::Timeout => "timeout",
        ProviderErrorKind::Unavailable => "unavailable",
        ProviderErrorKind::Conflict => "conflict",
        ProviderErrorKind::ContentBlocked => "content_blocked",
        ProviderErrorKind::Cancelled => "cancelled",
        ProviderErrorKind::Transport => "transport",
        ProviderErrorKind::Protocol => "protocol",
        ProviderErrorKind::LimitExceeded => "limit_exceeded",
        ProviderErrorKind::Internal => "internal",
    }
}

fn provider_kind_label(kind: ProviderKind) -> ProviderKindLabel {
    match kind {
        ProviderKind::Gemini => ProviderKindLabel::Gemini,
        ProviderKind::Router => ProviderKindLabel::Router,
        ProviderKind::CodexCli => ProviderKindLabel::CodexCli,
    }
}

fn provider_profile_from_draft(
    draft: ProviderProfileDraft,
) -> Result<ProviderProfile, &'static str> {
    match draft.kind {
        ProviderKindLabel::Gemini => Ok(ProviderProfile::gemini()),
        ProviderKindLabel::Router => ProviderProfile::router(
            draft.base_url.trim().to_owned(),
            nonempty(draft.project),
            nonempty(draft.auth_header),
        ),
        ProviderKindLabel::CodexCli => Ok(ProviderProfile::codex_cli()),
    }
}

fn available_codex_profile_id(profiles: &[autoharness_app::profiles::ManagedProfile]) -> ProfileId {
    for suffix in 1_u32..=10_000 {
        let candidate = if suffix == 1 {
            "codex".to_owned()
        } else {
            format!("codex-{suffix}")
        };
        if !profiles
            .iter()
            .any(|managed| managed.id.as_str() == candidate)
        {
            return ProfileId::new(candidate).expect("generated Codex profile IDs are valid");
        }
    }
    ProfileId::new("codex-connected").expect("fallback Codex profile ID is valid")
}

fn nonempty(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn profile_validation_failure(reason: impl Into<String>) -> UiFailure {
    UiFailure::new(ErrorClass::Validation, reason.into(), RetryPolicy::Never)
}

fn profile_unavailable_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Unavailable,
        "Profile management is unavailable in this application mode",
        RetryPolicy::Never,
    )
}

fn profile_failure(error: &ProfileManagementError) -> UiFailure {
    match error {
        ProfileManagementError::Store(ProfileStoreError::Invalid(reason)) => {
            profile_validation_failure(*reason)
        }
        ProfileManagementError::Store(ProfileStoreError::UnknownProfile) => {
            profile_validation_failure("that profile does not exist")
        }
        ProfileManagementError::Store(ProfileStoreError::Io) => UiFailure::new(
            ErrorClass::Unavailable,
            "The profile settings document could not be updated",
            RetryPolicy::Now,
        ),
        ProfileManagementError::Vault(VaultError::InvalidSecret(reason)) => {
            profile_validation_failure(*reason)
        }
        ProfileManagementError::Vault(VaultError::MissingEntry)
        | ProfileManagementError::CredentialNotStored => UiFailure::new(
            ErrorClass::Authentication,
            "The selected profile has no stored credential",
            RetryPolicy::Never,
        ),
        ProfileManagementError::Vault(VaultError::Unavailable | VaultError::Platform(_)) => {
            UiFailure::new(
                ErrorClass::Unavailable,
                "The operating-system credential vault is unavailable",
                RetryPolicy::Now,
            )
        }
        ProfileManagementError::Conflict(reason) => {
            UiFailure::new(ErrorClass::Conflict, *reason, RetryPolicy::Never)
        }
        ProfileManagementError::RecoveryPending => UiFailure::new(
            ErrorClass::Unavailable,
            "The profile is safe, but credential-vault repair remains pending",
            RetryPolicy::Now,
        ),
    }
}

fn classified_failure(error: &(impl ClassifiedError + std::fmt::Display)) -> UiFailure {
    UiFailure::new(
        error.class(),
        error.to_string(),
        RetryPolicy::from_advice(error.retry_advice(), 0),
    )
}

fn engine_failure(error: &DurableEngineError) -> UiFailure {
    classified_failure(error)
}

fn provider_failure(error: &ProviderError) -> UiFailure {
    classified_failure(error).with_code(provider_code(error.kind()))
}

fn start_attempt_failure(error: &StartAttemptError) -> UiFailure {
    match error {
        StartAttemptError::Engine(error) => engine_failure(error),
        StartAttemptError::Provider(error) => provider_failure(error),
        StartAttemptError::Context(error) => context_failure(error),
    }
}

fn context_failure(error: &AppError) -> UiFailure {
    let (class, message, retry) = match error {
        AppError::Store(_) | AppError::WorkerStopped => (
            ErrorClass::Storage,
            "The request context could not be committed to local storage",
            RetryPolicy::Now,
        ),
        AppError::Memory(_) | AppError::Configuration => (
            ErrorClass::Validation,
            "The request did not fit the deterministic context policy",
            RetryPolicy::Never,
        ),
        AppError::CredentialRedactionUnavailable => (
            ErrorClass::Unavailable,
            "Saved credentials could not be checked, so the provider request was not prepared",
            RetryPolicy::Now,
        ),
        AppError::Engine(_)
        | AppError::Provider(_)
        | AppError::MemoryCommand(_)
        | AppError::FileSystem
        | AppError::WriterAlreadyRunning
        | AppError::Terminal => (
            ErrorClass::Unavailable,
            "The provider request could not be prepared safely",
            RetryPolicy::Now,
        ),
    };
    UiFailure::new(class, message, retry).with_code("context_not_committed")
}

fn default_memory_view_query() -> Result<MemoryViewQuery, AppError> {
    MemoryViewQuery::new(
        "",
        MemoryStatusFilter::All,
        MemoryScopeFilter::All,
        MemoryPageDirection::First,
        None,
        MEMORY_VIEW_PAGE_SIZE,
    )
    .map_err(|_| AppError::Configuration)
}

fn memory_view_statuses(filter: MemoryStatusFilter) -> Vec<MemoryInspectionStatus> {
    match filter {
        MemoryStatusFilter::Eligible | MemoryStatusFilter::Active => {
            vec![MemoryInspectionStatus::Active]
        }
        MemoryStatusFilter::Proposed => vec![MemoryInspectionStatus::Proposed],
        MemoryStatusFilter::Inactive => vec![
            MemoryInspectionStatus::Conflicting,
            MemoryInspectionStatus::Superseded,
            MemoryInspectionStatus::Rejected,
            MemoryInspectionStatus::Retracted,
            MemoryInspectionStatus::Expired,
            MemoryInspectionStatus::Deleted,
        ],
        MemoryStatusFilter::All => Vec::new(),
    }
}

fn decode_memory_view_cursor(
    cursor: &MemoryViewCursor,
) -> Result<MemoryInspectionCursor, AppError> {
    let (updated_at, memory_id) = cursor
        .as_str()
        .split_once(':')
        .ok_or(AppError::Configuration)?;
    let updated_at = updated_at
        .parse::<i64>()
        .map(TimestampMillis::new)
        .map_err(|_| AppError::Configuration)?;
    let memory_id = MemoryId::new(memory_id.to_owned()).map_err(|_| AppError::Configuration)?;
    Ok(MemoryInspectionCursor::new(updated_at, memory_id))
}

fn encode_memory_view_cursor(
    record: &autoharness_store::MemoryInspectionRecord,
) -> Result<MemoryViewCursor, AppError> {
    MemoryViewCursor::new(format!(
        "{}:{}",
        record.updated_at().get(),
        record.memory_id().as_str()
    ))
    .map_err(|_| AppError::Configuration)
}

fn memory_view_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Storage,
        "The requested Memory view could not be loaded from durable storage",
        RetryPolicy::Now,
    )
    .with_code("memory_query")
}

fn memory_import_failure(error: ImportDocumentError) -> UiFailure {
    match error {
        ImportDocumentError::InvalidRelativePath | ImportDocumentError::PathEscapesWorkspace => {
            UiFailure::new(
                ErrorClass::Validation,
                "Choose a document path inside the current workspace without traversal",
                RetryPolicy::Never,
            )
            .with_code("memory_import_path")
        }
        ImportDocumentError::DocumentNotFound => UiFailure::new(
            ErrorClass::Validation,
            "That workspace document does not exist",
            RetryPolicy::Never,
        )
        .with_code("memory_import_missing"),
        ImportDocumentError::NotRegularFile => UiFailure::new(
            ErrorClass::Validation,
            "Choose a regular UTF-8 text file to import",
            RetryPolicy::Never,
        )
        .with_code("memory_import_file"),
        ImportDocumentError::DocumentTooLarge => UiFailure::new(
            ErrorClass::Validation,
            "The workspace document exceeds the 16 KiB memory import limit",
            RetryPolicy::Never,
        )
        .with_code("memory_import_size"),
        ImportDocumentError::InvalidUtf8 | ImportDocumentError::UnsafeControlCharacter => {
            UiFailure::new(
                ErrorClass::Validation,
                "The workspace document must contain safe UTF-8 text",
                RetryPolicy::Never,
            )
            .with_code("memory_import_text")
        }
        ImportDocumentError::InvalidDomainValue => UiFailure::new(
            ErrorClass::Validation,
            "The workspace document could not become a bounded memory proposal",
            RetryPolicy::Never,
        )
        .with_code("memory_import_policy"),
        ImportDocumentError::WorkspaceUnavailable | ImportDocumentError::DocumentUnavailable => {
            UiFailure::new(
                ErrorClass::Storage,
                "The workspace document could not be read",
                RetryPolicy::Now,
            )
            .with_code("memory_import_storage")
        }
    }
}

fn stale_memory_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Conflict,
        "That memory changed or is no longer in the requested state",
        RetryPolicy::Now,
    )
    .with_code("stale_memory")
}

fn memory_redaction_unavailable_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Unavailable,
        "Saved credentials could not be checked, so durable memory was not changed",
        RetryPolicy::Now,
    )
    .with_code("memory_redaction_unavailable")
}

fn prompt_redaction_unavailable_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Unavailable,
        "Saved credentials could not be checked, so the prompt was not sent or saved",
        RetryPolicy::Now,
    )
    .with_code("prompt_redaction_unavailable")
}

fn memory_failure(error: &AppError) -> UiFailure {
    use crate::memory_runtime::MemoryCommandError;

    match error {
        AppError::MemoryCommand(MemoryCommandError::VersionConflict) => stale_memory_failure(),
        AppError::MemoryCommand(MemoryCommandError::InvalidTransition) => UiFailure::new(
            ErrorClass::Conflict,
            "That memory lifecycle action is no longer valid",
            RetryPolicy::Now,
        )
        .with_code("memory_transition"),
        AppError::MemoryCommand(MemoryCommandError::ValidationRejected) => UiFailure::new(
            ErrorClass::Validation,
            "The memory was not saved because deterministic validation rejected it",
            RetryPolicy::Never,
        )
        .with_code("memory_validation"),
        AppError::MemoryCommand(
            MemoryCommandError::UnsupportedCommand | MemoryCommandError::Policy(_),
        )
        | AppError::Memory(_)
        | AppError::Configuration => UiFailure::new(
            ErrorClass::Validation,
            "The memory request did not satisfy durable-memory policy",
            RetryPolicy::Never,
        )
        .with_code("memory_policy"),
        AppError::Store(autoharness_store::StoreError::MemoryVersionConflict { .. }) => {
            stale_memory_failure()
        }
        AppError::Store(_)
        | AppError::Engine(_)
        | AppError::FileSystem
        | AppError::WorkerStopped
        | AppError::WriterAlreadyRunning => UiFailure::new(
            ErrorClass::Storage,
            "The durable memory operation could not be completed",
            RetryPolicy::Now,
        )
        .with_code("memory_storage"),
        AppError::Provider(_) | AppError::Terminal | AppError::CredentialRedactionUnavailable => {
            UiFailure::new(
                ErrorClass::Unavailable,
                "The durable memory operation is temporarily unavailable",
                RetryPolicy::Now,
            )
            .with_code("memory_unavailable")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex};
    use std::time::Duration;

    use autoharness_app::profiles::ProfileStore;
    use autoharness_app::vault::{FakeVault, VaultPort};
    use autoharness_domain::{
        Causation, CommandId, CorrelationId, EventEnvelope, EventId, EventPayload, InputId,
        MemoryEvidenceSource, MemoryOperationPayload, ModelId, ModelRef, ProviderCallId,
        ProviderId, SessionSequence, TimestampMillis, ToolArguments, ToolName,
    };
    use autoharness_provider::{
        CapabilitySupport, Catalog, CatalogFreshness, CatalogRequest, Chat, CompletionReason,
        ModelCapabilities, ModelCatalog, ProviderAvailability, ProviderEventStream,
        ProviderMetadata, SecretRedactor as _, TextDelta, UsageSnapshot as ProviderUsage,
    };
    use autoharness_provider_openai::{OpenAiRouterProvider, RouterCredential, RouterSettings};
    use autoharness_settings::CredentialReference;
    use autoharness_store::SessionStore as _;
    use autoharness_store_sqlite::SqliteStore;
    use autoharness_tui::{SessionProjection, TranscriptItem, UiPorts, bounded_ports};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use super::*;

    static FAKE_PROVIDER_ID: LazyLock<ProviderId> =
        LazyLock::new(|| ProviderId::new("google-ai-studio").expect("fixture provider ID"));

    #[derive(Default)]
    struct FakeProvider {
        calls: AtomicUsize,
        requests: Mutex<Vec<ChatRequest>>,
    }

    #[async_trait::async_trait]
    impl Catalog for FakeProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor(), alternate_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for FakeProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Ok(Box::pin(async_stream::stream! {
                    yield Ok(ProviderStreamEvent::Started);
                    yield Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("partial").expect("text delta"),
                    ));
                    cancellation.cancelled().await;
                    yield Ok(ProviderStreamEvent::Cancelled);
                }))
            } else {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("recovered").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Usage(ProviderUsage {
                        input_tokens: Some(3),
                        output_tokens: Some(2),
                        cached_input_tokens: Some(1),
                        reasoning_tokens: Some(1),
                        tool_tokens: Some(0),
                        total_tokens: Some(5),
                    })),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ])))
            }
        }
    }

    impl autoharness_provider::SecretRedactor for FakeProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.replace("test-api-secret", "[REDACTED]")
        }
    }

    impl ProviderMetadata for FakeProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    #[derive(Clone, Copy, Default)]
    enum ToolLoopCall {
        #[default]
        Write,
        Read,
    }

    #[derive(Default)]
    struct ToolLoopProvider {
        first_call: ToolLoopCall,
        calls: AtomicUsize,
        requests: Mutex<Vec<ChatRequest>>,
    }

    impl ToolLoopProvider {
        fn reading() -> Self {
            Self {
                first_call: ToolLoopCall::Read,
                ..Self::default()
            }
        }
    }

    #[async_trait::async_trait]
    impl Catalog for ToolLoopProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for ToolLoopProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                let (provider_call_id, tool_name, arguments) = match self.first_call {
                    ToolLoopCall::Write => (
                        "call-write-1",
                        "fs_write",
                        ToolArguments::new(serde_json::json!({
                            "path": "result.txt",
                            "content": "written by tool"
                        }))
                        .expect("tool arguments"),
                    ),
                    ToolLoopCall::Read => (
                        "call-read-1",
                        "fs_read",
                        ToolArguments::new(serde_json::json!({"path": ".env"}))
                            .expect("tool arguments"),
                    ),
                };
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::ToolCall(ProviderToolCall {
                        provider_call_id: ProviderCallId::new(provider_call_id)
                            .expect("provider call ID"),
                        tool_name: ToolName::new(tool_name).expect("tool name"),
                        arguments,
                    })),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::ToolCalls,
                    }),
                ])))
            } else {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("tool complete").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Usage(ProviderUsage {
                        input_tokens: Some(8),
                        output_tokens: Some(2),
                        cached_input_tokens: None,
                        reasoning_tokens: None,
                        tool_tokens: Some(1),
                        total_tokens: Some(10),
                    })),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ])))
            }
        }
    }

    impl autoharness_provider::SecretRedactor for ToolLoopProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.replace("test-api-secret", "[REDACTED]")
        }
    }

    impl ProviderMetadata for ToolLoopProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    #[derive(Default)]
    struct RecordingProvider {
        calls: AtomicUsize,
        requests: Mutex<Vec<ChatRequest>>,
    }

    #[async_trait::async_trait]
    impl Catalog for RecordingProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for RecordingProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(futures_util::stream::iter([
                Ok(ProviderStreamEvent::Started),
                Ok(ProviderStreamEvent::TextDelta(
                    TextDelta::new("recorded").expect("text delta"),
                )),
                Ok(ProviderStreamEvent::Completed {
                    reason: CompletionReason::Stop,
                }),
            ])))
        }
    }

    impl autoharness_provider::SecretRedactor for RecordingProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.replace("configured-secret", "[REDACTED]")
        }
    }

    impl ProviderMetadata for RecordingProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    struct CompactionProvider {
        responses: Mutex<VecDeque<String>>,
        requests: Mutex<Vec<ChatRequest>>,
        protect_configured_secret: AtomicBool,
    }

    impl CompactionProvider {
        fn new(responses: impl IntoIterator<Item = String>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                requests: Mutex::new(Vec::new()),
                protect_configured_secret: AtomicBool::new(true),
            }
        }

        fn allow_unprotected_seed(&self) {
            self.protect_configured_secret
                .store(false, Ordering::SeqCst);
        }

        fn protect_configured_secret(&self) {
            self.protect_configured_secret.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl Catalog for CompactionProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![compaction_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for CompactionProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let response = self
                .responses
                .lock()
                .expect("response lock")
                .pop_front()
                .unwrap_or_else(|| "compaction follow-up complete".to_owned());
            Ok(Box::pin(futures_util::stream::iter([
                Ok(ProviderStreamEvent::Started),
                Ok(ProviderStreamEvent::TextDelta(
                    TextDelta::new(response).expect("text delta"),
                )),
                Ok(ProviderStreamEvent::Completed {
                    reason: CompletionReason::Stop,
                }),
            ])))
        }
    }

    impl autoharness_provider::SecretRedactor for CompactionProvider {
        fn redact_secrets(&self, value: &str) -> String {
            if self.protect_configured_secret.load(Ordering::SeqCst) {
                value.replace("configured-secret", "[REDACTED]")
            } else {
                value.to_owned()
            }
        }
    }

    impl ProviderMetadata for CompactionProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    enum ProposalProviderTurn {
        Tools(Vec<ProviderToolCall>),
        Complete(&'static str),
        CompleteOwned(String),
        TextFragments(Vec<String>),
        TextAndTools {
            text: String,
            calls: Vec<ProviderToolCall>,
        },
    }

    struct ProposalProvider {
        turns: Mutex<VecDeque<ProposalProviderTurn>>,
        requests: Mutex<Vec<ChatRequest>>,
    }

    impl ProposalProvider {
        fn new(turns: Vec<ProposalProviderTurn>) -> Self {
            Self {
                turns: Mutex::new(turns.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl Catalog for ProposalProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for ProposalProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let turn = self
                .turns
                .lock()
                .expect("turn lock")
                .pop_front()
                .expect("scripted provider turn");
            let mut events = vec![Ok(ProviderStreamEvent::Started)];
            match turn {
                ProposalProviderTurn::Tools(calls) => {
                    events.extend(
                        calls.into_iter().map(|call| {
                            Ok::<_, ProviderError>(ProviderStreamEvent::ToolCall(call))
                        }),
                    );
                    events.push(Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::ToolCalls,
                    }));
                }
                ProposalProviderTurn::Complete(text) => {
                    events.push(Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new(text).expect("scripted text"),
                    )));
                    events.push(Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }));
                }
                ProposalProviderTurn::CompleteOwned(text) => {
                    events.push(Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new(text).expect("scripted text"),
                    )));
                    events.push(Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }));
                }
                ProposalProviderTurn::TextFragments(fragments) => {
                    events.extend(fragments.into_iter().map(|text| {
                        Ok::<_, ProviderError>(ProviderStreamEvent::TextDelta(
                            TextDelta::new(text).expect("scripted text fragment"),
                        ))
                    }));
                    events.push(Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }));
                }
                ProposalProviderTurn::TextAndTools { text, calls } => {
                    events.push(Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new(text).expect("scripted text"),
                    )));
                    events.extend(
                        calls.into_iter().map(|call| {
                            Ok::<_, ProviderError>(ProviderStreamEvent::ToolCall(call))
                        }),
                    );
                    events.push(Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::ToolCalls,
                    }));
                }
            }
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    impl autoharness_provider::SecretRedactor for ProposalProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.replace("test-api-secret", "[REDACTED]")
        }
    }

    impl ProviderMetadata for ProposalProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    fn scripted_tool_call(
        provider_call_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> ProviderToolCall {
        ProviderToolCall {
            provider_call_id: ProviderCallId::new(provider_call_id).expect("provider call ID"),
            tool_name: ToolName::new(tool_name).expect("tool name"),
            arguments: ToolArguments::new(arguments).expect("tool arguments"),
        }
    }

    struct InvalidToolRepairProvider {
        repair_after_first: bool,
        calls: AtomicUsize,
        requests: Mutex<Vec<ChatRequest>>,
    }

    impl Default for InvalidToolRepairProvider {
        fn default() -> Self {
            Self {
                repair_after_first: true,
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl InvalidToolRepairProvider {
        fn never_repairs() -> Self {
            Self {
                repair_after_first: false,
                ..Self::default()
            }
        }
    }

    #[async_trait::async_trait]
    impl Catalog for InvalidToolRepairProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for InvalidToolRepairProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 || !self.repair_after_first {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::ToolCall(ProviderToolCall {
                        provider_call_id: ProviderCallId::new(format!("call-invalid-{call}"))
                            .expect("provider call ID"),
                        tool_name: ToolName::new("web_search").expect("tool name"),
                        arguments: ToolArguments::new(serde_json::json!({
                            "query": "news today"
                        }))
                        .expect("tool arguments"),
                    })),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::ToolCalls,
                    }),
                ])))
            } else {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("recovered after invalid tool call").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ])))
            }
        }
    }

    impl autoharness_provider::SecretRedactor for InvalidToolRepairProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for InvalidToolRepairProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    struct CommitThenCancelledFilesystem {
        root: std::path::PathBuf,
    }

    #[async_trait::async_trait]
    impl autoharness_tool::FilesystemCapability for CommitThenCancelledFilesystem {
        async fn read(
            &self,
            path: &std::path::Path,
            _cancellation: &CancellationToken,
        ) -> Result<Vec<u8>, ToolError> {
            std::fs::read(self.root.join(path)).map_err(|_| {
                ToolError::new(
                    autoharness_tool::ToolErrorKind::Filesystem,
                    RetryAdvice::Never,
                )
            })
        }

        async fn write(
            &self,
            path: &std::path::Path,
            content: &[u8],
            _cancellation: &CancellationToken,
        ) -> Result<Vec<u8>, ToolError> {
            std::fs::write(self.root.join(path), content).expect("committed fixture effect");
            Err(ToolError::new(
                autoharness_tool::ToolErrorKind::Cancelled,
                RetryAdvice::Never,
            ))
        }
    }

    #[derive(Default)]
    struct ToolThenErrorProvider;

    #[async_trait::async_trait]
    impl Catalog for ToolThenErrorProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for ToolThenErrorProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            let arguments = ToolArguments::new(serde_json::json!({
                "path": "result.txt",
                "content": "must not execute"
            }))
            .expect("tool arguments");
            Ok(Box::pin(futures_util::stream::iter([
                Ok(ProviderStreamEvent::Started),
                Ok(ProviderStreamEvent::ToolCall(ProviderToolCall {
                    provider_call_id: ProviderCallId::new("call-before-error")
                        .expect("provider call ID"),
                    tool_name: ToolName::new("fs_write").expect("tool name"),
                    arguments,
                })),
                Err(ProviderError::new(
                    ProviderErrorKind::Protocol,
                    RetryAdvice::Never,
                )),
            ])))
        }
    }

    impl autoharness_provider::SecretRedactor for ToolThenErrorProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for ToolThenErrorProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    #[derive(Default)]
    struct UnsolicitedCancellationProvider {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Catalog for UnsolicitedCancellationProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for UnsolicitedCancellationProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                vec![
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::Cancelled),
                ]
            } else {
                vec![
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("recovered").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ]
            };
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    impl autoharness_provider::SecretRedactor for UnsolicitedCancellationProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for UnsolicitedCancellationProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }
    }

    struct AuthenticationProvider;

    #[async_trait::async_trait]
    impl Catalog for AuthenticationProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for AuthenticationProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ))
        }
    }

    impl autoharness_provider::SecretRedactor for AuthenticationProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for AuthenticationProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }
    }

    #[derive(Default)]
    struct TransientCatalogProvider {
        catalog_calls: AtomicUsize,
        chat: FakeProvider,
    }

    #[async_trait::async_trait]
    impl Catalog for TransientCatalogProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            if self.catalog_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(ProviderError::new(
                    ProviderErrorKind::Timeout,
                    RetryAdvice::Immediate,
                ));
            }
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor(), alternate_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for TransientCatalogProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.chat.stream_chat(request, cancellation).await
        }
    }

    impl autoharness_provider::SecretRedactor for TransientCatalogProvider {
        fn redact_secrets(&self, value: &str) -> String {
            self.chat.redact_secrets(value)
        }
    }

    impl ProviderMetadata for TransientCatalogProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }
    }

    fn fixture_model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider ID"),
            ModelId::new("models/gemini-fixture").expect("model ID"),
        )
    }

    fn fixture_model_descriptor() -> ModelDescriptor {
        let model = fixture_model();
        ModelDescriptor {
            provider_id: model.provider_id().clone(),
            model_id: model.model_id().clone(),
            display_name: "Gemini fixture".to_owned(),
            description: None,
            input_token_limit: Some(32_768),
            output_token_limit: Some(1_024),
            capabilities: ModelCapabilities {
                chat: CapabilitySupport::Supported,
                streaming: CapabilitySupport::Supported,
                managed_interactions: CapabilitySupport::Unknown,
                thinking: CapabilitySupport::Unknown,
                tool_calling: CapabilitySupport::Supported,
            },
        }
    }

    fn compaction_model_descriptor() -> ModelDescriptor {
        let model = fixture_model();
        ModelDescriptor {
            provider_id: model.provider_id().clone(),
            model_id: model.model_id().clone(),
            display_name: "Compaction fixture".to_owned(),
            description: None,
            input_token_limit: Some(900),
            output_token_limit: Some(8_192),
            capabilities: ModelCapabilities {
                chat: CapabilitySupport::Supported,
                streaming: CapabilitySupport::Supported,
                managed_interactions: CapabilitySupport::Unknown,
                thinking: CapabilitySupport::Unknown,
                tool_calling: CapabilitySupport::Unsupported,
            },
        }
    }

    fn alternate_model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider ID"),
            ModelId::new("models/gemini-alternate").expect("model ID"),
        )
    }

    fn alternate_model_descriptor() -> ModelDescriptor {
        let model = alternate_model();
        ModelDescriptor {
            provider_id: model.provider_id().clone(),
            model_id: model.model_id().clone(),
            display_name: "Gemini alternate".to_owned(),
            description: None,
            input_token_limit: Some(32_768),
            output_token_limit: Some(1_024),
            capabilities: ModelCapabilities {
                chat: CapabilitySupport::Supported,
                streaming: CapabilitySupport::Supported,
                managed_interactions: CapabilitySupport::Unknown,
                thinking: CapabilitySupport::Unknown,
                tool_calling: CapabilitySupport::Supported,
            },
        }
    }

    async fn seed_pending_bound_turn(
        database: std::path::PathBuf,
        workspace: &std::path::Path,
    ) -> (SessionId, AttemptId, ChatRequest) {
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let (_ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(Arc::new(RecordingProvider::default())),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(workspace),
            app,
            shutdown,
        );
        coordinator.workspace = workspace.to_path_buf();
        coordinator
            .initialize_context_scope()
            .await
            .expect("initialize context scope");
        coordinator.catalog_models = vec![fixture_model_descriptor()];
        coordinator
            .execute(CommandPayload::SelectModel {
                session_id: session_id.clone(),
                model: fixture_model(),
            })
            .await
            .expect("select fixture model");
        let attempt_id = ids::attempt_id();
        coordinator
            .execute(CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                input_id: ids::input_id(),
                prompt: PromptText::new("resume the exact durable provider request")
                    .expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            })
            .await
            .expect("prepare attempt");
        coordinator
            .execute(CommandPayload::ConfigureRunBudget {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                limits: RunLimits::default(),
            })
            .await
            .expect("configure budget");
        coordinator
            .execute(CommandPayload::StartAttempt {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
            })
            .await
            .expect("start attempt");
        let prepared = coordinator
            .prepare_and_bind_context(&attempt_id, true)
            .await
            .expect("bind exact context");
        let request = prepared.request().clone();
        let attempt = coordinator
            .session
            .attempt(&attempt_id)
            .expect("bound attempt");
        assert_eq!(attempt.status(), EngineAttemptStatus::InFlight);
        assert!(attempt.is_provider_dispatch_ready());
        assert_eq!(attempt.turns_started(), 0);
        drop(coordinator);
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
        (session_id, attempt_id, request)
    }

    async fn wait_for_provider_calls(provider: &RecordingProvider, expected: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if provider.calls.load(Ordering::SeqCst) == expected {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("provider call timeout");
    }

    async fn wait_for_catalog(ui: &mut UiPorts) {
        loop {
            if matches!(
                &**ui.catalogs.borrow_and_update(),
                CatalogProjection::Ready { .. }
            ) {
                return;
            }
            tokio::time::timeout(Duration::from_secs(5), ui.catalogs.changed())
                .await
                .expect("catalog timeout")
                .expect("catalog sender remains open");
        }
    }

    async fn wait_for_session(
        sessions: &mut watch::Receiver<Arc<SessionProjection>>,
        predicate: impl Fn(&SessionProjection) -> bool,
    ) -> Arc<SessionProjection> {
        loop {
            let current = Arc::clone(&sessions.borrow_and_update());
            if predicate(&current) {
                return current;
            }
            tokio::time::timeout(Duration::from_secs(5), sessions.changed())
                .await
                .expect("session timeout")
                .expect("session sender remains open");
        }
    }

    async fn expect_commit(ui: &mut UiPorts, request_id: RequestId) {
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert_eq!(notice, UiNotice::IntentCommitted { request_id });
    }

    async fn submit_and_wait_for_completion(
        ui: &mut UiPorts,
        request_id: RequestId,
        prompt: &str,
        completed_attempts: usize,
    ) {
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id,
                prompt: prompt.to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(ui, request_id).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection
                .transcript
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Completed,
                            ..
                        }
                    )
                })
                .count()
                == completed_attempts
        })
        .await;
    }

    async fn handle_next_provider_message(coordinator: &mut Coordinator) {
        let message = tokio::time::timeout(Duration::from_secs(5), coordinator.message_rx.recv())
            .await
            .expect("provider message timeout")
            .expect("provider message channel remains open");
        coordinator
            .handle_async(message)
            .await
            .expect("handle provider message");
    }

    struct SeededMemory {
        memory_id: MemoryId,
        revision_id: MemoryRevisionId,
        last_sequence: u64,
    }

    async fn seed_workspace_memory(
        handle: &EngineHandle,
        workspace: &std::path::Path,
        content: &str,
    ) -> SeededMemory {
        let workspace_id = handle
            .resolve_workspace_id(
                workspace_locator_digest(workspace).expect("workspace locator digest"),
            )
            .await
            .expect("workspace binding");
        seed_memory_in_scope(handle, DomainMemoryScope::Workspace(workspace_id), content).await
    }

    async fn seed_memory_in_scope(
        handle: &EngineHandle,
        scope: DomainMemoryScope,
        content: &str,
    ) -> SeededMemory {
        let memory_id = ids::memory_id();
        let revision = user_memory_draft(
            MemoryRevisionNumber::FIRST,
            None,
            content.to_owned(),
            ConfidenceBasisPoints::new(10_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
        )
        .expect("memory draft");
        let revision_id = revision.revision_id().clone();
        let commit = handle
            .execute_memory_command(ids::memory_command(
                memory_id.clone(),
                None,
                MemoryCommandPayload::CreateMemory {
                    scope,
                    memory_kind: MemoryKind::Preference,
                    revision,
                },
            ))
            .await
            .expect("seed memory");
        SeededMemory {
            memory_id,
            revision_id,
            last_sequence: commit.receipt().last_sequence(),
        }
    }

    #[tokio::test]
    async fn authoritative_memory_query_filters_before_paging() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("memory-query.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let first_seeded = seed_workspace_memory(&handle, &workspace, "needle first").await;
        let second_seeded = seed_workspace_memory(&handle, &workspace, "needle second").await;
        let distractor = seed_workspace_memory(&handle, &workspace, "newest distractor").await;
        let inactive = seed_workspace_memory(&handle, &workspace, "needle inactive").await;
        handle
            .execute_memory_command(ids::memory_command(
                inactive.memory_id,
                Some(MemorySequence::new(inactive.last_sequence).expect("inactive sequence")),
                MemoryCommandPayload::DeleteMemory {
                    revision_id: inactive.revision_id,
                },
            ))
            .await
            .expect("delete inactive fixture");
        let _session_scoped = seed_memory_in_scope(
            &handle,
            DomainMemoryScope::Session(session_id.clone()),
            "needle session scoped",
        )
        .await;
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::new(session_id, session, handle, None, app, shutdown);
        coordinator.workspace = workspace;
        coordinator
            .initialize_context_scope()
            .await
            .expect("context scope");

        let first_query = MemoryViewQuery::new(
            "needle",
            MemoryStatusFilter::Active,
            MemoryScopeFilter::Workspace,
            MemoryPageDirection::First,
            None,
            1,
        )
        .expect("first query");
        let first = coordinator
            .load_memory_view(&MemoryViewState {
                generation: 41,
                query: first_query,
            })
            .await
            .expect("first authoritative page");
        assert_eq!(first.view_generation(), 41);
        assert_eq!(first.summaries().len(), 1);
        assert!(first.summaries()[0].preview().contains("needle"));
        let cursor = first.next_cursor().cloned().expect("next page cursor");

        let second_query = MemoryViewQuery::new(
            "needle",
            MemoryStatusFilter::Active,
            MemoryScopeFilter::Workspace,
            MemoryPageDirection::Next,
            Some(cursor),
            1,
        )
        .expect("second query");
        coordinator.memory_view = Some(MemoryViewState {
            generation: 42,
            query: second_query.clone(),
        });
        let second = coordinator
            .load_memory_view(&MemoryViewState {
                generation: 42,
                query: second_query,
            })
            .await
            .expect("second authoritative page");
        assert_eq!(second.view_generation(), 42);
        assert_eq!(second.summaries().len(), 1);
        assert!(second.summaries()[0].preview().contains("needle"));
        assert_ne!(first.summaries()[0].id(), second.summaries()[0].id());
        let expected = [
            first_seeded.memory_id.as_str(),
            second_seeded.memory_id.as_str(),
        ];
        assert!(expected.contains(&first.summaries()[0].id()));
        assert!(expected.contains(&second.summaries()[0].id()));
        assert!(second.next_cursor().is_none());

        let refreshed_id = second.summaries()[0].id().to_owned();
        let mutation_request = RequestId::new(500);
        coordinator
            .commit_memory_command(
                mutation_request,
                ids::memory_command(
                    distractor.memory_id,
                    Some(
                        MemorySequence::new(distractor.last_sequence).expect("distractor sequence"),
                    ),
                    MemoryCommandPayload::DeleteMemory {
                        revision_id: distractor.revision_id,
                    },
                ),
            )
            .await
            .expect("lifecycle mutation");
        expect_commit(&mut ui, mutation_request).await;
        let refreshed = ui.memories.borrow().clone();
        assert_eq!(refreshed.view_generation(), 42);
        assert_eq!(refreshed.summaries().len(), 1);
        assert_eq!(refreshed.summaries()[0].id(), refreshed_id);
        assert!(refreshed.summaries()[0].preview().contains("needle"));
        assert!(refreshed.next_cursor().is_none());

        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn workspace_document_import_is_review_only_until_distinct_user_approval() {
        const IMPORTED: &str = "Imported decision: keep provider evidence attached to each fact.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("decision.txt"), IMPORTED).expect("import document");
        let database = directory.path().join("memory-import.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator =
            Coordinator::new(session_id, session, handle.clone(), None, app, shutdown);
        coordinator.workspace = workspace;
        coordinator
            .initialize_context_scope()
            .await
            .expect("context scope");
        let workspace_id = coordinator
            .context_scope()
            .expect("initialized scope")
            .workspace_id()
            .clone();

        let import_request = RequestId::new(1);
        coordinator
            .import_memory(import_request, "decision.txt".to_owned())
            .await
            .expect("import memory");
        expect_commit(&mut ui, import_request).await;
        let proposal_page = handle
            .inspect_memories(
                MemoryInspectionQuery::new(
                    vec![DomainMemoryScope::Workspace(workspace_id)],
                    vec![MemoryRevisionStatus::Proposed],
                    None,
                    10,
                )
                .expect("proposal query"),
            )
            .await
            .expect("proposal page");
        assert_eq!(proposal_page.records().len(), 1);
        let proposal = &proposal_page.records()[0];
        assert_eq!(
            proposal.content().expect("retained content").as_str(),
            IMPORTED
        );
        assert_eq!(proposal.lifecycle(), MemoryRevisionStatus::Proposed);
        assert!(proposal.active_revision_id().is_none());
        assert_eq!(
            proposal.latest_revision().origin(),
            MemoryOrigin::ImportedDocument
        );
        assert_eq!(
            proposal.latest_revision().trust_class(),
            TrustClass::Imported
        );
        assert!(matches!(
            proposal.latest_revision().evidence()[0].source(),
            MemoryEvidenceSource::ImportedDocument { .. }
        ));
        assert_eq!(
            proposal
                .latest_validation()
                .expect("deterministic validation")
                .status(),
            autoharness_domain::MemoryValidationStatus::NeedsReview
        );
        let memory_id = proposal.memory_id().clone();
        let proposal_revision_id = proposal.latest_revision().revision_id().clone();
        let expected_last_sequence = proposal.last_sequence();

        let approval_request = RequestId::new(2);
        coordinator
            .approve_memory_proposal(
                approval_request,
                memory_id.as_str().to_owned(),
                expected_last_sequence,
                proposal_revision_id.as_str().to_owned(),
            )
            .await
            .expect("approve imported proposal");
        expect_commit(&mut ui, approval_request).await;
        let revisions = handle
            .load_memory_revisions(memory_id)
            .await
            .expect("approved revisions");
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0].revision_id(), &proposal_revision_id);
        assert_eq!(revisions[0].status(), MemoryRevisionStatus::Superseded);
        assert_ne!(revisions[1].revision_id(), &proposal_revision_id);
        assert_eq!(revisions[1].status(), MemoryRevisionStatus::Active);
        assert_eq!(revisions[1].origin(), MemoryOrigin::ExplicitUser);
        assert_eq!(revisions[1].trust_class(), TrustClass::UserApproved);

        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn configured_credential_is_rejected_before_any_memory_durable_surface() {
        const SENTINEL: &str = "test-api-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("memory-secret.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let handle = actor.handle();
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id,
            session,
            handle.clone(),
            Some(Arc::new(FakeProvider::default())),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        let request_id = RequestId::new(91);
        let intent = UiIntent::RememberMemory {
            request_id,
            content: autoharness_tui::MemoryContent::new(SENTINEL).expect("memory content"),
        };
        assert!(!format!("{intent:?}").contains(SENTINEL));
        ui.intents.send(intent).await.expect("remember intent");

        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        match notice {
            UiNotice::IntentRejected {
                request_id: rejected,
                failure,
            } => {
                assert_eq!(rejected, request_id);
                assert_eq!(failure.code, "memory_secret");
                assert!(!format!("{failure:?}").contains(SENTINEL));
            }
            other => panic!("expected safe memory rejection, got {other:?}"),
        }
        assert!(ui.memories.borrow().summaries().is_empty());
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("mutation generation")
                .get(),
            0
        );

        shutdown.cancel();
        task.await.expect("coordinator task").expect("coordinator");
        actor.shutdown().await.expect("actor shutdown");
        for bytes in all_file_bytes(directory.path()) {
            assert!(
                !bytes
                    .windows(SENTINEL.len())
                    .any(|window| window == SENTINEL.as_bytes())
            );
        }
    }

    #[tokio::test]
    async fn configured_credential_in_evidence_is_rejected_before_any_memory_write() {
        const SENTINEL: &str = "test-api-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("memory-evidence-secret.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::new(
            session_id.clone(),
            session,
            handle.clone(),
            Some(Arc::new(FakeProvider::default())),
            app,
            shutdown,
        );
        let excerpt =
            autoharness_domain::MemoryEvidenceExcerpt::new(SENTINEL).expect("evidence excerpt");
        let evidence = autoharness_domain::MemoryEvidence::new(
            autoharness_domain::MemoryEvidenceId::new("credential-evidence").expect("evidence ID"),
            autoharness_domain::MemoryEvidenceSource::UserInput {
                session_id: session_id.clone(),
                input_id: InputId::new("credential-input").expect("input ID"),
            },
            autoharness_domain::MemoryEvidenceRelation::Supports,
            Some(excerpt.clone()),
            Some(normalized_content_hash(excerpt.as_str()).expect("excerpt digest")),
        )
        .expect("evidence");
        let content = MemoryContent::new("ordinary safe memory content").expect("memory content");
        let revision = MemoryRevisionDraft::new(
            ids::memory_revision_id(),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content digest"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(10_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            vec![evidence],
            Vec::new(),
        )
        .expect("memory draft");
        let memory_id = ids::memory_id();
        let command = ids::memory_command(
            memory_id.clone(),
            None,
            MemoryCommandPayload::CreateMemory {
                scope: DomainMemoryScope::Session(session_id),
                memory_kind: MemoryKind::Fact,
                revision,
            },
        );
        assert!(!format!("{command:?}").contains(SENTINEL));
        let request_id = RequestId::new(92);
        coordinator
            .commit_memory_command(request_id, command)
            .await
            .expect("safe rejection");

        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        match notice {
            UiNotice::IntentRejected {
                request_id: rejected,
                failure,
            } => {
                assert_eq!(rejected, request_id);
                assert_eq!(failure.code, "memory_secret");
                assert!(!format!("{failure:?}").contains(SENTINEL));
            }
            other => panic!("expected safe memory rejection, got {other:?}"),
        }
        assert!(
            handle
                .load_memory_revisions(memory_id)
                .await
                .expect("memory revisions")
                .is_empty()
        );
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("mutation generation")
                .get(),
            0
        );

        drop(coordinator);
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
        for bytes in all_file_bytes(directory.path()) {
            assert!(
                !bytes
                    .windows(SENTINEL.len())
                    .any(|window| window == SENTINEL.as_bytes())
            );
        }
    }

    #[tokio::test]
    async fn inactive_saved_profile_credential_blocks_memory_and_workspace_context() {
        const SENTINEL: &str = "inactive-profile-redaction-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("credential-import.txt"), SENTINEL)
            .expect("credential import fixture");
        let database = directory.path().join("inactive-credential.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        assert_eq!(provider.redact_secrets(SENTINEL), SENTINEL);
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;

        let memory_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::RememberMemory {
                request_id: memory_request,
                content: autoharness_tui::MemoryContent::new(SENTINEL).expect("memory content"),
            })
            .await
            .expect("remember intent");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("memory notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == memory_request && failure.code == "memory_secret"
        ));
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("memory generation")
                .get(),
            0
        );

        let import_request = RequestId::new(10);
        ui.intents
            .send(UiIntent::ImportMemory {
                request_id: import_request,
                path: autoharness_tui::MemoryImportPath::new("credential-import.txt")
                    .expect("import path"),
            })
            .await
            .expect("import intent");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("import notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == import_request && failure.code == "memory_secret"
        ));
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("memory generation after import")
                .get(),
            0
        );

        let select_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        submit_and_wait_for_completion(
            &mut ui,
            RequestId::new(3),
            &format!("redact this configured value: {SENTINEL}"),
            1,
        )
        .await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        {
            let requests = provider.requests.lock().expect("request lock");
            let serialized = serde_json::to_string(&requests[0]).expect("serialize request");
            assert!(!serialized.contains(SENTINEL));
            assert!(serialized.contains("[REDACTED]"));
        }

        std::fs::write(
            workspace.join("AGENTS.md"),
            format!("Never persist {SENTINEL} from this workspace source."),
        )
        .expect("secret-bearing workspace source");
        let submit_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "read the workspace instructions".to_owned(),
            })
            .await
            .expect("submit prompt");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("context notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == submit_request && failure.code == "context_not_committed"
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.requests.lock().expect("request lock").len(), 1);
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.payload(), EventPayload::ContextTurnBound { .. }))
                .count(),
            1
        );
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains(SENTINEL)
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
        for entry in std::fs::read_dir(directory.path()).expect("state directory") {
            let entry = entry.expect("state entry");
            if entry.file_type().expect("file type").is_file() {
                let bytes = std::fs::read(entry.path()).expect("durable state file");
                assert!(
                    !bytes
                        .windows(SENTINEL.len())
                        .any(|window| window == SENTINEL.as_bytes()),
                    "inactive credential must not reach durable application state"
                );
            }
        }
    }

    #[tokio::test]
    async fn inactive_saved_credential_split_across_provider_deltas_never_completes_durably() {
        const SENTINEL: &str = "inactive-provider-output-secret";
        const PREFIX: &str = "inactive-provider-output-";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("provider-output-secret.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::TextFragments(vec![PREFIX.to_owned(), "secret".to_owned()]),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "emit protected output".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        ..
                    } if text == PREFIX && code == "credential_in_provider_data"
                )
            })
        })
        .await;
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains(SENTINEL)
        );
        assert_eq!(provider.requests.lock().expect("request lock").len(), 1);

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn inactive_saved_credential_split_across_tool_arguments_is_never_proposed() {
        const SENTINEL: &str = "argument-half-secret-half";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("tool-argument-secret.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![ProposalProviderTurn::Tools(
            vec![scripted_tool_call(
                "call-secret-arguments",
                "fs_write",
                serde_json::json!({
                    "path": "argument-half-",
                    "content": "secret-half"
                }),
            )],
        )]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "attempt unsafe tool arguments".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        ..
                    } if code == "credential_in_provider_data"
                )
            })
        })
        .await;
        let recovered = handle
            .load_session(session_id.clone())
            .await
            .expect("load session")
            .expect("session");
        assert!(recovered.tool_calls().is_empty());
        assert!(!workspace.join("argument-half-").exists());
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert!(
            !events
                .iter()
                .any(|event| { matches!(event.payload(), EventPayload::ToolCallProposed { .. }) })
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn inactive_saved_credential_in_provider_call_identity_is_never_proposed() {
        const SENTINEL: &str = "inactive_identity_secret";

        assert_inactive_provider_tool_ingress_blocked(
            SENTINEL,
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                SENTINEL,
                "fs_read",
                serde_json::json!({"path":"safe.txt"}),
            )]),
            "",
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn inactive_saved_credential_split_across_text_and_identities_is_never_proposed() {
        const SENTINEL: &str = "identity-prefix_identity";
        const PREFIX: &str = "identity-";

        assert_inactive_provider_tool_ingress_blocked(
            SENTINEL,
            ProposalProviderTurn::TextAndTools {
                text: PREFIX.to_owned(),
                calls: vec![scripted_tool_call(
                    "prefix_",
                    "identity",
                    serde_json::json!({"path":"safe.txt"}),
                )],
            },
            PREFIX,
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn inactive_saved_credential_split_across_argument_key_and_value_is_never_proposed() {
        const SENTINEL: &str = "key-fragmentvalue-fragment";

        assert_inactive_provider_tool_ingress_blocked(
            SENTINEL,
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "key-value-call",
                "fs_write",
                serde_json::json!({"key-fragment":"value-fragment"}),
            )]),
            "",
            0,
        )
        .await;
    }

    #[tokio::test]
    async fn inactive_saved_credential_split_across_tool_calls_is_never_completed_durably() {
        const SENTINEL: &str = "call-prefixcall-suffix";

        assert_inactive_provider_tool_ingress_blocked(
            SENTINEL,
            ProposalProviderTurn::Tools(vec![
                scripted_tool_call(
                    "first-safe-call",
                    "fs_read",
                    serde_json::json!({"path":"call-prefix"}),
                ),
                scripted_tool_call(
                    "call-suffix",
                    "fs_read",
                    serde_json::json!({"path":"safe.txt"}),
                ),
            ]),
            "",
            1,
        )
        .await;
    }

    #[tokio::test]
    async fn inactive_saved_credential_in_local_tool_output_is_never_saved_or_redispatched() {
        const SENTINEL: &str = "inactive-local-tool-output-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join(".env"), SENTINEL).expect("tool output fixture");
        let database = directory.path().join("tool-output-secret.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ToolLoopProvider::reading());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "read protected workspace output".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        assert_eq!(permission.tool_name, "fs_read");
        let allow_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: allow_request,
                tool_call_id: permission.tool_call_id,
                allow: true,
            })
            .await
            .expect("allow permission");
        expect_commit(&mut ui, allow_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    } if text == "tool complete"
                )
            })
        })
        .await;

        let recovered = handle
            .load_session(session_id.clone())
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(recovered.tool_calls().len(), 1);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Unknown
        );
        assert!(recovered.tool_calls()[0].output().is_none());
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert!(
            events
                .iter()
                .any(|event| matches!(event.payload(), EventPayload::ToolCallMarkedUnknown { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.payload(), EventPayload::ToolCallCompleted { .. }))
        );
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains(SENTINEL)
        );
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            assert!(
                !serde_json::to_string(&*requests)
                    .expect("serialize requests")
                    .contains(SENTINEL)
            );
        }

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn recovered_tool_continuation_seeds_split_provider_credential_guard() {
        const SENTINEL: &str = "recovered-provider-output-secret";
        const PREFIX: &str = "recovered-provider-output-";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("ordinary.txt"), "ordinary tool output")
            .expect("ordinary tool fixture");
        let database = directory.path().join("recovered-output-secret.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::TextAndTools {
                text: PREFIX.to_owned(),
                calls: vec![scripted_tool_call(
                    "recovery-read",
                    "fs_read",
                    serde_json::json!({"path":"ordinary.txt"}),
                )],
            },
            ProposalProviderTurn::CompleteOwned("secret".to_owned()),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "resume a protected provider response".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        shutdown.cancel();
        task.await
            .expect("first coordinator join")
            .expect("first coordinator shutdown");
        drop(handle);
        actor.shutdown().await.expect("first actor shutdown");
        drop(ui);

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(
            recovered.attempts().last().expect("attempt").status(),
            EngineAttemptStatus::AwaitingTools
        );
        let reopened_handle = reopened.handle();
        let (mut resumed_ui, resumed_app) = bounded_ports(
            Arc::new(projection::session(&recovered)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let resumed_profiles = ProfileRuntime::new(
            manager,
            Arc::new(|_, _, _| Ok(Arc::new(RecordingProvider::default()) as Arc<dyn Provider>)),
            EnvironmentCredentials {
                gemini: None,
                router: None,
            },
            workspace.to_string_lossy().into_owned(),
        );
        let resumed_shutdown = CancellationToken::new();
        let resumed = Coordinator::with_runtime(
            session_id.clone(),
            recovered,
            reopened_handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(resumed_profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            resumed_app,
            resumed_shutdown.clone(),
        );
        let resumed_task = tokio::spawn(resumed.run());
        wait_for_catalog(&mut resumed_ui).await;
        let recovered_permission = wait_for_session(&mut resumed_ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        assert_eq!(
            recovered_permission.permission_requests[0].tool_call_id,
            permission.tool_call_id
        );
        let allow_request = RequestId::new(3);
        resumed_ui
            .intents
            .send(UiIntent::AnswerPermission {
                request_id: allow_request,
                tool_call_id: permission.tool_call_id,
                allow: true,
            })
            .await
            .expect("allow permission");
        expect_commit(&mut resumed_ui, allow_request).await;
        wait_for_session(&mut resumed_ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        ..
                    } if text == PREFIX && code == "credential_in_provider_data"
                )
            })
        })
        .await;

        let recovered = reopened_handle
            .load_session(session_id.clone())
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(recovered.tool_calls().len(), 1);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Completed
        );
        assert_eq!(
            recovered.tool_calls()[0]
                .output()
                .expect("safe output")
                .content(),
            "ordinary tool output"
        );
        let events = reopened_handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains(SENTINEL)
        );
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            assert!(
                !serde_json::to_string(&*requests)
                    .expect("serialize requests")
                    .contains(SENTINEL)
            );
        }

        resumed_shutdown.cancel();
        resumed_task
            .await
            .expect("resumed coordinator join")
            .expect("resumed coordinator shutdown");
        drop(reopened_handle);
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn provider_configuration_mutations_are_rejected_while_attempt_is_active() {
        const SENTINEL: &str = "credential-before-active-attempt";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("active-credential-mutation.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, manager, vault, reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider = Arc::new(FakeProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id,
            session,
            actor.handle(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "keep this attempt active".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Streaming,
                        ..
                    } if text == "partial"
                )
            })
        })
        .await;

        let mutation_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::ReplaceProfileCredential {
                request_id: mutation_request,
                profile_id: "inactive-redaction-profile".to_owned(),
                credential: ApiCredential::new("replacement-during-active".to_owned())
                    .expect("replacement credential"),
            })
            .await
            .expect("replace credential");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("mutation notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == mutation_request && failure.code == "credential_change_active"
        ));
        assert_eq!(
            vault.load(&reference).expect("saved credential").as_str(),
            SENTINEL
        );

        let profile_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::UpsertProfile {
                request_id: profile_request,
                profile: ProviderProfileDraft {
                    id: "active-mutation-codex".to_owned(),
                    kind: ProviderKindLabel::CodexCli,
                    base_url: String::new(),
                    project: String::new(),
                    auth_header: String::new(),
                },
            })
            .await
            .expect("upsert profile");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("profile notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == profile_request && failure.code == "credential_change_active"
        ));
        assert!(
            manager
                .snapshot()
                .expect("profile snapshot")
                .profiles
                .iter()
                .all(|profile| profile.id.as_str() != "active-mutation-codex")
        );

        let default_request = RequestId::new(5);
        ui.intents
            .send(UiIntent::SetProfileDefault {
                request_id: default_request,
                profile_id: "inactive-redaction-profile".to_owned(),
                model: fixture_model(),
                reasoning_effort: Some("high".to_owned()),
            })
            .await
            .expect("set profile default");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("default notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == default_request && failure.code == "credential_change_active"
        ));

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn missing_saved_credential_blocks_prompt_and_memory_before_persistence() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory
            .path()
            .join("missing-redaction-credential.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, vault, reference) =
            inactive_profile_runtime(&profile_path, &workspace, "missing-vault-secret");
        vault.delete(&reference).expect("remove saved credential");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        assert_eq!(
            coordinator.redact_configured_secrets(
                "token=overlapping-secret",
                &[
                    Zeroizing::new("overlapping".to_owned()),
                    Zeroizing::new("overlapping-secret".to_owned()),
                ],
            ),
            "token=[REDACTED]"
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;

        let memory_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::RememberMemory {
                request_id: memory_request,
                content: autoharness_tui::MemoryContent::new("ordinary safe memory")
                    .expect("memory content"),
            })
            .await
            .expect("remember intent");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("memory notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == memory_request
                    && failure.code == "memory_redaction_unavailable"
        ));
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("memory generation")
                .get(),
            0
        );

        let select_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "ordinary safe prompt".to_owned(),
            })
            .await
            .expect("submit prompt");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("prompt notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == submit_request
                    && failure.code == "prompt_redaction_unavailable"
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(
            !handle
                .load_events(session_id)
                .await
                .expect("session events")
                .iter()
                .any(|event| matches!(event.payload(), EventPayload::AttemptPrepared { .. }))
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn later_configured_credential_blocks_existing_memory_admission() {
        const SENTINEL: &str = "credential-configured-after-memory";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("later-credential.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        seed_workspace_memory(
            &handle,
            &workspace,
            &format!("needle-existing-memory {SENTINEL}"),
        )
        .await;
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let provider = Arc::new(RecordingProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "needle-existing-memory".to_owned(),
            })
            .await
            .expect("submit prompt");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("context notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == submit_request && failure.code == "context_not_committed"
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(
            !handle
                .load_events(session_id)
                .await
                .expect("session events")
                .iter()
                .any(|event| matches!(event.payload(), EventPayload::ContextTurnBound { .. }))
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn newly_configured_credential_blocks_recovered_bound_request() {
        const SENTINEL: &str = "credential-configured-after-context-binding";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(
            workspace.join("AGENTS.md"),
            format!("Treat {SENTINEL} as ordinary text for the initial binding."),
        )
        .expect("workspace source");
        let database = directory.path().join("recovered-credential.sqlite3");
        let (session_id, attempt_id, _request) =
            seed_pending_bound_turn(database.clone(), &workspace).await;
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, SENTINEL);
        let (actor, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&recovered)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            recovered,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(provider.requests.lock().expect("request lock").is_empty());
        let recovered = handle
            .load_session(session_id.clone())
            .await
            .expect("load session")
            .expect("recovered session");
        let attempt = recovered.attempt(&attempt_id).expect("pending attempt");
        assert!(attempt.is_provider_dispatch_ready());
        assert_eq!(attempt.turns_started(), 0);
        assert!(
            !handle
                .load_events(session_id)
                .await
                .expect("session events")
                .iter()
                .any(|event| matches!(event.payload(), EventPayload::RunTurnStarted { .. }))
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    fn all_file_bytes(root: &std::path::Path) -> Vec<Vec<u8>> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            for entry in std::fs::read_dir(path).expect("read test directory") {
                let entry = entry.expect("directory entry");
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(std::fs::read(path).expect("read durable test surface"));
                }
            }
        }
        files
    }

    fn inactive_profile_runtime(
        profile_path: &std::path::Path,
        workspace: &std::path::Path,
        secret: &str,
    ) -> (
        ProfileRuntime,
        Arc<ProfileManager>,
        Arc<FakeVault>,
        CredentialReference,
    ) {
        let store = ProfileStore::open(profile_path).expect("profile store");
        let vault = Arc::new(FakeVault::new());
        let manager = Arc::new(ProfileManager::new(store, vault.clone()));
        let inactive = ProfileId::new("inactive-redaction-profile").expect("profile ID");
        manager
            .upsert(&inactive, &ProviderProfile::gemini())
            .expect("inactive profile");
        let reference = manager
            .save_credential(&inactive, secret)
            .expect("inactive credential");
        assert!(
            manager
                .snapshot()
                .expect("profile snapshot")
                .profiles
                .iter()
                .any(|profile| profile.id == inactive && !profile.active)
        );
        let factory: ProfileProviderFactory =
            Arc::new(|_, _, _| Ok(Arc::new(RecordingProvider::default()) as Arc<dyn Provider>));
        (
            ProfileRuntime::new(
                Arc::clone(&manager),
                factory,
                EnvironmentCredentials {
                    gemini: None,
                    router: None,
                },
                workspace.to_string_lossy().into_owned(),
            ),
            manager,
            vault,
            reference,
        )
    }

    async fn assert_inactive_provider_tool_ingress_blocked(
        sentinel: &str,
        turn: ProposalProviderTurn,
        expected_text: &str,
        expected_tool_calls: usize,
    ) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("provider-tool-ingress.sqlite3");
        let profile_path = directory.path().join("profiles.json");
        let (profiles, _manager, _vault, _reference) =
            inactive_profile_runtime(&profile_path, &workspace, sentinel);
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![turn]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id.clone(),
            session,
            handle.clone(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: Some(provider.clone()),
                    factory: Arc::new(|_| {
                        Err(ProviderError::new(
                            ProviderErrorKind::MissingCredential,
                            RetryAdvice::Never,
                        ))
                    }),
                },
                profiles: Some(profiles),
                tool_runtime: test_tool_runtime_at(&workspace),
                artifact_root: Some(workspace.join("artifacts")),
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "attempt protected provider tool data".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        ..
                    } if text == expected_text && code == "credential_in_provider_data"
                )
            })
        })
        .await;

        let recovered = handle
            .load_session(session_id.clone())
            .await
            .expect("load session")
            .expect("session");
        assert_eq!(recovered.tool_calls().len(), expected_tool_calls);
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.payload(), EventPayload::ToolCallProposed { .. }))
                .count(),
            expected_tool_calls
        );
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains(sentinel)
        );
        assert_eq!(provider.requests.lock().expect("request lock").len(), 1);

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    async fn wait_for_profiles(
        profiles: &mut watch::Receiver<Arc<ProfilesProjection>>,
        predicate: impl Fn(&ProfilesProjection) -> bool,
    ) -> Arc<ProfilesProjection> {
        loop {
            let current = Arc::clone(&profiles.borrow_and_update());
            if predicate(&current) {
                return current;
            }
            tokio::time::timeout(Duration::from_secs(5), profiles.changed())
                .await
                .expect("profiles timeout")
                .expect("profiles sender remains open");
        }
    }

    #[tokio::test]
    async fn composed_multi_provider_profile_lifecycle_is_scoped_and_restart_safe() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("profiles.sqlite3");
        let profile_path = directory.path().join("autoharness.profiles.json");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let store = ProfileStore::open(&profile_path).expect("profile store");
        let vault = Arc::new(FakeVault::new());
        let manager = Arc::new(ProfileManager::new(store.clone(), vault.clone()));
        let built_kinds = Arc::new(Mutex::new(Vec::new()));
        let kinds = Arc::clone(&built_kinds);
        let profile_factory: ProfileProviderFactory = Arc::new(move |_id, profile, _credential| {
            kinds.lock().expect("kind mutex").push(profile.kind());
            Ok(Arc::new(FakeProvider::default()) as Arc<dyn Provider>)
        });
        let fallback_provider = Arc::new(FakeProvider::default());
        let fallback_factory: ProviderFactory =
            Arc::new(move |_credential| Ok(Arc::clone(&fallback_provider) as Arc<dyn Provider>));
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id,
            session,
            actor.handle(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: None,
                    factory: fallback_factory,
                },
                profiles: Some(ProfileRuntime::new(
                    Arc::clone(&manager),
                    profile_factory,
                    EnvironmentCredentials {
                        gemini: None,
                        router: None,
                    },
                    directory.path().to_string_lossy().into_owned(),
                )),
                tool_runtime: test_tool_runtime(),
                artifact_root: None,
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let gemini = ProviderProfileDraft {
            id: "personal-gemini".to_owned(),
            kind: ProviderKindLabel::Gemini,
            base_url: String::new(),
            project: String::new(),
            auth_header: String::new(),
        };
        ui.intents
            .send(UiIntent::UpsertProfile {
                request_id: RequestId::new(1),
                profile: gemini,
            })
            .await
            .expect("create Gemini profile");
        expect_commit(&mut ui, RequestId::new(1)).await;
        ui.intents
            .send(UiIntent::SaveProfileCredential {
                request_id: RequestId::new(2),
                profile_id: "personal-gemini".to_owned(),
                credential: ApiCredential::new("gemini-profile-secret".to_owned())
                    .expect("Gemini credential"),
            })
            .await
            .expect("save Gemini credential");
        expect_commit(&mut ui, RequestId::new(2)).await;

        let router = ProviderProfileDraft {
            id: "work-router".to_owned(),
            kind: ProviderKindLabel::Router,
            base_url: "https://router.example.test/v1/".to_owned(),
            project: "work".to_owned(),
            auth_header: "x-router-key".to_owned(),
        };
        ui.intents
            .send(UiIntent::UpsertProfile {
                request_id: RequestId::new(3),
                profile: router,
            })
            .await
            .expect("create router profile");
        expect_commit(&mut ui, RequestId::new(3)).await;
        ui.intents
            .send(UiIntent::SaveProfileCredential {
                request_id: RequestId::new(4),
                profile_id: "work-router".to_owned(),
                credential: ApiCredential::new("router-profile-secret".to_owned())
                    .expect("router credential"),
            })
            .await
            .expect("save router credential");
        expect_commit(&mut ui, RequestId::new(4)).await;

        ui.intents
            .send(UiIntent::ActivateProfile {
                request_id: RequestId::new(5),
                profile_id: "personal-gemini".to_owned(),
            })
            .await
            .expect("activate Gemini");
        expect_commit(&mut ui, RequestId::new(5)).await;
        wait_for_catalog(&mut ui).await;
        ui.intents
            .send(UiIntent::SetProfileDefault {
                request_id: RequestId::new(101),
                profile_id: "personal-gemini".to_owned(),
                model: fixture_model(),
                reasoning_effort: Some("high".to_owned()),
            })
            .await
            .expect("set Gemini model and thinking defaults");
        expect_commit(&mut ui, RequestId::new(101)).await;
        assert_eq!(
            manager
                .snapshot()
                .expect("default snapshot")
                .profiles
                .into_iter()
                .find(|profile| profile.id.as_str() == "personal-gemini")
                .and_then(|profile| profile.profile.default_model().map(str::to_owned))
                .as_deref(),
            Some("models/gemini-fixture")
        );
        assert_eq!(
            manager
                .snapshot()
                .expect("default snapshot")
                .profiles
                .into_iter()
                .find(|profile| profile.id.as_str() == "personal-gemini")
                .and_then(|profile| {
                    profile
                        .profile
                        .default_reasoning_effort()
                        .map(str::to_owned)
                })
                .as_deref(),
            Some("high")
        );
        let previous_session = ui.sessions.borrow().session_id.clone();
        ui.intents
            .send(UiIntent::CreateSession {
                request_id: RequestId::new(102),
            })
            .await
            .expect("create session with agent default");
        expect_commit(&mut ui, RequestId::new(102)).await;
        let created = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id != previous_session && projection.selected_model.is_some()
        })
        .await;
        assert_eq!(created.selected_model.as_ref(), Some(&fixture_model()));
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: RequestId::new(103),
                model: alternate_model(),
            })
            .await
            .expect("select a session-specific model");
        expect_commit(&mut ui, RequestId::new(103)).await;
        ui.intents
            .send(UiIntent::RefreshCatalog {
                request_id: RequestId::new(104),
            })
            .await
            .expect("refresh catalog without replacing the session model");
        expect_commit(&mut ui, RequestId::new(104)).await;
        assert_eq!(
            ui.sessions.borrow().selected_model.as_ref(),
            Some(&alternate_model()),
            "catalog refresh must not overwrite a session-specific model with the profile default"
        );
        ui.intents
            .send(UiIntent::TestProfile {
                request_id: RequestId::new(6),
                profile_id: "personal-gemini".to_owned(),
            })
            .await
            .expect("test Gemini");
        expect_commit(&mut ui, RequestId::new(6)).await;

        ui.intents
            .send(UiIntent::ActivateProfile {
                request_id: RequestId::new(7),
                profile_id: "work-router".to_owned(),
            })
            .await
            .expect("activate router");
        expect_commit(&mut ui, RequestId::new(7)).await;
        wait_for_catalog(&mut ui).await;
        ui.intents
            .send(UiIntent::TestProfile {
                request_id: RequestId::new(8),
                profile_id: "work-router".to_owned(),
            })
            .await
            .expect("test router");
        expect_commit(&mut ui, RequestId::new(8)).await;

        let projection = wait_for_profiles(&mut ui.profiles, |profiles| {
            profiles.profiles.iter().any(|profile| {
                profile.id == "work-router"
                    && profile.active
                    && profile.connection == ProfileConnectionState::Ready
            })
        })
        .await;
        assert_eq!(
            projection.user.default_profile.as_deref(),
            Some("work-router")
        );
        assert_eq!(
            projection.user.workspace,
            directory.path().to_string_lossy()
        );

        ui.intents
            .send(UiIntent::DeleteProfile {
                request_id: RequestId::new(9),
                profile_id: "work-router".to_owned(),
            })
            .await
            .expect("delete router");
        expect_commit(&mut ui, RequestId::new(9)).await;

        let snapshot = manager.snapshot().expect("profile snapshot");
        assert_eq!(snapshot.profiles.len(), 1);
        assert_eq!(snapshot.profiles[0].id.as_str(), "personal-gemini");
        assert_eq!(
            snapshot.profiles[0].credential_state,
            StoredCredentialState::Stored
        );
        let gemini_reference =
            CredentialReference::new("autoharness/profile/personal-gemini").expect("reference");
        let router_reference =
            CredentialReference::new("autoharness/profile/work-router").expect("reference");
        assert_eq!(
            &*vault.load(&gemini_reference).expect("Gemini remains"),
            "gemini-profile-secret"
        );
        assert!(matches!(
            vault.load(&router_reference),
            Err(VaultError::MissingEntry)
        ));
        let document = store.read_document().expect("profile document");
        assert!(!document.contains("gemini-profile-secret"));
        assert!(!document.contains("router-profile-secret"));

        let kinds = built_kinds.lock().expect("kind mutex").clone();
        assert!(kinds.contains(&ProviderKind::Gemini));
        assert!(kinds.contains(&ProviderKind::Router));

        shutdown.cancel();
        task.await.expect("coordinator task").expect("coordinator");
        actor.shutdown().await.expect("engine shutdown");
        let reopened = ProfileStore::open(&profile_path)
            .expect("reopen profile store")
            .snapshot()
            .expect("reopen snapshot");
        assert_eq!(reopened.profiles.len(), 1);
        assert_eq!(reopened.profiles[0].id.as_str(), "personal-gemini");
    }

    #[tokio::test]
    async fn new_session_intent_commits_without_a_provider_or_catalog() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("new-session.sqlite3");
        let (actor, original_session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            original_session_id.clone(),
            session,
            actor.handle(),
            None,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let request_id = RequestId::new(1);
        ui.intents
            .send(UiIntent::CreateSession { request_id })
            .await
            .expect("new session intent");
        expect_commit(&mut ui, request_id).await;
        let created = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id != original_session_id.as_str()
        })
        .await;
        assert_eq!(created.revision, 1);
        assert!(created.transcript.is_empty());
        assert!(created.selected_model.is_none());
        let list =
            wait_for_session_list(&mut ui.session_lists, |list| list.sessions.len() >= 2).await;
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|entry| { entry.session_id == created.session_id && entry.active })
        );

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let store = SqliteStore::open(database).expect("reopen store");
        let created_session_id = SessionId::new(&created.session_id).expect("created session ID");
        let summaries = store.list_sessions().expect("session summaries");
        assert!(
            summaries
                .iter()
                .any(|summary| summary.session_id() == &created_session_id),
            "the fresh session must remain durably discoverable"
        );
    }

    #[tokio::test]
    async fn deleting_the_current_session_switches_to_the_next_open_session() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("delete-current.sqlite3");
        let (actor, first_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            first_id.clone(),
            session,
            actor.handle(),
            None,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());
        let _ =
            wait_for_session_list(&mut ui.session_lists, |list| !list.sessions.is_empty()).await;

        let create_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::CreateSession {
                request_id: create_request,
            })
            .await
            .expect("create intent");
        expect_commit(&mut ui, create_request).await;
        let second = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id != first_id.as_str()
        })
        .await;

        let delete_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::DeleteSession {
                request_id: delete_request,
                session_id: second.session_id.clone(),
            })
            .await
            .expect("delete current intent");
        expect_commit(&mut ui, delete_request).await;
        let replacement = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id == first_id.as_str()
        })
        .await;
        assert_eq!(replacement.session_id, first_id.as_str());
        let listed =
            wait_for_session_list(&mut ui.session_lists, |list| list.sessions.len() == 1).await;
        assert_eq!(listed[0].session_id, first_id.as_str());
        assert!(listed[0].active);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let store = SqliteStore::open(database).expect("reopen store");
        let summaries = store.list_sessions().expect("session summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].session_id(), &first_id);
        let archive_prefix = format!("autoharness-session-{}.export.v3-", second.session_id);
        assert!(
            std::fs::read_dir(directory.path())
                .expect("archive directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .any(|name| name.starts_with(&archive_prefix) && name.ends_with(".json"))
        );
    }

    #[tokio::test]
    async fn two_sessions_switch_rename_and_survive_restart_with_replay_equivalence() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("lifecycle.sqlite3");
        let (actor, first_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            first_id.clone(),
            session,
            actor.handle(),
            None,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        // Wait for the initial session list publication.
        let listed =
            wait_for_session_list(&mut ui.session_lists, |list| !list.sessions.is_empty()).await;
        assert_eq!(listed.len(), 1);
        assert!(listed[0].active);

        // Create a second session; the coordinator activates it.
        let create_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::CreateSession {
                request_id: create_request,
            })
            .await
            .expect("create intent");
        expect_commit(&mut ui, create_request).await;
        let second = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id != first_id.as_str()
        })
        .await;
        let second_id = SessionId::new(&second.session_id).expect("second session ID");

        // Rename the second session and observe the browser list update.
        let rename_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::RenameSession {
                request_id: rename_request,
                session_id: second.session_id.clone(),
                title: "Deep dive".to_owned(),
            })
            .await
            .expect("rename intent");
        expect_commit(&mut ui, rename_request).await;
        let renamed = wait_for_session_list(&mut ui.session_lists, |list| {
            list.sessions
                .iter()
                .any(|entry| entry.title == "Deep dive" && entry.active)
        })
        .await;
        assert_eq!(renamed.len(), 2);

        // Archive the first session while working in the second.
        let archive_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::ArchiveSession {
                request_id: archive_request,
                session_id: first_id.as_str().to_owned(),
            })
            .await
            .expect("archive intent");
        expect_commit(&mut ui, archive_request).await;
        let archived = wait_for_session_list(&mut ui.session_lists, |list| {
            list.sessions
                .iter()
                .any(|entry| entry.session_id == first_id.as_str() && entry.archived)
        })
        .await;
        assert_eq!(archived.len(), 2);
        assert_eq!(
            ui.sessions.borrow().session_id,
            second_id.as_str(),
            "mutating an inactive session must not replace the active projection"
        );

        // Switch back into the first session; unarchive it first because an
        // archived session accepts no ordinary commands after activation.
        let unarchive_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::UnarchiveSession {
                request_id: unarchive_request,
                session_id: first_id.as_str().to_owned(),
            })
            .await
            .expect("unarchive intent");
        expect_commit(&mut ui, unarchive_request).await;
        let open_request = RequestId::new(5);
        ui.intents
            .send(UiIntent::OpenSession {
                request_id: open_request,
                session_id: first_id.as_str().to_owned(),
            })
            .await
            .expect("open intent");
        expect_commit(&mut ui, open_request).await;
        let reopened = wait_for_session(&mut ui.sessions, |projection| {
            projection.session_id == first_id.as_str()
        })
        .await;
        // Created, archived, and unarchived: three durable events so far.
        assert_eq!(reopened.revision, 3);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        // Restart replays both sessions with their durable lifecycle state.
        let (actor, active_id, active_session) =
            crate::engine_actor::EngineActor::start(database).expect("restart engine actor");
        assert_eq!(active_id, first_id, "the most recent session reactivates");
        assert!(!active_session.is_archived());
        assert_eq!(
            active_session
                .title()
                .map(autoharness_domain::SessionTitle::as_str),
            None,
            "the first session was never renamed"
        );

        let summaries = actor
            .handle()
            .list_sessions()
            .await
            .expect("restart listing");
        assert_eq!(summaries.len(), 2);
        let second_summary = summaries
            .iter()
            .find(|summary| summary.session_id() == &second_id)
            .expect("second session summary");
        assert_eq!(
            second_summary
                .title()
                .map(autoharness_domain::SessionTitle::as_str),
            Some("Deep dive")
        );
        assert_eq!(
            second_summary.status(),
            autoharness_store::SessionStatus::Active
        );
        let replay = autoharness_engine::SessionAggregate::rehydrate(
            second_id.clone(),
            &actor
                .handle()
                .load_events(second_id)
                .await
                .expect("second session events"),
        )
        .expect("second session replays");
        assert_eq!(
            replay.title().map(autoharness_domain::SessionTitle::as_str),
            Some("Deep dive")
        );

        actor.shutdown().await.expect("restart shutdown");
    }

    async fn wait_for_session_list(
        ui: &mut tokio::sync::watch::Receiver<Arc<SessionsProjection>>,
        condition: impl Fn(&SessionsProjection) -> bool,
    ) -> Vec<SessionBrowserEntry> {
        loop {
            {
                let current = ui.borrow_and_update();
                if condition(&current) {
                    return current.sessions.clone();
                }
            }
            if ui.changed().await.is_err() {
                panic!("session list channel closed");
            }
        }
    }

    #[test]
    fn provider_request_excludes_partial_failed_assistant_output() {
        let session_id = SessionId::new("session-1").expect("valid session ID");
        let input_id = InputId::new("input-1").expect("valid input ID");
        let attempt_id = AttemptId::new("attempt-1").expect("valid attempt ID");
        let model = ModelRef::new(
            ProviderId::new("gemini").expect("valid provider ID"),
            ModelId::new("models/gemini-test").expect("valid model ID"),
        );
        let payloads = [
            EventPayload::SessionCreated,
            EventPayload::ModelSelected {
                model: model.clone(),
            },
            EventPayload::InputAdmitted {
                input_id: input_id.clone(),
                prompt: PromptText::new("hello").expect("valid prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: attempt_id.clone(),
                input_id,
                model,
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: attempt_id.clone(),
            },
            EventPayload::AttemptTextAppended {
                attempt_id: attempt_id.clone(),
                text: ResponseText::new("partial secret").expect("valid response"),
            },
        ];
        let events: Vec<_> = payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                EventEnvelope::new_v1(
                    EventId::new(format!("event-{index}")).expect("valid event ID"),
                    session_id.clone(),
                    SessionSequence::new(index as u64 + 1).expect("valid sequence"),
                    TimestampMillis::new(index as i64),
                    Causation::Command(
                        CommandId::new(format!("command-{index}")).expect("valid command ID"),
                    ),
                    CorrelationId::new(format!("correlation-{index}"))
                        .expect("valid correlation ID"),
                    payload,
                )
            })
            .collect();
        let aggregate = SessionAggregate::rehydrate(session_id, &events).expect("valid history");

        let request = build_request(&aggregate, &attempt_id, true).expect("valid provider request");
        let plain_request =
            build_request(&aggregate, &attempt_id, false).expect("valid plain request");

        assert_eq!(request.messages.len(), 1);
        assert!(!request.tools.is_empty());
        assert!(plain_request.tools.is_empty());
        assert_eq!(
            request.messages[0]
                .content()
                .expect("text message")
                .as_str(),
            "hello"
        );
    }

    #[test]
    fn provider_request_does_not_replay_a_prior_failed_prompt() {
        let session_id = SessionId::new("session-failed-context").expect("session ID");
        let first_input = InputId::new("input-failed").expect("input ID");
        let second_input = InputId::new("input-current").expect("input ID");
        let first_attempt = AttemptId::new("attempt-failed").expect("attempt ID");
        let second_attempt = AttemptId::new("attempt-current").expect("attempt ID");
        let model = fixture_model();
        let payloads = [
            EventPayload::SessionCreated,
            EventPayload::ModelSelected {
                model: model.clone(),
            },
            EventPayload::InputAdmitted {
                input_id: first_input.clone(),
                prompt: PromptText::new("Search the web for news today").expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: first_attempt.clone(),
                input_id: first_input,
                model: model.clone(),
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: first_attempt.clone(),
            },
            EventPayload::AttemptFailed {
                attempt_id: first_attempt,
                failure: completion_failure(
                    ErrorClass::Protocol,
                    "protocol",
                    "The provider response was invalid",
                    RetryAdvice::Never,
                ),
            },
            EventPayload::InputAdmitted {
                input_id: second_input.clone(),
                prompt: PromptText::new("Hello").expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: second_attempt.clone(),
                input_id: second_input,
                model,
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: second_attempt.clone(),
            },
        ];
        let events: Vec<_> = payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                EventEnvelope::new_v1(
                    EventId::new(format!("failed-event-{index}")).expect("event ID"),
                    session_id.clone(),
                    SessionSequence::new(index as u64 + 1).expect("sequence"),
                    TimestampMillis::new(index as i64),
                    Causation::Command(
                        CommandId::new(format!("failed-command-{index}")).expect("command ID"),
                    ),
                    CorrelationId::new(format!("failed-correlation-{index}"))
                        .expect("correlation ID"),
                    payload,
                )
            })
            .collect();
        let aggregate = SessionAggregate::rehydrate(session_id, &events).expect("valid history");

        let request =
            build_request(&aggregate, &second_attempt, true).expect("valid provider request");

        assert_eq!(request.messages.len(), 1);
        assert_eq!(
            request.messages[0]
                .content()
                .expect("text message")
                .as_str(),
            "Hello"
        );
    }

    #[test]
    fn provider_request_cutoff_removes_only_completed_history() {
        let session_id = SessionId::new("session-compacted-request").expect("session ID");
        let prior_input = InputId::new("input-prior").expect("input ID");
        let current_input = InputId::new("input-current").expect("input ID");
        let prior_attempt = AttemptId::new("attempt-prior").expect("attempt ID");
        let current_attempt = AttemptId::new("attempt-current").expect("attempt ID");
        let model = fixture_model();
        let payloads = [
            EventPayload::SessionCreated,
            EventPayload::ModelSelected {
                model: model.clone(),
            },
            EventPayload::InputAdmitted {
                input_id: prior_input.clone(),
                prompt: PromptText::new("old committed input").expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: prior_attempt.clone(),
                input_id: prior_input,
                model: model.clone(),
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: prior_attempt.clone(),
            },
            EventPayload::AttemptTextAppended {
                attempt_id: prior_attempt.clone(),
                text: ResponseText::new("old committed response").expect("response"),
            },
            EventPayload::AttemptCompleted {
                attempt_id: prior_attempt,
            },
            EventPayload::InputAdmitted {
                input_id: current_input.clone(),
                prompt: PromptText::new("current unsettled input").expect("prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: current_attempt.clone(),
                input_id: current_input,
                model,
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: current_attempt.clone(),
            },
        ];
        let events = payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                EventEnvelope::new_v1(
                    EventId::new(format!("compacted-event-{index}")).expect("event ID"),
                    session_id.clone(),
                    SessionSequence::new(index as u64 + 1).expect("sequence"),
                    TimestampMillis::new(index as i64),
                    Causation::Command(
                        CommandId::new(format!("compacted-command-{index}")).expect("command ID"),
                    ),
                    CorrelationId::new(format!("compacted-correlation-{index}"))
                        .expect("correlation ID"),
                    payload,
                )
            })
            .collect::<Vec<_>>();
        let aggregate = SessionAggregate::rehydrate(session_id, &events).expect("valid history");
        let cutoff = aggregate
            .attempts()
            .first()
            .and_then(autoharness_engine::AttemptProjection::completed_sequence)
            .expect("durable completion sequence");

        let complete = build_request(&aggregate, &current_attempt, false).expect("full request");
        let compacted =
            build_request_after_cutoff(&aggregate, &current_attempt, false, Some(cutoff))
                .expect("cutoff request");

        assert_eq!(complete.messages.len(), 3);
        assert_eq!(compacted.messages.len(), 1);
        assert_eq!(
            compacted.messages[0]
                .content()
                .expect("current input")
                .as_str(),
            "current unsettled input"
        );
    }

    #[test]
    fn non_normal_provider_stops_become_visible_durable_failures() {
        let attempt_id = AttemptId::new("attempt-stop").expect("attempt ID");

        let length = completion_payload(
            &SessionId::new("session-stop").expect("session ID"),
            attempt_id.clone(),
            CompletionReason::Length,
        );
        let safety = completion_payload(
            &SessionId::new("session-stop").expect("session ID"),
            attempt_id,
            CompletionReason::Safety,
        );

        assert!(matches!(
            length,
            CommandPayload::FailAttempt { failure, .. }
                if failure.code().as_str() == "generation_limit"
                    && failure.retry_advice() == RetryAdvice::Immediate
        ));
        assert!(matches!(
            safety,
            CommandPayload::FailAttempt { failure, .. }
                if failure.code().as_str() == "safety_stop"
                    && failure.retry_advice() == RetryAdvice::Never
        ));
    }

    #[tokio::test]
    async fn in_app_credential_configures_provider_without_persisting_the_key() {
        let sentinel = "gemini-in-app-secret-sentinel";
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("credential.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider = Arc::new(FakeProvider::default());
        let provider_for_factory = provider.clone();
        let factory: ProviderFactory = Arc::new(move |credential| {
            let key = GeminiApiKey::new(credential.into_string())?;
            assert_eq!(format!("{key:?}"), "GeminiApiKey([REDACTED])");
            Ok(provider_for_factory.clone())
        });
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id,
            session,
            actor.handle(),
            ProviderComposition {
                initial: None,
                factory,
            },
            test_tool_runtime(),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let request_id = RequestId::new(1);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id,
                credential: ApiCredential::new(sentinel.to_owned()).expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        wait_for_catalog(&mut ui).await;
        expect_commit(&mut ui, request_id).await;

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        for entry in std::fs::read_dir(directory.path()).expect("read data directory") {
            let path = entry.expect("data file entry").path();
            if path.is_file() {
                let bytes = std::fs::read(path).expect("read data file");
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes()),
                    "in-app credential must not reach durable files"
                );
            }
        }
    }

    #[tokio::test]
    async fn slash_export_writes_markdown_beside_the_database_without_touching_history() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("export-md.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider_for_factory = Arc::new(FakeProvider::default());
        let factory: ProviderFactory = Arc::new(move |_credential| {
            Ok(Arc::clone(&provider_for_factory) as Arc<dyn autoharness_provider::Provider>)
        });
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            actor.handle(),
            ProviderComposition {
                initial: None,
                factory,
            },
            test_tool_runtime(),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        // Connect the fake provider through the same in-app path users take.
        let connect_id = RequestId::new(9);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: connect_id,
                credential: ApiCredential::new("export-fixture-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        expect_commit(&mut ui, connect_id).await;
        wait_for_catalog(&mut ui).await;

        // Select the fake catalog's model so submissions are admitted.
        let select_request = RequestId::new(8);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;

        // Admit one prompt so the transcript has content.
        let submit_id = RequestId::new(1);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_id,
                prompt: "export me".to_owned(),
            })
            .await
            .expect("submit intent");
        expect_commit(&mut ui, submit_id).await;

        // Export through the same typed intent the terminal dispatches.
        let export_id = RequestId::new(2);
        ui.intents
            .send(UiIntent::ExportTranscript {
                request_id: export_id,
                session_id: session_id.as_str().to_owned(),
            })
            .await
            .expect("export intent");
        expect_commit(&mut ui, export_id).await;

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .expect("read data directory")
            .collect();
        let markdown = entries
            .iter()
            .filter_map(|entry| entry.as_ref().ok())
            .find(|entry| {
                entry
                    .path()
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(".md"))
            })
            .map(|entry| entry.path())
            .expect("a Markdown transcript must exist beside the database");
        let contents = std::fs::read_to_string(&markdown).expect("read export");
        assert!(contents.contains("# "));
        assert!(contents.contains("export me"));

        // The exported session is untouched and still lists durably.
        let store =
            SqliteStore::open(directory.path().join("export-md.sqlite3")).expect("reopen store");
        assert_eq!(store.list_sessions().expect("sessions").len(), 1);
    }

    #[tokio::test]
    async fn rejected_in_app_credential_can_be_replaced_without_restarting() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("credential-retry.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let valid_provider = Arc::new(FakeProvider::default());
        let valid_provider_for_factory = valid_provider.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let factory_calls = calls.clone();
        let factory: ProviderFactory = Arc::new(move |credential| {
            let _key = GeminiApiKey::new(credential.into_string())?;
            if factory_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Arc::new(AuthenticationProvider))
            } else {
                Ok(valid_provider_for_factory.clone())
            }
        });
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id,
            session,
            actor.handle(),
            ProviderComposition {
                initial: None,
                factory,
            },
            test_tool_runtime(),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let rejected_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: rejected_request,
                credential: ApiCredential::new("syntactically-valid-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        let rejected = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            rejected,
            UiNotice::IntentRejected {
                request_id,
                failure: UiFailure {
                    class: ErrorClass::Authentication,
                    ..
                },
            } if request_id == rejected_request
        ));

        let accepted_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: accepted_request,
                credential: ApiCredential::new("replacement-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("replacement credential intent");
        wait_for_catalog(&mut ui).await;
        expect_commit(&mut ui, accepted_request).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn transient_credential_catalog_failure_retains_session_connection_for_refresh() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("credential-transient.sqlite3");
        let profile_path = directory.path().join("autoharness.profiles.json");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let store = ProfileStore::open(&profile_path).expect("profile store");
        let vault = Arc::new(FakeVault::new());
        let manager = Arc::new(ProfileManager::new(store, vault));
        let profile_id = ProfileId::new("session-gemini").expect("profile ID");
        manager
            .upsert(&profile_id, &ProviderProfile::gemini())
            .expect("profile upsert");
        manager
            .activate(Some(&profile_id))
            .expect("profile activation");
        let profile_factory: ProfileProviderFactory =
            Arc::new(|_, _, _| Ok(Arc::new(FakeProvider::default()) as Arc<dyn Provider>));
        let provider = Arc::new(TransientCatalogProvider::default());
        let provider_for_factory = Arc::clone(&provider);
        let factory: ProviderFactory =
            Arc::new(move |_credential| Ok(Arc::clone(&provider_for_factory) as Arc<dyn Provider>));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_runtime(
            session_id,
            session,
            actor.handle(),
            RuntimeComposition {
                provider: ProviderComposition {
                    initial: None,
                    factory,
                },
                profiles: Some(ProfileRuntime::new(
                    manager,
                    profile_factory,
                    EnvironmentCredentials {
                        gemini: None,
                        router: None,
                    },
                    directory.path().to_string_lossy().into_owned(),
                )),
                tool_runtime: test_tool_runtime(),
                artifact_root: None,
            },
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let credential_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: credential_request,
                credential: ApiCredential::new("transient-session-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let session_connected = {
                    let current = ui.settings.borrow_and_update();
                    current.provider_status.credential_connected
                        && current.provider_status.credential_source
                            == CredentialSourceLabel::SessionOnly
                };
                if session_connected {
                    break;
                }
                ui.settings
                    .changed()
                    .await
                    .expect("settings sender remains open");
            }
        })
        .await
        .expect("session credential projection timeout");
        let rejected = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            rejected,
            UiNotice::IntentRejected {
                request_id,
                failure: UiFailure {
                    class: ErrorClass::Timeout,
                    ..
                },
            } if request_id == credential_request
        ));
        assert!(ui.settings.borrow().provider_status.credential_connected);

        let refresh_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::RefreshCatalog {
                request_id: refresh_request,
            })
            .await
            .expect("refresh intent");
        wait_for_catalog(&mut ui).await;
        expect_commit(&mut ui, refresh_request).await;
        assert_eq!(provider.catalog_calls.load(Ordering::SeqCst), 2);
        assert!(ui.settings.borrow().provider_status.credential_connected);
        let active_profile = ui
            .profiles
            .borrow()
            .profiles
            .iter()
            .find(|profile| profile.active)
            .expect("active profile")
            .clone();
        assert_eq!(active_profile.connection, ProfileConnectionState::Ready);

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn configured_router_runs_the_same_catalog_and_session_path() {
        let (base_url, server) = spawn_router_fixture().await;
        let settings =
            RouterSettings::new(base_url.parse().expect("fixture URL"), Some("composed"))
                .expect("router settings")
                .with_authentication("x-router-key", "Token")
                .expect("router authentication");
        let provider = Arc::new(
            OpenAiRouterProvider::new(
                settings,
                RouterCredential::new("router-composed-secret").expect("credential"),
            )
            .expect("router provider"),
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("router-composition.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id,
            session,
            actor.handle(),
            Some(provider),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let model = ModelRef::new(
            ProviderId::new("router:composed").expect("provider ID"),
            ModelId::new("router-model").expect("model ID"),
        );
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: model.clone(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "router composed prompt".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    } if text == "router response"
                )
            })
        })
        .await;
        assert_eq!(completed.selected_model.as_ref(), Some(&model));

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
        let requests = server.await.expect("fixture server");
        assert_eq!(
            requests,
            vec!["GET /v1/models?limit=1000", "POST /v1/chat/completions"]
        );
    }

    #[tokio::test]
    async fn invalid_tool_call_is_denied_durably_and_repaired_in_the_same_attempt() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("invalid-tool-repair.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(InvalidToolRepairProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id.clone(),
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Search the web for news today".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    } if text == "recovered after invalid tool call"
                )
            })
        })
        .await;
        assert!(completed.permission_requests.is_empty());

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            assert!(
                requests[1]
                    .messages
                    .iter()
                    .any(|message| matches!(message, ChatMessage::ToolCall(call) if call.tool_name.as_str() == "web_search"))
            );
            assert!(requests[1].messages.iter().any(|message| {
                matches!(
                    message,
                    ChatMessage::ToolResult { content, .. }
                        if content.as_str().contains("use only an advertised tool")
                )
            }));
        }

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(recovered.attempts().len(), 1);
        assert_eq!(recovered.attempts()[0].turns_started(), 2);
        assert_eq!(recovered.tool_calls().len(), 1);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Denied
        );
        assert_eq!(
            recovered.tool_calls()[0].call().capability.kind,
            autoharness_domain::CapabilityKind::InvalidToolCall
        );
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn repeated_invalid_tool_calls_stop_at_the_durable_turn_limit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("invalid-tool-limit.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(InvalidToolRepairProvider::never_repairs());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id.clone(),
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Keep requesting an unknown tool".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        ..
                    } if code == "tool_turn_limit"
                )
            })
        })
        .await;

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            usize::try_from(RunLimits::default().max_turns).expect("turn limit fits usize")
        );

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(recovered.attempts()[0].turns_started(), 8);
        assert_eq!(recovered.tool_calls().len(), 8);
        assert!(recovered.tool_calls().iter().all(|call| {
            call.status() == autoharness_engine::ToolCallStatus::Denied
                && call.call().capability.kind
                    == autoharness_domain::CapabilityKind::InvalidToolCall
        }));
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn compaction_survives_restart_without_reexpanding_raw_history() {
        const FIRST_INPUT: &str = "old input that must not reappear after compaction";
        const FIRST_OUTPUT_PREFIX: &str = "old-output-";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("compaction-restart.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(CompactionProvider::new([
            format!("{FIRST_OUTPUT_PREFIX}{}", "history".repeat(1_200)),
            "second response".to_owned(),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace.clone();
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        submit_and_wait_for_completion(&mut ui, RequestId::new(2), FIRST_INPUT, 1).await;
        submit_and_wait_for_completion(
            &mut ui,
            RequestId::new(3),
            "second input triggers compaction",
            2,
        )
        .await;

        let checkpoint = handle
            .load_latest_compaction_checkpoint(session_id.clone())
            .await
            .expect("load checkpoint")
            .expect("compaction checkpoint");
        let summary_revision_id = checkpoint
            .boundary()
            .summary_revision_id()
            .expect("summary proposal")
            .clone();
        let proposal_page = handle
            .inspect_memories(
                MemoryInspectionQuery::new(
                    vec![DomainMemoryScope::Session(session_id.clone())],
                    vec![MemoryRevisionStatus::Proposed],
                    None,
                    10,
                )
                .expect("proposal query"),
            )
            .await
            .expect("proposal page");
        assert_eq!(proposal_page.records().len(), 1);
        let proposal = &proposal_page.records()[0];
        assert_eq!(
            proposal.latest_revision().revision_id(),
            &summary_revision_id
        );
        assert_eq!(
            proposal.latest_revision().origin(),
            MemoryOrigin::Compaction
        );
        assert_eq!(
            proposal.latest_revision().trust_class(),
            TrustClass::UntrustedProposal
        );
        assert_eq!(
            proposal.latest_revision().status(),
            MemoryRevisionStatus::Proposed
        );
        let operations = handle
            .load_memory_operations(proposal.memory_id().clone(), 0, 16)
            .await
            .expect("proposal operations");
        assert_eq!(operations.len(), 2);
        assert!(operations.iter().all(|operation| {
            !matches!(
                operation.payload(),
                MemoryOperationPayload::RevisionActivated { .. }
            )
        }));
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1].messages.len(), 1);
            assert!(
                requests[1]
                    .context
                    .as_ref()
                    .is_some_and(|context| context.as_str().contains(FIRST_INPUT))
            );
        }

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");

        let (reopened, reopened_session_id, reopened_session) =
            crate::engine_actor::EngineActor::start(database).expect("reopened actor");
        assert_eq!(reopened_session_id, session_id);
        let reopened_handle = reopened.handle();
        let restart_provider = Arc::new(CompactionProvider::new(["third response".to_owned()]));
        let (mut restart_ui, restart_app) = bounded_ports(
            Arc::new(projection::session(&reopened_session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let restart_shutdown = CancellationToken::new();
        let mut restart_coordinator = Coordinator::with_provider_factory(
            reopened_session_id,
            reopened_session,
            reopened_handle,
            ProviderComposition {
                initial: Some(restart_provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            restart_app,
            restart_shutdown.clone(),
        );
        restart_coordinator.workspace = workspace;
        let restart_task = tokio::spawn(restart_coordinator.run());
        wait_for_catalog(&mut restart_ui).await;
        submit_and_wait_for_completion(
            &mut restart_ui,
            RequestId::new(4),
            "third input after restart",
            3,
        )
        .await;
        {
            let requests = restart_provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 1);
            assert!(requests[0].messages.iter().all(|message| {
                message
                    .content()
                    .is_none_or(|content| !content.as_str().contains(FIRST_INPUT))
            }));
            assert!(requests[0].messages.iter().all(|message| {
                message
                    .content()
                    .is_none_or(|content| !content.as_str().contains(FIRST_OUTPUT_PREFIX))
            }));
            assert!(
                requests[0]
                    .context
                    .as_ref()
                    .is_some_and(|context| context.as_str().contains(FIRST_INPUT))
            );
        }

        restart_shutdown.cancel();
        restart_task
            .await
            .expect("restart coordinator join")
            .expect("restart shutdown");
        reopened.shutdown().await.expect("reopened shutdown");
    }

    #[tokio::test]
    async fn post_compaction_tool_continuation_freezes_replacement_epoch() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory
            .path()
            .join("compaction-tool-continuation.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::CompleteOwned(format!("old-{}", "history".repeat(1_200))),
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "invalid-before-compaction",
                "not_a_registered_tool",
                serde_json::json!({"stage":1,"payload":"x".repeat(6_000)}),
            )]),
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "invalid-after-compaction",
                "still_not_registered",
                serde_json::json!({"stage":2}),
            )]),
            ProposalProviderTurn::Complete("replacement epoch remained frozen"),
        ]));
        let (_ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown,
        );
        coordinator.workspace = workspace;
        coordinator
            .initialize_context_scope()
            .await
            .expect("context scope");
        coordinator.catalog_models = vec![fixture_model_descriptor()];
        coordinator
            .execute(CommandPayload::SelectModel {
                session_id: session_id.clone(),
                model: fixture_model(),
            })
            .await
            .expect("select model");

        let prior_attempt = ids::attempt_id();
        coordinator
            .execute(CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id.clone(),
                attempt_id: prior_attempt.clone(),
                input_id: ids::input_id(),
                prompt: PromptText::new("old turn").expect("prior prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            })
            .await
            .expect("prepare prior attempt");
        assert!(
            coordinator.start_attempt(prior_attempt, None).await.is_ok(),
            "prior attempt starts"
        );
        while coordinator.active.is_some() {
            handle_next_provider_message(&mut coordinator).await;
        }

        let current_attempt = ids::attempt_id();
        coordinator
            .execute(CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id.clone(),
                attempt_id: current_attempt.clone(),
                input_id: ids::input_id(),
                prompt: PromptText::new("current tool turn").expect("current prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            })
            .await
            .expect("prepare current attempt");
        let initial_request =
            build_request(&coordinator.session, &current_attempt, true).expect("initial request");
        let exact_bytes = u64::try_from(
            serde_json::to_vec(&initial_request)
                .expect("serialize initial request")
                .len(),
        )
        .expect("request byte count");
        let minimum_budget = exact_bytes
            .checked_mul(4)
            .expect("context sizing scale")
            .checked_add(2)
            .expect("context sizing rounding")
            / 3;
        let exact_token_limit = minimum_budget
            .checked_add(256)
            .expect("context sizing slack")
            .checked_add(CONTEXT_SIZER_BYTES_PER_TOKEN - 1)
            .expect("rounded byte count")
            / CONTEXT_SIZER_BYTES_PER_TOKEN;
        coordinator.catalog_models[0].input_token_limit = Some(exact_token_limit);
        match coordinator
            .start_attempt(current_attempt.clone(), None)
            .await
        {
            Ok(()) => {}
            Err(StartAttemptError::Engine(error)) => panic!("engine start failure: {error}"),
            Err(StartAttemptError::Provider(error)) => panic!("provider start failure: {error}"),
            Err(StartAttemptError::Context(error)) => panic!("context start failure: {error}"),
        }

        for _ in 0..3 {
            handle_next_provider_message(&mut coordinator).await;
        }
        let turn_two = handle
            .load_attempt_context_turn(current_attempt.clone(), 2)
            .await
            .expect("load turn two")
            .expect("compaction turn two");
        let replacement_epoch = handle
            .load_context_epoch(turn_two.epoch_id().clone())
            .await
            .expect("load replacement epoch")
            .expect("replacement epoch");
        assert_eq!(
            replacement_epoch.reason(),
            autoharness_domain::ContextEpochReason::Compaction
        );
        assert_eq!(turn_two.run_turn(), 2);
        assert_eq!(
            handle
                .load_latest_compaction_checkpoint(session_id.clone())
                .await
                .expect("checkpoint lookup")
                .expect("checkpoint")
                .baseline_turn(),
            &turn_two
        );

        for _ in 0..2 {
            handle_next_provider_message(&mut coordinator).await;
        }
        let prepared = coordinator
            .prepare_context_snapshot(&current_attempt, true)
            .await
            .expect("frozen continuation preparation");
        assert!(prepared.compaction_boundary.is_none());
        let prepared_turn_three = prepared.turn.manifest();
        assert_eq!(prepared_turn_three.epoch_id(), turn_two.epoch_id());
        assert_eq!(
            prepared_turn_three.rendered_hash(),
            turn_two.rendered_hash()
        );
        assert_eq!(
            prepared_turn_three.admissions().len(),
            turn_two.admissions().len()
        );
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 3);
            assert_eq!(requests[2].context, prepared.turn.request().context);
        }

        handle_next_provider_message(&mut coordinator).await;
        let turn_three = handle
            .load_attempt_context_turn(current_attempt.clone(), 3)
            .await
            .expect("load turn three")
            .expect("frozen continuation turn");
        assert_eq!(turn_three.run_turn(), 3);
        assert_eq!(turn_three.epoch_id(), turn_two.epoch_id());
        assert_eq!(turn_three.rendered_hash(), turn_two.rendered_hash());
        assert_eq!(turn_three.admissions().len(), turn_two.admissions().len());
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 4);
            assert_eq!(requests[2].context, requests[3].context);
        }
        for _ in 0..3 {
            handle_next_provider_message(&mut coordinator).await;
        }
        assert_eq!(
            coordinator
                .session
                .attempt(&current_attempt)
                .expect("current attempt")
                .status(),
            EngineAttemptStatus::Completed
        );

        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn secret_bearing_compaction_writes_neither_proposal_nor_boundary() {
        const SENTINEL: &str = "configured-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("compaction-secret.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(CompactionProvider::new([format!(
            "{SENTINEL}-{}",
            "history".repeat(1_200)
        )]));
        provider.allow_unprotected_seed();
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace;
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        submit_and_wait_for_completion(
            &mut ui,
            RequestId::new(2),
            "produce secret-bearing old history",
            1,
        )
        .await;
        provider.protect_configured_secret();
        let compact_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: compact_request,
                prompt: "this request would require compaction".to_owned(),
            })
            .await
            .expect("submit compaction prompt");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            notice,
            UiNotice::IntentRejected { request_id, failure }
                if request_id == compact_request && failure.code == "context_not_committed"
        ));

        assert!(
            handle
                .load_latest_compaction_checkpoint(session_id.clone())
                .await
                .expect("checkpoint lookup")
                .is_none()
        );
        let proposals = handle
            .inspect_memories(
                MemoryInspectionQuery::new(
                    vec![DomainMemoryScope::Session(session_id)],
                    vec![MemoryRevisionStatus::Proposed],
                    None,
                    10,
                )
                .expect("proposal query"),
            )
            .await
            .expect("proposal page");
        assert!(proposals.records().is_empty());

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn configured_credential_in_workspace_agents_is_rejected_before_context_commit() {
        const SENTINEL: &str = "configured-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(
            workspace.join("AGENTS.md"),
            format!("Never persist {SENTINEL} from this source."),
        )
        .expect("secret-bearing agents source");
        let database = directory.path().join("agents-secret.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace;
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "read the workspace instructions".to_owned(),
            })
            .await
            .expect("submit prompt");
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        match notice {
            UiNotice::IntentRejected {
                request_id,
                failure,
            } => {
                assert_eq!(request_id, submit_request);
                assert_eq!(failure.code, "context_not_committed");
                assert!(!format!("{failure:?}").contains(SENTINEL));
            }
            other => panic!("expected safe context rejection, got {other:?}"),
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(provider.requests.lock().expect("request lock").is_empty());
        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        assert!(
            !events
                .iter()
                .any(|event| { matches!(event.payload(), EventPayload::ContextTurnBound { .. }) })
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
        for entry in std::fs::read_dir(directory.path()).expect("state directory") {
            let entry = entry.expect("state entry");
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("agents-secret")
            {
                let bytes = std::fs::read(entry.path()).expect("durable state file");
                assert!(
                    !bytes
                        .windows(SENTINEL.len())
                        .any(|window| window == SENTINEL.as_bytes()),
                    "configured credential must not reach durable context state"
                );
            }
        }
    }

    #[tokio::test]
    async fn bound_turn_recovery_waits_for_an_exact_live_catalog_and_dispatches_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(
            workspace.join("AGENTS.md"),
            "Keep recovered provider requests byte identical.",
        )
        .expect("workspace instructions");
        let database = directory.path().join("bound-turn-recovery.sqlite3");
        let (session_id, attempt_id, expected_request) =
            seed_pending_bound_turn(database.clone(), &workspace).await;

        let (actor, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        let recovered_attempt = recovered.attempt(&attempt_id).expect("recovered attempt");
        assert_eq!(recovered_attempt.status(), EngineAttemptStatus::InFlight);
        assert!(recovered_attempt.is_provider_dispatch_ready());
        assert_eq!(recovered_attempt.turns_started(), 0);

        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        let (_ui, app) = bounded_ports(
            Arc::new(projection::session(&recovered)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            recovered,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace;
        coordinator
            .initialize_context_scope()
            .await
            .expect("initialize recovered context scope");

        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        coordinator
            .handle_catalog(
                0,
                None,
                Ok(ModelCatalog::new(
                    vec![fixture_model_descriptor()],
                    CatalogFreshness::Cached,
                )),
            )
            .await
            .expect("cached catalog");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        coordinator
            .handle_catalog(
                0,
                None,
                Ok(ModelCatalog::new(
                    vec![fixture_model_descriptor()],
                    CatalogFreshness::Stale,
                )),
            )
            .await
            .expect("stale catalog");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

        let mut incompatible = fixture_model_descriptor();
        incompatible.capabilities.tool_calling = CapabilitySupport::Unsupported;
        coordinator
            .handle_catalog(
                0,
                None,
                Ok(ModelCatalog::new(
                    vec![incompatible],
                    CatalogFreshness::Live,
                )),
            )
            .await
            .expect("incompatible live catalog fails closed");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        let still_pending = coordinator
            .session
            .attempt(&attempt_id)
            .expect("still-pending attempt");
        assert!(still_pending.is_provider_dispatch_ready());
        assert_eq!(still_pending.turns_started(), 0);

        coordinator
            .handle_catalog(
                0,
                None,
                Ok(ModelCatalog::new(
                    vec![fixture_model_descriptor()],
                    CatalogFreshness::Live,
                )),
            )
            .await
            .expect("matching live catalog");
        wait_for_provider_calls(&provider, 1).await;
        assert_eq!(
            provider.requests.lock().expect("request lock").as_slice(),
            std::slice::from_ref(&expected_request),
            "restart recovery must dispatch the byte-identical provider-neutral request"
        );
        let started = coordinator
            .session
            .attempt(&attempt_id)
            .expect("started recovered attempt");
        assert_eq!(started.turns_started(), 1);
        assert!(!started.is_provider_dispatch_ready());

        coordinator
            .handle_catalog(
                0,
                None,
                Ok(ModelCatalog::new(
                    vec![fixture_model_descriptor()],
                    CatalogFreshness::Live,
                )),
            )
            .await
            .expect("repeated live catalog");
        tokio::task::yield_now().await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        shutdown.cancel();
        drop(coordinator);
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn workspace_agents_persists_available_stale_and_absent_observations() {
        const BASELINE: &str = "Keep the last verified workspace instruction.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let agents_path = workspace.join("AGENTS.md");
        std::fs::write(&agents_path, BASELINE).expect("baseline agents source");
        let database = directory.path().join("agents-observations.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(RecordingProvider::default());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace.clone();
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;

        for (request_id, expected_completed) in [(2, 1_usize), (3, 2), (4, 3), (5, 4)] {
            if request_id == 3 {
                std::fs::remove_file(&agents_path).expect("remove agents file");
                std::fs::create_dir(&agents_path).expect("unavailable agents fixture");
            } else if request_id == 4 {
                std::fs::remove_dir(&agents_path).expect("missing agents fixture");
            } else if request_id == 5 {
                std::fs::create_dir(&agents_path).expect("second unavailable agents fixture");
            }
            let request_id = RequestId::new(request_id);
            ui.intents
                .send(UiIntent::SubmitPrompt {
                    request_id,
                    prompt: format!("observation {expected_completed}"),
                })
                .await
                .expect("submit prompt");
            expect_commit(&mut ui, request_id).await;
            wait_for_session(&mut ui.sessions, |projection| {
                projection
                    .transcript
                    .iter()
                    .filter(|item| {
                        matches!(
                            item,
                            TranscriptItem::Assistant {
                                status: autoharness_tui::AttemptStatus::Completed,
                                ..
                            }
                        )
                    })
                    .count()
                    == expected_completed
            })
            .await;
        }

        let events = handle
            .load_events(session_id.clone())
            .await
            .expect("events");
        let attempt_ids = events
            .iter()
            .filter_map(|event| match event.payload() {
                EventPayload::AttemptPrepared { attempt_id, .. } => Some(attempt_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(attempt_ids.len(), 4);
        let mut turns = Vec::new();
        for attempt_id in attempt_ids {
            turns.push(
                handle
                    .load_attempt_context_turn(attempt_id, 1)
                    .await
                    .expect("load turn")
                    .expect("turn"),
            );
        }
        let workspace_sources = turns
            .iter()
            .map(|turn| {
                assert_eq!(turn.sources().len(), 3);
                assert_eq!(
                    turn.sources()
                        .iter()
                        .filter(|source| {
                            source.visibility()
                                == autoharness_domain::ContextSourceVisibility::AuditOnly
                        })
                        .count(),
                    2
                );
                turn.sources()
                    .iter()
                    .find(|source| source.source_key().as_str() == "workspace:agents-md:v1")
                    .expect("workspace AGENTS source")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            workspace_sources[0].observation_state(),
            autoharness_domain::ContextObservationState::Available
        );
        assert_eq!(
            workspace_sources[1].observation_state(),
            autoharness_domain::ContextObservationState::RetainedStale
        );
        assert_eq!(
            workspace_sources[2].observation_state(),
            autoharness_domain::ContextObservationState::ObservedAbsent
        );
        assert_eq!(
            workspace_sources[3].observation_state(),
            autoharness_domain::ContextObservationState::RetainedStale
        );
        assert_eq!(
            workspace_sources[0].source_revision(),
            workspace_sources[1].source_revision()
        );
        assert_eq!(
            workspace_sources[0].source_revision(),
            workspace_sources[3].source_revision()
        );
        assert!(workspace_sources[2].source_revision().is_none());
        assert_eq!(turns[0].admissions().len(), 1);
        assert_eq!(turns[1].admissions().len(), 1);
        assert!(turns[2].admissions().is_empty());
        assert_eq!(turns[3].admissions().len(), 1);
        let first = handle
            .load_context_turn_content(turns[0].context_turn_id().clone())
            .await
            .expect("first source read")
            .expect("first source");
        let stale = handle
            .load_context_turn_content(turns[1].context_turn_id().clone())
            .await
            .expect("stale source read")
            .expect("stale source");
        let recovered_stale = handle
            .load_context_turn_content(turns[3].context_turn_id().clone())
            .await
            .expect("recovered stale source read")
            .expect("recovered stale source");
        assert_eq!(first, stale);
        assert_eq!(first, recovered_stale);
        assert!(first.as_str().contains(BASELINE));
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 4);
            assert_eq!(requests[0].context, requests[1].context);
            assert!(requests[2].context.is_none());
            assert_eq!(requests[0].context, requests[3].context);
        }

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn restarted_tool_continuation_reuses_frozen_agents_and_memory_baseline() {
        const BASELINE_AGENTS: &str = "Always preserve the first verified workspace baseline.";
        const CHANGED_AGENTS: &str = "This changed instruction belongs to a later epoch.";
        const BASELINE_MEMORY: &str = "Prefer frozen context across tool continuations.";
        const CHANGED_MEMORY: &str = "Prefer newly mutated context immediately.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("AGENTS.md"), BASELINE_AGENTS)
            .expect("baseline agents source");
        std::fs::write(workspace.join(".env"), "ordinary fixture").expect("tool read fixture");
        let database = directory.path().join("frozen-context.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let handle = actor.handle();
        let seeded = seed_workspace_memory(&handle, &workspace, BASELINE_MEMORY).await;
        let provider = Arc::new(ToolLoopProvider::reading());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace.clone();
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: BASELINE_MEMORY.to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        std::fs::write(workspace.join("AGENTS.md"), CHANGED_AGENTS).expect("changed agents source");
        let changed_revision = user_memory_draft(
            MemoryRevisionNumber::new(2).expect("revision number"),
            None,
            CHANGED_MEMORY.to_owned(),
            ConfidenceBasisPoints::new(10_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
        )
        .expect("changed memory draft");
        handle
            .execute_memory_command(ids::memory_command(
                seeded.memory_id.clone(),
                Some(MemorySequence::new(seeded.last_sequence).expect("memory sequence")),
                MemoryCommandPayload::ReviseMemory {
                    revision: changed_revision,
                    supersedes_revision_id: seeded.revision_id.clone(),
                },
            ))
            .await
            .expect("mutate memory between turns");

        shutdown.cancel();
        task.await
            .expect("first coordinator join")
            .expect("first coordinator shutdown");
        drop(handle);
        actor.shutdown().await.expect("first actor shutdown");
        drop(ui);

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(
            recovered.attempts().last().expect("attempt").status(),
            EngineAttemptStatus::AwaitingTools
        );
        let reopened_handle = reopened.handle();
        let (mut resumed_ui, resumed_app) = bounded_ports(
            Arc::new(projection::session(&recovered)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let resumed_shutdown = CancellationToken::new();
        let mut resumed = Coordinator::with_provider_factory(
            session_id.clone(),
            recovered,
            reopened_handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            resumed_app,
            resumed_shutdown.clone(),
        );
        resumed.workspace = workspace.clone();
        let resumed_task = tokio::spawn(resumed.run());
        wait_for_catalog(&mut resumed_ui).await;
        let recovered_permission = wait_for_session(&mut resumed_ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        assert_eq!(
            recovered_permission.permission_requests[0].tool_call_id,
            permission.tool_call_id
        );
        let deny_request = RequestId::new(3);
        resumed_ui
            .intents
            .send(UiIntent::AnswerPermission {
                request_id: deny_request,
                tool_call_id: permission.tool_call_id,
                allow: false,
            })
            .await
            .expect("deny permission");
        expect_commit(&mut resumed_ui, deny_request).await;
        wait_for_session(&mut resumed_ui.sessions, |projection| {
            projection.permission_requests.is_empty()
                && projection.transcript.iter().any(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Completed,
                            text,
                            ..
                        } if text == "tool complete"
                    )
                })
        })
        .await;

        let attempts = reopened_handle
            .load_events(session_id.clone())
            .await
            .expect("events");
        let attempt_id = attempts
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::AttemptPrepared { attempt_id, .. } => Some(attempt_id.clone()),
                _ => None,
            })
            .expect("attempt ID");
        let first_turn = reopened_handle
            .load_attempt_context_turn(attempt_id.clone(), 1)
            .await
            .expect("load first turn")
            .expect("first turn");
        let second_turn = reopened_handle
            .load_attempt_context_turn(attempt_id, 2)
            .await
            .expect("load second turn")
            .expect("second turn");
        let first_prelude = reopened_handle
            .load_context_turn_content(first_turn.context_turn_id().clone())
            .await
            .expect("first prelude read")
            .expect("first prelude");
        let second_prelude = reopened_handle
            .load_context_turn_content(second_turn.context_turn_id().clone())
            .await
            .expect("second prelude read")
            .expect("second prelude");
        assert_eq!(first_prelude, second_prelude);
        assert!(first_prelude.as_str().contains(BASELINE_AGENTS));
        assert!(first_prelude.as_str().contains(BASELINE_MEMORY));
        assert!(!first_prelude.as_str().contains(CHANGED_AGENTS));
        assert!(!first_prelude.as_str().contains(CHANGED_MEMORY));
        assert_eq!(first_turn.epoch_id(), second_turn.epoch_id());
        assert_eq!(
            first_turn.memory_generation(),
            second_turn.memory_generation()
        );
        let prelude_sources = |turn: &autoharness_domain::ContextTurnManifest| {
            turn.sources()
                .iter()
                .filter(|source| {
                    source.visibility()
                        == autoharness_domain::ContextSourceVisibility::PreludeEligible
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(prelude_sources(&first_turn), prelude_sources(&second_turn));
        let audit_sources = |turn: &autoharness_domain::ContextTurnManifest| {
            turn.sources()
                .iter()
                .filter(|source| {
                    source.visibility() == autoharness_domain::ContextSourceVisibility::AuditOnly
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let first_audit = audit_sources(&first_turn);
        let second_audit = audit_sources(&second_turn);
        assert_eq!(first_audit.len(), 2);
        assert_eq!(
            first_audit
                .iter()
                .map(|source| source.source_key())
                .collect::<Vec<_>>(),
            second_audit
                .iter()
                .map(|source| source.source_key())
                .collect::<Vec<_>>()
        );
        for (first, second) in first_audit.iter().zip(second_audit) {
            assert_ne!(first.source_revision(), second.source_revision());
        }
        assert_eq!(first_turn.rendered_hash(), second_turn.rendered_hash());
        assert_ne!(first_turn.request_hash(), second_turn.request_hash());
        assert_eq!(
            first_turn.admissions().len(),
            second_turn.admissions().len()
        );
        for (first, second) in first_turn.admissions().iter().zip(second_turn.admissions()) {
            assert_ne!(first.admission_id(), second.admission_id());
            assert_eq!(first.source_key(), second.source_key());
            assert_eq!(first.source_revision(), second.source_revision());
            assert_eq!(first.memory_revision_id(), second.memory_revision_id());
            assert_eq!(first.rendered_hash(), second.rendered_hash());
            assert_eq!(first.token_count(), second.token_count());
        }
        assert!(
            first_turn
                .admissions()
                .iter()
                .any(|admission| { admission.memory_revision_id() == Some(&seeded.revision_id) })
        );
        assert!(
            reopened_handle
                .memory_generation()
                .await
                .expect("current generation")
                > first_turn.memory_generation()
        );
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].context, requests[1].context);
        }

        resumed_shutdown.cancel();
        resumed_task
            .await
            .expect("resumed coordinator join")
            .expect("resumed coordinator shutdown");
        drop(reopened_handle);
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn erased_frozen_memory_blocks_tool_continuation_before_dispatch() {
        const BASELINE_MEMORY: &str = "Keep this exact memory for the tool-loop epoch.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join(".env"), "ordinary fixture").expect("tool read fixture");
        let database = directory.path().join("erased-frozen-context.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let seeded = seed_workspace_memory(&handle, &workspace, BASELINE_MEMORY).await;
        let provider = Arc::new(ToolLoopProvider::reading());
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace;
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: BASELINE_MEMORY.to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        handle
            .execute_memory_command(ids::memory_command(
                seeded.memory_id,
                Some(MemorySequence::new(seeded.last_sequence).expect("memory sequence")),
                MemoryCommandPayload::DeleteMemory {
                    revision_id: seeded.revision_id,
                },
            ))
            .await
            .expect("erase admitted memory");

        let deny_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: deny_request,
                tool_call_id: permission.tool_call_id,
                allow: false,
            })
            .await
            .expect("deny permission");
        expect_commit(&mut ui, deny_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.is_empty()
                && projection.transcript.iter().any(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Failed(UiFailure {
                                code,
                                ..
                            }),
                            ..
                        } if code == "context_not_committed"
                    )
                })
        })
        .await;
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "erased frozen bytes must block the second provider dispatch"
        );

        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        let attempt_id = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::AttemptPrepared { attempt_id, .. } => Some(attempt_id.clone()),
                _ => None,
            })
            .expect("attempt ID");
        assert!(
            handle
                .load_attempt_context_turn(attempt_id.clone(), 1)
                .await
                .expect("first turn read")
                .is_some()
        );
        assert!(
            handle
                .load_attempt_context_turn(attempt_id, 2)
                .await
                .expect("second turn read")
                .is_none()
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn model_proposals_remain_review_only_and_approval_creates_user_revision() {
        const FIRST_PROPOSAL: &str = "The project uses rustfmt before review.";
        const SECOND_PROPOSAL: &str = "The team prefers compact status updates.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("model-proposals.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::Tools(vec![
                scripted_tool_call(
                    "proposal-call-1",
                    "memory_propose",
                    serde_json::json!({
                        "content": FIRST_PROPOSAL,
                        "kind": "fact",
                        "scope": "session",
                        "sensitivity": "internal"
                    }),
                ),
                scripted_tool_call(
                    "proposal-call-2",
                    "memory_propose",
                    serde_json::json!({
                        "content": SECOND_PROPOSAL,
                        "kind": "preference",
                        "scope": "workspace",
                        "sensitivity": "public"
                    }),
                ),
            ]),
            ProposalProviderTurn::Complete("proposal tools settled"),
            ProposalProviderTurn::Complete("second request settled"),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(directory.path()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Propose useful review-only memory.".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Completed,
                        text,
                        ..
                    } if text == "proposal tools settled"
                )
            })
        })
        .await;

        let events = handle
            .load_events(session_id.clone())
            .await
            .expect("session events");
        let input_id = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::InputAdmitted { input_id, .. } => Some(input_id.clone()),
                _ => None,
            })
            .expect("input ID");
        let proposal_calls = events
            .iter()
            .filter_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.capability.kind
                        == autoharness_domain::CapabilityKind::MemoryProposal =>
                {
                    Some(call.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(proposal_calls.len(), 2);
        for (index, (call, expected_content)) in proposal_calls
            .iter()
            .zip([FIRST_PROPOSAL, SECOND_PROPOSAL])
            .enumerate()
        {
            let memory_id = ids::memory_proposal_memory_id(&call.tool_call_id);
            let revisions = handle
                .load_memory_revisions(memory_id.clone())
                .await
                .expect("proposal revisions");
            assert_eq!(revisions.len(), 1);
            let revision = &revisions[0];
            assert_eq!(revision.status(), MemoryRevisionStatus::Proposed);
            assert_eq!(revision.origin(), MemoryOrigin::ModelProposal);
            assert_eq!(revision.trust_class(), TrustClass::UntrustedProposal);
            assert_eq!(
                handle
                    .load_memory_content(revision.revision_id().clone())
                    .await
                    .expect("proposal content")
                    .expect("retained proposal")
                    .as_str(),
                expected_content
            );
            assert!(matches!(
                revision.evidence(),
                [evidence]
                    if matches!(
                        evidence.source(),
                        MemoryEvidenceSource::UserInput {
                            session_id: evidence_session,
                            input_id: evidence_input,
                        } if evidence_session == &session_id && evidence_input == &input_id
                    )
            ));
            let operations = handle
                .load_memory_operations(memory_id, 0, 16)
                .await
                .expect("proposal operations");
            assert_eq!(operations.len(), 2, "proposal {index} must not activate");
            assert!(operations.iter().all(|operation| !matches!(
                operation.payload(),
                MemoryOperationPayload::RevisionActivated { .. }
            )));
        }
        assert_eq!(
            handle
                .memory_generation()
                .await
                .expect("eligibility generation")
                .get(),
            0
        );
        let completed_outputs = events
            .iter()
            .filter_map(|event| match event.payload() {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    output,
                } if proposal_calls
                    .iter()
                    .any(|call| &call.tool_call_id == tool_call_id) =>
                {
                    Some(output)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(completed_outputs.len(), 2);
        assert!(completed_outputs.iter().all(|output| {
            output.content().is_empty() && output.original_bytes() == 0 && !output.truncated()
        }));
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 2);
            let results = requests[1]
                .messages
                .iter()
                .filter_map(|message| match message {
                    ChatMessage::ToolResult { content, .. } => Some(content.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                results,
                vec![
                    "Memory proposal recorded for review",
                    "Memory proposal recorded for review"
                ]
            );
            assert!(results.iter().all(
                |result| !result.contains(FIRST_PROPOSAL) && !result.contains(SECOND_PROPOSAL)
            ));
        }

        let second_submit = RequestId::new(3);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: second_submit,
                prompt: FIRST_PROPOSAL.to_owned(),
            })
            .await
            .expect("second prompt");
        expect_commit(&mut ui, second_submit).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection
                .transcript
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Completed,
                            ..
                        }
                    )
                })
                .count()
                == 2
        })
        .await;
        let events = handle
            .load_events(session_id.clone())
            .await
            .expect("updated events");
        let attempt_ids = events
            .iter()
            .filter_map(|event| match event.payload() {
                EventPayload::AttemptPrepared { attempt_id, .. } => Some(attempt_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let second_turn = handle
            .load_attempt_context_turn(attempt_ids[1].clone(), 1)
            .await
            .expect("second attempt context")
            .expect("second attempt turn");
        assert!(
            second_turn
                .admissions()
                .iter()
                .all(|admission| admission.memory_revision_id().is_none())
        );

        let target = &proposal_calls[0];
        let target_memory_id = ids::memory_proposal_memory_id(&target.tool_call_id);
        let target_revision_id = ids::memory_proposal_revision_id(&target.tool_call_id);
        let approve_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::ApproveMemoryProposal {
                request_id: approve_request,
                memory_id: target_memory_id.as_str().to_owned(),
                expected_last_sequence: 2,
                proposal_revision_id: target_revision_id.as_str().to_owned(),
            })
            .await
            .expect("approve proposal");
        expect_commit(&mut ui, approve_request).await;
        let approved = handle
            .load_memory_revisions(target_memory_id)
            .await
            .expect("approved revisions");
        assert_eq!(approved.len(), 2);
        assert_eq!(approved[0].revision_id(), &target_revision_id);
        assert_eq!(approved[0].status(), MemoryRevisionStatus::Superseded);
        assert_ne!(approved[1].revision_id(), &target_revision_id);
        assert_eq!(approved[1].status(), MemoryRevisionStatus::Active);
        assert_eq!(approved[1].origin(), MemoryOrigin::ExplicitUser);
        assert_eq!(approved[1].trust_class(), TrustClass::UserApproved);

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn unverified_or_secret_model_proposals_fail_without_memory_writes() {
        const SENTINEL: &str = "test-api-secret";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("evidence.txt"), "not completed yet")
            .expect("evidence fixture");
        let database = directory.path().join("invalid-proposals.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::Tools(vec![
                scripted_tool_call(
                    "incomplete-read",
                    "fs_read",
                    serde_json::json!({"path":"evidence.txt"}),
                ),
                scripted_tool_call(
                    "proposal-incomplete",
                    "memory_propose",
                    serde_json::json!({
                        "content":"This proposal cites an unfinished call.",
                        "kind":"fact",
                        "scope":"session",
                        "sensitivity":"internal",
                        "source_provider_call_id":"incomplete-read"
                    }),
                ),
                scripted_tool_call(
                    "proposal-forged",
                    "memory_propose",
                    serde_json::json!({
                        "content":"This proposal cites a nonexistent call.",
                        "kind":"fact",
                        "scope":"session",
                        "sensitivity":"internal",
                        "source_provider_call_id":"forged-provider-call"
                    }),
                ),
                scripted_tool_call(
                    "proposal-secret",
                    "memory_propose",
                    serde_json::json!({
                        "content":format!("Never retain {SENTINEL}"),
                        "kind":"fact",
                        "scope":"session",
                        "sensitivity":"internal"
                    }),
                ),
            ]),
            ProposalProviderTurn::Complete("invalid proposals settled"),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace;
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Try proposals with invalid evidence.".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure { code, .. }),
                        text,
                        ..
                    } if text.is_empty() && code == "credential_in_provider_data"
                )
            })
        })
        .await;

        let events = handle
            .load_events(session_id)
            .await
            .expect("session events");
        let proposal_calls = events
            .iter()
            .filter_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.capability.kind
                        == autoharness_domain::CapabilityKind::MemoryProposal =>
                {
                    Some(call)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(proposal_calls.len(), 2);
        for call in proposal_calls {
            let memory_id = ids::memory_proposal_memory_id(&call.tool_call_id);
            assert!(
                handle
                    .load_memory_operations(memory_id.clone(), 0, 16)
                    .await
                    .expect("memory operations")
                    .is_empty()
            );
            assert!(
                handle
                    .load_memory_revisions(memory_id)
                    .await
                    .expect("memory revisions")
                    .is_empty()
            );
        }
        assert!(!events.iter().any(|event| matches!(
            event.payload(),
            EventPayload::ToolCallProposed { call, .. }
                if call.provider_call_id.as_str() == "proposal-secret"
        )));
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize durable events")
                .contains(SENTINEL)
        );
        assert_eq!(
            handle
                .memory_mutation_generation()
                .await
                .expect("mutation generation")
                .get(),
            0
        );
        {
            let requests = provider.requests.lock().expect("request lock");
            assert_eq!(requests.len(), 1);
            assert!(
                !serde_json::to_string(&*requests)
                    .expect("serialize provider requests")
                    .contains(SENTINEL)
            );
        }

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn inline_tool_provenance_and_exact_proposal_retry_are_idempotent() {
        const EVIDENCE: &str = "verified inline observation";
        const PROPOSAL: &str = "The verified inline observation is available.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::write(workspace.join("inline.txt"), EVIDENCE).expect("inline evidence fixture");
        let database = directory.path().join("inline-proposal.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let handle = actor.handle();
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "inline-read",
                "fs_read",
                serde_json::json!({"path":"inline.txt"}),
            )]),
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "inline-proposal",
                "memory_propose",
                serde_json::json!({
                    "content":PROPOSAL,
                    "kind":"fact",
                    "scope":"session",
                    "sensitivity":"internal",
                    "source_provider_call_id":"inline-read"
                }),
            )]),
            ProposalProviderTurn::Complete("inline proposal settled"),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace.clone();
        coordinator.artifact_root = Some(workspace.join("artifacts"));
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Read and ground an inline proposal.".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let allow_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: allow_request,
                tool_call_id: awaiting_permission.permission_requests[0]
                    .tool_call_id
                    .clone(),
                allow: true,
            })
            .await
            .expect("allow inline read");
        expect_commit(&mut ui, allow_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Completed,
                        text,
                        ..
                    } if text == "inline proposal settled"
                )
            })
        })
        .await;

        let events = handle
            .load_events(session_id.clone())
            .await
            .expect("session events");
        let read_call = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.provider_call_id.as_str() == "inline-read" =>
                {
                    Some(call.clone())
                }
                _ => None,
            })
            .expect("inline read call");
        let proposal_call = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.provider_call_id.as_str() == "inline-proposal" =>
                {
                    Some(call.clone())
                }
                _ => None,
            })
            .expect("inline proposal call");
        let read_output = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    output,
                } if tool_call_id == &read_call.tool_call_id => Some(output),
                _ => None,
            })
            .expect("inline read output");
        assert!(!read_output.truncated());
        assert!(read_output.artifact().is_none());
        assert_eq!(read_output.content(), EVIDENCE);
        let memory_id = ids::memory_proposal_memory_id(&proposal_call.tool_call_id);
        let revisions = handle
            .load_memory_revisions(memory_id.clone())
            .await
            .expect("inline proposal revisions");
        assert!(matches!(
            revisions.as_slice(),
            [revision]
                if revision.status() == MemoryRevisionStatus::Proposed
                    && revision.origin() == MemoryOrigin::VerifiedTool
                    && revision.trust_class() == TrustClass::VerifiedObservation
                    && matches!(
                        revision.evidence(),
                        [evidence]
                            if matches!(
                                evidence.source(),
                                MemoryEvidenceSource::ToolObservation {
                                    session_id: evidence_session,
                                    tool_call_id,
                                    output_hash,
                                } if evidence_session == &session_id
                                    && tool_call_id == &read_call.tool_call_id
                                    && output_hash == &raw_sha256(EVIDENCE.as_bytes())
                                        .expect("inline evidence hash")
                            )
                    )
        ));
        let proposal_output = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    output,
                } if tool_call_id == &proposal_call.tool_call_id => Some(output),
                _ => None,
            })
            .expect("proposal output");
        assert_eq!(proposal_output.content(), "");
        assert_eq!(proposal_output.original_bytes(), 0);
        {
            let requests = provider.requests.lock().expect("request lock");
            assert!(requests[2].messages.iter().any(|message| {
                matches!(
                    message,
                    ChatMessage::ToolResult {
                        provider_call_id,
                        content,
                        ..
                    } if provider_call_id.as_str() == "inline-proposal"
                        && content.as_str() == "Memory proposal recorded for review"
                )
            }));
        }

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");

        let (actor, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        let handle = actor.handle();
        let proposal_projection = recovered
            .tool_call(&proposal_call.tool_call_id)
            .expect("recovered proposal call")
            .clone();
        let replanned = replan(proposal_projection.call().clone()).expect("replanned proposal");
        let proposal = replanned
            .memory_proposal()
            .expect("memory proposal")
            .clone();
        let attempt_id = proposal_projection.attempt_id().clone();
        let turn = handle
            .load_attempt_context_turn(attempt_id, 1)
            .await
            .expect("load context turn")
            .expect("context turn");
        let (_unused_ui, retry_ports) = bounded_ports(
            Arc::new(projection::session(&recovered)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let mut retry = Coordinator::with_provider_factory(
            session_id.clone(),
            recovered,
            handle.clone(),
            ProviderComposition {
                initial: None,
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            retry_ports,
            CancellationToken::new(),
        );
        retry.context_scope = Some(ContextScope::local(
            turn.eligibility().workspace_id().clone(),
        ));
        retry.artifact_root = Some(workspace.join("artifacts"));
        retry
            .persist_memory_proposal(&proposal_projection, &proposal)
            .await
            .expect("exact retry reconciles as committed");
        assert_eq!(
            handle
                .load_memory_operations(memory_id, 0, 16)
                .await
                .expect("idempotent proposal operations")
                .len(),
            2,
            "exact command retry must not append a second mutation"
        );
        drop(retry);
        drop(handle);
        actor.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn completed_artifact_tool_provenance_is_verified_before_proposal_write() {
        const PROPOSAL: &str = "The verified artifact contains the expected repeated bytes.";

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let evidence_bytes = vec![b'x'; 70_000];
        std::fs::write(workspace.join("evidence.bin"), &evidence_bytes)
            .expect("artifact evidence fixture");
        let database = directory.path().join("verified-proposal.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let handle = actor.handle();
        let expected_workspace = handle
            .resolve_workspace_id(
                workspace_locator_digest(&workspace).expect("workspace locator digest"),
            )
            .await
            .expect("workspace binding");
        let provider = Arc::new(ProposalProvider::new(vec![
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "verified-read",
                "fs_read",
                serde_json::json!({"path":"evidence.bin"}),
            )]),
            ProposalProviderTurn::Tools(vec![scripted_tool_call(
                "verified-proposal",
                "memory_propose",
                serde_json::json!({
                    "content":PROPOSAL,
                    "kind":"fact",
                    "scope":"workspace",
                    "sensitivity":"internal",
                    "source_provider_call_id":"verified-read"
                }),
            )]),
            ProposalProviderTurn::Complete("verified proposal settled"),
        ]));
        let (mut ui, app) = bounded_ports(
            Arc::new(projection::session(&session)),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let mut coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            handle.clone(),
            ProviderComposition {
                initial: Some(provider),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        coordinator.workspace = workspace.clone();
        coordinator.artifact_root = Some(workspace.join("artifacts"));
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "Read and ground a review-only proposal.".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        let allow_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: allow_request,
                tool_call_id: permission.tool_call_id,
                allow: true,
            })
            .await
            .expect("allow evidence read");
        expect_commit(&mut ui, allow_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Completed,
                        text,
                        ..
                    } if text == "verified proposal settled"
                )
            })
        })
        .await;

        let events = handle
            .load_events(session_id.clone())
            .await
            .expect("session events");
        let read_call = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.provider_call_id.as_str() == "verified-read" =>
                {
                    Some(call.clone())
                }
                _ => None,
            })
            .expect("read call");
        let proposal_call = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallProposed { call, .. }
                    if call.provider_call_id.as_str() == "verified-proposal" =>
                {
                    Some(call.clone())
                }
                _ => None,
            })
            .expect("proposal call");
        let read_output = events
            .iter()
            .find_map(|event| match event.payload() {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    output,
                } if tool_call_id == &read_call.tool_call_id => Some(output),
                _ => None,
            })
            .expect("read output");
        assert!(read_output.truncated());
        assert!(read_output.artifact().is_some());

        let memory_id = ids::memory_proposal_memory_id(&proposal_call.tool_call_id);
        let revisions = handle
            .load_memory_revisions(memory_id.clone())
            .await
            .expect("verified proposal revisions");
        assert_eq!(revisions.len(), 1);
        let revision = &revisions[0];
        assert_eq!(revision.status(), MemoryRevisionStatus::Proposed);
        assert_eq!(revision.origin(), MemoryOrigin::VerifiedTool);
        assert_eq!(revision.trust_class(), TrustClass::VerifiedObservation);
        assert!(matches!(
            revision.evidence(),
            [evidence]
                if matches!(
                    evidence.source(),
                    MemoryEvidenceSource::ToolObservation {
                        session_id: evidence_session,
                        tool_call_id,
                        output_hash,
                    } if evidence_session == &session_id
                        && tool_call_id == &read_call.tool_call_id
                        && output_hash == &raw_sha256(&evidence_bytes).expect("evidence hash")
                )
        ));
        let operations = handle
            .load_memory_operations(memory_id, 0, 16)
            .await
            .expect("verified proposal operations");
        assert!(matches!(
            operations[0].payload(),
            MemoryOperationPayload::MemoryCreated {
                scope: DomainMemoryScope::Workspace(workspace_id),
                ..
            } if workspace_id == &expected_workspace
        ));
        assert!(operations.iter().all(|operation| !matches!(
            operation.payload(),
            MemoryOperationPayload::RevisionActivated { .. }
        )));

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        drop(handle);
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn composed_tool_permission_execution_continuation_and_restart_are_durable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("tool-composition.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(ToolLoopProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let factory: ProviderFactory = Arc::new(|_| {
            Err(ProviderError::new(
                ProviderErrorKind::MissingCredential,
                RetryAdvice::Never,
            ))
        });
        let coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            actor.handle(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory,
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;

        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "write the requested result".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        assert_eq!(permission.tool_name, "fs_write");
        assert_eq!(permission.capability, "filesystem write");
        assert_eq!(permission.resource, "workspace:result.txt");
        assert!(
            permission
                .details
                .iter()
                .any(|detail| { detail.label == "Path" && detail.value == "result.txt" })
        );
        assert!(
            permission
                .details
                .iter()
                .any(|detail| { detail.label == "Content bytes" && detail.value == "15" })
        );
        assert!(
            !workspace.join("result.txt").exists(),
            "no side effect may happen before the exact human answer is durable"
        );

        let permission_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: permission_request,
                tool_call_id: permission.tool_call_id,
                allow: true,
            })
            .await
            .expect("permission answer");
        expect_commit(&mut ui, permission_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    } if text == "tool complete"
                )
            })
        })
        .await;
        assert!(completed.permission_requests.is_empty());
        assert!(completed.transcript.iter().any(|item| {
            matches!(
                item,
                TranscriptItem::Tool(row)
                    if row.tool_name == "fs_write"
                        && row.status == "completed"
                        && row.resource == "workspace:result.txt"
            )
        }));
        assert_eq!(
            std::fs::read_to_string(workspace.join("result.txt")).expect("tool output file"),
            "written by tool"
        );

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(projection::session(&recovered), *completed);
        assert_eq!(recovered.tool_calls().len(), 1);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Completed
        );
        reopened.shutdown().await.expect("reopened actor shutdown");

        let requests = provider.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert!(requests[0].messages.iter().all(|message| {
            !matches!(
                message,
                ChatMessage::ToolCall(_) | ChatMessage::ToolResult { .. }
            )
        }));
        assert!(requests[1].messages.iter().any(|message| {
            matches!(
                message,
                ChatMessage::ToolCall(call)
                    if call.provider_call_id.as_str() == "call-write-1"
            )
        }));
        assert!(requests[1].messages.iter().any(|message| {
            matches!(
                message,
                ChatMessage::ToolResult {
                    provider_call_id,
                    tool_name,
                    content,
                } if provider_call_id.as_str() == "call-write-1"
                    && tool_name.as_str() == "fs_write"
                    && content.as_str() == "wrote 15 bytes"
            )
        }));
    }

    #[tokio::test]
    async fn workspace_secret_read_requires_a_human_answer_before_persistence_or_replay() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let sentinel = "workspace-secret-sentinel";
        std::fs::write(workspace.join(".env"), sentinel).expect("secret fixture");
        let database = directory.path().join("read-permission.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(ToolLoopProvider::reading());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id,
            session,
            actor.handle(),
            ProviderComposition {
                initial: Some(provider.clone()),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            test_tool_runtime_at(&workspace),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "read configuration".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        assert_eq!(permission.tool_name, "fs_read");
        assert!(
            provider.requests.lock().expect("request lock").len() == 1,
            "the provider must not receive a tool result before the human answer"
        );

        let deny_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: deny_request,
                tool_call_id: permission.tool_call_id,
                allow: false,
            })
            .await
            .expect("deny permission");
        expect_commit(&mut ui, deny_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.is_empty()
                && projection.transcript.iter().any(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Completed,
                            ..
                        }
                    )
                })
        })
        .await;

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
        for entry in std::fs::read_dir(directory.path()).expect("state directory") {
            let entry = entry.expect("state entry");
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("read-permission")
            {
                let bytes = std::fs::read(entry.path()).expect("durable state file");
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes()),
                    "workspace secret must not reach durable state"
                );
            }
        }
        let requests = provider.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        let serialized = serde_json::to_vec(&*requests).expect("provider requests");
        assert!(
            !serialized
                .windows(sentinel.len())
                .any(|window| window == sentinel.as_bytes()),
            "workspace secret must not reach provider-visible history"
        );
    }

    #[tokio::test]
    async fn provider_failure_settles_pending_tools_before_the_parent_attempt() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let database = directory.path().join("tool-error.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id.clone(),
            session,
            actor.handle(),
            Some(Arc::new(ToolThenErrorProvider)),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "tool then error".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let failed = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.is_empty()
                && projection.transcript.iter().any(|item| {
                    matches!(
                        item,
                        TranscriptItem::Assistant {
                            status: autoharness_tui::AttemptStatus::Failed(_),
                            ..
                        }
                    )
                })
        })
        .await;
        assert!(failed.permission_requests.is_empty());
        assert!(!workspace.join("result.txt").exists());

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(recovered.tool_calls().len(), 1);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Cancelled
        );
        assert_eq!(
            recovered.attempts().last().expect("attempt").status(),
            EngineAttemptStatus::Failed
        );
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn post_commit_tool_error_is_durably_unknown_and_not_failed_or_cancelled() {
        use autoharness_tool::{
            FileArtifactStore, LocalHttp, LocalProcess, PermissionPolicy, ToolRuntime,
        };

        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace directory");
        let artifacts = workspace.join("artifacts");
        let runtime = Arc::new(
            ToolRuntime::new(
                Arc::new(CommitThenCancelledFilesystem {
                    root: workspace.clone(),
                }),
                Arc::new(LocalProcess::new(&workspace, 1024 * 1024).expect("process")),
                Arc::new(LocalHttp::new(1024 * 1024).expect("HTTP")),
                Arc::new(FileArtifactStore::new(artifacts).expect("artifacts")),
                PermissionPolicy::local_default(),
                2,
                Duration::from_secs(10),
                64 * 1024,
            )
            .expect("runtime"),
        );
        let database = directory.path().join("ambiguous-effect.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(ToolLoopProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id.clone(),
            session,
            actor.handle(),
            ProviderComposition {
                initial: Some(provider),
                factory: Arc::new(|_| {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                }),
            },
            runtime,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "write with ambiguous settlement".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        let allow_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: allow_request,
                tool_call_id: permission.tool_call_id,
                allow: true,
            })
            .await
            .expect("allow permission");
        expect_commit(&mut ui, allow_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    }
                )
            })
        })
        .await;
        assert_eq!(
            std::fs::read_to_string(workspace.join("result.txt")).expect("committed effect"),
            "written by tool"
        );

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(
            recovered.tool_calls()[0].status(),
            autoharness_engine::ToolCallStatus::Unknown
        );
        reopened.shutdown().await.expect("reopened actor shutdown");
    }

    #[tokio::test]
    async fn redacted_first_prompt_titles_the_session_and_replays_after_retry() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("composition.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(FakeProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id.clone(),
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select intent");
        expect_commit(&mut ui, select_request).await;

        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "exact test-api-secret user prompt".to_owned(),
            })
            .await
            .expect("submit intent");
        expect_commit(&mut ui, submit_request).await;
        let titled_sessions = wait_for_session_list(&mut ui.session_lists, |list| {
            list.sessions.iter().any(|entry| {
                entry.session_id == session_id.as_str()
                    && entry.title == "exact [REDACTED] user prompt"
            })
        })
        .await;
        assert!(titled_sessions.iter().any(|entry| {
            entry.session_id == session_id.as_str() && entry.title == "exact [REDACTED] user prompt"
        }));

        let streaming = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Streaming,
                        ..
                    } if text == "partial"
                )
            })
        })
        .await;
        let first_attempt = streaming
            .transcript
            .iter()
            .find_map(|item| match item {
                TranscriptItem::Assistant { attempt_id, .. } => Some(attempt_id.clone()),
                TranscriptItem::Tool(_) | TranscriptItem::User { .. } => None,
            })
            .expect("streaming attempt");

        let cancel_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::CancelAttempt {
                request_id: cancel_request,
                attempt_id: first_attempt.clone(),
            })
            .await
            .expect("cancel intent");
        expect_commit(&mut ui, cancel_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        attempt_id,
                        status: autoharness_tui::AttemptStatus::Cancelled,
                        ..
                    } if attempt_id == &first_attempt
                )
            })
        })
        .await;

        let retry_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::RetryAttempt {
                request_id: retry_request,
                attempt_id: first_attempt,
            })
            .await
            .expect("retry intent");
        expect_commit(&mut ui, retry_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        retry_of: Some(_),
                        ..
                    } if text == "recovered"
                )
            })
        })
        .await;
        assert_eq!(completed.selected_model.as_ref(), Some(&fixture_model()));

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(
            recovered
                .title()
                .map(autoharness_domain::SessionTitle::as_str),
            Some("exact [REDACTED] user prompt")
        );
        assert_eq!(projection::session(&recovered), *completed);
        let recovered_usage = recovered
            .attempts()
            .last()
            .and_then(|attempt| attempt.usage())
            .expect("recovered usage");
        assert_eq!(recovered_usage.cached_input_tokens(), Some(1));
        assert_eq!(recovered_usage.reasoning_tokens(), Some(1));
        assert_eq!(recovered_usage.tool_tokens(), Some(0));
        reopened.shutdown().await.expect("reopened actor shutdown");

        let requests = provider.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].messages.len(), 1);
        assert_eq!(
            requests[1].messages[0]
                .content()
                .expect("text message")
                .as_str(),
            "exact [REDACTED] user prompt"
        );
        drop(requests);
        for entry in std::fs::read_dir(directory.path()).expect("read data directory") {
            let path = entry.expect("data file entry").path();
            if path.is_file() {
                let bytes = std::fs::read(path).expect("read data file");
                assert!(
                    !bytes
                        .windows("test-api-secret".len())
                        .any(|window| window == b"test-api-secret"),
                    "provider credential sentinel must not reach durable files"
                );
            }
        }
    }

    #[tokio::test]
    async fn unsolicited_provider_cancellation_is_a_retryable_failure() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("unsolicited-cancellation.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider = Arc::new(UnsolicitedCancellationProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id,
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select intent");
        expect_commit(&mut ui, select_request).await;

        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "retry provider cancellation".to_owned(),
            })
            .await
            .expect("submit intent");
        expect_commit(&mut ui, submit_request).await;
        let failed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure {
                            class: ErrorClass::Cancelled,
                            retry: RetryPolicy::Now,
                            ..
                        }),
                        ..
                    }
                )
            })
        })
        .await;
        let failed_attempt = failed
            .transcript
            .iter()
            .find_map(|item| match item {
                TranscriptItem::Assistant { attempt_id, .. } => Some(attempt_id.clone()),
                TranscriptItem::Tool(_) | TranscriptItem::User { .. } => None,
            })
            .expect("failed attempt");

        let retry_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::RetryAttempt {
                request_id: retry_request,
                attempt_id: failed_attempt,
            })
            .await
            .expect("retry intent");
        expect_commit(&mut ui, retry_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        retry_of: Some(_),
                        ..
                    } if text == "recovered"
                )
            })
        })
        .await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    async fn spawn_router_fixture() -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind router fixture");
        let address = listener.local_addr().expect("fixture address");
        let base_url = format!("http://{address}/");
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (content_type, body) in [
                (
                    "application/json",
                    r#"{"data":[{"id":"router-model","name":"Router model","capabilities":{"chat":true,"streaming":true}}],"has_more":false}"#,
                ),
                (
                    "text/event-stream",
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"router response\"},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                ),
            ] {
                let (mut socket, _) = listener.accept().await.expect("fixture request");
                requests.push(read_router_request(&mut socket).await);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("fixture response");
            }
            requests
        });
        (base_url, task)
    }

    async fn read_router_request(socket: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let header_end = loop {
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("fixture read");
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
        };
        let (request_line, content_length) = {
            let headers = std::str::from_utf8(&bytes[..header_end]).expect("request headers");
            let request_line = headers
                .lines()
                .next()
                .expect("request line")
                .trim_end_matches(" HTTP/1.1")
                .to_owned();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .or_else(|| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            (request_line, content_length)
        };
        while bytes.len() < header_end.saturating_add(content_length) {
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("fixture body");
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
        }
        request_line
    }
}
