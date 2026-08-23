use autoharness_provider::{
    CancellationToken, Catalog as _, CatalogRequest, Chat as _, ChatContent, ChatMessage,
    ChatRequest, ChatRole, CompletionReason, ProviderStreamEvent, ProviderToolDefinition,
};
use autoharness_provider_openai::OpenAiRouterProvider;
use futures_util::StreamExt as _;

fn registry() -> Vec<ProviderToolDefinition> {
    autoharness_tool::definitions()
        .into_iter()
        .map(|definition| {
            ProviderToolDefinition::new_v1(
                definition.name,
                definition.description,
                definition.parameters,
            )
            .expect("trusted registry")
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires configured AUTOHARNESS_ROUTER_* variables and live router access"]
async fn live_router_streams_plain_chat_to_completion() {
    let provider = OpenAiRouterProvider::from_env().expect("configured router provider");
    let model = provider
        .list_models(CatalogRequest::Refresh, CancellationToken::new())
        .await
        .expect("live router catalog")
        .into_models()
        .into_iter()
        .find(|descriptor| descriptor.capabilities.supports_streamed_chat())
        .expect("a streaming router model");
    let request = ChatRequest::new(
        model.model_id,
        vec![ChatMessage::text(
            ChatRole::User,
            ChatContent::new("Reply with one short greeting.").expect("probe prompt"),
        )],
    )
    .expect("probe request");
    let mut stream = provider
        .stream_chat(request, CancellationToken::new())
        .await
        .expect("live router stream");
    let mut started = false;
    let mut text_bytes = 0_usize;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        match event.expect("normalized live event") {
            ProviderStreamEvent::Started => started = true,
            ProviderStreamEvent::TextDelta(delta) => {
                text_bytes = text_bytes.saturating_add(delta.as_str().len());
            }
            ProviderStreamEvent::Completed {
                reason: CompletionReason::Stop,
            } => {
                completed = true;
                break;
            }
            ProviderStreamEvent::Usage(_)
            | ProviderStreamEvent::ToolCall(_)
            | ProviderStreamEvent::Completed { .. }
            | ProviderStreamEvent::Cancelled => {}
        }
    }
    assert!(started && text_bytes > 0 && completed);
}

#[tokio::test]
#[ignore = "requires configured AUTOHARNESS_ROUTER_* variables and live router access"]
async fn live_router_normalizes_the_http_function_dialect() {
    let provider = OpenAiRouterProvider::from_env().expect("configured router provider");
    let model = provider
        .list_models(CatalogRequest::Refresh, CancellationToken::new())
        .await
        .expect("live router catalog")
        .into_models()
        .into_iter()
        .find(|descriptor| {
            descriptor.capabilities.supports_streamed_chat()
                && descriptor.capabilities.supports_tool_calling()
        })
        .expect("a tool-capable router model");
    let request = ChatRequest::new(
        model.model_id,
        vec![ChatMessage::text(
            ChatRole::User,
            ChatContent::new(
                "Call http_request once with GET and https://example.com, then wait for its result.",
            )
            .expect("probe prompt"),
        )],
    )
    .expect("probe request")
    .with_tools(registry());
    let mut stream = provider
        .stream_chat(request, CancellationToken::new())
        .await
        .expect("live router stream");
    let mut valid_http_call = false;
    let mut tool_completion = false;
    while let Some(event) = stream.next().await {
        match event.expect("normalized live event") {
            ProviderStreamEvent::ToolCall(call) if call.tool_name.as_str() == "http_request" => {
                let arguments = call.arguments.as_object();
                valid_http_call = arguments.get("method").and_then(|value| value.as_str())
                    == Some("GET")
                    && arguments.get("url").and_then(|value| value.as_str())
                        == Some("https://example.com");
            }
            ProviderStreamEvent::Completed {
                reason: CompletionReason::ToolCalls,
            } => {
                tool_completion = true;
                break;
            }
            ProviderStreamEvent::Started
            | ProviderStreamEvent::TextDelta(_)
            | ProviderStreamEvent::ToolCall(_)
            | ProviderStreamEvent::Usage(_)
            | ProviderStreamEvent::Completed { .. }
            | ProviderStreamEvent::Cancelled => {}
        }
    }
    assert!(valid_http_call && tool_completion);
}
