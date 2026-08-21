use std::pin::Pin;

use async_trait::async_trait;
use autoharness_domain::ProviderId;
use futures_core::Stream;

use crate::{
    CancellationToken, CatalogRequest, ChatRequest, ModelCatalog, ProviderAvailability,
    ProviderError, ProviderStreamEvent,
};

/// A boxed provider-neutral event stream.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<ProviderStreamEvent, ProviderError>> + Send + 'static>>;

/// Asynchronous dynamic model discovery.
#[async_trait]
pub trait Catalog: Send + Sync {
    /// Lists all compatible models, following provider pagination internally.
    async fn list_models(
        &self,
        request: CatalogRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelCatalog, ProviderError>;
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

/// Stable provider identity and process-local availability metadata.
pub trait ProviderMetadata: Send + Sync {
    /// Returns the adapter and project identity used by durable catalog caching.
    fn provider_id(&self) -> &ProviderId;

    /// Returns whether the constructed adapter can accept requests.
    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Ready
    }
}

/// Combined provider port used by application composition.
pub trait Provider: Catalog + Chat + SecretRedactor + ProviderMetadata {}

impl<T> Provider for T where T: Catalog + Chat + SecretRedactor + ProviderMetadata {}
