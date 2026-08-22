use std::sync::Arc;

use autoharness_tui::{
    CatalogProjection, Focus, Message, Model, ModelSummary, SessionBrowserEntry, SessionProjection,
    SessionsProjection, UiEffect, UiIntent, UiNotice, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn model_ref(id: &str) -> autoharness_domain::ModelRef {
    autoharness_domain::ModelRef::new(
        autoharness_domain::ProviderId::new("google-ai-studio").expect("valid provider ID"),
        autoharness_domain::ModelId::new(id).expect("valid model ID"),
    )
}

fn ready_catalog() -> Arc<CatalogProjection> {
    Arc::new(CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: model_ref("models/gemini-2.5-pro"),
            display_name: "Gemini 2.5 Pro".to_owned(),
            detail: String::new(),
            selectable: true,
        }],
        stale: false,
    })
}

fn session(session_id: &str) -> Arc<SessionProjection> {
    Arc::new(SessionProjection {
        session_id: session_id.to_owned(),
        revision: 1,
        selected_model: Some(model_ref("models/gemini-2.5-pro")),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    })
}

fn sessions(entries: &[SessionBrowserEntry]) -> Arc<SessionsProjection> {
    Arc::new(SessionsProjection {
        sessions: entries.to_vec(),
    })
}

fn entry(session_id: &str, title: &str, archived: bool, active: bool) -> SessionBrowserEntry {
    SessionBrowserEntry {
        session_id: session_id.to_owned(),
        title: title.to_owned(),
        archived,
        selected_model: Some(model_ref("models/gemini-2.5-pro")),
        updated_at_ms: 1_000,
        active,
    }
}

fn key_input(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn char_input(character: char) -> Input {
    key_input(Key::Char(character))
}

fn ctrl(character: char) -> Input {
    Input {
        key: Key::Char(character),
        ctrl: true,
        alt: false,
        shift: false,
    }
}

/// Commits every dispatched intent so pending state clears like production.
fn commit_all(model: &mut Model, effects: &[UiEffect]) {
    for effect in effects {
        if let UiEffect::Dispatch(intent) = effect {
            let request_id = intent.request_id();
            let _ = update(
                model,
                Message::Notice(UiNotice::IntentCommitted { request_id }),
            );
        }
    }
}

fn render_text(model: &Model, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| view(frame, model))
        .expect("draw test frame");
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ctrl_l_opens_browser_and_lists_durable_sessions() {
    let mut model = Model::new(
        session("session-a"),
        sessions(&[
            entry("session-b", "Deep dive", false, false),
            entry("session-a", "Untitled", false, true),
        ]),
        ready_catalog(),
    );
    assert!(!model.browser_open());

    let _ = update(&mut model, Message::Input(ctrl('l')));

    assert!(model.browser_open());
    assert_eq!(model.focus, Focus::Browser);
    // Opening pre-selects the active session.
    assert_eq!(model.browser_selection(), Some("session-a"));
    let rendered = render_text(&model, 80, 24);
    assert!(rendered.contains("Sessions"));
    assert!(rendered.contains("Deep dive"));
    assert!(rendered.contains("[active]"));

    // Escape closes and restores composer focus.
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.browser_open());
    assert_eq!(model.focus, Focus::Composer);
}

#[test]
fn browser_search_filters_rows_and_enter_opens_the_selected_session() {
    let mut model = Model::new(
        session("session-a"),
        sessions(&[
            entry("session-b", "Café research", false, false),
            entry("session-c", "Unrelated", false, false),
            entry("session-a", "Active work", false, true),
        ]),
        ready_catalog(),
    );
    let _ = update(&mut model, Message::Input(ctrl('l')));

    for character in "research".chars() {
        let _ = update(&mut model, Message::Input(char_input(character)));
    }

    // Case-insensitive filter narrows to the café row and moves selection.
    let visible = model.browser_entries();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].title, "Café research");
    assert_eq!(model.browser_selection(), Some("session-b"));

    let effects = update(&mut model, Message::Input(key_input(Key::Enter)));
    match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::OpenSession { session_id, .. })] => {
            assert_eq!(session_id, "session-b");
        }
        other => panic!("expected open-session intent, got {other:?}"),
    }

    // The acknowledgement closes the browser.
    commit_all(&mut model, &effects);
    assert!(!model.browser_open());
}

#[test]
fn plain_letters_extend_the_filter_instead_of_triggering_actions() {
    let mut model = Model::new(
        session("session-a"),
        sessions(&[entry("session-b", "Old work", true, false)]),
        ready_catalog(),
    );
    let _ = update(&mut model, Message::Input(ctrl('l')));

    // Plain r, a, d characters extend the query; nothing is dispatched.
    for character in ['r', 'a', 'd'] {
        let effects = update(&mut model, Message::Input(char_input(character)));
        assert!(effects.is_empty(), "{character} must filter only");
        assert!(!model.browser_renaming());
    }
    assert_eq!(model.browser_query(), "rad");

    // Clear the filter, then the chord form dispatches the real action.
    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key_input(Key::Backspace)));
    }
    let rename = update(&mut model, Message::Input(ctrl('r')));
    assert!(model.browser_renaming());
    commit_all(&mut model, &rename);
}

#[test]
fn switching_stashes_the_outgoing_draft_and_restores_the_incoming_draft() {
    let mut model = Model::new(
        session("session-a"),
        sessions(&[
            entry("session-a", "Active work", false, true),
            entry("session-b", "Other", false, false),
        ]),
        ready_catalog(),
    );
    // Type a draft in the current session.
    let _ = update(&mut model, Message::Paste("draft for a".to_owned()));

    let _ = update(&mut model, Message::Input(ctrl('l')));
    // Move to the other session and press Enter.
    let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    let effects = update(&mut model, Message::Input(key_input(Key::Enter)));
    match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::OpenSession { session_id, .. })] => {
            assert_eq!(session_id, "session-b");
        }
        other => panic!("expected open-session intent, got {other:?}"),
    }
    commit_all(&mut model, &effects);

    // Simulate the projection swap performed by application composition.
    let _ = update(&mut model, Message::SessionChanged(session("session-b")));

    // The incoming composer is empty because session-b had no saved draft.
    assert_eq!(model.composer.text(), "");
    let _ = update(&mut model, Message::Paste("draft for b".to_owned()));
    assert_eq!(model.composer.text(), "draft for b");

    // Switching back restores session-a's stashed draft.
    let _ = update(&mut model, Message::Input(ctrl('l')));
    let _ = update(&mut model, Message::Input(key_input(Key::Up)));
    let effects = update(&mut model, Message::Input(key_input(Key::Enter)));
    commit_all(&mut model, &effects);
    let _ = update(&mut model, Message::SessionChanged(session("session-a")));
    assert_eq!(model.composer.text(), "draft for a");
}

#[test]
fn archive_unarchive_rename_and_confirmed_delete_dispatch_exact_intents() {
    let mut model = Model::new(
        session("session-live"),
        sessions(&[
            entry("session-live", "Live work", false, true),
            entry("session-old", "Old work", true, false),
        ]),
        ready_catalog(),
    );

    // Rename flow buffers from the current title of the live session.
    let _ = update(&mut model, Message::Input(ctrl('l')));
    assert_eq!(model.browser_selection(), Some("session-live"));
    let rename_effects = {
        let _ = update(&mut model, Message::Input(ctrl('r')));
        assert!(model.browser_renaming());
        assert_eq!(model.browser_rename_buffer(), "Live work");
        for character in " renamed".chars() {
            let _ = update(&mut model, Message::Input(char_input(character)));
        }
        update(&mut model, Message::Input(key_input(Key::Enter)))
    };
    match rename_effects.as_slice() {
        [
            UiEffect::Dispatch(UiIntent::RenameSession {
                session_id, title, ..
            }),
        ] => {
            assert_eq!(session_id, "session-live");
            assert_eq!(title, "Live work renamed");
        }
        other => panic!("expected rename intent, got {other:?}"),
    }
    commit_all(&mut model, &rename_effects);

    // Archive dispatches for the highlighted non-active session.
    let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    assert_eq!(model.browser_selection(), Some("session-old"));
    let archive_effects = update(&mut model, Message::Input(ctrl('a')));
    assert!(
        matches!(
            archive_effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::UnarchiveSession { session_id, .. })]
                if session_id == "session-old"
        ),
        "archiving an archived row must unarchive it"
    );
    commit_all(&mut model, &archive_effects);

    // Delete requires two presses: Ctrl+D arms it, Y confirms.
    let arm = update(&mut model, Message::Input(ctrl('d')));
    assert!(arm.is_empty());
    assert_eq!(model.browser_delete_confirmation(), Some("session-old"));
    let confirm = update(&mut model, Message::Input(char_input('y')));
    assert!(
        matches!(
            confirm.as_slice(),
            [UiEffect::Dispatch(UiIntent::DeleteSession { session_id, .. })]
                if session_id == "session-old"
        ),
        "expected delete intent"
    );
    commit_all(&mut model, &confirm);
    assert!(model.browser_delete_confirmation().is_none());

    // A stray Y with nothing armed is ignored.
    let stray = update(&mut model, Message::Input(char_input('y')));
    assert!(stray.is_empty());
}

#[test]
fn ctrl_c_quits_from_inside_the_browser_overlay() {
    let mut model = Model::new(
        session("session-live"),
        sessions(&[
            entry("session-live", "Live work", false, true),
            entry("session-old", "Old work", false, false),
        ]),
        ready_catalog(),
    );
    let _ = update(&mut model, Message::Input(ctrl('l')));
    assert!(model.browser_open());
    assert!(!model.should_quit);

    let effects = update(&mut model, Message::Input(ctrl('c')));

    assert!(model.should_quit);
    assert!(matches!(effects.as_slice(), [UiEffect::Quit]));
}

#[test]
fn deleting_the_active_session_is_refused_locally() {
    let mut model = Model::new(
        session("session-live"),
        sessions(&[entry("session-live", "Live work", false, true)]),
        ready_catalog(),
    );
    let _ = update(&mut model, Message::Input(ctrl('l')));

    let effects = update(&mut model, Message::Input(ctrl('d')));

    assert!(effects.is_empty());
    assert!(model.browser_delete_confirmation().is_none());
    assert!(
        model
            .notice
            .as_ref()
            .is_some_and(|notice| format!("{notice:?}").contains("Switch to another"))
    );
}
