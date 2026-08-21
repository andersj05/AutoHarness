use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use autoharness_domain::{ClassifiedError, ProviderId, RetryAdvice};
use autoharness_provider::{
    CancellationToken, Catalog, CatalogFreshness, CatalogRequest, Chat, ChatRequest, ChatRole,
    ModelCatalog, ProviderAvailability, ProviderError, ProviderErrorKind, ProviderEventStream,
    ProviderMetadata, ProviderStreamEvent, SecretRedactor, SseDecoder,
};
use futures_util::StreamExt as _;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{Method, Response, Url};
use serde::Serialize;
use serde_json::Value;

use crate::models::{ModelsPage, request_model_name};
use crate::native_stream::{NativeStreamState, classify_error_value};
use crate::{RouterCredential, RouterSettings};

const CATALOG_PAGE_SIZE: u16 = 1000;
const MAX_CATALOG_PAGES: usize = 1000;
const MAX_CURSOR_BYTES: usize = 16 * 1024;
const MAX_CATALOG_PAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_SSE_FRAME_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_HEADERS_TIMEOUT: Duration = Duration::from_secs(30);

/// Configurable OpenAI-compatible model-router provider.
#[derive(Clone)]
pub struct OpenAiRouterProvider {
    client: reqwest::Client,
    credential: Arc<RouterCredential>,
    settings: RouterSettings,
}

impl OpenAiRouterProvider {
    /// Constructs a router adapter from validated non-secret settings and credential.
    pub fn new(
        settings: RouterSettings,
        credential: RouterCredential,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| internal_error())?;
        Ok(Self {
            client,
            credential: Arc::new(credential),
            settings,
        })
    }

    /// Reads router settings and the credential from environment variables.
    pub fn from_env() -> Result<Self, ProviderError> {
        Self::new(RouterSettings::from_env()?, RouterCredential::from_env()?)
    }

    fn auth_header_value(&self) -> Result<HeaderValue, ProviderError> {
        let raw = if self.settings.auth_scheme().is_empty() {
            self.credential.expose().to_owned()
        } else {
            format!(
                "{} {}",
                self.settings.auth_scheme(),
                self.credential.expose()
            )
        };
        let mut header = HeaderValue::from_str(&raw).map_err(|_| {
            ProviderError::new(ProviderErrorKind::Authentication, RetryAdvice::Never)
        })?;
        header.set_sensitive(true);
        Ok(header)
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
            .header(self.settings.auth_header(), self.auth_header_value()?)
            .header(ACCEPT, accept);
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(cancelled_error()),
            result = tokio::time::timeout(RESPONSE_HEADERS_TIMEOUT, request.send()) => result,
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
        let body = match read_bounded(response, MAX_ERROR_BODY_BYTES, cancellation).await {
            Ok(body) => body,
            Err(error) if error.kind() == ProviderErrorKind::Cancelled => return Err(error),
            Err(_) => Vec::new(),
        };
        let value = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        let mut error = classify_error_value(&value, Some(status));
        if let Some(delay_ms) = retry_after
            && matches!(error.retry_advice(), RetryAdvice::Backoff)
        {
            error = ProviderError::new(error.kind(), RetryAdvice::After { delay_ms })
                .with_http_status(status.as_u16());
        }
        Err(error)
    }

    fn decode_stream(
        &self,
        response: Response,
        cancellation: CancellationToken,
    ) -> ProviderEventStream {
        let credential = Arc::clone(&self.credential);
        Box::pin(async_stream::stream! {
            let mut bytes = response.bytes_stream();
            let mut decoder = SseDecoder::new(MAX_SSE_FRAME_BYTES);
            let mut state = NativeStreamState::new();
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
                    } else {
                        yield Err(protocol_error());
                    }
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
                    let events = match state.handle(frame, &credential) {
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

impl Debug for OpenAiRouterProvider {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAiRouterProvider")
            .field("provider_id", self.settings.provider_id())
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl ProviderMetadata for OpenAiRouterProvider {
    fn provider_id(&self) -> &ProviderId {
        self.settings.provider_id()
    }

    fn availability(&self) -> ProviderAvailability {
        ProviderAvailability::Ready
    }
}

impl SecretRedactor for OpenAiRouterProvider {
    fn redact_secrets(&self, value: &str) -> String {
        self.credential.redact(value)
    }
}

#[async_trait]
impl Catalog for OpenAiRouterProvider {
    async fn list_models(
        &self,
        _request: CatalogRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelCatalog, ProviderError> {
        let mut cursor = None;
        let mut seen = HashSet::new();
        let mut models = BTreeMap::new();
        for _ in 0..MAX_CATALOG_PAGES {
            let mut url = self.settings.models_endpoint()?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("limit", &CATALOG_PAGE_SIZE.to_string());
                if let Some(cursor) = cursor.as_deref() {
                    query.append_pair("after", cursor);
                }
            }
            if self.credential.contains(url.as_str()) {
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
            let body = read_bounded(response, MAX_CATALOG_PAGE_BYTES, &cancellation)
                .await
                .map_err(classify_catalog_read_error)?;
            let page: ModelsPage = serde_json::from_slice(&body).map_err(|_| protocol_error())?;
            let fallback_cursor = page
                .data
                .last()
                .and_then(|model| model.cursor().map(str::to_owned));
            for native in page.data {
                if let Some(model) = native.into_descriptor(self.provider_id(), &self.credential) {
                    models
                        .entry(model.model_id.as_str().to_owned())
                        .or_insert(model);
                }
            }
            if !page.has_more {
                return Ok(ModelCatalog::new(
                    models.into_values().collect(),
                    CatalogFreshness::Live,
                ));
            }
            let next = page
                .last_id
                .or(fallback_cursor)
                .ok_or_else(protocol_error)?;
            if next.len() > MAX_CURSOR_BYTES
                || self.credential.contains(&next)
                || !seen.insert(next.clone())
            {
                return Err(protocol_error());
            }
            cursor = Some(next);
        }
        Err(ProviderError::new(
            ProviderErrorKind::LimitExceeded,
            RetryAdvice::Never,
        ))
    }
}

#[async_trait]
impl Chat for OpenAiRouterProvider {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError> {
        let model = request_model_name(&request.model_id, &self.credential)?;
        let body = chat_body(model, &request, &self.credential)?;
        if body.len() > MAX_REQUEST_BODY_BYTES {
            return Err(ProviderError::new(
                ProviderErrorKind::LimitExceeded,
                RetryAdvice::Never,
            ));
        }
        let response = self
            .send(
                Method::POST,
                self.settings.chat_endpoint()?,
                Some(body),
                "text/event-stream",
                ReplaySafety::AmbiguousAfterDispatch,
                &cancellation,
            )
            .await?;
        let response = self.require_success(response, &cancellation).await?;
        if !is_event_stream(response.headers()) {
            return Err(protocol_error());
        }
        Ok(self.decode_stream(response, cancellation))
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
struct NativeChatRequest<'a> {
    model: &'a str,
    messages: Vec<NativeMessage>,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct NativeMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

fn chat_body(
    model: &str,
    request: &ChatRequest,
    credential: &RouterCredential,
) -> Result<Vec<u8>, ProviderError> {
    let messages = request
        .messages
        .iter()
        .map(|message| NativeMessage {
            role: match message.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
            },
            content: credential.redact(message.content.as_str()),
        })
        .collect();
    serde_json::to_vec(&NativeChatRequest {
        model,
        messages,
        stream: true,
        stream_options: StreamOptions {
            include_usage: true,
        },
    })
    .map_err(|_| internal_error())
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
        .map(|seconds| seconds.min(86_400).saturating_mul(1000))
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

fn cancelled_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Cancelled, RetryAdvice::Never)
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

fn internal_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoharness_domain::ModelId;
    use autoharness_provider::{
        CapabilitySupport, ChatContent, ChatMessage, TextDelta, UsageSnapshot,
    };

    use crate::test_http::{ResponseSpec, spawn, spawn_slow_sse};

    fn request() -> ChatRequest {
        ChatRequest::new(
            ModelId::new("model-a").expect("model"),
            vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: ChatContent::new("first router-secret").expect("content"),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: ChatContent::new("second").expect("content"),
                },
            ],
        )
        .expect("request")
    }

    fn provider(base_url: Url) -> OpenAiRouterProvider {
        let settings = RouterSettings::new(base_url, Some("fixture"))
            .expect("settings")
            .with_authentication("x-router-key", "Token")
            .expect("authentication");
        OpenAiRouterProvider::new(
            settings,
            RouterCredential::new("router-secret").expect("credential"),
        )
        .expect("provider")
    }

    #[test]
    fn request_is_stateless_complete_history_with_usage_enabled() {
        let credential = RouterCredential::new("router-secret").expect("credential");
        let request = ChatRequest::new(
            ModelId::new("model-a").expect("model"),
            vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: ChatContent::new("first").expect("content"),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: ChatContent::new("second").expect("content"),
                },
            ],
        )
        .expect("request");
        let value: Value =
            serde_json::from_slice(&chat_body("model-a", &request, &credential).expect("body"))
                .expect("JSON");

        assert_eq!(value["model"], "model-a");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(2));
    }

    #[tokio::test]
    async fn paginated_catalog_uses_configured_header_and_shared_conformance() {
        let (base_url, server) = spawn(vec![
            ResponseSpec::json(
                200,
                include_bytes!("../tests/fixtures/models-page-1.json").as_slice(),
            ),
            ResponseSpec::json(
                200,
                include_bytes!("../tests/fixtures/models-page-2.json").as_slice(),
            ),
        ])
        .await;
        let provider = provider(base_url);

        let catalog = provider
            .list_models(CatalogRequest::Refresh, CancellationToken::new())
            .await
            .expect("catalog");
        autoharness_provider::conformance::assert_catalog(
            &catalog,
            provider.provider_id(),
            &["model-a", "model-z"],
        );
        assert_eq!(
            catalog.models()[0].capabilities.chat,
            CapabilitySupport::Unknown
        );
        assert_eq!(
            catalog.models()[1].capabilities.streaming,
            CapabilitySupport::Supported
        );
        assert!(
            catalog.models()[0]
                .description
                .as_deref()
                .is_some_and(|description| description.contains("[REDACTED]"))
        );
        let requests = server.await.expect("server");
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request.method() == "GET"
                && request.header_equals("x-router-key", "Token router-secret")
                && request.body().is_empty()
                && !request.target().contains("router-secret")
        }));
        let second =
            Url::parse(&format!("http://fixture{}", requests[1].target())).expect("target URL");
        assert_eq!(
            second
                .query_pairs()
                .find(|(name, _)| name == "after")
                .map(|(_, value)| value.into_owned()),
            Some("opaque +/= cursor?".to_owned())
        );
        autoharness_provider::conformance::assert_secret_redaction(&provider, "router-secret");
    }

    #[tokio::test]
    async fn recorded_chat_stream_normalizes_lifecycle_usage_and_redaction() {
        let mut body = include_bytes!("../tests/fixtures/chat-stream.sse").to_vec();
        body.extend_from_slice(b"\r\n");
        let (base_url, server) = spawn(vec![ResponseSpec::sse(body)]).await;
        let provider = provider(base_url);
        let stream = provider
            .stream_chat(request(), CancellationToken::new())
            .await
            .expect("stream");
        let events = autoharness_provider::conformance::collect_stream(stream).await;

        autoharness_provider::conformance::assert_stream_lifecycle(&events);
        autoharness_provider::conformance::assert_normal_completion(&events);
        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Started,
                ProviderStreamEvent::TextDelta(TextDelta::new("hello ").expect("delta")),
                ProviderStreamEvent::TextDelta(TextDelta::new("[REDACTED] world").expect("delta")),
                ProviderStreamEvent::Usage(UsageSnapshot {
                    input_tokens: Some(4),
                    output_tokens: Some(2),
                    cached_input_tokens: Some(1),
                    reasoning_tokens: Some(1),
                    tool_tokens: None,
                    total_tokens: Some(6),
                }),
                ProviderStreamEvent::Completed {
                    reason: autoharness_provider::CompletionReason::Stop,
                },
            ]
        );
        let requests = server.await.expect("server");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method(), "POST");
        assert!(requests[0].header_equals("x-router-key", "Token router-secret"));
        let body: Value = serde_json::from_slice(requests[0].body()).expect("request JSON");
        assert_eq!(body["model"], "model-a");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
        assert!(
            !std::str::from_utf8(requests[0].body())
                .expect("UTF-8")
                .contains("router-secret")
        );
    }

    #[tokio::test]
    async fn cancellation_aborts_in_progress_router_stream() {
        let initial = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\r\n",
            "\r\n"
        )
        .as_bytes()
        .to_vec();
        let (base_url, server) = spawn_slow_sse(initial).await;
        let provider = provider(base_url);
        let cancellation = CancellationToken::new();
        let mut stream = provider
            .stream_chat(request(), cancellation.clone())
            .await
            .expect("stream");

        assert_eq!(
            stream.next().await.expect("started").expect("event"),
            ProviderStreamEvent::Started
        );
        assert!(matches!(
            stream.next().await.expect("delta").expect("event"),
            ProviderStreamEvent::TextDelta(_)
        ));
        cancellation.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(500), stream.next())
                .await
                .expect("prompt cancellation")
                .expect("cancel event")
                .expect("event"),
            ProviderStreamEvent::Cancelled
        );
        drop(stream);
        let (_, disconnected) = server.await.expect("server");
        assert!(disconnected);
    }

    #[tokio::test]
    async fn cancellation_before_dispatch_is_non_retryable() {
        let provider = provider(Url::parse("http://127.0.0.1:9/").expect("URL"));
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let Err(error) = provider.stream_chat(request(), cancellation).await else {
            panic!("cancelled dispatch must fail");
        };
        autoharness_provider::conformance::assert_non_retryable(
            &error,
            ProviderErrorKind::Cancelled,
        );
    }

    #[tokio::test]
    async fn redirects_are_rejected_without_forwarding_credentials() {
        let (base_url, server) = spawn(vec![
            ResponseSpec::json(302, b"redirect refused".as_slice())
                .with_header("Location", "http://127.0.0.1:9/credential-sink"),
        ])
        .await;
        let provider = provider(base_url);

        let Err(error) = provider
            .stream_chat(request(), CancellationToken::new())
            .await
        else {
            panic!("redirect must fail");
        };
        assert_eq!(error.kind(), ProviderErrorKind::Protocol);
        assert_eq!(server.await.expect("server").len(), 1);
    }
}
