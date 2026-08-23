use autoharness_app::vault::{FakeVault, VaultError, VaultPort};
use autoharness_app::{CredentialSourceName, ProfileCredentialResolver};
use autoharness_settings::{CredentialReference, LayerKind, ProviderKind, SettingsBuilder};

struct UnavailableVault;

impl VaultPort for UnavailableVault {
    fn save(&self, _: &str, _: &str) -> Result<CredentialReference, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn load(&self, _: &CredentialReference) -> Result<zeroize::Zeroizing<String>, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn replace(&self, _: &CredentialReference, _: &str) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }

    fn delete(&self, _: &CredentialReference) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }
}

fn layers_json(profile: Option<&str>, credential: bool) -> String {
    let credential_block = if credential {
        r#",
            "credential": {"reference": "autoharness/profile/home-router"}"#
    } else {
        ""
    };
    let active = profile.map_or_else(String::new, |name| {
        format!(r#", "active_profile": "{name}""#)
    });
    format!(
        r#"{{
    "schema_version": 1,
    "profiles": {{
        "home-router": {{
            "kind": "router",
            "base_url": "https://router.example.test/base/",
            "project": "home"{credential_block}
        }}
    }}{active}
}}"#
    )
}

#[test]
fn environment_credential_wins_over_everything() {
    let vault = FakeVault::new();
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), true))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .with_environment([("AUTOHARNESS_ROUTER_API_KEY", "env-secret")])
        .resolve(&settings)
        .expect("resolution succeeds");

    assert_eq!(
        resolved.source_name(),
        CredentialSourceName::Environment,
        "environment must take precedence over the vault"
    );
    assert_eq!(resolved.credential(), "env-secret");
    assert_eq!(resolved.profile_id(), Some("home-router"));
    assert_eq!(resolved.provider_kind(), Some(ProviderKind::Router));
}

#[test]
fn environment_credential_must_match_the_effective_provider() {
    let vault = FakeVault::new();
    let gemini_settings = SettingsBuilder::new()
        .with_environment([("AUTOHARNESS_PROVIDER", "gemini")])
        .resolve()
        .expect("Gemini settings");
    let gemini = ProfileCredentialResolver::new(&vault)
        .with_environment([
            ("AUTOHARNESS_ROUTER_API_KEY", "router-secret"),
            ("GEMINI_API_KEY", "gemini-secret"),
        ])
        .resolve(&gemini_settings)
        .expect("Gemini credential");
    assert_eq!(gemini.credential(), "gemini-secret");
    assert_eq!(gemini.provider_kind(), Some(ProviderKind::Gemini));

    let router_settings = SettingsBuilder::new()
        .with_environment([("AUTOHARNESS_PROVIDER", "router")])
        .resolve()
        .expect("router settings");
    let router = ProfileCredentialResolver::new(&vault)
        .with_environment([
            ("AUTOHARNESS_ROUTER_API_KEY", "router-secret"),
            ("GEMINI_API_KEY", "gemini-secret"),
        ])
        .resolve(&router_settings)
        .expect("router credential");
    assert_eq!(router.credential(), "router-secret");
    assert_eq!(router.provider_kind(), Some(ProviderKind::Router));
}

#[test]
fn active_profile_with_vault_reference_reconnects_after_restart() {
    let vault = FakeVault::new();
    let reference = vault
        .save("autoharness/profile/home-router", "vault-stored-secret")
        .expect("seeded credential");
    let _ = reference;

    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), true))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve(&settings)
        .expect("reconnect from the vault");

    assert_eq!(
        resolved.source_name(),
        CredentialSourceName::CredentialVault
    );
    assert_eq!(resolved.credential(), "vault-stored-secret");
    assert_eq!(resolved.provider_kind(), Some(ProviderKind::Router));
}

#[test]
fn missing_vault_entry_degrades_to_session_only_without_failure() {
    let vault = FakeVault::new();
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), true))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve(&settings)
        .expect("degradation is not an error");

    assert_eq!(resolved.source_name(), CredentialSourceName::SessionOnly);
    assert!(resolved.credential().is_empty());
}

#[test]
fn locked_vault_degrades_to_session_only_without_failure() {
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), true))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&UnavailableVault)
        .resolve(&settings)
        .expect("locked vault degrades instead of blocking startup");

    assert_eq!(resolved.source_name(), CredentialSourceName::SessionOnly);
    assert_eq!(resolved.profile_id(), Some("home-router"));
    assert_eq!(resolved.provider_kind(), Some(ProviderKind::Router));
    assert!(resolved.credential().is_empty());
}

#[test]
fn profile_without_credential_link_is_session_only() {
    let vault = FakeVault::new();
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), false))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve(&settings)
        .expect("resolution succeeds");

    assert_eq!(resolved.source_name(), CredentialSourceName::SessionOnly);
}

#[test]
fn no_active_profile_means_no_provider_credential() {
    let vault = FakeVault::new();
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(None, false))
        .resolve()
        .expect("valid settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve(&settings)
        .expect("resolution succeeds");

    assert_eq!(resolved.source_name(), CredentialSourceName::SessionOnly);
    assert_eq!(resolved.provider_kind(), Some(ProviderKind::Gemini));
}

#[test]
fn provenance_reports_each_source_in_safe_terms() {
    let vault = FakeVault::new();
    vault
        .save("autoharness/profile/home-router", "s")
        .expect("saved");
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, layers_json(Some("home-router"), true))
        .resolve()
        .expect("settings");

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve(&settings)
        .expect("resolved");

    assert_eq!(
        resolved.source_description(),
        "credential vault profile 'home-router'"
    );
}

#[test]
fn unknown_active_profile_falls_back_to_environment_then_session_only() {
    let vault = FakeVault::new();
    // A document whose active_profile names nothing declared: the settings
    // resolver rejects that combination, so build the mismatch by hand.
    let document = serde_json::json!({
        "schema_version": 1,
        "active_profile": "ghost"
    });

    let resolved = ProfileCredentialResolver::new(&vault)
        .resolve_document(&document)
        .expect("degraded resolution succeeds");

    assert_eq!(resolved.source_name(), CredentialSourceName::SessionOnly);
    let _ = CredentialReference::new("unused").expect("type import kept alive");
}
