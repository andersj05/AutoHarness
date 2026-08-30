//! Feature-gated Tauri carrier over the existing application coordinator ports.

use std::collections::BTreeSet;
use std::mem;
use std::sync::Arc;
use std::time::Duration;

use autoharness_client::{
    AttemptId as ClientAttemptId, AttemptState as ClientAttemptState, AuthenticationState,
    CapabilitySupport, CatalogProjection as ClientCatalogProjection, ClientCommand,
    ClientLifecycle, ClientNotice, ClientSnapshot, CommandEnvelope, CommandReceipt,
    ConnectionId as ClientConnectionId, CredentialSource as ClientCredentialSource, DecimalU64,
    FailureClass, InputId as ClientInputId, ModelId as ClientModelId, ModelRef as ClientModelRef,
    ModelSummary as ClientModelSummary, PermissionDecision, PermissionDetail,
    PermissionRequest as ClientPermissionRequest, ProviderId as ClientProviderId,
    ProviderProjection as ClientProviderProjection, ProviderStatus as ClientProviderStatus,
    RequestId as ClientRequestId, RetryDirective, SafeFailure, SecretIngress, ServerFrame,
    SessionId as ClientSessionId, SessionProjection as ClientSessionProjection, SessionRevision,
    SessionSummary, SessionTitle, ShutdownState, SnapshotReason, ToolCallId as ClientToolCallId,
    ToolCallProjection as ClientToolCallProjection, ToolCallState, TranscriptContent,
    TranscriptItem as ClientTranscriptItem, TransportRevision, UsageProjection,
};
use autoharness_domain::{
    ErrorClass, ModelId as DomainModelId, ModelRef as DomainModelRef,
    ProviderId as DomainProviderId,
};
use autoharness_tui::{
    ApiCredential, AttemptStatus as TuiAttemptStatus, CatalogProjection as TuiCatalogProjection,
    CredentialSourceLabel, ProfileConnectionState, ProfileCredentialStateLabel,
    ProfilesProjection as TuiProfilesProjection, ProviderKindLabel, RequestId as TuiRequestId,
    RetryPolicy as TuiRetryPolicy, SessionProjection as TuiSessionProjection,
    SessionsProjection as TuiSessionsProjection, SettingsProjection as TuiSettingsProjection,
    ToolCallKey, TranscriptItem as TuiTranscriptItem, UiFailure, UiIntent, UiNotice, UiPorts,
};
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

const HOST_REQUEST_CAPACITY: usize = 32;
const MAX_PENDING_REQUESTS: usize = HOST_REQUEST_CAPACITY;
const PROJECTION_FRAME_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Clone)]
struct GuiState {
    requests: mpsc::Sender<HostRequest>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct GuiIpcError {
    code: &'static str,
    message: &'static str,
}

impl GuiIpcError {
    const fn busy() -> Self {
        Self {
            code: "host_busy",
            message: "the application is busy; try again",
        }
    }

    const fn disconnected() -> Self {
        Self {
            code: "host_disconnected",
            message: "the application host is no longer available",
        }
    }

    const fn invalid_command() -> Self {
        Self {
            code: "invalid_command",
            message: "the command does not match current authoritative state",
        }
    }

    const fn invalid_projection() -> Self {
        Self {
            code: "invalid_projection",
            message: "the authoritative state could not be represented safely",
        }
    }

    const fn invalid_credential() -> Self {
        Self {
            code: "invalid_credential",
            message: "the credential is empty, too large, or targets an inactive provider",
        }
    }
}

enum HostRequest {
    Connect {
        channel: Channel<ServerFrame>,
        reply: oneshot::Sender<Result<(), GuiIpcError>>,
    },
    Dispatch {
        command: ClientCommand,
        reply: oneshot::Sender<Result<CommandReceipt, GuiIpcError>>,
    },
    Credential {
        ingress: SecretIngress,
        reply: oneshot::Sender<Result<CommandReceipt, GuiIpcError>>,
    },
}

#[tauri::command]
async fn gui_connect(
    state: tauri::State<'_, GuiState>,
    on_frame: Channel<ServerFrame>,
) -> Result<(), GuiIpcError> {
    let requests = state.requests.clone();
    let (reply, response) = oneshot::channel();
    requests
        .send(HostRequest::Connect {
            channel: on_frame,
            reply,
        })
        .await
        .map_err(|_| GuiIpcError::disconnected())?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

#[tauri::command]
async fn gui_dispatch(
    state: tauri::State<'_, GuiState>,
    command: CommandEnvelope,
) -> Result<CommandReceipt, GuiIpcError> {
    let requests = state.requests.clone();
    let (reply, response) = oneshot::channel();
    requests
        .send(HostRequest::Dispatch {
            command: command.command,
            reply,
        })
        .await
        .map_err(|_| GuiIpcError::disconnected())?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

#[tauri::command]
async fn gui_submit_credential(
    state: tauri::State<'_, GuiState>,
    connection_id: ClientConnectionId,
    credential: String,
) -> Result<CommandReceipt, GuiIpcError> {
    let ingress = SecretIngress::new(connection_id, credential)
        .map_err(|_| GuiIpcError::invalid_credential())?;
    let requests = state.requests.clone();
    let (reply, response) = oneshot::channel();
    requests
        .send(HostRequest::Credential { ingress, reply })
        .await
        .map_err(|_| GuiIpcError::disconnected())?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

pub(crate) async fn run(ui_ports: UiPorts, shutdown: CancellationToken) -> Result<(), AppError> {
    let (request_tx, request_rx) = mpsc::channel(HOST_REQUEST_CAPACITY);
    let bridge_shutdown = shutdown.clone();
    let bridge_task = tokio::spawn(async move {
        BridgeActor::new(ui_ports, request_rx, bridge_shutdown)
            .run()
            .await
    });

    let app = tauri::Builder::default()
        .manage(GuiState {
            requests: request_tx,
        })
        .invoke_handler(tauri::generate_handler![
            gui_connect,
            gui_dispatch,
            gui_submit_credential
        ])
        .build(tauri::generate_context!())
        .map_err(|_| AppError::Configuration)?;

    let handle = app.handle().clone();
    let exit_shutdown = shutdown.clone();
    let exit_task = tokio::spawn(async move {
        exit_shutdown.cancelled().await;
        handle.exit(0);
    });
    let event_shutdown = shutdown.clone();
    app.run(move |_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            event_shutdown.cancel();
        }
    });

    shutdown.cancel();
    exit_task.abort();
    match bridge_task.await {
        Ok(result) => result,
        Err(_) => Err(AppError::WorkerStopped),
    }
}

struct BridgeActor {
    intents: mpsc::Sender<UiIntent>,
    session: tokio::sync::watch::Receiver<Arc<TuiSessionProjection>>,
    session_list: tokio::sync::watch::Receiver<Arc<TuiSessionsProjection>>,
    catalog: tokio::sync::watch::Receiver<Arc<TuiCatalogProjection>>,
    profiles: tokio::sync::watch::Receiver<Arc<TuiProfilesProjection>>,
    settings: tokio::sync::watch::Receiver<Arc<TuiSettingsProjection>>,
    notices: mpsc::Receiver<UiNotice>,
    requests: mpsc::Receiver<HostRequest>,
    channel: Option<Channel<ServerFrame>>,
    last_snapshot: Option<ClientSnapshot>,
    next_request_id: u64,
    next_revision: TransportRevision,
    pending_requests: BTreeSet<u64>,
    catalog_generation: u64,
    projection_dirty: bool,
    shutdown_notice_sent: bool,
    shutdown: CancellationToken,
}

impl BridgeActor {
    fn new(
        ports: UiPorts,
        requests: mpsc::Receiver<HostRequest>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            intents: ports.intents,
            session: ports.sessions,
            session_list: ports.session_lists,
            catalog: ports.catalogs,
            profiles: ports.profiles,
            settings: ports.settings,
            notices: ports.notices,
            requests,
            channel: None,
            last_snapshot: None,
            next_request_id: 1,
            next_revision: TransportRevision::INITIAL,
            pending_requests: BTreeSet::new(),
            catalog_generation: 1,
            projection_dirty: true,
            shutdown_notice_sent: false,
            shutdown,
        }
    }

    async fn run(mut self) -> Result<(), AppError> {
        let mut frames = tokio::time::interval(PROJECTION_FRAME_INTERVAL);
        frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    self.emit_shutdown_notice();
                    return Ok(());
                }
                request = self.requests.recv() => {
                    let request = request.ok_or(AppError::WorkerStopped)?;
                    self.handle_request(request);
                }
                notice = self.notices.recv() => {
                    let notice = notice.ok_or(AppError::WorkerStopped)?;
                    self.handle_notice(notice)?;
                }
                result = self.session.changed() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.session.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.session_list.changed() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.session_list.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.catalog.changed() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.catalog.borrow_and_update();
                    self.catalog_generation = self.catalog_generation.saturating_add(1);
                    self.projection_dirty = true;
                }
                result = self.profiles.changed() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.profiles.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.settings.changed() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.settings.borrow_and_update();
                    self.projection_dirty = true;
                }
                _ = frames.tick(), if self.projection_dirty => {
                    self.publish_projection();
                }
            }
        }
    }

    fn handle_request(&mut self, request: HostRequest) {
        match request {
            HostRequest::Connect { channel, reply } => {
                self.channel = Some(channel);
                let result = self.snapshot().and_then(|snapshot| {
                    self.emit_snapshot(SnapshotReason::Initial, snapshot.clone())?;
                    self.last_snapshot = Some(snapshot.clone());
                    self.projection_dirty = false;
                    Ok(())
                });
                if result.is_err() {
                    self.channel = None;
                }
                let _ = reply.send(result);
            }
            HostRequest::Dispatch { command, reply } => {
                let result = self.dispatch(command);
                let _ = reply.send(result);
            }
            HostRequest::Credential { ingress, reply } => {
                let result = self.dispatch_credential(ingress);
                let _ = reply.send(result);
            }
        }
    }

    fn dispatch(&mut self, command: ClientCommand) -> Result<CommandReceipt, GuiIpcError> {
        let request_id = self.issue_request_id()?;
        let action = map_command(command, request_id, &self.session.borrow())?;
        match action {
            CommandAction::Intent(intent) => self.admit_intent(request_id, intent)?,
            CommandAction::Resynchronize => {
                let snapshot = self.snapshot()?;
                self.emit_snapshot(SnapshotReason::Resynchronization, snapshot.clone())?;
                self.last_snapshot = Some(snapshot);
                self.projection_dirty = false;
                self.emit_notice(ClientNotice::CommandCommitted { request_id })?;
            }
            CommandAction::Shutdown => {
                self.emit_notice(ClientNotice::CommandCommitted { request_id })?;
                self.emit_shutdown_notice();
                self.shutdown.cancel();
            }
        }
        Ok(CommandReceipt::new(request_id))
    }

    fn dispatch_credential(
        &mut self,
        ingress: SecretIngress,
    ) -> Result<CommandReceipt, GuiIpcError> {
        let snapshot = self.snapshot()?;
        let targets_active_connection =
            credential_targets_active_connection(&snapshot.providers, ingress.connection_id());
        if !targets_active_connection {
            return Err(GuiIpcError::invalid_credential());
        }
        let request_id = self.issue_request_id()?;
        let mut credential = ingress.into_credential();
        let credential = ApiCredential::new(mem::take(&mut *credential))
            .map_err(|_| GuiIpcError::invalid_credential())?;
        self.admit_intent(
            request_id,
            UiIntent::ConfigureCredential {
                request_id: TuiRequestId::new(request_id.get()),
                credential,
            },
        )?;
        Ok(CommandReceipt::new(request_id))
    }

    fn admit_intent(
        &mut self,
        request_id: ClientRequestId,
        intent: UiIntent,
    ) -> Result<(), GuiIpcError> {
        if self.pending_requests.len() >= MAX_PENDING_REQUESTS {
            return Err(GuiIpcError::busy());
        }
        self.send_intent(intent)?;
        self.pending_requests.insert(request_id.get());
        Ok(())
    }

    fn handle_notice(&mut self, notice: UiNotice) -> Result<(), AppError> {
        let Ok(notice) = map_notice(notice) else {
            return Ok(());
        };
        let terminal_request_id = match &notice {
            ClientNotice::CommandCommitted { request_id }
            | ClientNotice::CommandRejected { request_id, .. } => Some(request_id.get()),
            ClientNotice::Authentication { .. } | ClientNotice::Shutdown { .. } => None,
        };
        if terminal_request_id.is_some_and(|request_id| !self.pending_requests.remove(&request_id))
        {
            return Ok(());
        }
        self.observe_pending_projections()?;
        if self.projection_dirty {
            self.publish_projection();
        }
        let _ = self.emit_notice(notice);
        Ok(())
    }

    fn observe_pending_projections(&mut self) -> Result<(), AppError> {
        if self
            .session
            .has_changed()
            .map_err(|_| AppError::WorkerStopped)?
        {
            self.session.borrow_and_update();
            self.projection_dirty = true;
        }
        if self
            .session_list
            .has_changed()
            .map_err(|_| AppError::WorkerStopped)?
        {
            self.session_list.borrow_and_update();
            self.projection_dirty = true;
        }
        if self
            .catalog
            .has_changed()
            .map_err(|_| AppError::WorkerStopped)?
        {
            self.catalog.borrow_and_update();
            self.catalog_generation = self.catalog_generation.saturating_add(1);
            self.projection_dirty = true;
        }
        if self
            .profiles
            .has_changed()
            .map_err(|_| AppError::WorkerStopped)?
        {
            self.profiles.borrow_and_update();
            self.projection_dirty = true;
        }
        if self
            .settings
            .has_changed()
            .map_err(|_| AppError::WorkerStopped)?
        {
            self.settings.borrow_and_update();
            self.projection_dirty = true;
        }
        Ok(())
    }

    fn emit_shutdown_notice(&mut self) {
        if self.shutdown_notice_sent {
            return;
        }
        if self
            .emit_notice(ClientNotice::Shutdown {
                state: ShutdownState::Requested,
            })
            .is_ok()
        {
            self.shutdown_notice_sent = true;
        }
    }

    fn send_intent(&self, intent: UiIntent) -> Result<(), GuiIpcError> {
        match self.intents.try_send(intent) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(GuiIpcError::busy()),
            Err(TrySendError::Closed(_)) => Err(GuiIpcError::disconnected()),
        }
    }

    fn issue_request_id(&mut self) -> Result<ClientRequestId, GuiIpcError> {
        let value = self.next_request_id;
        self.next_request_id = value.checked_add(1).ok_or_else(GuiIpcError::disconnected)?;
        ClientRequestId::new(value).map_err(|_| GuiIpcError::disconnected())
    }

    fn publish_projection(&mut self) {
        let Ok(snapshot) = self.snapshot() else {
            return;
        };
        self.projection_dirty = false;
        if self.last_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        if self.channel.is_none() {
            self.last_snapshot = Some(snapshot);
            return;
        }
        if self
            .emit_snapshot(SnapshotReason::Projection, snapshot.clone())
            .is_ok()
        {
            self.last_snapshot = Some(snapshot);
        }
    }

    fn snapshot(&self) -> Result<ClientSnapshot, GuiIpcError> {
        map_snapshot(
            &self.session.borrow(),
            &self.session_list.borrow(),
            &self.catalog.borrow(),
            &self.profiles.borrow(),
            &self.settings.borrow(),
            self.catalog_generation,
            self.shutdown.is_cancelled(),
        )
    }

    fn emit_snapshot(
        &mut self,
        reason: SnapshotReason,
        snapshot: ClientSnapshot,
    ) -> Result<(), GuiIpcError> {
        let revision = self.take_revision()?;
        self.emit(ServerFrame::snapshot(revision, reason, snapshot))
    }

    fn emit_notice(&mut self, notice: ClientNotice) -> Result<(), GuiIpcError> {
        let revision = self.take_revision()?;
        self.emit(ServerFrame::notice(revision, notice))
    }

    fn take_revision(&mut self) -> Result<TransportRevision, GuiIpcError> {
        let revision = self.next_revision;
        self.next_revision = revision.next().map_err(|_| GuiIpcError::disconnected())?;
        Ok(revision)
    }

    fn emit(&mut self, frame: ServerFrame) -> Result<(), GuiIpcError> {
        let channel = self.channel.clone().ok_or_else(GuiIpcError::disconnected)?;
        if channel.send(frame).is_err() {
            self.channel = None;
            return Err(GuiIpcError::disconnected());
        }
        Ok(())
    }
}

enum CommandAction {
    Intent(UiIntent),
    Resynchronize,
    Shutdown,
}

fn map_command(
    command: ClientCommand,
    request_id: ClientRequestId,
    active_session: &TuiSessionProjection,
) -> Result<CommandAction, GuiIpcError> {
    let request_id = TuiRequestId::new(request_id.get());
    let action = match command {
        ClientCommand::CreateSession => {
            CommandAction::Intent(UiIntent::CreateSession { request_id })
        }
        ClientCommand::OpenSession { session_id } => CommandAction::Intent(UiIntent::OpenSession {
            request_id,
            session_id: session_id.into_inner(),
        }),
        ClientCommand::RefreshCatalog => {
            CommandAction::Intent(UiIntent::RefreshCatalog { request_id })
        }
        ClientCommand::SelectModel { session_id, model } => {
            require_active_session(&session_id, active_session)?;
            CommandAction::Intent(UiIntent::SelectModel {
                request_id,
                model: domain_model_ref(model)?,
            })
        }
        ClientCommand::SubmitPrompt { session_id, prompt } => {
            require_active_session(&session_id, active_session)?;
            CommandAction::Intent(UiIntent::SubmitPrompt {
                request_id,
                prompt: prompt.into_inner(),
            })
        }
        ClientCommand::CancelAttempt {
            session_id,
            attempt_id,
        } => {
            require_active_session(&session_id, active_session)?;
            CommandAction::Intent(UiIntent::CancelAttempt {
                request_id,
                attempt_id: autoharness_tui::AttemptKey::new(attempt_id.into_inner())
                    .map_err(|_| GuiIpcError::invalid_command())?,
            })
        }
        ClientCommand::RetryAttempt {
            session_id,
            attempt_id,
        } => {
            require_active_session(&session_id, active_session)?;
            CommandAction::Intent(UiIntent::RetryAttempt {
                request_id,
                attempt_id: autoharness_tui::AttemptKey::new(attempt_id.into_inner())
                    .map_err(|_| GuiIpcError::invalid_command())?,
            })
        }
        ClientCommand::AnswerPermission {
            session_id,
            tool_call_id,
            decision,
        } => {
            require_active_session(&session_id, active_session)?;
            if !active_session
                .permission_requests
                .iter()
                .any(|request| request.tool_call_id.as_str() == tool_call_id.as_str())
            {
                return Err(GuiIpcError::invalid_command());
            }
            CommandAction::Intent(UiIntent::AnswerPermission {
                request_id,
                tool_call_id: ToolCallKey::new(tool_call_id.into_inner())
                    .map_err(|_| GuiIpcError::invalid_command())?,
                allow: decision == PermissionDecision::AllowOnce,
            })
        }
        // A full baseline is intentionally unconditional. Even a missing,
        // stale, or impossible client cursor must be repairable without
        // trusting renderer state.
        ClientCommand::RequestResynchronization { .. } => CommandAction::Resynchronize,
        ClientCommand::RequestShutdown => CommandAction::Shutdown,
    };
    Ok(action)
}

fn require_active_session(
    requested: &ClientSessionId,
    active: &TuiSessionProjection,
) -> Result<(), GuiIpcError> {
    if requested.as_str() == active.session_id {
        Ok(())
    } else {
        Err(GuiIpcError::invalid_command())
    }
}

fn domain_model_ref(model: ClientModelRef) -> Result<DomainModelRef, GuiIpcError> {
    let provider = DomainProviderId::new(model.provider_id.into_inner())
        .map_err(|_| GuiIpcError::invalid_command())?;
    let model = DomainModelId::new(model.model_id.into_inner())
        .map_err(|_| GuiIpcError::invalid_command())?;
    Ok(DomainModelRef::new(provider, model))
}

fn map_notice(notice: UiNotice) -> Result<ClientNotice, GuiIpcError> {
    match notice {
        UiNotice::IntentCommitted { request_id } => Ok(ClientNotice::CommandCommitted {
            request_id: client_request_id(request_id)?,
        }),
        UiNotice::IntentRejected {
            request_id,
            failure,
        } => Ok(ClientNotice::CommandRejected {
            request_id: client_request_id(request_id)?,
            failure: map_failure(&failure)?,
        }),
        UiNotice::CodexLoginBrowserOpened { request_id } => Ok(ClientNotice::Authentication {
            request_id: client_request_id(request_id)?,
            state: AuthenticationState::BrowserOpened,
        }),
        UiNotice::CodexLoginCompleted { request_id } => Ok(ClientNotice::Authentication {
            request_id: client_request_id(request_id)?,
            state: AuthenticationState::Completed,
        }),
    }
}

fn client_request_id(request_id: TuiRequestId) -> Result<ClientRequestId, GuiIpcError> {
    ClientRequestId::new(request_id.get()).map_err(|_| GuiIpcError::invalid_projection())
}

fn map_snapshot(
    active: &TuiSessionProjection,
    session_list: &TuiSessionsProjection,
    catalog: &TuiCatalogProjection,
    profiles: &TuiProfilesProjection,
    settings: &TuiSettingsProjection,
    catalog_generation: u64,
    shutting_down: bool,
) -> Result<ClientSnapshot, GuiIpcError> {
    let active_session = map_session(active)?;
    let active_id = active_session.session_id.clone();
    let mut sessions = session_list
        .sessions
        .iter()
        .map(|session| map_session_summary(session, active))
        .collect::<Result<Vec<_>, _>>()?;
    if !sessions
        .iter()
        .any(|session| session.session_id == active_id)
    {
        let title =
            SessionTitle::new("New session").map_err(|_| GuiIpcError::invalid_projection())?;
        sessions.insert(
            0,
            SessionSummary::new(
                active_id.clone(),
                title,
                SessionRevision::new(active.revision),
                active_session.selected_model.clone(),
                None,
                None,
                false,
            ),
        );
    }
    let catalog = map_catalog(catalog, catalog_generation)?;
    let providers = map_providers(profiles, settings, &catalog)?;
    let lifecycle = if shutting_down {
        ClientLifecycle::ShuttingDown
    } else if providers
        .iter()
        .any(|provider| provider.active && matches!(&provider.status, ClientProviderStatus::Ready))
    {
        ClientLifecycle::Ready
    } else {
        match &catalog {
            ClientCatalogProjection::Loading => ClientLifecycle::Starting,
            ClientCatalogProjection::Ready { .. }
            | ClientCatalogProjection::CredentialRequired
            | ClientCatalogProjection::Failed { .. } => ClientLifecycle::Offline,
        }
    };
    ClientSnapshot::new(
        lifecycle,
        Some(active_id),
        sessions,
        Some(active_session),
        catalog,
        providers,
    )
    .map_err(|_| GuiIpcError::invalid_projection())
}

fn map_session(active: &TuiSessionProjection) -> Result<ClientSessionProjection, GuiIpcError> {
    let session_id = ClientSessionId::new(active.session_id.clone())
        .map_err(|_| GuiIpcError::invalid_projection())?;
    let selected_model = active
        .selected_model
        .as_ref()
        .map(client_model_ref)
        .transpose()?;
    let transcript = active
        .transcript
        .iter()
        .map(map_transcript_item)
        .collect::<Result<Vec<_>, _>>()?;
    let permission_requests = active
        .permission_requests
        .iter()
        .map(|request| {
            let details = request
                .details
                .iter()
                .map(|detail| {
                    PermissionDetail::new(detail.label.clone(), detail.value.clone())
                        .map_err(|_| GuiIpcError::invalid_projection())
                })
                .collect::<Result<Vec<_>, _>>()?;
            ClientPermissionRequest::new(
                ClientToolCallId::new(request.tool_call_id.as_str())
                    .map_err(|_| GuiIpcError::invalid_projection())?,
                request.tool_name.clone(),
                request.capability.clone(),
                request.resource.clone(),
                details,
            )
            .map_err(|_| GuiIpcError::invalid_projection())
        })
        .collect::<Result<Vec<_>, _>>()?;
    ClientSessionProjection::new(
        session_id,
        SessionRevision::new(active.revision),
        selected_model,
        transcript,
        permission_requests,
    )
    .map_err(|_| GuiIpcError::invalid_projection())
}

fn map_transcript_item(item: &TuiTranscriptItem) -> Result<ClientTranscriptItem, GuiIpcError> {
    match item {
        TuiTranscriptItem::User { input_id, text } => Ok(ClientTranscriptItem::User {
            input_id: ClientInputId::new(input_id.clone())
                .map_err(|_| GuiIpcError::invalid_projection())?,
            content: TranscriptContent::new(text.clone())
                .map_err(|_| GuiIpcError::invalid_projection())?,
        }),
        TuiTranscriptItem::Assistant {
            attempt_id,
            text,
            status,
            usage,
            retry_of,
        } => Ok(ClientTranscriptItem::Assistant {
            attempt_id: ClientAttemptId::new(attempt_id.as_str())
                .map_err(|_| GuiIpcError::invalid_projection())?,
            content: TranscriptContent::new(text.clone())
                .map_err(|_| GuiIpcError::invalid_projection())?,
            state: match status {
                TuiAttemptStatus::Streaming => ClientAttemptState::Streaming,
                TuiAttemptStatus::Cancelling => ClientAttemptState::Cancelling,
                TuiAttemptStatus::Completed => ClientAttemptState::Completed,
                TuiAttemptStatus::Cancelled => ClientAttemptState::Cancelled,
                TuiAttemptStatus::Failed(failure) => ClientAttemptState::Failed {
                    failure: map_failure(failure)?,
                },
            },
            usage: usage.map(|usage| UsageProjection {
                input_tokens: Some(DecimalU64::new(usage.input_tokens)),
                output_tokens: Some(DecimalU64::new(usage.output_tokens)),
                total_tokens: usage
                    .input_tokens
                    .checked_add(usage.output_tokens)
                    .map(DecimalU64::new),
                ..UsageProjection::default()
            }),
            retry_of: retry_of
                .as_ref()
                .map(|attempt| {
                    ClientAttemptId::new(attempt.as_str())
                        .map_err(|_| GuiIpcError::invalid_projection())
                })
                .transpose()?,
        }),
        TuiTranscriptItem::Tool(tool) => {
            let state = match tool.status.as_str() {
                "proposed" => ToolCallState::Proposed,
                "permission pending" => ToolCallState::PermissionPending,
                "authorized" => ToolCallState::Authorized,
                "denying" => ToolCallState::Denying,
                "running" => ToolCallState::Running,
                "completed" => ToolCallState::Completed,
                "failed" => ToolCallState::Failed {
                    failure: SafeFailure::new(
                        FailureClass::Internal,
                        "tool_failed",
                        tool.summary.as_deref().unwrap_or("tool execution failed"),
                        RetryDirective::Never,
                    )
                    .map_err(|_| GuiIpcError::invalid_projection())?,
                },
                "denied" => ToolCallState::Denied,
                "cancelled" => ToolCallState::Cancelled,
                _ => ToolCallState::Unknown,
            };
            Ok(ClientTranscriptItem::Tool(
                ClientToolCallProjection::new(
                    ClientToolCallId::new(tool.tool_call_id.as_str())
                        .map_err(|_| GuiIpcError::invalid_projection())?,
                    tool.tool_name.clone(),
                    "tool execution",
                    tool.resource.clone(),
                    state,
                    tool.summary.clone(),
                )
                .map_err(|_| GuiIpcError::invalid_projection())?,
            ))
        }
    }
}

fn map_session_summary(
    summary: &autoharness_tui::SessionBrowserEntry,
    active: &TuiSessionProjection,
) -> Result<SessionSummary, GuiIpcError> {
    let session_id = ClientSessionId::new(summary.session_id.clone())
        .map_err(|_| GuiIpcError::invalid_projection())?;
    let title = SessionTitle::new(if summary.title.trim().is_empty() {
        "Untitled session".to_owned()
    } else {
        summary.title.clone()
    })
    .map_err(|_| GuiIpcError::invalid_projection())?;
    let revision = if summary.session_id == active.session_id {
        active.revision
    } else {
        0
    };
    Ok(SessionSummary::new(
        session_id,
        title,
        SessionRevision::new(revision),
        summary
            .selected_model
            .as_ref()
            .map(client_model_ref)
            .transpose()?,
        Some(summary.updated_at_ms),
        Some(summary.message_count),
        summary.archived,
    ))
}

fn map_catalog(
    catalog: &TuiCatalogProjection,
    generation: u64,
) -> Result<ClientCatalogProjection, GuiIpcError> {
    match catalog {
        TuiCatalogProjection::CredentialRequired => Ok(ClientCatalogProjection::CredentialRequired),
        TuiCatalogProjection::Loading => Ok(ClientCatalogProjection::Loading),
        TuiCatalogProjection::Failed(failure) => Ok(ClientCatalogProjection::Failed {
            failure: map_failure(failure)?,
        }),
        TuiCatalogProjection::Ready { models, stale } => {
            let models = models
                .iter()
                .map(|model| {
                    ClientModelSummary::new(
                        client_model_ref(&model.model)?,
                        model.display_name.clone(),
                        model.detail.clone(),
                        model.context_window_tokens,
                        model.selectable,
                        if model.selectable {
                            CapabilitySupport::Supported
                        } else {
                            CapabilitySupport::Unsupported
                        },
                        if model.selectable {
                            CapabilitySupport::Supported
                        } else {
                            CapabilitySupport::Unsupported
                        },
                        CapabilitySupport::Unknown,
                        CapabilitySupport::Unknown,
                    )
                    .map_err(|_| GuiIpcError::invalid_projection())
                })
                .collect::<Result<Vec<_>, GuiIpcError>>()?;
            ClientCatalogProjection::ready(generation, models, *stale)
                .map_err(|_| GuiIpcError::invalid_projection())
        }
    }
}

fn map_providers(
    profiles: &TuiProfilesProjection,
    settings: &TuiSettingsProjection,
    catalog: &ClientCatalogProjection,
) -> Result<Vec<ClientProviderProjection>, GuiIpcError> {
    if profiles.profiles.is_empty() {
        return Ok(vec![fallback_provider(settings, catalog)?]);
    }
    profiles
        .profiles
        .iter()
        .map(|profile| {
            let connected = profile.active && settings.provider_status.credential_connected;
            let credential_source = map_credential_source(profile.credential_source, connected);
            let status = match &profile.connection {
                ProfileConnectionState::Testing => ClientProviderStatus::Connecting,
                ProfileConnectionState::Ready if connected => ClientProviderStatus::Ready,
                ProfileConnectionState::Ready
                    if profile.credential_state == ProfileCredentialStateLabel::Disconnected =>
                {
                    ClientProviderStatus::CredentialRequired
                }
                ProfileConnectionState::Ready => ClientProviderStatus::Offline,
                ProfileConnectionState::Failed(message) => ClientProviderStatus::Failed {
                    failure: SafeFailure::new(
                        FailureClass::Unavailable,
                        "provider_connection_failed",
                        message.clone(),
                        RetryDirective::Immediate,
                    )
                    .map_err(|_| GuiIpcError::invalid_projection())?,
                },
                ProfileConnectionState::Untested if connected => ClientProviderStatus::Ready,
                ProfileConnectionState::Untested
                    if profile.credential_state == ProfileCredentialStateLabel::Disconnected =>
                {
                    ClientProviderStatus::CredentialRequired
                }
                ProfileConnectionState::Untested => ClientProviderStatus::Offline,
            };
            let display_name = format!("{} - {}", profile.kind.as_str(), profile.id);
            ClientProviderProjection::new(
                ClientConnectionId::new(profile.id.clone())
                    .map_err(|_| GuiIpcError::invalid_projection())?,
                profile_provider_id(profile, catalog)?,
                display_name,
                profile.active,
                status,
                credential_source,
                None,
            )
            .map_err(|_| GuiIpcError::invalid_projection())
        })
        .collect()
}

fn fallback_provider(
    settings: &TuiSettingsProjection,
    catalog: &ClientCatalogProjection,
) -> Result<ClientProviderProjection, GuiIpcError> {
    let kind = settings
        .provider_status
        .provider_kind
        .unwrap_or(ProviderKindLabel::Gemini);
    let connected = settings.provider_status.credential_connected;
    let status = match catalog {
        ClientCatalogProjection::CredentialRequired => ClientProviderStatus::CredentialRequired,
        ClientCatalogProjection::Loading if connected => ClientProviderStatus::Connecting,
        ClientCatalogProjection::Loading => ClientProviderStatus::Disconnected,
        ClientCatalogProjection::Ready { .. } if connected => ClientProviderStatus::Ready,
        ClientCatalogProjection::Ready { .. } => ClientProviderStatus::CredentialRequired,
        ClientCatalogProjection::Failed { failure } => ClientProviderStatus::Failed {
            failure: failure.clone(),
        },
    };
    let connection_id = settings
        .provider_status
        .active_profile
        .clone()
        .unwrap_or_else(|| format!("session:{}", provider_kind_id(kind)));
    ClientProviderProjection::new(
        ClientConnectionId::new(connection_id).map_err(|_| GuiIpcError::invalid_projection())?,
        catalog_provider_id(catalog, kind).unwrap_or(
            ClientProviderId::new(provider_kind_id(kind))
                .map_err(|_| GuiIpcError::invalid_projection())?,
        ),
        kind.as_str(),
        true,
        status,
        map_credential_source(settings.provider_status.credential_source, connected),
        None,
    )
    .map_err(|_| GuiIpcError::invalid_projection())
}

const fn provider_kind_id(kind: ProviderKindLabel) -> &'static str {
    match kind {
        ProviderKindLabel::Gemini => "gemini",
        ProviderKindLabel::Router => "router",
        ProviderKindLabel::CodexCli => "codex-cli",
    }
}

fn profile_provider_id(
    profile: &autoharness_tui::ProviderProfileProjection,
    catalog: &ClientCatalogProjection,
) -> Result<ClientProviderId, GuiIpcError> {
    if profile.active
        && let Some(provider_id) = catalog_provider_id(catalog, profile.kind)
    {
        return Ok(provider_id);
    }
    let provider_id = match profile.kind {
        ProviderKindLabel::Router if !profile.project.trim().is_empty() => {
            format!("router:{}", profile.project.trim())
        }
        kind => provider_kind_id(kind).to_owned(),
    };
    ClientProviderId::new(provider_id).map_err(|_| GuiIpcError::invalid_projection())
}

fn catalog_provider_id(
    catalog: &ClientCatalogProjection,
    kind: ProviderKindLabel,
) -> Option<ClientProviderId> {
    match catalog {
        ClientCatalogProjection::Ready { models, .. } => models
            .first()
            .map(|model| model.model.provider_id.clone())
            .filter(|provider_id| provider_id_matches_kind(provider_id, kind)),
        ClientCatalogProjection::CredentialRequired
        | ClientCatalogProjection::Loading
        | ClientCatalogProjection::Failed { .. } => None,
    }
}

fn provider_id_matches_kind(provider_id: &ClientProviderId, kind: ProviderKindLabel) -> bool {
    match kind {
        ProviderKindLabel::Gemini => provider_id.as_str() == "gemini",
        ProviderKindLabel::Router => provider_id.as_str().starts_with("router:"),
        ProviderKindLabel::CodexCli => provider_id.as_str() == "codex-cli",
    }
}

const fn map_credential_source(
    source: CredentialSourceLabel,
    connected: bool,
) -> ClientCredentialSource {
    if !connected {
        return ClientCredentialSource::None;
    }
    match source {
        CredentialSourceLabel::Environment => ClientCredentialSource::Environment,
        CredentialSourceLabel::CredentialVault => ClientCredentialSource::Vault,
        CredentialSourceLabel::SessionOnly => ClientCredentialSource::SessionOnly,
    }
}

fn client_model_ref(model: &DomainModelRef) -> Result<ClientModelRef, GuiIpcError> {
    Ok(ClientModelRef::new(
        ClientProviderId::new(model.provider_id().as_str())
            .map_err(|_| GuiIpcError::invalid_projection())?,
        ClientModelId::new(model.model_id().as_str())
            .map_err(|_| GuiIpcError::invalid_projection())?,
    ))
}

fn credential_targets_active_connection(
    providers: &[ClientProviderProjection],
    connection_id: &ClientConnectionId,
) -> bool {
    providers
        .iter()
        .any(|provider| provider.active && provider.connection_id == *connection_id)
}

fn map_failure(failure: &UiFailure) -> Result<SafeFailure, GuiIpcError> {
    let class = match failure.class {
        ErrorClass::Validation => FailureClass::Validation,
        ErrorClass::NotFound => FailureClass::NotFound,
        ErrorClass::Conflict => FailureClass::Conflict,
        ErrorClass::Authentication => FailureClass::Authentication,
        ErrorClass::PermissionDenied => FailureClass::PermissionDenied,
        ErrorClass::RateLimited => FailureClass::RateLimited,
        ErrorClass::Timeout => FailureClass::Timeout,
        ErrorClass::Unavailable => FailureClass::Unavailable,
        ErrorClass::Cancelled => FailureClass::Cancelled,
        ErrorClass::Protocol => FailureClass::Protocol,
        ErrorClass::Storage => FailureClass::Storage,
        ErrorClass::Internal => FailureClass::Internal,
    };
    let retry = match failure.retry {
        TuiRetryPolicy::Never => RetryDirective::Never,
        TuiRetryPolicy::Now => RetryDirective::Immediate,
        // `At` is a TUI-process monotonic deadline that cannot be transported
        // safely. Never retrying early is the conservative renderer-neutral
        // representation until the coordinator exposes a relative duration.
        TuiRetryPolicy::At(_) => RetryDirective::Never,
        TuiRetryPolicy::After { delay_ms } => {
            RetryDirective::after(delay_ms).unwrap_or(RetryDirective::Never)
        }
    };
    SafeFailure::new(class, failure.code.clone(), failure.message.clone(), retry)
        .map_err(|_| GuiIpcError::invalid_projection())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn active_session() -> TuiSessionProjection {
        TuiSessionProjection {
            session_id: "session-one".to_owned(),
            revision: 1,
            selected_model: None,
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }
    }

    #[test]
    fn gui_is_opt_in_and_uses_string_safe_protocol_ids() {
        let request_id = ClientRequestId::new(u64::MAX).expect("positive request ID");
        let receipt = CommandReceipt::new(request_id);
        let encoded = serde_json::to_value(receipt).expect("serialize receipt");

        assert_eq!(encoded["request_id"], u64::MAX.to_string());
    }

    #[test]
    fn permission_mapping_requires_the_exact_pending_tool_call() {
        let session = active_session();
        let command = ClientCommand::AnswerPermission {
            session_id: ClientSessionId::new("session-one").expect("session ID"),
            tool_call_id: ClientToolCallId::new("missing-call").expect("tool call ID"),
            decision: PermissionDecision::AllowOnce,
        };

        assert!(
            map_command(
                command,
                ClientRequestId::new(1).expect("request ID"),
                &session,
            )
            .is_err()
        );
    }

    #[test]
    fn secret_ingress_is_not_part_of_the_serializable_command_enum() {
        let command = CommandEnvelope::new(ClientCommand::RefreshCatalog);
        let encoded = serde_json::to_string(&command).expect("serialize public command");

        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("secret"));
    }

    #[test]
    fn cached_catalog_without_a_live_credential_is_an_offline_snapshot() {
        let snapshot = map_snapshot(
            &active_session(),
            &TuiSessionsProjection::default(),
            &TuiCatalogProjection::Ready {
                models: Vec::new(),
                stale: true,
            },
            &TuiProfilesProjection::default(),
            &TuiSettingsProjection::default(),
            7,
            false,
        )
        .expect("cached snapshot");

        assert!(matches!(snapshot.lifecycle, ClientLifecycle::Offline));
        assert!(matches!(
            snapshot.providers[0].status,
            ClientProviderStatus::CredentialRequired
        ));
        assert_eq!(
            snapshot.providers[0].credential_source,
            ClientCredentialSource::None
        );
        assert_eq!(snapshot.sessions[0].updated_at_ms, None);
        assert_eq!(snapshot.sessions[0].message_count, None);
    }

    #[test]
    fn inactive_ready_profile_never_claims_a_live_connection() {
        let profile = autoharness_tui::ProviderProfileProjection {
            id: "backup".to_owned(),
            kind: ProviderKindLabel::Gemini,
            active: false,
            base_url: String::new(),
            project: String::new(),
            auth_header: String::new(),
            credential_state: ProfileCredentialStateLabel::Stored,
            credential_source: CredentialSourceLabel::CredentialVault,
            connection: ProfileConnectionState::Ready,
            default_model: None,
            default_mode: "auto".to_owned(),
        };
        let providers = map_providers(
            &TuiProfilesProjection {
                profiles: vec![profile],
                ..TuiProfilesProjection::default()
            },
            &TuiSettingsProjection::default(),
            &ClientCatalogProjection::Loading,
        )
        .expect("provider projection");

        assert!(matches!(providers[0].status, ClientProviderStatus::Offline));
        assert_eq!(providers[0].credential_source, ClientCredentialSource::None);
        assert_eq!(providers[0].connection_id.as_str(), "backup");
        assert_eq!(providers[0].provider_id.as_str(), "gemini");
    }

    #[test]
    fn stale_catalog_cannot_relabel_a_different_provider_kind() {
        let router_model = ClientModelSummary::new(
            ClientModelRef::new(
                ClientProviderId::new("router:old").expect("provider ID"),
                ClientModelId::new("old-model").expect("model ID"),
            ),
            "Old router model",
            "stale cache",
            None,
            true,
            CapabilitySupport::Supported,
            CapabilitySupport::Supported,
            CapabilitySupport::Unknown,
            CapabilitySupport::Unknown,
        )
        .expect("model summary");
        let catalog = ClientCatalogProjection::ready(1, vec![router_model], true)
            .expect("catalog projection");

        let provider = fallback_provider(&TuiSettingsProjection::default(), &catalog)
            .expect("fallback provider");

        assert_eq!(provider.provider_id.as_str(), "gemini");
    }

    #[test]
    fn connect_and_resynchronization_share_one_ordered_channel() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = BridgeActor::new(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            let frame = body
                .deserialize::<ServerFrame>()
                .expect("serialized server frame");
            channel_frames.lock().expect("frame lock").push(frame);
            Ok(())
        });

        let (connect_reply, connect_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel,
            reply: connect_reply,
        });
        connect_response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");
        let receipt = bridge
            .dispatch(ClientCommand::RequestResynchronization {
                last_applied_revision: Some(TransportRevision::INITIAL),
            })
            .expect("resynchronization request");

        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].revision.get(), 1);
        assert_eq!(frames[1].revision.get(), 2);
        assert_eq!(frames[2].revision.get(), 3);
        assert!(matches!(
            frames[0].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Initial,
                ..
            }
        ));
        assert!(matches!(
            frames[1].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Resynchronization,
                ..
            }
        ));
        assert!(matches!(
            frames[0].classify_after(None),
            autoharness_client::FrameDisposition::Baseline
        ));
        assert!(matches!(
            frames[1].classify_after(Some(frames[0].revision)),
            autoharness_client::FrameDisposition::Baseline
        ));
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
    }

    #[test]
    fn connect_reports_a_channel_that_cannot_accept_its_baseline() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = BridgeActor::new(ui_ports, request_rx, CancellationToken::new());
        let channel = Channel::new(|_| Err(std::io::Error::other("test channel is closed").into()));
        let (connect_reply, connect_response) = oneshot::channel();

        bridge.handle_request(HostRequest::Connect {
            channel,
            reply: connect_reply,
        });

        assert!(
            connect_response
                .blocking_recv()
                .expect("connect response")
                .is_err()
        );
        assert!(bridge.channel.is_none());
    }

    #[test]
    fn projection_precedes_one_correlated_terminal_notice() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = BridgeActor::new(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            let frame = body
                .deserialize::<ServerFrame>()
                .expect("serialized server frame");
            channel_frames.lock().expect("frame lock").push(frame);
            Ok(())
        });
        let (connect_reply, connect_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel,
            reply: connect_reply,
        });
        connect_response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");

        let receipt = bridge
            .dispatch(ClientCommand::RefreshCatalog)
            .expect("admitted command");
        app_ports
            .catalogs
            .send(Arc::new(TuiCatalogProjection::Loading))
            .expect("catalog projection");
        let notice = UiNotice::IntentCommitted {
            request_id: TuiRequestId::new(receipt.request_id.get()),
        };
        bridge
            .handle_notice(notice.clone())
            .expect("terminal notice");
        bridge
            .handle_notice(notice)
            .expect("duplicate terminal notice is harmless");

        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 3);
        assert!(matches!(
            frames[1].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Projection,
                ..
            }
        ));
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
        assert_eq!(frames[1].revision.get(), 2);
        assert_eq!(frames[2].revision.get(), 3);
    }

    #[test]
    fn prompt_mapping_preserves_exact_user_whitespace() {
        let exact = " \n  keep\tthis exactly  \n";
        let command = ClientCommand::SubmitPrompt {
            session_id: ClientSessionId::new("session-one").expect("session ID"),
            prompt: autoharness_client::PromptContent::new(exact).expect("prompt"),
        };
        let action = map_command(
            command,
            ClientRequestId::new(9).expect("request ID"),
            &active_session(),
        )
        .expect("mapped prompt");

        match action {
            CommandAction::Intent(UiIntent::SubmitPrompt { prompt, .. }) => {
                assert_eq!(prompt, exact);
            }
            CommandAction::Intent(_) | CommandAction::Resynchronize | CommandAction::Shutdown => {
                panic!("unexpected command mapping")
            }
        }
    }

    #[test]
    fn credential_targeting_uses_connection_identity_and_requires_active() {
        let provider_id = ClientProviderId::new("gemini").expect("provider ID");
        let active_connection = ClientConnectionId::new("work").expect("connection ID");
        let inactive_connection = ClientConnectionId::new("backup").expect("connection ID");
        let providers = vec![
            ClientProviderProjection::new(
                active_connection.clone(),
                provider_id.clone(),
                "Work",
                true,
                ClientProviderStatus::Offline,
                ClientCredentialSource::None,
                None,
            )
            .expect("active provider"),
            ClientProviderProjection::new(
                inactive_connection.clone(),
                provider_id,
                "Backup",
                false,
                ClientProviderStatus::Offline,
                ClientCredentialSource::None,
                None,
            )
            .expect("inactive provider"),
        ];

        assert!(credential_targets_active_connection(
            &providers,
            &active_connection
        ));
        assert!(!credential_targets_active_connection(
            &providers,
            &inactive_connection
        ));
    }

    #[test]
    fn stalled_terminal_notices_bound_forwarded_intents() {
        let (ui_ports, mut app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = BridgeActor::new(ui_ports, request_rx, CancellationToken::new());
        bridge
            .pending_requests
            .extend(1..=u64::try_from(MAX_PENDING_REQUESTS).expect("pending bound"));

        assert!(bridge.dispatch(ClientCommand::RefreshCatalog).is_err());
        assert_eq!(bridge.pending_requests.len(), MAX_PENDING_REQUESTS);
        assert!(app_ports.intents.try_recv().is_err());
    }
}
