use autoharness_provider::{CompletionReason, ProviderError};

/// Emits safe process lifecycle telemetry without paths, content, or credentials.
pub fn app_started() {
    tracing::info!(event = "app_started");
}

/// Emits safe terminal process shutdown telemetry.
pub fn app_stopped() {
    tracing::info!(event = "app_stopped");
}

/// Emits safe provider initialization state.
pub fn provider_ready() {
    tracing::info!(event = "provider_ready", provider = "gemini");
}

/// Emits a sanitized provider initialization failure.
pub fn provider_unavailable(error: &ProviderError) {
    tracing::warn!(
        event = "provider_unavailable",
        provider = "gemini",
        kind = ?error.kind(),
        http_status = error.http_status()
    );
}

/// Emits replay recovery counts without session or transcript values.
pub fn storage_recovered(
    active_sessions: usize,
    failed_before_dispatch: usize,
    marked_unknown: usize,
) {
    tracing::info!(
        event = "storage_recovered",
        active_sessions,
        failed_before_dispatch,
        marked_unknown
    );
}

/// Emits the durable command boundary using only structural counts.
pub fn command_committed(event_count: usize, last_sequence: u64) {
    tracing::debug!(event = "command_committed", event_count, last_sequence);
}

/// Emits the start of bounded model discovery.
pub fn catalog_refresh_started(generation: u64, interactive: bool) {
    tracing::info!(event = "catalog_refresh_started", generation, interactive);
}

/// Emits a successful provider-neutral catalog projection.
pub fn catalog_ready(model_count: usize) {
    tracing::info!(event = "catalog_ready", model_count);
}

/// Emits a sanitized catalog failure.
pub fn catalog_failed(error: &ProviderError) {
    tracing::warn!(
        event = "catalog_failed",
        kind = ?error.kind(),
        http_status = error.http_status()
    );
}

/// Emits an input and attempt preparation boundary without text or identity.
pub fn attempt_prepared() {
    tracing::info!(event = "attempt_prepared");
}

/// Emits the durable boundary immediately before provider dispatch.
pub fn attempt_started() {
    tracing::info!(event = "attempt_started");
}

/// Emits a durable cooperative cancellation request.
pub fn cancellation_requested() {
    tracing::info!(event = "attempt_cancellation_requested");
}

/// Emits a durable response segment size without response content.
pub fn response_segment_committed(bytes: usize) {
    tracing::debug!(event = "response_segment_committed", bytes);
}

/// Emits durable cumulative usage values, which never contain content.
pub fn usage_committed(input: Option<u64>, output: Option<u64>, total: Option<u64>) {
    tracing::debug!(
        event = "usage_committed",
        input_tokens = input,
        output_tokens = output,
        total_tokens = total
    );
}

/// Emits a safe provider completion reason before its durable terminal projection.
pub fn completion_observed(reason: CompletionReason) {
    tracing::info!(event = "provider_completion_observed", reason = ?reason);
}

/// Emits a terminal attempt state and optional stable provider error kind.
pub fn attempt_settled(outcome: &'static str, error: Option<&ProviderError>) {
    tracing::info!(
        event = "attempt_settled",
        outcome,
        provider_error = ?error.map(ProviderError::kind)
    );
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use autoharness_domain::{PromptText, ResponseText, RetryAdvice};
    use autoharness_provider::{ProviderError, ProviderErrorKind};
    use autoharness_provider_gemini::GeminiApiKey;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for CaptureWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("capture lock")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for Capture {
        type Writer = CaptureWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            CaptureWriter(Arc::clone(&self.0))
        }
    }

    #[test]
    fn secret_bearing_debug_values_are_redacted_in_trace_output() {
        let sentinel = "trace-secret-sentinel";
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(capture.clone())
            .finish();
        let key = GeminiApiKey::new(sentinel).expect("fixture key");
        let prompt = PromptText::new(sentinel).expect("fixture prompt");
        let response = ResponseText::new(sentinel).expect("fixture response");
        let error = ProviderError::new(ProviderErrorKind::Authentication, RetryAdvice::Never);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                ?key,
                ?prompt,
                ?response,
                ?error,
                "redaction contract fixture"
            );
        });

        let output = String::from_utf8(capture.0.lock().expect("capture lock").clone())
            .expect("trace output is UTF-8");
        assert!(!output.contains(sentinel));
        assert!(output.contains("[REDACTED]"));
    }
}
