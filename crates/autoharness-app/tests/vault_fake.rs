use autoharness_settings::CredentialReference;

use autoharness_app::vault::{FakeVault, VaultError, VaultPort};

const REFERENCE: &str = "autoharness/profile/home-router";
const SECRET: &str = "AIzaSy-test-secret-value-000";

#[test]
fn saved_credentials_round_trip_and_delete_removes_them() {
    let vault = FakeVault::new();

    let reference = vault
        .save(REFERENCE, SECRET)
        .expect("save into the fake vault");
    assert_eq!(reference.as_str(), REFERENCE);

    let loaded = vault.load(&reference).expect("load from the fake vault");
    assert_eq!(&*loaded, SECRET);

    vault
        .delete(&reference)
        .expect("delete from the fake vault");
    assert!(
        matches!(vault.load(&reference), Err(VaultError::MissingEntry)),
        "deleted credentials must not resolve"
    );
}

#[test]
fn replacing_a_reference_overwrites_the_previous_secret() {
    let vault = FakeVault::new();
    let reference = vault.save(REFERENCE, SECRET).expect("initial save");

    vault
        .replace(&reference, "rotated-secret")
        .expect("replace in place");

    let loaded = vault.load(&reference).expect("load after replace");
    assert_eq!(&*loaded, "rotated-secret");
}

#[test]
fn loading_an_unknown_reference_reports_missing_entry() {
    let vault = FakeVault::new();
    let unknown = CredentialReference::new("autoharness/profile/unknown").expect("valid reference");

    assert!(matches!(
        vault.load(&unknown),
        Err(VaultError::MissingEntry)
    ));
}

#[test]
fn empty_secrets_are_rejected() {
    let vault = FakeVault::new();
    assert!(matches!(
        vault.save(REFERENCE, ""),
        Err(VaultError::InvalidSecret(_))
    ));
}

#[test]
fn debug_output_never_contains_secret_material() {
    let vault = FakeVault::new();
    let reference = vault.save(REFERENCE, SECRET).expect("save");
    let _loaded = vault.load(&reference).expect("load");

    let rendered = format!("{vault:?}");
    assert!(!rendered.contains(SECRET));
    assert!(!rendered.contains("AIzaSy"));
}

#[test]
fn opaque_oauth_payloads_can_exceed_the_manual_api_key_bound() {
    let vault = FakeVault::new();
    let payload = "x".repeat(8 * 1024);
    let reference = vault
        .save("autoharness/profile/codex", &payload)
        .expect("save opaque OAuth payload");
    let loaded = vault.load(&reference).expect("load opaque OAuth payload");
    assert_eq!(loaded.len(), payload.len());
    assert_eq!(&*loaded, &payload);
}
