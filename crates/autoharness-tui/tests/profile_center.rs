use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, CredentialSourceLabel, Focus, LocalUserProfileProjection, Message, Model,
    ModelSummary, MouseAction, ProfileConnectionState, ProfileCredentialStateLabel,
    ProfilesProjection, ProviderKindLabel, ProviderProfileProjection, RetryPolicy,
    SessionProjection, SessionsProjection, UiEffect, UiFailure, UiIntent, UiNotice, hit_test,
    update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn model_ref() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider id"),
        ModelId::new("models/gemini-2.5-pro").expect("model id"),
    )
}

fn model() -> Model {
    let session = Arc::new(SessionProjection {
        session_id: "session-fixture".to_owned(),
        revision: 1,
        selected_model: Some(model_ref()),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    });
    let catalog = Arc::new(CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: model_ref(),
            display_name: "Gemini 2.5 Pro".to_owned(),
            detail: "thinking".to_owned(),
            context_window_tokens: Some(1_000_000),
            selectable: true,
        }],
        stale: false,
    });
    let mut model = Model::new(session, Arc::new(SessionsProjection::default()), catalog);
    model.apply_profiles(Arc::new(ProfilesProjection {
        user: LocalUserProfileProjection {
            display_label: Some("Jensen".to_owned()),
            workspace: "C:/work/autoharness".to_owned(),
            default_profile: Some("personal-gemini".to_owned()),
            default_model: Some("gemini-2.5-pro".to_owned()),
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
                default_model: Some("gemini-2.5-pro".to_owned()),
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

fn alt(character: char) -> Input {
    Input {
        alt: true,
        ..key(Key::Char(character))
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
fn providers_list_connection_choices_at_responsive_sizes() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));

    assert!(model.profile_center_open());
    assert_eq!(model.focus, Focus::Profiles);
    let wide = render_text(&model, 120, 40);
    assert!(wide.contains("Provider catalog"));
    assert!(wide.contains("Saved connections"));
    for provider in [
        "Gemini",
        "Google AI Studio API",
        "Cursor",
        "Codex",
        "Claude Code",
    ] {
        assert!(wide.contains(provider), "missing {provider}");
    }
    assert!(wide.contains("Unavailable"));
    assert!(wide.contains("adapter not available"));
    for section in ["Identity", "Connection", "Credential", "Defaults"] {
        assert!(
            wide.contains(section),
            "missing provider detail section {section}"
        );
    }
    for (width, height) in [(80, 24), (60, 18), (40, 12)] {
        assert!(render_text(&model, width, height).contains("Providers"));
    }
}

#[test]
fn codex_provider_selection_opens_the_subscription_authentication_page() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let rendered = render_text(&model, 120, 40);
    assert!(rendered.contains("Sign in to Codex"));
    assert!(rendered.contains("Sign in with ChatGPT"));
    assert!(rendered.contains("default browser"));
    assert!((0..40).any(|row| {
        (0..120).any(|column| {
            matches!(
                hit_test(&model, 120, 40, column, row),
                Some(MouseAction::CodexLogin)
            )
        })
    }));

    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::StartCodexLogin { request_id })] => *request_id,
        other => panic!("unexpected login effects: {other:?}"),
    };
    assert!(
        render_text(&model, 120, 40).contains("Opening your browser"),
        "the authentication popup should show launch progress"
    );
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::CodexLoginBrowserOpened { request_id }),
    );
    assert!(render_text(&model, 120, 40).contains("Browser opened"));
    let effects = update(
        &mut model,
        Message::Notice(UiNotice::CodexLoginCompleted { request_id }),
    );
    assert!(effects.is_empty());
    assert!(render_text(&model, 120, 40).contains("Codex subscription connected"));
}

#[test]
fn codex_sign_in_can_be_cancelled_or_retried_after_failure() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::StartCodexLogin { request_id })] => *request_id,
        other => panic!("unexpected login effects: {other:?}"),
    };
    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Esc))).as_slice(),
        [UiEffect::Dispatch(UiIntent::CancelCodexLogin { request_id: cancelled })]
            if *cancelled == request_id
    ));

    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted { request_id }),
    );
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    let failed_request = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::StartCodexLogin { request_id })] => *request_id,
        other => panic!("unexpected retry effects: {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentRejected {
            request_id: failed_request,
            failure: UiFailure::new(
                ErrorClass::Unavailable,
                "The browser could not be opened",
                RetryPolicy::Now,
            ),
        }),
    );
    assert!(render_text(&model, 120, 40).contains("Try sign-in again"));
    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Enter))).as_slice(),
        [UiEffect::Dispatch(UiIntent::StartCodexLogin { .. })]
    ));
}

#[test]
fn models_tab_saves_the_active_profiles_model_and_thinking_mode() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('4')));
    for _ in 0..4 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));

    let rendered = render_text(&model, 120, 40);
    assert!(rendered.contains("Models"));
    assert!(rendered.contains("Thinking"));
    assert!(rendered.contains("Active profile  personal-gemini"));
    for _ in 0..4 {
        let _ = update(&mut model, Message::Input(key(Key::Right)));
    }
    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Enter))).as_slice(),
        [UiEffect::Dispatch(UiIntent::SetProfileDefault { profile_id, model, reasoning_effort, .. })]
            if profile_id == "personal-gemini" && *model == model_ref() && reasoning_effort.as_deref() == Some("high")
    ));
}

#[test]
fn provider_arrows_preserve_connected_profile_selection() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    for _ in 0..6 {
        let _ = update(&mut model, Message::Input(key(Key::Down)));
    }
    assert!(render_text(&model, 80, 24).contains("Saved connections"));
    assert_eq!(model.profile_selection(), Some("personal-gemini"));

    let _ = update(&mut model, Message::Input(key(Key::Right)));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    assert_eq!(model.profile_selection(), Some("work-router"));
    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ActivateProfile { profile_id, .. })]
            if profile_id == "work-router"
    ));

    let _ = update(&mut model, Message::Input(key(Key::Up)));
    assert_eq!(model.profile_selection(), Some("personal-gemini"));
    let _ = update(&mut model, Message::Input(key(Key::Up)));
    assert_eq!(model.profile_selection(), Some("personal-gemini"));
    let _ = update(&mut model, Message::Input(key(Key::Left)));
    for _ in 0..5 {
        let _ = update(&mut model, Message::Input(key(Key::Up)));
    }
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert!(render_text(&model, 120, 40).contains("Connect Gemini"));
}

#[test]
fn provider_editor_uses_arrows_instead_of_tab() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let before = render_text(&model, 80, 24);
    let _ = update(&mut model, Message::Input(key(Key::Tab)));
    assert_eq!(render_text(&model, 80, 24), before);
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    assert!(render_text(&model, 80, 24).contains("> Provider"));
}
#[test]
fn credential_entry_is_masked_redacted_and_profile_scoped() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    assert_eq!(model.profile_selection(), Some("personal-gemini"));
    let _ = update(&mut model, Message::Input(alt('k')));
    assert_eq!(
        hit_test(&model, 80, 24, 12, 19),
        Some(MouseAction::ProfileCredentialSubmit)
    );
    assert_eq!(
        hit_test(&model, 80, 24, 60, 19),
        Some(MouseAction::ProfileCredentialCancel)
    );
    let sentinel = "router-secret-sentinel";
    let _ = update(&mut model, Message::Paste(sentinel.to_owned()));

    let rendered = render_text(&model, 80, 24);
    assert!(rendered.contains("••••••••"));
    assert!(!rendered.contains(sentinel));
    assert!(!format!("{model:?}").contains(sentinel));

    let effects = update(
        &mut model,
        Message::Mouse(MouseAction::ProfileCredentialSubmit),
    );
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ReplaceProfileCredential { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));
    assert!(!format!("{effects:?}").contains(sentinel));
}

#[test]
fn models_picker_can_save_the_selected_model_as_profile_default() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('p')));
    let effects = update(&mut model, Message::Input(key(Key::Char('d'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::SetProfileDefaultModel { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));
}

#[test]
fn every_profile_detail_button_has_a_semantic_click_target() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    for expected in [
        MouseAction::ProfileCredential,
        MouseAction::ProfileTest,
        MouseAction::ProfileDefaultModel,
        MouseAction::ProfileDisconnect,
        MouseAction::ProfileDelete,
    ] {
        assert!(
            (0..120).any(|column| (0..40)
                .any(|row| hit_test(&model, 120, 40, column, row) == Some(expected.clone()))),
            "missing profile click target for {expected:?}"
        );
    }
}

#[test]
fn compact_profile_clicks_follow_rendered_content_rows() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    assert!((0..80).any(
        |column| (0..24).any(|row| hit_test(&model, 80, 24, column, row)
            == Some(MouseAction::SelectProfile("personal-gemini".to_owned())))
    ));
    assert!((0..80).any(|column| (0..24).any(
        |row| hit_test(&model, 80, 24, column, row) == Some(MouseAction::ProfileDefaultModel)
    )));
}
#[test]
#[ignore = "visual review harness for the Phase 3.6 profile center"]
fn render_profile_center_review_sizes() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    for (width, height) in [(120, 50), (120, 40), (80, 24), (60, 18), (40, 12)] {
        println!(
            "=== profiles {width}x{height} ===\n{}",
            render_text(&model, width, height)
        );
    }
}
