use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use autoharness_domain::{ProviderId, RetryAdvice};
use autoharness_provider::{
    CATALOG_CACHE_SCHEMA_V1, CatalogCache, CatalogCacheEntry, ProviderError, ProviderErrorKind,
};
use autoharness_store::StoreError;
use autoharness_store_sqlite::{SqliteCatalogCacheRecord, SqliteStore};

use crate::error::AppError;

/// Async cache handle backed by a dedicated blocking SQLite connection.
#[derive(Clone)]
pub struct SqliteCatalogCache {
    store: Arc<Mutex<SqliteStore>>,
}

impl SqliteCatalogCache {
    /// Opens the durable catalog cache in the application database.
    pub fn open(path: PathBuf) -> Result<Self, AppError> {
        Ok(Self {
            store: Arc::new(Mutex::new(SqliteStore::open(path)?)),
        })
    }
}

#[async_trait]
impl CatalogCache for SqliteCatalogCache {
    async fn load(
        &self,
        provider_id: &ProviderId,
    ) -> Result<Option<CatalogCacheEntry>, ProviderError> {
        let store = Arc::clone(&self.store);
        let provider_id = provider_id.clone();
        tokio::task::spawn_blocking(move || {
            let record = store
                .lock()
                .map_err(|_| internal_error())?
                .load_catalog_cache(&provider_id)
                .map_err(store_error)?;
            let Some(record) = record else {
                return Ok(None);
            };
            if record.schema_version() != CATALOG_CACHE_SCHEMA_V1 {
                return Err(protocol_error());
            }
            let entry: CatalogCacheEntry =
                serde_json::from_slice(record.catalog_json()).map_err(|_| protocol_error())?;
            if entry.schema_version() != record.schema_version()
                || entry.refreshed_at_ms() != record.refreshed_at_ms()
                || entry
                    .models()
                    .iter()
                    .any(|model| model.provider_id != provider_id)
            {
                return Err(protocol_error());
            }
            Ok(Some(entry))
        })
        .await
        .map_err(|_| internal_error())?
    }

    async fn store(
        &self,
        provider_id: &ProviderId,
        entry: &CatalogCacheEntry,
    ) -> Result<(), ProviderError> {
        if entry.schema_version() != CATALOG_CACHE_SCHEMA_V1
            || entry
                .models()
                .iter()
                .any(|model| &model.provider_id != provider_id)
        {
            return Err(protocol_error());
        }
        let store = Arc::clone(&self.store);
        let record = SqliteCatalogCacheRecord::new(
            provider_id.clone(),
            entry.schema_version(),
            entry.refreshed_at_ms(),
            serde_json::to_vec(entry).map_err(|_| internal_error())?,
        );
        tokio::task::spawn_blocking(move || {
            store
                .lock()
                .map_err(|_| internal_error())?
                .replace_catalog_cache(&record)
                .map_err(store_error)
        })
        .await
        .map_err(|_| internal_error())?
    }
}

fn store_error(error: StoreError) -> ProviderError {
    let retry = if matches!(error, StoreError::Busy) {
        RetryAdvice::Backoff
    } else {
        RetryAdvice::Never
    };
    ProviderError::new(ProviderErrorKind::Internal, retry)
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

fn internal_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use autoharness_domain::ModelId;
    use autoharness_provider::{CapabilitySupport, ModelCapabilities, ModelDescriptor};

    use super::*;

    #[tokio::test]
    async fn cache_round_trips_only_provider_neutral_descriptors() {
        let directory = tempfile::tempdir().expect("directory");
        let cache =
            SqliteCatalogCache::open(directory.path().join("cache.sqlite3")).expect("cache");
        let provider_id = ProviderId::new("router:test").expect("provider");
        let entry = CatalogCacheEntry::new_v1(
            100,
            vec![ModelDescriptor {
                provider_id: provider_id.clone(),
                model_id: ModelId::new("model-a").expect("model"),
                display_name: "Model A".to_owned(),
                description: None,
                input_token_limit: None,
                output_token_limit: None,
                capabilities: ModelCapabilities {
                    chat: CapabilitySupport::Supported,
                    streaming: CapabilitySupport::Unknown,
                    managed_interactions: CapabilitySupport::Unsupported,
                    thinking: CapabilitySupport::Unknown,
                },
            }],
        );

        cache.store(&provider_id, &entry).await.expect("store");
        assert_eq!(cache.load(&provider_id).await.expect("load"), Some(entry));
    }
}
