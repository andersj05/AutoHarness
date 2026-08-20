use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_engine::DurableEngineError;
use autoharness_provider::ProviderError;
use autoharness_store::StoreError;

/// Safe top-level application failure.
#[derive(Debug)]
pub enum AppError {
    /// A local filesystem operation failed.
    FileSystem,
    /// Another AutoHarness process owns the local writer lease.
    WriterAlreadyRunning,
    /// Durable storage failed.
    Store(StoreError),
    /// Durable command execution or replay failed.
    Engine(DurableEngineError),
    /// Provider initialization failed safely.
    Provider(ProviderError),
    /// Terminal initialization, input, drawing, or restoration failed.
    Terminal,
    /// A required application worker stopped unexpectedly.
    WorkerStopped,
    /// Process configuration could not satisfy a required invariant.
    Configuration,
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileSystem => formatter.write_str("a local application file operation failed"),
            Self::WriterAlreadyRunning => {
                formatter.write_str("another AutoHarness process is using this data directory")
            }
            Self::Store(source) => Display::fmt(source, formatter),
            Self::Engine(source) => Display::fmt(source, formatter),
            Self::Provider(source) => Display::fmt(source, formatter),
            Self::Terminal => formatter.write_str("terminal operation failed"),
            Self::WorkerStopped => formatter.write_str("an application worker stopped"),
            Self::Configuration => formatter.write_str("application configuration is invalid"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(source) => Some(source),
            Self::Engine(source) => Some(source),
            Self::Provider(source) => Some(source),
            Self::FileSystem
            | Self::WriterAlreadyRunning
            | Self::Terminal
            | Self::WorkerStopped
            | Self::Configuration => None,
        }
    }
}

impl From<StoreError> for AppError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<DurableEngineError> for AppError {
    fn from(value: DurableEngineError) -> Self {
        Self::Engine(value)
    }
}

impl From<ProviderError> for AppError {
    fn from(value: ProviderError) -> Self {
        Self::Provider(value)
    }
}
