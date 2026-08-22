//! Durable provider-profile storage with credential linkage.
//!
//! The store owns one non-secret JSON document in the application data
//! directory. Credential material goes only to the vault port; the
//! document receives an opaque reference string.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use autoharness_settings::{CredentialReference, ProfileId, SETTINGS_SCHEMA_VERSION};

use crate::vault::VaultPort;

/// Errors surfaced by profile-store operations without secret detail.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProfileStoreError {
    /// The directory or file could not be created or written.
    Io,
    /// The proposed change violates profile validation rules.
    Invalid(&'static str),
    /// The referenced profile does not exist.
    UnknownProfile,
}

impl std::fmt::Display for ProfileStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io => formatter.write_str("the profile settings file could not be written"),
            Self::Invalid(reason) => write!(formatter, "{reason}"),
            Self::UnknownProfile => formatter.write_str("that profile does not exist"),
        }
    }
}

impl std::error::Error for ProfileStoreError {}

/// Durable non-secret profile document plus vault-backed credential links.
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    /// Opens (or recovers) the profile document at `path`.
    pub fn open(path: &Path) -> Result<Self, ProfileStoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| ProfileStoreError::Io)?;
        }
        match fs::read_to_string(path) {
            Ok(existing) => {
                let trimmed = existing.trim();
                if trimmed.is_empty() {
                    write_default_document(path)?;
                } else if serde_json::from_str::<serde_json::Value>(trimmed).is_err() {
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

    /// Inserts or replaces one named profile body given its JSON object.
    pub fn upsert_profile(
        &self,
        name: &str,
        profile_json: &str,
    ) -> Result<ProfileId, ProfileStoreError> {
        let id = ProfileId::new(name).map_err(ProfileStoreError::Invalid)?;
        let profile: serde_json::Value = serde_json::from_str(profile_json).map_err(|_| {
            ProfileStoreError::Invalid("the profile definition is not a valid JSON object")
        })?;
        if !profile.is_object() {
            return Err(ProfileStoreError::Invalid(
                "the profile definition must be a JSON object",
            ));
        }

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
            profiles.insert(id.as_str().to_owned(), profile);
            Ok(())
        })?;
        Ok(id)
    }

    /// Removes one profile and clears an active reference to it.
    pub fn delete_profile(&self, name: &str) -> Result<(), ProfileStoreError> {
        let id = ProfileId::new(name).map_err(|_| ProfileStoreError::UnknownProfile)?;
        self.mutate_document(|document| {
            let object = document.as_object_mut().expect("documents are objects");
            let profiles = object
                .get_mut("profiles")
                .and_then(|value| value.as_object_mut())
                .ok_or(ProfileStoreError::UnknownProfile)?;
            if profiles.remove(name).is_none() {
                return Err(ProfileStoreError::UnknownProfile);
            }
            if profiles.is_empty() {
                object.remove("profiles");
            }
            if object.get("active_profile").and_then(|v| v.as_str()) == Some(id.as_str()) {
                object.remove("active_profile");
            }
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

    /// Stores `secret` in the vault and records its reference on the profile.
    ///
    /// The secret never touches this store or any other durable file.
    pub fn link_credential<V: VaultPort + ?Sized>(
        &self,
        vault: &V,
        profile_name: &str,
        secret: &str,
    ) -> Result<CredentialReference, ProfileStoreError> {
        let id = ProfileId::new(profile_name).map_err(|_| ProfileStoreError::UnknownProfile)?;
        let declared = self
            .parsed()?
            .get("profiles")
            .and_then(|profiles| profiles.get(id.as_str()))
            .is_some();
        if !declared {
            return Err(ProfileStoreError::UnknownProfile);
        }
        let reference_name = format!("autoharness/profile/{profile_name}");
        let reference = vault
            .save(&reference_name, secret)
            .map_err(|_| ProfileStoreError::Io)?;

        self.mutate_document(|document| {
            let profile = document
                .get_mut("profiles")
                .and_then(|profiles| profiles.get_mut(id.as_str()))
                .and_then(|value| value.as_object_mut())
                .ok_or(ProfileStoreError::UnknownProfile)?;
            profile.insert(
                "credential".to_owned(),
                serde_json::json!({ "reference": reference.as_str() }),
            );
            Ok(())
        })?;
        Ok(reference)
    }

    /// Removes the stored reference from the profile document.
    ///
    /// The vault entry itself is deleted by the caller-owned flow so a
    /// partial failure remains recoverable.
    pub fn unlink_credential(
        &self,
        reference: &CredentialReference,
    ) -> Result<(), ProfileStoreError> {
        self.mutate_document(|document| {
            let mut found = false;
            if let Some(profiles) = document.get_mut("profiles").and_then(|v| v.as_object_mut()) {
                for (_, profile) in profiles.iter_mut() {
                    let Some(object) = profile.as_object_mut() else {
                        continue;
                    };
                    let matches = object
                        .get("credential")
                        .and_then(|c| c.get("reference"))
                        .and_then(|r| r.as_str())
                        .is_some_and(|value| value == reference.as_str());
                    if matches {
                        object.remove("credential");
                        found = true;
                    }
                }
            }
            if !found {
                return Err(ProfileStoreError::UnknownProfile);
            }
            Ok(())
        })
    }

    /// Removes the credential link and deletes the vault entry.
    pub fn disconnect_credential<V: VaultPort + ?Sized>(
        &self,
        vault: &V,
        reference: &CredentialReference,
    ) -> Result<(), ProfileStoreError> {
        self.unlink_credential(reference)?;
        vault.delete(reference).map_err(|_| ProfileStoreError::Io)
    }

    fn parsed(&self) -> Result<serde_json::Value, ProfileStoreError> {
        let text = self.read_document()?;
        serde_json::from_str(&text).map_err(|_| ProfileStoreError::Io)
    }

    fn mutate_document<F>(&self, mutate: F) -> Result<(), ProfileStoreError>
    where
        F: FnOnce(&mut serde_json::Value) -> Result<(), ProfileStoreError>,
    {
        let mut document: serde_json::Value =
            serde_json::from_str(&self.read_document()?).map_err(|_| ProfileStoreError::Io)?;
        mutate(&mut document)?;

        let object = document.as_object_mut().expect("documents are objects");
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
    let mut name = path.file_name().map_or_else(
        || "autoharness-profiles.json".to_owned(),
        |name| name.to_string_lossy().to_string(),
    );
    name.push_str(".tmp");
    path.with_file_name(name)
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(
        || "autoharness-profiles.json".to_owned(),
        |name| name.to_string_lossy().to_string(),
    );
    path.with_file_name(format!("{name}.bad"))
}
