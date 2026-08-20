use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

use autoharness_domain::ErrorClass;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt as _;
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::model::{
    CatalogProjection, Message, Model, RetryPolicy, SessionProjection, UiEffect, UiFailure,
    UiIntent, UiNotice,
};
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
    /// Latest catalog projection, coalesced by `watch`.
    pub catalogs: watch::Receiver<Arc<CatalogProjection>>,
    /// Bounded commit and rejection notices.
    pub notices: mpsc::Receiver<UiNotice>,
}

/// Channel endpoints owned by application composition.
pub struct AppPorts {
    /// Bounded user-intent receiver.
    pub intents: mpsc::Receiver<UiIntent>,
    /// Session projection publisher.
    pub sessions: watch::Sender<Arc<SessionProjection>>,
    /// Catalog projection publisher.
    pub catalogs: watch::Sender<Arc<CatalogProjection>>,
    /// Bounded commit and rejection publisher.
    pub notices: mpsc::Sender<UiNotice>,
}

/// Creates bounded/coalescing runner channels and their application endpoints.
#[must_use]
pub fn bounded_ports(
    session: Arc<SessionProjection>,
    catalog: Arc<CatalogProjection>,
) -> (UiPorts, AppPorts) {
    let (intent_tx, intent_rx) = mpsc::channel(INTENT_CAPACITY);
    let (session_tx, session_rx) = watch::channel(session);
    let (catalog_tx, catalog_rx) = watch::channel(catalog);
    let (notice_tx, notice_rx) = mpsc::channel(APP_NOTICE_CAPACITY);
    (
        UiPorts {
            intents: intent_tx,
            sessions: session_rx,
            catalogs: catalog_rx,
            notices: notice_rx,
        },
        AppPorts {
            intents: intent_rx,
            sessions: session_tx,
            catalogs: catalog_tx,
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
        mut catalogs,
        mut notices,
    } = ports;
    let mut events = EventStream::new();
    let mut frames = tokio::time::interval(Duration::from_millis(16));
    frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let started = Instant::now();

    draw(terminal, &mut model)?;

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
                let effects = terminal_message(terminal_event)
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
            result = catalogs.changed() => {
                result.map_err(|_| RunnerError::ApplicationDisconnected("catalog projection"))?;
                let catalog = Arc::clone(&catalogs.borrow_and_update());
                let _ = update(&mut model, Message::CatalogChanged(catalog));
            }
            notice = notices.recv() => {
                let notice = notice.ok_or(RunnerError::ApplicationDisconnected("notice"))?;
                let _ = update(&mut model, Message::Notice(notice));
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

fn terminal_message(event: Event) -> Option<Message> {
    match event {
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            Some(Message::Input(key.into()))
        }
        Event::Paste(text) => Some(Message::Paste(text)),
        Event::Resize(_, _) => Some(Message::Resize),
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
            UiEffect::Dispatch(intent) => {
                let request_id = intent.request_id();
                if let Err(error) = intents.try_send(intent) {
                    let (message, retry) = match error {
                        TrySendError::Full(_) => (
                            "Application is busy; the request was not queued",
                            RetryPolicy::Now,
                        ),
                        TrySendError::Closed(_) => {
                            ("Application command channel is closed", RetryPolicy::Never)
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

fn draw<B>(terminal: &mut Terminal<B>, model: &mut Model) -> Result<(), RunnerError>
where
    B: Backend,
    B::Error: Display,
{
    terminal
        .draw(|frame| view(frame, model))
        .map_err(|error| RunnerError::Draw(error.to_string()))?;
    model.dirty = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{ModelId, ModelRef, ProviderId};
    use ratatui_textarea::{Input, Key};

    use super::*;
    use crate::model::{ModelSummary, Notice, PendingKind};

    fn selected_model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("test-provider").expect("valid provider ID"),
            ModelId::new("models/test-model").expect("valid model ID"),
        )
    }

    fn model_with_draft() -> Model {
        let selected = selected_model();
        let session = Arc::new(SessionProjection {
            revision: 1,
            selected_model: Some(selected.clone()),
            transcript: Vec::new(),
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
        let mut model = Model::new(session, catalog);
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
}
