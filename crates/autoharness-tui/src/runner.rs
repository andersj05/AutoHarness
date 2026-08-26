use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Display, Formatter};
use std::io::{BufRead as _, BufReader};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use autoharness_domain::ErrorClass;
use crossterm::event::{Event, EventStream, KeyEventKind, MouseButton, MouseEventKind};
use futures_util::StreamExt as _;
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::model::{
    CatalogProjection, Message, Model, ProfilesProjection, RetryPolicy, SessionProjection,
    SessionsProjection, SettingsProjection, UiEffect, UiFailure, UiIntent, UiNotice,
};
use crate::{update, view};

/// Maximum queued user intents before explicit backpressure is presented.
pub const INTENT_CAPACITY: usize = 32;
/// Maximum queued application notices before their producer is backpressured.
pub const APP_NOTICE_CAPACITY: usize = 128;
const CODEX_STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CODEX_AUTH_URL_PREFIX: &str = "https://auth.openai.com/";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CodexLoginEventKind {
    BrowserOpened,
    AlreadyAuthenticated,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CodexLoginEvent {
    generation: u64,
    kind: CodexLoginEventKind,
}

struct CodexLoginController {
    events: mpsc::UnboundedSender<CodexLoginEvent>,
    cancellation: Option<CancellationToken>,
    generation: u64,
}

impl CodexLoginController {
    fn new(events: mpsc::UnboundedSender<CodexLoginEvent>) -> Self {
        Self {
            events,
            cancellation: None,
            generation: 0,
        }
    }

    fn launch(&mut self, executable: OsString) -> std::io::Result<()> {
        self.cancel();
        let generation = self.generation;
        let cancellation = CancellationToken::new();
        self.cancellation = Some(cancellation.clone());
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("autoharness-codex-login".to_owned())
            .spawn(move || run_codex_login(executable, cancellation, events, generation))
            .map(|_| ())
    }

    fn cancel(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn accepts(&self, event: CodexLoginEvent) -> bool {
        event.generation == self.generation
    }

    fn settle(&mut self, event: CodexLoginEvent) {
        if self.accepts(event)
            && matches!(
                event.kind,
                CodexLoginEventKind::AlreadyAuthenticated
                    | CodexLoginEventKind::Completed
                    | CodexLoginEventKind::Failed
            )
        {
            self.cancellation = None;
        }
    }
}

impl Drop for CodexLoginController {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Channel endpoints owned by the terminal runner.
pub struct UiPorts {
    /// Bounded user-intent sender.
    pub intents: mpsc::Sender<UiIntent>,
    /// Latest session projection, coalesced by `watch`.
    pub sessions: watch::Receiver<Arc<SessionProjection>>,
    /// Latest all-sessions read model, coalesced by `watch`.
    pub session_lists: watch::Receiver<Arc<SessionsProjection>>,
    /// Latest catalog projection, coalesced by `watch`.
    pub catalogs: watch::Receiver<Arc<CatalogProjection>>,
    /// Latest local profile and provider-connection projection.
    pub profiles: watch::Receiver<Arc<ProfilesProjection>>,
    /// Latest resolved settings and provenance projection.
    pub settings: watch::Receiver<Arc<SettingsProjection>>,
    /// Bounded commit and rejection notices.
    pub notices: mpsc::Receiver<UiNotice>,
}

/// Channel endpoints owned by application composition.
pub struct AppPorts {
    /// Bounded user-intent receiver.
    pub intents: mpsc::Receiver<UiIntent>,
    /// Session projection publisher.
    pub sessions: watch::Sender<Arc<SessionProjection>>,
    /// All-sessions read-model publisher.
    pub session_lists: watch::Sender<Arc<SessionsProjection>>,
    /// Catalog projection publisher.
    pub catalogs: watch::Sender<Arc<CatalogProjection>>,
    /// Local profile and provider-connection publisher.
    pub profiles: watch::Sender<Arc<ProfilesProjection>>,
    /// Resolved settings and provenance publisher.
    pub settings: watch::Sender<Arc<SettingsProjection>>,
    /// Bounded commit and rejection publisher.
    pub notices: mpsc::Sender<UiNotice>,
}

/// Creates bounded/coalescing runner channels and their application endpoints.
#[must_use]
pub fn bounded_ports(
    session: Arc<SessionProjection>,
    session_list: Arc<SessionsProjection>,
    catalog: Arc<CatalogProjection>,
) -> (UiPorts, AppPorts) {
    let (intent_tx, intent_rx) = mpsc::channel(INTENT_CAPACITY);
    let (session_tx, session_rx) = watch::channel(session);
    let (session_list_tx, session_list_rx) = watch::channel(session_list);
    let (catalog_tx, catalog_rx) = watch::channel(catalog);
    let (profile_tx, profile_rx) = watch::channel(Arc::new(ProfilesProjection::default()));
    let (settings_tx, settings_rx) = watch::channel(Arc::new(SettingsProjection::default()));
    let (notice_tx, notice_rx) = mpsc::channel(APP_NOTICE_CAPACITY);
    (
        UiPorts {
            intents: intent_tx,
            sessions: session_rx,
            session_lists: session_list_rx,
            catalogs: catalog_rx,
            profiles: profile_rx,
            settings: settings_rx,
            notices: notice_rx,
        },
        AppPorts {
            intents: intent_rx,
            sessions: session_tx,
            session_lists: session_list_tx,
            catalogs: catalog_tx,
            profiles: profile_tx,
            settings: settings_tx,
            notices: notice_tx,
        },
    )
}

/// Reason the terminal loop returned normally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitReason {
    /// User requested quit.
    UserQuit,
    /// Application shutdown token was cancelled.
    Shutdown,
    /// Terminal input stream closed.
    InputClosed,
}

/// Failure in terminal drawing, input, or application coordination.
#[derive(Debug)]
pub enum RunnerError {
    /// Reading a Crossterm input event failed.
    Input(std::io::Error),
    /// Ratatui backend drawing failed.
    Draw(String),
    /// An application-owned channel closed unexpectedly.
    ApplicationDisconnected(&'static str),
}

impl Display for RunnerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(formatter, "terminal input failed: {error}"),
            Self::Draw(error) => write!(formatter, "terminal draw failed: {error}"),
            Self::ApplicationDisconnected(channel) => {
                write!(formatter, "application {channel} channel closed")
            }
        }
    }
}

impl Error for RunnerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Draw(_) | Self::ApplicationDisconnected(_) => None,
        }
    }
}

/// Runs the terminal input/update/draw loop without network or storage work.
pub async fn run<B>(
    terminal: &mut Terminal<B>,
    mut model: Model,
    ports: UiPorts,
    shutdown: CancellationToken,
) -> Result<ExitReason, RunnerError>
where
    B: Backend,
    B::Error: Display,
{
    let UiPorts {
        intents,
        mut sessions,
        mut session_lists,
        mut catalogs,
        mut profiles,
        mut settings,
        mut notices,
    } = ports;
    let mut events = EventStream::new();
    let mut frames = tokio::time::interval(Duration::from_millis(16));
    frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let started = Instant::now();
    let (codex_login_tx, mut codex_login_rx) = mpsc::unbounded_channel();
    let mut codex_login = CodexLoginController::new(codex_login_tx);

    draw(terminal, &mut model)?;
    #[cfg(feature = "benchmark-instrumentation")]
    crate::benchmark::first_draw_completed();

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                let _ = update(&mut model, Message::ShutdownRequested);
                return Ok(ExitReason::Shutdown);
            }
            terminal_event = events.next() => {
                let Some(terminal_event) = terminal_event else {
                    return Ok(ExitReason::InputClosed);
                };
                let terminal_event = terminal_event.map_err(RunnerError::Input)?;
                let size = terminal
                    .size()
                    .map_err(|error| RunnerError::Draw(error.to_string()))?;
                let effects = terminal_message(
                    terminal_event,
                    &model,
                    size.width,
                    size.height,
                )
                .map_or_else(Vec::new, |message| update(&mut model, message));
                if dispatch_effects(&mut model, effects, &intents, &mut codex_login) {
                    return Ok(ExitReason::UserQuit);
                }
            }
            result = sessions.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("session projection"))?;
                let session = Arc::clone(&sessions.borrow_and_update());
                let _ = update(&mut model, Message::SessionChanged(session));
            }
            result = session_lists.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("session list"))?;
                let list = Arc::clone(&session_lists.borrow_and_update());
                let _ = update(&mut model, Message::SessionsChanged(list));
            }
            result = catalogs.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("catalog projection"))?;
                let catalog = Arc::clone(&catalogs.borrow_and_update());
                let _ = update(&mut model, Message::CatalogChanged(catalog));
            }
            result = profiles.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("profiles projection"))?;
                let profiles = Arc::clone(&profiles.borrow_and_update());
                let _ = update(&mut model, Message::ProfilesChanged(profiles));
            }
            result = settings.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("settings projection"))?;
                let settings = Arc::clone(&settings.borrow_and_update());
                let _ = update(&mut model, Message::SettingsChanged(settings));
            }
            notice = notices.recv() => {
                let notice = notice.ok_or(RunnerError::ApplicationDisconnected("notice"))?;
                let _ = update(&mut model, Message::Notice(notice));
            }
            login_event = codex_login_rx.recv() => {
                let Some(login_event) = login_event else {
                    continue;
                };
                if codex_login.accepts(login_event) {
                    codex_login.settle(login_event);
                    let message = match login_event.kind {
                        CodexLoginEventKind::BrowserOpened => Message::CodexLoginBrowserOpened,
                        CodexLoginEventKind::AlreadyAuthenticated => {
                            Message::CodexLoginAlreadyAuthenticated
                        }
                        CodexLoginEventKind::Completed => Message::CodexLoginCompleted,
                        CodexLoginEventKind::Failed => Message::CodexLoginFailed,
                    };
                    let effects = update(&mut model, message);
                    if dispatch_effects(&mut model, effects, &intents, &mut codex_login) {
                        return Ok(ExitReason::UserQuit);
                    }
                }
            }
            _ = ticks.tick() => {
                let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let _ = update(&mut model, Message::Tick(elapsed));
            }
            _ = frames.tick() => {
                if model.dirty {
                    draw(terminal, &mut model)?;
                }
            }
        }
    }
}

fn terminal_message(
    event: Event,
    model: &crate::model::Model,
    width: u16,
    height: u16,
) -> Option<Message> {
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            Some(Message::Input(key.into()))
        }
        Event::Paste(text) => Some(Message::Paste(text)),
        Event::Resize(_, _) => Some(Message::Resize),
        Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) => {
            crate::view::hit_test(model, width, height, mouse.column, mouse.row).map(Message::Mouse)
        }
        Event::FocusGained | Event::FocusLost | Event::Key(_) | Event::Mouse(_) => None,
    }
}

fn dispatch_effects(
    model: &mut Model,
    effects: Vec<UiEffect>,
    intents: &mpsc::Sender<UiIntent>,
    codex_login: &mut CodexLoginController,
) -> bool {
    for effect in effects {
        match effect {
            UiEffect::Quit => return true,
            UiEffect::LaunchCodexLogin => {
                let executable = std::env::var_os("AUTOHARNESS_CODEX_EXECUTABLE")
                    .unwrap_or_else(|| std::ffi::OsString::from("codex"));
                if codex_login.launch(executable).is_err() {
                    let _ = update(model, Message::CodexLoginFailed);
                }
            }
            UiEffect::CancelCodexLogin => codex_login.cancel(),
            UiEffect::CopyTranscript(text) => {
                // OSC 52 copy; failure is non-fatal because terminals may
                // simply not advertise clipboard support.
                let _ = crossterm::execute!(
                    std::io::stdout(),
                    crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
                );
            }
            UiEffect::Dispatch(intent) => {
                let request_id = intent.request_id();
                #[cfg(feature = "benchmark-instrumentation")]
                if matches!(&intent, UiIntent::SubmitPrompt { .. }) {
                    crate::benchmark::input_accepted(request_id);
                }
                if let Err(error) = intents.try_send(intent) {
                    let (message, retry) = match error {
                        TrySendError::Full(_) => {
                            ("Application is busy; try again", RetryPolicy::Now)
                        }
                        TrySendError::Closed(_) => {
                            ("Application is no longer available", RetryPolicy::Never)
                        }
                    };
                    let _ = update(
                        model,
                        Message::Notice(UiNotice::IntentRejected {
                            request_id,
                            failure: UiFailure::new(ErrorClass::Unavailable, message, retry),
                        }),
                    );
                }
            }
        }
    }
    false
}

fn run_codex_login(
    executable: OsString,
    cancellation: CancellationToken,
    events: mpsc::UnboundedSender<CodexLoginEvent>,
    generation: u64,
) {
    match codex_login_status(&executable, &cancellation) {
        Ok(true) => {
            send_codex_login_event(
                &events,
                &cancellation,
                generation,
                CodexLoginEventKind::AlreadyAuthenticated,
            );
            return;
        }
        Ok(false) => {}
        Err(_) => {
            send_codex_login_event(
                &events,
                &cancellation,
                generation,
                CodexLoginEventKind::Failed,
            );
            return;
        }
    }

    let mut command = codex_login_command(&executable);
    let Ok(mut child) = command.spawn() else {
        send_codex_login_event(
            &events,
            &cancellation,
            generation,
            CodexLoginEventKind::Failed,
        );
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        send_codex_login_event(
            &events,
            &cancellation,
            generation,
            CodexLoginEventKind::Failed,
        );
        return;
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        send_codex_login_event(
            &events,
            &cancellation,
            generation,
            CodexLoginEventKind::Failed,
        );
        return;
    };

    let (output_tx, output_rx) = std_mpsc::channel();
    if drain_codex_login_output(stdout, output_tx.clone()).is_err()
        || drain_codex_login_output(stderr, output_tx).is_err()
    {
        terminate_child(&mut child);
        send_codex_login_event(
            &events,
            &cancellation,
            generation,
            CodexLoginEventKind::Failed,
        );
        return;
    }
    let deadline = Instant::now() + CODEX_LOGIN_TIMEOUT;
    let mut streams_open = 2_u8;
    let mut browser_reported = false;
    let mut exit_status = None;

    loop {
        if cancellation.is_cancelled() {
            terminate_child(&mut child);
            return;
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child);
            send_codex_login_event(
                &events,
                &cancellation,
                generation,
                CodexLoginEventKind::Failed,
            );
            return;
        }

        match output_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Some(line)) if !browser_reported && contains_codex_auth_url(&line) => {
                browser_reported = true;
                send_codex_login_event(
                    &events,
                    &cancellation,
                    generation,
                    CodexLoginEventKind::BrowserOpened,
                );
            }
            Ok(Some(_)) | Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Ok(None) => streams_open = streams_open.saturating_sub(1),
            Err(std_mpsc::RecvTimeoutError::Disconnected) => streams_open = 0,
        }

        if exit_status.is_none() {
            match child.try_wait() {
                Ok(status) => exit_status = status,
                Err(_) => {
                    terminate_child(&mut child);
                    send_codex_login_event(
                        &events,
                        &cancellation,
                        generation,
                        CodexLoginEventKind::Failed,
                    );
                    return;
                }
            }
        }
        if let Some(status) = exit_status
            && streams_open == 0
        {
            send_codex_login_event(
                &events,
                &cancellation,
                generation,
                if status.success() {
                    CodexLoginEventKind::Completed
                } else {
                    CodexLoginEventKind::Failed
                },
            );
            return;
        }
    }
}

fn codex_login_status(
    executable: &OsStr,
    cancellation: &CancellationToken,
) -> std::io::Result<bool> {
    let mut child = std::process::Command::new(executable)
        .args(["login", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    wait_for_child(&mut child, cancellation, CODEX_STATUS_TIMEOUT).map(|status| status.success())
}

fn codex_login_command(executable: &OsStr) -> std::process::Command {
    let mut command = std::process::Command::new(executable);
    command
        .arg("login")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn wait_for_child(
    child: &mut Child,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> std::io::Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancellation.is_cancelled() {
            terminate_child(child);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Codex login was cancelled",
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            terminate_child(child);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Codex login status timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn drain_codex_login_output(
    output: impl std::io::Read + Send + 'static,
    sender: std_mpsc::Sender<Option<String>>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("autoharness-codex-login-output".to_owned())
        .spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if sender.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = sender.send(None);
        })
        .map(|_| ())
}

fn contains_codex_auth_url(line: &str) -> bool {
    line.split_ascii_whitespace().any(|word| {
        word.starts_with(CODEX_AUTH_URL_PREFIX)
            && word.len() <= 16 * 1024
            && word.chars().all(|character| character.is_ascii_graphic())
    })
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn send_codex_login_event(
    events: &mpsc::UnboundedSender<CodexLoginEvent>,
    cancellation: &CancellationToken,
    generation: u64,
    kind: CodexLoginEventKind,
) {
    if !cancellation.is_cancelled() {
        let _ = events.send(CodexLoginEvent { generation, kind });
    }
}

fn draw<B>(terminal: &mut Terminal<B>, model: &mut Model) -> Result<(), RunnerError>
where
    B: Backend,
    B::Error: Display,
{
    terminal
        .draw(|frame| view(frame, model))
        .map_err(|error| RunnerError::Draw(error.to_string()))?;
    #[cfg(feature = "benchmark-instrumentation")]
    crate::benchmark::rendered_projection(&model.session.session_id, model.session.revision);
    model.dirty = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{ModelId, ModelRef, ProviderId};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui_textarea::{Input, Key};

    use super::*;
    use crate::model::{ModelSummary, MouseAction, Notice, PendingKind};

    fn selected_model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("test-provider").expect("valid provider ID"),
            ModelId::new("models/test-model").expect("valid model ID"),
        )
    }

    fn model_with_draft() -> Model {
        let selected = selected_model();
        let session = Arc::new(SessionProjection {
            session_id: "session-fixture".to_owned(),
            revision: 1,
            selected_model: Some(selected.clone()),
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        });
        let catalog = Arc::new(CatalogProjection::Ready {
            models: vec![ModelSummary {
                model: selected,
                display_name: "Test model".to_owned(),
                detail: String::new(),
                selectable: true,
            }],
            stale: false,
        });
        let mut model = Model::new(
            session,
            Arc::new(crate::model::SessionsProjection::default()),
            catalog,
        );
        let _ = update(&mut model, Message::Paste("draft survives".to_owned()));
        model
    }

    fn submit_effect(model: &mut Model) -> Vec<UiEffect> {
        update(
            model,
            Message::Input(Input {
                key: Key::Char('s'),
                ctrl: true,
                alt: false,
                shift: false,
            }),
        )
    }

    fn login_controller() -> CodexLoginController {
        let (sender, _receiver) = mpsc::unbounded_channel();
        CodexLoginController::new(sender)
    }

    #[test]
    fn full_intent_mailbox_becomes_an_explicit_rejection() {
        let mut model = model_with_draft();
        let effects = submit_effect(&mut model);
        let (sender, _receiver) = mpsc::channel(1);
        sender
            .try_send(UiIntent::RefreshCatalog {
                request_id: crate::model::RequestId::new(999),
            })
            .expect("fill mailbox");

        assert!(!dispatch_effects(
            &mut model,
            effects,
            &sender,
            &mut login_controller(),
        ));

        assert!(model.pending().is_empty());
        assert_eq!(model.composer.text(), "draft survives");
        assert!(matches!(
            &model.notice,
            Some(Notice::Failure(UiFailure {
                retry: RetryPolicy::Now,
                ..
            }))
        ));
        assert!(
            !model
                .pending()
                .values()
                .any(|pending| matches!(pending, PendingKind::SubmitPrompt(_)))
        );
    }

    #[test]
    fn closed_intent_mailbox_is_reported_as_non_retryable() {
        let mut model = model_with_draft();
        let effects = submit_effect(&mut model);
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);

        assert!(!dispatch_effects(
            &mut model,
            effects,
            &sender,
            &mut login_controller(),
        ));
        assert!(matches!(
            &model.notice,
            Some(Notice::Failure(UiFailure {
                retry: RetryPolicy::Never,
                ..
            }))
        ));
    }

    #[test]
    fn left_mouse_down_becomes_a_semantic_click() {
        let model = model_with_draft();
        let profile_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 23,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            terminal_message(profile_event, &model, 80, 24),
            Some(Message::Mouse(MouseAction::SettingsTab(2)))
        ));
        let settings_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 14,
            row: 23,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            terminal_message(settings_event, &model, 80, 24),
            Some(Message::Mouse(MouseAction::SettingsTab(0)))
        ));
    }

    #[test]
    fn non_click_mouse_events_are_ignored() {
        let model = model_with_draft();
        for kind in [
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Moved,
        ] {
            let event = Event::Mouse(MouseEvent {
                kind,
                column: 2,
                row: 0,
                modifiers: KeyModifiers::NONE,
            });
            assert!(terminal_message(event, &model, 80, 24).is_none());
        }
    }

    #[test]
    fn codex_login_runs_the_validated_executable_directly() {
        let executable = std::ffi::OsStr::new(r"C:\Program Files\Codex\codex.exe");
        let command = codex_login_command(executable);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), executable);
        assert_eq!(args, ["login"]);
    }

    #[test]
    fn codex_auth_url_detection_is_strict_and_bounded() {
        assert!(contains_codex_auth_url(
            "https://auth.openai.com/oauth/authorize?state=opaque"
        ));
        assert!(!contains_codex_auth_url(
            "https://example.test/oauth/authorize?state=opaque"
        ));
        assert!(!contains_codex_auth_url("not a URL"));
    }
}
