//! Deterministic context construction and revisioned memory policy.

mod builder;
mod canonical;
mod compaction;
mod error;
mod rank;
mod render;
mod sizer;
mod source;
mod validate;

pub use builder::{
    BuiltContext, CONTEXT_BUILDER_VERSION, CONTEXT_RENDERER_VERSION, ContextBuildRequest,
    ContextBuilder, MAX_RENDERED_CONTEXT_BYTES, context_manifest_hash, rendered_context_hash,
    verify_context_manifest_hash, verify_rendered_context_hash,
};
pub use canonical::CanonicalEncoder;
pub use compaction::{
    COMPACTION_FACTS_VERSION, EffectiveDurableFactsFingerprint, PendingSessionFact,
    PendingSessionFactKind, effective_durable_facts, effective_durable_facts_hash,
    pending_session_facts_from_events, verify_effective_durable_facts_hash,
};
pub use error::MemoryError;
pub use rank::{
    DeterministicRankerV1, MemoryCandidate, MemoryRanker, RankFactor, RankReason, RankedMemory,
    RetrievalScope,
};
pub use render::{
    CONTEXT_PRELUDE_V1, MEMORY_RENDERER_V1, RenderedMemory, RenderedSource, SOURCE_RENDERER_V1,
    render_context_prelude, render_memory, render_source, verify_admission_rendered_hash,
};
pub use sizer::{ContextSizer, Utf8ByteSizerV1};
pub use source::{
    ContextSource, ContextSourcePolicy, ContextSourceRead, ContextSourceRegistry,
    ContextSourceValue, MAX_CONTEXT_SOURCE_VALUE_BYTES, ObservedContextSource,
    RetainedContextSource,
};
pub use validate::{
    ExistingMemory, MemoryValidationOutcome, MemoryValidationPolicy, MemoryValidatorV1,
    normalized_content_hash,
};
