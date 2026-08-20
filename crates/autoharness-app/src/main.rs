mod config;
mod coordinator;
mod engine_actor;
mod error;
mod ids;
mod projection;
mod telemetry;
mod terminal;

use std::fs::OpenOptions;
use std::process::ExitCode;
use std::sync::Arc;

use autoharness_domain::ClassifiedError as _;
use autoharness_provider::{Provider, ProviderErrorKind};
use autoharness_provider_gemini::GeminiProvider;
use autoharness_tui::{CatalogProjection, Model, RetryPolicy, UiFailure, bounded_ports};
use config::{AppPaths, WriterLease};
use coordinator::Coordinator;
use engine_actor::EngineActor;
use error::AppError;
use terminal::TerminalGuard;
use tokio_util::sync::CancellationToken;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("AutoHarness: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), AppError> {
    let paths = AppPaths::prepare()?;
    let _writer_lease = WriterLease::acquire(&paths.writer_lock())?;
    let _trace_guard = initialize_tracing(&paths)?;
    telemetry::app_started();
    let (engine_actor, session_id, session) = EngineActor::start(paths.database())?;

    let (provider, initial_catalog) = match GeminiProvider::from_env() {
        Ok(provider) => {
            telemetry::provider_ready();
            (
                Some(Arc::new(provider) as Arc<dyn Provider>),
                CatalogProjection::Loading,
            )
        }
        Err(error) => {
            telemetry::provider_unavailable(&error);
            if error.kind() == ProviderErrorKind::MissingCredential {
                (None, CatalogProjection::CredentialRequired)
            } else {
                let failure = UiFailure::new(
                    error.class(),
                    error.to_string(),
                    RetryPolicy::from_advice(error.retry_advice(), 0),
                );
                (None, CatalogProjection::Failed(failure))
            }
        }
    };

    let initial_session = Arc::new(projection::session(&session));
    let initial_catalog = Arc::new(initial_catalog);
    let model = Model::new(Arc::clone(&initial_session), Arc::clone(&initial_catalog));
    let (ui_ports, app_ports) = bounded_ports(initial_session, initial_catalog);
    let shutdown = CancellationToken::new();

    let mut terminal = match TerminalGuard::enter() {
        Ok(terminal) => terminal,
        Err(error) => {
            engine_actor.shutdown().await?;
            return Err(error);
        }
    };

    let coordinator = Coordinator::new(
        session_id,
        session,
        engine_actor.handle(),
        provider,
        app_ports,
        shutdown.clone(),
    );
    let coordinator_task = tokio::spawn(coordinator.run());
    let signal_task = spawn_signal_handler(shutdown.clone());

    let ui_result =
        autoharness_tui::run(terminal.terminal_mut(), model, ui_ports, shutdown.clone()).await;
    terminal.restore();
    shutdown.cancel();

    let coordinator_result = match coordinator_task.await {
        Ok(result) => result,
        Err(_) => Err(AppError::WorkerStopped),
    };
    signal_task.abort();
    let engine_result = engine_actor.shutdown().await;

    coordinator_result?;
    engine_result?;
    ui_result.map_err(|_| AppError::Terminal)?;
    telemetry::app_stopped();
    Ok(())
}

fn initialize_tracing(paths: &AppPaths) -> Result<WorkerGuard, AppError> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log())
        .map_err(|_| AppError::FileSystem)?;
    let (writer, guard) = tracing_appender::non_blocking(log);
    let filter = EnvFilter::new(config::log_filter_directive()?);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer)
        .try_init()
        .map_err(|_| AppError::Configuration)?;
    Ok(guard)
}

fn spawn_signal_handler(shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if result.is_ok() {
                    shutdown.cancel();
                }
            }
            () = shutdown.cancelled() => {}
        }
    })
}
