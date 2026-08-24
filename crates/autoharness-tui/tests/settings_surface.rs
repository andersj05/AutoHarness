use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, Focus, Model, ModelSummary, ProviderStatusProjection, SessionProjection,
    SettingsProjection,
};

fn catalog_ready() -> CatalogProjection {
    CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: ModelRef::new(
                ProviderId::new("router:home").expect("provider id"),
                ModelId::new("models/test").expect("model id"),
            ),
            display_name: "Test model".to_owned(),
            detail: String::new(),
            selectable: true,
        }],
        stale: false,
    }
}

fn session() -> SessionProjection {
    let mut session = SessionProjection::empty();
    session.session_id = "session-1".to_owned();
    session
}

#[test]
fn default_projection_reports_defaults_and_session_only_source() {
    let projection = SettingsProjection::default();
    let mut model = Model::new(
        Arc::new(session()),
        Arc::new(autoharness_tui::SessionsProjection::default()),
        Arc::new(CatalogProjection::CredentialRequired),
    );
    model.apply_settings(Arc::new(projection));

    assert_eq!(model.settings().provider_label(), "gemini (default)");
    assert_eq!(model.settings().credential_label(), "session only");
}

#[test]
fn profile_projection_shows_active_profile_and_vault_source() {
    let projection = ProviderStatusProjection {
        active_profile: Some("home-router".to_owned()),
        provider_kind: Some(autoharness_tui::ProviderKindLabel::Router),
        credential_source: autoharness_tui::CredentialSourceLabel::CredentialVault,
        credential_connected: true,
    };
    let mut model = Model::new(
        Arc::new(session()),
        Arc::new(autoharness_tui::SessionsProjection::default()),
        Arc::new(catalog_ready()),
    );
    model.apply_settings(Arc::new(SettingsProjection {
        provider_status: projection,
        ..SettingsProjection::default()
    }));

    assert_eq!(
        model.settings().provider_label(),
        "router via 'home-router'"
    );
    assert_eq!(model.settings().credential_label(), "credential vault");
}

#[test]
fn disconnected_profile_reports_session_only_without_a_credential() {
    let projection = SettingsProjection {
        provider_status: ProviderStatusProjection {
            active_profile: Some("home-router".to_owned()),
            provider_kind: Some(autoharness_tui::ProviderKindLabel::Router),
            credential_source: autoharness_tui::CredentialSourceLabel::SessionOnly,
            credential_connected: false,
        },
        ..SettingsProjection::default()
    };
    let mut model = Model::new(
        Arc::new(session()),
        Arc::new(autoharness_tui::SessionsProjection::default()),
        Arc::new(catalog_ready()),
    );
    model.apply_settings(Arc::new(projection));

    assert_eq!(
        model.settings().credential_label(),
        "session only; press Ctrl+K to connect"
    );
}

#[test]
fn settings_route_opens_with_ctrl_comma_and_returns_to_chat() {
    let selected = ModelRef::new(
        ProviderId::new("router:home").expect("provider id"),
        ModelId::new("models/test").expect("model id"),
    );
    let mut session = SessionProjection::empty();
    session.session_id = "session-1".to_owned();
    session.selected_model = Some(selected);

    let mut model = Model::new(
        Arc::new(session),
        Arc::new(autoharness_tui::SessionsProjection::default()),
        Arc::new(catalog_ready()),
    );
    assert!(!model.settings_open());

    let _ = autoharness_tui::update(
        &mut model,
        autoharness_tui::Message::Input(ratatui_textarea::Input {
            key: ratatui_textarea::Key::Char(','),
            ctrl: true,
            alt: false,
            shift: false,
        }),
    );
    assert!(model.settings_open());
    assert_eq!(model.focus, Focus::Settings, "settings route owns input");

    let _ = autoharness_tui::update(
        &mut model,
        autoharness_tui::Message::Input(ratatui_textarea::Input {
            key: ratatui_textarea::Key::Char(','),
            ctrl: true,
            alt: false,
            shift: false,
        }),
    );
    assert!(!model.settings_open());
}
