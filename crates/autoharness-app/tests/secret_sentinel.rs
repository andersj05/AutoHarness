use std::fs;
use std::sync::Arc;

use autoharness_app::profiles::{ProfileManager, ProfileStore};
use autoharness_app::vault::{FakeVault, VaultError, VaultPort};
use autoharness_settings::{
    CredentialReference, LayerKind, ProfileId, ProviderProfile, SettingsBuilder,
};

const SECRET: &str = "AIzaSy-SENTINEL-secret-value-do-not-persist";

fn seeded_store(
    dir: &tempfile::TempDir,
) -> (
    ProfileStore,
    Arc<FakeVault>,
    ProfileManager,
    ProfileId,
    CredentialReference,
) {
    let path = dir.path().join("autoharness.profiles.json");
    let vault = Arc::new(FakeVault::new());
    let store = ProfileStore::open(&path).expect("open store");
    let manager = ProfileManager::new(store.clone(), vault.clone());
    let id = ProfileId::new("home-router").expect("id");
    let profile = ProviderProfile::router(
        "https://router.example.test/base/",
        Some("home".to_owned()),
        None,
    )
    .expect("router profile");
    manager.upsert(&id, &profile).expect("upsert");
    let reference = manager.save_credential(&id, SECRET).expect("linked");
    (store, vault, manager, id, reference)
}

#[test]
fn no_durable_file_ever_contains_the_secret() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, _vault, _manager, _id, _reference) = seeded_store(&dir);

    let mut scanned = 0;
    for entry in fs::read_dir(dir.path()).expect("data directory") {
        let entry = entry.expect("directory entry");
        if entry.file_type().expect("file type").is_file() {
            scanned += 1;
            let bytes = fs::read(entry.path()).expect("durable file");
            assert!(
                !bytes
                    .windows(SECRET.len())
                    .any(|window| window == SECRET.as_bytes()),
                "credential marker reached durable file"
            );
        }
    }
    assert!(scanned >= 1, "expected at least one durable file");

    let document = store.read_document().expect("document");
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, document)
        .resolve()
        .expect("settings");
    assert!(!format!("{resolved:?}").contains(SECRET));
}

#[test]
fn disconnect_removes_both_reference_and_vault_entry() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, manager, id, reference) = seeded_store(&dir);

    manager.disconnect(&id).expect("disconnect");

    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
    let document = store.read_document().expect("document");
    assert!(!document.contains(reference.as_str()));
    assert!(document.contains(id.as_str()), "profile itself remains");
}

#[test]
fn replacing_a_credential_rotates_without_plaintext_on_disk() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, manager, id, reference) = seeded_store(&dir);
    let rotated = format!("{SECRET}-rotated");

    manager
        .replace_credential(&id, &rotated)
        .expect("rotate in place");

    let document = store.read_document().expect("document");
    assert!(!document.contains(&rotated));
    assert!(!document.contains(SECRET));
    assert_eq!(document.matches(reference.as_str()).count(), 1);
    assert_eq!(&*vault.load(&reference).expect("reload"), rotated.as_str());
}

#[test]
fn profile_deletion_leaves_no_credential_reference_behind() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let (store, vault, manager, id, reference) = seeded_store(&dir);
    manager.activate(Some(&id)).expect("activate");

    manager.delete(&id).expect("delete");

    let document = store.read_document().expect("document");
    assert!(!document.contains(id.as_str()));
    assert!(!document.contains(reference.as_str()));
    assert!(!document.contains("active_profile"));
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
}
