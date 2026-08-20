use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;

use crate::{CancellationToken, ChatRequest, ModelDescriptor, ProviderError, ProviderStreamEvent};

/// A boxed provider-neutral event stream.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<ProviderStreamEvent, ProviderError>> + Send + 'static>>;

/// Asynchronous dynamic model discovery.
#[async_trait]
pub trait Catalog: Send + Sync {
    /// Lists all compatible models, following provider pagination internally.
    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelDescriptor>, ProviderError>;
}

/// Asynchronous stateless chat streaming.
#[async_trait]
pub trait Chat: Send + Sync {
    /// Dispatches a complete local conversation and returns normalized events.
    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError>;
}

/// Removes provider-owned credential values before content crosses a durable boundary.
pub trait SecretRedactor: Send + Sync {
    /// Returns model text with every configured provider credential occurrence removed.
    fn redact_secrets(&self, value: &str) -> String;
}

/// Combined provider port used by application composition.
pub trait Provider: Catalog + Chat + SecretRedactor {}

impl<T> Provider for T where T: Catalog + Chat + SecretRedactor {}
