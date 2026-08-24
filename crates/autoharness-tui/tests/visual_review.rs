// Visual review harness: renders the golden snapshot model plus each new
// overlay at the plan's review sizes and prints them for inspection.
use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, Message, Model, ModelSummary,
    PermissionDetailView, PermissionRequestView, RetryPolicy, SessionProjection,
    SessionsProjection, SettingsProjection, ToolCallKey, ToolRowView, TranscriptItem, UiFailure,
    UiNotice, UsageView, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

fn model_ref(id: &str) -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider"),
        ModelId::new(id).expect("model"),
    )
}

fn pro_model() -> ModelRef {
    model_ref("models/gemini-2.5-pro")
}

fn ready_catalog() -> Arc<CatalogProjection> {
    Arc::new(CatalogProjection::Ready {
        models: vec![
            ModelSummary {
                model: pro_model(),
                display_name: "Gemini 2.5 Pro".to_owned(),
                detail: "reasoning | text".to_owned(),
                selectable: true,
            },
            ModelSummary {
                model: model_ref("models/gemini-2.5-flash"),
                display_name: "Gemini 2.5 Flash".to_owned(),
                detail: "fast".to_owned(),
                selectable: true,
            },
        ],
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

fn snapshot_model() -> Model {
    let failure = UiFailure::new(
        ErrorClass::RateLimited,
        "Capacity is temporarily exhausted; no credentials were exposed.",
        RetryPolicy::Now,
    );
    let transcript = vec![
        TranscriptItem::User {
            input_id: "input-1".to_owned(),
            text: "Plan a cafe launch.\nKeep it practical.".to_owned(),
        },
        TranscriptItem::Tool(ToolRowView {
            tool_call_id: ToolCallKey::new("tool-call-1").expect("tool call"),
            tool_name: "fs_read".to_owned(),
            resource: "workspace:menu.md".to_owned(),
            status: "completed".to_owned(),
            summary: Some("27 bytes read".to_owned()),
        }),
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-1").expect("attempt"),
            text: "Start with a two-week validation sprint.".to_owned(),
            status: AttemptStatus::Completed,
            usage: Some(UsageView {
                input_tokens: 18,
                output_tokens: 41,
            }),
            retry_of: None,
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-2").expect("attempt"),
            text: "partial".to_owned(),
            status: AttemptStatus::Failed(failure),
            usage: None,
            retry_of: Some(AttemptKey::new("attempt-1").expect("attempt")),
        },
    ];
    let mut model = Model::new(
        session(8, transcript),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let _ = update(
        &mut model,
        Message::Paste("Retry with smaller scope.".to_owned()),
    );
    let _ = update(&mut model, Message::Tick(1_400));
    model
}

fn render(model: &Model, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(frame, model)).expect("draw");
    let backend = terminal.backend().clone();
    let area = backend.buffer().area;
    let mut out = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        for x in area.x..area.right() {
            line.push_str(backend.buffer().cell((x, y)).expect("cell").symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn key(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn ctrl(c: char) -> Input {
    Input {
        key: Key::Char(c),
        ctrl: true,
        alt: false,
        shift: false,
    }
}

fn apply_presentation(model: &mut Model, preferences: &str) {
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, preferences)
        .resolve()
        .expect("visual review preferences");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: settings.local_profile().clone(),
        ..SettingsProjection::default()
    }));
}

#[test]
#[ignore = "accessibility visual review harness; run with --ignored --nocapture"]
fn render_accessibility_review_matrix() {
    let modes = [
        (
            "no-color ASCII compact single-column",
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "color_mode": "no_color",
                        "glyph_mode": "ascii",
                        "reduced_motion": true,
                        "density": "compact",
                        "layout": "single_column"
                    }
                }
            }"#,
        ),
        (
            "high-contrast ASCII compact single-column",
            r#"{
                "schema_version": 3,
                "local_profile": {
                    "preferences": {
                        "color_mode": "high_contrast",
                        "glyph_mode": "ascii",
                        "reduced_motion": true,
                        "density": "compact",
                        "layout": "single_column"
                    }
                }
            }"#,
        ),
    ];
    for (name, preferences) in modes {
        for (width, height) in [(120u16, 50u16), (120, 40), (80, 24), (60, 18), (40, 12)] {
            let mut model = snapshot_model();
            apply_presentation(&mut model, preferences);
            let _ = update(&mut model, Message::Input(ctrl(',')));
            println!(
                "=== {name} Settings {width}x{height} ===\n{}",
                render(&model, width, height)
            );
        }
    }
    let mut permission = (*session(9, Vec::new())).clone();
    permission.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("visual-permission").expect("tool call"),
        tool_name: "fs_write".to_owned(),
        capability: "filesystem write".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: vec![PermissionDetailView {
            label: "Path".to_owned(),
            value: "src/lib.rs".to_owned(),
        }],
    });
    for (name, preferences) in modes {
        for (width, height) in [(120u16, 50u16), (120, 40), (80, 24), (60, 18), (40, 12)] {
            let mut model = Model::new(
                Arc::new(permission.clone()),
                Arc::new(SessionsProjection::default()),
                ready_catalog(),
            );
            apply_presentation(&mut model, preferences);
            println!(
                "=== {name} Permission {width}x{height} ===\n{}",
                render(&model, width, height)
            );
        }
    }
}

#[test]
#[ignore = "visual review harness; run with --ignored"]
fn render_review_screens() {
    // Main surface at every reviewed size.
    for (w, h) in [(120u16, 50u16), (120, 40), (80, 24), (60, 18), (40, 12)] {
        println!("=== main {w}x{h} ===\n{}", render(&snapshot_model(), w, h));
    }

    // Command palette.
    let mut m = snapshot_model();
    let _ = update(&mut m, Message::Input(ctrl('/')));
    println!("=== palette 80x24 ===\n{}", render(&m, 80, 24));

    // Help overlay from composer.
    let mut m = snapshot_model();
    let _ = update(
        &mut m,
        Message::Input(ratatui_textarea::Input {
            key: Key::F(1),
            ctrl: false,
            alt: false,
            shift: false,
        }),
    );
    println!("=== help 80x24 ===\n{}", render(&m, 80, 24));

    // Search bar with matches.
    let mut m = snapshot_model();
    let _ = update(&mut m, Message::Input(ctrl('f')));
    for c in "sprint".chars() {
        let _ = update(&mut m, Message::Input(key(Key::Char(c))));
    }
    let _ = update(&mut m, Message::Input(key(Key::Enter)));
    println!("=== search 80x24 ===\n{}", render(&m, 80, 24));

    // Expanded tools + undo notice.
    let mut m = snapshot_model();
    let _ = update(&mut m, Message::Input(ctrl('a')));
    let _ = update(
        &mut m,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: autoharness_tui::RequestId::new(1),
        }),
    );
    let _ = update(&mut m, Message::Input(ctrl('x')));
    println!("=== expanded tools 100x30 ===\n{}", render(&m, 100, 30));
}
