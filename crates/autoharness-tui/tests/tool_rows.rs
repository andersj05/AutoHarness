use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, Message, Model, ModelSummary, SessionProjection,
    SessionsProjection, ToolCallKey, ToolRowView, TranscriptItem, UsageView, update,
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

fn tool_row(status: &str) -> TranscriptItem {
    TranscriptItem::Tool(ToolRowView {
        tool_call_id: ToolCallKey::new("tool-call-1").expect("tool-call ID"),
        tool_name: "fs_read".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        status: status.to_owned(),
        summary: Some("27 bytes read".to_owned()),
    })
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
fn tool_rows_render_one_collapsed_line_with_status_and_summary() {
    let model = Model::new(
        session(
            3,
            vec![
                TranscriptItem::User {
                    input_id: "input-1".to_owned(),
                    text: "read the file".to_owned(),
                },
                tool_row("completed"),
            ],
        ),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(
        rendered.contains("fs_read"),
        "the collapsed row names the tool"
    );
    assert!(rendered.contains("completed"), "the settled status shows");
    assert!(rendered.contains("27 bytes"), "the one-line summary shows");
    assert!(!rendered.contains("workspace:src/lib.rs"));
}

#[test]
fn expanding_reveals_the_resource_and_collapse_hides_it_again() {
    let mut model = Model::new(
        session(3, vec![tool_row("completed")]),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('x'))));
    assert!(model.tools_expanded(), "Ctrl+X expands all tool rows");

    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(rendered.contains("workspace:src/lib.rs"));

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('x'))));
    assert!(!model.tools_expanded());

    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(!rendered.contains("workspace:src/lib.rs"));
}

#[test]
fn pending_and_denied_tool_rows_use_distinct_visible_states() {
    let model = Model::new(
        session(4, vec![tool_row("denied by policy"), tool_row("running")]),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(rendered.contains("denied"));
    assert!(rendered.contains("running"));
}

#[test]
fn retry_lineage_and_recovery_stay_visible_alongside_tool_rows() {
    let with_failure = vec![
        TranscriptItem::User {
            input_id: "input-1".to_owned(),
            text: "go".to_owned(),
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-x").expect("valid attempt"),
            text: String::new(),
            status: AttemptStatus::Failed(autoharness_tui::UiFailure::new(
                autoharness_domain::ErrorClass::RateLimited,
                "slow down",
                autoharness_tui::RetryPolicy::Now,
            )),
            usage: None,
            retry_of: None,
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-y").expect("valid attempt"),
            text: "done after retry".to_owned(),
            status: AttemptStatus::Completed,
            usage: Some(UsageView {
                input_tokens: 1,
                output_tokens: 1,
            }),
            retry_of: Some(AttemptKey::new("attempt-x").expect("valid attempt")),
        },
    ];
    let model = Model::new(
        session(6, with_failure),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );
    let rendered = buffer_text(&render_model(&model, 100, 30));
    assert!(rendered.contains("retry"), "retry lineage stays visible");
    assert!(rendered.contains("done after retry"));
}
