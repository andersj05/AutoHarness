use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    CatalogProjection, Focus, Message, Model, ModelSummary, ProviderStatusProjection, Route,
    SessionProjection, SettingsProjection, style_snapshot, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn catalog_ready() -> CatalogProjection {
    CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: ModelRef::new(
                ProviderId::new("router:home").expect("provider id"),
                ModelId::new("models/test").expect("model id"),
            ),
            display_name: "Test model".to_owned(),
            detail: String::new(),
            context_window_tokens: Some(32_000),
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

fn settings_model() -> Model {
    Model::new(
        Arc::new(session()),
        Arc::new(autoharness_tui::SessionsProjection::default()),
        Arc::new(catalog_ready()),
    )
}

fn key(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn ctrl(character: char) -> Input {
    Input {
        key: Key::Char(character),
        ctrl: true,
        alt: false,
        shift: false,
    }
}

fn render(model: &Model, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(frame, model)).expect("draw");
    terminal.backend().buffer().clone()
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let mut text = String::new();
    for row in buffer.area.y..buffer.area.bottom() {
        for column in buffer.area.x..buffer.area.right() {
            text.push_str(buffer[(column, row)].symbol());
        }
        text.push('\n');
    }
    text
}

fn type_query(model: &mut Model, query: &str) {
    for character in query.chars() {
        let _ = update(model, Message::Input(key(Key::Char(character))));
    }
}

fn open_settings(model: &mut Model) {
    let _ = update(model, Message::Input(ctrl('4')));
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

#[test]
fn settings_categories_share_clamped_navigation_and_escape_levels() {
    let categories = [
        "Appearance",
        "Chat & Composer",
        "Accessibility",
        "Providers",
        "Models & Thinking",
        "Profile",
        "Sessions & Data",
        "Shortcuts",
        "About",
    ];
    for (index, category) in categories.into_iter().enumerate() {
        let mut model = settings_model();
        open_settings(&mut model);
        for _ in 0..index {
            let _ = update(&mut model, Message::Input(key(Key::Down)));
        }
        assert!(buffer_text(&render(&model, 80, 24)).contains(category));
        let _ = update(&mut model, Message::Input(key(Key::Tab)));
        let _ = update(&mut model, Message::Input(key(Key::Up)));
        let _ = update(&mut model, Message::Input(key(Key::Down)));
        let _ = update(&mut model, Message::Input(key(Key::Left)));
        assert_eq!(model.route(), Route::Settings, "{category}");
        let _ = update(&mut model, Message::Input(key(Key::Esc)));
        assert_eq!(model.route(), Route::Settings, "{category}");
        let _ = update(&mut model, Message::Input(key(Key::Esc)));
        assert_eq!(model.route(), Route::Chat, "{category}");
    }
}

#[test]
fn info_rows_are_visible_but_skipped_by_selection() {
    let mut model = settings_model();
    open_settings(&mut model);
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    let rendered = buffer_text(&render(&model, 80, 24));
    assert!(rendered.contains("Keyboard navigation"));
    assert!(rendered.contains("State indicators"));
    assert!(rendered.contains("Color mode  color"));
    assert!(
        !rendered
            .lines()
            .rev()
            .take(2)
            .any(|line| line.contains("Keyboard navigation  "))
    );
}

#[test]
fn settings_search_finds_preference_labels_and_current_values() {
    for (query, expected_category, expected_row) in [
        ("Submit prompt", "Chat & Composer", "Submit prompt"),
        ("responsive", "Chat & Composer", "Layout"),
        ("Ctrl+S", "Chat & Composer", "Submit prompt"),
        ("redacted", "Sessions & Data", "Logging"),
    ] {
        let mut model = settings_model();
        open_settings(&mut model);
        let _ = update(&mut model, Message::Input(ctrl('f')));
        type_query(&mut model, query);
        let rendered = buffer_text(&render(&model, 80, 24));
        assert!(rendered.contains(expected_category), "query {query}");
        assert!(rendered.contains(expected_row), "query {query}");
    }
}

#[test]
fn settings_reset_footer_names_results_and_hides_inert_inherit() {
    let mut inherited = settings_model();
    open_settings(&mut inherited);
    let _ = update(&mut inherited, Message::Input(key(Key::Tab)));
    let inherited_footer = buffer_text(&render(&inherited, 120, 40));
    assert!(!inherited_footer.contains("Backspace inherit ->"));
    assert!(inherited_footer.contains("Shift+Backspace default -> system"));

    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 4,
                "local_profile": { "preferences": { "theme_preset": "dark" } }
            }"#,
        )
        .resolve()
        .expect("user theme");
    let mut overridden = settings_model();
    overridden.apply_settings(Arc::new(SettingsProjection {
        local_profile: resolved.local_profile().clone(),
        ..SettingsProjection::default()
    }));
    open_settings(&mut overridden);
    let _ = update(&mut overridden, Message::Input(key(Key::Tab)));
    let override_footer = buffer_text(&render(&overridden, 120, 40));
    assert!(override_footer.contains("Backspace inherit -> system"));
    assert!(override_footer.contains("Shift+Backspace default -> system"));
}

#[test]
fn profile_category_is_populated_and_source_prefixes_are_retired() {
    let mut model = settings_model();
    open_settings(&mut model);
    for _ in 0..5 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    let rendered = buffer_text(&render(&model, 120, 40));
    for expected in [
        "Profile",
        "Active profile",
        "Workspace",
        "Display label",
        "Provider profiles",
    ] {
        assert!(rendered.contains(expected), "missing {expected}");
    }
    assert!(!rendered.contains("Source:"));
}

#[test]
fn ascii_settings_pages_contain_only_ascii_chrome() {
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 4,
                "local_profile": { "preferences": { "glyph_mode": "ascii" } }
            }"#,
        )
        .resolve()
        .expect("ASCII settings");
    let mut model = settings_model();
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: resolved.local_profile().clone(),
        ..SettingsProjection::default()
    }));
    open_settings(&mut model);
    for index in 0..9 {
        let rendered = buffer_text(&render(&model, 80, 24));
        assert!(
            rendered.is_ascii(),
            "category {index} contained non-ASCII chrome"
        );
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
}

#[test]
fn theme_picker_and_glyph_check_render_across_all_presets_and_modes() {
    for preset in [
        "system", "light", "dark", "aurora", "ember", "midnight", "ocean", "forest", "rose",
    ] {
        for glyph in ["unicode", "nerd_font", "ascii"] {
            let document = format!(
                r#"{{
                    "schema_version": 4,
                    "local_profile": {{ "preferences": {{
                        "theme_preset": "{preset}",
                        "glyph_mode": "{glyph}"
                    }} }}
                }}"#
            );
            let resolved = SettingsBuilder::new()
                .with_layer(LayerKind::UserFile, &document)
                .resolve()
                .expect("preview preferences");
            let mut model = settings_model();
            model.apply_settings(Arc::new(SettingsProjection {
                local_profile: resolved.local_profile().clone(),
                ..SettingsProjection::default()
            }));
            open_settings(&mut model);
            let _ = update(&mut model, Message::Input(key(Key::Tab)));
            let _ = update(&mut model, Message::Input(key(Key::Enter)));
            let snapshot = style_snapshot(&render(&model, 120, 40));
            assert!(snapshot.contains(preset));
            assert!(snapshot.contains("Choose theme"));
        }
    }
}
