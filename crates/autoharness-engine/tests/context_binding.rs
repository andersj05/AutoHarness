use autoharness_domain::{
    AttemptId, Causation, CommandEnvelope, CommandId, CommandPayload, ContextTurnId, CorrelationId,
    DeliveryMode, EventEnvelope, EventId, EventPayload, InputId, ModelId, ModelRef, PromptText,
    ProviderId, RunLimits, SessionId, Sha256Digest, TimestampMillis,
};
use autoharness_engine::{
    CommandRejection, EngineError, EventMetadataSource, GeneratedEventMetadata, InMemoryEngine,
    ReplayError,
};

#[derive(Debug)]
struct CounterMetadata {
    next: u64,
}

impl CounterMetadata {
    const fn new(next: u64) -> Self {
        Self { next }
    }
}

impl EventMetadataSource for CounterMetadata {
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata {
        let current = self.next;
        self.next += 1;
        GeneratedEventMetadata::new(
            EventId::new(format!("event-{current}")).expect("valid event ID"),
            TimestampMillis::new(current as i64),
        )
    }
}

#[test]
fn next_turn_requires_an_exact_binding_and_restart_preserves_the_boundary() {
    let (mut engine, attempt_id) = started_attempt();
    let before = engine.events().to_vec();

    let missing = engine
        .execute(&command(
            "start-without-context",
            CommandPayload::StartRunTurn {
                session_id: session_id(),
                attempt_id: attempt_id.clone(),
            },
        ))
        .expect_err("provider turn cannot start without durable context");
    assert!(matches!(
        missing,
        EngineError::CommandRejected(CommandRejection::InvalidContextTurnBinding { .. })
    ));
    assert_eq!(engine.events(), before);

    let wrong_turn = engine
        .execute(&bind_command(
            "wrong-turn",
            &attempt_id,
            2,
            "context-turn-2",
            '2',
        ))
        .expect_err("binding must target the exact next turn");
    assert!(matches!(
        wrong_turn,
        EngineError::CommandRejected(CommandRejection::InvalidContextTurnBinding { .. })
    ));

    engine
        .execute(&bind_command(
            "bind-turn-one",
            &attempt_id,
            1,
            "context-turn-1",
            'a',
        ))
        .expect("bind exact first turn");
    let live_attempt = engine
        .session(&session_id())
        .and_then(|session| session.attempt(&attempt_id))
        .expect("attempt projection");
    let pending = live_attempt
        .pending_context_turn()
        .expect("pending binding");
    assert_eq!(pending.run_turn(), 1);
    assert_eq!(pending.context_turn_id().as_str(), "context-turn-1");
    assert_eq!(pending.manifest_hash(), &digest('a'));
    assert!(live_attempt.is_provider_dispatch_ready());

    let encoded = serde_json::to_vec(engine.events()).expect("serialize restart history");
    let restored: Vec<EventEnvelope> =
        serde_json::from_slice(&encoded).expect("deserialize restart history");
    let mut restarted =
        InMemoryEngine::replay(CounterMetadata::new(100), restored).expect("replay bound turn");

    let emitted = restarted
        .execute(&command(
            "start-after-restart",
            CommandPayload::StartRunTurn {
                session_id: session_id(),
                attempt_id: attempt_id.clone(),
            },
        ))
        .expect("restart can consume the exact pending binding");
    assert!(matches!(
        emitted.as_slice(),
        [event] if matches!(
            event.payload(),
            EventPayload::RunTurnStarted { attempt_id: found, turn: 1 }
                if found == &attempt_id
        )
    ));
    let restarted_attempt = restarted
        .session(&session_id())
        .and_then(|session| session.attempt(&attempt_id))
        .expect("restarted attempt projection");
    assert_eq!(restarted_attempt.turns_started(), 1);
    assert!(restarted_attempt.pending_context_turn().is_none());
    assert!(!restarted_attempt.is_provider_dispatch_ready());
    assert_eq!(restarted_attempt.context_turn_bindings().len(), 1);

    let repeated = restarted
        .execute(&command(
            "repeat-without-resume",
            CommandPayload::StartRunTurn {
                session_id: session_id(),
                attempt_id,
            },
        ))
        .expect_err("another turn requires a tool continuation boundary");
    assert!(matches!(
        repeated,
        EngineError::CommandRejected(CommandRejection::InvalidContextTurnBinding { .. })
    ));
}

#[test]
fn duplicate_conflicting_and_non_adjacent_bindings_fail_closed() {
    let (mut engine, attempt_id) = started_attempt();
    engine
        .execute(&bind_command(
            "bind-turn-one",
            &attempt_id,
            1,
            "context-turn-1",
            'a',
        ))
        .expect("bind exact first turn");
    let bound_history = engine.events().to_vec();

    let duplicate = engine
        .execute(&bind_command(
            "duplicate-identity",
            &attempt_id,
            1,
            "context-turn-1",
            'b',
        ))
        .expect_err("context identity cannot bind conflicting manifest bytes");
    assert!(matches!(
        duplicate,
        EngineError::CommandRejected(CommandRejection::DuplicateContextTurn { .. })
    ));
    let conflicting = engine
        .execute(&bind_command(
            "conflicting-binding",
            &attempt_id,
            1,
            "context-turn-other",
            'b',
        ))
        .expect_err("one run turn accepts one binding");
    assert!(matches!(
        conflicting,
        EngineError::CommandRejected(CommandRejection::InvalidContextTurnBinding { .. })
    ));
    assert_eq!(engine.events(), bound_history);

    engine
        .execute(&command(
            "intervening-model-selection",
            CommandPayload::SelectModel {
                session_id: session_id(),
                model: model(),
            },
        ))
        .expect("unrelated session event remains valid");
    let non_adjacent = engine
        .execute(&command(
            "non-adjacent-start",
            CommandPayload::StartRunTurn {
                session_id: session_id(),
                attempt_id,
            },
        ))
        .expect_err("binding must immediately precede the run boundary");
    assert!(matches!(
        non_adjacent,
        EngineError::CommandRejected(CommandRejection::InvalidContextTurnBinding { .. })
    ));
}

#[test]
fn replay_rejects_missing_wrong_and_duplicate_context_boundaries() {
    let (engine, attempt_id) = started_attempt();
    let base = engine.events().to_vec();
    let missing_binding = with_next_event(
        base.clone(),
        "event-invalid-start",
        EventPayload::RunTurnStarted {
            attempt_id: attempt_id.clone(),
            turn: 1,
        },
    );
    assert!(matches!(
        InMemoryEngine::replay(CounterMetadata::new(100), missing_binding),
        Err(ReplayError::IllegalContextTurnBinding { .. })
    ));

    let wrong_turn = with_next_event(
        base,
        "event-invalid-binding",
        EventPayload::ContextTurnBound {
            attempt_id: attempt_id.clone(),
            run_turn: 2,
            context_turn_id: context_turn_id("context-turn-2"),
            manifest_hash: digest('2'),
        },
    );
    assert!(matches!(
        InMemoryEngine::replay(CounterMetadata::new(100), wrong_turn),
        Err(ReplayError::IllegalContextTurnBinding { .. })
    ));

    let (mut bound_engine, bound_attempt_id) = started_attempt();
    bound_engine
        .execute(&bind_command(
            "bind-for-duplicate-replay",
            &bound_attempt_id,
            1,
            "context-turn-duplicate",
            'd',
        ))
        .expect("bind context");
    let duplicate = with_next_event(
        bound_engine.events().to_vec(),
        "event-duplicate-binding",
        EventPayload::ContextTurnBound {
            attempt_id: bound_attempt_id,
            run_turn: 1,
            context_turn_id: context_turn_id("context-turn-duplicate"),
            manifest_hash: digest('e'),
        },
    );
    assert!(matches!(
        InMemoryEngine::replay(CounterMetadata::new(100), duplicate),
        Err(ReplayError::DuplicateContextTurn { .. })
    ));
}

fn started_attempt() -> (InMemoryEngine<CounterMetadata>, AttemptId) {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
    let attempt_id = AttemptId::new("attempt-context").expect("attempt ID");
    execute(
        &mut engine,
        "create",
        CommandPayload::CreateSession {
            session_id: session_id(),
        },
    );
    execute(
        &mut engine,
        "select",
        CommandPayload::SelectModel {
            session_id: session_id(),
            model: model(),
        },
    );
    execute(
        &mut engine,
        "admit",
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: InputId::new("input-context").expect("input ID"),
            prompt: PromptText::new("remember this").expect("prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );
    execute(
        &mut engine,
        "prepare",
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: attempt_id.clone(),
            input_id: InputId::new("input-context").expect("input ID"),
            retry_of: None,
        },
    );
    execute(
        &mut engine,
        "budget",
        CommandPayload::ConfigureRunBudget {
            session_id: session_id(),
            attempt_id: attempt_id.clone(),
            limits: RunLimits::default(),
        },
    );
    execute(
        &mut engine,
        "start-attempt",
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: attempt_id.clone(),
        },
    );
    (engine, attempt_id)
}

fn bind_command(
    command_name: &str,
    attempt_id: &AttemptId,
    run_turn: u32,
    context_turn: &str,
    digest_character: char,
) -> CommandEnvelope {
    command(
        command_name,
        CommandPayload::BindContextTurn {
            session_id: session_id(),
            attempt_id: attempt_id.clone(),
            run_turn,
            context_turn_id: context_turn_id(context_turn),
            manifest_hash: digest(digest_character),
        },
    )
}

fn with_next_event(
    mut events: Vec<EventEnvelope>,
    event_id: &str,
    payload: EventPayload,
) -> Vec<EventEnvelope> {
    let prior = events.last().expect("base event");
    events.push(EventEnvelope::new_v1(
        EventId::new(event_id).expect("event ID"),
        session_id(),
        prior.sequence().checked_next().expect("next sequence"),
        TimestampMillis::new(10_000),
        Causation::Event(prior.event_id().clone()),
        CorrelationId::new("correlation-forged").expect("correlation ID"),
        payload,
    ));
    events
}

fn execute(engine: &mut InMemoryEngine<CounterMetadata>, name: &str, payload: CommandPayload) {
    engine
        .execute(&command(name, payload))
        .expect("command should succeed");
}

fn command(name: &str, payload: CommandPayload) -> CommandEnvelope {
    CommandEnvelope::new(
        CommandId::new(format!("command-{name}")).expect("command ID"),
        CorrelationId::new(format!("correlation-{name}")).expect("correlation ID"),
        payload,
    )
}

fn session_id() -> SessionId {
    SessionId::new("session-context").expect("session ID")
}

fn model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("provider-test").expect("provider ID"),
        ModelId::new("model-test").expect("model ID"),
    )
}

fn context_turn_id(value: &str) -> ContextTurnId {
    ContextTurnId::new(value).expect("context turn ID")
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
}
