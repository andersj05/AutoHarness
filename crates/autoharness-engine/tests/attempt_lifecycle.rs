use autoharness_domain::{
    AttemptFailure, AttemptId, CommandEnvelope, CommandId, CommandPayload, CorrelationId,
    DeliveryMode, ErrorClass, ErrorCode, EventEnvelope, EventId, InputId, ModelId, ModelRef,
    PromptText, ProviderId, PublicMessage, ResponseText, RetryAdvice, SessionId, TimestampMillis,
    UsageSnapshot,
};
use autoharness_engine::{
    AttemptStatus, CommandRejection, EngineError, EventMetadataSource, GeneratedEventMetadata,
    InMemoryEngine,
};

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

fn session_id() -> SessionId {
    SessionId::new("session-attempts").expect("valid session ID")
}

fn input_id() -> InputId {
    InputId::new("input-1").expect("valid input ID")
}

fn attempt_id(value: &str) -> AttemptId {
    AttemptId::new(value).expect("valid attempt ID")
}

fn model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("valid provider ID"),
        ModelId::new("models/gemini-test").expect("valid model ID"),
    )
}

fn command(name: &str, payload: CommandPayload) -> CommandEnvelope {
    CommandEnvelope::new(
        CommandId::new(format!("command-{name}")).expect("valid command ID"),
        CorrelationId::new(format!("correlation-{name}")).expect("valid correlation ID"),
        payload,
    )
}

fn execute(engine: &mut InMemoryEngine<CounterMetadata>, name: &str, payload: CommandPayload) {
    engine
        .execute(&command(name, payload))
        .expect("command should succeed");
}

fn create_selected_session(engine: &mut InMemoryEngine<CounterMetadata>) {
    execute(
        engine,
        "create",
        CommandPayload::CreateSession {
            session_id: session_id(),
        },
    );
    execute(
        engine,
        "select",
        CommandPayload::SelectModel {
            session_id: session_id(),
            model: model(),
        },
    );
    execute(
        engine,
        "admit",
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: input_id(),
            prompt: PromptText::new("hello\n世界").expect("valid prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );
}

#[test]
fn admission_and_first_attempt_prepare_commit_as_one_replayable_batch() {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
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
    let attempt = attempt_id("attempt-atomic");
    let command = command(
        "admit-prepare",
        CommandPayload::AdmitPromptAndPrepareAttempt {
            session_id: session_id(),
            input_id: input_id(),
            prompt: PromptText::new("atomic prompt").expect("valid prompt"),
            delivery_mode: DeliveryMode::NextTurn,
            attempt_id: attempt.clone(),
        },
    );

    let batch = engine.execute(&command).expect("atomic command succeeds");

    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].sequence().get(), 3);
    assert_eq!(batch[1].sequence().get(), 4);
    assert_eq!(
        batch[0].causation(),
        &autoharness_domain::Causation::Command(command.command_id().clone())
    );
    assert_eq!(
        batch[1].causation(),
        &autoharness_domain::Causation::Event(batch[0].event_id().clone())
    );
    let live = engine.session(&session_id()).expect("live session");
    assert_eq!(live.admitted_inputs()[0].promoted_by(), Some(&attempt));
    assert_eq!(
        live.attempt(&attempt).expect("prepared attempt").status(),
        AttemptStatus::Prepared
    );

    let replayed = InMemoryEngine::replay(CounterMetadata::new(100), engine.events().to_vec())
        .expect("batch history replays");
    assert_eq!(replayed.session(&session_id()), Some(live));
}

#[test]
fn failed_attempt_can_be_retried_and_replay_matches_live_projection() {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
    create_selected_session(&mut engine);
    let first = attempt_id("attempt-1");
    let retry = attempt_id("attempt-2");

    execute(
        &mut engine,
        "prepare-1",
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: first.clone(),
            input_id: input_id(),
            retry_of: None,
        },
    );
    execute(
        &mut engine,
        "start-1",
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: first.clone(),
        },
    );
    execute(
        &mut engine,
        "text-1",
        CommandPayload::AppendAttemptText {
            session_id: session_id(),
            attempt_id: first.clone(),
            text: ResponseText::new("partial ").expect("non-empty response"),
        },
    );
    execute(
        &mut engine,
        "usage-1",
        CommandPayload::RecordAttemptUsage {
            session_id: session_id(),
            attempt_id: first.clone(),
            usage: UsageSnapshot::new(Some(3), Some(2), Some(5)),
        },
    );
    execute(
        &mut engine,
        "fail-1",
        CommandPayload::FailAttempt {
            session_id: session_id(),
            attempt_id: first.clone(),
            failure: AttemptFailure::new(
                ErrorClass::Unavailable,
                ErrorCode::new("provider_unavailable").expect("valid code"),
                PublicMessage::new("The provider is temporarily unavailable")
                    .expect("valid message"),
                RetryAdvice::Backoff,
            ),
        },
    );
    execute(
        &mut engine,
        "prepare-2",
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: retry.clone(),
            input_id: input_id(),
            retry_of: Some(first.clone()),
        },
    );
    execute(
        &mut engine,
        "start-2",
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: retry.clone(),
        },
    );
    execute(
        &mut engine,
        "text-2",
        CommandPayload::AppendAttemptText {
            session_id: session_id(),
            attempt_id: retry.clone(),
            text: ResponseText::new("complete 世界").expect("non-empty response"),
        },
    );
    execute(
        &mut engine,
        "complete-2",
        CommandPayload::CompleteAttempt {
            session_id: session_id(),
            attempt_id: retry.clone(),
        },
    );

    let live = engine.session(&session_id()).expect("live session").clone();
    assert_eq!(live.attempts().len(), 2);
    assert_eq!(
        live.attempt(&first).expect("first attempt").status(),
        AttemptStatus::Failed
    );
    let retried = live.attempt(&retry).expect("retry attempt");
    assert_eq!(retried.retry_of(), Some(&first));
    assert_eq!(retried.status(), AttemptStatus::Completed);
    assert_eq!(retried.response_text(), "complete 世界");

    let serialized = serde_json::to_vec(engine.events()).expect("serialize history");
    let decoded: Vec<EventEnvelope> =
        serde_json::from_slice(&serialized).expect("deserialize history");
    let replayed =
        InMemoryEngine::replay(CounterMetadata::new(100), decoded).expect("history should replay");
    assert_eq!(replayed.session(&session_id()), Some(&live));
}

#[test]
fn completion_wins_a_cancellation_race_and_late_cancel_is_rejected_atomically() {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
    create_selected_session(&mut engine);
    let attempt = attempt_id("attempt-cancel-race");
    execute(
        &mut engine,
        "prepare",
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: attempt.clone(),
            input_id: input_id(),
            retry_of: None,
        },
    );
    execute(
        &mut engine,
        "start",
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    execute(
        &mut engine,
        "request-cancel",
        CommandPayload::RequestAttemptCancellation {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    execute(
        &mut engine,
        "complete",
        CommandPayload::CompleteAttempt {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    let event_count = engine.events().len();

    let rejection = engine
        .execute(&command(
            "late-cancel",
            CommandPayload::CancelAttempt {
                session_id: session_id(),
                attempt_id: attempt.clone(),
            },
        ))
        .expect_err("terminal attempt cannot settle twice");

    assert!(matches!(
        rejection,
        EngineError::CommandRejected(CommandRejection::InvalidAttemptState { .. })
    ));
    assert_eq!(engine.events().len(), event_count);
    assert_eq!(
        engine
            .session(&session_id())
            .expect("session")
            .attempt(&attempt)
            .expect("attempt")
            .status(),
        AttemptStatus::Completed
    );
}

#[test]
fn attempt_preparation_requires_a_selected_model_without_consuming_metadata() {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
    execute(
        &mut engine,
        "create",
        CommandPayload::CreateSession {
            session_id: session_id(),
        },
    );
    execute(
        &mut engine,
        "admit",
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: input_id(),
            prompt: PromptText::new("hello").expect("valid prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );
    let event_count = engine.events().len();

    let rejection = engine
        .execute(&command(
            "prepare",
            CommandPayload::PrepareAttempt {
                session_id: session_id(),
                attempt_id: attempt_id("attempt-1"),
                input_id: input_id(),
                retry_of: None,
            },
        ))
        .expect_err("model selection is required");

    assert!(matches!(
        rejection,
        EngineError::CommandRejected(CommandRejection::ModelNotSelected { .. })
    ));
    assert_eq!(engine.events().len(), event_count);
}
