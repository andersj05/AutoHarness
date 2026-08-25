use autoharness_domain::{
    AttemptFailure, AttemptId, CapabilityKind, CapabilityRequest, Causation, CommandEnvelope,
    CommandId, CommandPayload, CorrelationId, DeliveryMode, ErrorClass, ErrorCode, EventEnvelope,
    EventId, EventPayload, InputId, ModelId, ModelRef, PermissionAnswer, PermissionDecisionId,
    PermissionOutcome, PromptText, ProviderCallId, ProviderId, PublicMessage, ResourceRef,
    ResponseText, RetryAdvice, RunLimits, SessionId, TimestampMillis, ToolArguments, ToolCallId,
    ToolCallSpec, ToolName, ToolOutput, UsageSnapshot,
};
use autoharness_engine::{
    AttemptStatus, CommandRejection, EngineError, EventMetadataSource, GeneratedEventMetadata,
    InMemoryEngine, ReplayError, ToolCallStatus,
};

#[derive(Debug)]
struct CounterMetadata {
    next: u64,
}

#[test]
fn tool_effect_requires_frozen_permission_and_resumes_after_durable_settlement() {
    let mut engine = InMemoryEngine::new(CounterMetadata::new(1));
    create_selected_session(&mut engine);
    let attempt = attempt_id("attempt-tool");
    execute(
        &mut engine,
        "prepare-tool",
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: attempt.clone(),
            input_id: input_id(),
            retry_of: None,
        },
    );
    execute(
        &mut engine,
        "budget-tool",
        CommandPayload::ConfigureRunBudget {
            session_id: session_id(),
            attempt_id: attempt.clone(),
            limits: RunLimits::default(),
        },
    );
    execute(
        &mut engine,
        "start-tool-attempt",
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    execute(
        &mut engine,
        "turn-one",
        CommandPayload::StartRunTurn {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    let tool_call_id = ToolCallId::new("tool-call-1").expect("tool-call ID");
    let call = ToolCallSpec {
        tool_call_id: tool_call_id.clone(),
        provider_call_id: ProviderCallId::new("provider-call-1").expect("provider call ID"),
        tool_name: ToolName::new("fs_write").expect("tool name"),
        schema_version: 1,
        arguments: ToolArguments::new(serde_json::json!({"path":"a","content":"b"}))
            .expect("arguments"),
        capability: CapabilityRequest {
            kind: CapabilityKind::FilesystemWrite,
            resource: ResourceRef::new("workspace:a").expect("resource"),
        },
    };
    execute(
        &mut engine,
        "propose-tool",
        CommandPayload::ProposeToolCall {
            session_id: session_id(),
            attempt_id: attempt.clone(),
            call,
        },
    );
    execute(
        &mut engine,
        "ask-tool",
        CommandPayload::RecordToolPermission {
            session_id: session_id(),
            tool_call_id: tool_call_id.clone(),
            decision_id: PermissionDecisionId::new("permission-policy").expect("decision ID"),
            outcome: PermissionOutcome::Ask,
        },
    );

    let terminal_with_pending_tool = engine
        .execute(&command(
            "complete-with-pending-tool",
            CommandPayload::CompleteAttempt {
                session_id: session_id(),
                attempt_id: attempt.clone(),
            },
        ))
        .expect_err("parent cannot settle while a tool remains pending");
    assert!(matches!(
        terminal_with_pending_tool,
        EngineError::CommandRejected(CommandRejection::InvalidAttemptState { .. })
    ));

    let unknown_with_pending_tool = engine
        .execute(&command(
            "unknown-with-pending-tool",
            CommandPayload::MarkAttemptUnknown {
                session_id: session_id(),
                attempt_id: attempt.clone(),
            },
        ))
        .expect_err("unknown parent cannot retain a live tool");
    assert!(matches!(
        unknown_with_pending_tool,
        EngineError::CommandRejected(CommandRejection::InvalidAttemptState { .. })
    ));

    let mut invalid_history = engine.events().to_vec();
    let prior = invalid_history.last().expect("prior event");
    invalid_history.push(EventEnvelope::new_v1(
        EventId::new("event-invalid-unknown").expect("event ID"),
        session_id(),
        prior.sequence().checked_next().expect("next sequence"),
        TimestampMillis::new(99),
        Causation::Event(prior.event_id().clone()),
        prior.correlation_id().clone(),
        EventPayload::AttemptMarkedUnknown {
            attempt_id: attempt.clone(),
        },
    ));
    let replay_error = InMemoryEngine::replay(CounterMetadata::new(100), invalid_history)
        .expect_err("replay rejects unknown parent with a live tool");
    assert!(matches!(
        replay_error,
        ReplayError::IllegalAttemptTransition { .. }
    ));

    let rejected = engine
        .execute(&command(
            "start-without-answer",
            CommandPayload::StartToolCall {
                session_id: session_id(),
                tool_call_id: tool_call_id.clone(),
            },
        ))
        .expect_err("ask is not execution authority");
    assert!(matches!(
        rejected,
        EngineError::CommandRejected(CommandRejection::InvalidToolCallState { .. })
    ));

    execute(
        &mut engine,
        "answer-tool",
        CommandPayload::AnswerToolPermission {
            session_id: session_id(),
            tool_call_id: tool_call_id.clone(),
            decision_id: PermissionDecisionId::new("permission-human").expect("decision ID"),
            answer: PermissionAnswer::AllowOnce,
        },
    );
    execute(
        &mut engine,
        "start-exact-tool",
        CommandPayload::StartToolCall {
            session_id: session_id(),
            tool_call_id: tool_call_id.clone(),
        },
    );
    let failure_with_running_tool = engine
        .execute(&command(
            "fail-with-running-tool",
            CommandPayload::FailAttempt {
                session_id: session_id(),
                attempt_id: attempt.clone(),
                failure: AttemptFailure::new(
                    ErrorClass::Protocol,
                    ErrorCode::new("provider_failed").expect("error code"),
                    PublicMessage::new("The provider failed").expect("message"),
                    RetryAdvice::Never,
                ),
            },
        ))
        .expect_err("parent cannot fail while a tool is running");
    assert!(matches!(
        failure_with_running_tool,
        EngineError::CommandRejected(CommandRejection::InvalidAttemptState { .. })
    ));
    execute(
        &mut engine,
        "pause-for-tool",
        CommandPayload::PauseAttemptForTools {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    execute(
        &mut engine,
        "complete-tool",
        CommandPayload::CompleteToolCall {
            session_id: session_id(),
            tool_call_id: tool_call_id.clone(),
            output: ToolOutput::new("ok", None, 2, false).expect("output"),
        },
    );
    execute(
        &mut engine,
        "resume-tool",
        CommandPayload::ResumeAttemptAfterTools {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );
    execute(
        &mut engine,
        "turn-two",
        CommandPayload::StartRunTurn {
            session_id: session_id(),
            attempt_id: attempt.clone(),
        },
    );

    let live = engine.session(&session_id()).expect("session").clone();
    assert_eq!(live.attempt(&attempt).expect("attempt").turns_started(), 2);
    assert_eq!(
        live.tool_call(&tool_call_id).expect("tool call").status(),
        ToolCallStatus::Completed
    );
    let replayed = InMemoryEngine::replay(CounterMetadata::new(100), engine.events().to_vec())
        .expect("replay");
    assert_eq!(replayed.session(&session_id()), Some(&live));
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
fn admission_title_and_first_attempt_prepare_commit_as_one_replayable_batch() {
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

    assert_eq!(batch.len(), 3);
    assert!(matches!(
        batch[0].payload(),
        EventPayload::InputAdmitted { .. }
    ));
    assert!(matches!(
        batch[1].payload(),
        EventPayload::SessionRenamed { title } if title.as_str() == "atomic prompt"
    ));
    assert!(matches!(
        batch[2].payload(),
        EventPayload::AttemptPrepared { .. }
    ));
    assert_eq!(batch[0].sequence().get(), 3);
    assert_eq!(batch[1].sequence().get(), 4);
    assert_eq!(batch[2].sequence().get(), 5);
    assert_eq!(
        batch[0].causation(),
        &autoharness_domain::Causation::Command(command.command_id().clone())
    );
    assert_eq!(
        batch[1].causation(),
        &autoharness_domain::Causation::Event(batch[0].event_id().clone())
    );
    assert_eq!(
        batch[2].causation(),
        &autoharness_domain::Causation::Event(batch[1].event_id().clone())
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
