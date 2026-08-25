use std::collections::VecDeque;

use autoharness_domain::{
    Causation, CommandEnvelope, CommandId, CommandPayload, CorrelationId, DeliveryMode,
    EventEnvelope, EventId, EventPayload, InputId, ModelId, ModelRef, PromptText, ProviderId,
    SessionId, SessionSequence, TimestampMillis,
};
use autoharness_engine::{
    CommandRejection, EngineError, EventMetadataSource, GeneratedEventMetadata, InMemoryEngine,
    ReplayError, SessionAggregate,
};

#[derive(Debug)]
struct ScriptedMetadata {
    values: VecDeque<GeneratedEventMetadata>,
}

impl ScriptedMetadata {
    fn new(values: impl IntoIterator<Item = (EventId, i64)>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|(event_id, timestamp)| {
                    GeneratedEventMetadata::new(event_id, TimestampMillis::new(timestamp))
                })
                .collect(),
        }
    }
}

impl EventMetadataSource for ScriptedMetadata {
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata {
        self.values
            .pop_front()
            .expect("test supplied enough event metadata")
    }
}

fn session_id() -> SessionId {
    SessionId::new("session-1").expect("valid test ID")
}

fn correlation_id() -> CorrelationId {
    CorrelationId::new("correlation-1").expect("valid test ID")
}

fn event_id(value: &str) -> EventId {
    EventId::new(value).expect("valid test ID")
}

fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("valid test ID")
}

fn input_id(value: &str) -> InputId {
    InputId::new(value).expect("valid test ID")
}

fn model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("valid provider ID"),
        ModelId::new("models/gemini-test").expect("valid model ID"),
    )
}

fn command(command_id: &str, payload: CommandPayload) -> CommandEnvelope {
    CommandEnvelope::new(self::command_id(command_id), correlation_id(), payload)
}

fn create_command() -> CommandEnvelope {
    command(
        "command-create",
        CommandPayload::CreateSession {
            session_id: session_id(),
        },
    )
}

fn select_command() -> CommandEnvelope {
    command(
        "command-select",
        CommandPayload::SelectModel {
            session_id: session_id(),
            model: model(),
        },
    )
}

fn admit_command(command_id: &str, input: &str, prompt: &str) -> CommandEnvelope {
    command(
        command_id,
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: input_id(input),
            prompt: PromptText::new(prompt).expect("non-empty test prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    )
}

fn scripted_engine() -> InMemoryEngine<ScriptedMetadata> {
    InMemoryEngine::new(ScriptedMetadata::new([
        (event_id("event-1"), 300),
        (event_id("event-2"), 200),
        (event_id("event-3"), 100),
        (event_id("event-4"), 400),
    ]))
}

#[test]
fn command_events_round_trip_and_replay_the_same_visible_session() {
    let exact_prompt = "  first line\nsecond line: こんにちは  ";
    let commands = [
        create_command(),
        select_command(),
        admit_command("command-admit-1", "input-1", exact_prompt),
    ];
    let mut live = scripted_engine();

    for command in &commands {
        live.execute(command).expect("valid command");
    }

    let sequences: Vec<_> = live
        .events()
        .iter()
        .map(|event| event.sequence().get())
        .collect();
    assert_eq!(sequences, [1, 2, 3, 4]);
    assert_eq!(
        live.events()[0].causation(),
        &Causation::Command(commands[0].command_id().clone())
    );
    assert_eq!(
        live.events()[1].causation(),
        &Causation::Command(commands[1].command_id().clone())
    );
    assert_eq!(
        live.events()[2].causation(),
        &Causation::Command(commands[2].command_id().clone())
    );
    assert_eq!(
        live.events()[3].causation(),
        &Causation::Event(live.events()[2].event_id().clone())
    );
    assert!(
        live.events()
            .iter()
            .all(|event| event.correlation_id() == &correlation_id())
    );

    let live_session = live
        .session(&session_id())
        .expect("created session")
        .clone();
    assert_eq!(live_session.selected_model(), Some(&model()));
    assert_eq!(
        live_session.admitted_inputs()[0].prompt().as_str(),
        exact_prompt
    );
    assert!(matches!(
        live.events()[3].payload(),
        EventPayload::SessionRenamed { title } if title.as_str() == "first line"
    ));

    let serialized = serde_json::to_vec(live.events()).expect("serialize event log");
    let restored_events: Vec<EventEnvelope> =
        serde_json::from_slice(&serialized).expect("deserialize event log");
    let replayed = InMemoryEngine::replay(
        ScriptedMetadata::new([(event_id("event-5"), 500)]),
        restored_events,
    )
    .expect("valid replay");

    assert_eq!(replayed.session(&session_id()), Some(&live_session));
    assert_eq!(replayed.events(), live.events());

    let mut replayed = replayed;
    let next = replayed
        .execute(&admit_command(
            "command-admit-2",
            "input-2",
            "after restart",
        ))
        .expect("replayed engine accepts a later command");
    assert_eq!(next[0].sequence().get(), 5);
}

#[test]
fn rejected_command_does_not_consume_metadata_or_mutate_history() {
    let mut engine = scripted_engine();
    let rejection = engine
        .execute(&select_command())
        .expect_err("selection before creation must fail");

    assert_eq!(
        rejection,
        EngineError::CommandRejected(CommandRejection::SessionNotFound {
            session_id: session_id()
        })
    );
    assert!(engine.events().is_empty());
    assert!(engine.session(&session_id()).is_none());

    let emitted = engine
        .execute(&create_command())
        .expect("valid command after rejection");
    assert_eq!(emitted[0].event_id(), &event_id("event-1"));
    assert_eq!(emitted[0].sequence(), SessionSequence::FIRST);
}

#[test]
fn accepted_command_ids_cannot_be_reused() {
    let mut engine = scripted_engine();
    engine.execute(&create_command()).expect("create session");
    let before = engine.events().to_vec();
    let reused = command(
        "command-create",
        CommandPayload::SelectModel {
            session_id: session_id(),
            model: model(),
        },
    );

    let error = engine
        .execute(&reused)
        .expect_err("accepted command identity must not be reused");

    assert_eq!(
        error,
        EngineError::CommandRejected(CommandRejection::DuplicateCommand {
            command_id: command_id("command-create"),
        })
    );
    assert_eq!(engine.events(), before);

    let mut replayed = InMemoryEngine::replay(
        ScriptedMetadata::new([(event_id("event-after-replay"), 10)]),
        engine.events().to_vec(),
    )
    .expect("replay valid history");
    assert_eq!(
        replayed
            .execute(&reused)
            .expect_err("replay must restore accepted command identities"),
        EngineError::CommandRejected(CommandRejection::DuplicateCommand {
            command_id: command_id("command-create"),
        })
    );

    let emitted = engine
        .execute(&select_command())
        .expect("a fresh command identity remains valid");
    assert_eq!(emitted[0].event_id(), &event_id("event-2"));
}

#[test]
fn duplicate_input_rejection_is_atomic() {
    let mut engine = scripted_engine();
    engine.execute(&create_command()).expect("create session");
    engine
        .execute(&admit_command("command-admit-1", "input-1", "first"))
        .expect("admit input");
    let before_events = engine.events().to_vec();
    let before_session = engine
        .session(&session_id())
        .expect("created session")
        .clone();

    let error = engine
        .execute(&admit_command(
            "command-admit-2",
            "input-1",
            "different content",
        ))
        .expect_err("duplicate input must fail");

    assert_eq!(
        error,
        EngineError::CommandRejected(CommandRejection::DuplicateInput {
            session_id: session_id(),
            input_id: input_id("input-1"),
        })
    );
    assert_eq!(engine.events(), before_events);
    assert_eq!(engine.session(&session_id()), Some(&before_session));
}

#[test]
fn duplicate_session_creation_is_rejected_without_a_new_event() {
    let mut engine = scripted_engine();
    engine.execute(&create_command()).expect("create session");
    let before = engine.events().to_vec();
    let duplicate_create = command(
        "command-create-again",
        CommandPayload::CreateSession {
            session_id: session_id(),
        },
    );

    let error = engine
        .execute(&duplicate_create)
        .expect_err("duplicate creation must fail");

    assert_eq!(
        error,
        EngineError::CommandRejected(CommandRejection::SessionAlreadyExists {
            session_id: session_id(),
        })
    );
    assert_eq!(engine.events(), before);
}

#[test]
fn replay_rejects_gaps_without_mutating_the_aggregate() {
    let create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(200),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let gap = EventEnvelope::new_v1(
        event_id("event-2"),
        session_id(),
        SessionSequence::new(3).expect("nonzero sequence"),
        TimestampMillis::new(100),
        Causation::Command(command_id("command-select")),
        correlation_id(),
        EventPayload::ModelSelected { model: model() },
    );
    let mut aggregate = SessionAggregate::empty(session_id());
    aggregate
        .apply_batch(std::slice::from_ref(&create))
        .expect("valid create event");
    let before = aggregate.clone();

    let error = aggregate
        .apply_batch(std::slice::from_ref(&gap))
        .expect_err("sequence gap must fail");

    assert_eq!(
        error,
        ReplayError::NonContiguousSequence {
            session_id: session_id(),
            expected: 2,
            found: 3,
            event_id: event_id("event-2"),
        }
    );
    assert_eq!(aggregate, before);
}

#[test]
fn replay_validates_a_complete_batch_before_committing_any_event() {
    let create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let create_again = EventEnvelope::new_v1(
        event_id("event-2"),
        session_id(),
        SessionSequence::new(2).expect("nonzero sequence"),
        TimestampMillis::new(2),
        Causation::Command(command_id("command-create-again")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let mut aggregate = SessionAggregate::empty(session_id());
    let before = aggregate.clone();

    let error = aggregate
        .apply_batch(&[create, create_again])
        .expect_err("second creation must invalidate the batch");

    assert_eq!(
        error,
        ReplayError::SessionAlreadyCreated {
            session_id: session_id(),
            event_id: event_id("event-2"),
        }
    );
    assert_eq!(aggregate, before);
}

#[test]
fn replay_rejects_events_before_creation_and_for_the_wrong_session() {
    let select_before_create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-select")),
        correlation_id(),
        EventPayload::ModelSelected { model: model() },
    );
    let not_created =
        SessionAggregate::rehydrate(session_id(), std::slice::from_ref(&select_before_create))
            .expect_err("selection cannot precede creation");
    assert_eq!(
        not_created,
        ReplayError::SessionNotCreated {
            session_id: session_id(),
            event_id: event_id("event-1"),
        }
    );

    let other_session = SessionId::new("session-2").expect("valid test ID");
    let wrong_session = EventEnvelope::new_v1(
        event_id("event-2"),
        other_session.clone(),
        SessionSequence::FIRST,
        TimestampMillis::new(2),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let error = SessionAggregate::rehydrate(session_id(), std::slice::from_ref(&wrong_session))
        .expect_err("aggregate cannot accept a different session");
    assert_eq!(
        error,
        ReplayError::WrongSession {
            expected: session_id(),
            found: other_session,
            event_id: event_id("event-2"),
        }
    );
}

#[test]
fn replay_requires_event_causation_to_reference_an_applied_event() {
    let create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let selected = EventEnvelope::new_v1(
        event_id("event-2"),
        session_id(),
        SessionSequence::new(2).expect("nonzero sequence"),
        TimestampMillis::new(2),
        Causation::Event(event_id("event-1")),
        correlation_id(),
        EventPayload::ModelSelected { model: model() },
    );
    let mut aggregate = SessionAggregate::rehydrate(session_id(), [&create, &selected])
        .expect("a prior event is a valid direct cause");
    let before = aggregate.clone();
    let self_caused = EventEnvelope::new_v1(
        event_id("event-3"),
        session_id(),
        SessionSequence::new(3).expect("nonzero sequence"),
        TimestampMillis::new(3),
        Causation::Event(event_id("event-3")),
        correlation_id(),
        EventPayload::InputAdmitted {
            input_id: input_id("input-1"),
            prompt: PromptText::new("hello").expect("non-empty prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );

    let error = aggregate
        .apply_batch(std::slice::from_ref(&self_caused))
        .expect_err("self or future causation must fail");

    assert_eq!(
        error,
        ReplayError::UnknownCausation {
            event_id: event_id("event-3"),
            cause_event_id: event_id("event-3"),
        }
    );
    assert_eq!(aggregate, before);
}

#[test]
fn replay_rejects_reused_direct_command_causation() {
    let create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-shared")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let selected = EventEnvelope::new_v1(
        event_id("event-2"),
        session_id(),
        SessionSequence::new(2).expect("nonzero sequence"),
        TimestampMillis::new(2),
        Causation::Command(command_id("command-shared")),
        correlation_id(),
        EventPayload::ModelSelected { model: model() },
    );

    let error = InMemoryEngine::replay(ScriptedMetadata::new([]), [create, selected])
        .expect_err("a command may directly cause only one event");

    assert_eq!(
        error,
        ReplayError::DuplicateCommandCausation {
            event_id: event_id("event-2"),
            command_id: command_id("command-shared"),
        }
    );

    let other_session = SessionId::new("session-2").expect("valid test ID");
    let create_first = EventEnvelope::new_v1(
        event_id("event-3"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(3),
        Causation::Command(command_id("command-global")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let create_second = EventEnvelope::new_v1(
        event_id("event-4"),
        other_session,
        SessionSequence::FIRST,
        TimestampMillis::new(4),
        Causation::Command(command_id("command-global")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let global_error =
        InMemoryEngine::replay(ScriptedMetadata::new([]), [create_first, create_second])
            .expect_err("command identity must also be unique across sessions");

    assert_eq!(
        global_error,
        ReplayError::DuplicateCommandCausation {
            event_id: event_id("event-4"),
            command_id: command_id("command-global"),
        }
    );
}

#[test]
fn replay_rejects_duplicate_or_decreasing_sequences() {
    let create = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(2),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let repeated_sequence = EventEnvelope::new_v1(
        event_id("event-2"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-select")),
        correlation_id(),
        EventPayload::ModelSelected { model: model() },
    );

    let error = SessionAggregate::rehydrate(session_id(), [&create, &repeated_sequence])
        .expect_err("repeated sequence must fail");

    assert_eq!(
        error,
        ReplayError::NonContiguousSequence {
            session_id: session_id(),
            expected: 2,
            found: 1,
            event_id: event_id("event-2"),
        }
    );
}

#[test]
fn replay_rejects_unsupported_schema_and_duplicate_event_identity() {
    let valid = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let mut unsupported_value = serde_json::to_value(valid).expect("serialize valid event");
    unsupported_value["schema_version"] = serde_json::json!(2);
    let unsupported: EventEnvelope =
        serde_json::from_value(unsupported_value).expect("decode unknown schema for replay");
    let schema_error = InMemoryEngine::replay(ScriptedMetadata::new([]), [unsupported])
        .expect_err("unknown schema must fail closed");
    assert_eq!(
        schema_error,
        ReplayError::UnsupportedSchema {
            event_id: event_id("event-1"),
            found: 2,
        }
    );

    let duplicate = EventEnvelope::new_v1(
        event_id("same-event"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(2),
        Causation::Command(command_id("command-create")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let other_session = SessionId::new("session-2").expect("valid test ID");
    let duplicate_elsewhere = EventEnvelope::new_v1(
        event_id("same-event"),
        other_session,
        SessionSequence::FIRST,
        TimestampMillis::new(3),
        Causation::Command(command_id("command-create-2")),
        correlation_id(),
        EventPayload::SessionCreated,
    );
    let duplicate_error =
        InMemoryEngine::replay(ScriptedMetadata::new([]), [duplicate, duplicate_elsewhere])
            .expect_err("event IDs must be globally unique");
    assert_eq!(
        duplicate_error,
        ReplayError::DuplicateEventId {
            event_id: event_id("same-event")
        }
    );
}
