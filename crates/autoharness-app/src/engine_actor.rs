use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use autoharness_domain::{
    AttemptFailure, CommandEnvelope, CommandPayload, ErrorClass, ErrorCode, PublicMessage,
    RetryAdvice, SessionId,
};
use autoharness_engine::{
    AttemptStatus, DurableEngine, DurableEngineError, SessionAggregate, ToolCallStatus,
};
use autoharness_store::{SessionStatus, SessionStore};
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

enum Request {
    Execute {
        command: Box<CommandEnvelope>,
        reply: oneshot::Sender<Result<EngineReply, DurableEngineError>>,
    },
    Shutdown,
}

/// Cloneable asynchronous handle to the dedicated blocking storage thread.
#[derive(Clone)]
pub struct EngineHandle {
    requests: mpsc::Sender<Request>,
}

impl EngineHandle {
    /// Executes one command and resolves only after its event batch is durable.
    pub async fn execute(
        &self,
        command: CommandEnvelope,
    ) -> Result<EngineReply, DurableEngineError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request::Execute {
                command: Box::new(command),
                reply,
            })
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?;
        response
            .await
            .map_err(|_| DurableEngineError::StoreInvariant)?
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
        let (requests, receiver) = mpsc::channel(64);
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
        let _ = self.handle.requests.send(Request::Shutdown).await;
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
    mut receiver: mpsc::Receiver<Request>,
    ready: std::sync::mpsc::SyncSender<Result<(SessionId, SessionAggregate), AppError>>,
) {
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
            Request::Execute { command, reply } => {
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
            Request::Shutdown => break,
        }
    }
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
            .map(|attempt| (attempt.attempt_id().clone(), attempt.status()))
            .collect();
        for (attempt_id, status) in interrupted {
            let payload = match status {
                AttemptStatus::Prepared => CommandPayload::FailAttempt {
                    session_id: session_id.clone(),
                    attempt_id,
                    failure: interrupted_before_dispatch(),
                },
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
        DeliveryMode, InputId, ModelId, ModelRef, PermissionDecisionId, PermissionOutcome,
        PromptText, ProviderCallId, ProviderId, ToolArguments, ToolName,
    };
    use autoharness_tool::{IncomingToolCall, plan};

    use super::*;

    fn seed_attempt(database: PathBuf, start: bool) -> (SessionId, autoharness_domain::AttemptId) {
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
        }
        (session_id, attempt_id)
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
