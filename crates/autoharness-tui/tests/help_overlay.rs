use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, Focus, Message, Model, ModelSummary, SessionProjection, SessionsProjection,
    update,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_textarea::{Input, Key};

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

fn pro_model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider id"),
        ModelId::new("models/gemini-2.5-pro").expect("model id"),
    )
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

fn f1() -> Input {
    key_input(Key::F(1))
}

fn enter() -> Input {
    key_input(Key::Enter)
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
fn f1_opens_modal_help_showing_global_and_composer_keys() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(f1()));

    assert!(model.help_open());
    assert_eq!(
        model.focus,
        Focus::Help,
        "the help overlay owns the keyboard"
    );

    let rendered = buffer_text(&render_model(&model, 120, 50));
    for expected in [
        "Help", "Global", "Composer", "Ctrl+S", "Ctrl+N", "Ctrl+L", "Ctrl+P", "Ctrl+F", "Ctrl+X",
        "Ctrl+Y", "Ctrl+Z", "Esc",
    ] {
        assert!(rendered.contains(expected), "help must mention {expected}");
    }

    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.help_open());
    assert_eq!(model.focus, Focus::Composer);
}

#[test]
fn help_content_names_the_focused_surface() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(f1()));
    let composer_help = buffer_text(&render_model(&model, 80, 24));
    assert!(
        composer_help.contains("Composer"),
        "composer focus must be named"
    );
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));

    // Open the session browser, then request help from inside it.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('l'))));
    let _ = update(&mut model, Message::Input(f1()));
    let browser_help = buffer_text(&render_model(&model, 80, 24));

    assert!(browser_help.contains("Browser"), "browser must be named");
    assert!(
        browser_help.contains("Ctrl+R"),
        "browser-only keys must appear"
    );

    // Esc returns to where the keyboard came from, not blindly to the composer.
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert_eq!(model.focus, Focus::Browser);
    assert!(model.browser_open());
}

#[test]
fn help_content_names_settings_navigation_and_reset_actions() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('4'))));
    let _ = update(&mut model, Message::Input(f1()));
    let rendered = buffer_text(&render_model(&model, 80, 24));
    for expected in ["Settings", "PageUp/PageDown", "Home/End", "R", "D"] {
        assert!(
            rendered.contains(expected),
            "settings help must mention {expected}"
        );
    }
}

#[test]
fn slash_help_and_the_palette_row_both_open_the_overlay() {
    let mut model = empty_model();
    type_text(&mut model, "/help");
    let _ = update(&mut model, Message::Input(enter()));

    assert!(model.help_open());
    assert!(model.composer.is_blank(), "executed commands clear");
    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert!(!model.help_open());

    // Through the palette table as well.
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('/'))));
    type_text(&mut model, "help");
    let _ = update(&mut model, Message::Input(enter()));
    assert!(model.help_open());
}

#[test]
fn help_scrolls_through_longer_content() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(f1()));

    assert_eq!(model.help_scroll(), 0);
    let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    assert_eq!(model.help_scroll(), 2);
    let scrolled = buffer_text(&render_model(&model, 40, 12));
    assert!(scrolled.contains("Help"));
    let _ = update(&mut model, Message::Input(key_input(Key::Up)));
    assert_eq!(model.help_scroll(), 1);

    // Scrolling saturates instead of underflowing.
    for _ in 0..10 {
        let _ = update(&mut model, Message::Input(key_input(Key::Up)));
    }
    assert_eq!(model.help_scroll(), 0);
}

#[test]
fn prompt_has_no_footer_text_below_the_composer() {
    let model = empty_model();
    for (width, height) in [(120, 24), (80, 24), (40, 12)] {
        let rendered = buffer_text(&render_model(&model, width, height));
        let bottom = rendered.lines().last().unwrap_or_default();
        assert!(!bottom.contains("send"));
        assert!(!bottom.contains("newline"));
        assert!(!bottom.contains("models"));
        assert!(!bottom.contains("sessions"));
    }
}

#[test]
fn f1_is_ignored_while_a_permission_decision_owns_the_keyboard() {
    use autoharness_tui::{PermissionDetailView, PermissionRequestView, ToolCallKey};
    let mut projection = (*session()).clone();
    projection.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("tool-call-help").expect("tool-call ID"),
        tool_name: "fs_write".to_owned(),
        capability: "filesystem write".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: vec![PermissionDetailView {
            label: "Path".to_owned(),
            value: "src/lib.rs".to_owned(),
        }],
    });
    let mut model = Model::new(
        Arc::new(projection),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );
    assert_eq!(model.focus, Focus::Permission);

    let _ = update(&mut model, Message::Input(f1()));

    assert!(!model.help_open());
    assert_eq!(
        model.focus,
        Focus::Permission,
        "decision keeps exclusive keys"
    );
}

#[test]
fn ctrl_c_quits_from_inside_help() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(f1()));
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('c'))));

    assert!(matches!(
        effects.as_slice(),
        [autoharness_tui::UiEffect::Quit]
    ));
    assert!(model.should_quit);
}

#[test]
fn enter_is_inert_inside_help_so_drafts_survive() {
    let mut model = empty_model();
    type_text(&mut model, "draft stays");
    let _ = update(&mut model, Message::Input(f1()));
    let _ = update(&mut model, Message::Input(enter()));
    let _ = update(&mut model, Message::Input(key_input(Key::Char('x'))));

    let _ = update(&mut model, Message::Input(key_input(Key::Esc)));
    assert_eq!(
        model.composer.text(),
        "draft stays",
        "help must never touch the composer buffer"
    );
}

#[test]
fn help_renders_across_sizes_without_panicking() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(f1()));
    for (width, height) in [(120, 50), (80, 24), (60, 18), (40, 12), (24, 7), (10, 4)] {
        let backend = render_model(&model, width, height);
        assert_eq!(backend.buffer().area.width, width);
        assert_eq!(backend.buffer().area.height, height);
    }
}
