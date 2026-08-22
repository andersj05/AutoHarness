use autoharness_provider::{
    CancellationToken, Catalog as _, CatalogRequest, Chat as _, ChatContent, ChatMessage,
    ChatRequest, ChatRole, CompletionReason, ModelDescriptor, ProviderStreamEvent,
    ProviderToolDefinition,
};
use autoharness_provider_gemini::GeminiProvider;
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

async fn model(provider: &GeminiProvider) -> ModelDescriptor {
    let models = provider
        .list_models(CatalogRequest::Refresh, CancellationToken::new())
        .await
        .expect("live Gemini catalog")
        .into_models();
    // Google retires model generations for new users while still listing them in
    // discovery, and catalog names sort unpredictably across families and preview
    // models. Prefer the newest verified Gemini generation, then fall back to any
    // other capable model so the probe always names its selection in failures.
    const PREFERRED_SUFFIXES: [&str; 3] =
        ["gemini-3.6-flash", "gemini-3.5-flash", "gemini-2.5-flash"];
    let capable = |descriptor: &&ModelDescriptor| {
        descriptor.capabilities.supports_streamed_chat()
            && descriptor.capabilities.supports_tool_calling()
    };
    let all: Vec<&ModelDescriptor> = models.iter().filter(capable).collect();
    assert!(!all.is_empty(), "a tool-capable Gemini model");
    for suffix in PREFERRED_SUFFIXES {
        if let Some(found) = all
            .iter()
            .find(|descriptor| descriptor.model_id.as_str().ends_with(suffix))
        {
            return (*found).clone();
        }
    }
    (*all[0]).clone()
}

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY and live Google AI Studio access"]
async fn live_plain_chat_accepts_the_complete_phase_three_registry() {
    let provider = GeminiProvider::from_env().expect("configured Gemini provider");
    let model = model(&provider).await;
    let selected_model = format!("{:?}", model.model_id);
    let request = ChatRequest::new(
        model.model_id,
        vec![ChatMessage::text(
            ChatRole::User,
            ChatContent::new("Reply with a brief greeting without calling a tool.")
                .expect("probe prompt"),
        )],
    )
    .expect("probe request")
    .with_tools(registry());
    let mut stream = provider
        .stream_chat(request, CancellationToken::new())
        .await
        .unwrap_or_else(|error| panic!("live Gemini stream with {selected_model}: {error}"));
    let mut started = false;
    let mut text_observed = false;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        match event.expect("normalized live event") {
            ProviderStreamEvent::Started => started = true,
            ProviderStreamEvent::TextDelta(_) => text_observed = true,
            ProviderStreamEvent::Completed {
                reason: CompletionReason::Stop,
            } => {
                completed = true;
                break;
            }
            ProviderStreamEvent::Completed { .. }
            | ProviderStreamEvent::Cancelled
            | ProviderStreamEvent::ToolCall(_)
            | ProviderStreamEvent::Usage(_) => {}
        }
    }
    assert!(started && text_observed && completed);
}

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY and live Google AI Studio access"]
async fn live_http_function_call_is_complete_before_tool_completion() {
    let provider = GeminiProvider::from_env().expect("configured Gemini provider");
    let model = model(&provider).await;
    let selected_model = format!("{:?}", model.model_id);
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
        .unwrap_or_else(|error| panic!("live Gemini stream with {selected_model}: {error}"));
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
