use std::sync::Arc;

use autoharness_app::profiles::ProfileStore;
use autoharness_app::vault::FakeVault;
use autoharness_settings::{LayerKind, SettingsBuilder};

#[test]
fn startup_resolution_publishes_provider_status_from_profile() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let profiles_path = dir.path().join("autoharness.profiles.json");
    let vault = FakeVault::new();

    // Seed a profile with a stored credential as a returning user would have.
    let store = ProfileStore::open(&profiles_path).expect("open profile store");
    store
        .upsert_profile(
            "home-router",
            r#"{"kind": "router", "base_url": "https://router.example.test/base/", "project": "home"}"#,
        )
        .expect("upsert profile");
    let reference = store
        .link_credential(&vault, "home-router", "AIzaSy-returning-user-key")
        .expect("linked");
    store
        .set_active_profile(Some(
            &autoharness_settings::ProfileId::new("home-router").expect("id"),
        ))
        .expect("activated");

    // Compose startup exactly like main.rs does.
    let user_json = std::fs::read_to_string(&profiles_path).expect("read profiles document");
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, user_json)
        .resolve()
        .expect("settings resolve");

    let environment: Vec<(String, String)> = Vec::new();
    let source = autoharness_app::ProfileCredentialResolver::new(&vault)
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

    // The projection the TUI receives names the same safe facts.
    let status = autoharness_tui::ProviderStatusProjection {
        active_profile: source.profile_id().map(str::to_owned),
        provider_kind: source.provider_kind().map(|kind| match kind {
            autoharness_settings::ProviderKind::Gemini => {
                autoharness_tui::ProviderKindLabel::Gemini
            }
            autoharness_settings::ProviderKind::Router => {
                autoharness_tui::ProviderKindLabel::Router
            }
        }),
        credential_source: match source.source_name() {
            autoharness_app::CredentialSourceName::Environment => {
                autoharness_tui::CredentialSourceLabel::Environment
            }
            autoharness_app::CredentialSourceName::CredentialVault => {
                autoharness_tui::CredentialSourceLabel::CredentialVault
            }
            autoharness_app::CredentialSourceName::SessionOnly => {
                autoharness_tui::CredentialSourceLabel::SessionOnly
            }
        },
        credential_connected: !source.credential().is_empty(),
    };
    assert_eq!(status.active_profile.as_deref(), Some("home-router"));

    // The raw secret stays out of the on-disk profile document.
    let document = std::fs::read_to_string(&profiles_path).expect("profile file");
    assert!(!document.contains("AIzaSy-returning-user-key"));
    let _ = Arc::new(());
    let _ = &reference;
}
