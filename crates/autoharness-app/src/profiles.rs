//! Durable provider-profile storage and recoverable credential management.
//!
//! The profile document contains only non-secret settings, opaque vault
//! references, and bounded recovery records. `ProfileManager` serializes
//! mutations across that document and the operating-system vault.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use autoharness_settings::{
    CredentialRecoveryKind, CredentialRecoveryRecord, CredentialReference, LayerKind, LocalProfile,
    ProfileId, ProviderProfile, ResolvedSettings, SETTINGS_SCHEMA_VERSION, SettingsBuilder,
    SettingsDocument,
};
use zeroize::Zeroizing;

use crate::vault::{VaultError, VaultPort};

/// Errors surfaced by profile-store operations without secret detail.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProfileStoreError {
    /// The directory or file could not be created, parsed, or written.
    Io,
    /// The proposed change violates profile validation rules.
    Invalid(&'static str),
    /// The referenced profile does not exist.
    UnknownProfile,
}

impl std::fmt::Display for ProfileStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io => formatter.write_str("the profile settings file could not be updated"),
            Self::Invalid(reason) => write!(formatter, "{reason}"),
            Self::UnknownProfile => formatter.write_str("that profile does not exist"),
        }
    }
}

impl std::error::Error for ProfileStoreError {}

/// Safe failures from one application-owned profile management operation.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProfileManagementError {
    /// The non-secret profile document could not be updated.
    Store(ProfileStoreError),
    /// The platform vault rejected an operation before a document commit.
    Vault(VaultError),
    /// The requested operation conflicts with the profile's current state.
    Conflict(&'static str),
    /// The selected profile has no linked stored credential.
    CredentialNotStored,
    /// The safe document state committed, but vault cleanup remains pending.
    RecoveryPending,
}

impl std::fmt::Display for ProfileManagementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Vault(error) => error.fmt(formatter),
            Self::Conflict(reason) => formatter.write_str(reason),
            Self::CredentialNotStored => {
                formatter.write_str("that profile has no stored credential")
            }
            Self::RecoveryPending => formatter
                .write_str("the profile is safe, but credential-vault cleanup remains pending"),
        }
    }
}

impl std::error::Error for ProfileManagementError {}

impl From<ProfileStoreError> for ProfileManagementError {
    fn from(error: ProfileStoreError) -> Self {
        Self::Store(error)
    }
}

/// Safe credential state for one provider profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredCredentialState {
    /// No vault reference is linked.
    Disconnected,
    /// The profile links one deterministic vault entry.
    Stored,
    /// A restart-safe save or cleanup operation still needs reconciliation.
    RecoveryPending,
}

/// One provider profile projected for application and TUI consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedProfile {
    /// Stable validated profile identity.
    pub id: ProfileId,
    /// Validated non-secret provider configuration.
    pub profile: ProviderProfile,
    /// Whether this profile currently selects the runtime provider.
    pub active: bool,
    /// Safe stored-credential status without secret metadata.
    pub credential_state: StoredCredentialState,
}

/// Complete safe profile-management read model.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileSnapshot {
    /// Profiles in stable identity order.
    pub profiles: Vec<ManagedProfile>,
    /// Number of non-secret recovery operations still pending.
    pub pending_recovery: usize,
}

/// Durable non-secret profile document.
#[derive(Clone, Debug)]
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    /// Opens or recovers the profile document at `path`.
    pub fn open(path: &Path) -> Result<Self, ProfileStoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| ProfileStoreError::Io)?;
        }
        match fs::read_to_string(path) {
            Ok(existing) => {
                let trimmed = existing.trim();
                let parsed = serde_json::from_str::<serde_json::Value>(trimmed).ok();
                let is_future_schema = parsed
                    .as_ref()
                    .and_then(|value| value.get("schema_version"))
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|version| version > u64::from(SETTINGS_SCHEMA_VERSION));
                if trimmed.is_empty()
                    || parsed
                        .as_ref()
                        .and_then(serde_json::Value::as_object)
                        .is_none()
                    || (!is_future_schema
                        && serde_json::from_str::<SettingsDocument>(trimmed).is_err())
                {
                    let backup = backup_path(path);
                    let _ = fs::rename(path, &backup);
                    write_default_document(path)?;
                }
            }
            Err(_) => write_default_document(path)?,
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Returns the raw non-secret document text.
    pub fn read_document(&self) -> Result<String, ProfileStoreError> {
        fs::read_to_string(&self.path).map_err(|_| ProfileStoreError::Io)
    }

    /// Returns the active profile name from the parsed document.
    pub fn active_profile(&self) -> Result<Option<String>, ProfileStoreError> {
        let document = self.parsed()?;
        Ok(document
            .get("active_profile")
            .and_then(|value| value.as_str())
            .map(str::to_owned))
    }

    /// Returns one fully validated safe snapshot.
    pub fn snapshot(&self) -> Result<ProfileSnapshot, ProfileStoreError> {
        let text = self.read_document()?;
        let resolved = SettingsBuilder::new()
            .with_layer(LayerKind::UserFile, text.clone())
            .resolve()
            .map_err(|_| ProfileStoreError::Io)?;
        let active = resolved.active_profile();
        let recovery =
            recovery_records(&serde_json::from_str(&text).map_err(|_| ProfileStoreError::Io)?)?;
        let profiles = resolved
            .profiles()
            .map(|(id, profile)| ManagedProfile {
                id: id.clone(),
                profile: profile.clone(),
                active: active == Some(id.as_str()),
                credential_state: if recovery.iter().any(|record| record.profile() == id) {
                    StoredCredentialState::RecoveryPending
                } else if profile.credential_reference().is_some() {
                    StoredCredentialState::Stored
                } else {
                    StoredCredentialState::Disconnected
                },
            })
            .collect();
        Ok(ProfileSnapshot {
            profiles,
            pending_recovery: recovery.len(),
        })
    }

    /// Returns all effective local settings resolved from the durable user layer.
    pub fn resolved_settings(&self) -> Result<ResolvedSettings, ProfileStoreError> {
        SettingsBuilder::new()
            .with_layer(LayerKind::UserFile, self.read_document()?)
            .resolve()
            .map_err(|_| ProfileStoreError::Io)
    }

    /// Replaces the typed non-secret local profile preference layer atomically.
    pub fn set_local_profile(&self, local_profile: LocalProfile) -> Result<(), ProfileStoreError> {
        let serialized = serde_json::to_value(local_profile).map_err(|_| ProfileStoreError::Io)?;
        self.mutate_document(|document| {
            let object = document.as_object_mut().ok_or(ProfileStoreError::Io)?;
            if serialized
                .as_object()
                .is_some_and(serde_json::Map::is_empty)
            {
                object.remove("local_profile");
            } else {
                object.insert("local_profile".to_owned(), serialized);
            }
            Ok(())
        })
    }

    /// Returns the persisted local profile layer without resolving lower layers.
    pub fn local_profile(&self) -> Result<LocalProfile, ProfileStoreError> {
        serde_json::from_str::<SettingsDocument>(&self.read_document()?)
            .map(|document| document.local_profile().clone())
            .map_err(|_| ProfileStoreError::Io)
    }

    /// Inserts or replaces one validated non-secret profile definition.
    ///
    /// Existing credential linkage is preserved; disconnect is a separate
    /// explicit operation.
    pub fn upsert_profile(
        &self,
        id: &ProfileId,
        profile: &ProviderProfile,
    ) -> Result<(), ProfileStoreError> {
        let mut replacement = serde_json::to_value(profile).map_err(|_| ProfileStoreError::Io)?;
        self.mutate_document(|document| {
            let object = document.as_object_mut().expect("documents are objects");
            let profiles = object
                .entry("profiles")
                .or_insert_with(|| serde_json::json!({}));
            let Some(profiles) = profiles.as_object_mut() else {
                return Err(ProfileStoreError::Invalid(
                    "the profiles section is not a JSON object",
                ));
            };
            if let Some(existing) = profiles.get(id.as_str()) {
                let existing_kind = existing.get("kind").and_then(serde_json::Value::as_str);
                let replacement_kind = replacement.get("kind").and_then(serde_json::Value::as_str);
                let existing_credential = existing.get("credential").cloned();
                if existing_credential.is_some() && existing_kind != replacement_kind {
                    return Err(ProfileStoreError::Invalid(
                        "disconnect the stored credential before changing provider kind",
                    ));
                }
                if let Some(existing_credential) = existing_credential {
                    replacement
                        .as_object_mut()
                        .expect("provider profiles serialize as objects")
                        .insert("credential".to_owned(), existing_credential);
                }
            }
            profiles.insert(id.as_str().to_owned(), replacement);
            Ok(())
        })
    }

    /// Activates one declared profile, or deactivates when `None`.
    pub fn set_active_profile(&self, profile: Option<&ProfileId>) -> Result<(), ProfileStoreError> {
        self.mutate_document(|document| {
            let object = document.as_object_mut().expect("documents are objects");
            match profile {
                Some(id) => {
                    let declared = object
                        .get("profiles")
                        .and_then(|profiles| profiles.get(id.as_str()))
                        .is_some();
                    if !declared {
                        return Err(ProfileStoreError::UnknownProfile);
                    }
                    object.insert(
                        "active_profile".to_owned(),
                        serde_json::Value::String(id.as_str().to_owned()),
                    );
                }
                None => {
                    object.remove("active_profile");
                }
            }
            Ok(())
        })
    }

    fn profile(&self, id: &ProfileId) -> Result<ProviderProfile, ProfileStoreError> {
        self.snapshot()?
            .profiles
            .into_iter()
            .find(|candidate| candidate.id == *id)
            .map(|candidate| candidate.profile)
            .ok_or(ProfileStoreError::UnknownProfile)
    }

    fn begin_save(
        &self,
        profile: &ProfileId,
        reference: &CredentialReference,
    ) -> Result<CredentialRecoveryRecord, ProfileStoreError> {
        self.profile(profile)?;
        let record = CredentialRecoveryRecord::new(
            profile.clone(),
            reference.clone(),
            CredentialRecoveryKind::UncommittedSave,
        );
        self.mutate_document(|document| push_recovery(document, &record))?;
        Ok(record)
    }

    fn commit_save(&self, record: &CredentialRecoveryRecord) -> Result<(), ProfileStoreError> {
        self.mutate_document(|document| {
            let profile = document
                .get_mut("profiles")
                .and_then(|profiles| profiles.get_mut(record.profile().as_str()))
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(ProfileStoreError::UnknownProfile)?;
            profile.insert(
                "credential".to_owned(),
                serde_json::json!({ "reference": record.reference().as_str() }),
            );
            remove_recovery(document, record)
        })
    }

    fn begin_disconnect(
        &self,
        profile_id: &ProfileId,
    ) -> Result<Option<CredentialRecoveryRecord>, ProfileStoreError> {
        let profile = self.profile(profile_id)?;
        let Some(reference) = profile.credential_reference() else {
            return Ok(None);
        };
        let reference = CredentialReference::new(reference).map_err(ProfileStoreError::Invalid)?;
        let record = CredentialRecoveryRecord::new(
            profile_id.clone(),
            reference,
            CredentialRecoveryKind::Delete,
        );
        self.mutate_document(|document| {
            let profile = document
                .get_mut("profiles")
                .and_then(|profiles| profiles.get_mut(profile_id.as_str()))
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(ProfileStoreError::UnknownProfile)?;
            profile.remove("credential");
            push_recovery(document, &record)
        })?;
        Ok(Some(record))
    }

    fn begin_delete(
        &self,
        profile_id: &ProfileId,
    ) -> Result<CredentialRecoveryRecord, ProfileStoreError> {
        self.profile(profile_id)?;
        let reference = deterministic_reference(profile_id)?;
        let record = CredentialRecoveryRecord::new(
            profile_id.clone(),
            reference,
            CredentialRecoveryKind::Delete,
        );
        self.mutate_document(|document| {
            let object = document.as_object_mut().expect("documents are objects");
            let profiles = object
                .get_mut("profiles")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(ProfileStoreError::UnknownProfile)?;
            if profiles.remove(profile_id.as_str()).is_none() {
                return Err(ProfileStoreError::UnknownProfile);
            }
            if profiles.is_empty() {
                object.remove("profiles");
            }
            if object
                .get("active_profile")
                .and_then(serde_json::Value::as_str)
                == Some(profile_id.as_str())
            {
                object.remove("active_profile");
            }
            push_recovery(document, &record)
        })?;
        Ok(record)
    }

    fn finish_recovery(&self, record: &CredentialRecoveryRecord) -> Result<(), ProfileStoreError> {
        self.mutate_document(|document| remove_recovery(document, record))
    }

    fn recovery_records(&self) -> Result<Vec<CredentialRecoveryRecord>, ProfileStoreError> {
        recovery_records(&self.parsed()?)
    }

    fn profile_links(
        &self,
        profile: &ProfileId,
        reference: &CredentialReference,
    ) -> Result<bool, ProfileStoreError> {
        Ok(self
            .parsed()?
            .get("profiles")
            .and_then(|profiles| profiles.get(profile.as_str()))
            .and_then(|profile| profile.get("credential"))
            .and_then(|credential| credential.get("reference"))
            .and_then(serde_json::Value::as_str)
            == Some(reference.as_str()))
    }

    fn parsed(&self) -> Result<serde_json::Value, ProfileStoreError> {
        let text = self.read_document()?;
        serde_json::from_str(&text).map_err(|_| ProfileStoreError::Io)
    }

    fn mutate_document<F>(&self, mutate: F) -> Result<(), ProfileStoreError>
    where
        F: FnOnce(&mut serde_json::Value) -> Result<(), ProfileStoreError>,
    {
        let document = serde_json::from_str::<SettingsDocument>(&self.read_document()?)
            .map_err(|_| ProfileStoreError::Io)?;
        let mut document = serde_json::to_value(document).map_err(|_| ProfileStoreError::Io)?;
        mutate(&mut document)?;

        let object = document.as_object_mut().ok_or(ProfileStoreError::Io)?;
        object.insert(
            "schema_version".to_owned(),
            serde_json::json!(SETTINGS_SCHEMA_VERSION),
        );

        let rendered =
            serde_json::to_string_pretty(&document).map_err(|_| ProfileStoreError::Io)?;
        let temporary = temporary_path(&self.path);
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temporary)
                .map_err(|_| ProfileStoreError::Io)?;
            writeln!(file, "{rendered}").map_err(|_| ProfileStoreError::Io)?;
            file.sync_all().map_err(|_| ProfileStoreError::Io)?;
        }
        fs::rename(&temporary, &self.path).map_err(|_| ProfileStoreError::Io)?;
        Ok(())
    }
}

/// Serialized application workflow for profile and credential lifecycle.
pub struct ProfileManager {
    store: ProfileStore,
    vault: Arc<dyn VaultPort>,
}

impl std::fmt::Debug for ProfileManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProfileManager")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

impl ProfileManager {
    /// Creates a manager around one profile document and one platform vault.
    #[must_use]
    pub fn new(store: ProfileStore, vault: Arc<dyn VaultPort>) -> Self {
        Self { store, vault }
    }

    /// Returns the latest safe profile projection.
    pub fn snapshot(&self) -> Result<ProfileSnapshot, ProfileManagementError> {
        self.store.snapshot().map_err(Into::into)
    }

    /// Returns effective local settings with per-leaf provenance.
    pub fn resolved_settings(&self) -> Result<ResolvedSettings, ProfileManagementError> {
        self.store.resolved_settings().map_err(Into::into)
    }

    /// Replaces the typed non-secret local profile preference layer.
    pub fn set_local_profile(
        &self,
        local_profile: LocalProfile,
    ) -> Result<(), ProfileManagementError> {
        self.store
            .set_local_profile(local_profile)
            .map_err(Into::into)
    }

    /// Returns the persisted local profile layer for one typed preference mutation.
    pub fn local_profile(&self) -> Result<LocalProfile, ProfileManagementError> {
        self.store.local_profile().map_err(Into::into)
    }

    /// Inserts or edits one profile while preserving credential linkage.
    pub fn upsert(
        &self,
        id: &ProfileId,
        profile: &ProviderProfile,
    ) -> Result<(), ProfileManagementError> {
        validate_profile(profile)?;
        self.store.upsert_profile(id, profile).map_err(Into::into)
    }

    /// Duplicates non-secret configuration without sharing credential linkage.
    pub fn duplicate(
        &self,
        source: &ProfileId,
        destination: &ProfileId,
    ) -> Result<(), ProfileManagementError> {
        if self.store.profile(destination).is_ok() {
            return Err(ProfileManagementError::Conflict(
                "the destination profile already exists",
            ));
        }
        let profile = self.store.profile(source)?.without_credential();
        validate_profile(&profile)?;
        self.store.upsert_profile(destination, &profile)?;
        Ok(())
    }

    /// Selects one profile as active, or enters session-only default mode.
    pub fn activate(&self, profile: Option<&ProfileId>) -> Result<(), ProfileManagementError> {
        self.store.set_active_profile(profile).map_err(Into::into)
    }
    /// Sets or clears one profile's default model identifier.
    pub fn set_default_model(
        &self,
        profile: &ProfileId,
        model: Option<String>,
    ) -> Result<(), ProfileManagementError> {
        let configured = self
            .store
            .profile(profile)?
            .with_default_model(model)
            .map_err(ProfileStoreError::Invalid)?;
        self.store.upsert_profile(profile, &configured)?;
        Ok(())
    }

    /// Atomically sets a profile's default model and reasoning effort.
    pub fn set_agent_defaults(
        &self,
        profile: &ProfileId,
        model: String,
        reasoning_effort: Option<String>,
    ) -> Result<(), ProfileManagementError> {
        let configured = self
            .store
            .profile(profile)?
            .with_default_model(Some(model))
            .and_then(|profile| profile.with_default_reasoning_effort(reasoning_effort))
            .map_err(ProfileStoreError::Invalid)?;
        self.store.upsert_profile(profile, &configured)?;
        Ok(())
    }

    /// Saves a first credential through the restart-safe three-step protocol.
    pub fn save_credential(
        &self,
        profile: &ProfileId,
        secret: &str,
    ) -> Result<CredentialReference, ProfileManagementError> {
        if self
            .store
            .profile(profile)?
            .credential_reference()
            .is_some()
        {
            return Err(ProfileManagementError::Conflict(
                "a stored credential already exists; replace it explicitly",
            ));
        }
        let reference = deterministic_reference(profile)?;
        let record = self.store.begin_save(profile, &reference)?;
        if let Err(error) = self.vault.save(reference.as_str(), secret) {
            return Err(ProfileManagementError::Vault(error));
        }
        if self.store.commit_save(&record).is_err() {
            return Err(ProfileManagementError::RecoveryPending);
        }
        Ok(reference)
    }

    /// Replaces exactly one linked stored credential.
    pub fn replace_credential(
        &self,
        profile: &ProfileId,
        secret: &str,
    ) -> Result<(), ProfileManagementError> {
        let configured = self.store.profile(profile)?;
        let reference = configured
            .credential_reference()
            .ok_or(ProfileManagementError::CredentialNotStored)?;
        let reference = CredentialReference::new(reference)
            .map_err(ProfileStoreError::Invalid)
            .map_err(ProfileManagementError::Store)?;
        self.vault
            .replace(&reference, secret)
            .map_err(ProfileManagementError::Vault)
    }

    /// Disconnects one profile before idempotently deleting its vault entry.
    pub fn disconnect(&self, profile: &ProfileId) -> Result<(), ProfileManagementError> {
        let Some(record) = self.store.begin_disconnect(profile)? else {
            return Ok(());
        };
        self.finish_vault_delete(&record)
    }

    /// Deletes one profile and schedules cleanup of its deterministic vault entry.
    pub fn delete(&self, profile: &ProfileId) -> Result<(), ProfileManagementError> {
        let record = self.store.begin_delete(profile)?;
        self.finish_vault_delete(&record)
    }

    /// Loads one linked credential into zeroizing memory for a connection test.
    pub fn credential_for_test(
        &self,
        profile: &ProfileId,
    ) -> Result<Zeroizing<String>, ProfileManagementError> {
        let configured = self.store.profile(profile)?;
        let reference = configured
            .credential_reference()
            .ok_or(ProfileManagementError::CredentialNotStored)?;
        let reference = CredentialReference::new(reference)
            .map_err(ProfileStoreError::Invalid)
            .map_err(ProfileManagementError::Store)?;
        self.vault
            .load(&reference)
            .map_err(ProfileManagementError::Vault)
    }

    /// Loads every configured profile credential into zeroizing memory for exact redaction.
    ///
    /// The result is ordered by opaque profile identity and fails closed when any linked vault
    /// entry cannot be read.
    pub fn configured_credentials_for_redaction(
        &self,
    ) -> Result<Vec<Zeroizing<String>>, ProfileManagementError> {
        let snapshot = self.snapshot()?;
        if snapshot.pending_recovery != 0 {
            return Err(ProfileManagementError::RecoveryPending);
        }
        let mut profiles = snapshot.profiles;
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        profiles
            .into_iter()
            .filter(|profile| profile.credential_state == StoredCredentialState::Stored)
            .map(|profile| self.credential_for_test(&profile.id))
            .collect()
    }

    /// Reconciles every durable recovery record idempotently.
    pub fn recover_pending(&self) -> Result<usize, ProfileManagementError> {
        let records = self.store.recovery_records()?;
        let mut remaining = 0;
        for record in records {
            let result = match record.kind() {
                CredentialRecoveryKind::UncommittedSave
                    if self
                        .store
                        .profile_links(record.profile(), record.reference())? =>
                {
                    self.store.finish_recovery(&record).map_err(Into::into)
                }
                CredentialRecoveryKind::UncommittedSave | CredentialRecoveryKind::Delete => {
                    self.finish_vault_delete(&record)
                }
            };
            if result.is_err() {
                remaining += 1;
            }
        }
        Ok(remaining)
    }

    fn finish_vault_delete(
        &self,
        record: &CredentialRecoveryRecord,
    ) -> Result<(), ProfileManagementError> {
        match self.vault.delete(record.reference()) {
            Ok(()) | Err(VaultError::MissingEntry) => {}
            Err(_) => return Err(ProfileManagementError::RecoveryPending),
        }
        self.store
            .finish_recovery(record)
            .map_err(|_| ProfileManagementError::RecoveryPending)
    }
}

fn validate_profile(profile: &ProviderProfile) -> Result<(), ProfileManagementError> {
    if profile.kind() != autoharness_settings::ProviderKind::Router {
        return Ok(());
    }
    let base_url = profile
        .base_url()
        .ok_or(ProfileManagementError::Conflict(
            "router profiles require a base URL",
        ))?
        .parse::<autoharness_provider_openai::RouterUrl>()
        .map_err(|_| ProfileManagementError::Conflict("the router base URL is invalid"))?;
    let settings = autoharness_provider_openai::RouterSettings::new(base_url, profile.project())
        .map_err(|_| ProfileManagementError::Conflict("the router base URL is not allowed"))?;
    match profile.auth_header() {
        Some(header) => settings
            .with_authentication(header, "Bearer")
            .map(|_| ())
            .map_err(|_| ProfileManagementError::Conflict("the router auth header is invalid")),
        None => Ok(()),
    }
}

fn deterministic_reference(profile: &ProfileId) -> Result<CredentialReference, ProfileStoreError> {
    CredentialReference::new(format!("autoharness/profile/{}", profile.as_str()))
        .map_err(ProfileStoreError::Invalid)
}

fn push_recovery(
    document: &mut serde_json::Value,
    record: &CredentialRecoveryRecord,
) -> Result<(), ProfileStoreError> {
    let object = document.as_object_mut().ok_or(ProfileStoreError::Io)?;
    let recovery = object
        .entry("credential_recovery")
        .or_insert_with(|| serde_json::json!([]));
    let recovery = recovery.as_array_mut().ok_or(ProfileStoreError::Invalid(
        "credential recovery state is not an array",
    ))?;
    recovery.retain(|candidate| {
        serde_json::from_value::<CredentialRecoveryRecord>(candidate.clone())
            .map(|existing| existing.reference() != record.reference())
            .unwrap_or(false)
    });
    recovery.push(serde_json::to_value(record).map_err(|_| ProfileStoreError::Io)?);
    Ok(())
}

fn remove_recovery(
    document: &mut serde_json::Value,
    record: &CredentialRecoveryRecord,
) -> Result<(), ProfileStoreError> {
    let object = document.as_object_mut().ok_or(ProfileStoreError::Io)?;
    let Some(recovery) = object
        .get_mut("credential_recovery")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };
    recovery.retain(|candidate| {
        serde_json::from_value::<CredentialRecoveryRecord>(candidate.clone())
            .map(|existing| existing != *record)
            .unwrap_or(true)
    });
    if recovery.is_empty() {
        object.remove("credential_recovery");
    }
    Ok(())
}

fn recovery_records(
    document: &serde_json::Value,
) -> Result<Vec<CredentialRecoveryRecord>, ProfileStoreError> {
    document.get("credential_recovery").cloned().map_or_else(
        || Ok(Vec::new()),
        |value| serde_json::from_value(value).map_err(|_| ProfileStoreError::Io),
    )
}

fn write_default_document(path: &Path) -> Result<(), ProfileStoreError> {
    let document = serde_json::json!({
        "schema_version": SETTINGS_SCHEMA_VERSION,
    });
    let rendered = serde_json::to_string_pretty(&document).map_err(|_| ProfileStoreError::Io)?;
    let temporary = temporary_path(path);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|_| ProfileStoreError::Io)?;
        writeln!(file, "{rendered}").map_err(|_| ProfileStoreError::Io)?;
        file.sync_all().map_err(|_| ProfileStoreError::Io)?;
    }
    fs::rename(&temporary, path).map_err(|_| ProfileStoreError::Io)?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    PathBuf::from(temporary)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".bad");
    PathBuf::from(backup)
}
