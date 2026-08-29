use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Arc;

use autoharness_domain::{
    AttemptFailure, AttemptId, ClassifiedError, CommandPayload, ConfidenceBasisPoints,
    ContextTokenBudget, DeliveryMode, ErrorClass, ErrorCode, EstimatedTokens, MemoryCommandPayload,
    MemoryContent, MemoryId, MemoryKind, MemoryOrigin, MemoryRejectionReason, MemoryRelationKind,
    MemoryRevision, MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionNumber,
    MemoryRevisionStatus, MemoryScope as DomainMemoryScope, MemorySequence, MemoryValidity,
    PermissionAnswer, PermissionOutcome, PromptText, PublicMessage, ResponseText, RetryAdvice,
    RunLimits, Sensitivity, SessionId, SessionTitle, ToolCallId, ToolOutput, TrustClass,
    UsageSnapshot as DomainUsage,
};
use autoharness_engine::{
    AttemptStatus as EngineAttemptStatus, DurableEngineError, SessionAggregate,
};
use autoharness_memory::{
    MemoryCandidate, RetainedContextSource, normalized_content_hash, verify_admission_rendered_hash,
};
use autoharness_provider::{
    CancellationToken, CatalogRequest, ChatContent, ChatMessage, ChatRequest, ChatRole,
    ModelCatalog, ModelDescriptor, Provider, ProviderError, ProviderErrorKind, ProviderStreamEvent,
    ProviderToolCall, ProviderToolDefinition,
};
use autoharness_provider_codex_cli::{CodexAuthProgress, login_with_browser};
#[cfg(test)]
use autoharness_provider_gemini::{GeminiApiKey, GeminiProvider};
use autoharness_settings::{DisplayLabel, ProfileId, ProviderKind, ProviderProfile};
use autoharness_store::{
    ContextAdmissionContent, ContextTurnContent, MemoryAdmissionKey, MemoryAdmissionQuery,
    MemoryContentState, MemoryInspectionQuery, MemorySearchQuery, SessionStatus,
};
use autoharness_tool::{
    IncomingToolCall, MemoryProposal, RunBudget, ToolError, ToolRuntime, definitions, plan, replan,
};
use autoharness_tui::{
    ApiCredential, AppPorts, AttemptKey, CatalogProjection, CredentialSourceLabel,
    LocalPreferenceChange, LocalUserProfileProjection, ProfileConnectionState,
    ProfileCredentialStateLabel, ProfilesProjection, ProviderKindLabel, ProviderProfileDraft,
    ProviderProfileProjection, RequestId, RetryPolicy, SessionBrowserEntry, SessionsProjection,
    SettingsProjection, ToolCallKey, UiFailure, UiIntent, UiNotice,
};
use futures_util::StreamExt as _;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::context_runtime::{
    ContextEpochMode, ContextPreparationInput, ContextScope, EpochCompatibility,
    FrozenContinuationInput, PreparedContextTurn, context_epoch_id, is_workspace_agents_admission,
    observe_workspace_agents, prepare_context_turn, prepare_frozen_continuation,
    retained_workspace_agents, workspace_locator_digest,
};
use crate::engine_actor::EngineHandle;
use crate::error::AppError;
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

/// Owns application orchestration while the terminal runner owns UI state.
pub struct Coordinator {
    session_id: SessionId,
    session: SessionAggregate,
    engine: EngineHandle,
    provider: Option<Arc<dyn Provider>>,
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
            UiIntent::QueryMemory { request_id, .. } => {
                self.publish_memories().await?;
                self.commit(request_id).await?;
            }
            UiIntent::RememberMemory {
                request_id,
                content,
            } => {
                self.remember_memory(request_id, content.into_string())
                    .await?;
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

    /// Rebuilds the bounded all-lifecycle Memory workspace from durable projections.
    async fn publish_memories(&self) -> Result<(), AppError> {
        let generation = self.engine.memory_mutation_generation().await?;
        let scopes = self.authorized_memory_scopes()?;
        let query = MemoryInspectionQuery::new(
            scopes,
            Vec::new(),
            None,
            projection::memory_projection_page_size(),
        )?;
        let page = self.engine.inspect_memories(query).await?;
        let stale = page.has_more();
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
        let projection = projection::memory(generation.get(), rows, stale)
            .map_err(|_| AppError::Configuration)?;
        self.ports.memories.send_replace(Arc::new(projection));
        Ok(())
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
        if self.memory_command_contains_configured_secret(&command) {
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
    ) -> bool {
        let Some(draft) = memory_command_draft(command.payload()) else {
            return false;
        };
        let contains_configured_secret = |value: &str| {
            self.provider
                .as_ref()
                .is_some_and(|provider| provider.redact_secrets(value) != value)
                || self.profiles.as_ref().is_some_and(|profiles| {
                    [
                        profiles.environment.gemini.as_deref(),
                        profiles.environment.router.as_deref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|secret| !secret.is_empty() && value.contains(secret))
                })
        };
        contains_configured_secret(draft.content().as_str())
            || draft.evidence().iter().any(|evidence| {
                evidence
                    .excerpt()
                    .is_some_and(|excerpt| contains_configured_secret(excerpt.as_str()))
            })
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
                let credential_source = if runtime.environment.has(managed.profile.kind()) {
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
        let credential_source =
            active_profile.map_or(CredentialSourceLabel::SessionOnly, |profile| {
                if runtime.environment.has(profile.profile.kind()) {
                    CredentialSourceLabel::Environment
                } else if profile.credential_state == StoredCredentialState::Stored {
                    CredentialSourceLabel::CredentialVault
                } else {
                    CredentialSourceLabel::SessionOnly
                }
            });
        let credential_connected = active_profile.is_some_and(|profile| {
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
                self.catalog_models.clear();
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
        let redacted_prompt = self
            .provider
            .as_ref()
            .expect("provider presence checked before prompt admission")
            .redact_secrets(&prompt);
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
        let base_request = match build_request(&self.session, &attempt_id, advertise_tools) {
            Ok(request) => request,
            Err(error) => {
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
        };
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
            .prepare_and_bind_context(&attempt_id, base_request)
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
        });
        Ok(())
    }

    async fn prepare_and_bind_context(
        &mut self,
        attempt_id: &AttemptId,
        request: ChatRequest,
    ) -> Result<PreparedContextTurn, AppError> {
        for attempt_index in 0..CONTEXT_SNAPSHOT_ATTEMPTS {
            let prepared = self
                .prepare_context_snapshot(attempt_id, request.clone())
                .await?;
            let binding = ids::command(CommandPayload::BindContextTurn {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                run_turn: prepared.manifest().run_turn(),
                context_turn_id: prepared.manifest().context_turn_id().clone(),
                manifest_hash: prepared.manifest().manifest_hash().clone(),
            });
            match self
                .engine
                .commit_context_turn_and_bind(prepared.commit().clone(), binding)
                .await
            {
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
                    return Ok(prepared);
                }
                Err(error)
                    if attempt_index + 1 < CONTEXT_SNAPSHOT_ATTEMPTS
                        && context_snapshot_conflict(&error) =>
                {
                    continue;
                }
                Err(error) => return Err(AppError::Engine(error)),
            }
        }
        Err(AppError::Configuration)
    }

    async fn prepare_context_snapshot(
        &self,
        attempt_id: &AttemptId,
        request: ChatRequest,
    ) -> Result<PreparedContextTurn, AppError> {
        let attempt = self
            .session
            .attempt(attempt_id)
            .ok_or(AppError::Configuration)?;
        let run_turn = attempt
            .turns_started()
            .checked_add(1)
            .ok_or(AppError::Configuration)?;
        let expected_session_sequence = self
            .session
            .last_sequence()
            .ok_or(AppError::Configuration)?;
        let descriptor = self.catalog_models.iter().find(|descriptor| {
            descriptor.provider_id == *attempt.model().provider_id()
                && descriptor.model_id == *attempt.model().model_id()
        });
        if run_turn > 1 {
            let epoch = self
                .engine
                .load_context_epoch(context_epoch_id(attempt_id))
                .await?
                .ok_or(AppError::Configuration)?;
            let baseline_turn = self
                .engine
                .load_attempt_context_turn(attempt_id.clone(), 1)
                .await?
                .ok_or(AppError::Configuration)?;
            let baseline_content = self.load_frozen_context_content(&baseline_turn).await?;
            let retrieval_scope = self
                .context_scope()?
                .retrieval_scope(self.session_id.clone(), epoch.started_at());
            let compatibility = EpochCompatibility::new(
                &request,
                descriptor,
                &retrieval_scope,
                epoch.token_budget(),
                baseline_turn.budget().durable_memory_limit(),
            )?;
            return prepare_frozen_continuation(FrozenContinuationInput {
                request,
                expected_session_sequence,
                run_turn,
                committed_at: ids::now(),
                epoch,
                baseline_turn,
                baseline_content,
                compatibility,
            });
        }

        let committed_at = ids::now();
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
            .collect();
        let reported_token_limit = descriptor
            .and_then(|descriptor| descriptor.input_token_limit)
            .unwrap_or(DEFAULT_CONTEXT_TOKEN_BUDGET);
        let token_budget = ContextTokenBudget::new(
            reported_token_limit.saturating_mul(CONTEXT_SIZER_BYTES_PER_TOKEN),
        )
        .map_err(|_| AppError::Configuration)?;
        let durable_memory_limit =
            EstimatedTokens::new(token_budget.get() / 4).map_err(|_| AppError::Configuration)?;
        let compatibility = EpochCompatibility::new(
            &request,
            descriptor,
            &retrieval_scope,
            token_budget,
            durable_memory_limit,
        )?;
        let retained_sources = self.retained_workspace_agents(attempt_id).await?;
        let environment_secrets = self.environment_credential_sentinels();
        let observed_sources = observe_workspace_agents(
            &self.workspace,
            self.provider.as_deref(),
            &environment_secrets,
            committed_at,
            retained_sources,
        )?;
        prepare_context_turn(ContextPreparationInput {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
            run_turn,
            expected_session_sequence,
            memory_generation: candidate_batch.generation(),
            model: attempt.model().clone(),
            request,
            retrieval_scope,
            compatibility,
            epoch: ContextEpochMode::NewAttempt {
                explicit_retry: attempt.retry_of().is_some(),
            },
            observed_sources,
            memory_candidates: candidates,
            committed_at,
        })
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
        Ok(ContextTurnContent::new(prelude, contents))
    }

    async fn retained_workspace_agents(
        &self,
        current_attempt_id: &AttemptId,
    ) -> Result<Vec<RetainedContextSource>, AppError> {
        for attempt in self.session.attempts().iter().rev() {
            if attempt.attempt_id() == current_attempt_id {
                continue;
            }
            let Some(turn) = self
                .engine
                .load_attempt_context_turn(attempt.attempt_id().clone(), 1)
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
            // A newer attempt may have observed the optional source as absent.
            // Keep searching for the latest retained value so an unavailable
            // read never silently discards previously verified instructions.
        }
        Ok(Vec::new())
    }

    fn environment_credential_sentinels(&self) -> Vec<&str> {
        self.profiles
            .as_ref()
            .into_iter()
            .flat_map(|profiles| {
                [
                    profiles
                        .environment
                        .gemini
                        .as_ref()
                        .map(|secret| secret.as_str()),
                    profiles
                        .environment
                        .router
                        .as_ref()
                        .map(|secret| secret.as_str()),
                ]
                .into_iter()
                .flatten()
            })
            .collect()
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
                }
                let failure = provider_failure(&error);
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Failed(failure.clone())));
                if let Some(request_id) = request_id {
                    self.reject(request_id, failure).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_stream(
        &mut self,
        attempt_id: AttemptId,
        result: Result<ProviderStreamEvent, ProviderError>,
        _benchmark_chunk_sequence: Option<u64>,
    ) -> Result<(), AppError> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.attempt_id != attempt_id {
            return Ok(());
        }
        let cancellation_requested = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| attempt.status() == EngineAttemptStatus::CancellationRequested);

        match result {
            Ok(ProviderStreamEvent::Started) => {}
            Ok(ProviderStreamEvent::TextDelta(delta)) => {
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
        if self.memory_command_contains_configured_secret(&command) {
            return Err(MemoryProposalPersistenceError::Safe(
                memory_proposal_invalid(),
            ));
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
        if self.provider.is_none()
            || self
                .session
                .attempt(&attempt_id)
                .is_none_or(|attempt| attempt.status() != EngineAttemptStatus::AwaitingTools)
            || !self.active_tool_calls_settled()
        {
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
        let base_request = match build_request(&self.session, &attempt_id, advertise_tools) {
            Ok(request) => request,
            Err(_) => {
                self.fail_context_preparation(&attempt_id).await?;
                self.active = None;
                return Ok(());
            }
        };
        let prepared = match self
            .prepare_and_bind_context(&attempt_id, base_request)
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
    let attempt = session
        .attempt(attempt_id)
        .ok_or_else(|| ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never))?;
    let mut messages = Vec::new();
    for input in session.admitted_inputs().iter().filter(|input| {
        input.promoted_by().is_some()
            && (input.input_id() == attempt.input_id()
                || session.attempts().iter().any(|candidate| {
                    candidate.input_id() == input.input_id()
                        && candidate.status() == EngineAttemptStatus::Completed
                }))
    }) {
        messages.push(ChatMessage::text(
            ChatRole::User,
            ChatContent::new(input.prompt().as_str())?,
        ));
        for response in session.attempts().iter().filter(|candidate| {
            candidate.input_id() == input.input_id()
                && (candidate.status() == EngineAttemptStatus::Completed
                    || (candidate.attempt_id() == attempt_id
                        && session
                            .tool_calls()
                            .iter()
                            .any(|call| call.attempt_id() == attempt_id)))
        }) {
            let text = response.response_text();
            if !text.trim().is_empty() {
                messages.push(ChatMessage::text(
                    ChatRole::Assistant,
                    ChatContent::new(text)?,
                ));
            }
            for tool_call in session
                .tool_calls()
                .iter()
                .filter(|call| call.attempt_id() == response.attempt_id())
            {
                messages.push(ChatMessage::ToolCall(ProviderToolCall {
                    provider_call_id: tool_call.call().provider_call_id.clone(),
                    tool_name: tool_call.call().tool_name.clone(),
                    arguments: tool_call.call().arguments.clone(),
                }));
                if tool_call.status().is_settled() {
                    let result = tool_result_content(tool_call);
                    messages.push(ChatMessage::ToolResult {
                        provider_call_id: tool_call.call().provider_call_id.clone(),
                        tool_name: tool_call.call().tool_name.clone(),
                        content: ChatContent::new(result)?,
                    });
                }
            }
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

fn recover_active_attempt(
    session: &SessionAggregate,
    shutdown: &CancellationToken,
) -> Option<ActiveAttempt> {
    let attempt = session
        .attempts()
        .iter()
        .rev()
        .find(|attempt| attempt.status() == EngineAttemptStatus::AwaitingTools)?;
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
    let budget = RunBudget::restore(
        limits,
        elapsed,
        attempt.turns_started(),
        tokens,
        output_bytes,
        0,
    )
    .expect("durable run counters fit configured limits");
    Some(ActiveAttempt {
        attempt_id: attempt.attempt_id().clone(),
        cancellation: shutdown.child_token(),
        budget,
        usage_base: attempt.usage().unwrap_or_default(),
    })
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

fn stale_memory_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Conflict,
        "That memory changed or is no longer in the requested state",
        RetryPolicy::Now,
    )
    .with_code("stale_memory")
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
        AppError::Provider(_) | AppError::Terminal => UiFailure::new(
            ErrorClass::Unavailable,
            "The durable memory operation is temporarily unavailable",
            RetryPolicy::Now,
        )
        .with_code("memory_unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
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
        ProviderMetadata, TextDelta, UsageSnapshot as ProviderUsage,
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
            value.to_owned()
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

    enum ProposalProviderTurn {
        Tools(Vec<ProviderToolCall>),
        Complete(&'static str),
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
                    scope: DomainMemoryScope::Workspace(workspace_id),
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
        let archive_prefix = format!("autoharness-session-{}.export.v2-", second.session_id);
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
        assert_eq!(turns[0].sources().len(), 1);
        assert_eq!(turns[1].sources().len(), 1);
        assert_eq!(turns[2].sources().len(), 1);
        assert_eq!(
            turns[0].sources()[0].observation_state(),
            autoharness_domain::ContextObservationState::Available
        );
        assert_eq!(
            turns[1].sources()[0].observation_state(),
            autoharness_domain::ContextObservationState::RetainedStale
        );
        assert_eq!(
            turns[2].sources()[0].observation_state(),
            autoharness_domain::ContextObservationState::ObservedAbsent
        );
        assert_eq!(
            turns[3].sources()[0].observation_state(),
            autoharness_domain::ContextObservationState::RetainedStale
        );
        assert_eq!(
            turns[0].sources()[0].source_revision(),
            turns[1].sources()[0].source_revision()
        );
        assert_eq!(
            turns[0].sources()[0].source_revision(),
            turns[3].sources()[0].source_revision()
        );
        assert!(turns[2].sources()[0].source_revision().is_none());
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
        assert_eq!(first_turn.sources(), second_turn.sources());
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
        let awaiting_permission = wait_for_session(&mut ui.sessions, |projection| {
            projection.permission_requests.len() == 1
        })
        .await;
        let permission = awaiting_permission.permission_requests[0].clone();
        let deny_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::AnswerPermission {
                request_id: deny_request,
                tool_call_id: permission.tool_call_id,
                allow: false,
            })
            .await
            .expect("deny incomplete source call");
        expect_commit(&mut ui, deny_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Completed,
                        text,
                        ..
                    } if text == "invalid proposals settled"
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
        assert_eq!(proposal_calls.len(), 3);
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
            assert!(events.iter().any(|event| matches!(
                event.payload(),
                EventPayload::ToolCallFailed { tool_call_id, .. }
                    if tool_call_id == &call.tool_call_id
            )));
        }
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
            assert_eq!(requests.len(), 2);
            let results = requests[1]
                .messages
                .iter()
                .filter_map(|message| match message {
                    ChatMessage::ToolResult { content, .. } => Some(content.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(results.len(), 4);
            assert!(results.iter().all(|result| !result.contains(SENTINEL)));
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
