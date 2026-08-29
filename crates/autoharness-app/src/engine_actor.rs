use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use autoharness_domain::{
    AttemptFailure, CommandEnvelope, CommandPayload, ErrorClass, ErrorCode, PublicMessage,
    RetryAdvice, SessionId, Sha256Digest, WorkspaceId,
};
use autoharness_engine::{
    AttemptStatus, DurableEngine, DurableEngineError, SessionAggregate, ToolCallStatus,
};
use autoharness_store::{DeletionDisposition, SessionStatus, SessionStore, SessionSummary};
use autoharness_store_sqlite::SqliteStore;
use tokio::sync::{mpsc, oneshot};

use crate::error::AppError;
use crate::ids::{self, RuntimeMetadata};
use crate::telemetry;

type LocalEngine = DurableEngine<SqliteStore, RuntimeMetadata>;

/// Successful durable command result returned to application composition.
#[derive(Clone, Debug)]
pub struct EngineReply {
    /// Newest replay-equivalent session projection.
    pub session: SessionAggregate,
}

/// Storage-level request served on the dedicated blocking thread.
#[allow(dead_code)] // Typed inspection/admin requests are the app's bounded storage-thread API.
pub enum StorageRequest {
    /// Executes one engine command against one session aggregate.
    Execute {
        command: Box<CommandEnvelope>,
        reply: oneshot::Sender<Result<EngineReply, DurableEngineError>>,
    },
    /// Lists every durable session summary in recent-first order.
    ListSessions {
        reply: oneshot::Sender<Result<Vec<SessionSummary>, AppError>>,
    },
    /// Resolves a canonical-locator digest to a random persisted workspace ID.
    ResolveWorkspaceId {
        locator_digest: Sha256Digest,
        reply: oneshot::Sender<Result<WorkspaceId, AppError>>,
    },
    /// Loads one session's full event history for replay or export.
    LoadEvents {
        session_id: SessionId,
        reply: oneshot::Sender<Result<Vec<autoharness_domain::EventEnvelope>, AppError>>,
    },
    /// Exports one session to JSON, then deletes it when the export succeeds.
    ExportAndDeleteSession {
        session_id: SessionId,
        expected_last_sequence: u64,
        reply: oneshot::Sender<Result<Option<std::path::PathBuf>, AppError>>,
    },
    /// Writes the active session transcript as Markdown beside the database.
    ExportTranscriptMarkdown {
        session_id: SessionId,
        reply: oneshot::Sender<Result<std::path::PathBuf, AppError>>,
    },
    /// Exports one authorized memory ledger and retained sidecars as atomic JSON.
    ExportMemory {
        memory_id: autoharness_domain::MemoryId,
        authorized_scopes: Vec<autoharness_domain::MemoryScope>,
        reply: oneshot::Sender<Result<std::path::PathBuf, AppError>>,
    },
    /// Appends one optimistic memory-ledger operation and all erasable sidecars.
    AppendMemory {
        request: autoharness_store::MemoryAppendRequest,
        reply: oneshot::Sender<Result<autoharness_store::MemoryAppendReceipt, AppError>>,
    },
    /// Executes one typed trusted memory command as an atomic contiguous batch.
    ExecuteMemoryCommand {
        command: autoharness_domain::MemoryCommandEnvelope,
        occurred_at: autoharness_domain::TimestampMillis,
        reply: oneshot::Sender<Result<crate::memory_runtime::MemoryCommandCommit, AppError>>,
    },
    /// Loads one bounded page from a memory item's authoritative ledger.
    LoadMemoryOperations {
        memory_id: autoharness_domain::MemoryId,
        after_sequence: u64,
        limit: u32,
        reply: oneshot::Sender<Result<Vec<autoharness_domain::MemoryOperationEnvelope>, AppError>>,
    },
    /// Loads every retained revision metadata record for one memory item.
    LoadMemoryRevisions {
        memory_id: autoharness_domain::MemoryId,
        reply: oneshot::Sender<Result<Vec<autoharness_domain::MemoryRevision>, AppError>>,
    },
    /// Loads an erasable revision content sidecar when it is still retained.
    LoadMemoryContent {
        revision_id: autoharness_domain::MemoryRevisionId,
        reply: oneshot::Sender<Result<Option<autoharness_domain::MemoryContent>, AppError>>,
    },
    /// Loads one integrity-checked immutable revision for frozen context reconstruction.
    LoadMemoryCandidate {
        revision_id: autoharness_domain::MemoryRevisionId,
        reply: oneshot::Sender<Result<Option<autoharness_store::StoredMemoryCandidate>, AppError>>,
    },
    /// Retrieves one immutable, generation-bound memory candidate batch.
    SearchMemory {
        query: autoharness_store::MemorySearchQuery,
        reply: oneshot::Sender<Result<autoharness_store::MemoryCandidateBatch, AppError>>,
    },
    /// Lists a bounded all-lifecycle page for the Memory workspace.
    InspectMemories {
        query: autoharness_store::MemoryInspectionQuery,
        reply: oneshot::Sender<Result<Vec<autoharness_store::MemoryInspectionRecord>, AppError>>,
    },
    /// Loads bounded newest-first memory admission history.
    LoadMemoryAdmissions {
        query: autoharness_store::MemoryAdmissionQuery,
        reply: oneshot::Sender<Result<Vec<autoharness_store::MemoryAdmissionRecord>, AppError>>,
    },
    /// Reads the current global memory eligibility generation.
    MemoryGeneration {
        reply: oneshot::Sender<Result<autoharness_domain::MemoryGeneration, AppError>>,
    },
    /// Reads the projection generation advanced by every logical mutation.
    MemoryMutationGeneration {
        reply: oneshot::Sender<Result<autoharness_store::MemoryMutationGeneration, AppError>>,
    },
    /// Rebuilds memory lifecycle and FTS projections from the durable ledger.
    RebuildMemoryProjections {
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    /// Atomically commits one immutable provider-turn context manifest.
    CommitContextTurn {
        request: autoharness_store::ContextTurnCommitRequest,
        reply: oneshot::Sender<Result<autoharness_store::ContextCommitDisposition, AppError>>,
    },
    /// Atomically commits one provider-turn manifest and its exact session binding event.
    CommitContextTurnAndBind {
        context: autoharness_store::ContextTurnCommitRequest,
        command: Box<CommandEnvelope>,
        reply: oneshot::Sender<Result<EngineReply, DurableEngineError>>,
    },
    /// Loads one immutable context epoch.
    LoadContextEpoch {
        epoch_id: autoharness_domain::ContextEpochId,
        reply: oneshot::Sender<Result<Option<autoharness_domain::ContextEpochManifest>, AppError>>,
    },
    /// Loads one exact immutable provider-turn context manifest.
    LoadContextTurn {
        context_turn_id: autoharness_domain::ContextTurnId,
        reply: oneshot::Sender<Result<Option<autoharness_domain::ContextTurnManifest>, AppError>>,
    },
    /// Loads all ordered admissions for one exact provider turn.
    LoadContextAdmissions {
        context_turn_id: autoharness_domain::ContextTurnId,
        reply: oneshot::Sender<Result<Vec<autoharness_domain::ContextAdmission>, AppError>>,
    },
    /// Loads retained exact provider-visible prelude bytes for one turn.
    LoadContextTurnContent {
        context_turn_id: autoharness_domain::ContextTurnId,
        reply: oneshot::Sender<Result<Option<autoharness_store::RenderedContextText>, AppError>>,
    },
    /// Loads retained exact bytes for one admission.
    LoadContextAdmissionContent {
        admission_id: autoharness_domain::ContextAdmissionId,
        reply: oneshot::Sender<Result<Option<autoharness_store::RenderedContextText>, AppError>>,
    },
    /// Resolves the durable manifest for an exact attempt and one-based run turn.
    LoadAttemptContextTurn {
        attempt_id: autoharness_domain::AttemptId,
        run_turn: u32,
        reply: oneshot::Sender<Result<Option<autoharness_domain::ContextTurnManifest>, AppError>>,
    },
    Shutdown,
}

/// Cloneable asynchronous handle to the dedicated blocking storage thread.
#[derive(Clone)]
pub struct EngineHandle {
    requests: mpsc::Sender<StorageRequest>,
}

#[allow(dead_code)] // Some audit/admin projections are intentionally not dispatched by this TUI yet.
impl EngineHandle {
    /// Executes one command and resolves only after its event batch is durable.
    pub async fn execute(
        &self,
        command: CommandEnvelope,
    ) -> Result<EngineReply, DurableEngineError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::Execute {
                command: Box::new(command),
                reply,
            })
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?;
        response
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?
    }

    /// Lists every durable session summary from the storage thread.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ListSessions { reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Resolves an opaque durable authority for one canonical workspace locator.
    pub async fn resolve_workspace_id(
        &self,
        locator_digest: Sha256Digest,
    ) -> Result<WorkspaceId, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ResolveWorkspaceId {
                locator_digest,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads one session's complete authoritative event history.
    pub async fn load_events(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<autoharness_domain::EventEnvelope>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadEvents { session_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Writes the active session transcript as Markdown beside the database.
    ///
    /// Returns the written file path. The session itself is untouched.
    pub async fn export_transcript_markdown(
        &self,
        session_id: SessionId,
    ) -> Result<std::path::PathBuf, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ExportTranscriptMarkdown { session_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Exports one authorized memory ledger and retained content beside the database.
    pub async fn export_memory(
        &self,
        memory_id: autoharness_domain::MemoryId,
        authorized_scopes: Vec<autoharness_domain::MemoryScope>,
    ) -> Result<std::path::PathBuf, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ExportMemory {
                memory_id,
                authorized_scopes,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Exports one session to JSON, then deletes it when the export succeeds.
    ///
    /// The archive is written beside the database. Returns the archive path
    /// when a deletion happened and `None` when the session was already
    /// absent.
    pub async fn export_and_delete_session(
        &self,
        session_id: SessionId,
        expected_last_sequence: u64,
    ) -> Result<Option<std::path::PathBuf>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ExportAndDeleteSession {
                session_id,
                expected_last_sequence,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Appends one memory operation through the application's single storage writer.
    pub async fn append_memory(
        &self,
        request: autoharness_store::MemoryAppendRequest,
    ) -> Result<autoharness_store::MemoryAppendReceipt, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::AppendMemory { request, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Executes one typed memory command as a trusted atomic store batch.
    pub async fn execute_memory_command(
        &self,
        command: autoharness_domain::MemoryCommandEnvelope,
    ) -> Result<crate::memory_runtime::MemoryCommandCommit, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::ExecuteMemoryCommand {
                command,
                occurred_at: ids::now(),
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads one bounded page of authoritative operations for a memory item.
    pub async fn load_memory_operations(
        &self,
        memory_id: autoharness_domain::MemoryId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<autoharness_domain::MemoryOperationEnvelope>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadMemoryOperations {
                memory_id,
                after_sequence,
                limit,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads all retained revision metadata for a memory item.
    pub async fn load_memory_revisions(
        &self,
        memory_id: autoharness_domain::MemoryId,
    ) -> Result<Vec<autoharness_domain::MemoryRevision>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadMemoryRevisions { memory_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads retained exact content for one memory revision.
    pub async fn load_memory_content(
        &self,
        revision_id: autoharness_domain::MemoryRevisionId,
    ) -> Result<Option<autoharness_domain::MemoryContent>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadMemoryContent { revision_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads one exact verified memory revision through the single storage owner.
    pub async fn load_memory_candidate(
        &self,
        revision_id: autoharness_domain::MemoryRevisionId,
    ) -> Result<Option<autoharness_store::StoredMemoryCandidate>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadMemoryCandidate { revision_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Searches active eligible memory through the single storage owner.
    pub async fn search_memory(
        &self,
        query: autoharness_store::MemorySearchQuery,
    ) -> Result<autoharness_store::MemoryCandidateBatch, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::SearchMemory { query, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Lists one bounded authorized page across every memory lifecycle.
    pub async fn inspect_memories(
        &self,
        query: autoharness_store::MemoryInspectionQuery,
    ) -> Result<Vec<autoharness_store::MemoryInspectionRecord>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::InspectMemories { query, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads newest-first durable admissions for one memory or revision.
    pub async fn load_memory_admissions(
        &self,
        query: autoharness_store::MemoryAdmissionQuery,
    ) -> Result<Vec<autoharness_store::MemoryAdmissionRecord>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadMemoryAdmissions { query, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Reads the exact global memory generation used by optimistic context commits.
    pub async fn memory_generation(
        &self,
    ) -> Result<autoharness_domain::MemoryGeneration, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::MemoryGeneration { reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Reads the generation used only to refresh mutation projections.
    pub async fn memory_mutation_generation(
        &self,
    ) -> Result<autoharness_store::MemoryMutationGeneration, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::MemoryMutationGeneration { reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Rebuilds memory projections and FTS from the authoritative ledger.
    pub async fn rebuild_memory_projections(&self) -> Result<(), AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::RebuildMemoryProjections { reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Commits one exact provider-turn manifest before any network dispatch.
    pub async fn commit_context_turn(
        &self,
        request: autoharness_store::ContextTurnCommitRequest,
    ) -> Result<autoharness_store::ContextCommitDisposition, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::CommitContextTurn { request, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Atomically commits context metadata, exact bytes, and its session binding event.
    pub async fn commit_context_turn_and_bind(
        &self,
        context: autoharness_store::ContextTurnCommitRequest,
        command: CommandEnvelope,
    ) -> Result<EngineReply, DurableEngineError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::CommitContextTurnAndBind {
                context,
                command: Box::new(command),
                reply,
            })
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?;
        response
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?
    }

    /// Loads one exact durable context epoch.
    pub async fn load_context_epoch(
        &self,
        epoch_id: autoharness_domain::ContextEpochId,
    ) -> Result<Option<autoharness_domain::ContextEpochManifest>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadContextEpoch { epoch_id, reply })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads one exact durable provider-turn context manifest.
    pub async fn load_context_turn(
        &self,
        context_turn_id: autoharness_domain::ContextTurnId,
    ) -> Result<Option<autoharness_domain::ContextTurnManifest>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadContextTurn {
                context_turn_id,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads admissions for one exact provider turn in deterministic rank order.
    pub async fn load_context_admissions(
        &self,
        context_turn_id: autoharness_domain::ContextTurnId,
    ) -> Result<Vec<autoharness_domain::ContextAdmission>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadContextAdmissions {
                context_turn_id,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads retained exact provider-visible prelude bytes for one context turn.
    pub async fn load_context_turn_content(
        &self,
        context_turn_id: autoharness_domain::ContextTurnId,
    ) -> Result<Option<autoharness_store::RenderedContextText>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadContextTurnContent {
                context_turn_id,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads retained exact provider-visible bytes for one context admission.
    pub async fn load_context_admission_content(
        &self,
        admission_id: autoharness_domain::ContextAdmissionId,
    ) -> Result<Option<autoharness_store::RenderedContextText>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadContextAdmissionContent {
                admission_id,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }

    /// Loads the manifest bound to one exact attempt and one-based run turn.
    pub async fn load_attempt_context_turn(
        &self,
        attempt_id: autoharness_domain::AttemptId,
        run_turn: u32,
    ) -> Result<Option<autoharness_domain::ContextTurnManifest>, AppError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(StorageRequest::LoadAttemptContextTurn {
                attempt_id,
                run_turn,
                reply,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)?;
        response.await.map_err(|_| AppError::WorkerStopped)?
    }
}

/// Owner of the blocking SQLite writer thread.
pub struct EngineActor {
    handle: EngineHandle,
    thread: Option<JoinHandle<()>>,
}

impl EngineActor {
    /// Opens storage, replays history, and creates the first session when empty.
    pub fn start(database_path: PathBuf) -> Result<(Self, SessionId, SessionAggregate), AppError> {
        let (requests, receiver) = mpsc::channel::<StorageRequest>(64);
        let (ready, startup) = std::sync::mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("autoharness-storage".to_owned())
            .spawn(move || run(database_path, receiver, ready))
            .map_err(|_| AppError::WorkerStopped)?;
        let (session_id, session) = startup.recv().map_err(|_| AppError::WorkerStopped)??;
        let handle = EngineHandle { requests };
        Ok((
            Self {
                handle,
                thread: Some(thread),
            },
            session_id,
            session,
        ))
    }

    /// Returns a cloneable command handle.
    #[must_use]
    pub fn handle(&self) -> EngineHandle {
        self.handle.clone()
    }

    /// Stops and joins the storage thread after the terminal has been restored.
    pub async fn shutdown(mut self) -> Result<(), AppError> {
        let _ = self.handle.requests.send(StorageRequest::Shutdown).await;
        drop(self.handle);
        let thread = self.thread.take().ok_or(AppError::WorkerStopped)?;
        tokio::task::spawn_blocking(move || thread.join())
            .await
            .map_err(|_| AppError::WorkerStopped)?
            .map_err(|_| AppError::WorkerStopped)
    }
}

fn run(
    database_path: PathBuf,
    mut receiver: mpsc::Receiver<StorageRequest>,
    ready: std::sync::mpsc::SyncSender<Result<(SessionId, SessionAggregate), AppError>>,
) {
    let export_directory = database_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let workspace_binding_directory = export_directory.join("workspace-bindings");
    let (mut engine, session_id, session) = match open(database_path) {
        Ok(startup) => startup,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok((session_id, session))).is_err() {
        return;
    }

    while let Some(request) = receiver.blocking_recv() {
        match request {
            StorageRequest::Execute { command, reply } => {
                let session_id = command.session_id().clone();
                let result = engine.execute(&command).and_then(|events| {
                    let session = engine
                        .session(&session_id)
                        .cloned()
                        .ok_or(DurableEngineError::StoreInvariant)?;
                    telemetry::command_committed(
                        events.len(),
                        session.last_sequence().map_or(0, |sequence| sequence.get()),
                    );
                    Ok(EngineReply { session })
                });
                let _ = reply.send(result);
            }
            StorageRequest::ListSessions { reply } => {
                let _ = reply.send(engine.store().list_sessions().map_err(AppError::from));
            }
            StorageRequest::ResolveWorkspaceId {
                locator_digest,
                reply,
            } => {
                let result = resolve_workspace_id(&workspace_binding_directory, &locator_digest);
                let _ = reply.send(result);
            }
            StorageRequest::LoadEvents { session_id, reply } => {
                let result = load_all_events(&mut engine, &session_id);
                let _ = reply.send(result);
            }
            StorageRequest::ExportTranscriptMarkdown { session_id, reply } => {
                // Markdown rendering shares the JSON archive's source of
                // truth (durable events) and destination (beside the
                // database) while leaving the session untouched.
                let result = (|| -> Result<std::path::PathBuf, AppError> {
                    let summaries = engine.store().list_sessions().map_err(AppError::from)?;
                    let Some(summary) = summaries
                        .iter()
                        .find(|summary| summary.session_id() == &session_id)
                        .cloned()
                    else {
                        return Err(AppError::Store(
                            autoharness_store::StoreError::VersionConflict {
                                session_id: session_id.clone(),
                                expected: 0,
                                actual: 0,
                            },
                        ));
                    };
                    let events = load_all_events(&mut engine, &session_id)?;
                    let bytes = autoharness_app::export_markdown::render_markdown(
                        &session_id,
                        &summary,
                        &events,
                    );
                    let file_name = format!(
                        "autoharness-transcript-{}.md",
                        summary.session_id().as_str()
                    );
                    let destination = export_directory.join(file_name);
                    std::fs::write(&destination, bytes).map_err(|_| AppError::FileSystem)?;
                    Ok(destination)
                })();
                let _ = reply.send(result);
            }
            StorageRequest::ExportMemory {
                memory_id,
                authorized_scopes,
                reply,
            } => {
                let result = crate::export::export_memory(
                    engine.store_mut(),
                    &memory_id,
                    &authorized_scopes,
                    &export_directory,
                );
                let _ = reply.send(result);
            }
            StorageRequest::ExportAndDeleteSession {
                session_id,
                expected_last_sequence,
                reply,
            } => {
                // The export and the delete share the single storage thread so
                // the pre-deletion archive always reflects durable state, and
                // a failed export aborts the deletion. Archives land beside the
                // database.
                let summaries = engine.store().list_sessions().map_err(AppError::from);
                let result = summaries.and_then(|summaries| {
                    let summary = summaries
                        .iter()
                        .find(|summary| summary.session_id() == &session_id)
                        .cloned();
                    let Some(summary) = summary else {
                        return Ok(None);
                    };
                    if summary.last_sequence().get() != expected_last_sequence {
                        return Err(AppError::Store(
                            autoharness_store::StoreError::VersionConflict {
                                session_id: session_id.clone(),
                                expected: expected_last_sequence,
                                actual: summary.last_sequence().get(),
                            },
                        ));
                    }
                    let archive = crate::export::export_session(
                        engine.store_mut(),
                        &summary,
                        &export_directory,
                    )
                    .map(Some)?;
                    match engine
                        .store_mut()
                        .delete_session(&session_id, expected_last_sequence)
                        .map_err(AppError::from)?
                    {
                        DeletionDisposition::Deleted => Ok(archive),
                        DeletionDisposition::NotFound => Ok(None),
                    }
                });
                telemetry::session_deleted(matches!(result, Ok(Some(_))));
                let _ = reply.send(result);
            }
            StorageRequest::AppendMemory { request, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store_mut()
                    .append_memory(&request)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::ExecuteMemoryCommand {
                command,
                occurred_at,
                reply,
            } => {
                let result = crate::memory_runtime::execute_memory_command(
                    engine.store_mut(),
                    &command,
                    occurred_at,
                );
                let _ = reply.send(result);
            }
            StorageRequest::LoadMemoryOperations {
                memory_id,
                after_sequence,
                limit,
                reply,
            } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .load_memory_operations(&memory_id, after_sequence, limit)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadMemoryRevisions { memory_id, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .load_memory_revisions(&memory_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadMemoryContent { revision_id, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .load_memory_content(&revision_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadMemoryCandidate { revision_id, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .load_memory_candidate(&revision_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::SearchMemory { query, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine.store().search_memory(&query).map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::InspectMemories { query, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .inspect_memories(&query)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadMemoryAdmissions { query, reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .load_memory_admissions(&query)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::MemoryGeneration { reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine.store().memory_generation().map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::MemoryMutationGeneration { reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store()
                    .memory_mutation_generation()
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::RebuildMemoryProjections { reply } => {
                use autoharness_store::MemoryStore as _;

                let result = engine
                    .store_mut()
                    .rebuild_memory_projections()
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::CommitContextTurn { request, reply } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store_mut()
                    .commit_context_turn(&request)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::CommitContextTurnAndBind {
                context,
                command,
                reply,
            } => {
                let session_id = command.session_id().clone();
                let result = engine
                    .commit_context_turn_and_bind(context, &command)
                    .and_then(|events| {
                        let session = engine
                            .session(&session_id)
                            .cloned()
                            .ok_or(DurableEngineError::StoreInvariant)?;
                        telemetry::command_committed(
                            events.len(),
                            session.last_sequence().map_or(0, |sequence| sequence.get()),
                        );
                        Ok(EngineReply { session })
                    });
                let _ = reply.send(result);
            }
            StorageRequest::LoadContextEpoch { epoch_id, reply } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_context_epoch(&epoch_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadContextTurn {
                context_turn_id,
                reply,
            } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_context_turn(&context_turn_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadContextAdmissions {
                context_turn_id,
                reply,
            } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_context_admissions(&context_turn_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadContextTurnContent {
                context_turn_id,
                reply,
            } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_context_turn_content(&context_turn_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadContextAdmissionContent {
                admission_id,
                reply,
            } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_context_admission_content(&admission_id)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::LoadAttemptContextTurn {
                attempt_id,
                run_turn,
                reply,
            } => {
                use autoharness_store::ContextStore as _;

                let result = engine
                    .store()
                    .load_attempt_context_turn(&attempt_id, run_turn)
                    .map_err(AppError::from);
                let _ = reply.send(result);
            }
            StorageRequest::Shutdown => break,
        }
    }
}

fn load_all_events(
    engine: &mut LocalEngine,
    session_id: &autoharness_domain::SessionId,
) -> Result<Vec<autoharness_domain::EventEnvelope>, AppError> {
    use autoharness_store::{DEFAULT_EVENT_PAGE_SIZE, SessionStore as _};

    let mut events = Vec::new();
    let mut after = 0_u64;
    loop {
        let page = engine
            .store()
            .load_events(session_id, after, DEFAULT_EVENT_PAGE_SIZE)
            .map_err(AppError::from)?;
        let loaded = page.len();
        if let Some(last) = page.last() {
            after = last.sequence().get();
        }
        events.extend(page);
        if loaded < usize::try_from(DEFAULT_EVENT_PAGE_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(events)
}

fn resolve_workspace_id(
    binding_directory: &std::path::Path,
    locator_digest: &Sha256Digest,
) -> Result<WorkspaceId, AppError> {
    use std::io::Write as _;

    std::fs::create_dir_all(binding_directory).map_err(|_| AppError::FileSystem)?;
    let destination = binding_directory.join(format!("{}.id", locator_digest.as_str()));
    if destination.exists() {
        return read_workspace_binding(&destination);
    }

    let workspace_id = ids::workspace_id();
    let temporary = binding_directory.join(format!("{}.tmp", workspace_id.as_str()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| AppError::FileSystem)?;
    file.write_all(workspace_id.as_str().as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|_| AppError::FileSystem)?;
    drop(file);

    match std::fs::rename(&temporary, &destination) {
        Ok(()) => Ok(workspace_id),
        Err(_) if destination.exists() => {
            let _ = std::fs::remove_file(&temporary);
            read_workspace_binding(&destination)
        }
        Err(_) => {
            let _ = std::fs::remove_file(&temporary);
            Err(AppError::FileSystem)
        }
    }
}

fn read_workspace_binding(path: &std::path::Path) -> Result<WorkspaceId, AppError> {
    let value = std::fs::read_to_string(path).map_err(|_| AppError::FileSystem)?;
    WorkspaceId::new(value.trim().to_owned()).map_err(|_| AppError::Configuration)
}

fn open(database_path: PathBuf) -> Result<(LocalEngine, SessionId, SessionAggregate), AppError> {
    let store = SqliteStore::open(database_path)?;
    let mut engine = DurableEngine::recover(store, RuntimeMetadata)?;
    let active_sessions: Vec<_> = engine
        .store()
        .list_sessions()?
        .into_iter()
        .filter(|summary| summary.status() == SessionStatus::Active)
        .map(|summary| summary.session_id().clone())
        .collect();
    let (failed_before_dispatch, marked_unknown) =
        reconcile_interrupted_attempts(&mut engine, &active_sessions)?;
    telemetry::storage_recovered(
        active_sessions.len(),
        failed_before_dispatch,
        marked_unknown,
    );

    let session_id = match active_sessions.into_iter().next() {
        Some(session_id) => session_id,
        None => {
            let session_id = ids::session_id();
            engine.execute(&ids::command(CommandPayload::CreateSession {
                session_id: session_id.clone(),
            }))?;
            session_id
        }
    };
    let session = engine
        .session(&session_id)
        .cloned()
        .ok_or(AppError::Configuration)?;
    Ok((engine, session_id, session))
}

fn reconcile_interrupted_attempts(
    engine: &mut LocalEngine,
    session_ids: &[SessionId],
) -> Result<(usize, usize), AppError> {
    let mut failed_before_dispatch = 0_usize;
    let mut marked_unknown = 0_usize;
    for session_id in session_ids {
        let interrupted_tools: Vec<_> = engine
            .session(session_id)
            .ok_or(AppError::Configuration)?
            .tool_calls()
            .iter()
            .filter(|call| !call.status().is_settled())
            .map(|call| {
                let attempt_status = engine
                    .session(session_id)
                    .and_then(|session| session.attempt(call.attempt_id()))
                    .map(autoharness_engine::AttemptProjection::status)
                    .ok_or(AppError::Configuration)?;
                Ok((
                    call.call().tool_call_id.clone(),
                    call.status(),
                    attempt_status,
                ))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        for (tool_call_id, status, attempt_status) in interrupted_tools {
            let payload = match status {
                ToolCallStatus::Running => CommandPayload::MarkToolCallUnknown {
                    session_id: session_id.clone(),
                    tool_call_id,
                },
                ToolCallStatus::DeniedPending => CommandPayload::DenyToolCall {
                    session_id: session_id.clone(),
                    tool_call_id,
                },
                ToolCallStatus::Proposed | ToolCallStatus::Authorized => {
                    CommandPayload::CancelToolCall {
                        session_id: session_id.clone(),
                        tool_call_id,
                    }
                }
                ToolCallStatus::PermissionPending
                    if matches!(
                        attempt_status,
                        AttemptStatus::InFlight | AttemptStatus::CancellationRequested
                    ) =>
                {
                    CommandPayload::CancelToolCall {
                        session_id: session_id.clone(),
                        tool_call_id,
                    }
                }
                ToolCallStatus::PermissionPending
                | ToolCallStatus::Completed
                | ToolCallStatus::Failed
                | ToolCallStatus::Denied
                | ToolCallStatus::Cancelled
                | ToolCallStatus::Unknown => continue,
            };
            engine.execute(&ids::command(payload))?;
        }
        let interrupted: Vec<_> = engine
            .session(session_id)
            .ok_or(AppError::Configuration)?
            .attempts()
            .iter()
            .filter(|attempt| !attempt.status().is_settled())
            .map(|attempt| {
                (
                    attempt.attempt_id().clone(),
                    attempt.status(),
                    attempt.turns_started(),
                )
            })
            .collect();
        for (attempt_id, status, turns_started) in interrupted {
            let payload = match status {
                AttemptStatus::Prepared => CommandPayload::FailAttempt {
                    session_id: session_id.clone(),
                    attempt_id,
                    failure: interrupted_before_dispatch(),
                },
                AttemptStatus::InFlight | AttemptStatus::CancellationRequested
                    if turns_started == 0 =>
                {
                    CommandPayload::FailAttempt {
                        session_id: session_id.clone(),
                        attempt_id,
                        failure: interrupted_before_dispatch(),
                    }
                }
                AttemptStatus::InFlight | AttemptStatus::CancellationRequested => {
                    CommandPayload::MarkAttemptUnknown {
                        session_id: session_id.clone(),
                        attempt_id,
                    }
                }
                AttemptStatus::AwaitingTools => continue,
                AttemptStatus::Completed
                | AttemptStatus::Failed
                | AttemptStatus::Cancelled
                | AttemptStatus::Unknown => continue,
            };
            match status {
                AttemptStatus::Prepared => failed_before_dispatch += 1,
                AttemptStatus::InFlight | AttemptStatus::CancellationRequested
                    if turns_started == 0 =>
                {
                    failed_before_dispatch += 1;
                }
                AttemptStatus::InFlight | AttemptStatus::CancellationRequested => {
                    marked_unknown += 1;
                }
                AttemptStatus::AwaitingTools => {}
                AttemptStatus::Completed
                | AttemptStatus::Failed
                | AttemptStatus::Cancelled
                | AttemptStatus::Unknown => {}
            }
            engine.execute(&ids::command(payload))?;
        }
    }
    Ok((failed_before_dispatch, marked_unknown))
}

fn interrupted_before_dispatch() -> AttemptFailure {
    AttemptFailure::new(
        ErrorClass::Unavailable,
        ErrorCode::new("interrupted_before_dispatch").expect("static recovery error code is valid"),
        PublicMessage::new("The attempt was interrupted before provider dispatch")
            .expect("static recovery message is valid"),
        RetryAdvice::Immediate,
    )
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        ContextTokenBudget, DeliveryMode, EstimatedTokens, InputId, ModelId, ModelRef,
        PermissionDecisionId, PermissionOutcome, PromptText, ProviderCallId, ProviderId,
        TimestampMillis, ToolArguments, ToolName, WorkspaceId,
    };
    use autoharness_provider::{ChatContent, ChatMessage, ChatRequest, ChatRole};
    use autoharness_store::{ContextStore as _, MemoryStore as _};
    use autoharness_tool::{IncomingToolCall, plan};

    use super::*;

    fn seed_attempt(database: PathBuf, start: bool) -> (SessionId, autoharness_domain::AttemptId) {
        seed_attempt_with_dispatch(database, start, start)
    }

    fn seed_attempt_with_dispatch(
        database: PathBuf,
        start: bool,
        dispatch: bool,
    ) -> (SessionId, autoharness_domain::AttemptId) {
        let (mut engine, session_id, _) = open(database).expect("open fixture store");
        let model = ModelRef::new(
            ProviderId::new("gemini").expect("provider ID"),
            ModelId::new("models/gemini-fixture").expect("model ID"),
        );
        engine
            .execute(&ids::command(CommandPayload::SelectModel {
                session_id: session_id.clone(),
                model,
            }))
            .expect("select model");
        let input_id = InputId::new("input-recovery").expect("input ID");
        let attempt_id = ids::attempt_id();
        engine
            .execute(&ids::command(
                CommandPayload::AdmitPromptAndPrepareAttempt {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                    input_id,
                    prompt: PromptText::new("recover me").expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            ))
            .expect("admit prompt and prepare attempt");
        if start {
            engine
                .execute(&ids::command(CommandPayload::StartAttempt {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                }))
                .expect("start attempt");
            if dispatch {
                mark_provider_dispatched(&mut engine, &session_id, &attempt_id);
            }
        }
        (session_id, attempt_id)
    }

    fn mark_provider_dispatched(
        engine: &mut LocalEngine,
        session_id: &SessionId,
        attempt_id: &autoharness_domain::AttemptId,
    ) {
        let session = engine.session(session_id).expect("session");
        let attempt = session.attempt(attempt_id).expect("attempt");
        let committed_at = TimestampMillis::new(10);
        let scope = crate::context_runtime::ContextScope::local(
            WorkspaceId::new("workspace-recovery-fixture").expect("workspace ID"),
        );
        let retrieval_scope = scope.retrieval_scope(session_id.clone(), committed_at);
        let request = ChatRequest::new(
            attempt.model().model_id().clone(),
            vec![ChatMessage::text(
                ChatRole::User,
                ChatContent::new("recovery fixture").expect("chat content"),
            )],
        )
        .expect("chat request");
        let compatibility = crate::context_runtime::EpochCompatibility::new(
            &request,
            None,
            &retrieval_scope,
            ContextTokenBudget::new(16_384).expect("context budget"),
            EstimatedTokens::new(4_096).expect("memory budget"),
        )
        .expect("compatibility");
        let prepared = crate::context_runtime::prepare_context_turn(
            crate::context_runtime::ContextPreparationInput {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                run_turn: 1,
                expected_session_sequence: session.last_sequence().expect("session sequence"),
                memory_generation: engine.store().memory_generation().expect("generation"),
                model: attempt.model().clone(),
                request,
                retrieval_scope,
                compatibility,
                existing_epoch: None,
                observed_sources: Vec::new(),
                memory_candidates: Vec::new(),
                committed_at,
                explicit_retry: false,
            },
        )
        .expect("prepared context");
        engine
            .store_mut()
            .commit_context_turn(prepared.commit())
            .expect("test-only standalone context commit");
        engine
            .execute(&ids::command(CommandPayload::BindContextTurn {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                run_turn: 1,
                context_turn_id: prepared.manifest().context_turn_id().clone(),
                manifest_hash: prepared.manifest().manifest_hash().clone(),
            }))
            .expect("bind context turn");
        engine
            .execute(&ids::command(CommandPayload::StartRunTurn {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
            }))
            .expect("start run turn");
    }

    fn seed_tool_state(
        database: PathBuf,
        outcome: PermissionOutcome,
        start_tool: bool,
        pause_attempt: bool,
    ) -> (
        SessionId,
        autoharness_domain::AttemptId,
        autoharness_domain::ToolCallId,
    ) {
        let (mut engine, session_id, _) = open(database).expect("open fixture store");
        let model = ModelRef::new(
            ProviderId::new("gemini").expect("provider ID"),
            ModelId::new("models/gemini-fixture").expect("model ID"),
        );
        engine
            .execute(&ids::command(CommandPayload::SelectModel {
                session_id: session_id.clone(),
                model,
            }))
            .expect("select model");
        let attempt_id = ids::attempt_id();
        engine
            .execute(&ids::command(
                CommandPayload::AdmitPromptAndPrepareAttempt {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                    input_id: InputId::new("input-tool-recovery").expect("input ID"),
                    prompt: PromptText::new("recover the tool").expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            ))
            .expect("admit prompt and prepare attempt");
        engine
            .execute(&ids::command(CommandPayload::StartAttempt {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
            }))
            .expect("start attempt");
        mark_provider_dispatched(&mut engine, &session_id, &attempt_id);
        let planned = plan(IncomingToolCall {
            tool_call_id: ids::tool_call_id(),
            provider_call_id: ProviderCallId::new("provider-call-recovery")
                .expect("provider call ID"),
            tool_name: ToolName::new("fs_write").expect("tool name"),
            arguments: ToolArguments::new(serde_json::json!({
                "path": "recovered.txt",
                "content": "durable"
            }))
            .expect("tool arguments"),
        })
        .expect("planned tool call");
        let tool_call_id = planned.spec().tool_call_id.clone();
        engine
            .execute(&ids::command(CommandPayload::ProposeToolCall {
                session_id: session_id.clone(),
                attempt_id: attempt_id.clone(),
                call: planned.spec().clone(),
            }))
            .expect("propose tool call");
        engine
            .execute(&ids::command(CommandPayload::RecordToolPermission {
                session_id: session_id.clone(),
                tool_call_id: tool_call_id.clone(),
                decision_id: PermissionDecisionId::new("permission-recovery")
                    .expect("permission ID"),
                outcome,
            }))
            .expect("record permission");
        if start_tool {
            engine
                .execute(&ids::command(CommandPayload::StartToolCall {
                    session_id: session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                }))
                .expect("start tool call");
        }
        if pause_attempt {
            engine
                .execute(&ids::command(CommandPayload::PauseAttemptForTools {
                    session_id: session_id.clone(),
                    attempt_id: attempt_id.clone(),
                }))
                .expect("pause attempt");
        }
        (session_id, attempt_id, tool_call_id)
    }

    #[test]
    fn recovery_fails_prepared_attempt_once_and_keeps_it_retryable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("recovery.sqlite3");
        let (session_id, attempt_id) = seed_attempt(database.clone(), false);

        let (engine, recovered_id, recovered) = open(database.clone()).expect("recover store");

        assert_eq!(recovered_id, session_id);
        let attempt = recovered.attempt(&attempt_id).expect("recovered attempt");
        assert_eq!(attempt.status(), AttemptStatus::Failed);
        assert_eq!(
            attempt
                .failure()
                .expect("safe recovery failure")
                .retry_advice(),
            RetryAdvice::Immediate
        );
        let settled_sequence = recovered.last_sequence();
        drop(engine);

        let (_, _, recovered_again) = open(database).expect("recover settled store");
        assert_eq!(recovered_again.last_sequence(), settled_sequence);
    }

    #[test]
    fn recovery_marks_dispatched_attempt_unknown_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("recovery.sqlite3");
        let (_, attempt_id) = seed_attempt(database.clone(), true);

        let (engine, _, recovered) = open(database.clone()).expect("recover store");

        assert_eq!(
            recovered
                .attempt(&attempt_id)
                .expect("recovered attempt")
                .status(),
            AttemptStatus::Unknown
        );
        let settled_sequence = recovered.last_sequence();
        drop(engine);

        let (_, _, recovered_again) = open(database).expect("recover settled store");
        assert_eq!(recovered_again.last_sequence(), settled_sequence);
    }

    #[test]
    fn recovery_fails_started_but_unbound_attempt_without_assuming_dispatch() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("recovery-unbound.sqlite3");
        let (_, attempt_id) = seed_attempt_with_dispatch(database.clone(), true, false);

        let (_, _, recovered) = open(database).expect("recover store");

        let attempt = recovered.attempt(&attempt_id).expect("recovered attempt");
        assert_eq!(attempt.status(), AttemptStatus::Failed);
        assert_eq!(attempt.turns_started(), 0);
        assert_eq!(
            attempt.failure().expect("recovery failure").code().as_str(),
            "interrupted_before_dispatch"
        );
    }

    #[test]
    fn recovery_marks_started_tool_unknown_and_preserves_resumable_attempt() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("tool-running-recovery.sqlite3");
        let (_, attempt_id, tool_call_id) =
            seed_tool_state(database.clone(), PermissionOutcome::Allow, true, true);

        let (engine, _, recovered) = open(database.clone()).expect("recover store");
        assert_eq!(
            recovered
                .attempt(&attempt_id)
                .expect("recovered attempt")
                .status(),
            AttemptStatus::AwaitingTools
        );
        assert_eq!(
            recovered
                .tool_call(&tool_call_id)
                .expect("recovered tool call")
                .status(),
            ToolCallStatus::Unknown
        );
        let settled_sequence = recovered.last_sequence();
        drop(engine);

        let (_, _, recovered_again) = open(database).expect("recover settled store");
        assert_eq!(recovered_again.last_sequence(), settled_sequence);
    }

    #[test]
    fn recovery_preserves_unanswered_permission_without_executing_the_tool() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("tool-ask-recovery.sqlite3");
        let (_, attempt_id, tool_call_id) =
            seed_tool_state(database.clone(), PermissionOutcome::Ask, false, true);

        let (_, _, recovered) = open(database).expect("recover store");
        assert_eq!(
            recovered
                .attempt(&attempt_id)
                .expect("recovered attempt")
                .status(),
            AttemptStatus::AwaitingTools
        );
        assert_eq!(
            recovered
                .tool_call(&tool_call_id)
                .expect("recovered tool call")
                .status(),
            ToolCallStatus::PermissionPending
        );
    }

    #[test]
    fn recovery_cancels_unanswered_permission_before_marking_parent_unknown() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("tool-ask-inflight-recovery.sqlite3");
        let (_, attempt_id, tool_call_id) =
            seed_tool_state(database.clone(), PermissionOutcome::Ask, false, false);

        let (engine, _, recovered) = open(database.clone()).expect("recover store");
        assert_eq!(
            recovered
                .tool_call(&tool_call_id)
                .expect("recovered tool call")
                .status(),
            ToolCallStatus::Cancelled
        );
        assert_eq!(
            recovered
                .attempt(&attempt_id)
                .expect("recovered attempt")
                .status(),
            AttemptStatus::Unknown
        );
        let settled_sequence = recovered.last_sequence();
        drop(engine);

        let (_, _, recovered_again) = open(database).expect("recover settled store");
        assert_eq!(recovered_again.last_sequence(), settled_sequence);
    }
}
