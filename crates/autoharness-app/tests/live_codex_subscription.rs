use std::path::PathBuf;

use autoharness_app::vault::{KeyringVault, VaultPort};
use autoharness_domain::ModelId;
use autoharness_provider::{
    CancellationToken, Chat, ChatContent, ChatMessage, ChatRequest, ChatRole, CompletionReason,
    ProviderStreamEvent,
};
use autoharness_provider_codex_cli::{CodexProvider, CodexSettings};
use autoharness_settings::{
    CredentialReference, LayerKind, ProfileId, ProviderKind, SettingsBuilder,
};
use futures_util::StreamExt as _;

#[tokio::test]
#[ignore = "opt-in live Codex subscription check; requires the user's configured profile"]
async fn luna_high_completes_through_the_native_adapter() {
    if std::env::var("AUTOHARNESS_RUN_CODEX_LIVE").as_deref() != Ok("1") {
        return;
    }

    let profile_path = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
        .join("AutoHarness")
        .join("autoharness.profiles.json");
    let document = std::fs::read_to_string(profile_path).expect("read profile document");
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, document)
        .resolve()
        .expect("resolve profile document");
    let profile_id = ProfileId::new(resolved.active_profile().expect("active profile"))
        .expect("valid profile ID");
    let profile = resolved
        .profile(&profile_id)
        .expect("active profile exists");
    assert_eq!(profile.kind(), ProviderKind::CodexCli);
    let reference = CredentialReference::new(
        profile
            .credential_reference()
            .expect("stored Codex credential"),
    )
    .expect("valid credential reference");
    let credential = KeyringVault::new()
        .load(&reference)
        .expect("load Codex credential");

    let provider = CodexProvider::new(
        CodexSettings::new()
            .expect("Codex settings")
            .with_reasoning_effort(Some("high"))
            .expect("high reasoning"),
        &credential,
        None,
    )
    .expect("Codex provider");
    let request = ChatRequest::new(
        ModelId::new("gpt-5.6-luna").expect("Luna model ID"),
        vec![ChatMessage::text(
            ChatRole::User,
            ChatContent::new("Reply with exactly: luna-ok").expect("prompt"),
        )],
    )
    .expect("chat request");
    let mut stream = provider
        .stream_chat(request, CancellationToken::new())
        .await
        .expect("start Luna response");
    let mut text = String::new();
    let mut completed = false;
    while let Some(event) = stream.next().await {
        match event.expect("valid provider event") {
            ProviderStreamEvent::TextDelta(delta) => text.push_str(delta.as_str()),
            ProviderStreamEvent::Completed {
                reason: CompletionReason::Stop,
            } => completed = true,
            _ => {}
        }
    }

    assert!(completed, "Luna response did not complete normally");
    assert_eq!(text.trim(), "luna-ok");
}
