use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, CredentialSourceLabel, Focus, LocalUserProfileProjection, Message, Model,
    ModelSummary, Notice, ProfileConnectionState, ProfileCredentialStateLabel, ProfilesProjection,
    ProviderKindLabel, ProviderProfileProjection, Route, SessionProjection, SessionsProjection,
    UiEffect, UiIntent, update,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn catalog_ready() -> Arc<CatalogProjection> {
    Arc::new(CatalogProjection::Ready {
        models: vec![
            ModelSummary {
                model: ModelRef::new(
                    ProviderId::new("google-ai-studio").expect("provider id"),
                    ModelId::new("models/gemini-2.5-pro").expect("model id"),
                ),
                display_name: "Gemini 2.5 Pro".to_owned(),
                detail: "text | thinking".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            },
            ModelSummary {
                model: ModelRef::new(
                    ProviderId::new("openai-compatible").expect("provider id"),
                    ModelId::new("router-reasoner").expect("model id"),
                ),
                display_name: "Router Reasoner".to_owned(),
                detail: "text | tools".to_owned(),
                context_window_tokens: Some(128_000),
                selectable: true,
            },
        ],
        stale: false,
    })
}

fn pro_model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider id"),
        ModelId::new("models/gemini-2.5-pro").expect("model id"),
    )
}

fn session() -> Arc<SessionProjection> {
    Arc::new(SessionProjection {
        session_id: "session-fixture".to_owned(),
        revision: 1,
        selected_model: Some(pro_model()),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    })
}

fn empty_model() -> Model {
    Model::new(
        session(),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    )
}

fn apply_active_profile(model: &mut Model) {
    model.apply_profiles(Arc::new(ProfilesProjection {
        user: LocalUserProfileProjection {
            default_profile: Some("personal".to_owned()),
            default_model: Some("models/gemini-2.5-pro".to_owned()),
            default_mode: "high".to_owned(),
            ..LocalUserProfileProjection::default()
        },
        profiles: vec![ProviderProfileProjection {
            id: "personal".to_owned(),
            kind: ProviderKindLabel::Gemini,
            active: true,
            base_url: String::new(),
            project: String::new(),
            auth_header: String::new(),
            credential_state: ProfileCredentialStateLabel::Stored,
            credential_source: CredentialSourceLabel::CredentialVault,
            connection: ProfileConnectionState::Ready,
            default_model: Some("models/gemini-2.5-pro".to_owned()),
            default_mode: "high".to_owned(),
        }],
        pending_recovery: 0,
    }));
}

fn key_input(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn ctrl(key: Key) -> Input {
    Input {
        ctrl: true,
        ..key_input(key)
    }
}

fn enter() -> Input {
    key_input(Key::Enter)
}

fn type_text(model: &mut Model, text: &str) {
    for character in text.chars() {
        let _ = update(model, Message::Input(key_input(Key::Char(character))));
    }
}

fn render_model(model: &Model, width: u16, height: u16) -> TestBackend {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| autoharness_tui::view(frame, model))
        .expect("deterministic draw");
    terminal.backend().clone()
}

fn buffer_text(backend: &TestBackend) -> String {
    let area = backend.buffer().area;
    let mut rendered = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        for x in area.x..area.right() {
            line.push_str(
                backend
                    .buffer()
                    .cell((x, y))
                    .expect("position inside test buffer")
                    .symbol(),
            );
        }
        rendered.push_str(line.trim_end());
        rendered.push('\n');
    }
    rendered
}

#[test]
fn ctrl_o_opens_a_modal_searchable_command_palette() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));

    assert!(model.palette_open());
    assert_eq!(model.focus, Focus::Palette, "the palette owns the keyboard");
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("/models"));
    assert!(rendered.contains("Commands"));
    assert!(rendered.contains("WORKSPACE"));
    assert!(rendered.contains("SESSION SETUP"));
    for expected in [
        "/chat",
        "/sessions",
        "/profile",
        "/provider",
        "/models",
        "/user",
        "/new",
        "/session-model",
    ] {
        assert!(rendered.contains(expected), "missing {expected} row");
    }
    type_text(&mut model, "settings");
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("/settings"));

    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.palette_open());
    assert_eq!(model.focus, Focus::Composer);
}

#[test]
fn typing_slash_opens_live_command_browser_and_filters_as_you_type() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(key_input(Key::Char('/'))));
    assert!(model.palette_open());
    let all = buffer_text(&render_model(&model, 80, 24));
    assert!(all.contains("/chat"));
    assert!(all.contains("/models"));
    assert!(all.contains("/provider"));
    type_text(&mut model, "mod");
    let filtered = buffer_text(&render_model(&model, 80, 24));
    assert!(filtered.lines().any(|line| line.contains("/mod")));
    assert!(filtered.contains("/models"));
    assert!(!filtered.contains("/sessions"));
}

#[test]
fn command_rows_are_unique_identifier_first_and_keep_cursor_visible() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("/profile") && line.contains("Profile settings"))
    );
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("/models") && line.contains("Default model"))
    );
    assert_eq!(rendered.matches("/profile").count(), 1);
    assert!(!rendered.contains("/profiles"));

    type_text(&mut model, "profile");
    let filtered = buffer_text(&render_model(&model, 80, 24));
    assert!(filtered.contains("❯ /profile"));
}

#[test]
fn inline_command_rows_follow_the_top_prompt_at_narrow_width() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(key_input(Key::Char('/'))));

    let rendered = render_model(&model, 40, 12);
    let text = buffer_text(&rendered);
    assert!(text.lines().take(4).any(|line| line.contains("❯ /")));
    let command = text.find("/chat").expect("command row");
    let prompt = text.find("❯ /").expect("top prompt");
    assert!(prompt < command);
}
#[test]
fn deleting_the_initial_slash_closes_command_browser() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(key_input(Key::Char('/'))));
    assert!(model.palette_open());
    let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    assert!(!model.palette_open());
    assert!(model.composer.is_blank());
}

#[test]
fn palette_filtering_is_case_insensitive_and_selection_stays_valid() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "API");

    assert!(!model.palette_query().is_empty());
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("/connect"));
    assert!(!rendered.contains("/sessions"));

    // An empty result set must keep the palette open and ignore Enter.
    type_text(&mut model, "zzz");
    let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    type_text(&mut model, "zzz");
    assert!(update(&mut model, Message::Input(enter())).is_empty());
    assert!(!model.palette_open());
    assert_eq!(model.composer.text(), "/APIzzz");

    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.palette_open());
}

#[test]
fn palette_ranks_exact_prefixes_and_typo_matches_first() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "new");
    assert_eq!(model.palette_selection(), Some("new"));
    assert_eq!(
        model.palette_entries().first().map(|entry| entry.id),
        Some("new")
    );

    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    }
    type_text(&mut model, "settngs");
    assert_eq!(model.palette_selection(), Some("settings"));
    assert_eq!(
        model.palette_entries().first().map(|entry| entry.id),
        Some("settings")
    );
}

#[test]
fn palette_enter_executes_sessions_and_returns_focus_to_the_browser() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "sess");

    let effects = update(&mut model, Message::Input(enter()));

    assert!(effects.is_empty(), "opening the browser is a local action");
    assert!(!model.palette_open());
    assert!(model.browser_open());
    assert_eq!(model.focus, Focus::Browser);
}

#[test]
fn palette_new_session_dispatches_the_same_typed_intent_as_ctrl_n() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "new");

    let effects = update(&mut model, Message::Input(enter()));

    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::CreateSession { .. })]
        ),
        "the palette must converge on the identical typed intent"
    );
    assert!(!model.palette_open());
}

#[test]
fn concise_command_names_execute_and_legacy_names_remain_compatible() {
    for command in ["/new", "/new-session"] {
        let mut model = empty_model();
        type_text(&mut model, command);
        assert!(matches!(
            update(&mut model, Message::Input(enter())).as_slice(),
            [UiEffect::Dispatch(UiIntent::CreateSession { .. })]
        ));
    }

    let mut model = empty_model();
    type_text(&mut model, "/tools");
    assert!(update(&mut model, Message::Input(enter())).is_empty());
    assert!(model.composer.is_blank());
}

#[test]
fn palette_settings_entry_toggles_the_same_non_modal_overlay() {
    let mut model = empty_model();
    assert!(!model.settings_open());
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));

    // Filter by stable command identity rather than relying on table position.
    type_text(&mut model, "settings");
    let _ = update(&mut model, Message::Input(enter()));

    assert!(model.settings_open());
    assert!(!model.palette_open());
}

#[test]
fn global_chords_still_work_while_the_palette_is_open() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "filter text");

    let effects = update(&mut model, Message::Input(ctrl(Key::Char('n'))));

    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::CreateSession { .. })]
        ),
        "Ctrl+N stays global"
    );
    assert!(
        !model.palette_open(),
        "the global action closes the modal slot"
    );
    assert_eq!(model.route(), Route::Chat);

    // Ctrl+C quits from inside the palette like everywhere else.
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('c'))));
    assert!(matches!(effects.as_slice(), [UiEffect::Quit]));
    assert!(model.should_quit);
}

#[test]
fn known_slash_commands_execute_and_clear_the_composer() {
    let mut model = empty_model();
    type_text(&mut model, "/settings");
    let _ = update(&mut model, Message::Input(enter()));

    assert!(model.settings_open());
    assert!(
        model.composer.is_blank(),
        "a executed command must not linger in the composer"
    );
    let mut model = empty_model();
    type_text(&mut model, "/profiles");
    let _ = update(&mut model, Message::Input(enter()));
    assert_eq!(model.route(), Route::Settings);
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("Providers"));

    // The historical /sessions spelling keeps working through the shared table.
    let mut model = empty_model();
    type_text(&mut model, "/sessions");
    let _ = update(&mut model, Message::Input(enter()));
    assert!(model.browser_open());

    assert!(model.composer.is_blank());
}
#[test]
fn provider_command_opens_provider_setup_route() {
    let mut model = empty_model();
    type_text(&mut model, "/provider");
    let _ = update(&mut model, Message::Input(enter()));
    assert_eq!(model.route(), Route::Profiles);
    assert_eq!(model.focus, Focus::Profiles);
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("Providers"));
}

#[test]
fn profile_slash_command_matches_palette_route() {
    let mut slash = empty_model();
    type_text(&mut slash, "/profile");
    let _ = update(&mut slash, Message::Input(enter()));
    assert_eq!(slash.route(), Route::Settings);
    assert_eq!(slash.focus, Focus::Settings);
    assert!(slash.composer.is_blank());
    assert!(buffer_text(&render_model(&slash, 80, 24)).contains("Profile"));

    let mut palette = empty_model();
    let _ = update(&mut palette, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut palette, "profile");
    let _ = update(&mut palette, Message::Input(enter()));
    assert_eq!(palette.route(), Route::Settings);
    assert_eq!(palette.focus, Focus::Settings);
}

#[test]
fn models_command_opens_the_integrated_default_model_tab() {
    let mut model = empty_model();
    type_text(&mut model, "/models");
    let _ = update(&mut model, Message::Input(enter()));
    assert_eq!(model.route(), Route::Settings);
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("Models"));
    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("Router Reasoner"));
    assert!(rendered.contains("1M context"));
    assert!(rendered.contains("128K context"));
    assert!(rendered.contains("thinking"));
    assert!(rendered.contains("tools"));
    assert!(rendered.contains("Thinking"));
}

#[test]
fn default_model_page_starts_at_the_saved_values_and_persists_them_together() {
    let mut model = empty_model();
    apply_active_profile(&mut model);
    type_text(&mut model, "/models");
    let _ = update(&mut model, Message::Input(enter()));

    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("Default"));
    assert!(rendered.contains("Thinking"));
    assert!(rendered.contains("high"));

    let effects = update(&mut model, Message::Input(enter()));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::SetProfileDefault {
            profile_id,
            model: selected,
            reasoning_effort: Some(effort),
            ..
        })] if profile_id == "personal"
            && selected == &pro_model()
            && effort == "high"
    ));
}

#[test]
fn thinking_mode_changes_inline_before_saving_with_the_model() {
    let mut model = empty_model();
    apply_active_profile(&mut model);
    type_text(&mut model, "/models");
    let _ = update(&mut model, Message::Input(enter()));

    let _ = update(&mut model, Message::Input(key_input(Key::Left)));
    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(rendered.contains("Thinking"));
    assert!(rendered.contains("medium"));

    let effects = update(&mut model, Message::Input(enter()));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::SetProfileDefault {
            reasoning_effort: Some(effort),
            ..
        })] if effort == "medium"
    ));
}

#[test]
fn unknown_slash_commands_are_rejected_without_losing_text() {
    let mut model = empty_model();
    type_text(&mut model, "/frobnicate");
    let effects = update(&mut model, Message::Input(enter()));

    assert!(effects.is_empty(), "an unknown command must not dispatch");
    assert_eq!(
        model.composer.text(),
        "/frobnicate",
        "rejected input stays editable"
    );
    assert!(
        matches!(&model.notice, Some(Notice::Failure(failure))
            if failure.message.contains("Unknown command") && failure.message.contains("/frobnicate")),
        "the rejection must name the offending command"
    );
}

#[test]
fn slash_commands_require_a_single_bare_token() {
    let mut model = empty_model();
    type_text(&mut model, "/settings extra");
    let _ = update(&mut model, Message::Input(enter()));
    assert!(
        !model.settings_open(),
        "arguments are not part of any command yet"
    );
    assert_eq!(model.composer.text(), "/settings extra");
}

#[test]
fn multiline_slash_text_submits_as_an_ordinary_prompt() {
    let mut model = empty_model();
    type_text(&mut model, "/sessions");
    let _ = update(&mut model, Message::Input(enter()));
    assert!(model.browser_open());
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));

    let mut model = empty_model();
    type_text(&mut model, "//");
    let _ = update(
        &mut model,
        Message::Paste("code block\nline two".to_owned()),
    );
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('s'))));

    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::SubmitPrompt { prompt, .. })]
                if prompt == "//code block\nline two"
        ),
        "multiline text starting with slash is a prompt, not a command"
    );
}

#[test]
fn doubled_leading_slash_escapes_into_a_literal_prompt() {
    let mut model = empty_model();
    type_text(&mut model, "//not a command");
    let effects = update(&mut model, Message::Input(enter()));

    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::SubmitPrompt { prompt, .. })]
            if prompt == "/not a command"
    ));
    assert!(model.composer.is_blank());
}

#[test]
fn slash_copy_places_the_transcript_on_the_clipboard_via_the_runner() {
    let mut model = empty_model();
    type_text(&mut model, "/copy");
    let effects = update(&mut model, Message::Input(enter()));

    assert!(
        matches!(effects.as_slice(), [UiEffect::CopyTranscript(_)]),
        "copy must emit the runner effect carrying transcript text"
    );
    assert!(model.composer.is_blank());
}

#[test]
fn slash_export_dispatches_the_durable_export_intent() {
    let mut model = empty_model();
    type_text(&mut model, "/export");
    let effects = update(&mut model, Message::Input(enter()));

    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::ExportTranscript { .. })]
        ),
        "export must dispatch the typed durable intent"
    );
    assert!(model.composer.is_blank());
}

#[test]
fn palette_rows_include_copy_and_export() {
    let ids: Vec<&str> = autoharness_tui::COMMANDS.iter().map(|c| c.id).collect();
    for id in ["copy", "export"] {
        assert!(ids.contains(&id), "{id} must be a first-class command");
    }
}

#[test]
fn palette_renders_at_small_sizes_without_panicking() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    for (width, height) in [(40, 12), (24, 7), (10, 4)] {
        let backend = render_model(&model, width, height);
        assert_eq!(backend.buffer().area.width, width);
        assert_eq!(backend.buffer().area.height, height);
    }
}

#[test]
fn inline_and_centered_palettes_paint_the_same_command_row() {
    let mut inline = empty_model();
    let _ = update(&mut inline, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut inline, "settings");
    let inline = buffer_text(&render_model(&inline, 80, 24));

    let mut centered = empty_model();
    let _ = update(&mut centered, Message::Input(ctrl(Key::Char('2'))));
    let _ = update(&mut centered, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut centered, "settings");
    let centered = buffer_text(&render_model(&centered, 80, 24));

    let command_row = |rendered: &str| {
        rendered
            .lines()
            .find(|line| line.contains("/settings") && line.contains("Show effective"))
            .map(str::trim)
            .expect("settings palette row")
            .to_owned()
    };
    assert_eq!(command_row(&inline), command_row(&centered));
}
