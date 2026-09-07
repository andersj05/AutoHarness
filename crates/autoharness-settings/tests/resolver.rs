use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, GuiFontSize, GuiZoomPercent, LayerKind,
    Layout, ProfileId, PromptStatusDetail, ProviderKind, SETTINGS_SCHEMA_VERSION, SettingsBuilder,
    SettingsDocument, SettingsError, Source, TerminalTimestampStyle, ThemePreset,
};

const USER_JSON: &str = r#"{
    "schema_version": 1,
    "provider": "router",
    "profiles": {
        "home-router": {
            "kind": "router",
            "base_url": "https://router.example.test/base/",
            "project": "home",
            "credential": {"reference": "autoharness/profile/home-router"}
        }
    },
    "active_profile": "home-router"
}"#;

fn user_layer(json: &str) -> SettingsBuilder {
    SettingsBuilder::new().with_layer(LayerKind::UserFile, json)
}

#[test]
fn defaults_resolve_without_any_file() {
    let resolved = SettingsBuilder::new()
        .resolve()
        .expect("defaults always resolve");

    assert_eq!(resolved.provider(), None);
    assert!(resolved.profiles().next().is_none());
    assert_eq!(resolved.active_profile(), None);
    assert_eq!(
        resolved
            .provenance()
            .get("local_profile.preferences.shared.theme_preset"),
        Some(&Source::Default)
    );
}

#[test]
fn v2_document_migrates_to_current_schema_with_default_local_preferences() {
    let document: SettingsDocument = serde_json::from_str(
        r#"{
            "schema_version": 2,
            "provider": "gemini"
        }"#,
    )
    .expect("v2 document remains readable");

    assert_eq!(document.schema_version(), SETTINGS_SCHEMA_VERSION);
    assert!(document.local_profile().is_empty());
    let serialized = serde_json::to_value(document).expect("current document serializes");
    assert_eq!(serialized["schema_version"], SETTINGS_SCHEMA_VERSION);

    let resolved = user_layer(r#"{"schema_version": 2}"#)
        .resolve()
        .expect("v2 layer resolves");
    let preferences = resolved.local_profile().preferences();
    assert_eq!(preferences.theme_preset().value(), &ThemePreset::System);
    assert_eq!(preferences.color_mode().value(), &ColorMode::Color);
    assert_eq!(preferences.glyph_mode().value(), &GlyphMode::Unicode);
    assert_eq!(preferences.density().value(), &Density::Comfortable);
    assert_eq!(preferences.layout().value(), &Layout::Responsive);
    assert_eq!(
        preferences.terminal_timestamp_style().value(),
        &TerminalTimestampStyle::Relative
    );
    assert_eq!(preferences.theme_preset().source(), Source::Default);
}

#[test]
fn v4_flat_preferences_migrate_into_separated_schema_five_groups() {
    let document: SettingsDocument = serde_json::from_str(
        r#"{
            "schema_version": 4,
            "local_profile": {
                "display_label": "Ada",
                "preferences": {
                    "theme_preset": "rose",
                    "color_mode": "vivid",
                    "glyph_mode": "ascii",
                    "reduced_motion": true,
                    "density": "compact",
                    "layout": "single_column",
                    "terminal_timestamp_style": "absolute",
                    "composer_submit_behavior": "enter",
                    "prompt_status_detail": "detailed"
                }
            }
        }"#,
    )
    .expect("v4 document remains readable");

    let mut preferences = document.local_profile().preferences().clone();
    preferences.set_gui_zoom_percent(Some(GuiZoomPercent::new(150).expect("valid zoom")));
    preferences.set_gui_font_size(Some(GuiFontSize::Large));
    let mut local_profile = document.local_profile().clone();
    local_profile.set_preferences(preferences);
    let migrated = serde_json::to_value(SettingsDocument::new().with_local_profile(local_profile))
        .expect("schema five serializes");

    assert_eq!(migrated["schema_version"], 5);
    assert_eq!(
        migrated["local_profile"]["preferences"]["shared"]["theme_preset"],
        "rose"
    );
    assert_eq!(
        migrated["local_profile"]["preferences"]["shared"]["timestamp_style"],
        "absolute"
    );
    assert_eq!(
        migrated["local_profile"]["preferences"]["gui"]["zoom_percent"],
        150
    );
    assert_eq!(
        migrated["local_profile"]["preferences"]["terminal"]["glyph_mode"],
        "ascii"
    );
    assert!(
        migrated["local_profile"]["preferences"]
            .get("terminal_timestamp_style")
            .is_none()
    );
}

#[test]
fn schema_five_resolves_shared_gui_and_terminal_preferences() {
    let resolved = user_layer(
        r#"{
            "schema_version": 5,
            "local_profile": {
                "preferences": {
                    "shared": {
                        "theme_preset": "light",
                        "color_mode": "high_contrast",
                        "reduced_motion": true,
                        "density": "compact",
                        "timestamp_style": "hidden",
                        "composer_submit_behavior": "enter"
                    },
                    "gui": {
                        "zoom_percent": 175,
                        "font_size": "extra_large"
                    },
                    "terminal": {
                        "glyph_mode": "nerd_font",
                        "layout": "single_column",
                        "prompt_status_detail": "essential"
                    }
                }
            }
        }"#,
    )
    .resolve()
    .expect("schema five preferences resolve");
    let preferences = resolved.local_profile().preferences();

    assert_eq!(preferences.theme_preset().value(), &ThemePreset::Light);
    assert_eq!(preferences.color_mode().value(), &ColorMode::HighContrast);
    assert!(*preferences.reduced_motion().value());
    assert_eq!(preferences.density().value(), &Density::Compact);
    assert_eq!(
        preferences.terminal_timestamp_style().value(),
        &TerminalTimestampStyle::Hidden
    );
    assert_eq!(
        preferences.composer_submit_behavior().value(),
        &ComposerSubmitBehavior::Enter
    );
    assert_eq!(preferences.gui_zoom_percent().value().get(), 175);
    assert_eq!(
        preferences.gui_font_size().value(),
        &GuiFontSize::ExtraLarge
    );
    assert_eq!(preferences.glyph_mode().value(), &GlyphMode::NerdFont);
    assert_eq!(preferences.layout().value(), &Layout::SingleColumn);
    assert_eq!(
        preferences.prompt_status_detail().value(),
        &PromptStatusDetail::Essential
    );
}

#[test]
fn user_layer_supplies_provider_and_profiles() {
    let resolved = user_layer(USER_JSON).resolve().expect("valid user layer");

    assert_eq!(resolved.provider(), Some(ProviderKind::Router));
    let profile = resolved
        .profile(&ProfileId::new("home-router").expect("valid profile id"))
        .expect("declared profile");
    assert_eq!(
        profile.credential_reference(),
        Some("autoharness/profile/home-router")
    );
}

#[test]
fn environment_overrides_the_user_file() {
    let resolved = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, USER_JSON)
        .with_environment([("AUTOHARNESS_PROVIDER", "gemini")])
        .resolve()
        .expect("valid layers");

    assert_eq!(resolved.provider(), Some(ProviderKind::Gemini));
    let provenance = resolved.provenance();
    assert_eq!(
        provenance.get("provider").expect("provider is set"),
        &Source::Environment
    );
}

#[test]
fn user_preferences_override_defaults_with_leaf_provenance() {
    let resolved = user_layer(
        r#"{
            "schema_version": 3,
            "local_profile": {
                "display_label": "Ada",
                "preferences": {
                    "theme_preset": "dark",
                    "color_mode": "no_color",
                    "glyph_mode": "ascii",
                    "prompt_status_detail": "detailed",
                    "reduced_motion": true,
                    "density": "compact",
                    "layout": "single_column",
                    "terminal_timestamp_style": "absolute",
                    "composer_submit_behavior": "enter"
                }
            }
        }"#,
    )
    .resolve()
    .expect("valid user preferences");

    let local_profile = resolved.local_profile();
    assert_eq!(
        local_profile
            .display_label()
            .value()
            .as_ref()
            .map(|label| label.as_str()),
        Some("Ada")
    );
    assert_eq!(local_profile.display_label().source(), Source::UserFile);
    let preferences = local_profile.preferences();
    assert_eq!(preferences.theme_preset().value(), &ThemePreset::Dark);
    assert_eq!(preferences.color_mode().value(), &ColorMode::NoColor);
    assert_eq!(preferences.glyph_mode().value(), &GlyphMode::Ascii);
    assert_eq!(
        preferences.prompt_status_detail().value(),
        &PromptStatusDetail::Detailed
    );
    assert_eq!(
        preferences.prompt_status_detail().source(),
        Source::UserFile
    );
    assert!(*preferences.reduced_motion().value());
    assert_eq!(preferences.density().value(), &Density::Compact);
    assert_eq!(preferences.layout().value(), &Layout::SingleColumn);
    assert_eq!(
        preferences.terminal_timestamp_style().value(),
        &TerminalTimestampStyle::Absolute
    );
    assert_eq!(preferences.theme_preset().source(), Source::UserFile);
    assert_eq!(
        resolved
            .provenance()
            .get("local_profile.preferences.shared.color_mode"),
        Some(&Source::UserFile)
    );
}

#[test]
fn workspace_precedence_does_not_depend_on_builder_insertion_order() {
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::WorkspaceFile,
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "glyph_mode": "ascii"
                    }
                }
            }"#,
        )
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "glyph_mode": "unicode"
                    }
                }
            }"#,
        )
        .resolve()
        .expect("fixed precedence resolves");

    let glyph_mode = resolved.local_profile().preferences().glyph_mode();
    assert_eq!(glyph_mode.value(), &GlyphMode::Ascii);
    assert_eq!(glyph_mode.source(), Source::WorkspaceFile);
}

#[test]
fn permitted_workspace_preferences_override_user_preferences() {
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "theme_preset": "light",
                        "density": "compact",
                        "color_mode": "color"
                    }
                }
            }"#,
        )
        .with_layer(
            LayerKind::WorkspaceFile,
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "theme_preset": "dark",
                        "color_mode": "high_contrast",
                        "terminal_timestamp_style": "absolute"
                    }
                }
            }"#,
        )
        .resolve()
        .expect("permitted workspace preferences resolve");

    let preferences = resolved.local_profile().preferences();
    assert_eq!(preferences.theme_preset().value(), &ThemePreset::Dark);
    assert_eq!(preferences.theme_preset().source(), Source::WorkspaceFile);
    assert_eq!(preferences.color_mode().value(), &ColorMode::HighContrast);
    assert_eq!(preferences.color_mode().source(), Source::WorkspaceFile);
    assert_eq!(preferences.density().value(), &Density::Compact);
    assert_eq!(preferences.density().source(), Source::UserFile);
    assert_eq!(
        preferences.terminal_timestamp_style().source(),
        Source::WorkspaceFile
    );
}

#[test]
fn schema_five_workspace_preferences_preserve_protected_interaction_settings() {
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 5,
                "local_profile": {
                    "preferences": {
                        "shared": {"composer_submit_behavior": "enter"},
                        "gui": {"zoom_percent": 100}
                    }
                }
            }"#,
        )
        .with_layer(
            LayerKind::WorkspaceFile,
            r#"{
                "schema_version": 5,
                "local_profile": {
                    "preferences": {
                        "shared": {"theme_preset": "dark"},
                        "gui": {"zoom_percent": 150, "font_size": "large"},
                        "terminal": {"glyph_mode": "ascii"}
                    }
                }
            }"#,
        )
        .resolve()
        .expect("permitted schema five workspace values resolve");
    let preferences = resolved.local_profile().preferences();

    assert_eq!(preferences.theme_preset().source(), Source::WorkspaceFile);
    assert_eq!(preferences.gui_zoom_percent().value().get(), 150);
    assert_eq!(
        preferences.gui_zoom_percent().source(),
        Source::WorkspaceFile
    );
    assert_eq!(preferences.gui_font_size().value(), &GuiFontSize::Large);
    assert_eq!(preferences.glyph_mode().source(), Source::WorkspaceFile);
    assert_eq!(
        preferences.composer_submit_behavior().source(),
        Source::UserFile
    );

    let error = SettingsBuilder::new()
        .with_layer(
            LayerKind::WorkspaceFile,
            r#"{
                "schema_version": 5,
                "local_profile": {
                    "preferences": {
                        "shared": {"composer_submit_behavior": "enter"}
                    }
                }
            }"#,
        )
        .resolve()
        .expect_err("submission behavior remains a user-only setting");
    assert!(matches!(
        error,
        SettingsError::DisallowedWorkspaceKey { key }
            if key == "local_profile.preferences.shared.composer_submit_behavior"
    ));
}

#[test]
fn workspace_rejects_every_protected_settings_group() {
    for (json, key) in [
        (r#"{"schema_version": 3, "provider": "gemini"}"#, "provider"),
        (r#"{"schema_version": 3, "profiles": {}}"#, "profiles"),
        (
            r#"{"schema_version": 3, "active_profile": "personal"}"#,
            "active_profile",
        ),
        (
            r#"{"schema_version": 3, "credential_recovery": []}"#,
            "credential_recovery",
        ),
        (
            r#"{"schema_version": 3, "approvals": {"mode": "allow"}}"#,
            "approvals",
        ),
        (
            r#"{"schema_version": 3, "retention": "forever"}"#,
            "retention",
        ),
        (r#"{"schema_version": 3, "telemetry": true}"#, "telemetry"),
        (r#"{"schema_version": 3, "sandbox": "off"}"#, "sandbox"),
        (
            r#"{"schema_version": 3, "credential": "secret"}"#,
            "credential",
        ),
        (
            r#"{
                "schema_version": 3,
                "local_profile": {"display_label": "workspace"}
            }"#,
            "local_profile.display_label",
        ),
        (
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {"composer_submit_behavior": "enter"}
                }
            }"#,
            "local_profile.preferences.composer_submit_behavior",
        ),
    ] {
        let error = SettingsBuilder::new()
            .with_layer(LayerKind::WorkspaceFile, json)
            .resolve()
            .expect_err("workspace protected settings must be rejected");
        assert!(matches!(
            error,
            SettingsError::DisallowedWorkspaceKey { key: found } if found == key
        ));
    }
}
