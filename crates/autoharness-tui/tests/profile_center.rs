use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, CredentialSourceLabel, Focus, LocalUserProfileProjection, Message, Model,
    ModelSummary, MouseAction, ProfileConnectionState, ProfileCredentialStateLabel,
    ProfilesProjection, ProviderKindLabel, ProviderProfileProjection, SessionProjection,
    SessionsProjection, UiEffect, UiIntent, hit_test, update, view,
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
            detail: String::new(),
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
fn profile_center_shows_local_profile_connections_and_responsive_layouts() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));

    assert!(model.profile_center_open());
    assert_eq!(model.focus, Focus::Profiles);
    assert_eq!(model.profile_selection(), Some("personal-gemini"));

    for (width, height) in [(120, 40), (80, 24), (60, 18), (40, 12)] {
        let rendered = render_text(&model, width, height);
        assert!(rendered.contains("Profiles & Providers"));
        assert!(rendered.contains("Jensen"));
        assert!(rendered.contains("personal-gemini"));
    }
    let wide = render_text(&model, 120, 40);
    assert!(wide.contains("credential vault"));
    assert!(wide.contains("connected"));
}

#[test]
fn create_forms_emit_typed_gemini_and_router_profiles() {
    let mut gemini = model();
    let _ = update(&mut gemini, Message::Input(ctrl('g')));
    let _ = update(&mut gemini, Message::Input(alt('n')));
    type_text(&mut gemini, "second-gemini");
    let effects = update(&mut gemini, Message::Input(key(Key::Enter)));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::UpsertProfile { profile, .. })]
            if profile.id == "second-gemini" && profile.kind == ProviderKindLabel::Gemini
    ));

    let mut router = model();
    let _ = update(&mut router, Message::Input(ctrl('g')));
    let _ = update(&mut router, Message::Input(alt('n')));
    type_text(&mut router, "second-router");
    let _ = update(&mut router, Message::Input(key(Key::Tab)));
    let _ = update(&mut router, Message::Input(key(Key::Right)));
    let _ = update(&mut router, Message::Input(key(Key::Tab)));
    type_text(&mut router, "https://router.example.test/v1/");
    let effects = update(&mut router, Message::Input(key(Key::Enter)));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::UpsertProfile { profile, .. })]
            if profile.id == "second-router"
                && profile.kind == ProviderKindLabel::Router
                && profile.base_url == "https://router.example.test/v1/"
    ));
}

#[test]
fn credential_entry_is_masked_redacted_and_profile_scoped() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));
    let _ = update(&mut model, Message::Input(key(Key::Down)));
    assert_eq!(model.profile_selection(), Some("work-router"));
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
        [UiEffect::Dispatch(UiIntent::SaveProfileCredential { profile_id, .. })]
            if profile_id == "work-router"
    ));
    assert!(!format!("{effects:?}").contains(sentinel));
}

#[test]
fn visible_profile_actions_converge_on_typed_intents_and_confirm_destruction() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));

    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Enter))).as_slice(),
        [UiEffect::Dispatch(UiIntent::ActivateProfile { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));
    assert!(matches!(
        update(&mut model, Message::Input(alt('t'))).as_slice(),
        [UiEffect::Dispatch(UiIntent::TestProfile { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));
    assert!(matches!(
        update(&mut model, Message::Input(alt('m'))).as_slice(),
        [UiEffect::Dispatch(UiIntent::SetProfileDefaultModel { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));

    let _ = update(&mut model, Message::Input(alt('x')));
    assert!(update(&mut model, Message::Input(key(Key::Char('n')))).is_empty());
    let _ = update(&mut model, Message::Input(alt('x')));
    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Char('y')))).as_slice(),
        [UiEffect::Dispatch(UiIntent::DisconnectProfile { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));

    let _ = update(&mut model, Message::Input(key(Key::Delete)));
    assert!(update(&mut model, Message::Input(key(Key::Char('n')))).is_empty());
    let _ = update(&mut model, Message::Input(key(Key::Delete)));
    assert!(matches!(
        update(&mut model, Message::Input(key(Key::Char('y')))).as_slice(),
        [UiEffect::Dispatch(UiIntent::DeleteProfile { profile_id, .. })]
            if profile_id == "personal-gemini"
    ));
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
    for (column, expected) in [
        (68, MouseAction::ProfileNew),
        (76, MouseAction::ProfileCredential),
        (84, MouseAction::ProfileTest),
        (92, MouseAction::ProfileDefaultModel),
        (68, MouseAction::ProfileDisconnect),
        (83, MouseAction::ProfileDelete),
    ] {
        assert!(
            (0..40).any(|row| hit_test(&model, 120, 40, column, row) == Some(expected.clone())),
            "missing profile click target at column {column}"
        );
    }
}

#[test]
fn compact_profile_clicks_follow_rendered_content_rows() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(ctrl('g')));

    assert_eq!(
        hit_test(&model, 80, 24, 2, 1),
        Some(MouseAction::OpenUserProfile)
    );
    assert_eq!(
        hit_test(&model, 80, 24, 2, 6),
        Some(MouseAction::SelectProfile("personal-gemini".to_owned()))
    );
    assert_eq!(
        hit_test(&model, 80, 24, 34, 15),
        Some(MouseAction::ProfileNew)
    );
    assert_eq!(hit_test(&model, 80, 24, 34, 14), None);
    assert_eq!(hit_test(&model, 80, 24, 34, 13), None);
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
