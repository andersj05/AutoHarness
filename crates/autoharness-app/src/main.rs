mod catalog_cache;
mod config;
mod coordinator;
mod engine_actor;
mod error;
mod export;
mod ids;
mod projection;
mod telemetry;
mod terminal;

use std::env;
use std::fs::OpenOptions;
use std::process::ExitCode;
use std::sync::Arc;

use autoharness_app::credential::CredentialSourceName;
use autoharness_app::profiles::{ProfileManager, ProfileStore};
use autoharness_app::vault::{KeyringVault, VaultPort};
use autoharness_domain::{ClassifiedError as _, RetryAdvice};
use autoharness_provider::{
    CatalogCache, ManagedProvider, Provider, ProviderError, ProviderErrorKind, ProviderPolicy,
};
use autoharness_provider_codex_cli::{CodexCredentialPersistence, CodexProvider, CodexSettings};
use autoharness_provider_gemini::{GeminiApiKey, GeminiProvider};
use autoharness_provider_openai::{OpenAiRouterProvider, RouterCredential, RouterSettings};
use autoharness_settings::{LayerKind, ProfileId, ProviderKind, ProviderProfile, SettingsBuilder};
use autoharness_tool::{
    FileArtifactStore, LocalFilesystem, LocalHttp, LocalProcess, PermissionPolicy, ToolRuntime,
};
use autoharness_tui::{
    ApiCredential, CatalogProjection, CredentialSourceLabel, Model, ProviderKindLabel,
    ProviderStatusProjection, RetryPolicy, SessionsProjection, SettingsProjection, UiFailure,
    bounded_ports,
};
use catalog_cache::SqliteCatalogCache;
use config::{AppPaths, WriterLease};
use coordinator::{
    Coordinator, EnvironmentCredentials, ProfileProviderFactory, ProfileRuntime, ProviderFactory,
    RuntimeComposition,
};
use engine_actor::EngineActor;
use error::AppError;
use terminal::TerminalGuard;
use tokio_util::sync::CancellationToken;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use zeroize::Zeroizing;

struct ConfiguredProvider {
    composition: coordinator::ProviderComposition,
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
    #[cfg(feature = "benchmark-instrumentation")]
    autoharness_tui::benchmark::initialize();
    let paths = AppPaths::prepare()?;
    let _writer_lease = WriterLease::acquire(&paths.writer_lock())?;
    let _trace_guard = initialize_tracing(&paths)?;
    telemetry::app_started();
    let cache: Arc<dyn CatalogCache> = Arc::new(SqliteCatalogCache::open(paths.database())?);
    let policy = config::provider_policy()?;
    let profile_store = ProfileStore::open(&paths.profiles()).map_err(|_| AppError::FileSystem)?;
    let initial_local_profile = profile_store
        .resolved_settings()
        .map(|settings| settings.local_profile().clone())
        .unwrap_or_default();
    let vault: Arc<dyn VaultPort> = Arc::new(KeyringVault::new());
    let resolved = resolve_launch(&profile_store, vault.as_ref());
    let profile_manager = Arc::new(ProfileManager::new(profile_store, vault));
    let provider = configure_provider(
        Arc::clone(&cache),
        policy.clone(),
        &resolved,
        Arc::clone(&profile_manager),
    )
    .await?;
    let profile_runtime = ProfileRuntime::new(
        Arc::clone(&profile_manager),
        configure_profile_provider_factory(Arc::clone(&cache), policy, profile_manager),
        environment_credentials(),
        config::workspace_root()?.display().to_string(),
    );
    let tool_runtime = configure_tool_runtime(&paths)?;
    let (engine_actor, session_id, session) = EngineActor::start(paths.database())?;

    let initial_session = Arc::new(projection::session(&session));
    let initial_catalog = Arc::new(provider.catalog);
    let initial_sessions = Arc::new(SessionsProjection::default());
    let initial_settings = Arc::new(settings_projection(&resolved, initial_local_profile));
    let model = Model::new(
        Arc::clone(&initial_session),
        Arc::clone(&initial_sessions),
        Arc::clone(&initial_catalog),
    );
    let mut model = model;
    model.apply_settings(Arc::clone(&initial_settings));
    let (ui_ports, app_ports) = bounded_ports(initial_session, initial_sessions, initial_catalog);
    let shutdown = CancellationToken::new();

    let mut terminal = match TerminalGuard::enter() {
        Ok(terminal) => terminal,
        Err(error) => {
            engine_actor.shutdown().await?;
            return Err(error);
        }
    };

    let coordinator = Coordinator::with_runtime(
        session_id,
        session,
        engine_actor.handle(),
        RuntimeComposition {
            provider: provider.composition,
            profiles: Some(profile_runtime),
            tool_runtime,
        },
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

/// Everything resolved before any UI or provider construction begins.
pub(crate) struct LaunchResolution {
    /// Effective credential source in safe terms.
    pub source: CredentialSourceName,
    /// Active profile identity, when one applies.
    pub active_profile: Option<String>,
    /// Provider kind selected by the active profile.
    pub provider_kind: Option<autoharness_settings::ProviderKind>,
    /// Provider-native reasoning effort selected by the active profile.
    pub active_profile_reasoning_effort: Option<String>,
    /// Credential bytes when one resolved; empty for session-only launches.
    pub credential: Zeroizing<String>,
    /// Non-secret router connection fields from the active profile.
    pub router: Option<RouterProfileFields>,
}

/// Non-secret router connection fields carried by a profile.
pub(crate) struct RouterProfileFields {
    pub base_url: String,
    pub project: Option<String>,
    pub auth_header: Option<String>,
}

impl RouterProfileFields {
    fn build_settings(&self) -> Result<RouterSettings, AppError> {
        // RouterSettings validates the URL; parse through the adapter's
        // re-exported URL type.
        let url = self
            .base_url
            .parse::<autoharness_provider_openai::RouterUrl>()
            .map_err(|_| AppError::Configuration)?;
        let mut settings = RouterSettings::new(url, self.project.as_deref())
            .map_err(|_| AppError::Configuration)?;
        if let Some(header) = &self.auth_header {
            settings = settings
                .with_authentication(header, "Bearer")
                .map_err(|_| AppError::Configuration)?;
        }
        Ok(settings)
    }
}

fn resolve_launch(store: &ProfileStore, vault: &dyn VaultPort) -> LaunchResolution {
    // Layered resolution: user profile document plus live environment.
    let document = store.read_document().unwrap_or_default();
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, document)
        .with_environment(env::vars())
        .resolve();
    match settings {
        Ok(settings) => {
            let resolver = autoharness_app::ProfileCredentialResolver::new(vault)
                .with_environment(env::vars());
            match resolver.resolve(&settings) {
                Ok(source) => {
                    let active_profile = source.profile_id().map(str::to_owned);
                    let active_profile_reasoning_effort = active_profile
                        .as_deref()
                        .and_then(|id| ProfileId::new(id).ok())
                        .and_then(|id| settings.profile(&id))
                        .and_then(ProviderProfile::default_reasoning_effort)
                        .map(str::to_owned);
                    LaunchResolution {
                        source: source.source_name(),
                        active_profile,
                        provider_kind: source.provider_kind(),
                        active_profile_reasoning_effort,
                        credential: source.into_credential(),
                        router: router_fields(&settings),
                    }
                }
                Err(_) => session_only_launch(&settings),
            }
        }
        // Malformed settings degrade to environment/session-only operation.
        Err(_) => environment_launch(),
    }
}

fn router_fields(settings: &autoharness_settings::ResolvedSettings) -> Option<RouterProfileFields> {
    let id = autoharness_settings::ProfileId::new(settings.active_profile()?).ok()?;
    let profile = settings.profile(&id)?;
    if profile.kind() != autoharness_settings::ProviderKind::Router {
        return None;
    }
    Some(RouterProfileFields {
        base_url: profile.base_url()?.to_owned(),
        project: profile.project().map(str::to_owned),
        auth_header: profile.auth_header().map(str::to_owned),
    })
}

fn session_only_launch(settings: &autoharness_settings::ResolvedSettings) -> LaunchResolution {
    let active_profile = settings.active_profile().map(str::to_owned);
    let profile_kind = settings.active_profile().and_then(|profile| {
        let id = autoharness_settings::ProfileId::new(profile).ok()?;
        settings.profile(&id).map(|profile| profile.kind())
    });
    LaunchResolution {
        source: CredentialSourceName::SessionOnly,
        active_profile,
        provider_kind: profile_kind
            .or(settings.provider())
            .or(Some(autoharness_settings::ProviderKind::Gemini)),
        active_profile_reasoning_effort: settings
            .active_profile()
            .and_then(|id| ProfileId::new(id).ok())
            .and_then(|id| settings.profile(&id))
            .and_then(ProviderProfile::default_reasoning_effort)
            .map(str::to_owned),
        credential: Zeroizing::new(String::new()),
        router: router_fields(settings),
    }
}

fn environment_launch() -> LaunchResolution {
    let provider_kind = config::provider_selection()
        .ok()
        .map(|selection| match selection {
            config::ProviderSelection::Gemini => autoharness_settings::ProviderKind::Gemini,
            config::ProviderSelection::Router => autoharness_settings::ProviderKind::Router,
            config::ProviderSelection::CodexCli => autoharness_settings::ProviderKind::CodexCli,
        });
    let credential = provider_kind
        .and_then(|kind| match kind {
            autoharness_settings::ProviderKind::Gemini => env::var("GEMINI_API_KEY").ok(),
            autoharness_settings::ProviderKind::Router => {
                env::var("AUTOHARNESS_ROUTER_API_KEY").ok()
            }
            autoharness_settings::ProviderKind::CodexCli => None,
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default();
    LaunchResolution {
        source: if credential.is_empty() {
            CredentialSourceName::SessionOnly
        } else {
            CredentialSourceName::Environment
        },
        active_profile: None,
        provider_kind,
        active_profile_reasoning_effort: None,
        credential: Zeroizing::new(credential),
        router: None,
    }
}

fn settings_projection(
    resolved: &LaunchResolution,
    local_profile: autoharness_settings::EffectiveLocalProfile,
) -> SettingsProjection {
    SettingsProjection {
        provider_status: ProviderStatusProjection {
            active_profile: resolved.active_profile.clone(),
            provider_kind: resolved.provider_kind.map(|kind| match kind {
                autoharness_settings::ProviderKind::Gemini => ProviderKindLabel::Gemini,
                autoharness_settings::ProviderKind::Router => ProviderKindLabel::Router,
                autoharness_settings::ProviderKind::CodexCli => ProviderKindLabel::CodexCli,
            }),
            credential_source: match resolved.source {
                CredentialSourceName::Environment => CredentialSourceLabel::Environment,
                CredentialSourceName::CredentialVault => CredentialSourceLabel::CredentialVault,
                CredentialSourceName::SessionOnly => CredentialSourceLabel::SessionOnly,
            },
            credential_connected: !resolved.credential.is_empty(),
        },
        local_profile,
        git_branch: None,
    }
}

fn configure_tool_runtime(paths: &AppPaths) -> Result<Arc<ToolRuntime>, AppError> {
    let workspace = config::workspace_root()?;
    let filesystem = Arc::new(
        LocalFilesystem::new(&workspace, 4 * 1024 * 1024).map_err(|_| AppError::Configuration)?,
    );
    let process =
        Arc::new(LocalProcess::new(&workspace, 1024 * 1024).map_err(|_| AppError::Configuration)?);
    let http = Arc::new(LocalHttp::new(4 * 1024 * 1024).map_err(|_| AppError::Configuration)?);
    let artifacts =
        Arc::new(FileArtifactStore::new(paths.artifacts()).map_err(|_| AppError::FileSystem)?);
    ToolRuntime::new(
        filesystem,
        process,
        http,
        artifacts,
        PermissionPolicy::local_default(),
        2,
        std::time::Duration::from_secs(120),
        64 * 1024,
    )
    .map(Arc::new)
    .map_err(|_| AppError::Configuration)
}

async fn configure_provider(
    cache: Arc<dyn CatalogCache>,
    policy: ProviderPolicy,
    resolved: &LaunchResolution,
    profile_manager: Arc<ProfileManager>,
) -> Result<ConfiguredProvider, AppError> {
    // A profile selects its adapter; otherwise the environment variable wins.
    let selection = match resolved.provider_kind {
        Some(kind) => Ok(match kind {
            autoharness_settings::ProviderKind::Gemini => config::ProviderSelection::Gemini,
            autoharness_settings::ProviderKind::Router => config::ProviderSelection::Router,
            autoharness_settings::ProviderKind::CodexCli => config::ProviderSelection::CodexCli,
        }),
        None => config::provider_selection(),
    };
    let (initial, factory): (Result<Arc<dyn Provider>, ProviderError>, ProviderFactory) =
        match selection? {
            config::ProviderSelection::Gemini => {
                let factory_cache = Arc::clone(&cache);
                let factory_policy = policy.clone();
                let factory: ProviderFactory = Arc::new(move |credential: ApiCredential| {
                    let api_key = GeminiApiKey::new(credential.into_string())?;
                    let provider: Arc<dyn Provider> = Arc::new(GeminiProvider::new(api_key)?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&factory_cache),
                        factory_policy.clone(),
                    ))
                });
                let initial = if resolved.credential.is_empty() {
                    GeminiApiKey::from_env().and_then(|key| {
                        let provider: Arc<dyn Provider> = Arc::new(GeminiProvider::new(key)?);
                        Ok(provider)
                    })
                } else {
                    GeminiApiKey::new(resolved.credential.as_str()).and_then(|key| {
                        let provider: Arc<dyn Provider> = Arc::new(GeminiProvider::new(key)?);
                        Ok(provider)
                    })
                };
                (initial, factory)
            }
            config::ProviderSelection::Router => {
                let settings = match &resolved.router {
                    Some(fields) => fields.build_settings()?,
                    None => RouterSettings::from_env().map_err(|_| AppError::Configuration)?,
                };
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
                let initial = if resolved.credential.is_empty() {
                    RouterCredential::from_env().and_then(|credential| {
                        let provider: Arc<dyn Provider> =
                            Arc::new(OpenAiRouterProvider::new(settings.clone(), credential)?);
                        Ok(managed_provider(
                            provider,
                            Arc::clone(&cache),
                            policy.clone(),
                        ))
                    })
                } else {
                    RouterCredential::new(resolved.credential.as_str()).and_then(|credential| {
                        let provider: Arc<dyn Provider> =
                            Arc::new(OpenAiRouterProvider::new(settings.clone(), credential)?);
                        Ok(managed_provider(
                            provider,
                            Arc::clone(&cache),
                            policy.clone(),
                        ))
                    })
                };
                (initial, factory)
            }
            config::ProviderSelection::CodexCli => {
                let factory_cache = Arc::clone(&cache);
                let factory_policy = policy.clone();
                let factory: ProviderFactory = Arc::new(move |credential: ApiCredential| {
                    let credential = Zeroizing::new(credential.into_string());
                    let provider: Arc<dyn Provider> = Arc::new(CodexProvider::new(
                        CodexSettings::new()?,
                        &credential,
                        None,
                    )?);
                    Ok(managed_provider(
                        provider,
                        Arc::clone(&factory_cache),
                        factory_policy.clone(),
                    ))
                });
                let initial = if resolved.credential.is_empty() {
                    Err(ProviderError::new(
                        ProviderErrorKind::MissingCredential,
                        RetryAdvice::Never,
                    ))
                } else {
                    let persistence = resolved
                        .active_profile
                        .as_deref()
                        .and_then(|id| ProfileId::new(id).ok())
                        .map(|id| codex_persistence(Arc::clone(&profile_manager), id));
                    CodexProvider::new(
                        codex_settings_for_launch(resolved)?,
                        &resolved.credential,
                        persistence,
                    )
                    .map(|provider| {
                        let provider: Arc<dyn Provider> = Arc::new(provider);
                        managed_provider(provider, Arc::clone(&cache), policy.clone())
                    })
                };
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
    Ok(ConfiguredProvider {
        composition: coordinator::ProviderComposition {
            initial: provider,
            factory,
        },
        catalog,
    })
}

fn environment_credentials() -> EnvironmentCredentials {
    EnvironmentCredentials {
        gemini: env::var("GEMINI_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Zeroizing::new),
        router: env::var("AUTOHARNESS_ROUTER_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Zeroizing::new),
    }
}

fn configure_profile_provider_factory(
    cache: Arc<dyn CatalogCache>,
    policy: ProviderPolicy,
    manager: Arc<ProfileManager>,
) -> ProfileProviderFactory {
    Arc::new(move |profile_id, profile, credential| {
        let provider: Arc<dyn Provider> = match profile.kind() {
            ProviderKind::Gemini => {
                let key = GeminiApiKey::new(credential.as_str())?;
                Arc::new(GeminiProvider::new(key)?)
            }
            ProviderKind::Router => {
                let settings = router_settings_for_profile(profile)?;
                let credential = RouterCredential::new(credential.as_str())?;
                Arc::new(OpenAiRouterProvider::new(settings, credential)?)
            }
            ProviderKind::CodexCli => Arc::new(CodexProvider::new(
                CodexSettings::new()?.with_reasoning_effort(profile.default_reasoning_effort())?,
                &credential,
                Some(codex_persistence(Arc::clone(&manager), profile_id.clone())),
            )?),
        };
        Ok(managed_provider(
            provider,
            Arc::clone(&cache),
            policy.clone(),
        ))
    })
}

fn codex_persistence(
    manager: Arc<ProfileManager>,
    profile_id: ProfileId,
) -> CodexCredentialPersistence {
    Arc::new(move |credential| {
        manager
            .replace_credential(&profile_id, credential)
            .map_err(|_| ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Immediate))
    })
}

fn codex_settings_for_launch(resolved: &LaunchResolution) -> Result<CodexSettings, ProviderError> {
    CodexSettings::new()?.with_reasoning_effort(resolved.active_profile_reasoning_effort.as_deref())
}

fn router_settings_for_profile(profile: &ProviderProfile) -> Result<RouterSettings, ProviderError> {
    let invalid = || ProviderError::new(ProviderErrorKind::InvalidRequest, RetryAdvice::Never);
    let url = profile
        .base_url()
        .ok_or_else(invalid)?
        .parse::<autoharness_provider_openai::RouterUrl>()
        .map_err(|_| invalid())?;
    let mut settings = RouterSettings::new(url, profile.project())?;
    if let Some(header) = profile.auth_header() {
        settings = settings.with_authentication(header, "Bearer")?;
    }
    if profile.models_path().is_some() || profile.chat_path().is_some() {
        settings = settings.with_paths(
            profile.models_path().unwrap_or("/v1/models"),
            profile.chat_path().unwrap_or("/v1/chat/completions"),
        )?;
    }
    Ok(settings)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_codex_settings_preserve_the_active_profile_reasoning_effort() {
        let resolved = LaunchResolution {
            source: CredentialSourceName::CredentialVault,
            active_profile: Some("codex-connected".to_owned()),
            provider_kind: Some(ProviderKind::CodexCli),
            active_profile_reasoning_effort: Some("high".to_owned()),
            credential: Zeroizing::new("test-credential".to_owned()),
            router: None,
        };

        let settings = codex_settings_for_launch(&resolved).expect("valid Codex settings");

        assert_eq!(settings.reasoning_effort(), Some("high"));
    }
}
