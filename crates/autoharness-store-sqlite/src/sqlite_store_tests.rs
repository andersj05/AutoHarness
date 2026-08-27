use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use autoharness_domain::{
    AttemptId, Causation, CommandId, CorrelationId, DeliveryMode, EventEnvelope, EventId,
    EventPayload, InputId, ModelId, ModelRef, PromptText, ProviderId, ResponseText, SessionId,
    SessionSequence, TimestampMillis, UsageSnapshot,
};
use autoharness_store::{
    AppendDisposition, AppendRequest, AttemptState, CorruptionArea, IdentityKind, InputState,
    SessionStore, StoreError, TranscriptRole, TranscriptSource, TranscriptState,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::{FailurePoint, SqliteCatalogCacheRecord, SqliteStore, SqliteStoreOptions};

struct TestDatabase {
    _directory: TempDir,
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("create temporary database directory");
        let path = directory.path().join("session-store.sqlite3");
        Self {
            _directory: directory,
            path,
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("open test store")
    }
}

fn session_id(value: &str) -> SessionId {
    SessionId::new(value).expect("valid session ID")
}

fn event_id(value: &str) -> EventId {
    EventId::new(value).expect("valid event ID")
}

fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command ID")
}

fn correlation_id(value: &str) -> CorrelationId {
    CorrelationId::new(value).expect("valid correlation ID")
}

fn input_id(value: &str) -> InputId {
    InputId::new(value).expect("valid input ID")
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

fn event(
    event: &str,
    session: &SessionId,
    sequence: u64,
    command: &str,
    occurred_at: i64,
    payload: EventPayload,
) -> EventEnvelope {
    EventEnvelope::new_v1(
        event_id(event),
        session.clone(),
        SessionSequence::new(sequence).expect("nonzero sequence"),
        TimestampMillis::new(occurred_at),
        Causation::Command(command_id(command)),
        correlation_id("correlation-1"),
        payload,
    )
}

fn initial_events(session: &SessionId, prompt: &str) -> Vec<EventEnvelope> {
    vec![
        event(
            "event-1",
            session,
            1,
            "command-create",
            300,
            EventPayload::SessionCreated,
        ),
        event(
            "event-2",
            session,
            2,
            "command-select",
            200,
            EventPayload::ModelSelected { model: model() },
        ),
        event(
            "event-3",
            session,
            3,
            "command-admit",
            100,
            EventPayload::InputAdmitted {
                input_id: input_id("input-1"),
                prompt: PromptText::new(prompt).expect("non-empty prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
        ),
    ]
}

#[test]
fn opening_verifies_durable_pragmas_and_migrations_are_idempotent() {
    let database = TestDatabase::new();
    let options = SqliteStoreOptions::new(Duration::from_millis(1_234));
    let store = SqliteStore::open_with_options(&database.path, options).expect("open store");

    assert_eq!(store.configuration().journal_mode(), "wal");
    assert_eq!(store.configuration().synchronous_level(), 2);
    assert!(store.configuration().foreign_keys());
    assert!(!store.configuration().trusted_schema());
    assert_eq!(store.configuration().busy_timeout_ms(), 1_234);
    assert_eq!(
        store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("read schema version"),
        3
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count migrations"),
        3
    );
    drop(store);

    let reopened = database.open();
    assert_eq!(
        reopened
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count migrations after reopen"),
        3
    );
}

#[test]
fn version_one_database_upgrades_catalog_cache_without_rewriting_history() {
    let database = TestDatabase::new();
    let connection = Connection::open(&database.path).expect("open version-one fixture");
    let migration = include_str!("../migrations/0001_session_store.sql");
    connection
        .execute_batch(
            r#"
            CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL CHECK (version > 0),
                name TEXT NOT NULL,
                checksum BLOB NOT NULL CHECK (length(checksum) = 32),
                applied_at_ms INTEGER NOT NULL
            ) STRICT;
            "#,
        )
        .expect("migration table");
    connection
        .execute_batch(migration)
        .expect("version-one schema");
    let checksum = Sha256::digest(migration.as_bytes());
    connection
        .execute(
            "INSERT INTO schema_migrations \
             (version, name, checksum, applied_at_ms) VALUES (1, 'session_store', ?1, 0)",
            params![checksum.as_slice()],
        )
        .expect("version-one history");
    connection
        .pragma_update(None, "user_version", 1)
        .expect("version-one pragma");
    drop(connection);

    let store = database.open();
    assert_eq!(
        store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .expect("schema version"),
        3
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("migration count"),
        3
    );
}

#[test]
fn catalog_cache_round_trips_replaces_and_fails_closed_on_corruption() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let provider_id = ProviderId::new("router:project-a").expect("provider ID");
    let first = SqliteCatalogCacheRecord::new(
        provider_id.clone(),
        1,
        100,
        br#"{"schema_version":1,"models":[]}"#.to_vec(),
    );
    store
        .replace_catalog_cache(&first)
        .expect("store first cache");
    assert_eq!(
        store
            .load_catalog_cache(&provider_id)
            .expect("load first cache"),
        Some(first)
    );

    let replacement = SqliteCatalogCacheRecord::new(
        provider_id.clone(),
        1,
        200,
        br#"{"schema_version":1,"models":[{"id":"model-a"}]}"#.to_vec(),
    );
    store
        .replace_catalog_cache(&replacement)
        .expect("replace cache");
    assert_eq!(
        store
            .load_catalog_cache(&provider_id)
            .expect("load replacement"),
        Some(replacement)
    );

    store
        .connection
        .execute(
            "UPDATE model_catalog_cache SET catalog_json = x'00' WHERE provider_id = ?1",
            params![provider_id.as_str()],
        )
        .expect("corrupt cache fixture");
    assert_eq!(
        store.load_catalog_cache(&provider_id),
        Err(StoreError::CorruptData {
            area: CorruptionArea::CatalogCache
        })
    );
}

#[test]
fn append_round_trips_events_inputs_sessions_and_transcript_exactly() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-1");
    let exact_prompt = "  first line\0\nsecond line: こんにちは  ";
    let events = initial_events(&session, exact_prompt);
    let receipt = store
        .append(&AppendRequest::new(session.clone(), 0, events.clone()))
        .expect("append initial events");

    assert_eq!(receipt.disposition(), AppendDisposition::Committed);
    assert_eq!(receipt.last_sequence(), 3);
    assert_eq!(
        store.load_events(&session, 0, 32).expect("load events"),
        events
    );
    assert_eq!(
        store.load_events(&session, 1, 1).expect("load one event"),
        events[1..2]
    );

    let sessions = store.list_sessions().expect("list sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id(), &session);
    assert_eq!(sessions[0].selected_model(), Some(&model()));
    assert_eq!(sessions[0].message_count(), 1);
    assert_eq!(sessions[0].last_sequence().get(), 3);
    assert_eq!(sessions[0].created_at().get(), 300);
    assert_eq!(sessions[0].updated_at().get(), 100);

    let inputs = store
        .load_admitted_inputs(&session)
        .expect("load admitted inputs");
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].prompt().as_str(), exact_prompt);
    assert_eq!(inputs[0].sequence().get(), 3);

    let transcript = store.load_transcript(&session).expect("load transcript");
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].role(), TranscriptRole::User);
    assert_eq!(
        transcript[0].source(),
        &TranscriptSource::Input(input_id("input-1"))
    );
    assert_eq!(transcript[0].content().as_str(), exact_prompt);

    drop(store);
    let reopened = database.open();
    assert_eq!(
        reopened
            .load_events(&session, 0, 32)
            .expect("replay after reopen"),
        events
    );
}

#[test]
fn append_rollback_is_atomic_before_commit() {
    for failure_point in [
        FailurePoint::FirstEventInserted,
        FailurePoint::ProjectionApplied,
    ] {
        let database = TestDatabase::new();
        let mut store = database.open();
        let session = session_id("session-rollback");
        let events = initial_events(&session, "must roll back");
        store.set_failure_point(Some(failure_point));

        assert_eq!(
            store.append(&AppendRequest::new(session.clone(), 0, events)),
            Err(StoreError::Backend)
        );
        assert!(
            store
                .load_events(&session, 0, 32)
                .expect("load rolled-back events")
                .is_empty()
        );
        assert!(store.list_sessions().expect("list sessions").is_empty());
        assert!(
            store
                .load_admitted_inputs(&session)
                .expect("load rolled-back inputs")
                .is_empty()
        );
        assert!(
            store
                .load_transcript(&session)
                .expect("load rolled-back transcript")
                .is_empty()
        );
    }
}

#[test]
fn ambiguous_commit_retries_are_exactly_idempotent() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-idempotent");
    let events = initial_events(&session, "committed once");
    let request = AppendRequest::new(session.clone(), 0, events.clone());
    store.set_failure_point(Some(FailurePoint::CommitCompleted));

    assert_eq!(store.append(&request), Err(StoreError::Backend));
    store.set_failure_point(None);
    let receipt = store.append(&request).expect("reconcile exact batch");

    assert_eq!(receipt.disposition(), AppendDisposition::AlreadyCommitted);
    assert_eq!(receipt.last_sequence(), 3);
    assert_eq!(
        store
            .load_events(&session, 0, 32)
            .expect("load reconciled events"),
        events
    );
    assert_eq!(
        store
            .load_admitted_inputs(&session)
            .expect("load one input")
            .len(),
        1
    );
}

#[test]
fn reused_event_identity_requires_identical_serialized_bytes() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-event-conflict");
    let original = event(
        "event-shared",
        &session,
        1,
        "command-create",
        1,
        EventPayload::SessionCreated,
    );
    store
        .append(&AppendRequest::new(session.clone(), 0, vec![original]))
        .expect("append original event");
    let different = event(
        "event-shared",
        &session,
        1,
        "command-create",
        2,
        EventPayload::SessionCreated,
    );

    assert_eq!(
        store.append(&AppendRequest::new(session, 0, vec![different])),
        Err(StoreError::IdentityConflict {
            kind: IdentityKind::Event
        })
    );
}

#[test]
fn an_overlapping_partial_retry_is_rejected_without_appending_its_suffix() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-partial-retry");
    let create = event(
        "event-partial-1",
        &session,
        1,
        "command-partial-create",
        1,
        EventPayload::SessionCreated,
    );
    store
        .append(&AppendRequest::new(
            session.clone(),
            0,
            vec![create.clone()],
        ))
        .expect("append first event");
    let select = event(
        "event-partial-2",
        &session,
        2,
        "command-partial-select",
        2,
        EventPayload::ModelSelected { model: model() },
    );

    assert_eq!(
        store.append(&AppendRequest::new(
            session.clone(),
            0,
            vec![create, select]
        )),
        Err(StoreError::IdentityConflict {
            kind: IdentityKind::Event
        })
    );
    assert_eq!(
        store
            .load_events(&session, 0, 32)
            .expect("load unchanged stream")
            .len(),
        1
    );
}

#[test]
fn stale_expected_sequence_fails_without_writing() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-version");
    let first_two = initial_events(&session, "unused")[..2].to_vec();
    store
        .append(&AppendRequest::new(session.clone(), 0, first_two))
        .expect("append first two events");
    let stale = event(
        "event-stale",
        &session,
        2,
        "command-stale",
        500,
        EventPayload::InputAdmitted {
            input_id: input_id("input-stale"),
            prompt: PromptText::new("stale").expect("non-empty prompt"),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );

    assert_eq!(
        store.append(&AppendRequest::new(session.clone(), 1, vec![stale])),
        Err(StoreError::VersionConflict {
            session_id: session.clone(),
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        store
            .load_events(&session, 0, 32)
            .expect("load unchanged events")
            .len(),
        2
    );
}

#[test]
fn command_identity_is_unique_across_sessions() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let first_session = session_id("session-command-1");
    let second_session = session_id("session-command-2");
    let first = event(
        "event-command-1",
        &first_session,
        1,
        "command-global",
        1,
        EventPayload::SessionCreated,
    );
    let second = event(
        "event-command-2",
        &second_session,
        1,
        "command-global",
        2,
        EventPayload::SessionCreated,
    );
    store
        .append(&AppendRequest::new(first_session, 0, vec![first]))
        .expect("append first command");

    assert_eq!(
        store.append(&AppendRequest::new(second_session, 0, vec![second])),
        Err(StoreError::IdentityConflict {
            kind: IdentityKind::Command
        })
    );
}

#[test]
fn projection_rebuild_restores_exact_content_from_events() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-rebuild");
    let exact_prompt = "  recover こんにちは  ";
    let events = initial_events(&session, exact_prompt);
    store
        .append(&AppendRequest::new(session.clone(), 0, events.clone()))
        .expect("append events");
    store
        .connection
        .execute(
            "UPDATE admitted_inputs SET prompt_utf8 = X'62726F6B656E' \
             WHERE session_id = ?1",
            [session.as_str()],
        )
        .expect("corrupt input projection");

    assert_eq!(
        store.load_admitted_inputs(&session),
        Err(StoreError::CorruptData {
            area: CorruptionArea::InputProjection
        })
    );
    store.rebuild_projections().expect("rebuild projections");

    assert_eq!(
        store
            .load_admitted_inputs(&session)
            .expect("load rebuilt input")[0]
            .prompt()
            .as_str(),
        exact_prompt
    );
    assert_eq!(
        store
            .load_transcript(&session)
            .expect("load rebuilt transcript")[0]
            .content()
            .as_str(),
        exact_prompt
    );
    assert_eq!(
        store
            .load_events(&session, 0, 32)
            .expect("authoritative events unchanged"),
        events
    );
}

#[test]
fn event_scalar_corruption_fails_closed() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-corrupt-event");
    store
        .append(&AppendRequest::new(
            session.clone(),
            0,
            initial_events(&session, "content"),
        ))
        .expect("append events");
    store
        .connection
        .execute(
            "UPDATE session_events SET event_kind = 'model_selected' WHERE event_id = 'event-1'",
            [],
        )
        .expect("corrupt indexed event kind");

    assert_eq!(
        store.load_events(&session, 0, 32),
        Err(StoreError::CorruptData {
            area: CorruptionArea::Event
        })
    );
}

#[test]
fn event_sequence_gaps_fail_closed() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-gap");
    store
        .append(&AppendRequest::new(
            session.clone(),
            0,
            initial_events(&session, "content"),
        ))
        .expect("append events");
    store
        .connection
        .pragma_update(None, "foreign_keys", "OFF")
        .expect("disable foreign keys for corruption fixture");
    store
        .connection
        .execute("DELETE FROM session_events WHERE sequence = 2", [])
        .expect("create event gap");
    store
        .connection
        .pragma_update(None, "foreign_keys", "ON")
        .expect("restore foreign keys");

    assert_eq!(
        store.load_events(&session, 0, 32),
        Err(StoreError::CorruptData {
            area: CorruptionArea::EventSequence
        })
    );
}

#[test]
fn migration_history_tampering_fails_closed_on_reopen() {
    let database = TestDatabase::new();
    let store = database.open();
    store
        .connection
        .execute("UPDATE schema_migrations SET checksum = zeroblob(32)", [])
        .expect("tamper with migration checksum");
    drop(store);

    assert_eq!(
        SqliteStore::open(&database.path).err(),
        Some(StoreError::CorruptData {
            area: CorruptionArea::MigrationHistory
        })
    );
}

#[test]
fn malformed_requests_are_rejected_before_sql() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let first_session = session_id("session-malformed-1");
    let second_session = session_id("session-malformed-2");

    assert_eq!(
        store.append(&AppendRequest::new(first_session.clone(), 0, Vec::new())),
        Err(StoreError::EmptyAppend)
    );

    let mixed = vec![
        event(
            "event-mixed-1",
            &first_session,
            1,
            "command-mixed-1",
            1,
            EventPayload::SessionCreated,
        ),
        event(
            "event-mixed-2",
            &second_session,
            2,
            "command-mixed-2",
            2,
            EventPayload::ModelSelected { model: model() },
        ),
    ];
    assert_eq!(
        store.append(&AppendRequest::new(first_session, 0, mixed)),
        Err(StoreError::MixedSessions)
    );
}

#[test]
fn pages_preserve_sequence_order_instead_of_timestamp_order() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-order");
    let events = initial_events(&session, "content");
    store
        .append(&AppendRequest::new(session.clone(), 0, events))
        .expect("append events");

    let mut pages = VecDeque::new();
    let mut after = 0;
    loop {
        let page = store
            .load_events(&session, after, 1)
            .expect("load one ordered page");
        let Some(event) = page.into_iter().next() else {
            break;
        };
        after = event.sequence().get();
        pages.push_back((after, event.occurred_at().get()));
    }

    assert_eq!(pages, [(1, 300), (2, 200), (3, 100)]);
}

#[test]
fn attempt_lifecycle_and_assistant_transcript_are_projected_durably() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-attempt");
    let mut events = initial_events(&session, "hello");
    events.extend([
        event(
            "event-4",
            &session,
            4,
            "command-prepare",
            400,
            EventPayload::AttemptPrepared {
                attempt_id: attempt_id("attempt-1"),
                input_id: input_id("input-1"),
                model: model(),
                retry_of: None,
            },
        ),
        event(
            "event-5",
            &session,
            5,
            "command-start",
            500,
            EventPayload::AttemptStarted {
                attempt_id: attempt_id("attempt-1"),
            },
        ),
        event(
            "event-6",
            &session,
            6,
            "command-text-1",
            600,
            EventPayload::AttemptTextAppended {
                attempt_id: attempt_id("attempt-1"),
                text: ResponseText::new(" ").expect("whitespace delta is valid"),
            },
        ),
        event(
            "event-7",
            &session,
            7,
            "command-text-2",
            700,
            EventPayload::AttemptTextAppended {
                attempt_id: attempt_id("attempt-1"),
                text: ResponseText::new("こんにちは").expect("non-empty delta"),
            },
        ),
        event(
            "event-8",
            &session,
            8,
            "command-usage",
            800,
            EventPayload::AttemptUsageRecorded {
                attempt_id: attempt_id("attempt-1"),
                usage: UsageSnapshot::new(Some(3), Some(4), Some(7)),
            },
        ),
        event(
            "event-9",
            &session,
            9,
            "command-complete",
            900,
            EventPayload::AttemptCompleted {
                attempt_id: attempt_id("attempt-1"),
            },
        ),
    ]);

    store
        .append(&AppendRequest::new(session.clone(), 0, events.clone()))
        .expect("append complete attempt");

    let inputs = store
        .load_admitted_inputs(&session)
        .expect("load promoted input");
    assert_eq!(inputs[0].state(), InputState::Promoted);

    let attempts = store.load_attempts(&session).expect("load attempts");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_id(), &attempt_id("attempt-1"));
    assert_eq!(attempts[0].input_id(), &input_id("input-1"));
    assert_eq!(attempts[0].state(), AttemptState::Completed);
    assert_eq!(
        attempts[0].usage(),
        Some(UsageSnapshot::new(Some(3), Some(4), Some(7)))
    );
    assert_eq!(attempts[0].started_at(), Some(TimestampMillis::new(500)));
    assert_eq!(attempts[0].settled_at(), Some(TimestampMillis::new(900)));

    let transcript = store
        .load_transcript(&session)
        .expect("load assistant transcript");
    assert_eq!(transcript.len(), 2);
    assert_eq!(
        transcript[1].source(),
        &TranscriptSource::Attempt(attempt_id("attempt-1"))
    );
    assert_eq!(transcript[1].role(), TranscriptRole::Assistant);
    assert_eq!(transcript[1].state(), TranscriptState::Complete);
    assert_eq!(transcript[1].content().as_str(), " こんにちは");

    store
        .connection
        .execute(
            "UPDATE provider_attempts SET started_at_ms = 501 WHERE attempt_id = 'attempt-1'",
            [],
        )
        .expect("corrupt attempt projection timestamp");
    assert_eq!(
        store.load_attempts(&session),
        Err(StoreError::CorruptData {
            area: CorruptionArea::AttemptProjection
        })
    );
    store
        .rebuild_projections()
        .expect("rebuild attempt projections");
    assert_eq!(
        store.load_attempts(&session).expect("load rebuilt attempt")[0].state(),
        AttemptState::Completed
    );
    assert_eq!(
        store
            .load_transcript(&session)
            .expect("load rebuilt assistant transcript")[1]
            .content()
            .as_str(),
        " こんにちは"
    );

    drop(store);
    let reopened = database.open();
    assert_eq!(
        reopened
            .load_events(&session, 0, 32)
            .expect("replay attempt after reopen"),
        events
    );
    assert_eq!(
        reopened
            .load_attempts(&session)
            .expect("load attempt after reopen")[0]
            .state(),
        AttemptState::Completed
    );
}

#[test]
fn attempt_projection_rolls_back_with_its_preparation_event() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-attempt-rollback");
    store
        .append(&AppendRequest::new(
            session.clone(),
            0,
            initial_events(&session, "hello"),
        ))
        .expect("append admitted input");
    let prepared = event(
        "event-prepare-rollback",
        &session,
        4,
        "command-prepare-rollback",
        400,
        EventPayload::AttemptPrepared {
            attempt_id: attempt_id("attempt-rollback"),
            input_id: input_id("input-1"),
            model: model(),
            retry_of: None,
        },
    );
    store.set_failure_point(Some(FailurePoint::ProjectionApplied));

    assert_eq!(
        store.append(&AppendRequest::new(session.clone(), 3, vec![prepared])),
        Err(StoreError::Backend)
    );
    store.set_failure_point(None);
    assert!(
        store
            .load_attempts(&session)
            .expect("load rolled-back attempts")
            .is_empty()
    );
    assert_eq!(
        store
            .load_admitted_inputs(&session)
            .expect("load input after rollback")[0]
            .state(),
        InputState::Admitted
    );
    assert_eq!(
        store
            .load_events(&session, 0, 32)
            .expect("load events after rollback")
            .len(),
        3
    );
}

#[test]
fn recovery_can_mark_a_reopened_in_flight_attempt_unknown() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-unknown");
    let mut events = initial_events(&session, "hello");
    events.extend([
        event(
            "event-unknown-prepare",
            &session,
            4,
            "command-unknown-prepare",
            400,
            EventPayload::AttemptPrepared {
                attempt_id: attempt_id("attempt-unknown"),
                input_id: input_id("input-1"),
                model: model(),
                retry_of: None,
            },
        ),
        event(
            "event-unknown-start",
            &session,
            5,
            "command-unknown-start",
            500,
            EventPayload::AttemptStarted {
                attempt_id: attempt_id("attempt-unknown"),
            },
        ),
    ]);
    store
        .append(&AppendRequest::new(session.clone(), 0, events))
        .expect("append in-flight attempt");
    drop(store);

    let mut reopened = database.open();
    assert_eq!(
        reopened
            .load_attempts(&session)
            .expect("load in-flight attempt")[0]
            .state(),
        AttemptState::InFlight
    );
    let unknown = event(
        "event-unknown-settle",
        &session,
        6,
        "command-unknown-settle",
        600,
        EventPayload::AttemptMarkedUnknown {
            attempt_id: attempt_id("attempt-unknown"),
        },
    );
    reopened
        .append(&AppendRequest::new(session.clone(), 5, vec![unknown]))
        .expect("mark attempt unknown durably");

    assert_eq!(
        reopened
            .load_attempts(&session)
            .expect("load unknown attempt")[0]
            .state(),
        AttemptState::Unknown
    );
}

#[test]
fn a_newer_database_schema_fails_closed() {
    let database = TestDatabase::new();
    let store = database.open();
    store
        .connection
        .pragma_update(None, "user_version", 4)
        .expect("set future schema fixture");
    drop(store);

    assert_eq!(
        SqliteStore::open(&database.path).err(),
        Some(StoreError::NewerSchema {
            found: 4,
            supported: 3
        })
    );
}

#[test]
fn session_lifecycle_events_update_the_session_projection() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-lifecycle");

    store
        .append(&AppendRequest::new(
            session.clone(),
            0,
            vec![event(
                "event-1",
                &session,
                1,
                "command-create",
                300,
                EventPayload::SessionCreated,
            )],
        ))
        .expect("create session");
    store
        .append(&AppendRequest::new(
            session.clone(),
            1,
            vec![event(
                "event-2",
                &session,
                2,
                "command-rename",
                250,
                EventPayload::SessionRenamed {
                    title: autoharness_domain::SessionTitle::new("Deep dive").expect("title"),
                },
            )],
        ))
        .expect("rename session");
    store
        .append(&AppendRequest::new(
            session.clone(),
            2,
            vec![event(
                "event-3",
                &session,
                3,
                "command-archive",
                200,
                EventPayload::SessionArchived,
            )],
        ))
        .expect("archive session");

    let summaries = store.list_sessions().expect("list sessions");
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].status(),
        autoharness_store::SessionStatus::Archived
    );
    assert_eq!(
        summaries[0]
            .title()
            .map(autoharness_domain::SessionTitle::as_str),
        Some("Deep dive")
    );

    store
        .append(&AppendRequest::new(
            session.clone(),
            3,
            vec![event(
                "event-4",
                &session,
                4,
                "command-unarchive",
                150,
                EventPayload::SessionUnarchived,
            )],
        ))
        .expect("unarchive session");
    let summaries = store.list_sessions().expect("list sessions again");
    assert_eq!(
        summaries[0].status(),
        autoharness_store::SessionStatus::Active
    );
}

#[test]
fn projection_rebuild_restores_title_and_status_from_history() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-rebuild");

    let events = vec![
        event(
            "event-1",
            &session,
            1,
            "command-create",
            300,
            EventPayload::SessionCreated,
        ),
        event(
            "event-2",
            &session,
            2,
            "command-rename",
            250,
            EventPayload::SessionRenamed {
                title: autoharness_domain::SessionTitle::new("Kept title").expect("title"),
            },
        ),
        event(
            "event-3",
            &session,
            3,
            "command-archive",
            200,
            EventPayload::SessionArchived,
        ),
    ];
    store
        .append(&AppendRequest::new(session.clone(), 0, events))
        .expect("seed lifecycle history");

    // Corrupt the projection, then rebuild strictly from retained events.
    store
        .connection
        .execute(
            "UPDATE sessions SET status = 'active', title = NULL WHERE session_id = ?1",
            params![session.as_str()],
        )
        .expect("corrupt projection");
    store.rebuild_projections().expect("rebuild projections");

    let summaries = store.list_sessions().expect("list rebuilt sessions");
    assert_eq!(
        summaries[0].status(),
        autoharness_store::SessionStatus::Archived
    );
    assert_eq!(
        summaries[0]
            .title()
            .map(autoharness_domain::SessionTitle::as_str),
        Some("Kept title")
    );
}

#[test]
fn deletion_removes_the_session_and_every_dependent_row_atomically() {
    let database = TestDatabase::new();
    let mut store = database.open();
    let session = session_id("session-delete");
    let other = session_id("session-kept");

    let mut history = initial_events(&session, "delete me");
    history.push(event(
        "event-4",
        &session,
        4,
        "command-prepare",
        90,
        EventPayload::AttemptPrepared {
            attempt_id: attempt_id("attempt-1"),
            input_id: input_id("input-1"),
            model: model(),
            retry_of: None,
        },
    ));
    history.push(event(
        "event-5",
        &session,
        5,
        "command-start",
        80,
        EventPayload::AttemptStarted {
            attempt_id: attempt_id("attempt-1"),
        },
    ));
    history.push(EventEnvelope::new_v1(
        event_id("event-6"),
        session.clone(),
        SessionSequence::new(6).expect("sequence"),
        TimestampMillis::new(70),
        Causation::Event(event_id("event-5")),
        correlation_id("correlation-1"),
        EventPayload::AttemptTextAppended {
            attempt_id: attempt_id("attempt-1"),
            text: ResponseText::new("answer").expect("non-empty response"),
        },
    ));
    history.push(event(
        "event-7",
        &session,
        7,
        "command-complete",
        60,
        EventPayload::AttemptCompleted {
            attempt_id: attempt_id("attempt-1"),
        },
    ));
    store
        .append(&AppendRequest::new(session.clone(), 0, history))
        .expect("seed settled session");

    store
        .append(&AppendRequest::new(
            other.clone(),
            0,
            vec![
                event(
                    "event-kept-1",
                    &other,
                    1,
                    "command-create-kept",
                    300,
                    EventPayload::SessionCreated,
                ),
                event(
                    "event-kept-2",
                    &other,
                    2,
                    "command-select-kept",
                    200,
                    EventPayload::ModelSelected { model: model() },
                ),
            ],
        ))
        .expect("seed kept session");

    // Version mismatch refuses to delete.
    assert!(matches!(
        store.delete_session(&session, 3),
        Err(StoreError::VersionConflict { .. })
    ));

    // Correct version deletes every dependent row in one transaction.
    assert_eq!(
        store.delete_session(&session, 7),
        Ok(autoharness_store::DeletionDisposition::Deleted)
    );
    assert_eq!(
        store.delete_session(&session, 7),
        Ok(autoharness_store::DeletionDisposition::NotFound)
    );

    assert!(store.list_sessions().expect("list after delete").len() == 1);
    assert!(
        store
            .load_events(&session, 0, 32)
            .expect("deleted history is gone")
            .is_empty()
    );
    assert_eq!(
        store.list_sessions().expect("list kept")[0].session_id(),
        &other
    );

    // The kept session remains fully readable and replayable.
    let reopened = database.open();
    assert_eq!(
        reopened.list_sessions().expect("reopen list").len(),
        1,
        "deletion is durable across reopen"
    );
}
