//! Provider-neutral values, commands, and durable event contracts.

mod command;
mod error;
mod event;
mod id;
mod value;

pub use command::{CommandEnvelope, CommandPayload};
pub use error::{ClassifiedError, ErrorClass, RetryAdvice, ValueError};
pub use event::{Causation, EVENT_SCHEMA_V1, EventEnvelope, EventPayload};
pub use id::{CommandId, CorrelationId, EventId, InputId, ModelId, ProviderId, SessionId};
pub use value::{DeliveryMode, ModelRef, PromptText, SessionSequence, TimestampMillis};
