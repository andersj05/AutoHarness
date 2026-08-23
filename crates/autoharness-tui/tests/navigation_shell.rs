use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, Focus, Message, Model, ModelSummary, OverlayKind, Route,
    SessionBrowserEntry, SessionProjection, SessionsProjection, update,
};
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
