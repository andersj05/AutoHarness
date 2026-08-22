use std::collections::VecDeque;

use autoharness_domain::{
    AttemptFailure, Causation, CommandEnvelope, CommandId, CommandPayload, CorrelationId,
    DeliveryMode, ErrorCode, EventId, EventPayload, InputId, ModelId, ModelRef, PromptText,
    ProviderId, PublicMessage, RetryAdvice, SessionId, SessionTitle,
};
use autoharness_engine::{
    AttemptStatus, CommandRejection, EngineError, EventMetadataSource, GeneratedEventMetadata,
    InMemoryEngine, ReplayError, SessionAggregate,
};

#[derive(Debug)]
struct ScriptedMetadata {
    values: VecDeque<GeneratedEventMetadata>,
}

impl ScriptedMetadata {
    fn new(count: usize) -> Self {
        Self {
            values: (0..count)
                .map(|index| {
                    GeneratedEventMetadata::new(
                        EventId::new(format!("event-{}", index + 1)).expect("valid test event ID"),
                        autoharness_domain::TimestampMillis::new(
                            i64::try_from(1_000 + index).expect("in-range test timestamp"),
                        ),
                    )
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
    CorrelationId::new("correlation-1").expect("valid correlation ID")
}

fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command ID")
}

fn command(name: &str, payload: CommandPayload) -> CommandEnvelope {
    CommandEnvelope::new(command_id(name), correlation_id(), payload)
}

fn create() -> CommandPayload {
    CommandPayload::CreateSession {
        session_id: session_id(),
    }
}

fn select() -> CommandPayload {
    CommandPayload::SelectModel {
        session_id: session_id(),
        model: ModelRef::new(
            ProviderId::new("google-ai-studio").expect("valid provider ID"),
            ModelId::new("models/gemini-test").expect("valid model ID"),
        ),
    }
}

fn rename(title: &str) -> CommandPayload {
    CommandPayload::RenameSession {
        session_id: session_id(),
        title: SessionTitle::new(title).expect("valid test title"),
    }
}

fn archive() -> CommandPayload {
    CommandPayload::ArchiveSession {
        session_id: session_id(),
    }
}

fn unarchive() -> CommandPayload {
    CommandPayload::UnarchiveSession {
        session_id: session_id(),
    }
}

fn admit_and_complete_attempt(engine: &mut InMemoryEngine<ScriptedMetadata>) {
    engine
        .execute(&command(
            "command-admit",
            CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id(),
                input_id: InputId::new("input-1").expect("valid input ID"),
                prompt: PromptText::new("hello").expect("non-empty prompt"),
                delivery_mode: DeliveryMode::NextTurn,
                attempt_id: autoharness_domain::AttemptId::new("attempt-1")
                    .expect("valid attempt ID"),
            },
        ))
        .expect("admit and prepare");
    engine
        .execute(&command(
            "command-start",
            CommandPayload::StartAttempt {
                session_id: session_id(),
                attempt_id: autoharness_domain::AttemptId::new("attempt-1")
                    .expect("valid attempt ID"),
            },
        ))
        .expect("start attempt");
    engine
        .execute(&command(
            "command-complete",
            CommandPayload::CompleteAttempt {
                session_id: session_id(),
                attempt_id: autoharness_domain::AttemptId::new("attempt-1")
                    .expect("valid attempt ID"),
            },
        ))
        .expect("complete attempt");
}

fn prepared_failure() -> AttemptFailure {
    AttemptFailure::new(
        autoharness_domain::ErrorClass::Unavailable,
        ErrorCode::new("interrupted_before_dispatch").expect("static recovery error code is valid"),
        PublicMessage::new("The attempt was interrupted before provider dispatch")
            .expect("static recovery message is valid"),
        RetryAdvice::Immediate,
    )
}

fn lifecycle_engine(commands: &[CommandPayload]) -> InMemoryEngine<ScriptedMetadata> {
    let mut engine = InMemoryEngine::new(ScriptedMetadata::new(32));
    for (index, payload) in commands.iter().enumerate() {
        let name = format!("command-{index}");
        engine
            .execute(&command(&name, payload.clone()))
            .unwrap_or_else(|error| panic!("command {index} rejected: {error}"));
    }
    engine
}

#[test]
fn rename_updates_the_replayed_title_without_disturbing_other_state() {
    let mut engine = lifecycle_engine(&[create(), rename("Deep dive")]);
    let session = engine.session(&session_id()).expect("created session");
    assert_eq!(session.title().map(SessionTitle::as_str), Some("Deep dive"));

    engine
        .execute(&command("command-rename-2", rename("Second name")))
        .expect("rename again");
    let replay = SessionAggregate::rehydrate(session_id(), engine.events())
        .expect("history replays cleanly");
    assert_eq!(
        replay.title().map(SessionTitle::as_str),
        Some("Second name")
    );
    assert!(!replay.is_archived());
    assert_eq!(engine.events().len(), 3);
}

#[test]
fn rename_requires_an_existing_session() {
    let mut engine = InMemoryEngine::new(ScriptedMetadata::new(4));
    let error = engine
        .execute(&command("command-rename", rename("Too early")))
        .expect_err("rename before creation must fail");
    match error {
        EngineError::CommandRejected(CommandRejection::SessionNotFound { .. }) => {}
        other => panic!("unexpected rejection: {other:?}"),
    }
}

#[test]
fn archived_sessions_reject_ordinary_commands_but_stay_readable() {
    let mut engine = lifecycle_engine(&[create(), rename("Reading room"), archive()]);

    let error = engine
        .execute(&command("command-select", select()))
        .expect_err("archived sessions reject ordinary commands");
    match error {
        EngineError::CommandRejected(CommandRejection::SessionArchived { .. }) => {}
        other => panic!("unexpected rejection: {other:?}"),
    }

    // The aggregate still replays and reports its durable state.
    let replay = SessionAggregate::rehydrate(session_id(), engine.events())
        .expect("archived history replays cleanly");
    assert!(replay.is_archived());
    assert_eq!(
        replay.title().map(SessionTitle::as_str),
        Some("Reading room")
    );
}

#[test]
fn unarchive_restores_command_eligibility_and_replays_exactly() {
    let mut engine = lifecycle_engine(&[
        create(),
        select(),
        archive(),
        unarchive(),
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: InputId::new("input-after-unarchive").expect("valid input ID"),
            prompt: PromptText::new("back again").expect("non-empty prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    ]);
    assert!(
        !engine
            .session(&session_id())
            .expect("session present")
            .is_archived()
    );

    let replay = SessionAggregate::rehydrate(session_id(), engine.events())
        .expect("mixed history replays cleanly");
    assert!(!replay.is_archived());
    assert_eq!(replay.admitted_inputs().len(), 1);
}

#[test]
fn double_archive_and_unarchive_of_active_session_are_conflicts() {
    let mut engine = lifecycle_engine(&[create()]);
    let error = engine
        .execute(&command("command-unarchive-active", unarchive()))
        .expect_err("unarchive of an active session must fail");
    assert!(matches!(
        error,
        EngineError::CommandRejected(CommandRejection::InvalidSessionState { .. })
    ));

    let mut engine = lifecycle_engine(&[create(), archive()]);
    let error = engine
        .execute(&command("command-archive-twice", archive()))
        .expect_err("double archive must fail");
    assert!(matches!(
        error,
        EngineError::CommandRejected(CommandRejection::InvalidSessionState { .. })
    ));
}

#[test]
fn archive_is_blocked_while_any_attempt_remains_unsettled() {
    let mut engine = InMemoryEngine::new(ScriptedMetadata::new(16));
    for (name, payload) in [
        ("c0", create()),
        ("c1", select()),
        (
            "c2",
            CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: session_id(),
                input_id: InputId::new("input-live").expect("valid input ID"),
                prompt: PromptText::new("still running").expect("non-empty prompt"),
                delivery_mode: DeliveryMode::NextTurn,
                attempt_id: autoharness_domain::AttemptId::new("attempt-live")
                    .expect("valid attempt ID"),
            },
        ),
    ] {
        engine
            .execute(&command(name, payload))
            .expect("prepare live attempt");
    }

    let error = engine
        .execute(&command("c-archive", archive()))
        .expect_err("prepared attempt blocks archive");
    assert!(matches!(
        error,
        EngineError::CommandRejected(CommandRejection::SessionHasUnsettledWork { .. })
    ));

    engine
        .execute(&command(
            "c-fail",
            CommandPayload::FailAttempt {
                session_id: session_id(),
                attempt_id: autoharness_domain::AttemptId::new("attempt-live")
                    .expect("valid attempt ID"),
                failure: prepared_failure(),
            },
        ))
        .expect("settle attempt");
    engine
        .execute(&command("c-archive-ok", archive()))
        .expect("settled session archives");
    assert!(
        engine
            .session(&session_id())
            .expect("session present")
            .is_archived()
    );

    let replay = SessionAggregate::rehydrate(session_id(), engine.events())
        .expect("history replays cleanly");
    assert!(replay.is_archived());
    let attempt = replay.attempts().first().expect("settled attempt");
    assert_eq!(attempt.status(), AttemptStatus::Failed);
    assert!(attempt.status().is_settled());
}

#[test]
fn replay_rejects_archive_events_that_contradict_durable_state() {
    let event = |sequence: u64, payload: EventPayload| {
        autoharness_domain::EventEnvelope::new_v1(
            EventId::new(format!("event-{sequence}")).expect("valid event ID"),
            session_id(),
            autoharness_domain::SessionSequence::new(sequence).expect("nonzero sequence"),
            autoharness_domain::TimestampMillis::new(i64::try_from(sequence).unwrap_or(1)),
            Causation::Command(command_id(&format!("command-{sequence}"))),
            correlation_id(),
            payload,
        )
    };

    // A second archive without an intervening unarchive is corrupt history.
    let history = [
        event(1, EventPayload::SessionCreated),
        event(2, EventPayload::SessionArchived),
        autoharness_domain::EventEnvelope::new_v1(
            EventId::new("event-3").expect("valid event ID"),
            session_id(),
            autoharness_domain::SessionSequence::new(3).expect("nonzero sequence"),
            autoharness_domain::TimestampMillis::new(3),
            Causation::Event(EventId::new("event-2").expect("valid event ID")),
            correlation_id(),
            EventPayload::SessionArchived,
        ),
    ];
    let error = SessionAggregate::rehydrate(session_id(), history.iter())
        .expect_err("double archive history is invalid");
    assert!(matches!(
        error,
        ReplayError::IllegalSessionTransition { .. }
    ));

    // An unarchive before any archive is also corrupt history.
    let history = [
        event(1, EventPayload::SessionCreated),
        event(2, EventPayload::SessionUnarchived),
    ];
    let error = SessionAggregate::rehydrate(session_id(), history.iter())
        .expect_err("unarchive-before-archive history is invalid");
    assert!(matches!(
        error,
        ReplayError::IllegalSessionTransition { .. }
    ));
}
