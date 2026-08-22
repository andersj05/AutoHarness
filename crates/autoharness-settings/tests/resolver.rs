use autoharness_settings::{
    LayerKind, ProfileId, ProviderKind, SettingsBuilder, SettingsError, Source,
};

const USER_JSON: &str = r#"{
    "schema_version": 1,
    "provider": "router",
    "profiles": {
        "home-router": {
            "kind": "router",
            "base_url": "https://router.example.test/base/",
            "project": "home",
            "credential": {"reference": "autoharness/profile/home-router"}
        }
    },
    "active_profile": "home-router"
}"#;

fn user_layer(json: &str) -> SettingsBuilder {
    SettingsBuilder::new().with_layer(LayerKind::UserFile, json)
}

#[test]
fn defaults_resolve_without_any_file() {
    let resolved = SettingsBuilder::new()
        .resolve()
        .expect("defaults always resolve");

    assert_eq!(resolved.provider(), None);
    assert!(resolved.profiles().next().is_none());
    assert_eq!(resolved.active_profile(), None);
}

#[test]
fn user_layer_supplies_provider_and_profiles() {
    let resolved = user_layer(USER_JSON).resolve().expect("valid user layer");

    assert_eq!(resolved.provider(), Some(ProviderKind::Router));
    let profile = resolved
        .profile(&ProfileId::new("home-router").expect("valid profile id"))
        .expect("declared profile");
    assert_eq!(
        profile.credential_reference(),
        Some("autoharness/profile/home-router")
    );
}

#[test]
fn environment_overrides_the_user_file() {
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, USER_JSON)
        .with_environment([("AUTOHARNESS_PROVIDER", "gemini")])
        .resolve()
        .expect("valid layers");

    assert_eq!(resolved.provider(), Some(ProviderKind::Gemini));
    let provenance = resolved.provenance();
    assert_eq!(
        provenance.get("provider").expect("provider is set"),
        &Source::Environment
    );
}

#[test]
fn workspace_cannot_override_protected_policy_keys() {
    let error = SettingsBuilder::new()
        .with_layer(
            LayerKind::WorkspaceFile,
            r#"{"schema_version": 1, "provider": "gemini"}"#,
        )
        .resolve()
        .expect_err("workspace provider selection must be rejected");

    assert!(matches!(
        error,
        SettingsError::DisallowedWorkspaceKey { key } if key == "provider"
    ));
}
