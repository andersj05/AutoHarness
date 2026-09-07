use autoharness_settings::{LayerKind, ProfileId, SettingsBuilder, SettingsError};

#[test]
fn malformed_user_layer_is_skipped_and_defaults_remain() {
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, "{not json")
        .with_layer(LayerKind::WorkspaceFile, r#"{"schema_version": 1}"#)
        .resolve()
        .expect("malformed layer must degrade to defaults");

    assert_eq!(resolved.provider(), None);
    assert_eq!(
        resolved
            .provenance()
            .get("local_profile.preferences.shared.theme_preset"),
        Some(&autoharness_settings::Source::Default)
    );
    assert_eq!(resolved.diagnostics().len(), 1);
}

#[test]
fn future_schema_version_is_refused_with_a_clear_error() {
    let error = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{"schema_version": 99, "provider": "gemini"}"#,
        )
        .resolve()
        .expect_err("future schema versions must fail closed");

    assert!(matches!(
        error,
        SettingsError::UnsupportedSchemaVersion { found: 99, .. }
    ));
}

#[test]
fn active_profile_must_reference_a_declared_profile() {
    let error = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{"schema_version": 1, "active_profile": "missing"}"#,
        )
        .resolve()
        .expect_err("unknown active profile must fail validation");

    assert!(matches!(error, SettingsError::InvalidMerge { .. }));
}

#[test]
fn legacy_profile_selection_retains_user_provenance() {
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, USER_JSON)
        .resolve()
        .expect("user layer resolves");

    // The user file selects a profile; provenance shows the source.
    let provenance = resolved.provenance();
    assert_eq!(
        provenance
            .get("active_profile")
            .expect("active profile is set"),
        &autoharness_settings::Source::UserFile
    );
}

#[test]
fn profile_id_rejects_empty_names() {
    assert!(ProfileId::new("").is_err());
    assert!(ProfileId::new("ok-name").is_ok());
}

const USER_JSON: &str = r#"{
    "schema_version": 1,
    "profiles": {
        "home-router": {"kind": "router", "base_url": "https://router.example.test/"}
    },
    "active_profile": "home-router"
}"#;
