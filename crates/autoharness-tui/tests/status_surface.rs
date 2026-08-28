use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, CredentialSourceLabel, Message, Model, ModelSummary, ProviderKindLabel,
    ProviderStatusProjection, RetryPolicy, SessionProjection, SessionsProjection,
    SettingsProjection, TranscriptItem, UiClock, UiFailure, style_snapshot, update,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

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

fn empty_model(settings: SettingsProjection) -> Model {
    let mut model = Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );
    model.apply_settings(Arc::new(settings));
    model
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
fn chat_surface_uses_compact_transparent_composer_metadata() {
    let model = empty_model(SettingsProjection::default());
    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("auto"));
    assert!(rendered.contains("ctx 0%"));
    assert!(!rendered.contains("auto ○○○○○○"));
    assert!(!rendered.contains("model:"));
    assert!(!rendered.contains("think:"));
    assert!(!rendered.contains("path:"));
    assert!(rendered.contains("Settings"));
    assert!(!rendered.contains("AutoHarness  |"));
    assert!(!rendered.contains("state:ready"));
}

#[test]
fn chat_rail_transcript_and_composer_preserve_terminal_background() {
    let model = empty_model(SettingsProjection::default());
    let rendered = render_model(&model, 120, 40);
    for (column, row, surface) in [
        (5, 30, "rail"),
        (80, 20, "transcript"),
        (110, 38, "composer"),
    ] {
        assert_eq!(
            rendered.buffer()[(column, row)].bg,
            ratatui::style::Color::Reset,
            "{surface} must inherit the terminal background"
        );
    }
}

#[test]
fn chat_surface_omits_provider_status_chrome() {
    let model = empty_model(SettingsProjection::default());
    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(!rendered.contains("gemini (default)"));
    assert!(!rendered.contains("session only"));
    assert!(rendered.contains("auto"));
    assert!(rendered.contains("ctx 0%"));
    assert!(!rendered.contains("auto ○○○○○○"));
    assert!(!rendered.contains(" │ ."));
}

#[test]
fn chat_surface_omits_credential_status_labels() {
    let model = empty_model(SettingsProjection {
        provider_status: ProviderStatusProjection {
            active_profile: Some("home-router".to_owned()),
            provider_kind: Some(ProviderKindLabel::Router),
            credential_source: CredentialSourceLabel::CredentialVault,
            credential_connected: false,
        },
        ..SettingsProjection::default()
    });
    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(!rendered.contains("credential vault"));
    assert!(!rendered.contains("disconnected"));
}

#[test]
fn chat_surface_omits_catalog_status_chrome() {
    let mut model = empty_model(SettingsProjection::default());
    let stale = match &*catalog_ready() {
        CatalogProjection::Ready { models, .. } => Arc::new(CatalogProjection::Ready {
            models: models.clone(),
            stale: true,
        }),
        _ => unreachable!(),
    };
    let _ = update(&mut model, Message::CatalogChanged(stale));
    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(!rendered.contains("stale"));
}

#[test]
fn aggregate_usage_appears_in_the_status_surface_after_turns() {
    use autoharness_tui::{AttemptKey, AttemptStatus, UsageView};
    let transcript = vec![TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-1").expect("valid attempt"),
        text: "answer".to_owned(),
        status: AttemptStatus::Completed,
        usage: Some(UsageView {
            input_tokens: 120,
            output_tokens: 340,
        }),
        retry_of: None,
    }];
    let model = Model::new(
        session(2, transcript),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(rendered.contains("120 in"));
    assert!(rendered.contains("340 out"));
}

#[test]
fn narrow_chat_keeps_prompt_metadata_without_status_header() {
    let model = empty_model(SettingsProjection::default());

    let rendered = buffer_text(&render_model(&model, 40, 12));
    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("auto"));
    assert!(!rendered.contains("auto ○○○○○○"));
    assert!(rendered.contains("0%"));
    assert!(!rendered.contains(" │ ."));
    assert!(!rendered.contains("AutoHarness  |"));

    let tiny = buffer_text(&render_model(&model, 24, 7));
    assert!(!tiny.contains("1 Chat"));
    assert!(!tiny.contains("state:ready"));
}

#[test]
fn prompt_is_the_only_content_in_a_new_conversation() {
    let model = empty_model(SettingsProjection::default());
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains('❯'));
    assert!(!rendered.contains("New conversation"));
    assert!(!rendered.contains("Connect a provider key"));
    assert!(!rendered.contains("Conversation"));
}

#[test]
fn active_generation_uses_a_tick_driven_ascii_scanner() {
    use autoharness_tui::{AttemptKey, AttemptStatus};
    let transcript = vec![TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-generating").expect("valid attempt"),
        text: String::new(),
        status: AttemptStatus::Streaming,
        usage: None,
        retry_of: None,
    }];
    let mut model = Model::new(
        session(2, transcript),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let first_backend = render_model(&model, 80, 24);
    let first = buffer_text(&first_backend);
    assert!(first.contains("generating") || first.contains("[==>-----]"));
    let first_style = style_snapshot(first_backend.buffer());
    let _ = update(&mut model, Message::Tick(UiClock::new(700, 0)));
    let later_style = style_snapshot(render_model(&model, 80, 24).buffer());
    assert_ne!(first_style, later_style);
}

#[test]
fn failed_attempt_state_is_visible_in_the_status_surface() {
    use autoharness_tui::{AttemptKey, AttemptStatus};
    let transcript = vec![TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-9").expect("valid attempt"),
        text: String::new(),
        status: AttemptStatus::Failed(UiFailure::new(
            autoharness_domain::ErrorClass::Unavailable,
            "provider unavailable",
            RetryPolicy::Never,
        )),
        usage: None,
        retry_of: None,
    }];
    let model = Model::new(
        session(2, transcript),
        Arc::new(SessionsProjection::default()),
        catalog_ready(),
    );

    let header = buffer_text(&render_model(&model, 120, 40));
    assert!(header.contains("failed"));
}
