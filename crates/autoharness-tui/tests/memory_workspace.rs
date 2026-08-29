use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    CatalogProjection, Focus, MemoryAdmission, MemoryDetail, MemoryPane, MemoryProjection,
    MemoryScope, MemoryStatus, MemoryStatusFilter, MemorySummary, MemoryTrust, Message, Model,
    ModelSummary, MouseAction, Route, SessionProjection, SessionsProjection, SettingsProjection,
    hit_test, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui_textarea::{Input, Key};

fn model_ref() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider"),
        ModelId::new("models/gemini-memory").expect("model"),
    )
}

fn model() -> Model {
    let model_ref = model_ref();
    let mut model = Model::new(
        Arc::new(SessionProjection {
            session_id: "memory-session".to_owned(),
            revision: 1,
            selected_model: Some(model_ref.clone()),
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::Ready {
            models: vec![ModelSummary {
                model: model_ref,
                display_name: "Gemini Memory".to_owned(),
                detail: String::new(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            }],
            stale: false,
        }),
    );
    model.apply_memory(Arc::new(memory_projection()));
    model
}

fn memory_projection() -> MemoryProjection {
    let summaries = vec![
        MemorySummary::new(
            "memory-concise",
            "Prefer concise implementation notes with concrete verification.",
            MemoryStatus::Active,
            MemoryScope::Workspace,
            1_725_000_000_000,
            Some(9_200),
            2,
        )
        .expect("summary"),
        MemorySummary::new(
            "memory-keyboard",
            "Keyboard-first navigation is preferred for terminal workflows.",
            MemoryStatus::Proposed,
            MemoryScope::User,
            1_724_000_000_000,
            Some(7_500),
            1,
        )
        .expect("summary"),
        MemorySummary::new(
            "memory-old-theme",
            "A superseded visual preference retained for audit history.",
            MemoryStatus::Superseded,
            MemoryScope::Session,
            1_700_000_000_000,
            None,
            0,
        )
        .expect("summary"),
    ];
    let admissions = vec![
        MemoryAdmission::new(
            "session-launch",
            "gemini-memory",
            "Matched the active workspace and ranked above session notes.",
            1_725_100_000_000,
            1,
        )
        .expect("admission"),
        MemoryAdmission::new(
            "session-review",
            "gemini-memory",
            "Reused while preparing a concise validation report.",
            1_725_200_000_000,
            2,
        )
        .expect("admission"),
    ];
    let details = vec![
        MemoryDetail::new(
            "memory-concise",
            3,
            "Prefer concise implementation notes, lead with outcomes, and include concrete test evidence.",
            "workspace instruction",
            MemoryTrust::VerifiedObservation,
            1_720_000_000_000,
            None,
            admissions,
        )
        .expect("detail"),
        MemoryDetail::new(
            "memory-keyboard",
            1,
            "Use keyboard-first navigation for terminal workflows while keeping mouse targets available.",
            "explicit user preference",
            MemoryTrust::UserApproved,
            1_724_000_000_000,
            None,
            vec![
                MemoryAdmission::new(
                    "session-setup",
                    "gemini-memory",
                    "Matched an interaction-design request.",
                    1_724_100_000_000,
                    1,
                )
                .expect("admission"),
            ],
        )
        .expect("detail"),
    ];
    MemoryProjection::ready(7, summaries, details, 3, false).expect("projection")
}

fn key(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
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

fn render(model: &Model, width: u16, height: u16) -> TestBackend {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(frame, model)).expect("draw");
    terminal.backend().clone()
}

fn text(backend: &TestBackend) -> String {
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
fn alt_six_and_slash_memory_share_the_read_only_route() {
    let mut shortcut = model();
    let effects = update(&mut shortcut, Message::Input(alt('6')));
    assert!(effects.is_empty());
    assert_eq!(shortcut.route(), Route::Memory);
    assert_eq!(shortcut.focus, Focus::Memory);

    let mut slash = model();
    type_text(&mut slash, "/memory");
    let effects = update(&mut slash, Message::Input(key(Key::Enter)));
    assert!(effects.is_empty());
    assert_eq!(slash.route(), Route::Memory);
    assert!(slash.composer.is_blank());
}

#[test]
fn responsive_memory_page_has_clear_progressive_disclosure() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));

    let wide = text(&render(&model, 120, 50));
    assert!(wide.contains("Memory index"));
    assert!(wide.contains("Revision detail"));
    assert!(!wide.contains("Admission history"));
    assert!(wide.contains("Prefer concise"));
    assert!(wide.contains("workspace instruction"));

    let extra_wide = text(&render(&model, 140, 50));
    assert!(extra_wide.contains("Admission history"));

    let medium = text(&render(&model, 80, 24));
    assert!(medium.contains("Memory index"));
    assert!(medium.contains("Revision detail"));
    assert!(!medium.contains("Admission history"));

    for (width, height) in [(60, 18), (40, 12)] {
        let compact = text(&render(&model, width, height));
        assert!(
            compact.contains("Memory index"),
            "missing index at {width}x{height}"
        );
        assert!(!compact.contains("Revision detail"));
    }

    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    assert_eq!(model.memory_pane(), MemoryPane::Detail);
    for (width, height) in [(60, 18), (40, 12)] {
        let compact = text(&render(&model, width, height));
        assert!(compact.contains("Revision detail"));
        assert!(compact.contains("Prefer concise"));
    }
}

#[test]
fn local_search_filters_selection_and_drill_down_do_not_emit_intents() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Mouse(MouseAction::MemoryCycleStatus));
    let _ = update(&mut model, Message::Input(key(Key::Char('/'))));
    type_text(&mut model, "keyboard");
    assert_eq!(model.memory_query(), "keyboard");
    assert_eq!(model.memory_selection(), Some("memory-keyboard"));

    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    assert!(effects.is_empty());
    assert_eq!(model.memory_pane(), MemoryPane::Detail);
    let effects = update(&mut model, Message::Mouse(MouseAction::MemoryAdmissions));
    assert!(effects.is_empty());
    assert_eq!(model.memory_pane(), MemoryPane::Admissions);

    let _ = update(&mut model, Message::Mouse(MouseAction::MemoryCycleStatus));
    assert_ne!(model.memory_status_filter(), MemoryStatusFilter::Eligible);
}

#[test]
fn memory_controls_have_typed_hit_regions() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    let mut actions = Vec::new();
    for column in 0..80 {
        for row in 0..24 {
            if let Some(action) = hit_test(&model, 80, 24, column, row) {
                actions.push(action);
            }
        }
    }
    assert!(actions.contains(&MouseAction::MemoryFocusSearch));
    assert!(actions.contains(&MouseAction::MemoryCycleStatus));
    assert!(actions.contains(&MouseAction::MemoryCycleScope));
    assert!(actions.contains(&MouseAction::MemoryOpen));
    assert!(
        actions.iter().any(
            |action| matches!(action, MouseAction::MemorySelect(id) if id == "memory-concise")
        )
    );
}

#[test]
fn ascii_and_no_color_memory_view_is_legible_and_terminal_safe() {
    let mut model = model();
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 4,
                "local_profile": { "preferences": {
                    "glyph_mode": "ascii",
                    "color_mode": "no_color"
                } }
            }"#,
        )
        .resolve()
        .expect("accessible settings");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: resolved.local_profile().clone(),
        ..SettingsProjection::default()
    }));
    let _ = update(&mut model, Message::Input(alt('6')));
    let backend = render(&model, 40, 12);
    let rendered = text(&backend);
    assert!(rendered.is_ascii());
    assert!(rendered.contains("Memory"));
    assert!(rendered.contains("State:"));
    assert!(
        backend
            .buffer()
            .content
            .iter()
            .all(|cell| { cell.fg == Color::Reset && cell.bg == Color::Reset })
    );
}

#[test]
fn memory_projection_bounds_and_debug_redaction_are_enforced() {
    assert!(
        MemorySummary::new(
            "x".repeat(129),
            "safe preview",
            MemoryStatus::Active,
            MemoryScope::User,
            0,
            None,
            0,
        )
        .is_err()
    );
    let projection = memory_projection();
    let debug = format!("{projection:?}");
    assert!(!debug.contains("Prefer concise"));
    assert!(!debug.contains("session-launch"));
    assert!(!debug.contains("workspace instruction"));
    assert!(debug.contains("summary_count"));
}

#[test]
fn stale_memory_generations_cannot_roll_the_workspace_back() {
    let mut model = model();
    model.apply_memory(Arc::new(
        MemoryProjection::ready(6, Vec::new(), Vec::new(), 0, false).expect("stale projection"),
    ));
    assert_eq!(model.memory().generation(), 7);
    assert_eq!(model.memory().total(), 3);
    assert_eq!(model.memory_selection(), Some("memory-concise"));
}

#[test]
#[ignore = "visual review harness for the read-only Memory workspace"]
fn render_memory_review_matrix() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    for (width, height) in [(120, 50), (80, 24), (60, 18), (40, 12)] {
        println!(
            "=== Memory {width}x{height} ===\n{}",
            text(&render(&model, width, height))
        );
    }
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    println!(
        "=== Memory detail 40x12 ===\n{}",
        text(&render(&model, 40, 12))
    );
    let _ = update(&mut model, Message::Mouse(MouseAction::MemoryAdmissions));
    println!(
        "=== Memory admissions 60x18 ===\n{}",
        text(&render(&model, 60, 18))
    );
}
