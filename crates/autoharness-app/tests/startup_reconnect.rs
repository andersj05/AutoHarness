use std::sync::Arc;

use autoharness_app::profiles::{ProfileManager, ProfileStore};
use autoharness_app::vault::FakeVault;
use autoharness_settings::{LayerKind, ProfileId, ProviderProfile, SettingsBuilder};

#[test]
fn startup_resolution_publishes_provider_status_from_profile() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let profiles_path = dir.path().join("autoharness.profiles.json");
    let vault = Arc::new(FakeVault::new());

    // Seed a profile with a stored credential as a returning user would have.
    let store = ProfileStore::open(&profiles_path).expect("open profile store");
    let manager = ProfileManager::new(store, vault.clone());
    let id = ProfileId::new("home-router").expect("id");
    let profile = ProviderProfile::router(
        "https://router.example.test/base/",
        Some("home".to_owned()),
        None,
    )
    .expect("router profile");
    manager.upsert(&id, &profile).expect("upsert profile");
    manager
        .save_credential(&id, "AIzaSy-returning-user-key")
        .expect("linked");
    manager.activate(Some(&id)).expect("activated");

    // Compose startup exactly like main.rs does.
    let user_json = std::fs::read_to_string(&profiles_path).expect("read profiles document");
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, user_json)
        .resolve()
        .expect("settings resolve");

    let environment: Vec<(String, String)> = Vec::new();
    let source = autoharness_app::ProfileCredentialResolver::new(vault.as_ref())
        .with_environment(environment)
        .resolve(&settings)
        .expect("credential resolves after restart");

    assert_eq!(
        source.source_name(),
        autoharness_app::CredentialSourceName::CredentialVault,
        "a returning profile must reconnect from the vault without prompting"
    );
    assert_eq!(
        source.provider_kind(),
        Some(autoharness_settings::ProviderKind::Router)
    );
    assert!(!source.credential().is_empty());

    // The projection the client receives names the same safe facts.
    let status = autoharness_client::runtime::ProviderStatusProjection {
        active_profile: source.profile_id().map(str::to_owned),
        provider_kind: source.provider_kind().map(|kind| match kind {
            autoharness_settings::ProviderKind::Gemini => {
                autoharness_client::runtime::ProviderKindLabel::Gemini
            }
            autoharness_settings::ProviderKind::Router => {
                autoharness_client::runtime::ProviderKindLabel::Router
            }
            autoharness_settings::ProviderKind::CodexCli => {
                autoharness_client::runtime::ProviderKindLabel::CodexCli
            }
        }),
        credential_source: match source.source_name() {
            autoharness_app::CredentialSourceName::Environment => {
                autoharness_client::runtime::CredentialSourceLabel::Environment
            }
            autoharness_app::CredentialSourceName::CredentialVault => {
                autoharness_client::runtime::CredentialSourceLabel::CredentialVault
            }
            autoharness_app::CredentialSourceName::SessionOnly => {
                autoharness_client::runtime::CredentialSourceLabel::SessionOnly
            }
        },
        credential_connected: !source.credential().is_empty(),
    };
    assert_eq!(status.active_profile.as_deref(), Some("home-router"));

    // The raw secret stays out of the on-disk profile document.
    let document = std::fs::read_to_string(&profiles_path).expect("profile file");
    assert!(!document.contains("AIzaSy-returning-user-key"));
    assert_eq!(manager.snapshot().expect("snapshot").profiles.len(), 1);
}
