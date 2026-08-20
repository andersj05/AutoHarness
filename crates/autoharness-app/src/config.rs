use std::env;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::AppError;

const DATA_DIR_ENV: &str = "AUTOHARNESS_DATA_DIR";
const DATABASE_FILE: &str = "autoharness.sqlite3";
const LOCK_FILE: &str = "autoharness.writer.lock";
const LOG_FILE: &str = "autoharness.log";
const LOG_LEVEL_ENV: &str = "AUTOHARNESS_LOG";

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
