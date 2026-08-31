//! Provider-neutral values, commands, and durable event contracts.

mod command;
mod context;
mod error;
mod event;
mod id;
mod memory;
mod text;
mod tool;
mod value;

pub use command::{CommandEnvelope, CommandPayload};
pub use context::{
    ContextAdmission, ContextAdmissionFactor, ContextAdmissionReason, ContextBudgetAllocation,
    ContextEligibility, ContextEpochHashes, ContextEpochManifest, ContextEpochReason,
    ContextEpochVersions, ContextObservationState, ContextSection, ContextSourceSnapshot,
    ContextSourceVisibility, ContextTokenBudget, ContextTurnManifest, EstimatedTokens,
    MAX_CONTEXT_ADMISSION_REASONS, MAX_CONTEXT_ADMISSIONS, MAX_CONTEXT_SOURCES, MemoryGeneration,
};
pub use error::{ClassifiedError, ErrorClass, RetryAdvice, ValueError};
pub use event::{Causation, EVENT_SCHEMA_V1, EventEnvelope, EventPayload};
pub use id::{
    AgentId, ArtifactId, AttemptId, CommandId, ContextAdmissionId, ContextEpochId,
    ContextSourceKey, ContextTurnId, CorrelationId, ErrorCode, EventId, InputId, MemoryEvidenceId,
    MemoryId, MemoryOperationId, MemoryRevisionId, MemorySubjectKey, ModelId, PermissionDecisionId,
    ProviderCallId, ProviderId, SessionId, ToolCallId, ToolName, UserId, WorkspaceId,
};
pub use memory::{
    ConfidenceBasisPoints, MAX_MEMORY_EVIDENCE, MAX_MEMORY_RELATIONS,
    MAX_MEMORY_VALIDATION_CANDIDATES, MAX_MEMORY_VALIDATION_ISSUES, MEMORY_SCHEMA_V1,
    MemoryCausation, MemoryCommandEnvelope, MemoryCommandPayload, MemoryContent, MemoryEvidence,
    MemoryEvidenceExcerpt, MemoryEvidenceMetadata, MemoryEvidenceRelation, MemoryEvidenceSource,
    MemoryKind, MemoryOperationEnvelope, MemoryOperationPayload, MemoryOrigin,
    MemoryRejectionReason, MemoryRelation, MemoryRelationKind, MemoryRevision, MemoryRevisionDraft,
    MemoryRevisionMetadata, MemoryRevisionNumber, MemoryRevisionStatus, MemoryScope,
    MemorySequence, MemoryValidationIssue, MemoryValidationResult, MemoryValidationStatus,
    MemoryValidity, MemoryValidityWindow, Sensitivity, Sha256Digest, TrustClass,
};
pub use text::{contains_unsafe_display_control, is_unsafe_display_control, security_display_safe};
pub use tool::{
    ArtifactRef, CapabilityKind, CapabilityRequest, MAX_INLINE_TOOL_OUTPUT_BYTES, PermissionAnswer,
    PermissionOutcome, ResourceRef, RunLimits, TOOL_SCHEMA_V1, ToolArguments, ToolCallSpec,
    ToolOutput,
};
pub use value::{
    AttemptFailure, DeliveryMode, ModelRef, PromptText, PublicMessage, ResponseText,
    SessionSequence, SessionTitle, TimestampMillis, UsageSnapshot,
};
