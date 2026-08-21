mod catalog_cache;
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
use autoharness_provider::{
    CatalogCache, ManagedProvider, Provider, ProviderError, ProviderErrorKind, ProviderPolicy,
};
use autoharness_provider_gemini::{GeminiApiKey, GeminiProvider};
use autoharness_provider_openai::{OpenAiRouterProvider, RouterCredential, RouterSettings};
use autoharness_tui::{
    ApiCredential, CatalogProjection, Model, RetryPolicy, UiFailure, bounded_ports,
};
use catalog_cache::SqliteCatalogCache;
use config::{AppPaths, ProviderSelection, WriterLease};
use coordinator::{Coordinator, ProviderFactory};
use engine_actor::EngineActor;
use error::AppError;
use terminal::TerminalGuard;
use tokio_util::sync::CancellationToken;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

struct ProviderComposition {
    initial: Option<Arc<dyn Provider>>,
    factory: ProviderFactory,
    catalog: CatalogProjection,
}

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
    let cache: Arc<dyn CatalogCache> = Arc::new(SqliteCatalogCache::open(paths.database())?);
    let policy = config::provider_policy()?;
    let provider = configure_provider(Arc::clone(&cache), policy)?;
    let (engine_actor, session_id, session) = EngineActor::start(paths.database())?;

    let initial_session = Arc::new(projection::session(&session));
    let initial_catalog = Arc::new(provider.catalog);
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

    let coordinator = Coordinator::with_provider_factory(
        session_id,
        session,
        engine_actor.handle(),
        provider.initial,
        provider.factory,
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

fn configure_provider(
    cache: Arc<dyn CatalogCache>,
    policy: ProviderPolicy,
) -> Result<ProviderComposition, AppError> {
    let (initial, factory): (Result<Arc<dyn Provider>, ProviderError>, ProviderFactory) =
        match config::provider_selection()? {
            ProviderSelection::Gemini => {
                let factory_cache = Arc::clone(&cache);
                let factory_policy = policy.clone();
                let factory: ProviderFactory = Arc::new(move |credential: ApiCredential| {
                    let provider: Arc<dyn Provider> = Arc::new(GeminiProvider::new(
                        GeminiApiKey::new(credential.into_string())?,
                    )?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&factory_cache),
                        factory_policy.clone(),
                    ))
                });
                let initial = GeminiApiKey::from_env().and_then(|key| {
                    let provider: Arc<dyn Provider> = Arc::new(GeminiProvider::new(key)?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&cache),
                        policy.clone(),
                    ))
                });
                (initial, factory)
            }
            ProviderSelection::Router => {
                let settings = RouterSettings::from_env().map_err(|_| AppError::Configuration)?;
                let factory_settings = settings.clone();
                let factory_cache = Arc::clone(&cache);
                let factory_policy = policy.clone();
                let factory: ProviderFactory = Arc::new(move |credential: ApiCredential| {
                    let provider: Arc<dyn Provider> = Arc::new(OpenAiRouterProvider::new(
                        factory_settings.clone(),
                        RouterCredential::new(credential.into_string())?,
                    )?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&factory_cache),
                        factory_policy.clone(),
                    ))
                });
                let initial = RouterCredential::from_env().and_then(|credential| {
                    let provider: Arc<dyn Provider> =
                        Arc::new(OpenAiRouterProvider::new(settings.clone(), credential)?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&cache),
                        policy.clone(),
                    ))
                });
                (initial, factory)
            }
        };

    let (provider, catalog) = match initial {
        Ok(provider) => {
            telemetry::provider_ready();
            (Some(provider), CatalogProjection::Loading)
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
    Ok(ProviderComposition {
        initial: provider,
        factory,
        catalog,
    })
}

fn managed_provider(
    provider: Arc<dyn Provider>,
    cache: Arc<dyn CatalogCache>,
    policy: ProviderPolicy,
) -> Arc<dyn Provider> {
    Arc::new(ManagedProvider::new(provider, cache, policy))
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
