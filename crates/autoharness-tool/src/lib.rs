//! Capability-scoped tool planning, permission policy, execution, and artifacts.

mod artifact;
mod budget;
mod capability;
mod error;
mod permission;
mod runtime;
mod schema;

pub use artifact::{ArtifactStore, FileArtifactStore};
pub use budget::RunBudget;
pub use capability::{
    FilesystemCapability, HttpCapability, HttpResult, LocalFilesystem, LocalHttp, LocalProcess,
    ProcessCapability, ProcessResult,
};
pub use error::{ToolError, ToolErrorKind};
pub use permission::{
    AuthorizedToolCall, PermissionEvidence, PermissionPolicy, PermissionRule, authorize,
};
pub use runtime::ToolRuntime;
pub use schema::{
    IncomingToolCall, MemoryProposal, MemoryProposalScope, PermissionDetail, PlannedToolCall,
    ToolDefinition, definitions, permission_details, plan, replan,
};
