//! Provider-neutral durable session storage ports and read models.

mod error;
mod port;
mod read_model;

pub use error::{CorruptionArea, IdentityKind, StoreError};
pub use port::{
    AppendDisposition, AppendReceipt, AppendRequest, DEFAULT_EVENT_PAGE_SIZE, DeletionDisposition,
    SessionStore,
};
pub use read_model::{
    AdmittedInputRecord, AttemptRecord, AttemptState, InputState, SessionStatus, SessionSummary,
    TranscriptEntry, TranscriptRole, TranscriptSource, TranscriptState, TranscriptText,
};
