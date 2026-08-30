//! Feature-gated Tauri carrier over the existing application coordinator ports.

use std::collections::{BTreeSet, VecDeque};
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
const FRAME_ACK_CAPACITY: usize = 1;
const MAX_PENDING_REQUESTS: usize = HOST_REQUEST_CAPACITY;
const MAX_PENDING_NOTICES: usize = MAX_PENDING_REQUESTS * 2 + 2;
const PROJECTION_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct GuiState {
    requests: mpsc::Sender<HostRequest>,
    acknowledgements: mpsc::Sender<FrameAcknowledgement>,
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

    const fn renderer_restart_required() -> Self {
        Self {
            code: "renderer_restart_required",
            message: "restart AutoHarness to recover the native renderer",
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

struct FrameAcknowledgement {
    revision: TransportRevision,
    reply: oneshot::Sender<Result<(), GuiIpcError>>,
}

#[tauri::command]
async fn gui_connect(
    state: tauri::State<'_, GuiState>,
    on_frame: Channel<ServerFrame>,
) -> Result<(), GuiIpcError> {
    let requests = state.requests.clone();
    let (reply, response) = oneshot::channel();
    try_enqueue_host_request(
        &requests,
        HostRequest::Connect {
            channel: on_frame,
            reply,
        },
    )?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

#[tauri::command]
async fn gui_dispatch(
    state: tauri::State<'_, GuiState>,
    command: CommandEnvelope,
) -> Result<CommandReceipt, GuiIpcError> {
    let requests = state.requests.clone();
    let (reply, response) = oneshot::channel();
    try_enqueue_host_request(
        &requests,
        HostRequest::Dispatch {
            command: command.command,
            reply,
        },
    )?;
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
    try_enqueue_host_request(&requests, HostRequest::Credential { ingress, reply })?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

#[tauri::command]
async fn gui_acknowledge_frame(
    state: tauri::State<'_, GuiState>,
    revision: TransportRevision,
) -> Result<(), GuiIpcError> {
    let acknowledgements = state.acknowledgements.clone();
    let (reply, response) = oneshot::channel();
    try_enqueue_frame_ack(&acknowledgements, FrameAcknowledgement { revision, reply })?;
    response.await.map_err(|_| GuiIpcError::disconnected())?
}

fn try_enqueue_host_request(
    requests: &mpsc::Sender<HostRequest>,
    request: HostRequest,
) -> Result<(), GuiIpcError> {
    match requests.try_send(request) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(GuiIpcError::busy()),
        Err(TrySendError::Closed(_)) => Err(GuiIpcError::disconnected()),
    }
}

fn try_enqueue_frame_ack(
    acknowledgements: &mpsc::Sender<FrameAcknowledgement>,
    acknowledgement: FrameAcknowledgement,
) -> Result<(), GuiIpcError> {
    match acknowledgements.try_send(acknowledgement) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(GuiIpcError::busy()),
        Err(TrySendError::Closed(_)) => Err(GuiIpcError::disconnected()),
    }
}

pub(crate) async fn run(ui_ports: UiPorts, shutdown: CancellationToken) -> Result<(), AppError> {
    let (request_tx, request_rx) = mpsc::channel(HOST_REQUEST_CAPACITY);
    let (acknowledgement_tx, acknowledgement_rx) = mpsc::channel(FRAME_ACK_CAPACITY);
    let bridge_shutdown = shutdown.clone();
    let exit_ready = CancellationToken::new();
    let bridge_exit_ready = exit_ready.clone();
    let bridge_task = tokio::spawn(async move {
        let result = BridgeActor::new(ui_ports, request_rx, acknowledgement_rx, bridge_shutdown)
            .with_exit_ready(bridge_exit_ready.clone())
            .run()
            .await;
        bridge_exit_ready.cancel();
        result
    });

    let app = tauri::Builder::default()
        .manage(GuiState {
            requests: request_tx,
            acknowledgements: acknowledgement_tx,
        })
        .invoke_handler(tauri::generate_handler![
            gui_connect,
            gui_dispatch,
            gui_submit_credential,
            gui_acknowledge_frame
        ])
        .build(tauri::generate_context!())
        .map_err(|_| AppError::Configuration)?;

    let handle = app.handle().clone();
    let event_exit_ready = exit_ready.clone();
    let exit_task = tokio::spawn(async move {
        exit_ready.cancelled().await;
        handle.exit(0);
    });
    let event_shutdown = shutdown.clone();
    app.run(move |_handle, event| match event {
        tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if !event_exit_ready.is_cancelled() => {
            api.prevent_close();
            event_shutdown.cancel();
        }
        tauri::RunEvent::ExitRequested { api, .. } if !event_exit_ready.is_cancelled() => {
            api.prevent_exit();
            event_shutdown.cancel();
        }
        _ => {}
    });

    shutdown.cancel();
    exit_task.abort();
    match bridge_task.await {
        Ok(result) => result,
        Err(_) => Err(AppError::WorkerStopped),
    }
}

#[derive(Clone)]
struct QueuedNotice {
    notice: ClientNotice,
    terminal_request_id: Option<u64>,
    wait_for_projection: bool,
}

enum InFlightPayload {
    Snapshot,
    Notice(QueuedNotice),
}

struct InFlightFrame {
    revision: TransportRevision,
    payload: InFlightPayload,
}

struct PendingResynchronization {
    request_id: ClientRequestId,
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
    acknowledgements: mpsc::Receiver<FrameAcknowledgement>,
    channel: Option<Channel<ServerFrame>>,
    last_snapshot: Option<ClientSnapshot>,
    next_request_id: u64,
    next_revision: TransportRevision,
    pending_requests: BTreeSet<u64>,
    pending_notices: VecDeque<QueuedNotice>,
    in_flight: Option<InFlightFrame>,
    pending_resynchronization: Option<PendingResynchronization>,
    catalog_generation: u64,
    projection_dirty: bool,
    shutdown_requested_notice_sent: bool,
    shutdown_ready_notice_sent: bool,
    shutdown_started_at: Option<tokio::time::Instant>,
    shutdown_ready_acknowledged: bool,
    shutdown: CancellationToken,
    exit_ready: CancellationToken,
}

impl BridgeActor {
    fn new(
        ports: UiPorts,
        requests: mpsc::Receiver<HostRequest>,
        acknowledgements: mpsc::Receiver<FrameAcknowledgement>,
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
            acknowledgements,
            channel: None,
            last_snapshot: None,
            next_request_id: 1,
            next_revision: TransportRevision::INITIAL,
            pending_requests: BTreeSet::new(),
            pending_notices: VecDeque::new(),
            in_flight: None,
            pending_resynchronization: None,
            catalog_generation: 1,
            projection_dirty: true,
            shutdown_requested_notice_sent: false,
            shutdown_ready_notice_sent: false,
            shutdown_started_at: None,
            shutdown_ready_acknowledged: false,
            shutdown,
            exit_ready: CancellationToken::new(),
        }
    }

    fn with_exit_ready(mut self, exit_ready: CancellationToken) -> Self {
        self.exit_ready = exit_ready;
        self
    }

    async fn run(mut self) -> Result<(), AppError> {
        let mut frames = tokio::time::interval(PROJECTION_FRAME_INTERVAL);
        frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            if self.shutdown_ready_acknowledged {
                self.exit_ready.cancel();
                return Ok(());
            }
            tokio::select! {
                biased;
                acknowledgement = self.acknowledgements.recv() => {
                    let Some(acknowledgement) = acknowledgement else {
                        if self.shutdown_started_at.is_some() {
                            self.exit_ready.cancel();
                            return Ok(());
                        }
                        return Err(AppError::WorkerStopped);
                    };
                    self.handle_acknowledgement(acknowledgement);
                }
                () = self.shutdown.cancelled(), if self.shutdown_started_at.is_none() => {
                    self.drain_available_notices()?;
                    self.begin_shutdown(None)
                        .map_err(|_| AppError::WorkerStopped)?;
                }
                request = self.requests.recv() => {
                    let Some(request) = request else {
                        if self.shutdown_started_at.is_some() {
                            self.exit_ready.cancel();
                            return Ok(());
                        }
                        return Err(AppError::WorkerStopped);
                    };
                    self.handle_request(request);
                }
                notice = self.notices.recv(), if self.shutdown_started_at.is_none() => {
                    let notice = notice.ok_or(AppError::WorkerStopped)?;
                    self.handle_notice(notice)?;
                }
                result = self.session.changed(), if self.shutdown_started_at.is_none() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.session.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.session_list.changed(), if self.shutdown_started_at.is_none() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.session_list.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.catalog.changed(), if self.shutdown_started_at.is_none() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.catalog.borrow_and_update();
                    self.catalog_generation = self.catalog_generation.saturating_add(1);
                    self.projection_dirty = true;
                }
                result = self.profiles.changed(), if self.shutdown_started_at.is_none() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.profiles.borrow_and_update();
                    self.projection_dirty = true;
                }
                result = self.settings.changed(), if self.shutdown_started_at.is_none() => {
                    result.map_err(|_| AppError::WorkerStopped)?;
                    self.settings.borrow_and_update();
                    self.projection_dirty = true;
                }
                _ = frames.tick() => {
                    let _ = self.pump_outbound();
                    if self.shutdown_started_at.is_some_and(|started| {
                        started.elapsed() >= SHUTDOWN_ACK_TIMEOUT
                    }) {
                        self.exit_ready.cancel();
                        return Ok(());
                    }
                }
            }
        }
    }

    fn handle_request(&mut self, request: HostRequest) {
        match request {
            HostRequest::Connect { channel, reply } => {
                let result = self.connect(channel);
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

    fn handle_acknowledgement(&mut self, acknowledgement: FrameAcknowledgement) {
        let result = self.acknowledge_frame(acknowledgement.revision);
        let _ = acknowledgement.reply.send(result);
    }

    fn connect(&mut self, channel: Channel<ServerFrame>) -> Result<(), GuiIpcError> {
        if self.in_flight.is_some() {
            return Err(GuiIpcError::renderer_restart_required());
        }
        self.channel = Some(channel);
        let result = self.snapshot().and_then(|snapshot| {
            self.send_snapshot(SnapshotReason::Initial, snapshot.clone())?;
            self.last_snapshot = Some(snapshot);
            self.projection_dirty = false;
            Ok(())
        });
        if result.is_err() {
            self.channel = None;
            self.in_flight = None;
        }
        result
    }

    fn dispatch(&mut self, command: ClientCommand) -> Result<CommandReceipt, GuiIpcError> {
        self.require_channel()?;
        if self.shutdown_started_at.is_some() {
            return Err(GuiIpcError::invalid_command());
        }
        let request_id = self.issue_request_id()?;
        let action = map_command(command, request_id, &self.session.borrow())?;
        match action {
            CommandAction::Intent(intent) => self.admit_intent(request_id, intent)?,
            CommandAction::Resynchronize => {
                if self.pending_resynchronization.is_some()
                    || self.pending_requests.len() >= MAX_PENDING_REQUESTS
                {
                    return Err(GuiIpcError::busy());
                }
                self.pending_requests.insert(request_id.get());
                self.pending_resynchronization = Some(PendingResynchronization { request_id });
                if let Err(error) = self.pump_outbound() {
                    self.pending_resynchronization = None;
                    self.pending_requests.remove(&request_id.get());
                    return Err(error);
                }
            }
            CommandAction::Shutdown => {
                if self.pending_requests.len() >= MAX_PENDING_REQUESTS {
                    return Err(GuiIpcError::busy());
                }
                self.drain_available_notices()
                    .map_err(|_| GuiIpcError::disconnected())?;
                self.begin_shutdown(Some(request_id))?;
            }
        }
        Ok(CommandReceipt::new(request_id))
    }

    fn dispatch_credential(
        &mut self,
        ingress: SecretIngress,
    ) -> Result<CommandReceipt, GuiIpcError> {
        self.require_channel()?;
        if self.shutdown_started_at.is_some() {
            return Err(GuiIpcError::invalid_command());
        }
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
        if let Some(request_id) = terminal_request_id {
            if !self.pending_requests.contains(&request_id) {
                return Ok(());
            }
            if self.terminal_notice_outstanding(request_id) {
                return Ok(());
            }
        } else if notice
            .request_id()
            .is_some_and(|request_id| !self.pending_requests.contains(&request_id.get()))
        {
            return Ok(());
        }
        self.observe_pending_projections()?;
        self.queue_notice(notice, terminal_request_id)
            .map_err(|_| AppError::WorkerStopped)?;
        let _ = self.pump_outbound();
        Ok(())
    }

    fn queue_notice(
        &mut self,
        notice: ClientNotice,
        terminal_request_id: Option<u64>,
    ) -> Result<(), GuiIpcError> {
        if let ClientNotice::Authentication { request_id, .. } = &notice
            && let Some(queued) = self.pending_notices.iter_mut().find(|queued| {
                matches!(
                    &queued.notice,
                    ClientNotice::Authentication {
                        request_id: queued_request_id,
                        ..
                    } if queued_request_id == request_id
                )
            })
        {
            queued.notice = notice;
            return Ok(());
        }
        if self.pending_notices.len() >= MAX_PENDING_NOTICES {
            return Err(GuiIpcError::busy());
        }
        self.pending_notices.push_back(QueuedNotice {
            notice,
            terminal_request_id,
            wait_for_projection: terminal_request_id.is_some() && self.projection_dirty,
        });
        Ok(())
    }

    fn terminal_notice_outstanding(&self, request_id: u64) -> bool {
        self.pending_notices
            .iter()
            .any(|notice| notice.terminal_request_id == Some(request_id))
            || matches!(
                self.in_flight.as_ref(),
                Some(InFlightFrame {
                    payload: InFlightPayload::Notice(QueuedNotice {
                        terminal_request_id: Some(in_flight_request_id),
                        ..
                    }),
                    ..
                }) if *in_flight_request_id == request_id
            )
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

    fn drain_available_notices(&mut self) -> Result<(), AppError> {
        while let Ok(notice) = self.notices.try_recv() {
            self.handle_notice(notice)?;
        }
        Ok(())
    }

    fn begin_shutdown(
        &mut self,
        command_request_id: Option<ClientRequestId>,
    ) -> Result<(), GuiIpcError> {
        if self.shutdown_started_at.is_some() {
            return Ok(());
        }
        self.shutdown.cancel();
        self.shutdown_started_at = Some(tokio::time::Instant::now());
        self.projection_dirty = true;
        self.pending_resynchronization = None;
        if let Some(request_id) = command_request_id {
            self.pending_requests.insert(request_id.get());
            self.queue_notice(
                ClientNotice::CommandCommitted { request_id },
                Some(request_id.get()),
            )?;
        }
        self.queue_shutdown_notice(ShutdownState::Requested)?;
        self.queue_shutdown_notice(ShutdownState::Ready)?;
        self.pump_outbound()
    }

    fn queue_shutdown_notice(&mut self, state: ShutdownState) -> Result<(), GuiIpcError> {
        let already_sent = match state {
            ShutdownState::Requested => self.shutdown_requested_notice_sent,
            ShutdownState::Ready => self.shutdown_ready_notice_sent,
        };
        if already_sent {
            return Ok(());
        }
        self.queue_notice(ClientNotice::Shutdown { state }, None)?;
        match state {
            ShutdownState::Requested => self.shutdown_requested_notice_sent = true,
            ShutdownState::Ready => self.shutdown_ready_notice_sent = true,
        }
        Ok(())
    }

    fn acknowledge_frame(&mut self, revision: TransportRevision) -> Result<(), GuiIpcError> {
        let Some(in_flight) = self.in_flight.take() else {
            return Err(GuiIpcError::invalid_command());
        };
        if in_flight.revision != revision {
            self.in_flight = Some(in_flight);
            return Err(GuiIpcError::invalid_command());
        }
        if let InFlightPayload::Notice(queued) = in_flight.payload {
            if let Some(request_id) = queued.terminal_request_id {
                self.pending_requests.remove(&request_id);
            }
            if matches!(
                queued.notice,
                ClientNotice::Shutdown {
                    state: ShutdownState::Ready
                }
            ) {
                self.shutdown_ready_acknowledged = true;
            }
        }
        self.pump_outbound()
    }

    fn pump_outbound(&mut self) -> Result<(), GuiIpcError> {
        if self.in_flight.is_some() || self.channel.is_none() {
            return Ok(());
        }
        if let Some(request_id) = self
            .pending_resynchronization
            .as_ref()
            .map(|pending| pending.request_id)
        {
            let snapshot = self.snapshot()?;
            self.send_snapshot(SnapshotReason::Resynchronization, snapshot.clone())?;
            self.last_snapshot = Some(snapshot);
            self.projection_dirty = false;
            self.pending_resynchronization = None;
            self.queue_notice(
                ClientNotice::CommandCommitted { request_id },
                Some(request_id.get()),
            )?;
            return Ok(());
        }
        if self
            .pending_notices
            .front()
            .is_some_and(|queued| queued.wait_for_projection)
        {
            if self.projection_dirty {
                self.publish_projection();
                if !self.projection_dirty
                    && let Some(queued) = self.pending_notices.front_mut()
                {
                    queued.wait_for_projection = false;
                }
                if self.in_flight.is_some() || self.projection_dirty {
                    return Ok(());
                }
            } else if let Some(queued) = self.pending_notices.front_mut() {
                queued.wait_for_projection = false;
            }
        }
        let Some(queued) = self.pending_notices.pop_front() else {
            if self.projection_dirty {
                self.publish_projection();
            }
            return Ok(());
        };
        if let Err(error) = self.send_notice(queued.clone()) {
            self.pending_notices.push_front(queued);
            return Err(error);
        }
        Ok(())
    }

    fn send_intent(&self, intent: UiIntent) -> Result<(), GuiIpcError> {
        match self.intents.try_send(intent) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(GuiIpcError::busy()),
            Err(TrySendError::Closed(_)) => Err(GuiIpcError::disconnected()),
        }
    }

    fn require_channel(&self) -> Result<(), GuiIpcError> {
        self.channel
            .as_ref()
            .map(|_| ())
            .ok_or_else(GuiIpcError::disconnected)
    }

    fn issue_request_id(&mut self) -> Result<ClientRequestId, GuiIpcError> {
        let value = self.next_request_id;
        self.next_request_id = value.checked_add(1).ok_or_else(GuiIpcError::disconnected)?;
        ClientRequestId::new(value).map_err(|_| GuiIpcError::disconnected())
    }

    fn publish_projection(&mut self) {
        if self.in_flight.is_some() || self.channel.is_none() {
            return;
        }
        let Ok(snapshot) = self.snapshot() else {
            return;
        };
        if self.last_snapshot.as_ref() == Some(&snapshot) {
            self.projection_dirty = false;
            return;
        }
        if self
            .send_snapshot(SnapshotReason::Projection, snapshot.clone())
            .is_ok()
        {
            self.last_snapshot = Some(snapshot);
            self.projection_dirty = false;
        }
    }

    fn snapshot(&self) -> Result<ClientSnapshot, GuiIpcError> {
        map_snapshot(
            &self.session.borrow(),
            &self.session_list.borrow(),
            &self.catalog.borrow(),
            &self.profiles.borrow(),
            &self.settings.borrow(),
            SnapshotRuntime {
                catalog_generation: self.catalog_generation,
                shutting_down: self.shutdown.is_cancelled(),
            },
        )
    }

    fn send_snapshot(
        &mut self,
        reason: SnapshotReason,
        snapshot: ClientSnapshot,
    ) -> Result<(), GuiIpcError> {
        let revision = self.take_revision()?;
        self.send(
            ServerFrame::snapshot(revision, reason, snapshot),
            InFlightPayload::Snapshot,
        )
    }

    fn send_notice(&mut self, queued: QueuedNotice) -> Result<(), GuiIpcError> {
        let revision = self.take_revision()?;
        self.send(
            ServerFrame::notice(revision, queued.notice.clone()),
            InFlightPayload::Notice(queued),
        )
    }

    fn take_revision(&mut self) -> Result<TransportRevision, GuiIpcError> {
        let revision = self.next_revision;
        self.next_revision = revision.next().map_err(|_| GuiIpcError::disconnected())?;
        Ok(revision)
    }

    fn send(&mut self, frame: ServerFrame, payload: InFlightPayload) -> Result<(), GuiIpcError> {
        if self.in_flight.is_some() {
            return Err(GuiIpcError::busy());
        }
        let channel = self.channel.clone().ok_or_else(GuiIpcError::disconnected)?;
        let revision = frame.revision;
        if channel.send(frame).is_err() {
            self.channel = None;
            return Err(GuiIpcError::disconnected());
        }
        self.in_flight = Some(InFlightFrame { revision, payload });
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
            failure: match map_failure(&failure) {
                Ok(failure) => failure,
                Err(_) => SafeFailure::new(
                    FailureClass::Internal,
                    "notice_projection_failed",
                    "the command result could not be represented safely",
                    RetryDirective::Never,
                )
                .map_err(|_| GuiIpcError::invalid_projection())?,
            },
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

#[derive(Clone, Copy)]
struct SnapshotRuntime {
    catalog_generation: u64,
    shutting_down: bool,
}

fn map_snapshot(
    active: &TuiSessionProjection,
    session_list: &TuiSessionsProjection,
    catalog: &TuiCatalogProjection,
    profiles: &TuiProfilesProjection,
    settings: &TuiSettingsProjection,
    runtime: SnapshotRuntime,
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
                Some(active.revision),
                active_session.selected_model.clone(),
                None,
                None,
                false,
            ),
        );
    }
    let catalog = map_catalog(catalog, runtime.catalog_generation)?;
    let session_credential_connection = session_credential_connection(settings)?;
    let providers = map_providers(
        profiles,
        settings,
        &catalog,
        session_credential_connection.as_ref(),
    )?;
    let lifecycle = if runtime.shutting_down {
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
    let revision = (summary.session_id == active.session_id).then_some(active.revision);
    Ok(SessionSummary::new(
        session_id,
        title,
        revision,
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
    session_credential_connection: Option<&ClientConnectionId>,
) -> Result<Vec<ClientProviderProjection>, GuiIpcError> {
    if profiles.profiles.is_empty() {
        return Ok(vec![fallback_provider(
            settings,
            catalog,
            session_credential_connection,
        )?]);
    }
    profiles
        .profiles
        .iter()
        .map(|profile| {
            let connection_id = ClientConnectionId::new(profile.id.clone())
                .map_err(|_| GuiIpcError::invalid_projection())?;
            let session_connected =
                profile.active && session_credential_connection == Some(&connection_id);
            let persisted_connected =
                profile.active && settings.provider_status.credential_connected;
            let connected = session_connected || persisted_connected;
            let credential_source = if session_connected {
                ClientCredentialSource::SessionOnly
            } else {
                map_credential_source(profile.credential_source, persisted_connected)
            };
            let status = if session_connected
                && matches!(catalog, ClientCatalogProjection::Ready { .. })
            {
                ClientProviderStatus::Ready
            } else {
                match &profile.connection {
                    ProfileConnectionState::Testing => ClientProviderStatus::Connecting,
                    ProfileConnectionState::Ready if connected => ClientProviderStatus::Ready,
                    ProfileConnectionState::Ready
                        if profile.credential_state
                            == ProfileCredentialStateLabel::Disconnected =>
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
                        if profile.credential_state
                            == ProfileCredentialStateLabel::Disconnected =>
                    {
                        ClientProviderStatus::CredentialRequired
                    }
                    ProfileConnectionState::Untested => ClientProviderStatus::Offline,
                }
            };
            let display_name = format!("{} - {}", profile.kind.as_str(), profile.id);
            ClientProviderProjection::new(
                connection_id,
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

fn session_credential_connection(
    settings: &TuiSettingsProjection,
) -> Result<Option<ClientConnectionId>, GuiIpcError> {
    let status = &settings.provider_status;
    if !status.credential_connected
        || status.credential_source != CredentialSourceLabel::SessionOnly
    {
        return Ok(None);
    }
    let connection_id = status.active_profile.clone().unwrap_or_else(|| {
        let kind = status.provider_kind.unwrap_or(ProviderKindLabel::Gemini);
        format!("session:{}", provider_kind_id(kind))
    });
    ClientConnectionId::new(connection_id)
        .map(Some)
        .map_err(|_| GuiIpcError::invalid_projection())
}

fn fallback_provider(
    settings: &TuiSettingsProjection,
    catalog: &ClientCatalogProjection,
    session_credential_connection: Option<&ClientConnectionId>,
) -> Result<ClientProviderProjection, GuiIpcError> {
    let kind = settings
        .provider_status
        .provider_kind
        .unwrap_or(ProviderKindLabel::Gemini);
    let persisted_connected = settings.provider_status.credential_connected;
    let connection_id = settings
        .provider_status
        .active_profile
        .clone()
        .unwrap_or_else(|| format!("session:{}", provider_kind_id(kind)));
    let connection_id =
        ClientConnectionId::new(connection_id).map_err(|_| GuiIpcError::invalid_projection())?;
    let session_connected = session_credential_connection == Some(&connection_id);
    let connected = session_connected || persisted_connected;
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
    ClientProviderProjection::new(
        connection_id,
        catalog_provider_id(catalog, kind).unwrap_or(
            ClientProviderId::new(provider_kind_id(kind))
                .map_err(|_| GuiIpcError::invalid_projection())?,
        ),
        kind.as_str(),
        true,
        status,
        if session_connected {
            ClientCredentialSource::SessionOnly
        } else {
            map_credential_source(
                settings.provider_status.credential_source,
                persisted_connected,
            )
        },
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

    fn test_bridge_actor(
        ports: UiPorts,
        requests: mpsc::Receiver<HostRequest>,
        shutdown: CancellationToken,
    ) -> BridgeActor {
        let (_acknowledgement_tx, acknowledgement_rx) = mpsc::channel(FRAME_ACK_CAPACITY);
        BridgeActor::new(ports, requests, acknowledgement_rx, shutdown)
    }

    fn acknowledge_current_frame(bridge: &mut BridgeActor) {
        let revision = bridge
            .in_flight
            .as_ref()
            .expect("one in-flight frame")
            .revision;
        bridge
            .acknowledge_frame(revision)
            .expect("frame acknowledgement");
    }

    async fn acknowledge_actor_frame(
        acknowledgements: &mpsc::Sender<FrameAcknowledgement>,
        received: &Arc<Mutex<Vec<ServerFrame>>>,
        frame_count: usize,
    ) {
        let revision = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(revision) = received
                    .lock()
                    .expect("frame lock")
                    .get(frame_count.saturating_sub(1))
                    .map(|frame| frame.revision)
                {
                    break revision;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("frame delivery before acknowledgement");
        let (reply, response) = oneshot::channel();
        acknowledgements
            .send(FrameAcknowledgement { revision, reply })
            .await
            .expect("acknowledgement request");
        response
            .await
            .expect("acknowledgement response")
            .expect("accepted acknowledgement");
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
    fn saturated_host_queue_fails_admission_without_waiting() {
        let (requests, mut queued) = mpsc::channel(1);
        let (first_reply, _first_response) = oneshot::channel();
        try_enqueue_host_request(
            &requests,
            HostRequest::Dispatch {
                command: ClientCommand::RefreshCatalog,
                reply: first_reply,
            },
        )
        .expect("first request");
        let (second_reply, _second_response) = oneshot::channel();

        let error = try_enqueue_host_request(
            &requests,
            HostRequest::Dispatch {
                command: ClientCommand::RefreshCatalog,
                reply: second_reply,
            },
        )
        .expect_err("full queue must fail immediately");

        assert_eq!(error.code, "host_busy");
        assert!(matches!(
            queued.try_recv(),
            Ok(HostRequest::Dispatch { .. })
        ));
        assert!(queued.try_recv().is_err());
    }

    #[tokio::test]
    async fn saturated_command_mailbox_cannot_starve_the_exact_frame_ack() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (request_tx, request_rx) = mpsc::channel(HOST_REQUEST_CAPACITY);
        let (acknowledgement_tx, acknowledgement_rx) = mpsc::channel(FRAME_ACK_CAPACITY);
        let shutdown = CancellationToken::new();
        let mut bridge =
            BridgeActor::new(ui_ports, request_rx, acknowledgement_rx, shutdown.clone());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        bridge.connect(channel).expect("initial baseline");
        let initial_revision = bridge.in_flight.as_ref().expect("in-flight frame").revision;
        let mut changed = active_session();
        changed.revision = 2;
        app_ports
            .sessions
            .send(Arc::new(changed))
            .expect("changed session");
        bridge
            .observe_pending_projections()
            .expect("observe projection");
        for _ in 0..HOST_REQUEST_CAPACITY {
            let (reply, _response) = oneshot::channel();
            try_enqueue_host_request(
                &request_tx,
                HostRequest::Dispatch {
                    command: ClientCommand::RefreshCatalog,
                    reply,
                },
            )
            .expect("fill command mailbox");
        }
        assert_eq!(request_tx.capacity(), 0);
        let (ack_reply, ack_response) = oneshot::channel();
        try_enqueue_frame_ack(
            &acknowledgement_tx,
            FrameAcknowledgement {
                revision: initial_revision,
                reply: ack_reply,
            },
        )
        .expect("dedicated acknowledgement admission");

        let actor_task = tokio::spawn(bridge.run());
        ack_response
            .await
            .expect("acknowledgement response")
            .expect("exact acknowledgement");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if received.lock().expect("frame lock").len() == 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("coalesced projection after acknowledgement");

        {
            let frames = received.lock().expect("frame lock");
            assert!(matches!(
                frames[1].payload,
                autoharness_client::FramePayload::Snapshot {
                    reason: SnapshotReason::Projection,
                    ..
                }
            ));
        }
        actor_task.abort();
        let _ = actor_task.await;
    }

    #[test]
    fn saturated_credential_ingress_is_redacted_and_dropped() {
        const SENTINEL: &str = "saturated-secret-must-not-survive";
        let (requests, mut queued) = mpsc::channel(1);
        let (first_reply, _first_response) = oneshot::channel();
        try_enqueue_host_request(
            &requests,
            HostRequest::Dispatch {
                command: ClientCommand::RefreshCatalog,
                reply: first_reply,
            },
        )
        .expect("queue blocker");
        let ingress = SecretIngress::new(
            ClientConnectionId::new("session:gemini").expect("connection ID"),
            SENTINEL,
        )
        .expect("secret ingress");
        assert!(!format!("{ingress:?}").contains(SENTINEL));
        let (reply, _response) = oneshot::channel();

        let error = try_enqueue_host_request(&requests, HostRequest::Credential { ingress, reply })
            .expect_err("full queue must drop credential request");

        let safe_error = serde_json::to_string(&error).expect("safe IPC error");
        assert_eq!(error.code, "host_busy");
        assert!(!safe_error.contains(SENTINEL));
        assert!(matches!(
            queued.try_recv(),
            Ok(HostRequest::Dispatch { .. })
        ));
        assert!(queued.try_recv().is_err());
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
            SnapshotRuntime {
                catalog_generation: 7,
                shutting_down: false,
            },
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
    fn inactive_session_summary_does_not_invent_a_durable_revision() {
        let summary = autoharness_tui::SessionBrowserEntry {
            session_id: "inactive-session".to_owned(),
            title: "Previous work".to_owned(),
            archived: false,
            selected_model: None,
            message_count: 4,
            updated_at_ms: 123,
            active: false,
        };

        let mapped = map_session_summary(&summary, &active_session()).expect("session summary");

        assert_eq!(mapped.revision, None);
        assert!(mapped.updated_at_ms.is_some());
        assert!(mapped.message_count.is_some());
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
            None,
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

        let provider = fallback_provider(&TuiSettingsProjection::default(), &catalog, None)
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
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
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
        acknowledge_current_frame(&mut bridge);
        let receipt = bridge
            .dispatch(ClientCommand::RequestResynchronization {
                last_applied_revision: Some(TransportRevision::INITIAL),
            })
            .expect("resynchronization request");
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

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
    fn acknowledgement_requires_the_exact_in_flight_revision() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (reply, response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect { channel, reply });
        response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");
        let expected = bridge.in_flight.as_ref().expect("in-flight frame").revision;
        let wrong = expected.next().expect("next revision");

        let error = bridge
            .acknowledge_frame(wrong)
            .expect_err("wrong revision must be rejected");

        assert_eq!(error.code, "invalid_command");
        assert_eq!(
            bridge.in_flight.as_ref().expect("retained frame").revision,
            expected
        );
        assert_eq!(received.lock().expect("frame lock").len(), 1);
        bridge
            .acknowledge_frame(expected)
            .expect("exact acknowledgement");
        assert!(bridge.in_flight.is_none());
    }

    #[test]
    fn repeated_connect_before_ack_preserves_one_carrier_send() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let first_frames = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let first_received = Arc::clone(&first_frames);
        let first_channel = Channel::new(move |body| {
            first_received
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (first_reply, first_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel: first_channel,
            reply: first_reply,
        });
        first_response
            .blocking_recv()
            .expect("first connect response")
            .expect("first baseline");
        let first_revision = bridge.in_flight.as_ref().expect("in-flight frame").revision;
        let rejected_frames = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let rejected_received = Arc::clone(&rejected_frames);
        let rejected_channel = Channel::new(move |body| {
            rejected_received
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (rejected_reply, rejected_response) = oneshot::channel();

        bridge.handle_request(HostRequest::Connect {
            channel: rejected_channel,
            reply: rejected_reply,
        });

        let error = rejected_response
            .blocking_recv()
            .expect("repeated connect response")
            .expect_err("unacknowledged baseline must block replacement");
        assert_eq!(error.code, "renderer_restart_required");
        assert_eq!(
            error.message,
            "restart AutoHarness to recover the native renderer"
        );
        assert_eq!(first_frames.lock().expect("frame lock").len(), 1);
        assert!(rejected_frames.lock().expect("frame lock").is_empty());
        assert_eq!(
            bridge.in_flight.as_ref().expect("preserved frame").revision,
            first_revision
        );

        bridge
            .acknowledge_frame(first_revision)
            .expect("first acknowledgement");
        let replacement_frames = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let replacement_received = Arc::clone(&replacement_frames);
        let replacement_channel = Channel::new(move |body| {
            replacement_received
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (replacement_reply, replacement_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel: replacement_channel,
            reply: replacement_reply,
        });
        replacement_response
            .blocking_recv()
            .expect("replacement response")
            .expect("replacement after acknowledgement");

        assert_eq!(replacement_frames.lock().expect("frame lock").len(), 1);
        assert!(bridge.in_flight.is_some());
    }

    #[test]
    fn blocked_renderer_keeps_one_frame_and_coalesces_the_latest_projection() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (reply, response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect { channel, reply });
        response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");

        for revision in [2, 3, 9] {
            let mut changed = active_session();
            changed.revision = revision;
            app_ports
                .sessions
                .send(Arc::new(changed))
                .expect("session projection");
            bridge
                .observe_pending_projections()
                .expect("observe projection");
            bridge.pump_outbound().expect("coalesce projection");
        }

        assert_eq!(received.lock().expect("frame lock").len(), 1);
        assert!(bridge.projection_dirty);
        acknowledge_current_frame(&mut bridge);
        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 2);
        let latest = match &frames[1].payload {
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Projection,
                snapshot,
            } => snapshot,
            _ => panic!("expected coalesced projection"),
        };
        assert_eq!(
            latest
                .active_session
                .as_ref()
                .expect("active session")
                .revision
                .get(),
            9
        );
        drop(frames);
        acknowledge_current_frame(&mut bridge);
        assert_eq!(received.lock().expect("frame lock").len(), 2);
    }

    #[test]
    fn gap_resynchronization_queues_before_ack_and_commits_after_its_baseline() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (reply, response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect { channel, reply });
        response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");
        acknowledge_current_frame(&mut bridge);
        app_ports
            .catalogs
            .send(Arc::new(TuiCatalogProjection::Loading))
            .expect("gap projection");
        bridge
            .observe_pending_projections()
            .expect("observe gap projection");
        bridge.pump_outbound().expect("publish gap projection");

        let receipt = bridge
            .dispatch(ClientCommand::RequestResynchronization {
                last_applied_revision: Some(TransportRevision::INITIAL),
            })
            .expect("queue resynchronization before gap acknowledgement");

        assert_eq!(received.lock().expect("frame lock").len(), 2);
        assert!(bridge.pending_resynchronization.is_some());
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 4);
        assert!(matches!(
            frames[1].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Projection,
                ..
            }
        ));
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Resynchronization,
                ..
            }
        ));
        assert!(matches!(
            frames[3].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
        assert!(!bridge.pending_requests.contains(&receipt.request_id.get()));
    }

    #[test]
    fn connect_reports_a_channel_that_cannot_accept_its_baseline() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
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
    fn one_projection_precedes_terminal_notice_despite_continuous_churn() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
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
        acknowledge_current_frame(&mut bridge);

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
        app_ports
            .catalogs
            .send(Arc::new(TuiCatalogProjection::CredentialRequired))
            .expect("continued projection churn");
        bridge
            .observe_pending_projections()
            .expect("observe continued churn");
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 4);
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
        assert!(matches!(
            frames[3].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Projection,
                ..
            }
        ));
        assert_eq!(frames[1].revision.get(), 2);
        assert_eq!(frames[2].revision.get(), 3);
        assert_eq!(frames[3].revision.get(), 4);
    }

    #[test]
    fn failed_terminal_delivery_replays_after_a_fresh_baseline() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let first_channel = Channel::new(|_| Ok(()));
        let (connect_reply, connect_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel: first_channel,
            reply: connect_reply,
        });
        connect_response
            .blocking_recv()
            .expect("connect response")
            .expect("connect baseline");
        acknowledge_current_frame(&mut bridge);
        let receipt = bridge
            .dispatch(ClientCommand::RefreshCatalog)
            .expect("admitted command");
        bridge.channel = Some(Channel::new(|_| {
            Err(std::io::Error::other("renderer disconnected").into())
        }));

        bridge
            .handle_notice(UiNotice::IntentCommitted {
                request_id: TuiRequestId::new(receipt.request_id.get()),
            })
            .expect("terminal notice handling");

        assert!(bridge.pending_requests.contains(&receipt.request_id.get()));
        assert!(
            bridge
                .pending_notices
                .iter()
                .any(|notice| notice.terminal_request_id == Some(receipt.request_id.get()))
        );
        let disconnected = bridge
            .dispatch(ClientCommand::RefreshCatalog)
            .expect_err("lost channel must reject new work");
        assert_eq!(disconnected.code, "host_disconnected");
        let replayed = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let replay_frames = Arc::clone(&replayed);
        let channel = Channel::new(move |body| {
            replay_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (reconnect_reply, reconnect_response) = oneshot::channel();
        bridge.handle_request(HostRequest::Connect {
            channel,
            reply: reconnect_reply,
        });
        reconnect_response
            .blocking_recv()
            .expect("reconnect response")
            .expect("reconnect baseline and replay");
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

        let frames = replayed.lock().expect("frame lock");
        assert_eq!(frames.len(), 2);
        assert!(matches!(
            frames[0].payload,
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Initial,
                ..
            }
        ));
        assert!(matches!(
            frames[1].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
        assert!(!bridge.pending_requests.contains(&receipt.request_id.get()));
        assert!(bridge.pending_notices.is_empty());
    }

    #[test]
    fn invalid_projection_holds_terminal_notice_until_state_is_representable() {
        let (ui_ports, app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
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
        acknowledge_current_frame(&mut bridge);
        let receipt = bridge
            .dispatch(ClientCommand::RefreshCatalog)
            .expect("admitted command");
        app_ports
            .sessions
            .send(Arc::new(TuiSessionProjection::empty()))
            .expect("invalid transitional projection");

        bridge
            .handle_notice(UiNotice::IntentCommitted {
                request_id: TuiRequestId::new(receipt.request_id.get()),
            })
            .expect("terminal notice handling");

        assert!(bridge.projection_dirty);
        assert_eq!(received.lock().expect("frame lock").len(), 1);
        assert!(
            bridge
                .pending_notices
                .iter()
                .any(|notice| notice.terminal_request_id == Some(receipt.request_id.get()))
        );
        let mut repaired = active_session();
        repaired.revision = 2;
        app_ports
            .sessions
            .send(Arc::new(repaired))
            .expect("repaired projection");
        bridge
            .observe_pending_projections()
            .expect("observe repair");
        bridge.pump_outbound().expect("repaired projection");
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

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
    }

    #[tokio::test]
    async fn explicit_shutdown_publishes_lifecycle_and_ready_before_host_exit() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (request_tx, request_rx) = mpsc::channel(HOST_REQUEST_CAPACITY);
        let (acknowledgement_tx, acknowledgement_rx) = mpsc::channel(FRAME_ACK_CAPACITY);
        let shutdown = CancellationToken::new();
        let exit_ready = CancellationToken::new();
        let actor = BridgeActor::new(ui_ports, request_rx, acknowledgement_rx, shutdown.clone())
            .with_exit_ready(exit_ready.clone());
        let actor_task = tokio::spawn(actor.run());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (connect_reply, connect_response) = oneshot::channel();
        request_tx
            .send(HostRequest::Connect {
                channel,
                reply: connect_reply,
            })
            .await
            .expect("connect request");
        connect_response
            .await
            .expect("connect response")
            .expect("connect baseline");
        acknowledge_actor_frame(&acknowledgement_tx, &received, 1).await;
        let (pending_reply, pending_response) = oneshot::channel();
        request_tx
            .send(HostRequest::Dispatch {
                command: ClientCommand::RefreshCatalog,
                reply: pending_reply,
            })
            .await
            .expect("pending request");
        let pending_receipt = pending_response
            .await
            .expect("pending response")
            .expect("pending admission");
        let (dispatch_reply, dispatch_response) = oneshot::channel();
        request_tx
            .send(HostRequest::Dispatch {
                command: ClientCommand::RequestShutdown,
                reply: dispatch_reply,
            })
            .await
            .expect("shutdown request");
        let receipt = dispatch_response
            .await
            .expect("shutdown response")
            .expect("shutdown admission");
        acknowledge_actor_frame(&acknowledgement_tx, &received, 2).await;
        acknowledge_actor_frame(&acknowledgement_tx, &received, 3).await;
        acknowledge_actor_frame(&acknowledgement_tx, &received, 4).await;
        acknowledge_actor_frame(&acknowledgement_tx, &received, 5).await;
        actor_task
            .await
            .expect("bridge task")
            .expect("clean bridge shutdown");

        assert!(shutdown.is_cancelled());
        assert!(exit_ready.is_cancelled());
        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 5);
        let shutting_down = match &frames[1].payload {
            autoharness_client::FramePayload::Snapshot { snapshot, .. } => snapshot,
            autoharness_client::FramePayload::Notice(_) => {
                panic!("expected shutdown projection")
            }
        };
        assert!(matches!(
            shutting_down.lifecycle,
            ClientLifecycle::ShuttingDown
        ));
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
        assert!(matches!(
            frames[3].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::Shutdown {
                state: ShutdownState::Requested
            })
        ));
        assert!(matches!(
            frames[4].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::Shutdown {
                state: ShutdownState::Ready
            })
        ));
        assert!(frames.iter().all(|frame| {
            !matches!(
                &frame.payload,
                autoharness_client::FramePayload::Notice(notice)
                    if notice.request_id() == Some(pending_receipt.request_id)
            )
        }));
    }

    #[tokio::test]
    async fn native_close_publishes_lifecycle_and_ready_before_host_exit() {
        let (ui_ports, _app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (request_tx, request_rx) = mpsc::channel(HOST_REQUEST_CAPACITY);
        let (acknowledgement_tx, acknowledgement_rx) = mpsc::channel(FRAME_ACK_CAPACITY);
        let shutdown = CancellationToken::new();
        let exit_ready = CancellationToken::new();
        let actor = BridgeActor::new(ui_ports, request_rx, acknowledgement_rx, shutdown.clone())
            .with_exit_ready(exit_ready.clone());
        let actor_task = tokio::spawn(actor.run());
        let received = Arc::new(Mutex::new(Vec::<ServerFrame>::new()));
        let channel_frames = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            channel_frames
                .lock()
                .expect("frame lock")
                .push(body.deserialize::<ServerFrame>().expect("server frame"));
            Ok(())
        });
        let (connect_reply, connect_response) = oneshot::channel();
        request_tx
            .send(HostRequest::Connect {
                channel,
                reply: connect_reply,
            })
            .await
            .expect("connect request");
        connect_response
            .await
            .expect("connect response")
            .expect("connect baseline");
        acknowledge_actor_frame(&acknowledgement_tx, &received, 1).await;

        shutdown.cancel();
        acknowledge_actor_frame(&acknowledgement_tx, &received, 2).await;
        acknowledge_actor_frame(&acknowledgement_tx, &received, 3).await;
        acknowledge_actor_frame(&acknowledgement_tx, &received, 4).await;
        actor_task
            .await
            .expect("bridge task")
            .expect("clean bridge shutdown");

        assert!(exit_ready.is_cancelled());
        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 4);
        assert!(matches!(
            &frames[1].payload,
            autoharness_client::FramePayload::Snapshot { snapshot, .. }
                if matches!(snapshot.lifecycle, ClientLifecycle::ShuttingDown)
        ));
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::Shutdown {
                state: ShutdownState::Requested
            })
        ));
        assert!(matches!(
            frames[3].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::Shutdown {
                state: ShutdownState::Ready
            })
        ));
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
    fn authoritative_session_credential_projects_ready_on_the_exact_connection() {
        const SENTINEL: &str = "gui-session-credential-sentinel";
        let (ui_ports, mut app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
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
        acknowledge_current_frame(&mut bridge);

        let connection_id = ClientConnectionId::new("session:gemini").expect("connection ID");
        let receipt = bridge
            .dispatch_credential(
                SecretIngress::new(connection_id.clone(), SENTINEL).expect("secret ingress"),
            )
            .expect("credential admission");
        assert!(matches!(
            app_ports.intents.try_recv().expect("credential intent"),
            UiIntent::ConfigureCredential { .. }
        ));
        app_ports
            .catalogs
            .send(Arc::new(TuiCatalogProjection::Ready {
                models: Vec::new(),
                stale: false,
            }))
            .expect("ready catalog");
        let mut settings = TuiSettingsProjection::default();
        settings.provider_status.provider_kind = Some(ProviderKindLabel::Gemini);
        settings.provider_status.credential_source = CredentialSourceLabel::SessionOnly;
        settings.provider_status.credential_connected = true;
        app_ports
            .settings
            .send(Arc::new(settings))
            .expect("session credential status");
        bridge
            .handle_notice(UiNotice::IntentCommitted {
                request_id: TuiRequestId::new(receipt.request_id.get()),
            })
            .expect("credential commit");
        acknowledge_current_frame(&mut bridge);
        acknowledge_current_frame(&mut bridge);

        let frames = received.lock().expect("frame lock");
        assert_eq!(frames.len(), 3);
        let snapshot = match &frames[1].payload {
            autoharness_client::FramePayload::Snapshot {
                reason: SnapshotReason::Projection,
                snapshot,
            } => snapshot,
            autoharness_client::FramePayload::Snapshot { .. }
            | autoharness_client::FramePayload::Notice(_) => {
                panic!("expected credential projection")
            }
        };
        assert!(matches!(snapshot.lifecycle, ClientLifecycle::Ready));
        assert_eq!(snapshot.providers[0].connection_id, connection_id);
        assert!(matches!(
            snapshot.providers[0].status,
            ClientProviderStatus::Ready
        ));
        assert_eq!(
            snapshot.providers[0].credential_source,
            ClientCredentialSource::SessionOnly
        );
        assert!(matches!(
            frames[2].payload,
            autoharness_client::FramePayload::Notice(ClientNotice::CommandCommitted {
                request_id
            }) if request_id == receipt.request_id
        ));
        assert!(
            !serde_json::to_string(frames.as_slice())
                .expect("serialized frames")
                .contains(SENTINEL)
        );
    }

    #[test]
    fn stalled_terminal_notices_bound_forwarded_intents() {
        let (ui_ports, mut app_ports) = autoharness_tui::bounded_ports(
            Arc::new(active_session()),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let (_request_tx, request_rx) = mpsc::channel(1);
        let mut bridge = test_bridge_actor(ui_ports, request_rx, CancellationToken::new());
        bridge.channel = Some(Channel::new(|_| Ok(())));
        bridge
            .pending_requests
            .extend(1..=u64::try_from(MAX_PENDING_REQUESTS).expect("pending bound"));

        let busy = bridge
            .dispatch(ClientCommand::RefreshCatalog)
            .expect_err("pending bound must reject new work");
        assert_eq!(busy.code, "host_busy");
        assert_eq!(bridge.pending_requests.len(), MAX_PENDING_REQUESTS);
        assert!(app_ports.intents.try_recv().is_err());
    }
}
