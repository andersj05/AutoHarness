//! Provider-neutral durable session storage ports and read models.

mod context;
mod error;
mod memory;
mod port;
mod read_model;

pub use context::{
    BoundContextTurnCommitReceipt, BoundContextTurnCommitRequest, CompactionFactsSnapshot,
    ContextAdmissionContent, ContextCommitDisposition, ContextCompactionBoundary,
    ContextCompactionCheckpoint, ContextStore, ContextTurnCommitRequest, ContextTurnContent,
    MAX_RENDERED_CONTEXT_BYTES, RenderedContextText,
};
pub use error::{CorruptionArea, IdentityKind, StoreError};
pub use memory::{
    ActiveMemoryHead, ActiveMemoryHeadCursor, ActiveMemoryHeadPageQuery, ActiveMemoryHeadQuery,
    DEFAULT_MEMORY_PAGE_SIZE, MAX_MEMORY_INSPECTION_PAGE_SIZE, MAX_MEMORY_SEARCH_CANDIDATES,
    MemoryAdmissionCursor, MemoryAdmissionKey, MemoryAdmissionQuery, MemoryAdmissionRecord,
    MemoryAppendBatchRequest, MemoryAppendDisposition, MemoryAppendOperation, MemoryAppendReceipt,
    MemoryAppendRequest, MemoryCandidateBatch, MemoryContentState, MemoryEvidenceContent,
    MemoryEvidenceExcerptState, MemoryInspectionCursor, MemoryInspectionPage,
    MemoryInspectionQuery, MemoryInspectionRecord, MemoryMutationGeneration, MemoryRevisionContent,
    MemorySearchCandidate, MemorySearchQuery, MemoryStore, StoredMemoryCandidate,
    StoredMemoryEvidenceContent,
};
pub use port::{
    AppendDisposition, AppendReceipt, AppendRequest, DEFAULT_EVENT_PAGE_SIZE, DeletionDisposition,
    SessionStore,
};
pub use read_model::{
    AdmittedInputRecord, AttemptRecord, AttemptState, InputState, SessionStatus, SessionSummary,
    TranscriptEntry, TranscriptRole, TranscriptSource, TranscriptState, TranscriptText,
};
