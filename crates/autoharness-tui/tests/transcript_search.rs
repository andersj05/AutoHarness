use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, Focus, Message, Model, ModelSummary,
    SessionProjection, SessionsProjection, TranscriptItem, UsageView, update,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
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

fn session(revision: u64, transcript: Vec<TranscriptItem>) -> Arc<SessionProjection> {
    Arc::new(SessionProjection {
        session_id: "session-fixture".to_owned(),
        revision,
        selected_model: Some(pro_model()),
        transcript,
        permission_requests: Vec::new(),
    })
}

fn searchable_transcript() -> Vec<TranscriptItem> {
    (0..12)
        .map(|index| {
            if index % 3 == 0 {
                TranscriptItem::User {
                    input_id: format!("input-{index}"),
                    text: format!("question {index} about deployment"),
                }
            } else {
                TranscriptItem::Assistant {
                    attempt_id: AttemptKey::new(format!("attempt-{index}")).expect("valid attempt"),
                    text: format!("answer {index}: the deployment pipeline runs nightly."),
                    status: AttemptStatus::Completed,
                    usage: Some(UsageView {
                        input_tokens: 10,
                        output_tokens: 20,
                    }),
                    retry_of: None,
                }
            }
        })
        .collect()
}

fn model_with_history() -> Model {
    Model::new(
        session(9, searchable_transcript()),
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
fn ctrl_f_opens_a_search_bar_that_owns_the_keyboard() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));

    assert!(model.search_open());
    assert_eq!(model.focus, Focus::Search);

    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("Search:"));

    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.search_open());
    assert_eq!(model.focus, Focus::Composer);
    assert!(!buffer_text(&render_model(&model, 80, 24)).contains("Search:"));
}

#[test]
fn typing_a_query_reports_live_match_counts() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "deployment");

    assert!(model.search_match_count() > 0);
    assert!(
        model
            .search_status_label()
            .contains(&model.search_match_count().to_string()),
        "the label must include the match count"
    );

    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("/deployment"));
    assert!(rendered.contains("matches"));

    // A query with no matches says so explicitly.
    type_text(&mut model, "zzznotfound");
    assert_eq!(model.search_match_count(), 0);
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("no matches"));
}

#[test]
fn enter_jumps_to_matches_and_wraps_around() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "nightly");

    let count = model.search_match_count();
    assert!(count >= 4, "fixture must have several matches");

    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    let first = model.search_current_index();
    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    let second = model.search_current_index();
    assert_ne!(first, second, "Enter advances through matches");

    for _ in 1..count {
        let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    }
    assert_eq!(
        model.search_current_index(),
        first,
        "the walk wraps around to the starting match"
    );
}

#[test]
fn shift_enter_walks_backwards_through_matches() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "nightly");
    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    let after_first = model.search_current_index();

    let _ = update(
        &mut model,
        Message::Input(Input {
            key: Key::Tab,
            shift: true,
            ..key_input(Key::Tab)
        }),
    );
    assert_eq!(
        model.search_current_index(),
        (after_first + model.search_match_count() - 1) % model.search_match_count(),
        "backwards stepping wraps modulo the match count"
    );
}

#[test]
fn an_active_search_scrolls_the_matching_row_into_view() {
    let mut model = model_with_history();

    // At tail-follow, the earliest content is off-screen at this size.
    let before = buffer_text(&render_model(&model, 60, 14));
    assert!(!before.contains("question 0"));

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "question 0 about");

    let after = buffer_text(&render_model(&model, 60, 14));
    assert!(
        after.contains("question 0"),
        "the first match must be scrolled into view"
    );
}

#[test]
fn esc_clears_the_query_and_returns_to_tail_follow() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "question 0 about");
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));

    assert!(!model.search_open());
    assert_eq!(model.search_query(), "");
    assert_eq!(model.search_match_count(), 0);
    assert!(
        model.transcript.follow_tail
            || !buffer_text(&render_model(&model, 60, 14)).contains("question 0")
    );
}

#[test]
fn search_is_case_insensitive_and_safe_for_control_characters() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "DEPLOYMENT");

    assert!(model.search_match_count() > 0, "matching ignores case");

    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "a\u{1b}[31mb");
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(
        !rendered.contains('\u{1b}'),
        "queries are escaped on render"
    );
}

#[test]
fn global_chords_stay_global_while_searching() {
    let mut model = model_with_history();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('f'))));
    type_text(&mut model, "query in progress");

    let effects = update(&mut model, Message::Input(ctrl(Key::Char('n'))));
    assert!(
        matches!(
            effects.as_slice(),
            [autoharness_tui::UiEffect::Dispatch(
                autoharness_tui::UiIntent::CreateSession { .. }
            )]
        ),
        "Ctrl+N stays reachable mid-search"
    );
    assert!(!model.search_open(), "global action closes the modal slot");

    let effects = update(&mut model, Message::Input(ctrl(Key::Char('c'))));
    assert!(matches!(
        effects.as_slice(),
        [autoharness_tui::UiEffect::Quit]
    ));
}
