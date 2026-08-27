use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, Message, Model, ModelSummary, Notice, SessionProjection, SessionsProjection,
    UiEffect, UiIntent, update,
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

/// Submits the composer text and acknowledges the commit, mirroring the
/// runner and coordinator.
fn submit_and_commit(model: &mut Model, prompt: &str) {
    type_text(model, prompt);
    let effects = update(model, Message::Input(ctrl(Key::Char('s'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::SubmitPrompt { request_id, .. })] => *request_id,
        other => panic!("expected submit effect, got {other:?}"),
    };
    let _ = update(
        model,
        Message::Notice(autoharness_tui::UiNotice::IntentCommitted { request_id }),
    );
    assert!(model.composer.is_blank(), "committed prompts clear");
}

#[test]
fn ctrl_up_recalls_the_last_prompt_after_commit() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "first question");

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));

    assert_eq!(model.composer.text(), "first question");
}

#[test]
fn history_walks_backwards_and_forwards_without_dropping_the_draft() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "oldest");
    submit_and_commit(&mut model, "middle");
    submit_and_commit(&mut model, "newest");
    type_text(&mut model, "draft below");

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(model.composer.text(), "middle");
    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(model.composer.text(), "oldest");
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Up))).is_empty(),
        "history saturates at the oldest entry"
    );
    assert_eq!(model.composer.text(), "oldest");

    let _ = update(&mut model, Message::Input(ctrl(Key::Down)));
    assert_eq!(model.composer.text(), "middle");
    let _ = update(&mut model, Message::Input(ctrl(Key::Down)));
    assert_eq!(model.composer.text(), "newest");
    let _ = update(&mut model, Message::Input(ctrl(Key::Down)));
    assert_eq!(
        model.composer.text(),
        "draft below",
        "leaving history restores the in-progress draft"
    );
}

#[test]
fn editing_a_recalled_prompt_resets_the_walk() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "one");
    submit_and_commit(&mut model, "two");

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(model.composer.text(), "two");
    type_text(&mut model, "!");
    assert_eq!(model.composer.text(), "two!");

    // The walk position reset: Ctrl+Down returns to the draft slot, not
    // deeper history.
    let _ = update(&mut model, Message::Input(ctrl(Key::Down)));
    assert_eq!(model.composer.text(), "two!");
}

#[test]
fn committed_prompts_are_deduplicated_consecutively() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "repeat");
    submit_and_commit(&mut model, "repeat");
    submit_and_commit(&mut model, "repeat");

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(model.composer.text(), "repeat");
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Up))).is_empty(),
        "consecutive duplicates collapse to one history entry"
    );
}

#[test]
fn history_is_per_run_and_survives_session_switches() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "carried question");

    // Simulate switching sessions through the projection.
    let replacement = Arc::new(SessionProjection {
        session_id: "session-replacement".to_owned(),
        revision: 1,
        selected_model: Some(pro_model()),
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    });
    let _ = update(&mut model, Message::SessionChanged(replacement));
    assert!(model.composer.is_blank());

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(
        model.composer.text(),
        "carried question",
        "run history is not scoped to one session"
    );
}

#[test]
fn rejected_prompts_do_not_enter_history() {
    let mut model = empty_model();
    type_text(&mut model, "doomed prompt");
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('s'))));
    let request_id = match effects.as_slice() {
        [UiEffect::Dispatch(UiIntent::SubmitPrompt { request_id, .. })] => *request_id,
        other => panic!("expected submit effect, got {other:?}"),
    };
    let _ = update(
        &mut model,
        Message::Notice(autoharness_tui::UiNotice::IntentRejected {
            request_id,
            failure: autoharness_tui::UiFailure::new(
                autoharness_domain::ErrorClass::Unavailable,
                "busy",
                autoharness_tui::RetryPolicy::Now,
            ),
        }),
    );

    // The draft survives a rejection; nothing was committed, so nothing is
    // in history. Recall with an empty history must not fire.
    assert_eq!(model.composer.text(), "doomed prompt");
    assert!(update(&mut model, Message::Input(ctrl(Key::Up))).is_empty());
    assert_eq!(model.composer.text(), "doomed prompt");
}

#[test]
fn recall_with_empty_history_is_inert_and_quiet() {
    let mut model = empty_model();

    assert!(update(&mut model, Message::Input(ctrl(Key::Up))).is_empty());
    assert!(update(&mut model, Message::Input(ctrl(Key::Down))).is_empty());
    assert!(model.composer.is_blank());
    assert!(
        !matches!(model.notice, Some(Notice::Failure(_))),
        "an inert recall is not an error"
    );
}

#[test]
fn history_recall_does_not_fight_multiline_drafts() {
    let mut model = empty_model();
    submit_and_commit(&mut model, "past line");

    type_text(&mut model, "current");
    let _ = update(&mut model, Message::Input(enter()));
    type_text(&mut model, "draft");

    let _ = update(&mut model, Message::Input(ctrl(Key::Up)));
    assert_eq!(model.composer.text(), "past line");

    let _ = update(&mut model, Message::Input(ctrl(Key::Down)));
    assert_eq!(
        model.composer.text(),
        "current\ndraft",
        "the multiline draft is restored exactly"
    );
}
