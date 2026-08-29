use std::fs;
use std::sync::{Arc, Mutex};

use autoharness_app::profiles::{
    ProfileManagementError, ProfileManager, ProfileStore, StoredCredentialState,
};
use autoharness_app::vault::{FakeVault, VaultError, VaultPort};
use autoharness_settings::{
    ColorMode, CredentialReference, DisplayLabel, GlyphMode, LocalPreferences, LocalProfile,
    ProfileId, PromptStatusDetail, ProviderProfile, Source,
};
use zeroize::Zeroizing;

fn store_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary directory")
}

fn router_profile() -> ProviderProfile {
    ProviderProfile::router(
        "https://router.example.test/base/",
        Some("home".to_owned()),
        None,
    )
    .expect("router profile")
}

fn profile_id(value: &str) -> ProfileId {
    ProfileId::new(value).expect("profile id")
}

fn manager(path: &std::path::Path) -> (ProfileStore, Arc<FakeVault>, ProfileManager, ProfileId) {
    let store = ProfileStore::open(path).expect("open store");
    let vault = Arc::new(FakeVault::new());
    let manager = ProfileManager::new(store.clone(), vault.clone());
    let id = profile_id("home-router");
    manager.upsert(&id, &router_profile()).expect("upsert");
    (store, vault, manager, id)
}

#[test]
fn saved_profile_persists_across_store_reopen() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (store, _, _, id) = manager(&path);
    drop(store);

    let reopened = ProfileStore::open(&path).expect("reopen store");
    let snapshot = reopened.snapshot().expect("snapshot");
    assert_eq!(snapshot.profiles.len(), 1);
    assert_eq!(snapshot.profiles[0].id, id);
    assert_eq!(snapshot.profiles[0].profile, router_profile());
    assert_eq!(
        snapshot.profiles[0].credential_state,
        StoredCredentialState::Disconnected
    );
}

#[test]
fn set_active_profile_updates_document_atomically() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (store, _, manager, id) = manager(&path);

    manager.activate(Some(&id)).expect("activate");

    assert_eq!(
        store.active_profile().expect("active"),
        Some(id.to_string())
    );
    assert!(
        store
            .read_document()
            .expect("document")
            .contains("active_profile")
    );
}

#[test]
fn delete_profile_clears_active_state_and_vault_entry() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (store, vault, manager, id) = manager(&path);
    let reference = manager
        .save_credential(&id, "stored-secret-value")
        .expect("save");
    manager.activate(Some(&id)).expect("activate");

    manager.delete(&id).expect("delete profile");

    assert!(manager.snapshot().expect("snapshot").profiles.is_empty());
    assert_eq!(store.active_profile().expect("active"), None);
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
    let document = store.read_document().expect("document");
    assert!(!document.contains(id.as_str()));
    assert!(!document.contains(reference.as_str()));
}

#[test]
fn malformed_existing_file_is_backed_up_and_replaced() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    fs::write(&path, "{corrupted").expect("write malformed file");

    let store = ProfileStore::open(&path).expect("recover store");

    let document = store.read_document().expect("replacement document");
    assert!(document.contains("\"schema_version\": 4"));
    let backup = dir.path().join("profiles.json.bad");
    assert_eq!(fs::read_to_string(backup).expect("backup"), "{corrupted");
}

#[test]
fn save_credential_links_one_reference_without_storing_secret() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (store, vault, manager, id) = manager(&path);
    let secret = "AIzaSy-test-secret-000";

    let reference = manager
        .save_credential(&id, secret)
        .expect("save credential");

    let document = store.read_document().expect("read after linking");
    assert_eq!(document.matches(reference.as_str()).count(), 1);
    assert!(!document.contains(secret));
    assert_eq!(&*vault.load(&reference).expect("vault load"), secret);
    let snapshot = manager.snapshot().expect("snapshot");
    assert_eq!(
        snapshot.profiles[0].credential_state,
        StoredCredentialState::Stored
    );
    assert_eq!(snapshot.pending_recovery, 0);
}

#[test]
fn redaction_credentials_include_inactive_profiles_in_stable_order() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, _, manager, home) = manager(&path);
    let work = profile_id("work-router");
    manager
        .upsert(&work, &router_profile())
        .expect("upsert work profile");
    manager
        .save_credential(&work, "work-profile-secret")
        .expect("save work credential");
    manager
        .save_credential(&home, "home-profile-secret")
        .expect("save home credential");
    manager
        .activate(Some(&work))
        .expect("activate work profile");

    let credentials = manager
        .configured_credentials_for_redaction()
        .expect("load redaction credentials");

    assert_eq!(credentials.len(), 2);
    assert_eq!(credentials[0].as_str(), "home-profile-secret");
    assert_eq!(credentials[1].as_str(), "work-profile-secret");
}

#[test]
fn redaction_credentials_fail_closed_when_a_linked_vault_entry_is_missing() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, vault, manager, id) = manager(&path);
    let reference = manager
        .save_credential(&id, "linked-profile-secret")
        .expect("save credential");
    vault.delete(&reference).expect("remove linked vault entry");

    assert!(matches!(
        manager.configured_credentials_for_redaction(),
        Err(ProfileManagementError::Vault(VaultError::MissingEntry))
    ));
}

#[test]
fn disconnect_removes_reference_and_keeps_profile() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (store, vault, manager, id) = manager(&path);
    let reference = manager
        .save_credential(&id, "another-secret-value")
        .expect("save");

    manager.disconnect(&id).expect("disconnect");

    let document = store.read_document().expect("read after unlink");
    assert!(document.contains(id.as_str()));
    assert!(!document.contains(reference.as_str()));
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
    assert_eq!(
        manager.snapshot().expect("snapshot").profiles[0].credential_state,
        StoredCredentialState::Disconnected
    );
}

#[test]
fn duplicate_copies_configuration_without_sharing_credential() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, _, manager, source) = manager(&path);
    manager
        .save_credential(&source, "source-secret-value")
        .expect("save");
    let copy = profile_id("work-router");

    manager.duplicate(&source, &copy).expect("duplicate");

    let snapshot = manager.snapshot().expect("snapshot");
    let duplicated = snapshot
        .profiles
        .iter()
        .find(|profile| profile.id == copy)
        .expect("duplicated profile");
    assert_eq!(duplicated.profile.kind(), router_profile().kind());
    assert_eq!(
        duplicated.credential_state,
        StoredCredentialState::Disconnected
    );
}

#[test]
fn profile_default_model_persists_without_disturbing_credential_linkage() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, _, manager, id) = manager(&path);
    manager
        .save_credential(&id, "profile-default-secret")
        .expect("save credential");

    manager
        .set_default_model(&id, Some("router-default-model".to_owned()))
        .expect("set default model");

    let profile = manager
        .snapshot()
        .expect("snapshot")
        .profiles
        .into_iter()
        .find(|profile| profile.id == id)
        .expect("profile");
    assert_eq!(
        profile.profile.default_model(),
        Some("router-default-model")
    );
    assert_eq!(profile.credential_state, StoredCredentialState::Stored);
}

#[test]
fn agent_defaults_persist_model_and_reasoning_together() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, _, manager, id) = manager(&path);

    manager
        .set_agent_defaults(&id, "gpt-5.6-terra".to_owned(), Some("high".to_owned()))
        .expect("set agent defaults");

    let profile = manager
        .snapshot()
        .expect("snapshot")
        .profiles
        .into_iter()
        .find(|profile| profile.id == id)
        .expect("profile");
    assert_eq!(profile.profile.default_model(), Some("gpt-5.6-terra"));
    assert_eq!(profile.profile.default_reasoning_effort(), Some("high"));
}

#[test]
fn local_preferences_persist_and_reset_to_inherited_defaults() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let (_, _, manager, _) = manager(&path);
    let mut preferences = LocalPreferences::new();
    preferences.set_color_mode(Some(ColorMode::NoColor));
    preferences.set_glyph_mode(Some(GlyphMode::Ascii));
    preferences.set_prompt_status_detail(Some(PromptStatusDetail::Detailed));
    let mut local_profile = LocalProfile::new();
    local_profile.set_display_label(Some(DisplayLabel::new("Jensen").expect("label")));
    local_profile.set_preferences(preferences);

    manager
        .set_local_profile(local_profile)
        .expect("persist local preferences");

    let resolved = manager
        .resolved_settings()
        .expect("resolve local preferences");
    assert_eq!(
        resolved
            .local_profile()
            .display_label()
            .value()
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("Jensen")
    );
    assert_eq!(
        resolved.local_profile().preferences().glyph_mode().value(),
        &GlyphMode::Ascii
    );
    assert_eq!(
        resolved.local_profile().preferences().glyph_mode().source(),
        Source::UserFile
    );
    assert_eq!(
        resolved
            .local_profile()
            .preferences()
            .prompt_status_detail()
            .value(),
        &PromptStatusDetail::Detailed
    );
    assert_eq!(
        resolved
            .local_profile()
            .preferences()
            .prompt_status_detail()
            .source(),
        Source::UserFile
    );

    let mut reset = manager.local_profile().expect("stored local preferences");
    let mut reset_preferences = reset.preferences().clone();
    reset_preferences.set_glyph_mode(None);
    reset_preferences.set_prompt_status_detail(None);
    reset.set_preferences(reset_preferences);
    manager
        .set_local_profile(reset)
        .expect("reset inherited glyph mode");

    let reopened = ProfileManager::new(
        ProfileStore::open(&path).expect("reopen"),
        Arc::new(FakeVault::new()),
    );
    let resolved = reopened
        .resolved_settings()
        .expect("resolve reopened preferences");
    assert_eq!(
        resolved.local_profile().preferences().glyph_mode().value(),
        &GlyphMode::Unicode
    );
    assert_eq!(
        resolved.local_profile().preferences().glyph_mode().source(),
        Source::Default
    );
    assert_eq!(
        resolved
            .local_profile()
            .preferences()
            .prompt_status_detail()
            .value(),
        &PromptStatusDetail::Workspace
    );
    assert_eq!(
        resolved
            .local_profile()
            .preferences()
            .prompt_status_detail()
            .source(),
        Source::Default
    );
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Fault {
    #[default]
    None,
    SaveAfterWrite,
    Delete,
}

#[derive(Default)]
struct FaultVault {
    inner: FakeVault,
    fault: Mutex<Fault>,
}

impl FaultVault {
    fn set_fault(&self, fault: Fault) {
        *self.fault.lock().expect("fault mutex") = fault;
    }
}

impl VaultPort for FaultVault {
    fn save(&self, reference: &str, secret: &str) -> Result<CredentialReference, VaultError> {
        let stored = self.inner.save(reference, secret)?;
        if *self.fault.lock().expect("fault mutex") == Fault::SaveAfterWrite {
            return Err(VaultError::Platform("injected save failure".to_owned()));
        }
        Ok(stored)
    }

    fn load(&self, reference: &CredentialReference) -> Result<Zeroizing<String>, VaultError> {
        self.inner.load(reference)
    }

    fn replace(&self, reference: &CredentialReference, secret: &str) -> Result<(), VaultError> {
        self.inner.replace(reference, secret)
    }

    fn delete(&self, reference: &CredentialReference) -> Result<(), VaultError> {
        if *self.fault.lock().expect("fault mutex") == Fault::Delete {
            return Err(VaultError::Unavailable);
        }
        self.inner.delete(reference)
    }
}

#[test]
fn interrupted_save_is_rolled_back_from_durable_recovery_record() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let store = ProfileStore::open(&path).expect("store");
    let vault = Arc::new(FaultVault::default());
    let manager = ProfileManager::new(store.clone(), vault.clone());
    let id = profile_id("home-router");
    manager.upsert(&id, &router_profile()).expect("upsert");
    vault.set_fault(Fault::SaveAfterWrite);

    assert!(matches!(
        manager.save_credential(&id, "uncommitted-secret"),
        Err(ProfileManagementError::Vault(VaultError::Platform(_)))
    ));
    assert_eq!(
        manager
            .snapshot()
            .expect("pending snapshot")
            .pending_recovery,
        1
    );
    assert!(
        !store
            .read_document()
            .expect("document")
            .contains("uncommitted-secret")
    );

    vault.set_fault(Fault::None);
    assert_eq!(manager.recover_pending().expect("recover"), 0);
    assert_eq!(
        manager
            .snapshot()
            .expect("recovered snapshot")
            .pending_recovery,
        0
    );
    let reference = CredentialReference::new("autoharness/profile/home-router").expect("reference");
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
}

#[test]
fn failed_disconnect_stays_visible_and_recovers_after_restart() {
    let dir = store_dir();
    let path = dir.path().join("profiles.json");
    let store = ProfileStore::open(&path).expect("store");
    let vault = Arc::new(FaultVault::default());
    let manager = ProfileManager::new(store.clone(), vault.clone());
    let id = profile_id("home-router");
    manager.upsert(&id, &router_profile()).expect("upsert");
    let reference = manager
        .save_credential(&id, "credential-to-delete")
        .expect("save");
    vault.set_fault(Fault::Delete);

    assert_eq!(
        manager.disconnect(&id),
        Err(ProfileManagementError::RecoveryPending)
    );
    let pending = manager.snapshot().expect("pending snapshot");
    assert_eq!(pending.pending_recovery, 1);
    assert_eq!(
        pending.profiles[0].credential_state,
        StoredCredentialState::RecoveryPending
    );
    assert!(
        !store
            .read_document()
            .expect("document")
            .contains("credential-to-delete")
    );

    vault.set_fault(Fault::None);
    let reopened = ProfileManager::new(ProfileStore::open(&path).expect("reopen"), vault.clone());
    assert_eq!(reopened.recover_pending().expect("recover"), 0);
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
    assert_eq!(
        reopened.snapshot().expect("snapshot").profiles[0].credential_state,
        StoredCredentialState::Disconnected
    );
}
