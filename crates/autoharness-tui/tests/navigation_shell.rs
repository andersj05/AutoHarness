use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, CredentialSourceLabel, Focus,
    LocalUserProfileProjection, Message, Model, ModelSummary, MouseAction, OverlayKind,
    PermissionDetailView, PermissionRequestView, ProfileConnectionState,
    ProfileCredentialStateLabel, ProfilesProjection, ProviderKindLabel, ProviderProfileProjection,
    RetryPolicy, Route, SessionBrowserEntry, SessionProjection, SessionsProjection,
    SettingsProjection, ToolCallKey, TranscriptItem, UiClock, UiFailure, UiIntent, hit_test,
    update, view,
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
                message_count: 2,
                updated_at_ms: 2,
                active: true,
            },
            SessionBrowserEntry {
                session_id: "session-other".to_owned(),
                title: "Other conversation".to_owned(),
                archived: false,
                selected_model: None,
                message_count: 0,
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
            context_window_tokens: Some(1_000_000),
            selectable: true,
        }],
        stale: false,
    });
    Model::new(session, sessions, catalog)
}

fn provider_model() -> Model {
    let mut model = model();
    model.apply_profiles(Arc::new(ProfilesProjection {
        user: LocalUserProfileProjection {
            display_label: Some("Conformance user".to_owned()),
            workspace: "C:/work/autoharness".to_owned(),
            default_profile: Some("personal-gemini".to_owned()),
            default_model: Some("gemini-shell".to_owned()),
            default_mode: "safe agent".to_owned(),
        },
        profiles: vec![
            ProviderProfileProjection {
                id: "personal-gemini".to_owned(),
                kind: ProviderKindLabel::Gemini,
                active: true,
                base_url: String::new(),
                project: String::new(),
                auth_header: String::new(),
                credential_state: ProfileCredentialStateLabel::Stored,
                credential_source: CredentialSourceLabel::CredentialVault,
                connection: ProfileConnectionState::Ready,
                default_model: Some("gemini-shell".to_owned()),
                default_mode: "safe agent".to_owned(),
            },
            ProviderProfileProjection {
                id: "work-router".to_owned(),
                kind: ProviderKindLabel::Router,
                active: false,
                base_url: "https://router.example.test/v1/".to_owned(),
                project: "work".to_owned(),
                auth_header: "x-router-key".to_owned(),
                credential_state: ProfileCredentialStateLabel::Disconnected,
                credential_source: CredentialSourceLabel::SessionOnly,
                connection: ProfileConnectionState::Untested,
                default_model: None,
                default_mode: "safe agent".to_owned(),
            },
        ],
        pending_recovery: 0,
    }));
    model
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
        ('6', Route::Memory, Focus::Memory),
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
        ('1', "Ask Agent"),
        ('2', "Sessions"),
        ('3', "Providers"),
        ('4', "Settings"),
        ('5', "Help"),
        ('6', "Memory index"),
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
            if key == '4' && width >= 80 {
                for section in [
                    "Appearance",
                    "Chat & Composer",
                    "Accessibility",
                    "Providers",
                    "Models & Thinking",
                    "Profile",
                    "Sessions & Data",
                    "Shortcuts",
                    "About",
                ] {
                    assert!(
                        rendered.contains(section),
                        "settings nav {section} missing at {width}x{height}"
                    );
                }
            }
            if width >= 100 {
                assert!(rendered.contains("Profiles"), "profiles rail missing");
                assert!(rendered.contains("Settings"), "settings rail missing");
                assert!(rendered.contains("Workspace"), "workspace section missing");
                assert!(!rendered.contains("PREVIOUS SESSIONS"));
            }
        }
    }
}

#[test]
fn chat_empty_states_leave_the_conversation_canvas_uncluttered() {
    let mut model = model();
    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::CredentialRequired)),
    );
    let _ = update(&mut model, Message::Input(ctrl('1')));
    let offline = render_text(&model, 80, 24);
    assert!(offline.contains("Ask Agent"));
    assert!(!offline.contains("Connect a provider key"));
    assert!(!offline.contains("Choose a compatible model"));

    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::Loading)),
    );
    let loading = render_text(&model, 80, 24);
    assert!(loading.contains("Ask Agent"));
    assert!(!loading.contains("Loading provider models"));

    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::Failed(UiFailure::new(
            ErrorClass::Unavailable,
            "provider unavailable",
            RetryPolicy::Now,
        )))),
    );
    let failed = render_text(&model, 80, 24);
    assert!(failed.contains("Ask Agent"));
    assert!(!failed.contains("Connection error"));
    assert!(!failed.contains("Ctrl+R retry"));
}
#[test]
fn startup_boot_surface_animates_and_exits_deterministically() {
    let mut model = loading_model();
    let initial = render_text(&model, 80, 24);
    assert!(initial.contains("AutoHarness"));
    assert!(initial.contains("Loading provider models..."));
    assert!(!initial.contains('%'));
    let _ = update(&mut model, Message::Tick(UiClock::new(100, 0)));
    let first = render_text(&model, 80, 24);
    let _ = update(&mut model, Message::Tick(UiClock::new(200, 0)));
    let second = render_text(&model, 80, 24);
    assert!(first.contains("Starting"));
    assert_ne!(first, second);

    let _ = update(&mut model, Message::Tick(UiClock::new(400, 0)));
    let settled = render_text(&model, 80, 24);
    assert!(!settled.contains("Starting"));
    assert!(settled.contains("Ask Agent"));
    assert!(!settled.contains("Loading provider models..."));
}

#[test]
fn startup_surface_exits_as_soon_as_model_loading_finishes() {
    let mut model = loading_model();
    let _ = update(
        &mut model,
        Message::CatalogChanged(Arc::new(CatalogProjection::CredentialRequired)),
    );

    let rendered = render_text(&model, 80, 24);
    assert!(!rendered.contains("Starting"));
    assert!(rendered.contains("Ask Agent"));
    assert!(!rendered.contains("Offline"));
}

#[test]
fn a_new_conversation_is_blank_apart_from_the_composer() {
    let model = model();
    let rendered = render_text(&model, 80, 24);
    assert!(rendered.contains("Ask Agent"));
    assert!(!rendered.contains("New conversation"));
    assert!(!rendered.contains("Write a prompt below"));
    assert!(!rendered.contains("Connect a provider key"));
    assert!(!rendered.contains("Choose a compatible model"));
    assert!(!rendered.contains("Conversation"));
}

#[test]
fn ascii_glyph_mode_uses_a_single_sidebar_divider_without_conversation_chrome() {
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
    assert!(rendered.lines().all(|line| !line.contains('│')));
    assert!(rendered.lines().filter(|line| line.contains('|')).count() >= 30);
    assert!(!rendered.contains("Conversation"));
}

#[test]
fn settings_selection_keeps_the_selected_preference_visible_when_narrow() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let _ = update(&mut model, Message::Input(key(Key::End)));
    let rendered = render_text(&model, 40, 12);
    assert!(rendered.contains("Glyph mode"));
    assert!(rendered.contains("Left/Right"));
}

#[test]
fn editable_settings_show_adjacent_values_as_a_scroll_wheel() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Input(key(Key::Tab)));

    for (width, height) in [(120, 40), (80, 24), (40, 12)] {
        let rendered = render_text(&model, width, height);
        assert!(rendered.contains("Theme"));
        assert!(
            rendered.contains("system"),
            "missing choice at {width}x{height}"
        );
        assert!(rendered.contains("Left/Right"));
    }
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let rendered = render_text(&model, 120, 40);
    for theme in [
        "system", "light", "dark", "aurora", "ember", "midnight", "ocean", "forest", "rose",
    ] {
        assert!(rendered.contains(theme), "missing theme preview {theme}");
    }
}
#[test]
fn settings_top_navigation_reaches_provider_and_future_sections() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let rendered = render_text(&model, 80, 24);
    for section in [
        "Appearance",
        "Chat & Composer",
        "Accessibility",
        "Providers",
        "Models & Thinking",
        "Profile",
        "Sessions & Data",
        "Shortcuts",
        "About",
    ] {
        assert!(rendered.contains(section), "missing settings nav {section}");
    }

    let _ = update(&mut model, Message::Input(key(Key::Right)));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    assert_eq!(model.route(), Route::Settings);
    assert_eq!(model.focus, Focus::Settings);
}

#[test]
fn tab_moves_between_settings_categories_and_rows() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let before = render_text(&model, 80, 24);
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let after = render_text(&model, 80, 24);
    assert_ne!(after, before);
    assert!(after.contains("Left/Right change"));
    let _ = update(
        &mut model,
        Message::Input(Input {
            key: Key::Tab,
            shift: true,
            ..key(Key::Tab)
        }),
    );
    assert!(render_text(&model, 80, 24).contains("Up/Down category"));
}

#[test]
fn providers_category_uses_typed_actions_and_steps_back_one_level() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let rendered = render_text(&model, 80, 24);
    assert!(rendered.contains("Connection"));
    assert!(rendered.contains("API credential"));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert_eq!(model.overlay(), Some(OverlayKind::SessionCredential));
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    assert_eq!(model.route(), Route::Settings);
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    assert_eq!(model.route(), Route::Chat);
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
                message_count: 2,
                updated_at_ms: 1,
                active: true,
            }],
        })),
    );
    let rendered = render_text(&model, 120, 40);
    assert!(!rendered.contains("1 Chat"));
    assert!(rendered.contains("Profiles"));
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
    assert!(rendered.contains("Ask Agent"));
    assert_eq!(
        hit_test(&model, 48, 18, 2, 2),
        Some(MouseAction::FocusComposer)
    );
}

#[test]
fn settings_tab_mouse_geometry_matches_persistent_shell() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    assert!(
        (0..120).any(|column| {
            (0..40).any(|row| {
                hit_test(&model, 120, 40, column, row) == Some(MouseAction::SettingsTab(3))
            })
        }),
        "Providers category must have a mouse target"
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
            ('6', Route::Memory),
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
        hit_test(&model, 120, 40, 2, 2),
        Some(MouseAction::Route(Route::Chat))
    );
    assert_eq!(
        hit_test(&model, 120, 40, 2, 5),
        Some(MouseAction::Route(Route::Settings))
    );
    assert_eq!(
        hit_test(&model, 120, 40, 14, 5),
        Some(MouseAction::Route(Route::Settings))
    );
    assert_eq!(
        hit_test(&model, 80, 24, 2, 2),
        Some(MouseAction::FocusComposer)
    );
}

#[test]
fn mouse_reaches_the_populated_profile_category_and_display_label_row() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    let _ = update(&mut model, Message::Mouse(MouseAction::SettingsTab(5)));
    assert!(render_text(&model, 120, 40).contains("Local identity"));
    assert!((0..120).any(|column| {
        (0..40)
            .any(|row| hit_test(&model, 120, 40, column, row) == Some(MouseAction::SettingsRow(2)))
    }));
    let _ = update(&mut model, Message::Mouse(MouseAction::SettingsRow(2)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let _ = update(&mut model, Message::Input(key(Key::Char('A'))));
    assert!(render_text(&model, 120, 40).contains("A|"));
}

#[test]
fn profile_help_row_has_no_hidden_mouse_action() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('3')));
    assert_eq!(hit_test(&model, 120, 40, 30, 38), None);
}
#[test]
fn mouse_session_action_bar_exposes_each_visible_action() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('2')));
    for expected in [
        MouseAction::SessionOpen,
        MouseAction::SessionRename,
        MouseAction::SessionArchive,
        MouseAction::SessionDelete,
    ] {
        assert!(
            (0..80).any(|column| hit_test(&model, 80, 24, column, 22) == Some(expected.clone())),
            "missing measured session action {expected:?}"
        );
    }
}

#[test]
fn mouse_modal_rows_select_models_and_run_commands() {
    let mut picker = model();
    let _ = update(&mut picker, Message::Input(ctrl('p')));
    let selection = (0..24).find_map(|row| {
        (0..80).find_map(|column| {
            hit_test(&picker, 80, 24, column, row)
                .filter(|action| matches!(action, MouseAction::PickerSelect(_)))
        })
    });
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
    type_text(&mut palette, "settings");
    let command = (0..24).find_map(|row| {
        (0..80).find_map(|column| {
            hit_test(&palette, 80, 24, column, row).filter(
                |action| matches!(action, MouseAction::PaletteRun(command) if command == "settings"),
            )
        })
    });
    assert!(matches!(command, Some(MouseAction::PaletteRun(_))));
    let effects = update(&mut palette, Message::Mouse(command.expect("palette row")));
    assert!(effects.is_empty());
    assert!(!palette.palette_open());
    assert_eq!(palette.route(), Route::Settings);
}

#[test]
fn mouse_credential_dialog_controls_are_clickable() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('k')));
    assert_eq!(model.overlay(), Some(OverlayKind::SessionCredential));
    for expected in [MouseAction::CredentialSubmit, MouseAction::CredentialCancel] {
        assert!((0..24).any(|row| {
            (0..80).any(|column| hit_test(&model, 80, 24, column, row) == Some(expected.clone()))
        }));
    }
    let _ = update(&mut model, Message::Mouse(MouseAction::CredentialCancel));
    assert!(model.overlay().is_none());
}

fn variant_name(action: &MouseAction) -> &'static str {
    match action {
        MouseAction::Route(_) => "Route",
        MouseAction::SettingsTab(_) => "SettingsTab",
        MouseAction::FocusComposer => "FocusComposer",
        MouseAction::FocusTranscript => "FocusTranscript",
        MouseAction::ChatModels => "ChatModels",
        MouseAction::ChatRetry => "ChatRetry",
        MouseAction::ChatFreshSession => "ChatFreshSession",
        MouseAction::SettingsRow(_) => "SettingsRow",
        MouseAction::ProfileCredential => "ProfileCredential",
        MouseAction::ProfileTest => "ProfileTest",
        MouseAction::ProfileDefaultModel => "ProfileDefaultModel",
        MouseAction::ProfileDisconnect => "ProfileDisconnect",
        MouseAction::ProfileDelete => "ProfileDelete",
        MouseAction::SelectProviderChoice(_) => "SelectProviderChoice",
        MouseAction::CodexLogin => "CodexLogin",
        MouseAction::CodexLoginCancel => "CodexLoginCancel",
        MouseAction::ProfileEditorSubmit => "ProfileEditorSubmit",
        MouseAction::ProfileEditorCancel => "ProfileEditorCancel",
        MouseAction::SelectProfile(_) => "SelectProfile",
        MouseAction::UserProfileSave => "UserProfileSave",
        MouseAction::SessionOpen => "SessionOpen",
        MouseAction::SessionRename => "SessionRename",
        MouseAction::SessionArchive => "SessionArchive",
        MouseAction::SessionDelete => "SessionDelete",
        MouseAction::Confirm => "Confirm",
        MouseAction::Cancel => "Cancel",
        MouseAction::UserProfileCancel => "UserProfileCancel",
        MouseAction::PickerSelect(_) => "PickerSelect",
        MouseAction::PaletteRun(_) => "PaletteRun",
        MouseAction::CredentialSubmit => "CredentialSubmit",
        MouseAction::CredentialCancel => "CredentialCancel",
        MouseAction::ProfileCredentialSubmit => "ProfileCredentialSubmit",
        MouseAction::ProfileCredentialCancel => "ProfileCredentialCancel",
        MouseAction::OverlayCancel => "OverlayCancel",
        MouseAction::PermissionAllow => "PermissionAllow",
        MouseAction::PermissionDeny => "PermissionDeny",
        MouseAction::MemoryFocusSearch => "MemoryFocusSearch",
        MouseAction::MemorySelect(_) => "MemorySelect",
        MouseAction::MemorySelectAdmission(_) => "MemorySelectAdmission",
        MouseAction::MemoryCycleStatus => "MemoryCycleStatus",
        MouseAction::MemoryCycleScope => "MemoryCycleScope",
        MouseAction::MemoryPreviousPage => "MemoryPreviousPage",
        MouseAction::MemoryNextPage => "MemoryNextPage",
        MouseAction::MemoryOpen => "MemoryOpen",
        MouseAction::MemoryBack => "MemoryBack",
        MouseAction::MemoryAdmissions => "MemoryAdmissions",
        MouseAction::MemoryRemember => "MemoryRemember",
        MouseAction::MemoryRevise => "MemoryRevise",
        MouseAction::MemoryReview => "MemoryReview",
        MouseAction::MemoryActions => "MemoryActions",
        MouseAction::MemoryRetract => "MemoryRetract",
        MouseAction::MemoryDelete => "MemoryDelete",
        MouseAction::MemoryExport => "MemoryExport",
        MouseAction::MemoryActionSelect(_) => "MemoryActionSelect",
        MouseAction::MemoryLifecycleSubmit => "MemoryLifecycleSubmit",
        MouseAction::MemoryProposalReject => "MemoryProposalReject",
        MouseAction::MemoryLifecycleCancel => "MemoryLifecycleCancel",
    }
}

fn collect_hit_variants(
    model: &Model,
    width: u16,
    height: u16,
    into: &mut std::collections::HashSet<&'static str>,
) {
    for column in 0..width {
        for row in 0..height {
            if let Some(action) = hit_test(model, width, height, column, row) {
                into.insert(variant_name(&action));
            }
        }
    }
}

#[test]
fn chat_composer_transcript_and_settings_rows_are_clickable() {
    let mut model = model();
    assert_eq!(
        hit_test(&model, 120, 40, 40, 20),
        Some(MouseAction::FocusTranscript)
    );
    assert!(
        (0..120).any(|column| {
            (0..40).any(|row| {
                hit_test(&model, 120, 40, column, row) == Some(MouseAction::FocusComposer)
            })
        }),
        "composer must be clickable"
    );
    assert!(
        (0..120).any(|column| {
            (0..40)
                .any(|row| hit_test(&model, 120, 40, column, row) == Some(MouseAction::ChatModels))
        }),
        "status-line model segment must open the picker"
    );
    let _ = update(&mut model, Message::Mouse(MouseAction::FocusComposer));
    assert_eq!(model.focus, Focus::Composer);

    let _ = update(&mut model, Message::Input(ctrl('4')));
    assert!(
        (0..120).any(|column| {
            (0..40).any(|row| {
                matches!(
                    hit_test(&model, 120, 40, column, row),
                    Some(MouseAction::SettingsRow(_))
                )
            })
        }),
        "General Settings rows must be clickable"
    );
}

#[test]
fn every_mouse_action_variant_is_produced_by_layout() {
    let mut produced = std::collections::HashSet::new();
    collect_hit_variants(&model(), 120, 40, &mut produced);
    collect_hit_variants(&model(), 80, 24, &mut produced);
    collect_hit_variants(&model(), 48, 18, &mut produced);

    let mut settings = model();
    let _ = update(&mut settings, Message::Input(ctrl('4')));
    collect_hit_variants(&settings, 120, 40, &mut produced);

    let mut user_profile = model();
    let _ = update(
        &mut user_profile,
        Message::Input(Input {
            key: Key::Char('u'),
            alt: true,
            ..key(Key::Char('u'))
        }),
    );
    collect_hit_variants(&user_profile, 120, 40, &mut produced);
    let _ = update(&mut settings, Message::Input(key(Key::Tab)));
    collect_hit_variants(&settings, 120, 40, &mut produced);

    let mut sessions = model();
    let _ = update(&mut sessions, Message::Input(ctrl('2')));
    collect_hit_variants(&sessions, 80, 24, &mut produced);
    let _ = update(&mut sessions, Message::Input(key(Key::Down)));
    let _ = update(&mut sessions, Message::Input(ctrl('d')));
    collect_hit_variants(&sessions, 80, 24, &mut produced);

    let mut picker = model();
    let _ = update(&mut picker, Message::Input(ctrl('p')));
    collect_hit_variants(&picker, 80, 24, &mut produced);

    let mut palette = model();
    let _ = update(&mut palette, Message::Input(ctrl('/')));
    collect_hit_variants(&palette, 80, 24, &mut produced);

    let mut credential = model();
    let _ = update(&mut credential, Message::Input(ctrl('k')));
    collect_hit_variants(&credential, 80, 24, &mut produced);

    let mut permission = model();
    let mut session = (*permission.session.clone()).clone();
    session.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("tool-call-1").expect("id"),
        tool_name: "fs_read".to_owned(),
        capability: "filesystem read".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: Vec::new(),
    });
    let _ = update(&mut permission, Message::SessionChanged(Arc::new(session)));
    collect_hit_variants(&permission, 80, 24, &mut produced);

    let mut providers = provider_model();
    let _ = update(&mut providers, Message::Input(ctrl('g')));
    collect_hit_variants(&providers, 120, 40, &mut produced);

    let mut profile_credential = provider_model();
    let _ = update(&mut profile_credential, Message::Input(ctrl('g')));
    let _ = update(&mut profile_credential, Message::Input(key(Key::Down)));
    let _ = update(
        &mut profile_credential,
        Message::Input(Input {
            key: Key::Char('k'),
            alt: true,
            ..key(Key::Char('k'))
        }),
    );
    collect_hit_variants(&profile_credential, 80, 24, &mut produced);

    let mut profile_editor = provider_model();
    let _ = update(&mut profile_editor, Message::Input(ctrl('g')));
    let _ = update(&mut profile_editor, Message::Input(key(Key::Enter)));
    collect_hit_variants(&profile_editor, 80, 24, &mut produced);

    let mut codex_login = provider_model();
    let _ = update(&mut codex_login, Message::Input(ctrl('g')));
    for _ in 0..3 {
        let _ = update(&mut codex_login, Message::Input(key(Key::Down)));
    }
    let _ = update(&mut codex_login, Message::Input(key(Key::Enter)));
    collect_hit_variants(&codex_login, 80, 24, &mut produced);

    let mut failed = model();
    let mut failed_session = (*failed.session).clone();
    failed_session.revision = 2;
    failed_session.transcript.push(TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-fail").expect("attempt"),
        text: String::new(),
        status: AttemptStatus::Failed(UiFailure::new(
            ErrorClass::Unavailable,
            "provider unavailable",
            RetryPolicy::Now,
        )),
        usage: None,
        retry_of: None,
    });
    let _ = update(
        &mut failed,
        Message::SessionChanged(Arc::new(failed_session)),
    );
    collect_hit_variants(&failed, 80, 24, &mut produced);

    for expected in [
        "Route",
        "SettingsTab",
        "FocusComposer",
        "FocusTranscript",
        "ChatModels",
        "ChatRetry",
        "ChatFreshSession",
        "SettingsRow",
        "ProfileCredential",
        "ProfileTest",
        "ProfileDefaultModel",
        "ProfileDisconnect",
        "ProfileDelete",
        "UserProfileSave",
        "UserProfileCancel",
        "SessionOpen",
        "SessionRename",
        "SessionArchive",
        "SessionDelete",
        "Confirm",
        "Cancel",
        "PickerSelect",
        "PaletteRun",
        "CredentialSubmit",
        "CredentialCancel",
        "ProfileCredentialSubmit",
        "ProfileCredentialCancel",
        "OverlayCancel",
        "PermissionAllow",
        "PermissionDeny",
        "SelectProviderChoice",
        "CodexLogin",
        "CodexLoginCancel",
        "ProfileEditorSubmit",
        "ProfileEditorCancel",
        "SelectProfile",
    ] {
        assert!(
            produced.contains(expected),
            "{expected} was not produced by layout; have {produced:?}"
        );
    }
}
