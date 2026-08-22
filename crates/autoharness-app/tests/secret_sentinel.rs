use std::fs;

use autoharness_app::profiles::ProfileStore;
use autoharness_app::vault::{FakeVault, VaultPort};
use autoharness_settings::{CredentialReference, LayerKind, ProfileId, SettingsBuilder};

const SECRET: &str = "AIzaSy-SENTINEL-secret-value-do-not-persist";

fn seeded_store(dir: &tempfile::TempDir) -> (ProfileStore, FakeVault, CredentialReference) {
    let path = dir.path().join("autoharness.profiles.json");
    let vault = FakeVault::new();
    let store = ProfileStore::open(&path).expect("open store");
    store
        .upsert_profile(
            "home-router",
            r#"{"kind": "router", "base_url": "https://router.example.test/base/", "project": "home"}"#,
        )
        .expect("upsert");
    let reference = store
        .link_credential(&vault, "home-router", SECRET)
        .expect("linked");
    (store, vault, reference)
}

#[test]
fn no_durable_file_ever_contains_the_secret() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, _vault, _reference) = seeded_store(&dir);

    // Every file the application wrote in this flow is scanned.
    let mut scanned = 0;
    for entry in fs::read_dir(dir.path()).expect("data directory") {
        let entry = entry.expect("directory entry");
        if !entry.path().is_file() {
            continue;
        }
        let contents = fs::read_to_string(entry.path()).unwrap_or_default();
        assert!(
            !contents.contains(SECRET),
            "secret leaked into {}",
            entry.path().display()
        );
        scanned += 1;
    }
    assert!(scanned >= 1, "expected at least one durable file");

    // The resolved settings document also stays clean.
    let document = store.read_document().expect("document");
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, document)
        .resolve()
        .expect("settings");
    let rendered = format!("{resolved:?}");
    assert!(!rendered.contains(SECRET), "debug output leaked the secret");
}

#[test]
fn disconnect_removes_both_reference_and_vault_entry() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, reference) = seeded_store(&dir);

    store
        .disconnect_credential(&vault, &reference)
        .expect("disconnect");

    assert!(
        matches!(
            vault.load(&reference),
            Err(autoharness_app::vault::VaultError::MissingEntry)
        ),
        "the vault entry must be gone after disconnect"
    );
    let document = store.read_document().expect("document");
    assert!(!document.contains(reference.as_str()));
    assert!(document.contains("home-router"), "profile itself remains");
}

#[test]
fn replacing_a_credential_rotates_without_plaintext_on_disk() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, old_reference) = seeded_store(&dir);

    let rotated = format!("{SECRET}-rotated");
    vault
        .replace(&old_reference, &rotated)
        .expect("rotate in place");

    let document = store.read_document().expect("document");
    assert!(!document.contains(&rotated));
    assert!(!document.contains(SECRET));
    assert_eq!(
        document.matches(old_reference.as_str()).count(),
        1,
        "exactly one stable reference"
    );

    let loaded = vault.load(&old_reference).expect("reload");
    assert_eq!(&*loaded, rotated.as_str());
}

#[test]
fn profile_deletion_leaves_no_credential_reference_behind() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, reference) = seeded_store(&dir);
    store
        .set_active_profile(Some(&ProfileId::new("home-router").expect("id")))
        .expect("activate");

    store.delete_profile("home-router").expect("deleted");
    vault.delete(&reference).expect("vault cleaned by flow");

    let document = store.read_document().expect("document");
    assert!(!document.contains("home-router"));
    assert!(!document.contains(reference.as_str()));
    assert!(!document.contains("active_profile"));
}
