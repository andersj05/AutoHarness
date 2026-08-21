//! Reusable assertions for fixture-backed provider adapter tests.

use autoharness_domain::{ClassifiedError, ProviderId, RetryAdvice};
use futures_util::StreamExt as _;

use crate::{
    CompletionReason, ModelCatalog, Provider, ProviderError, ProviderErrorKind,
    ProviderStreamEvent, UsageSnapshot,
};

/// Asserts stable catalog identity, ordering, uniqueness, and provider ownership.
pub fn assert_catalog(
    catalog: &ModelCatalog,
    provider_id: &ProviderId,
    expected_model_ids: &[&str],
) {
    let actual = catalog
        .models()
        .iter()
        .map(|model| model.model_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected_model_ids);
    assert!(
        catalog
            .models()
            .iter()
            .all(|model| &model.provider_id == provider_id)
    );
    assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
}

/// Collects one provider stream and requires every normalized item to succeed.
pub async fn collect_stream(mut stream: crate::ProviderEventStream) -> Vec<ProviderStreamEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("fixture stream must normalize successfully"));
    }
    events
}

/// Asserts the common started, cumulative usage, and terminal lifecycle contract.
pub fn assert_stream_lifecycle(events: &[ProviderStreamEvent]) {
    assert!(matches!(events.first(), Some(ProviderStreamEvent::Started)));
    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::Completed { .. } | ProviderStreamEvent::Cancelled)
    ));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderStreamEvent::Started))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                ProviderStreamEvent::Completed { .. } | ProviderStreamEvent::Cancelled
            ))
            .count(),
        1
    );

    let mut prior = UsageSnapshot::default();
    for usage in events.iter().filter_map(|event| match event {
        ProviderStreamEvent::Usage(usage) => Some(*usage),
        _ => None,
    }) {
        assert_monotonic(prior.input_tokens, usage.input_tokens);
        assert_monotonic(prior.output_tokens, usage.output_tokens);
        assert_monotonic(prior.total_tokens, usage.total_tokens);
        prior = usage;
    }
}

/// Asserts that provider credential text is removed before persistence boundaries.
pub fn assert_secret_redaction(provider: &dyn Provider, secret: &str) {
    let redacted = provider.redact_secrets(&format!("before {secret} after"));
    assert!(!redacted.contains(secret));
    assert!(redacted.contains("[REDACTED]"));
    assert!(!format!("{provider_id:?}", provider_id = provider.provider_id()).contains(secret));
}

/// Asserts cancellation or another preflight failure that is never automatically retried.
pub fn assert_non_retryable(error: &ProviderError, expected: ProviderErrorKind) {
    assert_eq!(error.kind(), expected);
    assert_eq!(error.retry_advice(), RetryAdvice::Never);
}

/// Asserts the normal stop mapping used by a successful fixture.
pub fn assert_normal_completion(events: &[ProviderStreamEvent]) {
    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::Completed {
            reason: CompletionReason::Stop
        })
    ));
}

fn assert_monotonic(prior: Option<u64>, current: Option<u64>) {
    if let (Some(prior), Some(current)) = (prior, current) {
        assert!(current >= prior);
    }
}
