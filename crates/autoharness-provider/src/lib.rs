//! Provider-neutral model catalog and streamed-chat ports.
//!
//! Provider-native wire values stop at adapter crates. The types in this crate
//! are safe to pass into the headless engine and deliberately contain no
//! authentication material.

mod cache;
mod catalog;
mod error;
mod managed;
mod port;
mod secret;
mod sse;
mod stream;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use cache::{CATALOG_CACHE_SCHEMA_V1, CatalogCache, CatalogCacheEntry, NoCatalogCache};
pub use catalog::{
    CapabilitySupport, CatalogFreshness, CatalogRequest, ModelCapabilities, ModelCatalog,
    ModelDescriptor, ProviderAvailability,
};
pub use error::{ProviderError, ProviderErrorKind};
pub use managed::{ManagedProvider, ProviderPolicy};
pub use port::{Catalog, Chat, Provider, ProviderEventStream, ProviderMetadata, SecretRedactor};
pub use secret::{SecretAccumulator, structured_value_may_contain_secret};
pub use sse::{SseDecoder, SseFrame};
pub use stream::{
    ChatContent, ChatMessage, ChatRequest, ChatRole, CompletionReason, ContextPrelude,
    MAX_CONTEXT_PRELUDE_BYTES, ProviderStreamEvent, ProviderToolCall, ProviderToolDefinition,
    TextDelta, UsageSnapshot,
};
pub use tokio_util::sync::CancellationToken;
