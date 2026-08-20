use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use autoharness_domain::{ClassifiedError, ProviderId, RetryAdvice};
use autoharness_provider::{
    CancellationToken, Catalog, Chat, ChatRequest, ChatRole, ModelDescriptor, ProviderError,
    ProviderErrorKind, ProviderEventStream, ProviderStreamEvent, SecretRedactor,
};
use futures_util::StreamExt as _;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{Method, Response, StatusCode, Url};
use serde::Serialize;
use serde_json::Value;

use crate::GeminiApiKey;
use crate::models::{ModelsPage, request_model_name};
use crate::native_stream::{NativeStreamState, Transport};
use crate::sse::SseDecoder;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/";
const API_KEY_HEADER: &str = "x-goog-api-key";
const CATALOG_PAGE_SIZE: u16 = 1000;
const MAX_CATALOG_PAGES: usize = 1000;
const MAX_PAGE_TOKEN_BYTES: usize = 16 * 1024;
const MAX_CATALOG_PAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_SSE_FRAME_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_HEADERS_TIMEOUT: Duration = Duration::from_secs(30);

/// Google AI Studio provider using dynamic discovery and stateless local history.
#[derive(Clone)]
pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: Arc<GeminiApiKey>,
    base_url: Url,
    provider_id: ProviderId,
    limits: Limits,
}

#[derive(Clone, Copy)]
struct Limits {
    max_catalog_pages: usize,
    max_catalog_page_bytes: usize,
    max_error_body_bytes: usize,
    max_request_body_bytes: usize,
    max_sse_frame_bytes: usize,
    response_headers_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_catalog_pages: MAX_CATALOG_PAGES,
            max_catalog_page_bytes: MAX_CATALOG_PAGE_BYTES,
            max_error_body_bytes: MAX_ERROR_BODY_BYTES,
            max_request_body_bytes: MAX_REQUEST_BODY_BYTES,
            max_sse_frame_bytes: MAX_SSE_FRAME_BYTES,
            response_headers_timeout: RESPONSE_HEADERS_TIMEOUT,
        }
    }
}

impl GeminiProvider {
    /// Constructs the production Google AI Studio adapter.
    pub fn new(api_key: GeminiApiKey) -> Result<Self, ProviderError> {
        let base_url = Url::parse(DEFAULT_BASE_URL).map_err(|_| internal_error())?;
        Self::with_base_url(api_key, base_url)
    }

    /// Reads `GEMINI_API_KEY` and constructs the production adapter.
    pub fn from_env() -> Result<Self, ProviderError> {
        Self::new(GeminiApiKey::from_env()?)
    }

    fn with_base_url(api_key: GeminiApiKey, base_url: Url) -> Result<Self, ProviderError> {
        validate_base_url(&base_url)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| internal_error())?;
        Ok(Self {
            client,
            api_key: Arc::new(api_key),
            base_url,
            provider_id: ProviderId::new("gemini").map_err(|_| internal_error())?,
            limits: Limits::default(),
        })
    }

    #[cfg(test)]
    fn for_test(api_key: GeminiApiKey, base_url: Url) -> Result<Self, ProviderError> {
        Self::with_base_url(api_key, base_url)
    }

    fn api_key_header(&self) -> Result<HeaderValue, ProviderError> {
        let mut header = HeaderValue::from_str(self.api_key.expose()).map_err(|_| {
            ProviderError::new(ProviderErrorKind::Authentication, RetryAdvice::Never)
        })?;
        header.set_sensitive(true);
        Ok(header)
    }

    fn endpoint(&self, path: &str) -> Result<Url, ProviderError> {
        let url = self.base_url.join(path).map_err(|_| internal_error())?;
        if url.origin() != self.base_url.origin()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(internal_error());
        }
        Ok(url)
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        accept: &'static str,
        replay: ReplaySafety,
        cancellation: &CancellationToken,
    ) -> Result<Response, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        let mut request = self
            .client
            .request(method, url)
            .header(API_KEY_HEADER, self.api_key_header()?)
            .header(ACCEPT, accept);
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }

        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled_error()),
            result = tokio::time::timeout(self.limits.response_headers_timeout, request.send()) => result,
        };
        match result {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(classify_transport_error(&error, replay)),
            Err(_) => Err(ProviderError::new(
                ProviderErrorKind::Timeout,
                replay.retry_advice(),
            )),
        }
    }

    async fn require_success(
        &self,
        response: Response,
        cancellation: &CancellationToken,
    ) -> Result<Response, ProviderError> {
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let retry_after = retry_after_seconds(response.headers());
        let body =
            match read_bounded(response, self.limits.max_error_body_bytes, cancellation).await {
                Ok(body) => body,
                Err(error) if error.kind() == ProviderErrorKind::Cancelled => return Err(error),
                Err(_) => Vec::new(),
            };
        Err(classify_http_error(status, &body, retry_after))
    }

    async fn send_sse(
        &self,
        url: Url,
        body: Vec<u8>,
        cancellation: &CancellationToken,
    ) -> Result<Response, ProviderError> {
        if body.len() > self.limits.max_request_body_bytes {
            return Err(ProviderError::new(
                ProviderErrorKind::LimitExceeded,
                RetryAdvice::Never,
            ));
        }
        let response = self
            .send(
                Method::POST,
                url,
                Some(body),
                "text/event-stream",
                ReplaySafety::AmbiguousAfterDispatch,
                cancellation,
            )
            .await?;
        let response = self.require_success(response, cancellation).await?;
        if !is_event_stream(response.headers()) {
            return Err(ProviderError::new(
                ProviderErrorKind::Protocol,
                RetryAdvice::Never,
            ));
        }
        Ok(response)
    }

    fn decode_stream(
        &self,
        response: Response,
        transport: Transport,
        cancellation: CancellationToken,
    ) -> ProviderEventStream {
        let key = Arc::clone(&self.api_key);
        let max_frame_bytes = self.limits.max_sse_frame_bytes;
        Box::pin(async_stream::stream! {
            let mut bytes = response.bytes_stream();
            let mut decoder = SseDecoder::new(max_frame_bytes);
            let mut state = NativeStreamState::new(transport);
            yield Ok(ProviderStreamEvent::Started);

            loop {
                let next = tokio::select! {
                    biased;
                    () = cancellation.cancelled() => {
                        yield Ok(ProviderStreamEvent::Cancelled);
                        return;
                    }
                    next = bytes.next() => next,
                };
                let Some(next) = next else {
                    if let Err(error) = decoder.finish() {
                        yield Err(error);
                        return;
                    }
                    yield Err(ProviderError::new(
                        ProviderErrorKind::Protocol,
                        RetryAdvice::Never,
                    ));
                    return;
                };
                let chunk = match next {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        yield Err(ProviderError::new(
                            ProviderErrorKind::Transport,
                            RetryAdvice::Never,
                        ));
                        return;
                    }
                };
                let frames = match decoder.push(&chunk) {
                    Ok(frames) => frames,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };
                for frame in frames {
                    let events = match state.handle(frame, &key) {
                        Ok(events) => events,
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    };
                    for event in events {
                        let terminal = matches!(
                            event,
                            ProviderStreamEvent::Completed { .. }
                                | ProviderStreamEvent::Cancelled
                        );
                        yield Ok(event);
                        if terminal {
                            return;
                        }
                    }
                }
            }
        })
    }
}

impl Debug for GeminiProvider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiProvider")
            .field("provider_id", &self.provider_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl SecretRedactor for GeminiProvider {
    fn redact_secrets(&self, value: &str) -> String {
        self.api_key.redact(value)
    }
}

#[async_trait]
impl Catalog for GeminiProvider {
    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelDescriptor>, ProviderError> {
        let mut page_token = None;
        let mut seen_tokens = HashSet::new();
        let mut models = BTreeMap::new();

        for _ in 0..self.limits.max_catalog_pages {
            let mut url = self.endpoint("v1beta/models")?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("pageSize", &CATALOG_PAGE_SIZE.to_string());
                if let Some(token) = page_token.as_deref() {
                    query.append_pair("pageToken", token);
                }
            }
            // Credentials belong only in the sensitive header, never the URL.
            if self.api_key.contains(url.as_str()) {
                return Err(internal_error());
            }
            let response = self
                .send(
                    Method::GET,
                    url,
                    None,
                    "application/json",
                    ReplaySafety::Idempotent,
                    &cancellation,
                )
                .await?;
            let response = self.require_success(response, &cancellation).await?;
            let body = read_bounded(response, self.limits.max_catalog_page_bytes, &cancellation)
                .await
                .map_err(classify_catalog_read_error)?;
            let page: ModelsPage = serde_json::from_slice(&body)
                .map_err(|_| ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never))?;
            for native in page.models {
                if let Some(model) = native.into_descriptor(&self.provider_id, &self.api_key) {
                    models
                        .entry(model.model_id.as_str().to_owned())
                        .or_insert(model);
                }
            }

            let Some(next_token) = page.next_page_token.filter(|token| !token.is_empty()) else {
                return Ok(models.into_values().collect());
            };
            if next_token.len() > MAX_PAGE_TOKEN_BYTES
                || self.api_key.contains(&next_token)
                || !seen_tokens.insert(next_token.clone())
            {
                return Err(ProviderError::new(
                    ProviderErrorKind::Protocol,
                    RetryAdvice::Never,
                ));
            }
            page_token = Some(next_token);
        }

        Err(ProviderError::new(
            ProviderErrorKind::LimitExceeded,
            RetryAdvice::Never,
        ))
    }
}

#[async_trait]
impl Chat for GeminiProvider {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError> {
        if self.api_key.contains(request.model_id.as_str()) {
            return Err(ProviderError::new(
                ProviderErrorKind::InvalidRequest,
                RetryAdvice::Never,
            ));
        }
        let model_name = request_model_name(&request.model_id)?;
        let interactions_body = interaction_body(model_name, &request, &self.api_key)?;
        let interactions_url = self.endpoint("v1/interactions?alt=sse")?;

        match self
            .send_sse(interactions_url, interactions_body, &cancellation)
            .await
        {
            Ok(response) => Ok(self.decode_stream(response, Transport::Interactions, cancellation)),
            Err(error) if error.allows_transport_fallback() => {
                let generate_body = generate_content_body(&request, &self.api_key)?;
                let path = format!("v1beta/models/{model_name}:streamGenerateContent?alt=sse");
                let response = self
                    .send_sse(self.endpoint(&path)?, generate_body, &cancellation)
                    .await?;
                Ok(self.decode_stream(response, Transport::GenerateContent, cancellation))
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Copy)]
enum ReplaySafety {
    Idempotent,
    AmbiguousAfterDispatch,
}

impl ReplaySafety {
    const fn retry_advice(self) -> RetryAdvice {
        match self {
            Self::Idempotent => RetryAdvice::Backoff,
            Self::AmbiguousAfterDispatch => RetryAdvice::Never,
        }
    }
}

#[derive(Serialize)]
struct InteractionRequest<'a> {
    model: &'a str,
    input: Vec<InteractionInput>,
    stream: bool,
    store: bool,
}

#[derive(Serialize)]
struct InteractionInput {
    #[serde(rename = "type")]
    kind: &'static str,
    content: [InteractionContent; 1],
}

#[derive(Serialize)]
struct InteractionContent {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<GenerateMessage>,
}

#[derive(Serialize)]
struct GenerateMessage {
    role: &'static str,
    parts: [GeneratePart; 1],
}

#[derive(Serialize)]
struct GeneratePart {
    text: String,
}

fn interaction_body(
    model: &str,
    request: &ChatRequest,
    key: &GeminiApiKey,
) -> Result<Vec<u8>, ProviderError> {
    let input = request
        .messages
        .iter()
        .map(|message| InteractionInput {
            kind: match message.role {
                ChatRole::User => "user_input",
                ChatRole::Assistant => "model_output",
            },
            content: [InteractionContent {
                kind: "text",
                text: key.redact(message.content.as_str()),
            }],
        })
        .collect();
    serde_json::to_vec(&InteractionRequest {
        model,
        input,
        stream: true,
        store: false,
    })
    .map_err(|_| internal_error())
}

fn generate_content_body(
    request: &ChatRequest,
    key: &GeminiApiKey,
) -> Result<Vec<u8>, ProviderError> {
    let contents = request
        .messages
        .iter()
        .map(|message| GenerateMessage {
            role: match message.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "model",
            },
            parts: [GeneratePart {
                text: key.redact(message.content.as_str()),
            }],
        })
        .collect();
    serde_json::to_vec(&GenerateContentRequest { contents }).map_err(|_| internal_error())
}

async fn read_bounded(
    response: Response,
    limit: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ProviderError> {
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled_error()),
            next = stream.next() => next,
        };
        let Some(next) = next else {
            return Ok(output);
        };
        let chunk =
            next.map_err(|_| ProviderError::new(ProviderErrorKind::Transport, RetryAdvice::Never))?;
        if output.len().saturating_add(chunk.len()) > limit {
            return Err(ProviderError::new(
                ProviderErrorKind::LimitExceeded,
                RetryAdvice::Never,
            ));
        }
        output.extend_from_slice(&chunk);
    }
}

fn validate_base_url(base_url: &Url) -> Result<(), ProviderError> {
    if !matches!(base_url.scheme(), "https" | "http")
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(ProviderError::new(
            ProviderErrorKind::InvalidRequest,
            RetryAdvice::Never,
        ));
    }
    Ok(())
}

fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("text/event-stream")
            })
        })
}

fn retry_after_seconds(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds.min(86_400))
}

fn classify_transport_error(error: &reqwest::Error, replay: ReplaySafety) -> ProviderError {
    if error.is_connect() {
        ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Backoff)
    } else if error.is_timeout() {
        ProviderError::new(ProviderErrorKind::Timeout, replay.retry_advice())
    } else {
        ProviderError::new(ProviderErrorKind::Transport, RetryAdvice::Never)
    }
}

fn classify_catalog_read_error(error: ProviderError) -> ProviderError {
    if error.kind() == ProviderErrorKind::Transport {
        ProviderError::new(ProviderErrorKind::Unavailable, RetryAdvice::Backoff)
    } else {
        error
    }
}

fn classify_http_error(status: StatusCode, body: &[u8], retry_after: Option<u64>) -> ProviderError {
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let mut error = classify_error_value(&value, Some(status));
    if let Some(seconds) = retry_after
        && matches!(error.retry_advice(), RetryAdvice::Backoff)
    {
        error = ProviderError::new(
            error.kind(),
            RetryAdvice::After {
                delay_ms: seconds.saturating_mul(1000),
            },
        )
        .with_http_status(status.as_u16());
    }
    error
}

pub(crate) fn classify_error_value(value: &Value, status: Option<StatusCode>) -> ProviderError {
    let code = value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/error/status").and_then(Value::as_str))
        .or_else(|| value.get("code").and_then(Value::as_str))
        .or_else(|| value.get("status").and_then(Value::as_str))
        .map(str::to_ascii_lowercase);

    let (kind, retry) = match code.as_deref() {
        Some("rate_limit_exceeded" | "resource_exhausted") => {
            (ProviderErrorKind::RateLimited, RetryAdvice::Backoff)
        }
        Some("quota_exceeded") => (ProviderErrorKind::QuotaExceeded, RetryAdvice::Never),
        Some("api_error" | "internal" | "service_unavailable" | "unavailable") => {
            (ProviderErrorKind::Unavailable, RetryAdvice::Backoff)
        }
        Some("aborted") => (ProviderErrorKind::Conflict, RetryAdvice::Immediate),
        Some("deadline_exceeded") => (ProviderErrorKind::Timeout, RetryAdvice::Never),
        Some("cancelled" | "canceled") => (ProviderErrorKind::Cancelled, RetryAdvice::Never),
        Some("unauthenticated") => (ProviderErrorKind::Authentication, RetryAdvice::Never),
        Some("permission_denied") => (ProviderErrorKind::PermissionDenied, RetryAdvice::Never),
        Some("invalid_request" | "invalid_argument" | "failed_precondition" | "out_of_range") => {
            (ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
        }
        Some("model_not_found" | "not_found") => {
            (ProviderErrorKind::ModelNotFound, RetryAdvice::Never)
        }
        Some("unimplemented") => (ProviderErrorKind::Unsupported, RetryAdvice::Never),
        Some(
            "safety"
            | "recitation"
            | "prohibited_content"
            | "blocklist"
            | "spii"
            | "malformed_function_call",
        ) => (ProviderErrorKind::ContentBlocked, RetryAdvice::Never),
        _ => classify_status(status),
    };
    let error = ProviderError::new(kind, retry);
    status.map_or(error.clone(), |status| {
        error.with_http_status(status.as_u16())
    })
}

fn classify_status(status: Option<StatusCode>) -> (ProviderErrorKind, RetryAdvice) {
    match status.map(|status| status.as_u16()) {
        Some(400 | 412 | 416 | 422) => (ProviderErrorKind::InvalidRequest, RetryAdvice::Never),
        Some(401) => (ProviderErrorKind::Authentication, RetryAdvice::Never),
        Some(403) => (ProviderErrorKind::PermissionDenied, RetryAdvice::Never),
        Some(404) => (ProviderErrorKind::ModelNotFound, RetryAdvice::Never),
        Some(408) => (ProviderErrorKind::Timeout, RetryAdvice::Backoff),
        Some(409) => (ProviderErrorKind::Conflict, RetryAdvice::Immediate),
        Some(429) => (ProviderErrorKind::RateLimited, RetryAdvice::Backoff),
        Some(499) => (ProviderErrorKind::Cancelled, RetryAdvice::Never),
        Some(500 | 502 | 503) => (ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
        Some(501) => (ProviderErrorKind::Unsupported, RetryAdvice::Never),
        Some(504) => (ProviderErrorKind::Timeout, RetryAdvice::Never),
        Some(code) if (400..500).contains(&code) => {
            (ProviderErrorKind::InvalidRequest, RetryAdvice::Never)
        }
        Some(code) if code >= 500 => (ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
        _ => (ProviderErrorKind::Protocol, RetryAdvice::Never),
    }
}

fn cancelled_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Cancelled, RetryAdvice::Never)
}

fn internal_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoharness_domain::{ClassifiedError, ModelId};
    use autoharness_provider::{
        CapabilitySupport, ChatContent, ChatMessage, CompletionReason, TextDelta, UsageSnapshot,
    };

    use crate::test_http::{ResponseSpec, spawn, spawn_slow_sse};

    fn request() -> ChatRequest {
        ChatRequest::new(
            ModelId::new("models/gemini-test").expect("model"),
            vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: ChatContent::new("first").expect("content"),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: ChatContent::new("second").expect("content"),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: ChatContent::new("third").expect("content"),
                },
            ],
        )
        .expect("request")
    }

    #[test]
    fn interactions_body_is_stateless_and_contains_full_history() {
        let key = GeminiApiKey::new("fixture-key").expect("key");
        let value: Value = serde_json::from_slice(
            &interaction_body("gemini-test", &request(), &key).expect("serialize request"),
        )
        .expect("request JSON");

        assert_eq!(value["model"], "gemini-test");
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], false);
        assert_eq!(value["input"].as_array().map(Vec::len), Some(3));
        assert_eq!(value["input"][0]["type"], "user_input");
        assert_eq!(value["input"][1]["type"], "model_output");
        assert!(value.get("previous_interaction_id").is_none());
    }

    #[test]
    fn both_request_transports_remove_credential_occurrences_from_model_input() {
        let sentinel = "sensitive+/=sentinel";
        let encoded = "sensitive%2B%2F%3Dsentinel";
        let key = GeminiApiKey::new(sentinel).expect("key");
        let request = ChatRequest::new(
            ModelId::new("models/gemini-test").expect("model"),
            vec![ChatMessage {
                role: ChatRole::User,
                content: ChatContent::new(format!("before {sentinel} and {encoded} after"))
                    .expect("content"),
            }],
        )
        .expect("request");

        for body in [
            interaction_body("gemini-test", &request, &key).expect("Interactions body"),
            generate_content_body(&request, &key).expect("Generate Content body"),
        ] {
            let text = std::str::from_utf8(&body).expect("JSON UTF-8");
            assert!(!text.contains(sentinel));
            assert!(!text.contains(encoded));
            assert_eq!(text.matches("[REDACTED]").count(), 2);
        }
    }

    #[test]
    fn retry_classification_is_explicit_and_safe() {
        let cases = [
            (408, ProviderErrorKind::Timeout, RetryAdvice::Backoff),
            (429, ProviderErrorKind::RateLimited, RetryAdvice::Backoff),
            (500, ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
            (503, ProviderErrorKind::Unavailable, RetryAdvice::Backoff),
            (504, ProviderErrorKind::Timeout, RetryAdvice::Never),
            (401, ProviderErrorKind::Authentication, RetryAdvice::Never),
            (403, ProviderErrorKind::PermissionDenied, RetryAdvice::Never),
            (499, ProviderErrorKind::Cancelled, RetryAdvice::Never),
            (501, ProviderErrorKind::Unsupported, RetryAdvice::Never),
        ];
        for (status, kind, retry) in cases {
            let status = StatusCode::from_u16(status).expect("status");
            let error = classify_http_error(status, b"{}", None);
            assert_eq!(error.kind(), kind, "status {status}");
            assert_eq!(error.retry_advice(), retry, "status {status}");
        }

        let quota = classify_http_error(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"code":"quota_exceeded","message":"ignored"}}"#,
            None,
        );
        assert_eq!(quota.kind(), ProviderErrorKind::QuotaExceeded);
        assert_eq!(quota.retry_advice(), RetryAdvice::Never);

        let delayed = classify_http_error(StatusCode::TOO_MANY_REQUESTS, b"{}", Some(7));
        assert_eq!(
            delayed.retry_advice(),
            RetryAdvice::After { delay_ms: 7000 }
        );

        let interrupted_catalog = classify_catalog_read_error(ProviderError::new(
            ProviderErrorKind::Transport,
            RetryAdvice::Never,
        ));
        assert_eq!(interrupted_catalog.kind(), ProviderErrorKind::Unavailable);
        assert_eq!(interrupted_catalog.retry_advice(), RetryAdvice::Backoff);
    }

    #[test]
    fn only_unsupported_and_model_not_found_allow_compatibility_fallback() {
        for (status, expected) in [(404, true), (501, true), (429, false), (401, false)] {
            let error =
                classify_http_error(StatusCode::from_u16(status).expect("status"), b"{}", None);
            assert_eq!(error.allows_transport_fallback(), expected);
        }
    }

    #[test]
    fn provider_debug_and_safe_errors_redact_secret_sentinel() {
        let sentinel = "gemini-secret-sentinel";
        let provider =
            GeminiProvider::new(GeminiApiKey::new(sentinel).expect("key")).expect("provider");
        let malicious =
            format!(r#"{{"error":{{"code":"unauthenticated","message":"echo {sentinel}"}}}}"#);
        let error = classify_http_error(StatusCode::UNAUTHORIZED, malicious.as_bytes(), None);
        let rendered = format!("{provider:?} {error:?} {error}");
        let serialized = serde_json::to_string(&error).expect("safe error JSON");

        assert!(!rendered.contains(sentinel));
        assert!(!serialized.contains(sentinel));
    }

    #[test]
    fn provider_redacts_its_configured_credential_before_persistence() {
        let secret = "credential-persistence-sentinel";
        let provider = GeminiProvider::new(GeminiApiKey::new(secret).expect("fixture key"))
            .expect("fixture provider");

        assert_eq!(
            provider.redact_secrets(&format!("before {secret} after")),
            "before [REDACTED] after"
        );
    }

    #[tokio::test]
    async fn paginated_catalog_preserves_opaque_tokens_and_redacts_external_echoes() {
        let sentinel = "gemini-secret-sentinel";
        let responses = vec![
            ResponseSpec::json(
                200,
                include_bytes!("../tests/fixtures/models-page-1.json").as_slice(),
            ),
            ResponseSpec::json(
                200,
                include_bytes!("../tests/fixtures/models-page-2.json").as_slice(),
            ),
            ResponseSpec::json(
                200,
                include_bytes!("../tests/fixtures/models-page-3.json").as_slice(),
            ),
        ];
        let (base_url, server) = spawn(responses).await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new(sentinel).expect("key"), base_url)
                .expect("fixture provider");

        let models = provider
            .list_models(CancellationToken::new())
            .await
            .expect("paginated catalog");
        let requests = server.await.expect("fixture server");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id.as_str(), "models/gemini-a");
        assert_eq!(models[0].display_name, "Gemini A");
        assert_eq!(models[1].model_id.as_str(), "models/gemini-z");
        assert_eq!(models[1].capabilities.chat, CapabilitySupport::Supported);
        assert_eq!(models[1].capabilities.streaming, CapabilitySupport::Unknown);
        let description = models[1].description.as_deref().expect("description");
        assert!(description.contains("[REDACTED]"));
        assert!(!description.contains(sentinel));

        assert_eq!(requests.len(), 3);
        let expected_tokens = [None, Some("opaque +/= token?"), Some("next/&=+ token")];
        for (request, expected_token) in requests.iter().zip(expected_tokens) {
            assert_eq!(request.method(), "GET");
            assert!(request.header_equals(API_KEY_HEADER, sentinel));
            assert!(request.body().is_empty());
            assert!(!request.target().contains(sentinel));
            assert!(!format!("{request:?}").contains(sentinel));
            let url = Url::parse(&format!("http://fixture{}", request.target()))
                .expect("recorded target URL");
            let pairs = url.query_pairs().collect::<BTreeMap<_, _>>();
            assert_eq!(
                pairs.get("pageSize").map(|value| value.as_ref()),
                Some("1000")
            );
            assert_eq!(
                pairs.get("pageToken").map(|value| value.as_ref()),
                expected_token
            );
        }
    }

    #[tokio::test]
    async fn repeated_catalog_token_stops_before_an_infinite_loop() {
        let page = br#"{"models":[],"nextPageToken":"repeat"}"#;
        let (base_url, server) = spawn(vec![
            ResponseSpec::json(200, page.as_slice()),
            ResponseSpec::json(200, page.as_slice()),
        ])
        .await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new("fixture-key").expect("key"), base_url)
                .expect("fixture provider");

        let error = provider
            .list_models(CancellationToken::new())
            .await
            .expect_err("repeated token must fail");
        let requests = server.await.expect("fixture server");

        assert_eq!(error.kind(), ProviderErrorKind::Protocol);
        assert_eq!(requests.len(), 2);
    }

    #[tokio::test]
    async fn raw_and_percent_encoded_credentials_never_enter_request_targets_or_bodies() {
        let sentinel = "sensitive+/=sentinel";
        let encoded = "sensitive%2b%2f%3dsentinel";
        let (base_url, server) = spawn(vec![ResponseSpec::json(
            200,
            br#"{"models":[]}"#.as_slice(),
        )])
        .await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new(sentinel).expect("key"), base_url)
                .expect("fixture provider");

        provider
            .list_models(CancellationToken::new())
            .await
            .expect("catalog");
        let requests = server.await.expect("fixture server");

        assert_eq!(requests.len(), 1);
        assert!(requests[0].header_equals(API_KEY_HEADER, sentinel));
        assert!(!requests[0].target().contains(sentinel));
        assert!(!requests[0].target().to_ascii_lowercase().contains(encoded));
        assert!(
            !requests[0]
                .body()
                .windows(sentinel.len())
                .any(|bytes| bytes == sentinel.as_bytes())
        );
        assert!(!format!("{:?}", requests[0]).contains(sentinel));
    }

    #[tokio::test]
    async fn pre_stream_model_not_found_uses_generate_content_fallback() {
        let sentinel = "gemini-secret-sentinel";
        let (base_url, server) = spawn(vec![
            ResponseSpec::json(
                404,
                br#"{"error":{"code":"model_not_found","message":"not echoed"}}"#.as_slice(),
            ),
            ResponseSpec::sse(
                include_bytes!("../tests/fixtures/generate-content-stream.sse").as_slice(),
            ),
        ])
        .await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new(sentinel).expect("key"), base_url)
                .expect("fixture provider");
        let mut stream = provider
            .stream_chat(request(), CancellationToken::new())
            .await
            .expect("compatibility stream");
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event.expect("normalized event"));
        }
        let requests = server.await.expect("fixture server");

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Started,
                ProviderStreamEvent::TextDelta(TextDelta::new("fallback ").expect("text")),
                ProviderStreamEvent::TextDelta(TextDelta::new("works").expect("text")),
                ProviderStreamEvent::Usage(UsageSnapshot {
                    input_tokens: Some(3),
                    output_tokens: Some(2),
                    cached_input_tokens: None,
                    reasoning_tokens: Some(1),
                    tool_tokens: None,
                    total_tokens: Some(6),
                }),
                ProviderStreamEvent::Completed {
                    reason: CompletionReason::Stop,
                },
            ]
        );
        assert_eq!(requests.len(), 2);
        assert!(requests[0].target().starts_with("/v1/interactions?"));
        assert!(
            requests[1]
                .target()
                .starts_with("/v1beta/models/gemini-test:streamGenerateContent?")
        );
        for recorded in &requests {
            assert!(recorded.header_equals(API_KEY_HEADER, sentinel));
            assert!(!recorded.target().contains(sentinel));
            assert!(
                !recorded
                    .body()
                    .windows(sentinel.len())
                    .any(|bytes| bytes == sentinel.as_bytes())
            );
        }
    }

    #[tokio::test]
    async fn stream_error_never_switches_transport_or_auto_retries() {
        let body = concat!(
            "event: interaction.created\n",
            "data: {\"event_type\":\"interaction.created\"}\n\n",
            "event: error\n",
            "data: {\"event_type\":\"error\",\"error\":{\"code\":\"model_not_found\",\"message\":\"gemini-secret-sentinel\"}}\n\n"
        );
        let (base_url, server) = spawn(vec![ResponseSpec::sse(body.as_bytes())]).await;
        let provider = GeminiProvider::for_test(
            GeminiApiKey::new("gemini-secret-sentinel").expect("key"),
            base_url,
        )
        .expect("fixture provider");
        let mut stream = provider
            .stream_chat(request(), CancellationToken::new())
            .await
            .expect("accepted stream");

        assert_eq!(
            stream.next().await.expect("started").expect("event"),
            ProviderStreamEvent::Started
        );
        let error = stream
            .next()
            .await
            .expect("stream error")
            .expect_err("must fail");
        assert_eq!(error.kind(), ProviderErrorKind::ModelNotFound);
        assert_eq!(error.retry_advice(), RetryAdvice::Never);
        assert!(stream.next().await.is_none());
        let requests = server.await.expect("fixture server");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].target().starts_with("/v1/interactions?"));
    }

    #[tokio::test]
    async fn cancellation_aborts_an_in_progress_response_body_without_retry() {
        let first_events = concat!(
            "event: interaction.created\r\n",
            "data: {\"event_type\":\"interaction.created\"}\r\n\r\n",
            "event: step.start\r\n",
            "data: {\"event_type\":\"step.start\",\"index\":1,\"step\":{\"type\":\"model_output\"}}\r\n\r\n",
            "event: step.delta\r\n",
            "data: {\"event_type\":\"step.delta\",\"index\":1,\"delta\":{\"type\":\"text\",\"text\":\"partial\"}}\r\n\r\n"
        )
        .as_bytes()
        .to_vec();
        let (base_url, server) = spawn_slow_sse(first_events).await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new("fixture-key").expect("key"), base_url)
                .expect("fixture provider");
        let cancellation = CancellationToken::new();
        let mut stream = provider
            .stream_chat(request(), cancellation.clone())
            .await
            .expect("accepted stream");

        assert_eq!(
            stream.next().await.expect("started").expect("event"),
            ProviderStreamEvent::Started
        );
        let partial = stream.next().await.expect("partial").expect("event");
        let ProviderStreamEvent::TextDelta(partial) = partial else {
            panic!("expected partial text");
        };
        assert_eq!(partial.as_str(), "partial");
        cancellation.cancel();
        let cancelled = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("cancellation must be prompt")
            .expect("cancelled event")
            .expect("normalized event");
        assert_eq!(cancelled, ProviderStreamEvent::Cancelled);
        assert!(stream.next().await.is_none());

        let (recorded, disconnected) = server.await.expect("fixture server");
        assert!(recorded.target().starts_with("/v1/interactions?"));
        assert!(
            disconnected,
            "server must observe the dropped response body"
        );
    }

    #[tokio::test]
    async fn cancellation_before_dispatch_performs_no_network_attempt() {
        let provider = GeminiProvider::for_test(
            GeminiApiKey::new("fixture-key").expect("key"),
            Url::parse("http://127.0.0.1:9/").expect("fixture URL"),
        )
        .expect("fixture provider");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = provider.stream_chat(request(), cancellation).await;
        let Err(error) = result else {
            panic!("cancelled dispatch must fail before returning a stream");
        };
        assert_eq!(error.kind(), ProviderErrorKind::Cancelled);
        assert_eq!(error.retry_advice(), RetryAdvice::Never);
    }

    #[tokio::test]
    async fn credential_bearing_model_identity_is_rejected_before_dispatch() {
        let provider = GeminiProvider::for_test(
            GeminiApiKey::new("sensitive-model").expect("key"),
            Url::parse("http://127.0.0.1:9/").expect("fixture URL"),
        )
        .expect("fixture provider");
        let request = ChatRequest::new(
            ModelId::new("models/sensitive-model").expect("model"),
            vec![ChatMessage {
                role: ChatRole::User,
                content: ChatContent::new("hello").expect("content"),
            }],
        )
        .expect("request");

        let result = provider
            .stream_chat(request, CancellationToken::new())
            .await;
        let Err(error) = result else {
            panic!("secret-bearing model identity must fail before dispatch");
        };
        assert_eq!(error.kind(), ProviderErrorKind::InvalidRequest);
    }

    #[tokio::test]
    async fn redirects_are_not_followed_with_the_credential() {
        let (base_url, server) = spawn(vec![
            ResponseSpec::json(302, b"redirect refused".as_slice())
                .with_header("Location", "http://127.0.0.1:9/credential-sink"),
        ])
        .await;
        let provider =
            GeminiProvider::for_test(GeminiApiKey::new("fixture-key").expect("key"), base_url)
                .expect("fixture provider");

        let result = provider
            .stream_chat(request(), CancellationToken::new())
            .await;
        let Err(error) = result else {
            panic!("redirect response must be rejected");
        };
        assert_eq!(error.kind(), ProviderErrorKind::Protocol);
        let requests = server.await.expect("fixture server");
        assert_eq!(requests.len(), 1);
    }
}
