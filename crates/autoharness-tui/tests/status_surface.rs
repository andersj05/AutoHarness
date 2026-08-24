use std::sync::Arc;

use autoharness_domain::{ModelId, ModelRef, ProviderId};
use autoharness_tui::{
    CatalogProjection, CredentialSourceLabel, Message, Model, ModelSummary, ProviderKindLabel,
    ProviderStatusProjection, RetryPolicy, SessionProjection, SessionsProjection,
    SettingsProjection, TranscriptItem, UiFailure, update,
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
fn wide_header_shows_provider_profile_credential_and_session_state() {
    let model = empty_model(SettingsProjection {
        provider_status: ProviderStatusProjection {
            active_profile: Some("home-router".to_owned()),
            provider_kind: Some(ProviderKindLabel::Router),
            credential_source: CredentialSourceLabel::CredentialVault,
            credential_connected: true,
        },
        ..SettingsProjection::default()
    });

    let header = buffer_text(&render_model(&model, 120, 40));

    for expected in [
        "AutoHarness",
        "router via 'home-router'",
        "credential vault",
        "Gemini 2.5 Pro",
        "ready",
    ] {
        assert!(header.contains(expected), "header must show {expected}");
    }
}

#[test]
fn default_launch_reports_gemini_default_and_session_only() {
    let model = empty_model(SettingsProjection::default());

    let header = buffer_text(&render_model(&model, 120, 40));

    assert!(header.contains("gemini (default)"));
    assert!(header.contains("session only"));
}

#[test]
fn disconnected_vault_profile_never_claims_a_connected_credential() {
    let model = empty_model(SettingsProjection {
        provider_status: ProviderStatusProjection {
            active_profile: Some("home-router".to_owned()),
            provider_kind: Some(ProviderKindLabel::Router),
            credential_source: CredentialSourceLabel::CredentialVault,
            credential_connected: false,
        },
        ..SettingsProjection::default()
    });

    let header = buffer_text(&render_model(&model, 120, 40));

    assert!(
        !header.contains("credential vault"),
        "an unconnected vault source must not display as connected"
    );
    assert!(header.contains("disconnected"));
}

#[test]
fn stale_catalog_is_visible_in_the_status_surface() {
    let mut model = empty_model(SettingsProjection::default());
    let stale = match &*catalog_ready() {
        CatalogProjection::Ready { models, .. } => Arc::new(CatalogProjection::Ready {
            models: models.clone(),
            stale: true,
        }),
        _ => unreachable!(),
    };
    let _ = update(&mut model, Message::CatalogChanged(stale));

    let header = buffer_text(&render_model(&model, 120, 40));
    assert!(header.contains("stale"));
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

    let header = buffer_text(&render_model(&model, 120, 40));
    assert!(
        header.contains("460 tok") || header.contains("460 tokens"),
        "aggregate usage must appear, got: {header}"
    );
}

#[test]
fn narrow_header_keeps_the_essential_state_only() {
    let model = empty_model(SettingsProjection::default());

    let header = buffer_text(&render_model(&model, 40, 12));
    assert!(header.contains("AutoHarness"));
    assert!(header.contains("ready"), "work state stays visible");

    let tiny = buffer_text(&render_model(&model, 24, 7));
    assert!(tiny.contains("AutoHarness"));
    assert!(
        !tiny.contains("gemini (default)"),
        "details drop before state"
    );
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
