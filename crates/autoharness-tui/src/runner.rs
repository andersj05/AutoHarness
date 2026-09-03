use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    CatalogProjection, MemoryProjection, Message, Model, MouseAction, ProfilesProjection,
    RetryPolicy, SessionProjection, SessionsProjection, SettingsProjection, UiClock, UiEffect,
    UiFailure, UiIntent, UiNotice,
};
use crate::ui::ColorDepth;
use crate::{update, view};

/// Maximum queued user intents before explicit backpressure is presented.
pub const INTENT_CAPACITY: usize = 32;
/// Maximum queued application notices before their producer is backpressured.
pub const APP_NOTICE_CAPACITY: usize = 128;
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
    /// Latest bounded memory read model, coalesced by `watch`.
    pub memories: watch::Receiver<Arc<MemoryProjection>>,
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
    /// Bounded memory read-model publisher.
    pub memories: watch::Sender<Arc<MemoryProjection>>,
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
    let (memory_tx, memory_rx) = watch::channel(Arc::new(MemoryProjection::default()));
    let (notice_tx, notice_rx) = mpsc::channel(APP_NOTICE_CAPACITY);
    (
        UiPorts {
            intents: intent_tx,
            sessions: session_rx,
            session_lists: session_list_rx,
            catalogs: catalog_rx,
            profiles: profile_rx,
            settings: settings_rx,
            memories: memory_rx,
            notices: notice_rx,
        },
        AppPorts {
            intents: intent_rx,
            sessions: session_tx,
            session_lists: session_list_tx,
            catalogs: catalog_tx,
            profiles: profile_tx,
            settings: settings_tx,
            memories: memory_tx,
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
        mut memories,
        mut notices,
    } = ports;
    let mut events = EventStream::new();
    let mut frames = tokio::time::interval(Duration::from_millis(16));
    frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let started = Instant::now();
    model.set_color_depth(ColorDepth::detect());

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
                if dispatch_effects(&mut model, effects, &intents) {
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
            result = memories.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("memory projection"))?;
                let memory = Arc::clone(&memories.borrow_and_update());
                let _ = update(&mut model, Message::MemoryChanged(memory));
            }
            notice = notices.recv() => {
                let notice = notice.ok_or(RunnerError::ApplicationDisconnected("notice"))?;
                let _ = update(&mut model, Message::Notice(notice));
            }
            _ = ticks.tick() => {
                let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let wall_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|elapsed_wall| i64::try_from(elapsed_wall.as_millis()).ok())
                    .unwrap_or(0);
                if update_and_dispatch(
                    &mut model,
                    Message::Tick(UiClock::new(elapsed, wall_ms)),
                    &intents,
                ) {
                    return Ok(ExitReason::UserQuit);
                }
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
        Event::Mouse(mouse)
            if matches!(
                crate::view::hit_test(model, width, height, mouse.column, mouse.row),
                Some(MouseAction::FocusTranscript | MouseAction::FocusComposer)
            ) =>
        {
            match mouse.kind {
                MouseEventKind::ScrollUp => Some(Message::TranscriptScroll(3)),
                MouseEventKind::ScrollDown => Some(Message::TranscriptScroll(-3)),
                _ => None,
            }
        }
        Event::FocusGained | Event::FocusLost | Event::Key(_) | Event::Mouse(_) => None,
    }
}

fn dispatch_effects(
    model: &mut Model,
    effects: Vec<UiEffect>,
    intents: &mpsc::Sender<UiIntent>,
) -> bool {
    for effect in effects {
        match effect {
            UiEffect::Quit => return true,
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

fn update_and_dispatch(
    model: &mut Model,
    message: Message,
    intents: &mpsc::Sender<UiIntent>,
) -> bool {
    let effects = update(model, message);
    dispatch_effects(model, effects, intents)
}

fn draw<B>(terminal: &mut Terminal<B>, model: &mut Model) -> Result<(), RunnerError>
where
    B: Backend,
    B::Error: Display,
{
    terminal
        .draw(|frame| {
            if let Some(transcript) = crate::ui::layout::chat_transcript_rect(frame.area(), model) {
                crate::ui::page::chat::normalize_scroll(transcript, model);
            }
            view(frame, model);
        })
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
                context_window_tokens: Some(32_000),
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

    #[test]
    fn memory_projection_mailbox_coalesces_to_the_latest_generation() {
        let model = model_with_draft();
        let (mut ui, app) = bounded_ports(
            Arc::clone(&model.session),
            Arc::clone(&model.sessions),
            Arc::clone(&model.catalog),
        );

        app.memories
            .send_replace(Arc::new(MemoryProjection::loading(1)));
        app.memories
            .send_replace(Arc::new(MemoryProjection::loading(2)));

        assert!(ui.memories.has_changed().expect("memory channel open"));
        assert_eq!(ui.memories.borrow_and_update().generation(), 2);
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

        assert!(!dispatch_effects(&mut model, effects, &sender));

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

        assert!(!dispatch_effects(&mut model, effects, &sender));
        assert!(matches!(
            &model.notice,
            Some(Notice::Failure(UiFailure {
                retry: RetryPolicy::Never,
                ..
            }))
        ));
    }

    #[test]
    fn timer_tick_dispatches_a_due_memory_query_to_the_application() {
        let mut model = model_with_draft();
        let (sender, mut receiver) = mpsc::channel(1);
        let _ = update(
            &mut model,
            Message::Input(Input {
                key: Key::Char('6'),
                ctrl: false,
                alt: true,
                shift: false,
            }),
        );
        let _ = update(
            &mut model,
            Message::Input(Input {
                key: Key::Char('/'),
                ctrl: false,
                alt: false,
                shift: false,
            }),
        );
        let _ = update(&mut model, Message::Paste("durable evidence".to_owned()));

        assert!(!update_and_dispatch(
            &mut model,
            Message::Tick(UiClock::new(149, 1_725_000_000_000)),
            &sender,
        ));
        assert!(receiver.try_recv().is_err());
        assert!(!update_and_dispatch(
            &mut model,
            Message::Tick(UiClock::new(150, 1_725_000_000_000)),
            &sender,
        ));
        let intent = receiver.try_recv().expect("due query dispatched");
        assert!(matches!(
            intent,
            UiIntent::QueryMemory { query, .. }
                if query.literal() == "durable evidence"
        ));
    }

    #[test]
    fn left_mouse_down_becomes_a_semantic_click() {
        let model = model_with_draft();
        let profile_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            terminal_message(profile_event, &model, 80, 24),
            Some(Message::Mouse(MouseAction::FocusComposer))
        ));
        let settings_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 14,
            row: 2,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            terminal_message(settings_event, &model, 80, 24),
            Some(Message::Mouse(MouseAction::FocusComposer))
        ));
    }

    #[test]
    fn mouse_wheel_over_chat_becomes_transcript_scroll() {
        let model = model_with_draft();
        for (kind, expected) in [
            (MouseEventKind::ScrollUp, Message::TranscriptScroll(3)),
            (MouseEventKind::ScrollDown, Message::TranscriptScroll(-3)),
        ] {
            let event = Event::Mouse(MouseEvent {
                kind,
                column: 10,
                row: 2,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(
                format!("{:?}", terminal_message(event, &model, 80, 24)),
                format!("Some({expected:?})")
            );
        }
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
}
