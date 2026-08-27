use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, Message, Model, ModelSummary, MouseAction,
    Notice, PermissionDetailView, PermissionRequestView, RetryPolicy, SessionBrowserEntry,
    SessionProjection, SessionsProjection, SettingsProjection, ToolCallKey, TranscriptItem,
    UiClock, UiEffect, UiFailure, UiIntent, UiNotice, UsageView, display_safe, hit_test,
    style_snapshot, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui_textarea::{Input, Key};
use zeroize::Zeroizing;

fn model_ref(id: &str) -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("valid provider ID"),
        ModelId::new(id).expect("valid model ID"),
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
                detail: "reasoning · text".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            },
            ModelSummary {
                model: model_ref("models/gemini-2.5-flash"),
                display_name: "Gemini 2.5 Flash".to_owned(),
                detail: "fast · text".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            },
            ModelSummary {
                model: model_ref("models/embedding-001"),
                display_name: "Embedding 001".to_owned(),
                detail: "embedding only".to_owned(),
                context_window_tokens: None,
                selectable: false,
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

fn empty_model() -> Model {
    Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    )
}

#[test]
fn permission_overlay_scopes_the_resource_and_dispatches_one_exact_answer() {
    let mut projection = (*session(2, Vec::new())).clone();
    projection.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("tool-call-1").expect("tool-call ID"),
        tool_name: "fs_write".to_owned(),
        capability: "filesystem write".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: vec![
            PermissionDetailView {
                label: "Path".to_owned(),
                value: "src/lib.rs".to_owned(),
            },
            PermissionDetailView {
                label: "Content bytes".to_owned(),
                value: "27".to_owned(),
            },
        ],
    });
    let mut model = Model::new(
        Arc::new(projection),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );

    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("Tool permission"));
    assert!(rendered.contains("workspace:src/lib.rs"));
    assert!(rendered.contains("Content bytes"));
    assert!(rendered.contains("27"));
    for expected in [MouseAction::PermissionAllow, MouseAction::PermissionDeny] {
        assert!((0..24).any(|row| {
            (0..80).any(|column| hit_test(&model, 80, 24, column, row) == Some(expected.clone()))
        }));
    }
    let effects = update(&mut model, Message::Mouse(MouseAction::PermissionAllow));

    assert_eq!(effects.len(), 1);
    let UiEffect::Dispatch(UiIntent::AnswerPermission {
        tool_call_id,
        allow,
        ..
    }) = &effects[0]
    else {
        panic!("expected permission answer");
    };
    assert_eq!(tool_call_id.as_str(), "tool-call-1");
    assert!(*allow);
    assert!(update(&mut model, Message::Input(key_input(Key::Char('y')))).is_empty());
}

#[test]
fn permission_details_are_scrollable_and_redacted_from_debug_output() {
    let sentinel = "permission-detail-secret";
    let mut projection = (*session(2, Vec::new())).clone();
    projection.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("tool-call-scroll").expect("tool-call ID"),
        tool_name: "process_run".to_owned(),
        capability: "process execute".to_owned(),
        resource: "program:cargo@workspace:.".to_owned(),
        details: (0..20)
            .map(|index| PermissionDetailView {
                label: "Argument".to_owned(),
                value: if index == 19 {
                    sentinel.to_owned()
                } else {
                    format!("argument-{index}")
                },
            })
            .collect(),
    });
    let mut model = Model::new(
        Arc::new(projection),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );

    assert!(!format!("{model:?}").contains(sentinel));
    let initial = buffer_text(&render_model(&model, 60, 12));
    assert!(initial.contains("argument-0"));
    assert!(!initial.contains(sentinel));
    for _ in 0..24 {
        let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    }
    let scrolled = buffer_text(&render_model(&model, 60, 12));
    assert!(scrolled.contains(sentinel));
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

fn alt(key: Key) -> Input {
    Input {
        alt: true,
        ..key_input(key)
    }
}

fn render_model(model: &Model, width: u16, height: u16) -> TestBackend {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| view(frame, model))
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

fn snapshot_model() -> Model {
    let failure = UiFailure::new(
        ErrorClass::RateLimited,
        "Capacity is temporarily exhausted; no credentials were exposed.",
        RetryPolicy::Now,
    );
    let transcript = vec![
        TranscriptItem::User {
            input_id: "input-1".to_owned(),
            text: "Plan a café launch.\nKeep it practical.".to_owned(),
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-1").expect("valid attempt"),
            text: "Start with a two-week validation sprint:\n1. Interview ten customers.\n2. Price three menu bundles.\n3. Measure repeat intent.".to_owned(),
            status: AttemptStatus::Completed,
            usage: Some(UsageView {
                input_tokens: 18,
                output_tokens: 41,
            }),
            retry_of: None,
        },
        TranscriptItem::User {
            input_id: "input-2".to_owned(),
            text: "What could go wrong?".to_owned(),
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-2").expect("valid attempt"),
            text: "The provider returned \u{1b}[31ma partial answer\u{1b}[0m.".to_owned(),
            status: AttemptStatus::Failed(failure),
            usage: None,
            retry_of: Some(AttemptKey::new("attempt-1").expect("valid attempt")),
        },
    ];
    let mut model = Model::new(
        session(8, transcript),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let _ = update(
        &mut model,
        Message::Paste("Retry with a smaller scope.\nKeep the checklist concise.".to_owned()),
    );
    let _ = update(&mut model, Message::Tick(UiClock::new(1_400, 0)));
    model
}

#[test]
fn composer_preserves_unicode_multiline_content() {
    let mut model = empty_model();

    for character in "Café ☕".chars() {
        let _ = update(&mut model, Message::Input(key_input(Key::Char(character))));
    }
    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    let _ = update(&mut model, Message::Paste("第二行\nemoji: 🧪".to_owned()));

    assert_eq!(model.composer.text(), "Café ☕\n第二行\nemoji: 🧪");
    assert_eq!(model.composer.lines().len(), 3);
}

#[test]
fn submission_keeps_draft_until_commit_and_rejection_keeps_it_editable() {
    let mut model = empty_model();
    let prompt = "  exact prompt\nwith trailing space  ";
    let _ = update(&mut model, Message::Paste(prompt.to_owned()));

    let effects = update(&mut model, Message::Input(ctrl(Key::Char('s'))));
    let UiEffect::Dispatch(UiIntent::SubmitPrompt {
        request_id,
        prompt: emitted,
    }) = effects.first().expect("submit effect")
    else {
        panic!("expected prompt submission");
    };
    assert_eq!(emitted, prompt);
    assert_eq!(model.composer.text(), prompt);

    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentRejected {
            request_id: *request_id,
            failure: UiFailure::new(
                ErrorClass::Unavailable,
                "Application is busy; the request was not queued",
                RetryPolicy::Now,
            ),
        }),
    );
    assert_eq!(model.composer.text(), prompt);
    assert!(model.pending().is_empty());
    assert!(matches!(model.notice, Some(Notice::Failure(_))));

    let effects = update(&mut model, Message::Input(ctrl(Key::Char('s'))));
    let request_id = effects
        .first()
        .and_then(|effect| match effect {
            UiEffect::Dispatch(intent) => Some(intent.request_id()),
            UiEffect::CopyTranscript(_) | UiEffect::Quit => None,
        })
        .expect("second request ID");
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted { request_id }),
    );
    assert_eq!(model.composer.text(), "");
}

#[test]
fn cancellation_and_retry_are_correlated_and_deduplicated() {
    let attempt = AttemptKey::new("attempt-streaming").expect("valid attempt");
    let streaming = TranscriptItem::Assistant {
        attempt_id: attempt.clone(),
        text: "partial".to_owned(),
        status: AttemptStatus::Streaming,
        usage: None,
        retry_of: None,
    };
    let mut model = Model::new(
        session(2, vec![streaming]),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );

    let first = update(&mut model, Message::Input(key_input(Key::Esc)));
    let cancel_request = match first.as_slice() {
        [
            UiEffect::Dispatch(UiIntent::CancelAttempt {
                request_id,
                attempt_id,
            }),
        ] if attempt_id == &attempt => *request_id,
        _ => panic!("expected cancellation intent"),
    };
    assert!(update(&mut model, Message::Input(key_input(Key::Esc))).is_empty());
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: cancel_request,
        }),
    );
    assert!(model.cancellation_requested(&attempt));
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("cancelling"));
    assert!(update(&mut model, Message::Input(key_input(Key::Esc))).is_empty());

    let projected_cancellation = TranscriptItem::Assistant {
        attempt_id: attempt.clone(),
        text: "partial".to_owned(),
        status: AttemptStatus::Cancelling,
        usage: None,
        retry_of: None,
    };
    let _ = update(
        &mut model,
        Message::SessionChanged(session(3, vec![projected_cancellation])),
    );
    assert!(model.cancellation_requested(&attempt));
    assert!(update(&mut model, Message::Input(key_input(Key::Esc))).is_empty());
    let _ = update(&mut model, Message::Tick(UiClock::new(10_000, 0)));

    let failed = TranscriptItem::Assistant {
        attempt_id: attempt.clone(),
        text: "partial".to_owned(),
        status: AttemptStatus::Failed(UiFailure::new(
            ErrorClass::RateLimited,
            "Try again after the provider window resets",
            RetryPolicy::After { delay_ms: 5_000 },
        )),
        usage: None,
        retry_of: None,
    };
    let _ = update(
        &mut model,
        Message::SessionChanged(session(4, vec![failed.clone()])),
    );
    let _ = update(&mut model, Message::Tick(UiClock::new(14_999, 0)));
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Char('r')))).is_empty(),
        "retry must wait for the projected deadline"
    );
    let _ = update(&mut model, Message::Tick(UiClock::new(15_000, 0)));
    let retry = update(&mut model, Message::Input(ctrl(Key::Char('r'))));
    let retry_request = match retry.as_slice() {
        [
            UiEffect::Dispatch(UiIntent::RetryAttempt {
                request_id,
                attempt_id,
            }),
        ] if attempt_id == &attempt => *request_id,
        _ => panic!("expected retry intent"),
    };
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: retry_request,
        }),
    );
    assert!(model.retry_requested(&attempt));
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("retrying"));
    assert!(
        update(&mut model, Message::Input(ctrl(Key::Char('r')))).is_empty(),
        "one durable retry request must remain deduplicated until projected"
    );

    let completed_retry = TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-retry").expect("valid retry attempt"),
        text: "recovered".to_owned(),
        status: AttemptStatus::Completed,
        usage: None,
        retry_of: Some(attempt.clone()),
    };
    let _ = update(
        &mut model,
        Message::SessionChanged(session(5, vec![failed, completed_retry])),
    );
    assert!(!model.retry_requested(&attempt));
    assert!(model.session.failed_attempt().is_none());
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("recovered"));
}

#[test]
fn settled_cancellation_can_be_retried_immediately() {
    let attempt = AttemptKey::new("attempt-cancelled").expect("valid attempt");
    let cancelled = TranscriptItem::Assistant {
        attempt_id: attempt.clone(),
        text: "partial".to_owned(),
        status: AttemptStatus::Cancelled,
        usage: None,
        retry_of: None,
    };
    let mut model = Model::new(
        session(3, vec![cancelled]),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );

    let retry = update(&mut model, Message::Input(ctrl(Key::Char('r'))));

    assert!(matches!(
        retry.as_slice(),
        [UiEffect::Dispatch(UiIntent::RetryAttempt { attempt_id, .. })]
            if attempt_id == &attempt
    ));
    assert!(buffer_text(&render_model(&model, 80, 24)).contains("retry"));
}

#[test]
fn catalog_refresh_respects_provider_retry_delay() {
    let catalog = Arc::new(CatalogProjection::Failed(UiFailure::new(
        ErrorClass::RateLimited,
        "Model discovery is rate limited",
        RetryPolicy::After { delay_ms: 5_000 },
    )));
    let mut model = Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        catalog,
    );
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('p'))));

    assert!(
        update(&mut model, Message::Input(ctrl(Key::Char('r')))).is_empty(),
        "refresh must wait for Retry-After"
    );
    let _ = update(&mut model, Message::Tick(UiClock::new(4_999, 0)));
    assert!(update(&mut model, Message::Input(ctrl(Key::Char('r')))).is_empty());
    let _ = update(&mut model, Message::Tick(UiClock::new(5_000, 0)));

    assert!(matches!(
        update(&mut model, Message::Input(ctrl(Key::Char('r')))).as_slice(),
        [UiEffect::Dispatch(UiIntent::RefreshCatalog { .. })]
    ));
}

#[test]
fn model_picker_filters_selectable_rows_and_waits_for_commit() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('p'))));
    for character in "flash".chars() {
        let _ = update(&mut model, Message::Input(key_input(Key::Char(character))));
    }

    assert!(model.picker_open());
    assert_eq!(model.picker_query(), "flash");
    assert_eq!(
        model
            .picker_selection()
            .expect("filtered selection")
            .model_id()
            .as_str(),
        "models/gemini-2.5-flash"
    );
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("Models"));
    assert!(rendered.contains("flash"));
    assert!(rendered.contains("Gemini 2.5 Flash"));
    assert!(!rendered.contains("Embedding 001"));

    let effects = update(&mut model, Message::Input(key_input(Key::Enter)));
    let request_id = match effects.as_slice() {
        [
            UiEffect::Dispatch(UiIntent::SelectModel {
                request_id, model, ..
            }),
        ] => {
            assert_eq!(model.model_id().as_str(), "models/gemini-2.5-flash");
            *request_id
        }
        _ => panic!("expected model-selection intent"),
    };
    assert!(model.picker_open());
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted { request_id }),
    );
    assert!(!model.picker_open());
}

#[test]
fn stale_picker_reserves_a_row_instead_of_covering_a_model() {
    let catalog = match &*ready_catalog() {
        CatalogProjection::Ready { models, .. } => Arc::new(CatalogProjection::Ready {
            models: models.clone(),
            stale: true,
        }),
        CatalogProjection::CredentialRequired
        | CatalogProjection::Loading
        | CatalogProjection::Failed(_) => unreachable!(),
    };
    let mut model = Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        catalog,
    );
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('p'))));

    let rendered = buffer_text(&render_model(&model, 40, 12));

    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("stale catalog"));
}

#[test]
fn missing_credential_opens_a_masked_zeroizing_editor() {
    let sentinel = "gemini-ui-secret-sentinel";
    let mut model = Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::CredentialRequired),
    );
    assert!(!model.credential_open());
    let _ = update(&mut model, Message::Tick(UiClock::new(2_000, 0)));
    for character in "/settings".chars() {
        let _ = update(&mut model, Message::Input(key_input(Key::Char(character))));
    }
    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    assert!(model.settings_open());
    for _ in 0..3 {
        let _ = update(&mut model, Message::Input(key_input(Key::Down)));
    }
    let _ = update(&mut model, Message::Input(key_input(Key::Tab)));
    let _ = update(&mut model, Message::Input(key_input(Key::Enter)));
    assert!(model.credential_open());

    let paste = Message::Paste(format!("{sentinel}\r\n"));
    assert!(!format!("{paste:?}").contains(sentinel));
    let _ = update(&mut model, paste);

    assert!(model.credential_has_value());
    assert!(!format!("{model:?}").contains(sentinel));
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(rendered.contains("••••••••••••"));
    assert!(!rendered.contains(sentinel));
    let tiny = buffer_text(&render_model(&model, 24, 7));
    assert!(tiny.contains("API key required"));
    assert!(tiny.contains("••••••••••••"));
    assert!(!tiny.contains(sentinel));

    let effects = update(&mut model, Message::Input(key_input(Key::Enter)));
    assert!(!format!("{effects:?}").contains(sentinel));
    assert!(!model.credential_open());
    assert!(!model.credential_has_value());
    let (request_id, credential) = match effects.into_iter().next() {
        Some(UiEffect::Dispatch(UiIntent::ConfigureCredential {
            request_id,
            credential,
        })) => (request_id, credential),
        _ => panic!("expected credential intent"),
    };
    assert!(!format!("{credential:?}").contains(sentinel));
    let exposed = Zeroizing::new(credential.into_string());
    assert_eq!(exposed.as_str(), sentinel);

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('k'))));
    assert!(!model.credential_open());
    assert!(matches!(model.notice, Some(Notice::Info(_))));

    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentRejected {
            request_id,
            failure: UiFailure::new(
                ErrorClass::Authentication,
                "provider authentication failed",
                RetryPolicy::Never,
            ),
        }),
    );
    assert!(!model.credential_open());
    assert!(model.settings_open());
    assert!(!model.credential_has_value());
}

#[test]
fn ctrl_k_reopens_the_credential_editor_without_touching_the_prompt() {
    let mut model = empty_model();
    let _ = update(&mut model, Message::Paste("draft remains".to_owned()));

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('k'))));

    assert!(model.credential_open());
    assert_eq!(model.composer.text(), "draft remains");
}

#[test]
fn ctrl_n_is_global_and_accepts_a_lower_revision_from_the_new_session() {
    let mut model = empty_model();
    let _ = update(
        &mut model,
        Message::Paste("draft from old session".to_owned()),
    );
    let effects = update(&mut model, Message::Input(ctrl(Key::Char('n'))));
    let [UiEffect::Dispatch(UiIntent::CreateSession { request_id })] = effects.as_slice() else {
        panic!("Ctrl+N must dispatch the typed new-session intent");
    };

    let replacement = Arc::new(SessionProjection {
        session_id: "session-replacement".to_owned(),
        revision: 1,
        selected_model: None,
        transcript: Vec::new(),
        permission_requests: Vec::new(),
    });
    let _ = update(&mut model, Message::SessionChanged(replacement));
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: *request_id,
        }),
    );

    assert_eq!(model.session.session_id, "session-replacement");
    assert_eq!(model.session.revision, 1);
    assert!(model.composer.is_blank());
    assert_eq!(
        model.notice,
        Some(Notice::Info("New session created".to_owned()))
    );
}

#[test]
fn a_replacement_credential_catalog_requires_a_valid_model_reselection() {
    let mut model = empty_model();
    let replacement = model_ref("models/gemini-replacement");
    let catalog = Arc::new(CatalogProjection::Ready {
        models: vec![ModelSummary {
            model: replacement.clone(),
            display_name: "Gemini replacement".to_owned(),
            detail: "text".to_owned(),
            context_window_tokens: Some(1_000_000),
            selectable: true,
        }],
        stale: false,
    });

    let _ = update(&mut model, Message::CatalogChanged(catalog));

    assert!(model.picker_open());
    assert_eq!(model.picker_selection(), Some(&replacement));
}

#[test]
fn stale_session_projection_cannot_roll_the_transcript_back() {
    let newest = TranscriptItem::User {
        input_id: "new".to_owned(),
        text: "newest".to_owned(),
    };
    let mut model = Model::new(
        session(9, vec![newest]),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let old = TranscriptItem::User {
        input_id: "old".to_owned(),
        text: "stale".to_owned(),
    };

    let _ = update(&mut model, Message::SessionChanged(session(8, vec![old])));

    assert_eq!(model.session.revision, 9);
    assert!(matches!(
        model.session.transcript.as_slice(),
        [TranscriptItem::User { text, .. }] if text == "newest"
    ));
}

#[test]
fn rendered_text_never_contains_terminal_or_directional_controls() {
    let safe = display_safe("before\u{1b}[31mred\r\u{202e}after\tend");

    assert_eq!(safe, "before\\u{1b}[31mred\\u{d}\\u{202e}after    end");
    assert!(!safe.contains('\u{1b}'));
    assert!(!safe.contains('\r'));
    assert!(!safe.contains('\u{202e}'));

    let mut model = empty_model();
    let _ = update(
        &mut model,
        Message::Paste("draft\u{1b}[2J\u{202e}\r\nnext\tcell".to_owned()),
    );
    assert_eq!(
        model.composer.text(),
        "draft\\u{1b}[2J\\u{202e}\nnext    cell"
    );
    let rendered = buffer_text(&render_model(&model, 80, 24));
    assert!(!rendered.contains('\u{1b}'));
    assert!(!rendered.contains('\u{202e}'));
}

#[test]
fn manual_scroll_pauses_tail_follow_and_end_resumes_it() {
    let transcript = (0..10)
        .map(|index| TranscriptItem::User {
            input_id: format!("input-{index}"),
            text: format!("message {index}"),
        })
        .collect();
    let mut model = Model::new(
        session(11, transcript),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let at_tail = buffer_text(&render_model(&model, 40, 12));
    assert!(at_tail.contains("message 9"));

    let _ = update(&mut model, Message::Input(alt(Key::Up)));
    let scrolled = buffer_text(&render_model(&model, 40, 12));
    assert!(!model.transcript.follow_tail);
    assert!(!scrolled.contains("message 9"));

    let _ = update(&mut model, Message::Input(ctrl(Key::End)));
    let followed = buffer_text(&render_model(&model, 40, 12));
    assert!(model.transcript.follow_tail);
    assert!(followed.contains("message 9"));
}

#[test]
fn tick_stores_wall_clock_beside_monotonic_time() {
    let mut model = empty_model();
    assert_eq!(model.now(), 0);
    assert_eq!(model.wall_ms(), 0);
    let _ = update(
        &mut model,
        Message::Tick(UiClock::new(1_400, 1_700_000_000_123)),
    );
    assert_eq!(model.now(), 1_400);
    assert_eq!(model.wall_ms(), 1_700_000_000_123);
}

#[test]
fn fixed_size_views_match_reviewed_golden_buffers() {
    let model = snapshot_model();
    let cases = [
        (
            120,
            50,
            "golden/main-120x50.txt",
            include_str!("golden/main-120x50.txt"),
        ),
        (
            120,
            40,
            "golden/main-120x40.txt",
            include_str!("golden/main-120x40.txt"),
        ),
        (
            80,
            24,
            "golden/main-80x24.txt",
            include_str!("golden/main-80x24.txt"),
        ),
        (
            60,
            18,
            "golden/main-60x18.txt",
            include_str!("golden/main-60x18.txt"),
        ),
        (
            40,
            12,
            "golden/main-40x12.txt",
            include_str!("golden/main-40x12.txt"),
        ),
    ];
    let update_goldens = std::env::var("AUTOHARNESS_UPDATE_GOLDENS").as_deref() == Ok("1");

    for (width, height, path, expected) in cases {
        let backend = render_model(&model, width, height);
        let actual = style_snapshot(backend.buffer());
        if update_goldens {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join(path);
            std::fs::write(path, &actual).expect("write updated golden");
        } else {
            assert_eq!(actual, expected, "golden mismatch at {width}x{height}");
        }
        assert!(
            backend.buffer().cell((0, 0)).is_some(),
            "shell must paint the origin cell at {width}x{height}"
        );
    }
}

#[test]
fn every_tiny_terminal_size_renders_without_panicking() {
    let mut model = snapshot_model();
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('p'))));

    for width in 1..=23 {
        for height in 1..=6 {
            let backend = render_model(&model, width, height);
            assert_eq!(backend.buffer().area.width, width);
            assert_eq!(backend.buffer().area.height, height);
        }
    }
}

struct VisualPreferences {
    color_mode: &'static str,
    theme: &'static str,
    glyph_mode: &'static str,
    reduced_motion: bool,
    density: &'static str,
    layout: &'static str,
    timestamp: &'static str,
}

fn apply_visual_preferences(model: &mut Model, preferences: VisualPreferences) {
    let preferences = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            format!(
                r#"{{"schema_version":3,"local_profile":{{"preferences":{{"theme_preset":"{}","color_mode":"{}","glyph_mode":"{}","reduced_motion":{},"density":"{}","layout":"{}","terminal_timestamp_style":"{}"}}}}}}"#,
                preferences.theme,
                preferences.color_mode,
                preferences.glyph_mode,
                preferences.reduced_motion,
                preferences.density,
                preferences.layout,
                preferences.timestamp,
            ),
        )
        .resolve()
        .expect("visual preference fixture");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: preferences.local_profile().clone(),
        ..SettingsProjection::default()
    }));
}

#[test]
fn reduced_motion_freezes_the_generation_scanner() {
    let transcript = vec![TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-reduced-motion").expect("attempt"),
        text: String::new(),
        status: AttemptStatus::Streaming,
        usage: None,
        retry_of: None,
    }];
    let mut model = Model::new(
        session(2, transcript),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "system",
            glyph_mode: "ascii",
            reduced_motion: true,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );

    let first = buffer_text(&render_model(&model, 80, 24));
    let _ = update(&mut model, Message::Tick(UiClock::new(700, 0)));
    let later = buffer_text(&render_model(&model, 80, 24));
    assert!(first.contains("[========]"));
    assert_eq!(first, later);
}

fn assert_style_golden(model: &Model, width: u16, height: u16, relative: &str, expected: &str) {
    let backend = render_model(model, width, height);
    let actual = style_snapshot(backend.buffer());
    if std::env::var("AUTOHARNESS_UPDATE_GOLDENS").as_deref() == Ok("1") {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(relative);
        std::fs::write(path, &actual).expect("write updated golden");
    } else {
        assert_eq!(actual, expected, "golden mismatch for {relative}");
    }
}

#[test]
fn chat_workspace_states_match_reviewed_snapshots() {
    let streaming = {
        let transcript = vec![TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-stream").expect("attempt"),
            text: String::new(),
            status: AttemptStatus::Streaming,
            usage: None,
            retry_of: None,
        }];
        Model::new(
            session(2, transcript),
            Arc::new(SessionsProjection::default()),
            ready_catalog(),
        )
    };
    assert_style_golden(
        &streaming,
        80,
        24,
        "golden/chat-streaming-80x24.txt",
        include_str!("golden/chat-streaming-80x24.txt"),
    );

    let mut cancelling = Model::new(
        session(
            2,
            vec![TranscriptItem::Assistant {
                attempt_id: AttemptKey::new("attempt-stream").expect("attempt"),
                text: String::new(),
                status: AttemptStatus::Streaming,
                usage: None,
                retry_of: None,
            }],
        ),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let first = update(&mut cancelling, Message::Input(key_input(Key::Esc)));
    if let [UiEffect::Dispatch(UiIntent::CancelAttempt { request_id, .. })] = first.as_slice() {
        let _ = update(
            &mut cancelling,
            Message::Notice(UiNotice::IntentCommitted {
                request_id: *request_id,
            }),
        );
    }
    assert_style_golden(
        &cancelling,
        80,
        24,
        "golden/chat-cancelling-80x24.txt",
        include_str!("golden/chat-cancelling-80x24.txt"),
    );

    let failed = Model::new(
        session(
            2,
            vec![TranscriptItem::Assistant {
                attempt_id: AttemptKey::new("attempt-fail").expect("attempt"),
                text: String::new(),
                status: AttemptStatus::Failed(UiFailure::new(
                    ErrorClass::Unavailable,
                    "provider unavailable",
                    RetryPolicy::Now,
                )),
                usage: None,
                retry_of: None,
            }],
        ),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    assert_style_golden(
        &failed,
        80,
        24,
        "golden/chat-failed-80x24.txt",
        include_str!("golden/chat-failed-80x24.txt"),
    );

    let mut offline = empty_model();
    let _ = update(
        &mut offline,
        Message::CatalogChanged(Arc::new(CatalogProjection::CredentialRequired)),
    );
    assert_style_golden(
        &offline,
        80,
        24,
        "golden/chat-offline-80x24.txt",
        include_str!("golden/chat-offline-80x24.txt"),
    );

    let loading = Model::new(
        session(1, Vec::new()),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::Loading),
    );
    let mut loading = loading;
    let _ = update(&mut loading, Message::Tick(UiClock::new(400, 0)));
    assert_style_golden(
        &loading,
        80,
        24,
        "golden/chat-loading-80x24.txt",
        include_str!("golden/chat-loading-80x24.txt"),
    );

    let mut no_model = Model::new(
        Arc::new(SessionProjection {
            session_id: "session-fixture".to_owned(),
            revision: 1,
            selected_model: None,
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    let _ = update(&mut no_model, Message::Input(key_input(Key::Esc)));
    assert_style_golden(
        &no_model,
        80,
        24,
        "golden/chat-no-model-80x24.txt",
        include_str!("golden/chat-no-model-80x24.txt"),
    );

    let empty_catalog = Model::new(
        Arc::new(SessionProjection {
            session_id: "session-fixture".to_owned(),
            revision: 1,
            selected_model: None,
            transcript: Vec::new(),
            permission_requests: Vec::new(),
        }),
        Arc::new(SessionsProjection::default()),
        Arc::new(CatalogProjection::Ready {
            models: Vec::new(),
            stale: false,
        }),
    );
    assert_style_golden(
        &empty_catalog,
        80,
        24,
        "golden/chat-empty-catalog-80x24.txt",
        include_str!("golden/chat-empty-catalog-80x24.txt"),
    );

    assert_style_golden(
        &empty_model(),
        80,
        24,
        "golden/chat-new-conversation-80x24.txt",
        include_str!("golden/chat-new-conversation-80x24.txt"),
    );
}

#[test]
fn accessibility_visual_matrix_preserves_security_text_and_ascii_borders() {
    let sizes = [(120, 50), (120, 40), (80, 24), (60, 18), (40, 12)];
    let mut permission = (*session(9, Vec::new())).clone();
    permission.permission_requests.push(PermissionRequestView {
        tool_call_id: ToolCallKey::new("matrix-permission").expect("tool call"),
        tool_name: "fs_write".to_owned(),
        capability: "filesystem write".to_owned(),
        resource: "workspace:src/lib.rs".to_owned(),
        details: vec![PermissionDetailView {
            label: "Path".to_owned(),
            value: "src/lib.rs".to_owned(),
        }],
    });
    let mut model = Model::new(
        Arc::new(permission),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "no_color",
            theme: "dark",
            glyph_mode: "ascii",
            reduced_motion: true,
            density: "compact",
            layout: "single_column",
            timestamp: "absolute",
        },
    );
    for (width, height) in sizes {
        let rendered = buffer_text(&render_model(&model, width, height));
        assert!(rendered.contains("Tool permission"));
        assert!(rendered.contains("filesystem write"));
        assert!(rendered.contains("workspace:src/lib.rs"));
        assert!(rendered.contains("Deny (N/Esc)"));
        if width > 40 && height > 12 {
            assert!(rendered.contains('+'));
            assert!(!rendered.contains('┌'));
        }
    }
}

#[test]
fn accessibility_confirmation_matrix_retains_destructive_copy() {
    let sessions = Arc::new(SessionsProjection {
        sessions: vec![SessionBrowserEntry {
            session_id: "matrix-session".to_owned(),
            title: "Security review".to_owned(),
            archived: false,
            selected_model: Some(pro_model()),
            message_count: 4,
            updated_at_ms: 1_700_000_000_000,
            active: false,
        }],
    });
    let mut model = Model::new(session(10, Vec::new()), sessions, ready_catalog());
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "high_contrast",
            theme: "light",
            glyph_mode: "ascii",
            reduced_motion: true,
            density: "compact",
            layout: "single_column",
            timestamp: "hidden",
        },
    );
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('l'))));
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('d'))));
    for (width, height) in [(120, 50), (120, 40), (80, 24), (60, 18), (40, 12)] {
        let rendered = buffer_text(&render_model(&model, width, height));
        assert!(rendered.contains("Delete session"));
        assert!(rendered.contains("Permanently delete"));
        assert!(rendered.contains("Confirm (Y)"));
        assert!(rendered.contains("Cancel (N/Esc)"));
    }
}

#[test]
fn theme_and_timestamp_preferences_change_rendered_output() {
    let sessions = Arc::new(SessionsProjection {
        sessions: vec![SessionBrowserEntry {
            session_id: "timestamp-session".to_owned(),
            title: "Timestamp fixture".to_owned(),
            archived: false,
            selected_model: Some(pro_model()),
            message_count: 4,
            updated_at_ms: 1_700_000_000_000,
            active: false,
        }],
    });
    let mut model = Model::new(session(11, Vec::new()), sessions, ready_catalog());
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "light",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "absolute",
        },
    );
    let light = render_model(&model, 120, 40);
    assert_eq!(
        light.buffer().cell((25, 0)).expect("chat divider").bg,
        Color::Reset
    );
    let _ = update(&mut model, Message::Input(ctrl(Key::Char('l'))));
    let sessions = buffer_text(&render_model(&model, 120, 40));
    assert!(sessions.contains("1700000000000 ms"));
    assert!(sessions.contains("Messages"));

    let _ = update(&mut model, Message::Input(ctrl(Key::Char('1'))));
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "dark",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "hidden",
        },
    );
    let dark_chat = render_model(&model, 120, 40);
    assert_eq!(
        dark_chat.buffer().cell((25, 0)).expect("chat divider").bg,
        Color::Reset
    );
    assert!(!buffer_text(&dark_chat).contains("updated 1700000000000"));
}

#[test]
fn aurora_and_ember_themes_have_distinct_color_anchors() {
    let mut model = Model::new(
        session(12, Vec::new()),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "aurora",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    let aurora = render_model(&model, 120, 40);
    assert_eq!(
        aurora.buffer().cell((25, 0)).expect("aurora divider").bg,
        Color::Reset
    );
    assert_eq!(
        aurora.buffer().cell((25, 0)).expect("aurora divider").fg,
        Color::Rgb(45, 212, 191)
    );

    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "ember",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    let ember = render_model(&model, 120, 40);
    assert_eq!(
        ember.buffer().cell((25, 0)).expect("ember divider").bg,
        Color::Reset
    );
    assert_eq!(
        ember.buffer().cell((25, 0)).expect("ember divider").fg,
        Color::Rgb(251, 146, 60)
    );
}

#[test]
fn additional_themes_and_color_treatments_have_distinct_visual_anchors() {
    let mut model = Model::new(
        session(14, Vec::new()),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    for (theme, expected) in [
        ("midnight", Color::Rgb(96, 165, 250)),
        ("ocean", Color::Rgb(34, 211, 238)),
        ("forest", Color::Rgb(74, 222, 128)),
        ("rose", Color::Rgb(244, 114, 182)),
    ] {
        apply_visual_preferences(
            &mut model,
            VisualPreferences {
                color_mode: "color",
                theme,
                glyph_mode: "unicode",
                reduced_motion: false,
                density: "comfortable",
                layout: "responsive",
                timestamp: "relative",
            },
        );
        assert_eq!(
            render_model(&model, 120, 40)
                .buffer()
                .cell((25, 0))
                .expect("theme anchor")
                .fg,
            expected,
            "theme {theme} must keep its own gradient accent"
        );
    }

    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "ocean",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    let color_fg = render_model(&model, 120, 40)
        .buffer()
        .cell((25, 0))
        .expect("ocean color anchor")
        .fg;

    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "soft",
            theme: "ocean",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    let soft = render_model(&model, 120, 40);
    let soft_cell = soft.buffer().cell((25, 0)).expect("soft theme anchor");
    assert_ne!(
        soft_cell.fg, color_fg,
        "soft mode must reduce chroma instead of using DIM"
    );
    assert!(!soft_cell.modifier.contains(ratatui::style::Modifier::DIM));

    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "vivid",
            theme: "ocean",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    let vivid = render_model(&model, 120, 40);
    assert!(
        vivid
            .buffer()
            .cell((25, 0))
            .expect("vivid theme anchor")
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
}

#[test]
fn nerd_font_mode_adds_optional_icons_without_changing_the_ascii_fallback() {
    let mut model = Model::new(
        session(16, Vec::new()),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    apply_visual_preferences(
        &mut model,
        VisualPreferences {
            color_mode: "color",
            theme: "aurora",
            glyph_mode: "nerd_font",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            timestamp: "relative",
        },
    );
    model.apply_profiles(Arc::new(autoharness_tui::ProfilesProjection {
        user: autoharness_tui::LocalUserProfileProjection {
            workspace: r"C:\Users\jense\Desktop\AutoHarness".to_owned(),
            default_mode: "high".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    }));
    model.apply_settings(Arc::new(SettingsProjection {
        git_branch: Some("feat/tui-polish".to_owned()),
        ..model.settings().clone()
    }));

    let rendered = buffer_text(&render_model(&model, 120, 40));
    assert!(rendered.contains("AutoHarness"));
    assert!(rendered.contains("~/Desktop/AutoHarness"));
    assert!(rendered.contains("feat/tui-polish"));
    assert!(rendered.contains('│'));
}

#[test]
fn every_theme_and_color_treatment_renders_across_responsive_sizes() {
    let mut model = Model::new(
        session(15, Vec::new()),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    for theme in [
        "system", "light", "dark", "aurora", "ember", "midnight", "ocean", "forest", "rose",
    ] {
        for color_mode in ["color", "soft", "vivid", "no_color", "high_contrast"] {
            apply_visual_preferences(
                &mut model,
                VisualPreferences {
                    color_mode,
                    theme,
                    glyph_mode: "unicode",
                    reduced_motion: false,
                    density: "comfortable",
                    layout: "responsive",
                    timestamp: "relative",
                },
            );
            for (width, height) in [(120, 40), (80, 24), (40, 12)] {
                let rendered = render_model(&model, width, height);
                assert_eq!(rendered.buffer().area.width, width);
                assert_eq!(rendered.buffer().area.height, height);
            }
        }
    }
}

#[test]
fn prompt_bar_shows_safe_runtime_metadata() {
    let transcript = vec![TranscriptItem::Assistant {
        attempt_id: AttemptKey::new("attempt-metrics").expect("attempt"),
        text: "answer".to_owned(),
        status: AttemptStatus::Completed,
        usage: Some(UsageView {
            input_tokens: 249_000,
            output_tokens: 1_000,
        }),
        retry_of: None,
    }];
    let mut model = Model::new(
        session(13, transcript),
        Arc::new(SessionsProjection::default()),
        ready_catalog(),
    );
    model.apply_profiles(Arc::new(autoharness_tui::ProfilesProjection {
        user: autoharness_tui::LocalUserProfileProjection {
            workspace: r"C:\Users\jense\Desktop\AutoHarness".to_owned(),
            default_mode: "high".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    }));
    let settings = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{"schema_version":3,"local_profile":{"preferences":{"prompt_status_detail":"detailed"}}}"#,
        )
        .resolve()
        .expect("prompt detail fixture");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: settings.local_profile().clone(),
        git_branch: Some("feat/prompt-bar".to_owned()),
        ..SettingsProjection::default()
    }));
    let rendered = buffer_text(&render_model(&model, 160, 40));
    assert!(rendered.contains("Gemini 2.5 Pro"));
    assert!(rendered.contains("high"));
    assert!(!rendered.contains("high ●●●●○○"));
    assert!(rendered.contains("ctx 25%"));
    assert!(rendered.contains("~/Desktop/AutoHarness"));
    assert!(rendered.contains("⑂ feat/prompt-bar"));
    assert!(rendered.contains("in 249.0k / out 1.0k"));
    assert!(rendered.matches('│').count() >= 4);
    for prefix in ["model:", "mode:", "think:", "path:", "git:"] {
        assert!(!rendered.contains(prefix));
    }
}
