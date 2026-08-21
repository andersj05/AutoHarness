use async_trait::async_trait;
use autoharness_domain::ProviderId;
use serde::{Deserialize, Serialize};

use crate::{ModelDescriptor, ProviderError};

/// Schema version for durable provider-neutral catalog cache payloads.
pub const CATALOG_CACHE_SCHEMA_V1: u16 = 1;

/// One integrity-protected provider-neutral catalog cache payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogCacheEntry {
    schema_version: u16,
    refreshed_at_ms: i64,
    models: Vec<ModelDescriptor>,
}

impl CatalogCacheEntry {
    /// Constructs a schema-v1 cache payload at the supplied wall-clock observation.
    #[must_use]
    pub const fn new_v1(refreshed_at_ms: i64, models: Vec<ModelDescriptor>) -> Self {
        Self {
            schema_version: CATALOG_CACHE_SCHEMA_V1,
            refreshed_at_ms,
            models,
        }
    }

    /// Returns the cache schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns when the live catalog was observed as Unix epoch milliseconds.
    #[must_use]
    pub const fn refreshed_at_ms(&self) -> i64 {
        self.refreshed_at_ms
    }

    /// Returns the cached provider-neutral descriptors.
    #[must_use]
    pub fn models(&self) -> &[ModelDescriptor] {
        &self.models
    }

    /// Consumes the cache payload and returns its descriptors.
    #[must_use]
    pub fn into_models(self) -> Vec<ModelDescriptor> {
        self.models
    }
}

/// Durable cache operations used by the shared provider management layer.
#[async_trait]
pub trait CatalogCache: Send + Sync {
    /// Loads the latest cache payload for one provider-project identity.
    async fn load(
        &self,
        provider_id: &ProviderId,
    ) -> Result<Option<CatalogCacheEntry>, ProviderError>;

    /// Replaces the cache payload for one provider-project identity.
    async fn store(
        &self,
        provider_id: &ProviderId,
        entry: &CatalogCacheEntry,
    ) -> Result<(), ProviderError>;
}

/// A cache implementation for tests or compositions that intentionally disable persistence.
#[derive(Debug, Default)]
pub struct NoCatalogCache;

#[async_trait]
impl CatalogCache for NoCatalogCache {
    async fn load(
        &self,
        _provider_id: &ProviderId,
    ) -> Result<Option<CatalogCacheEntry>, ProviderError> {
        Ok(None)
    }

    async fn store(
        &self,
        _provider_id: &ProviderId,
        _entry: &CatalogCacheEntry,
    ) -> Result<(), ProviderError> {
        Ok(())
    }
}
