//! Provider-neutral values, commands, and durable event contracts.

mod command;
mod error;
mod event;
mod id;
mod tool;
mod value;

pub use command::{CommandEnvelope, CommandPayload};
pub use error::{ClassifiedError, ErrorClass, RetryAdvice, ValueError};
pub use event::{Causation, EVENT_SCHEMA_V1, EventEnvelope, EventPayload};
pub use id::{
    ArtifactId, AttemptId, CommandId, CorrelationId, ErrorCode, EventId, InputId, ModelId,
    PermissionDecisionId, ProviderCallId, ProviderId, SessionId, ToolCallId, ToolName,
};
pub use tool::{
    ArtifactRef, CapabilityKind, CapabilityRequest, MAX_INLINE_TOOL_OUTPUT_BYTES, PermissionAnswer,
    PermissionOutcome, ResourceRef, RunLimits, TOOL_SCHEMA_V1, ToolArguments, ToolCallSpec,
    ToolOutput,
};
pub use value::{
    AttemptFailure, DeliveryMode, ModelRef, PromptText, PublicMessage, ResponseText,
    SessionSequence, TimestampMillis, UsageSnapshot,
};
