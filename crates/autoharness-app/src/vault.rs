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

/// Maximum accepted opaque credential payload length.
const MAX_SECRET_BYTES: usize = 32 * 1_024;
const KEYRING_CHUNK_BYTES: usize = 1_000;
const KEYRING_MANIFEST_PREFIX: &str = "autoharness-vault-v1";

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
        self.entries
            .lock()
            .expect("fake vault mutex")
            .remove(reference.as_str());
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

    fn chunk_reference(reference: &str, generation: char, index: usize) -> String {
        format!("{reference}#v1#{generation}#{index}")
    }

    fn manifest(generation: char, count: usize) -> String {
        format!("{KEYRING_MANIFEST_PREFIX}:{generation}:{count}")
    }

    fn parse_manifest(value: &str) -> Result<Option<(char, usize)>, VaultError> {
        if !value.starts_with(KEYRING_MANIFEST_PREFIX) {
            return Ok(None);
        }
        let mut fields = value.split(':');
        if fields.next() != Some(KEYRING_MANIFEST_PREFIX) {
            return Err(VaultError::Unavailable);
        }
        let generation = fields
            .next()
            .and_then(|value| value.chars().next().filter(|_| value.len() == 1))
            .filter(|value| matches!(value, 'a' | 'b'))
            .ok_or(VaultError::Unavailable)?;
        let count = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count > 0 && *count <= MAX_SECRET_BYTES.div_ceil(KEYRING_CHUNK_BYTES))
            .ok_or(VaultError::Unavailable)?;
        if fields.next().is_some() {
            return Err(VaultError::Unavailable);
        }
        Ok(Some((generation, count)))
    }

    fn delete_generation(&self, reference: &str, generation: char, count: usize) {
        for index in 0..count {
            let chunk = Self::chunk_reference(reference, generation, index);
            if let Ok(entry) = self.entry(&chunk) {
                let _ = entry.delete_credential();
            }
        }
    }

    fn write_chunked(
        &self,
        reference: &str,
        secret: &str,
        generation: char,
    ) -> Result<usize, VaultError> {
        let chunks = secret
            .as_bytes()
            .chunks(KEYRING_CHUNK_BYTES)
            .collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            let chunk = std::str::from_utf8(chunk).map_err(|_| VaultError::Unavailable)?;
            let chunk_reference = Self::chunk_reference(reference, generation, index);
            if let Err(error) = self
                .entry(&chunk_reference)?
                .set_password(chunk)
                .map_err(map_keyring_error)
            {
                self.delete_generation(reference, generation, index);
                return Err(error);
            }
        }
        let count = chunks.len();
        if let Err(error) = self
            .entry(reference)?
            .set_password(&Self::manifest(generation, count))
            .map_err(map_keyring_error)
        {
            self.delete_generation(reference, generation, count);
            return Err(error);
        }
        Ok(count)
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
        let existing = self
            .entry(stored.as_str())?
            .get_password()
            .ok()
            .and_then(|value| Self::parse_manifest(&value).ok().flatten());
        let generation = existing.map_or(
            'a',
            |(generation, _)| if generation == 'a' { 'b' } else { 'a' },
        );
        self.write_chunked(stored.as_str(), secret, generation)?;
        if let Some((old_generation, old_count)) = existing {
            self.delete_generation(stored.as_str(), old_generation, old_count);
        }
        Ok(stored)
    }

    fn load(&self, reference: &CredentialReference) -> Result<Zeroizing<String>, VaultError> {
        let entry = self.entry(reference.as_str())?;
        let secret = entry.get_password().map_err(map_keyring_error)?;
        let Some((generation, count)) = Self::parse_manifest(&secret)? else {
            return Ok(Zeroizing::new(secret));
        };
        let mut assembled = Zeroizing::new(String::new());
        for index in 0..count {
            let chunk_reference = Self::chunk_reference(reference.as_str(), generation, index);
            let chunk = self
                .entry(&chunk_reference)?
                .get_password()
                .map_err(map_keyring_error)?;
            assembled.push_str(&chunk);
        }
        validate_secret(&assembled)?;
        Ok(assembled)
    }

    fn replace(&self, reference: &CredentialReference, secret: &str) -> Result<(), VaultError> {
        validate_secret(secret)?;
        let current = self
            .entry(reference.as_str())?
            .get_password()
            .map_err(map_keyring_error)?;
        let existing = Self::parse_manifest(&current)?;
        let generation = existing.map_or(
            'a',
            |(generation, _)| if generation == 'a' { 'b' } else { 'a' },
        );
        self.write_chunked(reference.as_str(), secret, generation)?;
        if let Some((old_generation, old_count)) = existing {
            self.delete_generation(reference.as_str(), old_generation, old_count);
        }
        Ok(())
    }

    fn delete(&self, reference: &CredentialReference) -> Result<(), VaultError> {
        let entry = self.entry(reference.as_str())?;
        let existing = entry
            .get_password()
            .ok()
            .and_then(|value| Self::parse_manifest(&value).ok().flatten());
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(error)),
        }?;
        if let Some((generation, count)) = existing {
            self.delete_generation(reference.as_str(), generation, count);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_manifests_are_strict_and_bounded() {
        assert_eq!(
            KeyringVault::parse_manifest("autoharness-vault-v1:a:3"),
            Ok(Some(('a', 3)))
        );
        assert_eq!(KeyringVault::parse_manifest("ordinary-secret"), Ok(None));
        assert!(KeyringVault::parse_manifest("autoharness-vault-v1:c:3").is_err());
        assert!(KeyringVault::parse_manifest("autoharness-vault-v1:a:999").is_err());
    }

    #[test]
    fn chunk_references_are_generation_scoped() {
        assert_eq!(
            KeyringVault::chunk_reference("autoharness/profile/codex", 'b', 7),
            "autoharness/profile/codex#v1#b#7"
        );
    }
}
