//! Provider-neutral model catalog and streamed-chat ports.
//!
//! Provider-native wire values stop at adapter crates. The types in this crate
//! are safe to pass into the headless engine and deliberately contain no
//! authentication material.

mod catalog;
mod error;
mod port;
mod stream;

pub use catalog::{CapabilitySupport, ModelCapabilities, ModelDescriptor};
pub use error::{ProviderError, ProviderErrorKind};
pub use port::{Catalog, Chat, Provider, ProviderEventStream, SecretRedactor};
pub use stream::{
    ChatContent, ChatMessage, ChatRequest, ChatRole, CompletionReason, ProviderStreamEvent,
    TextDelta, UsageSnapshot,
};
pub use tokio_util::sync::CancellationToken;
