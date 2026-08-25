use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    CatalogProjection, Focus, Message, Model, ModelSummary, MouseAction, OverlayKind,
    PermissionDetailView, PermissionRequestView, RetryPolicy, Route, SessionBrowserEntry,
    SessionProjection, SessionsProjection, SettingsProjection, ToolCallKey, UiFailure, UiIntent,
    hit_test, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn model_ref() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider id"),
        ModelId::new("models/gemini-shell").expect("model id"),
    )
}

fn model() -> Model {
    let session = Arc::new(SessionProjection {
        session_id: "session-active".to_owned(),
        revision: 1,
        selected_model: Some(model_ref()),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    });
    let sessions = Arc::new(SessionsProjection {
        sessions: vec![
            SessionBrowserEntry {
                session_id: "session-active".to_owned(),
                title: "Active conversation".to_owned(),
                archived: false,
                selected_model: Some(model_ref()),
                updated_at_ms: 2,
                active: true,
            },
            SessionBrowserEntry {
                session_id: "session-other".to_owned(),
                title: "Other conversation".to_owned(),
                archived: false,
                selected_model: None,
                updated_at_ms: 1,
                active: false,
            },
        ],
    });
    let catalog = Arc::new(CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: model_ref(),
            display_name: "Gemini Shell".to_owned(),
            detail: String::new(),
            selectable: true,
        }],
        stale: false,
    });
    Model::new(session, sessions, catalog)
}

fn loading_model() -> Model {
    Model::new(
        Arc::new(SessionProjection {
            session_id: "startup-session".to_owned(),
            revision: 1,
            selected_model: None,
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::Loading),
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
        ctrl: true,
        ..key(Key::Char(character))
    }
}

fn type_text(model: &mut Model, text: &str) {
    for character in text.chars() {
        let _ = update(model, Message::Input(key(Key::Char(character))));
    }
}

fn render_text(model: &Model, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(frame, model)).expect("draw");
    let backend = terminal.backend();
    let area = backend.buffer().area;
    let mut rendered = String::new();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            rendered.push_str(backend.buffer()[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    rendered
}

#[test]
fn ctrl_number_routes_cover_every_primary_destination() {
    let mut model = model();
    for (key, route, focus) in [
        ('2', Route::Sessions, Focus::Browser),
        ('3', Route::Profiles, Focus::Profiles),
        ('4', Route::Settings, Focus::Settings),
        ('5', Route::Help, Focus::Help),
        ('1', Route::Chat, Focus::Composer),
    ] {
        let _ = update(&mut model, Message::Input(ctrl(key)));
        assert_eq!(model.route(), route);
        assert_eq!(model.focus, focus);
        assert_eq!(model.overlay(), None);
    }
}

#[test]
fn global_model_and_credential_overlays_restore_non_chat_routes() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Input(ctrl('p')));
    assert_eq!(model.overlay(), Some(OverlayKind::ModelPicker));
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    assert_eq!(model.route(), Route::Settings);
    assert_eq!(model.focus, Focus::Settings);

    let _ = update(&mut model, Message::Input(ctrl('3')));
    let _ = update(&mut model, Message::Input(ctrl('k')));
    assert_eq!(model.overlay(), Some(OverlayKind::SessionCredential));
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    assert_eq!(model.route(), Route::Settings);
    assert_eq!(model.focus, Focus::Settings);
}

#[test]
fn permission_preempts_the_modal_slot_and_restores_the_base_route() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('2')));
    let _ = update(&mut model, Message::Input(ctrl('/')));
    assert_eq!(model.overlay(), Some(OverlayKind::CommandPalette));

    let mut permission = (*model.session).clone();
    permission.revision = 2;
    permission.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("route-permission").expect("tool call id"),
        tool_name: "fs_read".to_owned(),
        capability: "filesystem read".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: vec![PermissionDetailView {
            label: "Path".to_owned(),
            value: "src/lib.rs".to_owned(),
        }],
    });
    let _ = update(
        &mut model,
        Message::SessionChanged(Arc::new(permission.clone())),
    );
    assert_eq!(model.route(), Route::Sessions);
    assert_eq!(model.overlay(), Some(OverlayKind::Permission));
    assert_eq!(model.focus, Focus::Permission);

    permission.revision = 3;
    permission.permission_requests.clear();
    let _ = update(&mut model, Message::SessionChanged(Arc::new(permission)));
    assert_eq!(model.route(), Route::Sessions);
    assert_eq!(model.overlay(), None);
    assert_eq!(model.focus, Focus::Browser);
}
#[test]
fn overlay_escape_restores_the_exact_route_and_focus() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('2')));
    let _ = update(&mut model, Message::Input(ctrl('/')));

    assert_eq!(model.route(), Route::Sessions);
    assert_eq!(model.overlay(), Some(OverlayKind::CommandPalette));
    assert_eq!(model.focus, Focus::Palette);

    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    assert_eq!(model.route(), Route::Sessions);
    assert_eq!(model.overlay(), None);
    assert_eq!(model.focus, Focus::Browser);
}

#[test]
fn route_change_closes_modal_state_and_clears_hidden_actions() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('2')));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    let _ = update(&mut model, Message::Input(ctrl('d')));
    assert_eq!(model.overlay(), Some(OverlayKind::Confirmation));
    assert_eq!(model.focus, Focus::Confirmation);
    assert!(model.browser_delete_confirmation().is_some());

    let _ = update(&mut model, Message::Input(ctrl('/')));
    type_text(&mut model, "profiles");
    assert_eq!(model.overlay(), Some(OverlayKind::CommandPalette));

    let _ = update(&mut model, Message::Input(ctrl('3')));
    assert_eq!(model.route(), Route::Profiles);
    assert_eq!(model.overlay(), None);
    assert!(model.browser_delete_confirmation().is_none());
}

#[test]
fn help_returns_to_the_route_it_was_opened_from() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('3')));
    let _ = update(&mut model, Message::Input(key(Key::F(1))));
    assert_eq!(model.route(), Route::Help);

    let _ = update(&mut model, Message::Input(key(Key::F(1))));
    assert_eq!(model.route(), Route::Profiles);
    assert_eq!(model.focus, Focus::Profiles);
}

#[test]
fn composer_draft_survives_primary_route_navigation() {
    let mut model = model();
    type_text(&mut model, "draft survives routes");
    let _ = update(&mut model, Message::Input(ctrl('2')));
    let _ = update(&mut model, Message::Input(ctrl('3')));
    let _ = update(&mut model, Message::Input(ctrl('1')));

    assert_eq!(model.composer.text(), "draft survives routes");
    assert_eq!(model.focus, Focus::Composer);
}

#[test]
fn every_route_renders_through_wide_rail_and_compact_tabs() {
    let cases = [
        ('1', "Conversation"),
        ('2', "Sessions"),
        ('3', "Providers & Connections"),
        ('4', "Settings & Provenance"),
        ('5', "Help"),
    ];
    for (width, height) in [(120, 40), (80, 24), (60, 18), (40, 12)] {
        let mut model = model();
        for (key, expected) in cases {
            let _ = update(&mut model, Message::Input(ctrl(key)));
            let rendered = render_text(&model, width, height);
            assert!(
                rendered.contains(expected),
                "{expected} missing at {width}x{height}"
            );
            if key == '4' && width >= 60 {
                for section in ["Settings", "Providers", "Profile", "Agents"] {
                    assert!(
                        rendered.contains(section),
                        "settings nav {section} missing at {width}x{height}"
                    );
                }
            }
            assert!(rendered.contains("Profile"), "profile action missing");
            assert!(rendered.contains("Settings"), "settings action missing");
            if width >= 100 {
                assert!(rendered.contains("PROJECTS"), "projects section missing");
                assert!(!rendered.contains("PREVIOUS SESSIONS"));
            }
        }
    }
}

#[test]
fn chat_empty_states_name_one_primary_recovery_action() {
    let mut model = model();
    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::CredentialRequired)),
    );
    let _ = update(&mut model, Message::Input(ctrl('1')));
    let offline = render_text(&model, 80, 24);
    assert!(offline.contains("OFFLINE"));
    assert!(offline.contains("Provider API key: use /settings"));

    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::Loading)),
    );
    let loading = render_text(&model, 80, 24);
    assert!(loading.contains("CONNECTING"));

    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::Failed(UiFailure::new(
            ErrorClass::Unavailable,
            "provider unavailable",
            RetryPolicy::Now,
        )))),
    );
    let failed = render_text(&model, 80, 24);
    assert!(failed.contains("CONNECTION ERROR"));

    assert!(failed.contains("Ctrl+R retry"));
}
#[test]
fn startup_boot_surface_animates_and_exits_deterministically() {
    let mut model = loading_model();
    let initial = render_text(&model, 80, 24);
    assert!(initial.contains("AutoHarness / boot"));
    let _ = update(&mut model, Message::Tick(100));
    let first = render_text(&model, 80, 24);
    let _ = update(&mut model, Message::Tick(200));
    let second = render_text(&model, 80, 24);
    assert!(first.contains("AUTOHARNESS"));
    assert!(first.contains("CONNECTING"));
    assert_ne!(first, second);

    let _ = update(&mut model, Message::Tick(2_000));
    let settled = render_text(&model, 80, 24);
    assert!(!settled.contains("AutoHarness / boot"));
    assert!(settled.contains("CONNECTING"));
}

#[test]
fn chat_empty_state_explains_the_zero_shell_start_path() {
    let model = model();
    let rendered = render_text(&model, 80, 24);
    assert!(rendered.contains("GET STARTED"));
    assert!(rendered.contains("/settings set a provider key"));
    assert!(rendered.contains("Conversation · Active conversation"));
}

#[test]
fn ascii_glyph_mode_uses_ascii_conversation_separators() {
    let mut model = model();
    let settings = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "glyph_mode": "ascii"
                    }
                }
            }"#,
        )
        .resolve()
        .expect("ASCII preferences");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: settings.local_profile().clone(),
        ..SettingsProjection::default()
    }));
    let rendered = render_text(&model, 120, 40);
    assert!(rendered.contains("Conversation | Active conversation"));
    assert!(!rendered.contains("Conversation · Active conversation"));
}

#[test]
fn settings_selection_keeps_the_selected_preference_visible_when_narrow() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Input(key(Key::End)));
    let rendered = render_text(&model, 40, 12);
    assert!(rendered.contains("Left/Right pages"));
}
#[test]
fn settings_top_navigation_reaches_provider_and_future_sections() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let rendered = render_text(&model, 80, 24);
    for section in ["Settings", "Providers", "Profile", "Agents"] {
        assert!(rendered.contains(section), "missing settings nav {section}");
    }

    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert_eq!(model.route(), Route::Settings);
    assert_eq!(model.focus, Focus::Settings);
}

#[test]
fn providers_returns_to_settings_navigation_before_leaving_the_route() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert!(render_text(&model, 80, 24).contains("Google AI Studio"));

    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    let _ = update(&mut model, Message::Input(key(Key::Right)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));

    assert_eq!(model.route(), Route::Settings);
    assert_eq!(model.focus, Focus::UserProfile);
}

#[test]
fn settings_arrows_move_between_pages_and_into_preferences() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));

    let _ = update(&mut model, Message::Input(key(Key::Left)));
    let _ = update(&mut model, Message::Input(key(Key::Right)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let _ = update(&mut model, Message::Input(key(Key::Up)));
    let _ = update(&mut model, Message::Input(key(Key::Right)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert_eq!(model.route(), Route::Settings);
}

#[test]
fn wide_shell_keeps_route_bar_persistent_and_sidebar_titles_single_line() {
    let mut model = model();
    let long_title =
        "A very long named session that must remain a single visible sidebar line".to_owned();
    let _ = update(
        &mut model,
        Message::SessionsChanged(Arc::new(SessionsProjection {
            sessions: vec![SessionBrowserEntry {
                session_id: "session-active".to_owned(),
                title: long_title,
                archived: false,
                selected_model: Some(model_ref()),
                updated_at_ms: 1,
                active: true,
            }],
        })),
    );
    let rendered = render_text(&model, 120, 40);
    assert!(!rendered.contains("1 Chat"));
    assert!(rendered.contains("Profile"));
    assert!(rendered.contains("Settings"));
    assert!(rendered.contains("…"));
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.contains('│') && line.contains("A very long named"))
            .count(),
        1
    );
}
#[test]
fn compact_shell_uses_commands_and_bottom_actions() {
    let model = model();
    let rendered = render_text(&model, 48, 18);
    assert!(!rendered.contains("1 Chat"));
    assert!(rendered.contains("Profile"));
    assert!(rendered.contains("Settings"));
    assert_eq!(
        hit_test(&model, 48, 18, 2, 17),
        Some(MouseAction::SettingsTab(2))
    );
    assert_eq!(
        hit_test(&model, 48, 18, 14, 17),
        Some(MouseAction::SettingsTab(0))
    );
}

#[test]
fn settings_tab_mouse_geometry_matches_persistent_shell() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    assert!(
        (28..80).any(|column| {
            hit_test(&model, 120, 40, column, 1) == Some(MouseAction::SettingsTab(1))
        }),
        "provider Settings tab must have a mouse target"
    );
}

#[test]
#[ignore = "visual review harness for the Phase 3.7 routed shell"]
fn render_route_review_matrix() {
    for (width, height) in [(120, 50), (120, 40), (80, 24), (60, 18), (40, 12)] {
        let mut model = model();

        for (key, route) in [
            ('1', Route::Chat),
            ('2', Route::Sessions),
            ('3', Route::Profiles),
            ('4', Route::Settings),
            ('5', Route::Help),
        ] {
            let _ = update(&mut model, Message::Input(ctrl(key)));
            println!(
                "=== {} {width}x{height} ===\n{}",
                route.label(),
                render_text(&model, width, height)
            );
        }
    }

    let mut confirmation = model();
    let _ = update(&mut confirmation, Message::Input(ctrl('2')));
    let _ = update(&mut confirmation, Message::Input(key(Key::Down)));
    let _ = update(&mut confirmation, Message::Input(ctrl('d')));
    println!(
        "=== Confirmation 80x24 ===\n{}",
        render_text(&confirmation, 80, 24)
    );
}
#[test]
fn mouse_hit_testing_covers_wide_sidebar_and_compact_routes() {
    let model = model();
    assert_eq!(
        hit_test(&model, 120, 40, 2, 1),
        Some(MouseAction::Route(Route::Sessions))
    );
    assert_eq!(
        hit_test(&model, 120, 40, 2, 38),
        Some(MouseAction::SettingsTab(2))
    );
    assert_eq!(
        hit_test(&model, 120, 40, 14, 38),
        Some(MouseAction::SettingsTab(0))
    );
    assert_eq!(
        hit_test(&model, 80, 24, 2, 23),
        Some(MouseAction::SettingsTab(2))
    );
}

#[test]
fn mouse_opens_and_saves_the_user_profile_dialog() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('3')));
    assert_eq!(
        hit_test(&model, 120, 40, 30, 1),
        Some(MouseAction::OpenUserProfile)
    );
    let _ = update(&mut model, Message::Mouse(MouseAction::OpenUserProfile));
    assert!(model.user_profile_open());
    assert!(render_text(&model, 120, 40).contains("User profile"));

    assert_eq!(
        hit_test(&model, 120, 40, 30, 22),
        Some(MouseAction::UserProfileSave)
    );
    assert_eq!(
        hit_test(&model, 120, 40, 70, 22),
        Some(MouseAction::UserProfileCancel)
    );
    let effects = update(&mut model, Message::Mouse(MouseAction::UserProfileSave));
    assert!(matches!(
        effects.as_slice(),
        [autoharness_tui::UiEffect::Dispatch(
            UiIntent::UpdateLocalPreference { .. }
        )]
    ));
    assert!(!model.user_profile_open());
}

#[test]
fn mouse_profile_actions_share_keyboard_intents() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('3')));
    assert_eq!(
        hit_test(&model, 120, 40, 30, 38),
        Some(MouseAction::ProfileNew)
    );
    let effects = update(&mut model, Message::Mouse(MouseAction::ProfileNew));

    assert!(effects.is_empty());
    assert!(render_text(&model, 120, 40).contains("Create provider profile"));
}
#[test]
fn mouse_session_action_bar_exposes_each_visible_action() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('2')));
    for (column, expected) in [
        (2, MouseAction::SessionOpen),
        (20, MouseAction::SessionRename),
        (40, MouseAction::SessionArchive),
        (60, MouseAction::SessionDelete),
    ] {
        assert_eq!(
            hit_test(&model, 80, 24, column, 22),
            Some(expected),
            "missing session action at column {column}"
        );
    }
}

#[test]
fn mouse_modal_rows_select_models_and_run_commands() {
    let mut picker = model();
    let _ = update(&mut picker, Message::Input(ctrl('p')));
    let selection = hit_test(&picker, 80, 24, 12, 5);
    assert!(matches!(selection, Some(MouseAction::PickerSelect(_))));
    let effects = update(&mut picker, Message::Mouse(selection.expect("picker row")));
    assert!(matches!(
        effects.as_slice(),
        [autoharness_tui::UiEffect::Dispatch(
            UiIntent::SelectModel { .. }
        )]
    ));

    let mut palette = model();
    let _ = update(&mut palette, Message::Input(ctrl('/')));
    let command = hit_test(&palette, 80, 24, 12, 14);
    assert!(matches!(command, Some(MouseAction::PaletteRun(_))));
    let effects = update(&mut palette, Message::Mouse(command.expect("palette row")));
    assert!(effects.is_empty());
    assert!(!palette.palette_open());
}

#[test]
fn mouse_credential_dialog_controls_are_clickable() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('k')));
    assert_eq!(model.overlay(), Some(OverlayKind::SessionCredential));
    assert_eq!(
        hit_test(&model, 80, 24, 10, 15),
        Some(MouseAction::CredentialSubmit)
    );
    assert_eq!(
        hit_test(&model, 80, 24, 60, 15),
        Some(MouseAction::CredentialCancel)
    );
    let _ = update(&mut model, Message::Mouse(MouseAction::CredentialCancel));
    assert!(model.overlay().is_none());
}
