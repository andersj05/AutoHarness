//! Headless command handling and deterministic session-event replay.

mod aggregate;
mod engine;
mod error;

pub use aggregate::{AdmittedInput, SessionAggregate};
pub use engine::{EventMetadataSource, GeneratedEventMetadata, InMemoryEngine};
pub use error::{CommandRejection, EngineError, ReplayError};
