use std::env;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;
use autoharness_provider::ProviderPolicy;

const DATA_DIR_ENV: &str = "AUTOHARNESS_DATA_DIR";
const DATABASE_FILE: &str = "autoharness.sqlite3";
const LOCK_FILE: &str = "autoharness.writer.lock";
const LOG_FILE: &str = "autoharness.log";
const LOG_LEVEL_ENV: &str = "AUTOHARNESS_LOG";
const PROVIDER_ENV: &str = "AUTOHARNESS_PROVIDER";
const PROVIDER_TIMEOUT_MS_ENV: &str = "AUTOHARNESS_PROVIDER_TIMEOUT_MS";
const PROVIDER_IDLE_TIMEOUT_MS_ENV: &str = "AUTOHARNESS_PROVIDER_IDLE_TIMEOUT_MS";
const PROVIDER_RETRY_ATTEMPTS_ENV: &str = "AUTOHARNESS_PROVIDER_RETRY_ATTEMPTS";
const PROVIDER_CONCURRENCY_ENV: &str = "AUTOHARNESS_PROVIDER_CONCURRENCY";
const PROVIDER_RATE_REQUESTS_ENV: &str = "AUTOHARNESS_PROVIDER_RATE_REQUESTS";
const PROVIDER_RATE_WINDOW_MS_ENV: &str = "AUTOHARNESS_PROVIDER_RATE_WINDOW_MS";
const CATALOG_REFRESH_MS_ENV: &str = "AUTOHARNESS_CATALOG_REFRESH_MS";
const CATALOG_MAX_STALE_MS_ENV: &str = "AUTOHARNESS_CATALOG_MAX_STALE_MS";
const WORKSPACE_ENV: &str = "AUTOHARNESS_WORKSPACE";

/// Configured production provider adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSelection {
    /// Google AI Studio Gemini.
    Gemini,
    /// Configurable OpenAI-compatible model router.
    Router,
}

/// Resolved application-owned paths outside the current working directory.
pub struct AppPaths {
    data_dir: PathBuf,
}

/// Builds a target-restricted tracing directive from the optional log-level setting.
pub fn log_filter_directive() -> Result<String, AppError> {
    let configured = env::var(LOG_LEVEL_ENV).ok();
    normalize_log_level(configured.as_deref())
        .map(|level| format!("autoharness={level}"))
        .ok_or(AppError::Configuration)
}

/// Resolves the selected provider, defaulting to Gemini for compatibility.
pub fn provider_selection() -> Result<ProviderSelection, AppError> {
    match env::var(PROVIDER_ENV)
        .unwrap_or_else(|_| "gemini".to_owned())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "gemini" => Ok(ProviderSelection::Gemini),
        "router" | "openai" => Ok(ProviderSelection::Router),
        _ => Err(AppError::Configuration),
    }
}

/// Resolves bounded shared provider policy from optional environment overrides.
pub fn provider_policy() -> Result<ProviderPolicy, AppError> {
    let mut policy = ProviderPolicy::default();
    if let Some(milliseconds) = positive_u64(PROVIDER_TIMEOUT_MS_ENV)? {
        let timeout = Duration::from_millis(milliseconds);
        policy = policy
            .with_dispatch_timeouts(timeout, timeout)
            .map_err(|_| AppError::Configuration)?;
    }
    if let Some(milliseconds) = positive_u64(PROVIDER_IDLE_TIMEOUT_MS_ENV)? {
        policy = policy
            .with_stream_idle_timeout(Duration::from_millis(milliseconds))
            .map_err(|_| AppError::Configuration)?;
    }
    if let Some(attempts) = positive_usize(PROVIDER_RETRY_ATTEMPTS_ENV)? {
        policy = policy
            .with_attempts(attempts, attempts)
            .map_err(|_| AppError::Configuration)?;
    }
    if let Some(concurrency) = positive_usize(PROVIDER_CONCURRENCY_ENV)? {
        policy = policy
            .with_max_concurrency(concurrency)
            .map_err(|_| AppError::Configuration)?;
    }
    let rate_requests = positive_usize(PROVIDER_RATE_REQUESTS_ENV)?;
    let rate_window = positive_u64(PROVIDER_RATE_WINDOW_MS_ENV)?;
    if rate_requests.is_some() || rate_window.is_some() {
        policy = policy
            .with_rate_limit(
                rate_requests.unwrap_or(60),
                Duration::from_millis(rate_window.unwrap_or(60_000)),
            )
            .map_err(|_| AppError::Configuration)?;
    }
    let refresh = positive_u64(CATALOG_REFRESH_MS_ENV)?;
    let max_stale = positive_u64(CATALOG_MAX_STALE_MS_ENV)?;
    if refresh.is_some() || max_stale.is_some() {
        policy = policy
            .with_catalog_cache_policy(
                Duration::from_millis(refresh.unwrap_or(5 * 60_000)),
                Duration::from_millis(max_stale.unwrap_or(7 * 24 * 60 * 60_000)),
            )
            .map_err(|_| AppError::Configuration)?;
    }
    Ok(policy)
}

/// Resolves and canonicalizes the only workspace visible to local tool capabilities.
pub fn workspace_root() -> Result<PathBuf, AppError> {
    let configured = env::var_os(WORKSPACE_ENV)
        .map(PathBuf::from)
        .map_or_else(std::env::current_dir, Ok)
        .map_err(|_| AppError::Configuration)?;
    let root = std::fs::canonicalize(configured).map_err(|_| AppError::Configuration)?;
    if !root.is_absolute() || !root.is_dir() {
        return Err(AppError::Configuration);
    }
    Ok(root)
}

impl AppPaths {
    /// Resolves and creates the application data directory.
    pub fn prepare() -> Result<Self, AppError> {
        let data_dir = discover_data_dir()?;
        if !data_dir.is_absolute() {
            return Err(AppError::Configuration);
        }
        std::fs::create_dir_all(&data_dir).map_err(|_| AppError::FileSystem)?;
        Ok(Self { data_dir })
    }

    /// Returns the durable SQLite database path.
    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE)
    }

    /// Returns the process writer-lease path.
    #[must_use]
    pub fn writer_lock(&self) -> PathBuf {
        self.data_dir.join(LOCK_FILE)
    }

    /// Returns the structured application log path.
    #[must_use]
    pub fn log(&self) -> PathBuf {
        self.data_dir.join(LOG_FILE)
    }

    /// Returns the content-addressed tool artifact directory.
    #[must_use]
    pub fn artifacts(&self) -> PathBuf {
        self.data_dir.join("artifacts")
    }

    /// Returns the non-secret provider-profile settings document path.
    #[must_use]
    pub fn profiles(&self) -> PathBuf {
        self.data_dir.join("autoharness.profiles.json")
    }
}

/// Exclusive operating-system lease held for the lifetime of one app process.
pub struct WriterLease {
    file: File,
}

impl WriterLease {
    /// Acquires a non-blocking exclusive lock on the data-directory sidecar.
    pub fn acquire(path: &Path) -> Result<Self, AppError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|_| AppError::FileSystem)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(AppError::WriterAlreadyRunning),
            Err(TryLockError::Error(_)) => Err(AppError::FileSystem),
        }
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn discover_data_dir() -> Result<PathBuf, AppError> {
    if let Some(override_path) = env::var_os(DATA_DIR_ENV) {
        if override_path.is_empty() {
            return Err(AppError::Configuration);
        }
        return Ok(PathBuf::from(override_path));
    }

    platform_data_dir().ok_or(AppError::Configuration)
}

fn normalize_log_level(value: Option<&str>) -> Option<&'static str> {
    match value.unwrap_or("info").trim().to_ascii_lowercase().as_str() {
        "off" => Some("off"),
        "error" => Some("error"),
        "warn" => Some("warn"),
        "info" => Some("info"),
        "debug" => Some("debug"),
        "trace" => Some("trace"),
        _ => None,
    }
}

fn positive_u64(name: &str) -> Result<Option<u64>, AppError> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(AppError::Configuration)
        })
        .transpose()
}

fn positive_usize(name: &str) -> Result<Option<usize>, AppError> {
    env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(AppError::Configuration)
        })
        .transpose()
}

#[cfg(target_os = "windows")]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("AutoHarness"))
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from).map(|root| {
        root.join("Library")
            .join("Application Support")
            .join("AutoHarness")
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|root| PathBuf::from(root).join(".local/share")))
        .map(|root| root.join("autoharness"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_files_share_the_resolved_directory() {
        let paths = AppPaths {
            data_dir: PathBuf::from("C:/fixture/autoharness"),
        };

        assert_eq!(
            paths.database(),
            PathBuf::from("C:/fixture/autoharness/autoharness.sqlite3")
        );
        assert_eq!(
            paths.writer_lock(),
            PathBuf::from("C:/fixture/autoharness/autoharness.writer.lock")
        );
        assert_eq!(
            paths.log(),
            PathBuf::from("C:/fixture/autoharness/autoharness.log")
        );
        assert_eq!(
            paths.artifacts(),
            PathBuf::from("C:/fixture/autoharness/artifacts")
        );
    }

    #[test]
    fn writer_lease_is_exclusive_and_released_on_drop() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("writer.lock");
        let first = WriterLease::acquire(&path).expect("first writer lease");

        assert!(matches!(
            WriterLease::acquire(&path),
            Err(AppError::WriterAlreadyRunning)
        ));

        drop(first);
        WriterLease::acquire(&path).expect("lease is released after drop");
    }

    #[test]
    fn log_filter_accepts_only_levels_and_restricts_targets() {
        assert_eq!(normalize_log_level(None), Some("info"));
        assert_eq!(normalize_log_level(Some(" DEBUG ")), Some("debug"));
        assert_eq!(normalize_log_level(Some("reqwest=trace")), None);
    }
}
