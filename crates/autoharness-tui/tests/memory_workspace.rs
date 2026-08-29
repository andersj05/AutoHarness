use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    CatalogProjection, Focus, MEMORY_VIEW_PAGE_SIZE, MemoryAdmission, MemoryAdmissionContext,
    MemoryDetail, MemoryEvidence, MemoryFindingKind, MemoryLifecycleMode, MemoryOrigin,
    MemoryPageDirection, MemoryPane, MemoryProjection, MemoryRelation, MemoryRelationKind,
    MemoryRevisionContext, MemoryScope, MemorySensitivity, MemoryStatus, MemoryStatusFilter,
    MemorySummary, MemoryTrust, MemoryValidationFinding, MemoryViewCursor, MemoryViewQuery,
    Message, Model, ModelSummary, MouseAction, OverlayKind, RetryPolicy, Route, SessionProjection,
    SessionsProjection, SettingsProjection, UiClock, UiEffect, UiFailure, UiIntent, UiNotice,
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
        .expect("admission")
        .with_context(
            MemoryAdmissionContext::new(
                "attempt-launch-1",
                4,
                "epoch-launch-a",
                38,
                "revision-concise-3",
                "memory-renderer-v1",
                vec![
                    "workspace scope matched".to_owned(),
                    "high confidence".to_owned(),
                ],
            )
            .expect("admission context"),
        ),
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
        .expect("detail")
        .with_revision_context(
            MemoryRevisionContext::new(
                11,
                "revision-concise-3",
                None,
                "workspace current-project",
                MemoryOrigin::ExplicitUser,
                MemorySensitivity::Internal,
                vec![MemoryEvidence::new(
                    "User instruction",
                    "session launch",
                    "Please keep implementation notes concise and verified.",
                )
                .expect("evidence")],
                vec![MemoryRelation::new(
                    MemoryRelationKind::DerivedFrom,
                    "memory-style-source",
                )
                .expect("relation")],
                vec![],
            )
            .expect("revision context"),
        ),
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
        .expect("detail")
        .with_revision_context(
            MemoryRevisionContext::new(
                17,
                "revision-keyboard-1",
                Some("proposal-keyboard-1".to_owned()),
                "user current-account",
                MemoryOrigin::ModelProposal,
                MemorySensitivity::Internal,
                vec![MemoryEvidence::new(
                    "Observed preference",
                    "session setup",
                    "The user requested keyboard-first terminal workflows.",
                )
                .expect("evidence")],
                vec![MemoryRelation::new(
                    MemoryRelationKind::Contradicts,
                    "memory-mouse-first",
                )
                .expect("relation")],
                vec![MemoryValidationFinding::new(
                    MemoryFindingKind::Contradiction,
                    "memory-mouse-first",
                    "An older preference may prioritize mouse-first controls.",
                )
                .expect("finding")],
            )
            .expect("revision context"),
        ),
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

fn ctrl(character: char) -> Input {
    Input {
        ctrl: true,
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
    assert!(medium.contains("Search all memory"));
    assert!(medium.contains("Revision detail"));
    assert!(!medium.contains("Admission history"));
    assert!(medium.contains("Alt+N remember  Alt+A actions"));

    let narrow = text(&render(&model, 60, 18));
    assert!(narrow.contains("Alt+N remember"));
    assert!(!narrow.contains("Alt+A ac"));

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
            "x".repeat(512),
            "safe preview",
            MemoryStatus::Active,
            MemoryScope::User,
            0,
            None,
            0,
        )
        .is_ok()
    );
    assert!(
        MemorySummary::new(
            "x".repeat(513),
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
    let detail = projection.detail("memory-concise").expect("detail");
    let admission_debug = format!("{:?}", detail.admissions()[0]);
    assert!(!admission_debug.contains("attempt-launch-1"));
    assert!(!admission_debug.contains("epoch-launch-a"));
    assert!(!admission_debug.contains("workspace scope matched"));
    let revision_debug = format!("{:?}", detail.revision_context().expect("context"));
    assert!(!revision_debug.contains("workspace current-project"));
    assert!(!revision_debug.contains("revision-concise-3"));
    assert_eq!(MemorySensitivity::Secret.label(), "secret");
}

#[test]
fn expanded_admission_pane_shows_exact_turn_coordinates_and_reason_factors() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Input(key(Key::Enter)));
    let _ = update(&mut model, Message::Mouse(MouseAction::MemoryAdmissions));
    let rendered = text(&render(&model, 80, 24));
    assert!(rendered.contains("attempt-launch-1"));
    assert!(rendered.contains("epoch-launch-a"));
    assert!(rendered.contains("revision-concise-3"));
    assert!(rendered.contains("memory-renderer-v1"));
    assert!(rendered.contains("workspace scope matched"));
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
fn memory_view_query_is_literal_bounded_and_debug_redacted() {
    let cursor = MemoryViewCursor::new("1725000000000:memory-literal").expect("cursor");
    let query = MemoryViewQuery::new(
        "$term* [literal]",
        MemoryStatusFilter::All,
        autoharness_tui::MemoryScopeFilter::Workspace,
        MemoryPageDirection::Next,
        Some(cursor.clone()),
        MEMORY_VIEW_PAGE_SIZE,
    )
    .expect("query");
    assert_eq!(query.literal(), "$term* [literal]");
    assert_eq!(query.status(), MemoryStatusFilter::All);
    assert_eq!(query.scope(), autoharness_tui::MemoryScopeFilter::Workspace);
    assert_eq!(query.direction(), MemoryPageDirection::Next);
    assert_eq!(query.before(), Some(&cursor));
    assert_eq!(query.limit(), MEMORY_VIEW_PAGE_SIZE);
    assert!(!format!("{query:?}").contains("$term"));
    assert!(!format!("{cursor:?}").contains("memory-literal"));
    assert!(
        MemoryViewQuery::new(
            "query",
            MemoryStatusFilter::All,
            autoharness_tui::MemoryScopeFilter::All,
            MemoryPageDirection::Next,
            None,
            MEMORY_VIEW_PAGE_SIZE,
        )
        .is_err()
    );
    assert!(
        MemoryViewQuery::new(
            "query",
            MemoryStatusFilter::All,
            autoharness_tui::MemoryScopeFilter::All,
            MemoryPageDirection::First,
            Some(cursor),
            MEMORY_VIEW_PAGE_SIZE,
        )
        .is_err()
    );
}

#[test]
fn memory_query_and_filters_coalesce_after_local_feedback() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    assert!(update(&mut model, Message::Mouse(MouseAction::MemoryCycleStatus)).is_empty());
    assert!(update(&mut model, Message::Mouse(MouseAction::MemoryCycleScope)).is_empty());
    let _ = update(&mut model, Message::Input(key(Key::Char('/'))));
    type_text(&mut model, "keyboard");

    assert_eq!(model.memory_selection(), Some("memory-keyboard"));
    assert!(model.memory_view_loading());
    assert!(text(&render(&model, 60, 18)).contains("Searching all memory"));
    assert!(
        update(
            &mut model,
            Message::Tick(UiClock::new(149, 1_725_000_000_000))
        )
        .is_empty()
    );

    let effects = update(
        &mut model,
        Message::Tick(UiClock::new(150, 1_725_000_000_000)),
    );
    let [
        UiEffect::Dispatch(UiIntent::QueryMemory {
            view_generation,
            query,
            ..
        }),
    ] = effects.as_slice()
    else {
        panic!("expected one coalesced Memory query");
    };
    assert_eq!(*view_generation, model.memory_view_generation());
    assert_eq!(query.literal(), "keyboard");
    assert_eq!(query.status(), MemoryStatusFilter::All);
    assert_eq!(query.scope(), autoharness_tui::MemoryScopeFilter::User);
    assert_eq!(query.direction(), MemoryPageDirection::First);
    assert!(query.before().is_none());
    assert_eq!(query.limit(), MEMORY_VIEW_PAGE_SIZE);
    assert!(
        update(
            &mut model,
            Message::Tick(UiClock::new(400, 1_725_000_000_000))
        )
        .is_empty()
    );
}

#[test]
fn memory_view_generation_rejects_stale_responses_independently() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    type_text(&mut model, "concise");
    let desired = model.memory_view_generation();

    let future_durable_stale_view = MemoryProjection::ready(99, Vec::new(), Vec::new(), 0, false)
        .expect("stale view")
        .with_view_page(desired.saturating_sub(1), None);
    let _ = update(
        &mut model,
        Message::MemoryChanged(Arc::new(future_durable_stale_view)),
    );
    assert_eq!(model.memory().generation(), 7);
    assert!(model.memory_view_loading());

    let effects = update(
        &mut model,
        Message::Tick(UiClock::new(150, 1_725_000_000_000)),
    );
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::QueryMemory { view_generation, .. })]
            if *view_generation == desired
    ));
    let matching = MemoryProjection::ready(8, Vec::new(), Vec::new(), 0, false)
        .expect("matching view")
        .with_view_page(desired, None);
    let _ = update(&mut model, Message::MemoryChanged(Arc::new(matching)));
    assert_eq!(model.memory().generation(), 8);
    assert!(!model.memory_view_loading());

    let stale_durable = MemoryProjection::ready(6, Vec::new(), Vec::new(), 0, false)
        .expect("stale durable")
        .with_view_page(desired, None);
    let _ = update(&mut model, Message::MemoryChanged(Arc::new(stale_durable)));
    assert_eq!(model.memory().generation(), 8);
}

#[test]
fn memory_pages_have_loading_keyboard_and_mouse_affordances() {
    let mut model = model();
    model.apply_memory(Arc::new(memory_projection().with_view_page(
        0,
        Some(MemoryViewCursor::new("page-two-boundary").expect("next cursor")),
    )));
    let _ = update(&mut model, Message::Input(alt('6')));

    let effects = update(&mut model, Message::Input(key(Key::PageDown)));
    let [
        UiEffect::Dispatch(UiIntent::QueryMemory {
            view_generation,
            query,
            ..
        }),
    ] = effects.as_slice()
    else {
        panic!("expected next-page query");
    };
    assert_eq!(query.direction(), MemoryPageDirection::Next);
    assert_eq!(
        query.before().map(MemoryViewCursor::as_str),
        Some("page-two-boundary")
    );
    let next_generation = *view_generation;
    assert!(text(&render(&model, 40, 12)).contains("Loading next page"));
    assert!(
        update(&mut model, Message::Input(key(Key::PageDown))).is_empty(),
        "an in-flight page request must coalesce duplicate navigation"
    );

    let page_two = memory_projection().with_view_page(
        next_generation,
        Some(MemoryViewCursor::new("page-three-boundary").expect("third cursor")),
    );
    let _ = update(&mut model, Message::MemoryChanged(Arc::new(page_two)));
    assert!(model.memory_has_previous_page());
    assert!(model.memory_has_next_page());
    let rendered = text(&render(&model, 80, 24));
    assert!(rendered.contains("PgUp"));
    assert!(rendered.contains("PgDn"));

    let mut actions = Vec::new();
    for column in 0..80 {
        for row in 0..24 {
            if let Some(action) = hit_test(&model, 80, 24, column, row) {
                actions.push(action);
            }
        }
    }
    assert!(actions.contains(&MouseAction::MemoryPreviousPage));
    assert!(actions.contains(&MouseAction::MemoryNextPage));

    let effects = update(&mut model, Message::Mouse(MouseAction::MemoryPreviousPage));
    let [UiEffect::Dispatch(UiIntent::QueryMemory { query, .. })] = effects.as_slice() else {
        panic!("expected previous-page query");
    };
    assert_eq!(query.direction(), MemoryPageDirection::Previous);
    assert!(query.before().is_none());
}

#[test]
fn partial_memory_page_never_claims_a_global_search_miss() {
    let mut model = model();
    let summary = MemorySummary::new(
        "memory-page-one",
        "A row from the first loaded page.",
        MemoryStatus::Active,
        MemoryScope::Workspace,
        1_725_000_000_000,
        Some(9_000),
        0,
    )
    .expect("summary");
    model.apply_memory(Arc::new(
        MemoryProjection::ready(30, vec![summary], vec![], 101, true).expect("partial projection"),
    ));
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Input(key(Key::Char('/'))));
    type_text(&mut model, "not-on-first-page");
    let rendered = text(&render(&model, 60, 18));
    assert!(rendered.contains("Searching all memory"));
    assert!(!rendered.contains("No memories match these filters"));
}

#[test]
fn conflicting_and_expired_memories_have_deliberate_states_and_safe_actions() {
    let mut model = model();
    let conflicting = MemorySummary::new(
        "memory-conflicting",
        "A proposed preference conflicts with current durable knowledge.",
        MemoryStatus::Conflicting,
        MemoryScope::Workspace,
        1_726_000_000_000,
        Some(6_000),
        0,
    )
    .expect("conflicting summary");
    let expired = MemorySummary::new(
        "memory-expired",
        "A formerly active preference has passed its validity boundary.",
        MemoryStatus::Expired,
        MemoryScope::Session,
        1_725_000_000_000,
        Some(8_000),
        1,
    )
    .expect("expired summary");
    let conflicting_detail = MemoryDetail::new(
        "memory-conflicting",
        1,
        "Prefer the contradictory behavior.",
        "model proposal",
        MemoryTrust::UntrustedProposal,
        1_726_000_000_000,
        None,
        vec![],
    )
    .expect("conflicting detail")
    .with_revision_context(
        MemoryRevisionContext::new(
            3,
            "revision-conflicting-1",
            Some("revision-conflicting-1".to_owned()),
            "workspace current-project",
            MemoryOrigin::ModelProposal,
            MemorySensitivity::Internal,
            vec![],
            vec![],
            vec![
                MemoryValidationFinding::new(
                    MemoryFindingKind::Contradiction,
                    "memory-existing",
                    "Conflicts with an active preference",
                )
                .expect("finding"),
            ],
        )
        .expect("conflicting context"),
    );
    let expired_detail = MemoryDetail::new(
        "memory-expired",
        2,
        "Use the former preference only during the launch window.",
        "explicit user memory",
        MemoryTrust::UserApproved,
        1_720_000_000_000,
        Some(1_724_000_000_000),
        vec![],
    )
    .expect("expired detail")
    .with_revision_context(
        MemoryRevisionContext::new(
            8,
            "revision-expired-2",
            None,
            "session memory-session",
            MemoryOrigin::ExplicitUser,
            MemorySensitivity::Internal,
            vec![],
            vec![],
            vec![],
        )
        .expect("expired context"),
    );
    model.apply_memory(Arc::new(
        MemoryProjection::ready(
            40,
            vec![conflicting, expired],
            vec![conflicting_detail, expired_detail],
            2,
            false,
        )
        .expect("status projection"),
    ));
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Mouse(MouseAction::MemoryCycleStatus));
    let rendered = text(&render(&model, 80, 24));
    assert!(rendered.contains("conflicting"));
    assert!(rendered.contains("expired"));

    let _ = update(
        &mut model,
        Message::Mouse(MouseAction::MemorySelect("memory-conflicting".to_owned())),
    );
    let _ = update(&mut model, Message::Input(alt('v')));
    assert_eq!(
        model.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Review)
    );
    let _ = update(&mut model, Message::Input(key(Key::Esc)));
    let _ = update(
        &mut model,
        Message::Mouse(MouseAction::MemorySelect("memory-expired".to_owned())),
    );
    let _ = update(&mut model, Message::Input(alt('e')));
    assert_eq!(
        model.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Revise)
    );
}

#[test]
fn remember_editor_is_bounded_redacted_pending_safe_and_restores_focus() {
    let mut model = model();
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Input(alt('n')));
    assert_eq!(model.overlay(), Some(OverlayKind::MemoryLifecycle));
    assert_eq!(model.focus, Focus::MemoryLifecycle);
    assert_eq!(
        model.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Remember)
    );

    let compact = text(&render(&model, 40, 12));
    assert!(compact.contains("Scope: Workspace"));
    assert!(compact.contains("rejects secrets"));
    type_text(&mut model, "Remember this exact preference.");
    assert!(!format!("{model:?}").contains("exact preference"));

    let effects = update(&mut model, Message::Input(ctrl('s')));
    let [
        UiEffect::Dispatch(UiIntent::RememberMemory {
            request_id,
            content,
        }),
    ] = effects.as_slice()
    else {
        panic!("expected typed remember intent");
    };
    assert_eq!(content.as_str(), "Remember this exact preference.");
    let request_id = *request_id;
    assert!(model.memory_lifecycle_pending());
    assert!(update(&mut model, Message::Input(ctrl('s'))).is_empty());

    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentRejected {
            request_id,
            failure: UiFailure::new(
                ErrorClass::Validation,
                "memory validation rejected the draft",
                RetryPolicy::Never,
            ),
        }),
    );
    assert_eq!(model.overlay(), Some(OverlayKind::MemoryLifecycle));
    assert!(!model.memory_lifecycle_pending());
    assert!(text(&render(&model, 60, 18)).contains("Remember this exact preference."));

    let effects = update(&mut model, Message::Input(ctrl('s')));
    let [UiEffect::Dispatch(UiIntent::RememberMemory { request_id, .. })] = effects.as_slice()
    else {
        panic!("expected retried remember intent");
    };
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: *request_id,
        }),
    );
    assert!(model.overlay().is_none());
    assert_eq!(model.route(), Route::Memory);
    assert_eq!(model.focus, Focus::Memory);
}

#[test]
fn correction_review_and_proposal_decisions_carry_exact_revision_guards() {
    let mut correction = model();
    let _ = update(&mut correction, Message::Input(alt('6')));
    let _ = update(&mut correction, Message::Input(alt('e')));
    type_text(&mut correction, " Corrected.");
    let effects = update(&mut correction, Message::Input(ctrl('s')));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ReviseMemory {
            memory_id,
            expected_last_sequence: 11,
            content,
            ..
        })] if memory_id == "memory-concise" && content.as_str().ends_with(" Corrected.")
    ));

    let mut review = model();
    let _ = update(&mut review, Message::Input(alt('6')));
    let _ = update(&mut review, Message::Mouse(MouseAction::MemoryCycleStatus));
    let _ = update(
        &mut review,
        Message::Mouse(MouseAction::MemorySelect("memory-keyboard".to_owned())),
    );
    let _ = update(&mut review, Message::Input(alt('v')));
    assert_eq!(
        review.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Review)
    );
    let rendered = text(&render(&review, 80, 24));
    assert!(rendered.contains("Exact proposed content"));
    assert!(rendered.contains("user current-account"));
    assert!(rendered.contains("Observed preference"));
    assert!(rendered.contains("possible contradiction"));
    let compact = text(&render(&review, 40, 12));
    assert!(compact.contains("Approve"));
    assert!(compact.contains("Reject"));

    let effects = update(&mut review, Message::Input(key(Key::Char('a'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ApproveMemoryProposal {
            memory_id,
            expected_last_sequence: 17,
            proposal_revision_id,
            ..
        })] if memory_id == "memory-keyboard" && proposal_revision_id == "proposal-keyboard-1"
    ));

    let mut reject = model();
    let _ = update(&mut reject, Message::Input(alt('6')));
    let _ = update(&mut reject, Message::Mouse(MouseAction::MemoryCycleStatus));
    let _ = update(
        &mut reject,
        Message::Mouse(MouseAction::MemorySelect("memory-keyboard".to_owned())),
    );
    let _ = update(&mut reject, Message::Input(alt('v')));
    let effects = update(&mut reject, Message::Input(key(Key::Char('r'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::RejectMemoryProposal {
            memory_id,
            expected_last_sequence: 17,
            proposal_revision_id,
            ..
        })] if memory_id == "memory-keyboard" && proposal_revision_id == "proposal-keyboard-1"
    ));
}

#[test]
fn retract_delete_and_export_are_distinct_explicit_lifecycle_intents() {
    let mut retract = model();
    let _ = update(&mut retract, Message::Input(alt('6')));
    let _ = update(&mut retract, Message::Input(alt('x')));
    let retract_copy = text(&render(&retract, 60, 18));
    assert!(retract_copy.contains("future admission"));
    assert!(retract_copy.contains("cannot be recalled"));
    assert!(update(&mut retract, Message::Input(key(Key::Char('n')))).is_empty());
    assert!(retract.overlay().is_none());
    let _ = update(&mut retract, Message::Input(alt('x')));
    let effects = update(&mut retract, Message::Input(key(Key::Char('y'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::RetractMemory {
            memory_id,
            expected_last_sequence: 11,
            revision_id,
            ..
        })] if memory_id == "memory-concise" && revision_id == "revision-concise-3"
    ));

    let mut delete = model();
    let _ = update(&mut delete, Message::Input(alt('6')));
    let _ = update(&mut delete, Message::Input(alt('d')));
    assert!(update(&mut delete, Message::Input(key(Key::Char('q')))).is_empty());
    let delete_copy = text(&render(&delete, 60, 18));
    assert!(delete_copy.contains("Logical delete"));
    assert!(delete_copy.contains("retraction only"));
    assert!(delete_copy.contains("cannot be recalled"));
    let effects = update(&mut delete, Message::Input(key(Key::Char('y'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::DeleteMemory {
            memory_id,
            expected_last_sequence: 11,
            ..
        })] if memory_id == "memory-concise"
    ));

    let mut export = model();
    let _ = update(&mut export, Message::Input(alt('6')));
    let _ = update(&mut export, Message::Input(alt('s')));
    let effects = update(&mut export, Message::Input(key(Key::Enter)));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::ExportMemory { memory_id, .. })]
            if memory_id == "memory-concise"
    ));
}

#[test]
fn lifecycle_metadata_remains_actionable_without_an_erasable_content_sidecar() {
    let mut model = model();
    let summary = MemorySummary::new(
        "memory-erased-sidecar",
        "Content unavailable; lifecycle metadata retained.",
        MemoryStatus::Active,
        MemoryScope::Workspace,
        1_725_000_000_000,
        Some(10_000),
        0,
    )
    .expect("summary");
    let detail = MemoryDetail::metadata_only(
        "memory-erased-sidecar",
        5,
        "explicit user memory",
        MemoryTrust::UserApproved,
        1_725_000_000_000,
        None,
        vec![],
    )
    .expect("metadata-only detail")
    .with_revision_context(
        MemoryRevisionContext::new(
            29,
            "revision-erased-5",
            None,
            "workspace current-project",
            MemoryOrigin::ExplicitUser,
            MemorySensitivity::Internal,
            vec![],
            vec![],
            vec![],
        )
        .expect("revision context"),
    );
    model.apply_memory(Arc::new(
        MemoryProjection::ready(29, vec![summary], vec![detail], 1, false)
            .expect("metadata-only projection"),
    ));
    let _ = update(&mut model, Message::Input(alt('6')));
    let _ = update(&mut model, Message::Input(alt('s')));
    let effects = update(&mut model, Message::Input(key(Key::Enter)));
    let [UiEffect::Dispatch(UiIntent::ExportMemory { request_id, .. })] = effects.as_slice() else {
        panic!("metadata-only export should dispatch");
    };
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted {
            request_id: *request_id,
        }),
    );
    let _ = update(&mut model, Message::Input(alt('e')));
    assert!(model.overlay().is_none(), "revision requires exact content");
    let _ = update(&mut model, Message::Input(alt('x')));
    assert_eq!(
        model.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Retract)
    );
    assert!(text(&render(&model, 60, 18)).contains("content sidecar unavailable"));
    let effects = update(&mut model, Message::Input(key(Key::Char('y'))));
    let request_id = match effects.as_slice() {
        [
            UiEffect::Dispatch(UiIntent::RetractMemory {
                request_id,
                expected_last_sequence: 29,
                revision_id,
                ..
            }),
        ] if revision_id == "revision-erased-5" => *request_id,
        _ => panic!("metadata-only retract should dispatch"),
    };
    let _ = update(
        &mut model,
        Message::Notice(UiNotice::IntentCommitted { request_id }),
    );
    let _ = update(&mut model, Message::Input(alt('d')));
    let effects = update(&mut model, Message::Input(key(Key::Char('y'))));
    assert!(matches!(
        effects.as_slice(),
        [UiEffect::Dispatch(UiIntent::DeleteMemory {
            expected_last_sequence: 29,
            ..
        })]
    ));
}

#[test]
fn lifecycle_actions_and_modal_controls_are_measured_at_every_target_size() {
    for (width, height) in [(120, 50), (80, 24), (60, 18), (40, 12)] {
        let mut model = model();
        let _ = update(&mut model, Message::Input(alt('6')));
        let mut actions = Vec::new();
        for column in 0..width {
            for row in 0..height {
                if let Some(action) = hit_test(&model, width, height, column, row) {
                    actions.push(action);
                }
            }
        }
        assert!(actions.contains(&MouseAction::MemoryRemember));
        assert!(actions.contains(&MouseAction::MemoryActions));

        let _ = update(&mut model, Message::Mouse(MouseAction::MemoryActions));
        let rendered = text(&render(&model, width, height));
        assert!(rendered.contains("Memory actions"));
        let mut overlay_actions = Vec::new();
        for column in 0..width {
            for row in 0..height {
                if let Some(action) = hit_test(&model, width, height, column, row) {
                    overlay_actions.push(action);
                }
            }
        }
        assert!(overlay_actions.contains(&MouseAction::MemoryLifecycleSubmit));
        assert!(overlay_actions.contains(&MouseAction::MemoryLifecycleCancel));
        assert!(
            overlay_actions
                .iter()
                .any(|action| { matches!(action, MouseAction::MemoryActionSelect(_)) })
        );
    }
}

#[test]
fn slash_commands_and_settings_cross_link_converge_on_memory_workflows() {
    let mut remember = model();
    type_text(&mut remember, "/remember");
    let effects = update(&mut remember, Message::Input(key(Key::Enter)));
    assert!(effects.is_empty());
    assert_eq!(remember.route(), Route::Memory);
    assert_eq!(
        remember.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Remember)
    );

    let mut export = model();
    type_text(&mut export, "/memory-export");
    let effects = update(&mut export, Message::Input(key(Key::Enter)));
    assert!(effects.is_empty());
    assert_eq!(export.route(), Route::Memory);
    assert_eq!(
        export.memory_lifecycle_mode(),
        Some(MemoryLifecycleMode::Export)
    );

    let mut settings = model();
    let _ = update(&mut settings, Message::Input(alt('4')));
    let _ = update(&mut settings, Message::Mouse(MouseAction::SettingsTab(6)));
    let _ = update(&mut settings, Message::Mouse(MouseAction::SettingsRow(3)));
    let _ = update(&mut settings, Message::Input(key(Key::Enter)));
    assert_eq!(settings.route(), Route::Memory);
    assert_eq!(settings.focus, Focus::Memory);
}

#[test]
fn ascii_no_color_reduced_motion_lifecycle_overlay_keeps_redundant_semantics() {
    let mut model = model();
    let resolved = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            r#"{
                "schema_version": 4,
                "local_profile": { "preferences": {
                    "glyph_mode": "ascii",
                    "color_mode": "no_color",
                    "reduced_motion": true
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
    let _ = update(&mut model, Message::Input(alt('n')));
    let backend = render(&model, 40, 12);
    let rendered = text(&backend);
    assert!(rendered.is_ascii());
    assert!(rendered.contains("Scope: Workspace"));
    assert!(rendered.contains("Sensitivity: Internal"));
    assert!(rendered.contains("rejects secrets"));
    assert!(!model.motion().animating());
    assert!(
        backend
            .buffer()
            .content
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
}

#[test]
#[ignore = "visual review harness for the complete Memory lifecycle workspace"]
fn render_memory_review_matrix() {
    let mut workspace = model();
    let _ = update(&mut workspace, Message::Input(alt('6')));
    for (width, height) in [(120, 50), (80, 24), (60, 18), (40, 12)] {
        println!(
            "=== Memory {width}x{height} ===\n{}",
            text(&render(&workspace, width, height))
        );
    }
    let _ = update(&mut workspace, Message::Input(key(Key::Enter)));
    println!(
        "=== Memory detail 40x12 ===\n{}",
        text(&render(&workspace, 40, 12))
    );
    let _ = update(
        &mut workspace,
        Message::Mouse(MouseAction::MemoryAdmissions),
    );
    println!(
        "=== Memory admissions 60x18 ===\n{}",
        text(&render(&workspace, 60, 18))
    );

    let mut remember = model();
    let _ = update(&mut remember, Message::Input(alt('6')));
    let _ = update(&mut remember, Message::Input(alt('n')));
    type_text(&mut remember, "A carefully reviewed workspace fact.");
    for (width, height) in [(120, 50), (80, 24), (60, 18), (40, 12)] {
        println!(
            "=== Remember {width}x{height} ===\n{}",
            text(&render(&remember, width, height))
        );
    }

    let mut review = model();
    let _ = update(&mut review, Message::Input(alt('6')));
    let _ = update(&mut review, Message::Mouse(MouseAction::MemoryCycleStatus));
    let _ = update(
        &mut review,
        Message::Mouse(MouseAction::MemorySelect("memory-keyboard".to_owned())),
    );
    let _ = update(&mut review, Message::Input(alt('v')));
    for (width, height) in [(80, 24), (40, 12)] {
        println!(
            "=== Proposal review {width}x{height} ===\n{}",
            text(&render(&review, width, height))
        );
    }

    let mut delete = model();
    let _ = update(&mut delete, Message::Input(alt('6')));
    let _ = update(&mut delete, Message::Input(alt('d')));
    println!(
        "=== Logical delete 40x12 ===\n{}",
        text(&render(&delete, 40, 12))
    );
}
