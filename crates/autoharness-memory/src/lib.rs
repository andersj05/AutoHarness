//! Deterministic context construction and revisioned memory policy.

mod builder;
mod canonical;
mod error;
mod rank;
mod render;
mod sizer;
mod source;
mod validate;

pub use builder::{
    BuiltContext, CONTEXT_BUILDER_VERSION, CONTEXT_RENDERER_VERSION, ContextBuildRequest,
    ContextBuilder, MAX_RENDERED_CONTEXT_BYTES, context_manifest_hash,
    verify_context_manifest_hash,
};
pub use canonical::CanonicalEncoder;
pub use error::MemoryError;
pub use rank::{
    DeterministicRankerV1, MemoryCandidate, MemoryRanker, RankFactor, RankReason, RankedMemory,
    RetrievalScope,
};
pub use render::{
    CONTEXT_PRELUDE_V1, MEMORY_RENDERER_V1, RenderedMemory, RenderedSource, SOURCE_RENDERER_V1,
    render_context_prelude, render_memory, render_source,
};
pub use sizer::{ContextSizer, Utf8ByteSizerV1};
pub use source::{
    ContextSource, ContextSourcePolicy, ContextSourceRead, ContextSourceRegistry,
    ContextSourceValue, MAX_CONTEXT_SOURCE_VALUE_BYTES, ObservedContextSource,
    RetainedContextSource,
};
pub use validate::{
    ExistingMemory, MemoryValidationPolicy, MemoryValidatorV1, normalized_content_hash,
};
