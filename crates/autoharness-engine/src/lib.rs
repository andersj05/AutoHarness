//! Headless command handling and deterministic session-event replay.

mod aggregate;
mod durable;
mod engine;
mod error;

pub use aggregate::{
    AdmittedInput, AttemptProjection, AttemptStatus, ContextTurnBinding, SessionAggregate,
    ToolCallProjection, ToolCallStatus,
};
pub use durable::{DurableEngine, DurableEngineError};
pub use engine::{EventMetadataSource, GeneratedEventMetadata, InMemoryEngine};
pub use error::{CommandRejection, EngineError, ReplayError};
