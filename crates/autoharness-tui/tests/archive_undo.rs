use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, Message, Model, ModelSummary, Notice, SessionBrowserEntry,
    SessionProjection, SessionsProjection, UiEffect, UiIntent, update,
};
use ratatui_textarea::{Input, Key};

fn pro_model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider id"),
        ModelId::new("models/gemini-2.5-pro").expect("model id"),
    )
}

fn catalog_ready() -> Arc<CatalogProjection> {
    Arc::new(CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: pro_model(),
            display_name: "Gemini 2.5 Pro".to_owned(),
            detail: String::new(),
            context_window_tokens: Some(1_000_000),
            selectable: true,
        }],
        stale: false,
    })
}

fn session() -> Arc<SessionProjection> {
    Arc::new(SessionProjection {
        session_id: "session-active".to_owned(),
        revision: 1,
        selected_model: Some(pro_model()),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    })
}

fn sessions_list() -> Arc<SessionsProjection> {
    Arc::new(SessionsProjection {
        sessions: vec![
            SessionBrowserEntry {
                session_id: "session-active".to_owned(),
                title: "Active session".to_owned(),
                archived: false,
                selected_model: Some(pro_model()),
                message_count: 2,
                updated_at_ms: 2,
                active: true,
            },
            SessionBrowserEntry {
                session_id: "session-other".to_owned(),
                title: "Other session".to_owned(),
                archived: false,
                selected_model: None,
                message_count: 0,
                updated_at_ms: 1,
                active: false,
            },
        ],
    })
}

fn empty_model() -> Model {
    Model::new(session(), sessions_list(), catalog_ready())
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

fn open_browser(model: &mut Model) {
    let _ = update(model, Message::Input(ctrl(Key::Char('l'))));
    // Move to the non-active row.
    let _ = update(model, Message::Input(key_input(Key::Down)));
}

#[test]
fn archive_arms_and_requires_y_before_dispatch() {
    let mut model = empty_model();
    open_browser(&mut model);

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    assert!(
        model.browser_archive_confirmation().is_some(),
        "archive arms like delete"
    );

    // N cancels without dispatching; the browser resumes normal operation.
    let _ = update(&mut model, Message::Input(key_input(Key::Char('n'))));
    assert!(model.browser_archive_confirmation().is_none());
    assert!(
        model.notice.is_none(),
        "cancel clears the armed-action notice"
    );
    assert!(
        !matches!(
            update(&mut model, Message::Input(key_input(Key::Backspace))).as_slice(),
            [UiEffect::Dispatch(UiIntent::ArchiveSession { .. })]
        ),
        "a cancelled archive must never dispatch"
    );

    // Re-arm and confirm with Y.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));

    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::ArchiveSession { session_id, .. })]
                if session_id == "session-other"
        ),
        "Y confirms the armed archive"
    );
    assert!(model.browser_archive_confirmation().is_none());
}

#[test]
fn esc_cancels_an_armed_archive() {
    let mut model = empty_model();
    open_browser(&mut model);
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));

    assert!(model.browser_archive_confirmation().is_none());
    assert!(model.notice.is_none(), "Esc clears the armed-action notice");
    assert!(
        model.browser_open(),
        "Esc only disarms the archive; the browser stays open like delete"
    );
    assert!(!matches!(
        update(&mut model, Message::Input(enter())).as_slice(),
        [UiEffect::Dispatch(UiIntent::ArchiveSession { .. })]
    ));
}

#[test]
fn recent_archive_can_be_undone_with_ctrl_z() {
    let mut model = empty_model();
    open_browser(&mut model);

    // Arm, confirm, and let the commit land.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::ArchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected archive intent, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    // Ctrl+Z reverses the archive with a fresh unarchive intent.
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('z'))));
    assert!(
        matches!(
            effects.as_slice(),
            [UiEffect::Dispatch(UiIntent::UnarchiveSession { session_id, .. })]
                if session_id == "session-other"
        ),
        "undo must dispatch the exact inverse intent"
    );
}

#[test]
fn unarchive_can_also_be_undone_back_to_archived() {
    let mut model = empty_model();
    open_browser(&mut model);

    // Arm, confirm an archive, commit, then simulate the projected archived
    // state and unarchive it.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::ArchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected archive intent, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    let archived_list = Arc::new(SessionsProjection {
        sessions: vec![
            SessionBrowserEntry {
                session_id: "session-active".to_owned(),
                title: "Active session".to_owned(),
                archived: false,
                selected_model: Some(pro_model()),
                message_count: 2,
                updated_at_ms: 2,
                active: true,
            },
            SessionBrowserEntry {
                session_id: "session-other".to_owned(),
                title: "Other session".to_owned(),
                archived: true,
                selected_model: None,
                message_count: 0,
                updated_at_ms: 1,
                active: false,
            },
        ],
    });
    let _ = update(&mut model, Message::SessionsChanged(archived_list));

    // Unarchive the now-archived row.
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let request_id = match effects.as_slice() {
        // Unarchive is immediate (non-destructive direction), no arming.
        [UiEffect::Dispatch(UiIntent::UnarchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected immediate unarchive, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    // Undo dispatches the archive back.
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('z'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ArchiveSession { session_id, .. })]
            if session_id == "session-other"
    ));
}

#[test]
fn undo_without_history_is_inert() {
    let mut model = empty_model();
    open_browser(&mut model);

    assert!(update(&mut model, Message::Input(ctrl(Key::Char('z')))).is_empty());
    assert!(
        !matches!(model.notice, Some(Notice::Failure(_))),
        "an inert undo is not an error"
    );
}

#[test]
fn undo_is_consumed_after_one_use() {
    let mut model = empty_model();
    open_browser(&mut model);

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::ArchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected archive intent, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    assert!(matches!(
        update(&mut model, Message::Input(ctrl(Key::Char('z')))).as_slice(),
        [UiEffect::Dispatch(UiIntent::UnarchiveSession { .. })]
    ));
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Char('z')))).is_empty(),
        "one undo per committed action"
    );
}

#[test]
fn a_new_archive_supersedes_the_undoable_one() {
    let mut model = empty_model();
    open_browser(&mut model);

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::ArchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected archive intent, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    // A second archive replaces the first in the undo slot.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('a'))));
    let effects = update(&mut model, Message::Input(key_input(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::ArchiveSession { request_id, .. })] => *request_id,
        other => panic!("expected second archive intent, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );

    assert!(matches!(
        update(&mut model, Message::Input(ctrl(Key::Char('z')))).as_slice(),
        [UiEffect::Dispatch(UiIntent::UnarchiveSession { .. })]
    ));
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Char('z')))).is_empty(),
        "only the newest action stays undoable"
    );
}
