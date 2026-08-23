//! Credential-vault port over operating-system credential storage.
//!
//! Raw credential material lives only inside the platform facility
//! (Windows Credential Manager, macOS Keychain, Linux Secret Service)
//! or, for tests, the in-process fake vault. References held by
//! provider profiles name vault entries; they never contain secrets.

use std::collections::BTreeMap;
use std::fmt;

use autoharness_settings::CredentialReference;
use zeroize::Zeroizing;

/// Errors surfaced by the credential-vault port without secret detail.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum VaultError {
    /// No entry exists under the requested reference.
    MissingEntry,
    /// The platform credential service is locked or unavailable.
    Unavailable,
    /// The secret is empty, oversized, or not usable for storage.
    InvalidSecret(&'static str),
    /// The platform vault rejected the operation.
    Platform(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEntry => formatter.write_str("no stored credential exists"),
            Self::Unavailable => formatter.write_str("credential vault is unavailable"),
            Self::InvalidSecret(reason) => write!(formatter, "{reason}"),
            Self::Platform(detail) => write!(formatter, "credential vault error: {detail}"),
        }
    }
}

impl std::error::Error for VaultError {}

/// Maximum accepted credential length, matching the TUI entry bound.
const MAX_SECRET_BYTES: usize = 4_096;

/// Application-owned port to the operating-system credential store.
pub trait VaultPort: Send + Sync {
    /// Stores a new secret and returns its stable reference.
    fn save(&self, reference: &str, secret: &str) -> Result<CredentialReference, VaultError>;

    /// Loads the secret for an existing reference.
    fn load(&self, reference: &CredentialReference) -> Result<Zeroizing<String>, VaultError>;

    /// Replaces the secret behind an existing reference.
    fn replace(&self, reference: &CredentialReference, secret: &str) -> Result<(), VaultError>;

    /// Removes a stored credential.
    fn delete(&self, reference: &CredentialReference) -> Result<(), VaultError>;
}

fn validate_secret(secret: &str) -> Result<&str, VaultError> {
    if secret.is_empty() {
        return Err(VaultError::InvalidSecret("credentials must not be empty"));
    }
    if secret.len() > MAX_SECRET_BYTES {
        return Err(VaultError::InvalidSecret("credential is too long"));
    }
    if !secret.chars().all(|c| c.is_ascii_graphic()) {
        return Err(VaultError::InvalidSecret(
            "credentials must contain visible ASCII characters only",
        ));
    }
    Ok(secret)
}

fn map_keyring_error(error: keyring::Error) -> VaultError {
    match error {
        keyring::Error::NoEntry => VaultError::MissingEntry,
        keyring::Error::NoStorageAccess(_) => VaultError::Unavailable,
        other => VaultError::Platform(other.to_string()),
    }
}

/// In-process fake vault used by tests and offline composition.
#[derive(Default)]
pub struct FakeVault {
    entries: std::sync::Mutex<BTreeMap<String, String>>,
}

impl FakeVault {
    /// Creates an empty fake vault.
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for FakeVault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self
            .entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0);
        formatter
            .debug_struct("FakeVault")
            .field("entries", &count)
            .finish()
    }
}

impl VaultPort for FakeVault {
    fn save(&self, reference: &str, secret: &str) -> Result<CredentialReference, VaultError> {
        validate_secret(secret)?;
        let stored = CredentialReference::new(reference).map_err(VaultError::InvalidSecret)?;
        self.entries
            .lock()
            .expect("fake vault mutex")
            .insert(stored.as_str().to_owned(), secret.to_owned());
        Ok(stored)
    }

    fn load(&self, reference: &CredentialReference) -> Result<Zeroizing<String>, VaultError> {
        match self
            .entries
            .lock()
            .expect("fake vault mutex")
            .get(reference.as_str())
        {
            Some(secret) => Ok(Zeroizing::new(secret.clone())),
            None => Err(VaultError::MissingEntry),
        }
    }

    fn replace(&self, reference: &CredentialReference, secret: &str) -> Result<(), VaultError> {
        validate_secret(secret)?;
        let mut entries = self.entries.lock().expect("fake vault mutex");
        if !entries.contains_key(reference.as_str()) {
            return Err(VaultError::MissingEntry);
        }
        entries.insert(reference.as_str().to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, reference: &CredentialReference) -> Result<(), VaultError> {
        let mut entries = self.entries.lock().expect("fake vault mutex");
        if entries.remove(reference.as_str()).is_none() {
            return Err(VaultError::MissingEntry);
        }
        Ok(())
    }
}

/// Operating-system backed vault implemented with the `keyring` crate.
///
/// On Windows this uses Credential Manager; on macOS the Keychain; and on
/// Linux the Secret Service where available. The service name isolates
/// AutoHarness entries from other applications.
pub struct KeyringVault {
    service: &'static str,
}

impl KeyringVault {
    /// Creates a vault bound to the AutoHarness service namespace.
    pub fn new() -> Self {
        Self {
            service: "AutoHarness",
        }
    }

    fn entry(&self, reference: &str) -> Result<keyring::Entry, VaultError> {
        keyring::Entry::new(self.service, reference).map_err(map_keyring_error)
    }
}

impl Default for KeyringVault {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for KeyringVault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyringVault")
            .finish_non_exhaustive()
    }
}

impl VaultPort for KeyringVault {
    fn save(&self, reference: &str, secret: &str) -> Result<CredentialReference, VaultError> {
        validate_secret(secret)?;
        let stored = CredentialReference::new(reference).map_err(VaultError::InvalidSecret)?;
        let entry = self.entry(stored.as_str())?;
        entry.set_password(secret).map_err(map_keyring_error)?;
        Ok(stored)
    }

    fn load(&self, reference: &CredentialReference) -> Result<Zeroizing<String>, VaultError> {
        let entry = self.entry(reference.as_str())?;
        let secret = entry.get_password().map_err(map_keyring_error)?;
        Ok(Zeroizing::new(secret))
    }

    fn replace(&self, reference: &CredentialReference, secret: &str) -> Result<(), VaultError> {
        validate_secret(secret)?;
        let entry = self.entry(reference.as_str())?;
        // Distinguish a missing target from a platform failure.
        match entry.get_password() {
            Ok(_) => {}
            Err(error) => return Err(map_keyring_error(error)),
        }
        entry.set_password(secret).map_err(map_keyring_error)
    }

    fn delete(&self, reference: &CredentialReference) -> Result<(), VaultError> {
        let entry = self.entry(reference.as_str())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(error)),
        }
    }
}
