use std::fs;

use autoharness_app::profiles::ProfileStore;
use autoharness_app::vault::FakeVault;
use autoharness_settings::ProfileId;

fn store_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary directory")
}

const ROUTER_PROFILE_JSON: &str = r#"{
    "schema_version": 1,
    "profiles": {
        "home-router": {
            "kind": "router",
            "base_url": "https://router.example.test/base/",
            "project": "home"
        }
    }
}"#;

#[test]
fn saved_profile_persists_across_store_reopen() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");

    {
        let store = ProfileStore::open(&path).expect("open new store");
        store
            .upsert_profile("home-router", ROUTER_PROFILE_JSON)
            .expect("upsert profile");
    }

    let reopened = ProfileStore::open(&path).expect("reopen store");
    assert_eq!(
        reopened.active_profile().expect("read active profile"),
        None,
        "no active profile yet"
    );
    let document = reopened.read_document().expect("read document");
    assert!(document.contains("home-router"));
    assert!(document.contains("router.example.test"));
}

#[test]
fn set_active_profile_updates_document_atomically() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let store = ProfileStore::open(&path).expect("open store");
    store
        .upsert_profile("home-router", ROUTER_PROFILE_JSON)
        .expect("upsert");

    store
        .set_active_profile(Some(&ProfileId::new("home-router").expect("id")))
        .expect("activate profile");

    let document = store.read_document().expect("read after activation");
    assert!(document.contains("\"active_profile\": \"home-router\""));
}

#[test]
fn delete_profile_removes_it_and_clears_active_reference() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let store = ProfileStore::open(&path).expect("open store");
    store
        .upsert_profile("home-router", ROUTER_PROFILE_JSON)
        .expect("upsert");
    store
        .set_active_profile(Some(&ProfileId::new("home-router").expect("id")))
        .expect("activate");

    store.delete_profile("home-router").expect("delete profile");

    let document = store.read_document().expect("read after deletion");
    assert!(!document.contains("home-router"));
    assert!(!document.contains("active_profile"));
}

#[test]
fn malformed_existing_file_is_backed_up_and_replaced() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    fs::write(&path, "{corrupted").expect("seed malformed file");

    let store = ProfileStore::open(&path).expect("recovery open");
    let document = store.read_document().expect("fresh document");
    assert!(document.contains("\"schema_version\""));

    let backup = dir.path().join("profiles.json.bad");
    let recovered = fs::read_to_string(&backup).expect("backup exists");
    assert_eq!(recovered, "{corrupted");
}

#[test]
fn link_credential_writes_reference_without_storing_the_secret() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let vault = FakeVault::new();
    let store = ProfileStore::open(&path).expect("open store");
    store
        .upsert_profile("home-router", ROUTER_PROFILE_JSON)
        .expect("upsert");

    let reference = store
        .link_credential(&vault, "home-router", "AIzaSy-test-secret-000")
        .expect("link credential");

    let document = store.read_document().expect("read after linking");
    assert!(document.contains(reference.as_str()));
    assert!(
        !document.contains("AIzaSy-test-secret-000"),
        "raw secret must never enter the settings file"
    );
    assert!(
        !fs::read_to_string(&path)
            .expect("file contents")
            .contains("secret"),
        "secret bytes must not reach disk"
    );
}

#[test]
fn unlink_credential_removes_the_reference_but_keeps_the_profile() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let vault = FakeVault::new();
    let store = ProfileStore::open(&path).expect("open store");
    store
        .upsert_profile("home-router", ROUTER_PROFILE_JSON)
        .expect("upsert");
    let reference = store
        .link_credential(&vault, "home-router", "another-secret-value")
        .expect("link");

    store.unlink_credential(&reference).expect("unlink");

    let document = store.read_document().expect("read after unlink");
    assert!(document.contains("home-router"));
    assert!(!document.contains(reference.as_str()));
}
