use std::collections::{BTreeMap, VecDeque};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use autoharness_domain::{ClassifiedError, ModelId, ProviderId, RetryAdvice};
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};

use crate::{
    CancellationToken, Catalog, CatalogCache, CatalogCacheEntry, CatalogFreshness, CatalogRequest,
    Chat, ChatRequest, ModelCatalog, ModelDescriptor, Provider, ProviderAvailability,
    ProviderError, ProviderErrorKind, ProviderEventStream, ProviderMetadata, ProviderStreamEvent,
    SecretRedactor,
};

const MAX_FUTURE_CACHE_SKEW: Duration = Duration::from_secs(5 * 60);

/// Shared timeout, retry, concurrency, rate-limit, and catalog-cache policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderPolicy {
    catalog_timeout: Duration,
    dispatch_timeout: Duration,
    stream_idle_timeout: Duration,
    retry_backoff: Duration,
    max_concurrency: usize,
    catalog_attempts: usize,
    dispatch_attempts: usize,
    rate_limit_requests: usize,
    rate_limit_window: Duration,
    catalog_refresh_after: Duration,
    catalog_max_stale: Duration,
}

impl Default for ProviderPolicy {
    fn default() -> Self {
        Self {
            catalog_timeout: Duration::from_secs(30),
            dispatch_timeout: Duration::from_secs(30),
            stream_idle_timeout: Duration::from_secs(120),
            retry_backoff: Duration::from_millis(250),
            max_concurrency: 4,
            catalog_attempts: 3,
            dispatch_attempts: 2,
            rate_limit_requests: 60,
            rate_limit_window: Duration::from_secs(60),
            catalog_refresh_after: Duration::from_secs(5 * 60),
            catalog_max_stale: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }
}

impl ProviderPolicy {
    /// Sets catalog and pre-stream dispatch deadlines.
    pub fn with_dispatch_timeouts(
        mut self,
        catalog: Duration,
        stream: Duration,
    ) -> Result<Self, ProviderError> {
        require_nonzero_duration(catalog)?;
        require_nonzero_duration(stream)?;
        self.catalog_timeout = catalog;
        self.dispatch_timeout = stream;
        Ok(self)
    }

    /// Sets the maximum silence permitted between normalized stream events.
    pub fn with_stream_idle_timeout(mut self, timeout: Duration) -> Result<Self, ProviderError> {
        require_nonzero_duration(timeout)?;
        self.stream_idle_timeout = timeout;
        Ok(self)
    }

    /// Sets bounded catalog and pre-stream dispatch attempt counts.
    pub fn with_attempts(
        mut self,
        catalog_attempts: usize,
        dispatch_attempts: usize,
    ) -> Result<Self, ProviderError> {
        require_nonzero(catalog_attempts)?;
        require_nonzero(dispatch_attempts)?;
        self.catalog_attempts = catalog_attempts;
        self.dispatch_attempts = dispatch_attempts;
        Ok(self)
    }

    /// Sets the base exponential retry delay.
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Result<Self, ProviderError> {
        require_nonzero_duration(backoff)?;
        self.retry_backoff = backoff;
        Ok(self)
    }

    /// Sets the maximum number of simultaneous requests for this provider project.
    pub fn with_max_concurrency(mut self, limit: usize) -> Result<Self, ProviderError> {
        require_nonzero(limit)?;
        self.max_concurrency = limit;
        Ok(self)
    }

    /// Sets the provider-project request count and monotonic accounting window.
    pub fn with_rate_limit(
        mut self,
        requests: usize,
        window: Duration,
    ) -> Result<Self, ProviderError> {
        require_nonzero(requests)?;
        require_nonzero_duration(window)?;
        self.rate_limit_requests = requests;
        self.rate_limit_window = window;
        Ok(self)
    }

    /// Sets the fresh-cache interval and maximum stale fallback age.
    pub fn with_catalog_cache_policy(
        mut self,
        refresh_after: Duration,
        max_stale: Duration,
    ) -> Result<Self, ProviderError> {
        require_nonzero_duration(refresh_after)?;
        if max_stale < refresh_after {
            return Err(invalid_policy());
        }
        self.catalog_refresh_after = refresh_after;
        self.catalog_max_stale = max_stale;
        Ok(self)
    }

    /// Returns the configured catalog request timeout.
    #[must_use]
    pub const fn catalog_timeout(&self) -> Duration {
        self.catalog_timeout
    }

    /// Returns the configured pre-stream request timeout.
    #[must_use]
    pub const fn dispatch_timeout(&self) -> Duration {
        self.dispatch_timeout
    }

    /// Returns the configured stream idle timeout.
    #[must_use]
    pub const fn stream_idle_timeout(&self) -> Duration {
        self.stream_idle_timeout
    }

    /// Returns the provider-project concurrency limit.
    #[must_use]
    pub const fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }
}

/// Provider wrapper that applies shared operational and durable-catalog behavior.
pub struct ManagedProvider {
    inner: Arc<dyn Provider>,
    cache: Arc<dyn CatalogCache>,
    policy: ProviderPolicy,
    concurrency: Arc<Semaphore>,
    rate_window: Mutex<VecDeque<Instant>>,
    catalog: RwLock<BTreeMap<ModelId, ModelDescriptor>>,
}

impl ManagedProvider {
    /// Wraps one configured provider-project adapter with shared Phase 2 policy.
    #[must_use]
    pub fn new(
        inner: Arc<dyn Provider>,
        cache: Arc<dyn CatalogCache>,
        policy: ProviderPolicy,
    ) -> Self {
        let concurrency = Arc::new(Semaphore::new(policy.max_concurrency));
        Self {
            inner,
            cache,
            policy,
            concurrency,
            rate_window: Mutex::new(VecDeque::new()),
            catalog: RwLock::new(BTreeMap::new()),
        }
    }

    async fn acquire(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<OwnedSemaphorePermit, ProviderError> {
        let permit = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled_error()),
            permit = Arc::clone(&self.concurrency).acquire_owned() => {
                permit.map_err(|_| internal_error())?
            }
        };
        self.admit_rate(cancellation).await?;
        Ok(permit)
    }

    async fn admit_rate(&self, cancellation: &CancellationToken) -> Result<(), ProviderError> {
        loop {
            let wait = {
                let now = Instant::now();
                let mut requests = self.rate_window.lock().await;
                while requests.front().is_some_and(|started| {
                    now.duration_since(*started) >= self.policy.rate_limit_window
                }) {
                    requests.pop_front();
                }
                if requests.len() < self.policy.rate_limit_requests {
                    requests.push_back(now);
                    return Ok(());
                }
                requests.front().map_or(Duration::ZERO, |started| {
                    self.policy
                        .rate_limit_window
                        .saturating_sub(now.duration_since(*started))
                })
            };
            tokio::select! {
                biased;
                () = cancellation.cancelled() => return Err(cancelled_error()),
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    async fn replace_catalog(&self, models: &[ModelDescriptor]) {
        let entries = models
            .iter()
            .cloned()
            .map(|model| (model.model_id.clone(), model))
            .collect();
        *self.catalog.write().await = entries;
    }

    async fn preflight(&self, request: &ChatRequest) -> Result<(), ProviderError> {
        if let Some(model) = self.catalog.read().await.get(&request.model_id)
            && !model.capabilities.supports_streamed_chat()
        {
            return Err(ProviderError::new(
                ProviderErrorKind::Unsupported,
                RetryAdvice::Never,
            ));
        }
        Ok(())
    }

    async fn cached_entry(&self) -> Option<CatalogCacheEntry> {
        self.cache.load(self.provider_id()).await.ok().flatten()
    }

    async fn live_catalog(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<ModelCatalog, ProviderError> {
        let mut last_error = None;
        for attempt in 0..self.policy.catalog_attempts {
            let permit = self.acquire(cancellation).await?;
            let result = tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(cancelled_error()),
                result = tokio::time::timeout(
                    self.policy.catalog_timeout,
                    self.inner.list_models(CatalogRequest::Refresh, cancellation.clone()),
                ) => result.unwrap_or_else(|_| Err(timeout_error(true))),
            };
            drop(permit);
            match result {
                Ok(catalog) => return Ok(catalog),
                Err(error) if attempt + 1 < self.policy.catalog_attempts && retryable(&error) => {
                    let delay = retry_delay(&error, attempt, self.policy.retry_backoff);
                    wait_or_cancel(delay, cancellation).await?;
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(internal_error))
    }
}

impl Debug for ManagedProvider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedProvider")
            .field("provider_id", self.provider_id())
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl ProviderMetadata for ManagedProvider {
    fn provider_id(&self) -> &ProviderId {
        self.inner.provider_id()
    }

    fn availability(&self) -> ProviderAvailability {
        self.inner.availability()
    }
}

impl SecretRedactor for ManagedProvider {
    fn redact_secrets(&self, value: &str) -> String {
        self.inner.redact_secrets(value)
    }
}

#[async_trait]
impl Catalog for ManagedProvider {
    async fn list_models(
        &self,
        request: CatalogRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelCatalog, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let now = unix_millis()?;
        let cached = self.cached_entry().await.filter(|entry| {
            entry.schema_version() == crate::CATALOG_CACHE_SCHEMA_V1
                && cache_age(now, entry.refreshed_at_ms()).is_some()
        });
        if matches!(request, CatalogRequest::PreferCache)
            && let Some(entry) = &cached
            && cache_age(now, entry.refreshed_at_ms())
                .is_some_and(|age| age <= self.policy.catalog_refresh_after)
        {
            self.replace_catalog(entry.models()).await;
            return Ok(ModelCatalog::new(
                entry.models().to_vec(),
                CatalogFreshness::Cached,
            ));
        }

        match self.live_catalog(&cancellation).await {
            Ok(catalog) => {
                let models = catalog.into_models();
                self.replace_catalog(&models).await;
                let entry = CatalogCacheEntry::new_v1(unix_millis()?, models.clone());
                let _ = self.cache.store(self.provider_id(), &entry).await;
                Ok(ModelCatalog::new(models, CatalogFreshness::Live))
            }
            Err(error) if transient_cache_fallback(&error) => {
                let Some(entry) = cached.filter(|entry| {
                    cache_age(now, entry.refreshed_at_ms())
                        .is_some_and(|age| age <= self.policy.catalog_max_stale)
                }) else {
                    return Err(error);
                };
                self.replace_catalog(entry.models()).await;
                Ok(ModelCatalog::new(
                    entry.into_models(),
                    CatalogFreshness::Stale,
                ))
            }
            Err(error) => Err(error),
        }
    }
}

#[async_trait]
impl Chat for ManagedProvider {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError> {
        self.preflight(&request).await?;
        let mut last_error = None;
        for attempt in 0..self.policy.dispatch_attempts {
            let permit = self.acquire(&cancellation).await?;
            let result = tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(cancelled_error()),
                result = tokio::time::timeout(
                    self.policy.dispatch_timeout,
                    self.inner.stream_chat(request.clone(), cancellation.clone()),
                ) => result.unwrap_or_else(|_| Err(timeout_error(false))),
            };
            match result {
                Ok(stream) => {
                    return Ok(limit_stream(
                        stream,
                        permit,
                        cancellation,
                        self.policy.stream_idle_timeout,
                    ));
                }
                Err(error) if attempt + 1 < self.policy.dispatch_attempts && retryable(&error) => {
                    drop(permit);
                    let delay = retry_delay(&error, attempt, self.policy.retry_backoff);
                    wait_or_cancel(delay, &cancellation).await?;
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(internal_error))
    }
}

fn limit_stream(
    mut stream: ProviderEventStream,
    permit: OwnedSemaphorePermit,
    cancellation: CancellationToken,
    idle_timeout: Duration,
) -> ProviderEventStream {
    Box::pin(async_stream::stream! {
        let _permit = permit;
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    yield Ok(ProviderStreamEvent::Cancelled);
                    return;
                }
                result = tokio::time::timeout(
                    idle_timeout,
                    std::future::poll_fn(|context| stream.as_mut().poll_next(context)),
                ) => {
                    match result {
                        Ok(item) => item,
                        Err(_) => {
                            yield Err(ProviderError::new(
                                ProviderErrorKind::Timeout,
                                RetryAdvice::Never,
                            ));
                            return;
                        }
                    }
                }
            };
            let Some(item) = next else {
                return;
            };
            let terminal = matches!(
                &item,
                Err(_) | Ok(ProviderStreamEvent::Completed { .. } | ProviderStreamEvent::Cancelled)
            );
            yield item;
            if terminal {
                return;
            }
        }
    })
}

fn unix_millis() -> Result<i64, ProviderError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| internal_error())?
        .as_millis();
    i64::try_from(millis).map_err(|_| internal_error())
}

fn cache_age(now_ms: i64, refreshed_at_ms: i64) -> Option<Duration> {
    let future_skew = refreshed_at_ms.saturating_sub(now_ms);
    if future_skew > i64::try_from(MAX_FUTURE_CACHE_SKEW.as_millis()).ok()? {
        return None;
    }
    let age_ms = now_ms.saturating_sub(refreshed_at_ms).max(0);
    Some(Duration::from_millis(u64::try_from(age_ms).ok()?))
}

fn retryable(error: &ProviderError) -> bool {
    !matches!(error.retry_advice(), RetryAdvice::Never)
}

fn transient_cache_fallback(error: &ProviderError) -> bool {
    matches!(
        error.kind(),
        ProviderErrorKind::RateLimited
            | ProviderErrorKind::Timeout
            | ProviderErrorKind::Unavailable
            | ProviderErrorKind::Transport
    )
}

fn retry_delay(error: &ProviderError, attempt: usize, base: Duration) -> Duration {
    match error.retry_advice() {
        RetryAdvice::After { delay_ms } => Duration::from_millis(delay_ms),
        RetryAdvice::Immediate => Duration::ZERO,
        RetryAdvice::Backoff => {
            let exponent = u32::try_from(attempt).unwrap_or(u32::MAX).min(16);
            base.saturating_mul(2_u32.saturating_pow(exponent))
        }
        RetryAdvice::Never => Duration::ZERO,
    }
}

async fn wait_or_cancel(
    delay: Duration,
    cancellation: &CancellationToken,
) -> Result<(), ProviderError> {
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(cancelled_error()),
        () = tokio::time::sleep(delay) => Ok(()),
    }
}

fn require_nonzero(value: usize) -> Result<(), ProviderError> {
    if value == 0 {
        Err(invalid_policy())
    } else {
        Ok(())
    }
}

fn require_nonzero_duration(value: Duration) -> Result<(), ProviderError> {
    if value.is_zero() {
        Err(invalid_policy())
    } else {
        Ok(())
    }
}

fn invalid_policy() -> ProviderError {
    ProviderError::new(ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
}

fn timeout_error(retryable: bool) -> ProviderError {
    ProviderError::new(
        ProviderErrorKind::Timeout,
        if retryable {
            RetryAdvice::Backoff
        } else {
            RetryAdvice::Never
        },
    )
}

fn cancelled_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Cancelled, RetryAdvice::Never)
}

fn internal_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex as StdMutex};

    use autoharness_domain::ModelId;

    use crate::{
        CapabilitySupport, CatalogCache, ChatContent, ChatMessage, ChatRole, CompletionReason,
        ModelCapabilities, NoCatalogCache,
    };

    use super::*;

    static PROVIDER_ID: LazyLock<ProviderId> =
        LazyLock::new(|| ProviderId::new("fixture:project").expect("provider ID"));

    struct FakeProvider {
        catalog_calls: AtomicUsize,
        chat_calls: AtomicUsize,
        catalogs: StdMutex<VecDeque<Result<ModelCatalog, ProviderError>>>,
        pending_stream: bool,
    }

    impl FakeProvider {
        fn new(catalogs: Vec<Result<ModelCatalog, ProviderError>>) -> Self {
            Self {
                catalog_calls: AtomicUsize::new(0),
                chat_calls: AtomicUsize::new(0),
                catalogs: StdMutex::new(catalogs.into()),
                pending_stream: false,
            }
        }

        fn pending() -> Self {
            Self {
                pending_stream: true,
                ..Self::new(Vec::new())
            }
        }
    }

    #[async_trait]
    impl Catalog for FakeProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            self.catalog_calls.fetch_add(1, Ordering::SeqCst);
            self.catalogs
                .lock()
                .expect("catalog lock")
                .pop_front()
                .unwrap_or_else(|| Ok(live_catalog(supported_capabilities())))
        }
    }

    #[async_trait]
    impl Chat for FakeProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.chat_calls.fetch_add(1, Ordering::SeqCst);
            if self.pending_stream {
                Ok(Box::pin(async_stream::stream! {
                    yield Ok(ProviderStreamEvent::Started);
                    std::future::pending::<()>().await;
                }))
            } else {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ])))
            }
        }
    }

    impl SecretRedactor for FakeProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for FakeProvider {
        fn provider_id(&self) -> &ProviderId {
            &PROVIDER_ID
        }
    }

    #[derive(Default)]
    struct MemoryCache {
        entry: Mutex<Option<CatalogCacheEntry>>,
    }

    #[async_trait]
    impl CatalogCache for MemoryCache {
        async fn load(
            &self,
            _provider_id: &ProviderId,
        ) -> Result<Option<CatalogCacheEntry>, ProviderError> {
            Ok(self.entry.lock().await.clone())
        }

        async fn store(
            &self,
            _provider_id: &ProviderId,
            entry: &CatalogCacheEntry,
        ) -> Result<(), ProviderError> {
            *self.entry.lock().await = Some(entry.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn fresh_cache_avoids_network_and_transient_refresh_uses_bounded_stale_data() {
        let now = unix_millis().expect("clock");
        let fresh_cache = Arc::new(MemoryCache {
            entry: Mutex::new(Some(CatalogCacheEntry::new_v1(
                now,
                vec![model(supported_capabilities())],
            ))),
        });
        let inner = Arc::new(FakeProvider::new(Vec::new()));
        let managed = ManagedProvider::new(inner.clone(), fresh_cache, ProviderPolicy::default());

        let catalog = managed
            .list_models(CatalogRequest::PreferCache, CancellationToken::new())
            .await
            .expect("fresh cache");
        assert_eq!(catalog.freshness(), CatalogFreshness::Cached);
        assert_eq!(inner.catalog_calls.load(Ordering::SeqCst), 0);

        let stale_cache = Arc::new(MemoryCache {
            entry: Mutex::new(Some(CatalogCacheEntry::new_v1(
                now.saturating_sub(10_000),
                vec![model(supported_capabilities())],
            ))),
        });
        let unavailable = ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Backoff);
        let inner = Arc::new(FakeProvider::new(vec![Err(unavailable)]));
        let policy = ProviderPolicy::default()
            .with_attempts(1, 1)
            .expect("attempts")
            .with_catalog_cache_policy(Duration::from_millis(1), Duration::from_secs(60))
            .expect("cache policy");
        let managed = ManagedProvider::new(inner, stale_cache, policy);

        let catalog = managed
            .list_models(CatalogRequest::Refresh, CancellationToken::new())
            .await
            .expect("stale fallback");
        assert_eq!(catalog.freshness(), CatalogFreshness::Stale);
    }

    #[tokio::test]
    async fn retry_is_pre_stream_and_known_unsupported_capability_never_dispatches_chat() {
        let retryable = ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Immediate);
        let unsupported = ModelCapabilities {
            chat: CapabilitySupport::Supported,
            streaming: CapabilitySupport::Unsupported,
            managed_interactions: CapabilitySupport::Unknown,
            thinking: CapabilitySupport::Unknown,
        };
        let inner = Arc::new(FakeProvider::new(vec![
            Err(retryable),
            Ok(live_catalog(unsupported)),
        ]));
        let policy = ProviderPolicy::default()
            .with_attempts(2, 2)
            .expect("attempts");
        let managed = ManagedProvider::new(inner.clone(), Arc::new(NoCatalogCache), policy);

        managed
            .list_models(CatalogRequest::Refresh, CancellationToken::new())
            .await
            .expect("retried catalog");
        assert_eq!(inner.catalog_calls.load(Ordering::SeqCst), 2);
        let Err(error) = managed
            .stream_chat(request(), CancellationToken::new())
            .await
        else {
            panic!("unsupported streaming must fail before dispatch");
        };
        assert_eq!(error.kind(), ProviderErrorKind::Unsupported);
        assert_eq!(inner.chat_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stream_holds_concurrency_permit_and_idle_timeout_settles_without_retry() {
        use futures_util::StreamExt as _;

        let inner = Arc::new(FakeProvider::pending());
        let policy = ProviderPolicy::default()
            .with_max_concurrency(1)
            .expect("concurrency")
            .with_stream_idle_timeout(Duration::from_millis(30))
            .expect("idle timeout");
        let managed = Arc::new(ManagedProvider::new(
            inner,
            Arc::new(NoCatalogCache),
            policy,
        ));
        let mut first = managed
            .stream_chat(request(), CancellationToken::new())
            .await
            .expect("first stream");
        assert_eq!(
            first.next().await.expect("started").expect("event"),
            ProviderStreamEvent::Started
        );

        let second_managed = Arc::clone(&managed);
        let second = tokio::spawn(async move {
            second_managed
                .stream_chat(request(), CancellationToken::new())
                .await
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!second.is_finished());
        let error = first
            .next()
            .await
            .expect("timeout item")
            .expect_err("idle timeout");
        assert_eq!(error.kind(), ProviderErrorKind::Timeout);
        drop(first);
        let mut second_stream = tokio::time::timeout(Duration::from_millis(100), second)
            .await
            .expect("permit released")
            .expect("join")
            .expect("second stream");
        assert!(second_stream.next().await.is_some());
    }

    #[tokio::test]
    async fn per_project_rate_window_delays_excess_dispatch_without_busy_waiting() {
        let inner = Arc::new(FakeProvider::new(Vec::new()));
        let policy = ProviderPolicy::default()
            .with_attempts(1, 1)
            .expect("attempts")
            .with_rate_limit(1, Duration::from_millis(30))
            .expect("rate limit");
        let managed = Arc::new(ManagedProvider::new(
            inner,
            Arc::new(NoCatalogCache),
            policy,
        ));
        let first = managed
            .stream_chat(request(), CancellationToken::new())
            .await
            .expect("first stream");
        drop(first);

        let second_managed = Arc::clone(&managed);
        let second = tokio::spawn(async move {
            second_managed
                .stream_chat(request(), CancellationToken::new())
                .await
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!second.is_finished());
        let stream = tokio::time::timeout(Duration::from_millis(100), second)
            .await
            .expect("rate window elapsed")
            .expect("join")
            .expect("second stream");
        drop(stream);
    }

    fn request() -> ChatRequest {
        ChatRequest::new(
            ModelId::new("model-a").expect("model"),
            vec![ChatMessage::text(
                ChatRole::User,
                ChatContent::new("hello").expect("content"),
            )],
        )
        .expect("request")
    }

    fn live_catalog(capabilities: ModelCapabilities) -> ModelCatalog {
        ModelCatalog::new(vec![model(capabilities)], CatalogFreshness::Live)
    }

    fn model(capabilities: ModelCapabilities) -> ModelDescriptor {
        ModelDescriptor {
            provider_id: PROVIDER_ID.clone(),
            model_id: ModelId::new("model-a").expect("model"),
            display_name: "Model A".to_owned(),
            description: None,
            input_token_limit: None,
            output_token_limit: None,
            capabilities,
        }
    }

    fn supported_capabilities() -> ModelCapabilities {
        ModelCapabilities {
            chat: CapabilitySupport::Supported,
            streaming: CapabilitySupport::Supported,
            managed_interactions: CapabilitySupport::Unknown,
            thinking: CapabilitySupport::Unknown,
        }
    }
}
