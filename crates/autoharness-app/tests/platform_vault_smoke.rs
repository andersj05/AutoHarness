use autoharness_app::vault::{KeyringVault, VaultError, VaultPort};
use autoharness_settings::CredentialReference;

struct Cleanup {
    vault: KeyringVault,
    reference: Option<CredentialReference>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(reference) = &self.reference {
            let _ = self.vault.delete(reference);
        }
    }
}

#[test]
#[ignore = "opt-in operating-system credential service smoke; run on each release platform"]
fn platform_vault_save_load_replace_and_delete_without_secret_output() {
    if std::env::var("AUTOHARNESS_RUN_PLATFORM_VAULT_SMOKE").as_deref() != Ok("1") {
        return;
    }
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let reference_name = format!("autoharness/platform-smoke/{unique}");
    let first = format!("AH-platform-smoke-first-{unique}");
    let second = format!("AH-platform-smoke-second-{unique}");
    let vault = KeyringVault::new();
    let reference = vault
        .save(&reference_name, &first)
        .expect("platform vault save");
    let mut cleanup = Cleanup {
        vault: KeyringVault::new(),
        reference: Some(reference.clone()),
    };

    assert_eq!(
        &*vault.load(&reference).expect("platform vault load"),
        &first
    );
    vault
        .replace(&reference, &second)
        .expect("platform vault replace");
    assert_eq!(
        &*vault.load(&reference).expect("platform vault reload"),
        &second
    );
    vault.delete(&reference).expect("platform vault delete");
    cleanup.reference = None;
    assert!(matches!(
        vault.load(&reference),
        Err(VaultError::MissingEntry)
    ));
}
