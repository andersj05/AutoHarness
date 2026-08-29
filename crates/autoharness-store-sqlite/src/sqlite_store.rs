use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use autoharness_domain::{
    AttemptFailure, AttemptId, Causation, DeliveryMode, EVENT_SCHEMA_V1, EventEnvelope,
    EventPayload, InputId, ModelId, ModelRef, PromptText, ProviderId, SessionId, SessionSequence,
    SessionTitle, TimestampMillis, UsageSnapshot,
};
use autoharness_store::{
    AdmittedInputRecord, AppendDisposition, AppendReceipt, AppendRequest, AttemptRecord,
    AttemptState, CorruptionArea, DeletionDisposition, IdentityKind, InputState, SessionStatus,
    SessionStore, SessionSummary, StoreError, TranscriptEntry, TranscriptRole, TranscriptSource,
    TranscriptState, TranscriptText,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};

use crate::migration;

const MAX_SQLITE_SEQUENCE: u64 = i64::MAX as u64;
const FULL_SYNCHRONOUS_LEVEL: i64 = 2;
const MAX_CATALOG_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// Integrity-checked provider-neutral model-catalog cache record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteCatalogCacheRecord {
    provider_id: ProviderId,
    schema_version: u16,
    refreshed_at_ms: i64,
    catalog_json: Vec<u8>,
}

impl SqliteCatalogCacheRecord {
    /// Constructs a cache record whose payload is validated when stored.
    #[must_use]
    pub const fn new(
        provider_id: ProviderId,
        schema_version: u16,
        refreshed_at_ms: i64,
        catalog_json: Vec<u8>,
    ) -> Self {
        Self {
            provider_id,
            schema_version,
            refreshed_at_ms,
            catalog_json,
        }
    }

    /// Returns the provider-project cache identity.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Returns the provider catalog schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the live refresh observation time.
    #[must_use]
    pub const fn refreshed_at_ms(&self) -> i64 {
        self.refreshed_at_ms
    }

    /// Returns the exact serialized provider-neutral catalog payload.
    #[must_use]
    pub fn catalog_json(&self) -> &[u8] {
        &self.catalog_json
    }
}

/// SQLite connection options whose defaults favor durable local operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqliteStoreOptions {
    busy_timeout: Duration,
}

impl SqliteStoreOptions {
    /// Creates options with a caller-selected bounded lock wait.
    #[must_use]
    pub const fn new(busy_timeout: Duration) -> Self {
        Self { busy_timeout }
    }

    /// Returns the configured lock wait.
    #[must_use]
    pub const fn busy_timeout(self) -> Duration {
        self.busy_timeout
    }
}

impl Default for SqliteStoreOptions {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
        }
    }
}

/// Verified connection settings applied by [`SqliteStore::open`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqliteConfiguration {
    journal_mode: String,
    synchronous_level: i64,
    foreign_keys: bool,
    trusted_schema: bool,
    fts5: bool,
    busy_timeout_ms: u64,
}

impl SqliteConfiguration {
    /// Returns the normalized SQLite journal mode.
    #[must_use]
    pub fn journal_mode(&self) -> &str {
        &self.journal_mode
    }

    /// Returns SQLite's numeric synchronous setting.
    #[must_use]
    pub const fn synchronous_level(&self) -> i64 {
        self.synchronous_level
    }

    /// Returns whether foreign-key enforcement is enabled.
    #[must_use]
    pub const fn foreign_keys(&self) -> bool {
        self.foreign_keys
    }

    /// Returns whether SQLite trusts schema-defined functions and virtual tables.
    #[must_use]
    pub const fn trusted_schema(&self) -> bool {
        self.trusted_schema
    }

    /// Returns whether the bundled SQLite engine provides FTS5.
    #[must_use]
    pub const fn fts5(&self) -> bool {
        self.fts5
    }

    /// Returns the verified busy timeout in milliseconds.
    #[must_use]
    pub const fn busy_timeout_ms(&self) -> u64 {
        self.busy_timeout_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailurePoint {
    FirstEventInserted,
    ProjectionApplied,
    CommitCompleted,
}

/// A single-connection SQLite durable session store.
///
/// The application should own this synchronous adapter on one storage task or
/// thread. The type is not a cross-process session-writer lease.
pub struct SqliteStore {
    pub(crate) connection: Connection,
    configuration: SqliteConfiguration,
    failure_point: Option<FailurePoint>,
}

impl SqliteStore {
    /// Opens, configures, verifies, and migrates a file-backed SQLite database.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_options(path, SqliteStoreOptions::default())
    }

    /// Opens a file-backed SQLite database using explicit connection options.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: SqliteStoreOptions,
    ) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path).map_err(map_sqlite_error)?;
        let configuration = configure(&connection, options)?;
        migration::apply(&mut connection)?;

        Ok(Self {
            connection,
            configuration,
            failure_point: None,
        })
    }

    /// Returns the verified SQLite durability configuration.
    #[must_use]
    pub const fn configuration(&self) -> &SqliteConfiguration {
        &self.configuration
    }

    /// Loads and integrity-checks one durable provider-neutral catalog cache record.
    pub fn load_catalog_cache(
        &mut self,
        provider_id: &ProviderId,
    ) -> Result<Option<SqliteCatalogCacheRecord>, StoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT schema_version, refreshed_at_ms, catalog_json, content_sha256 \
                 FROM model_catalog_cache WHERE provider_id = ?1",
                params![provider_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((schema_version, refreshed_at_ms, catalog_json, content_sha256)) = row else {
            return Ok(None);
        };
        let schema_version =
            u16::try_from(schema_version).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::CatalogCache,
            })?;
        if schema_version == 0
            || catalog_json.is_empty()
            || catalog_json.len() > MAX_CATALOG_CACHE_BYTES
            || Sha256::digest(&catalog_json).as_slice() != content_sha256.as_slice()
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::CatalogCache,
            });
        }
        Ok(Some(SqliteCatalogCacheRecord::new(
            provider_id.clone(),
            schema_version,
            refreshed_at_ms,
            catalog_json,
        )))
    }

    /// Transactionally replaces one provider-project catalog cache record.
    pub fn replace_catalog_cache(
        &mut self,
        record: &SqliteCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        if record.schema_version == 0
            || record.catalog_json.is_empty()
            || record.catalog_json.len() > MAX_CATALOG_CACHE_BYTES
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::CatalogCache,
            });
        }
        let content_sha256 = Sha256::digest(&record.catalog_json);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO model_catalog_cache \
                 (provider_id, schema_version, refreshed_at_ms, catalog_json, content_sha256) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(provider_id) DO UPDATE SET \
                    schema_version = excluded.schema_version, \
                    refreshed_at_ms = excluded.refreshed_at_ms, \
                    catalog_json = excluded.catalog_json, \
                    content_sha256 = excluded.content_sha256",
                params![
                    record.provider_id.as_str(),
                    i64::from(record.schema_version),
                    record.refreshed_at_ms,
                    &record.catalog_json,
                    content_sha256.as_slice(),
                ],
            )
            .map_err(map_sqlite_error)?;
        transaction.commit().map_err(map_sqlite_error)
    }

    #[cfg(test)]
    fn set_failure_point(&mut self, failure_point: Option<FailurePoint>) {
        self.failure_point = failure_point;
    }
}

impl SessionStore for SqliteStore {
    fn append(&mut self, request: &AppendRequest) -> Result<AppendReceipt, StoreError> {
        let encoded_events = validate_and_encode(request)?;
        let final_sequence = encoded_events
            .last()
            .map(|event| event.event.sequence().get())
            .ok_or(StoreError::EmptyAppend)?;
        let failure_point = self.failure_point;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;

        match existing_batch_state(&transaction, &encoded_events)? {
            ExistingBatchState::None => {}
            ExistingBatchState::All => {
                let durable_sequence = current_session_version(&transaction, request.session_id())?;
                if durable_sequence < final_sequence {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::SessionProjection,
                    });
                }
                transaction.commit().map_err(map_sqlite_error)?;
                return Ok(AppendReceipt::new(
                    AppendDisposition::AlreadyCommitted,
                    durable_sequence,
                ));
            }
            ExistingBatchState::Partial => {
                return Err(StoreError::IdentityConflict {
                    kind: IdentityKind::Event,
                });
            }
        }

        let actual_version = current_session_version(&transaction, request.session_id())?;
        if actual_version != request.expected_last_sequence() {
            return Err(StoreError::VersionConflict {
                session_id: request.session_id().clone(),
                expected: request.expected_last_sequence(),
                actual: actual_version,
            });
        }

        if actual_version == 0 {
            let first = encoded_events.first().ok_or(StoreError::EmptyAppend)?;
            if !matches!(first.event.payload(), EventPayload::SessionCreated) {
                return Err(StoreError::InvalidSessionTransition);
            }
            transaction
                .execute(
                    "INSERT INTO sessions (\
                        session_id, status, selected_provider_id, selected_model_id, \
                        last_sequence, created_at_ms, updated_at_ms\
                     ) VALUES (?1, 'active', NULL, NULL, 0, ?2, ?2)",
                    params![
                        request.session_id().as_str(),
                        first.event.occurred_at().get()
                    ],
                )
                .map_err(map_sqlite_error)?;
        }

        for (index, encoded) in encoded_events.iter().enumerate() {
            validate_durable_causation(&transaction, encoded.event)?;
            insert_event(&transaction, encoded)?;

            if index == 0 && failure_point == Some(FailurePoint::FirstEventInserted) {
                return Err(StoreError::Backend);
            }

            apply_projection(&transaction, encoded.event, ProjectionMode::Append)?;
            transaction
                .execute(
                    "UPDATE sessions \
                     SET last_sequence = ?2, updated_at_ms = ?3 \
                     WHERE session_id = ?1",
                    params![
                        request.session_id().as_str(),
                        to_sql_sequence(encoded.event.sequence().get())?,
                        encoded.event.occurred_at().get()
                    ],
                )
                .map_err(map_sqlite_error)?;
        }

        if failure_point == Some(FailurePoint::ProjectionApplied) {
            return Err(StoreError::Backend);
        }

        transaction.commit().map_err(map_sqlite_error)?;

        if failure_point == Some(FailurePoint::CommitCompleted) {
            return Err(StoreError::Backend);
        }

        Ok(AppendReceipt::new(
            AppendDisposition::Committed,
            final_sequence,
        ))
    }

    fn load_events(
        &self,
        session_id: &SessionId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<EventEnvelope>, StoreError> {
        if limit == 0 || after_sequence == MAX_SQLITE_SEQUENCE {
            return Ok(Vec::new());
        }
        let after_sequence = to_sql_sequence(after_sequence)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT \
                    e.event_id, e.session_id, e.sequence, e.schema_version, e.occurred_at_ms, \
                    e.caused_by_command_id, e.caused_by_event_id, e.correlation_id, \
                    e.event_kind, e.envelope_json, cause.sequence \
                 FROM session_events AS e \
                 LEFT JOIN session_events AS cause \
                   ON cause.session_id = e.session_id \
                  AND cause.event_id = e.caused_by_event_id \
                 WHERE e.session_id = ?1 AND e.sequence > ?2 \
                 ORDER BY e.sequence ASC \
                 LIMIT ?3",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(
                params![session_id.as_str(), after_sequence, i64::from(limit)],
                StoredEventRow::from_row,
            )
            .map_err(map_sqlite_error)?;

        let mut expected = u64::try_from(after_sequence)
            .map_err(|_| StoreError::SequenceOutOfRange)?
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
        let mut events = Vec::new();
        for row in rows {
            let row = row.map_err(map_sqlite_error)?;
            let event = row.validate()?;
            if event.sequence().get() != expected {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::EventSequence,
                });
            }
            expected = expected
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
            events.push(event);
        }
        Ok(events)
    }

    fn list_sessions(&self) -> Result<Vec<SessionSummary>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT \
                    s.session_id, s.status, s.title, s.selected_provider_id, s.selected_model_id, \
                    s.last_sequence, s.created_at_ms, s.updated_at_ms, \
                    (SELECT MIN(e.sequence) FROM session_events AS e WHERE e.session_id = s.session_id), \
                    (SELECT MAX(e.sequence) FROM session_events AS e WHERE e.session_id = s.session_id), \
                    (SELECT COUNT(*) FROM session_events AS e WHERE e.session_id = s.session_id), \
                    (SELECT COUNT(*) FROM transcript_messages AS m WHERE m.session_id = s.session_id) \
                 FROM sessions AS s \
                 ORDER BY s.updated_at_ms DESC, s.session_id ASC",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(SessionProjectionRow {
                    session_id: row.get(0)?,
                    status: row.get(1)?,
                    title: row.get(2)?,
                    selected_provider_id: row.get(3)?,
                    selected_model_id: row.get(4)?,
                    last_sequence: row.get(5)?,
                    created_at_ms: row.get(6)?,
                    updated_at_ms: row.get(7)?,
                    minimum_event_sequence: row.get(8)?,
                    maximum_event_sequence: row.get(9)?,
                    event_count: row.get(10)?,
                    message_count: row.get(11)?,
                })
            })
            .map_err(map_sqlite_error)?;

        rows.map(|row| row.map_err(map_sqlite_error)?.validate())
            .collect()
    }

    fn load_admitted_inputs(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<AdmittedInputRecord>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT \
                    input_id, admitted_event_id, admitted_sequence, delivery_mode, state, \
                    prompt_utf8, content_sha256, admitted_at_ms, promoted_at_ms, \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = admitted_inputs.admitted_event_id) \
                 FROM admitted_inputs \
                 WHERE session_id = ?1 \
                 ORDER BY admitted_sequence ASC",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![session_id.as_str()], |row| {
                Ok(InputProjectionRow {
                    input_id: row.get(0)?,
                    event_id: row.get(1)?,
                    sequence: row.get(2)?,
                    delivery_mode: row.get(3)?,
                    state: row.get(4)?,
                    prompt_utf8: row.get(5)?,
                    content_sha256: row.get(6)?,
                    admitted_at_ms: row.get(7)?,
                    promoted_at_ms: row.get(8)?,
                    event_json: row.get(9)?,
                })
            })
            .map_err(map_sqlite_error)?;

        rows.map(|row| row.map_err(map_sqlite_error)?.validate(session_id.clone()))
            .collect()
    }

    fn load_attempts(&self, session_id: &SessionId) -> Result<Vec<AttemptRecord>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT \
                    attempt_id, input_id, provider_id, model_id, retry_of_attempt_id, state, \
                    prepared_event_id, prepared_sequence, started_event_id, settled_event_id, \
                    cancellation_requested_event_id, usage_event_id, prepared_at_ms, started_at_ms, \
                    settled_at_ms, cancellation_requested_at_ms, usage_json, failure_json, \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = provider_attempts.prepared_event_id), \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = provider_attempts.started_event_id), \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = provider_attempts.settled_event_id), \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = provider_attempts.cancellation_requested_event_id), \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = provider_attempts.usage_event_id) \
                 FROM provider_attempts \
                 WHERE session_id = ?1 \
                 ORDER BY prepared_sequence ASC",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![session_id.as_str()], |row| {
                Ok(AttemptProjectionRow {
                    attempt_id: row.get(0)?,
                    input_id: row.get(1)?,
                    provider_id: row.get(2)?,
                    model_id: row.get(3)?,
                    retry_of_attempt_id: row.get(4)?,
                    state: row.get(5)?,
                    prepared_event_id: row.get(6)?,
                    prepared_sequence: row.get(7)?,
                    started_event_id: row.get(8)?,
                    settled_event_id: row.get(9)?,
                    cancellation_requested_event_id: row.get(10)?,
                    usage_event_id: row.get(11)?,
                    prepared_at_ms: row.get(12)?,
                    started_at_ms: row.get(13)?,
                    settled_at_ms: row.get(14)?,
                    cancellation_requested_at_ms: row.get(15)?,
                    usage_json: row.get(16)?,
                    failure_json: row.get(17)?,
                    prepared_event_json: row.get(18)?,
                    started_event_json: row.get(19)?,
                    settled_event_json: row.get(20)?,
                    cancellation_requested_event_json: row.get(21)?,
                    usage_event_json: row.get(22)?,
                })
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| row.map_err(map_sqlite_error)?.validate(session_id.clone()))
            .collect()
    }

    fn load_transcript(&self, session_id: &SessionId) -> Result<Vec<TranscriptEntry>, StoreError> {
        let messages = load_transcript_messages(&self.connection, session_id)?;
        messages
            .into_iter()
            .map(|message| message.validate(&self.connection, session_id))
            .collect()
    }

    fn rebuild_projections(&mut self) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;
        let stored_rows = load_all_stored_event_rows(&transaction)?;
        let mut streams: BTreeMap<SessionId, Vec<EventEnvelope>> = BTreeMap::new();
        for row in stored_rows {
            let event = row.validate()?;
            streams
                .entry(event.session_id().clone())
                .or_default()
                .push(event);
        }

        validate_complete_streams(&streams)?;

        transaction
            .execute_batch(
                "DELETE FROM context_turn_bindings; \
                 DELETE FROM transcript_segments; \
                 DELETE FROM transcript_messages; \
                 UPDATE provider_attempts \
                 SET state = 'prepared', started_event_id = NULL, settled_event_id = NULL, \
                     cancellation_requested_event_id = NULL, usage_event_id = NULL, \
                     started_at_ms = NULL, settled_at_ms = NULL, \
                     cancellation_requested_at_ms = NULL, usage_json = NULL, failure_json = NULL; \
                 UPDATE admitted_inputs SET state = 'admitted', promoted_at_ms = NULL; \
                 UPDATE sessions \
                 SET status = 'active', title = NULL, selected_provider_id = NULL, \
                     selected_model_id = NULL, last_sequence = 0, created_at_ms = 0, \
                     updated_at_ms = 0;",
            )
            .map_err(map_sqlite_error)?;

        for events in streams.values() {
            for event in events {
                apply_projection(&transaction, event, ProjectionMode::Rebuild)?;
                transaction
                    .execute(
                        "UPDATE sessions \
                         SET last_sequence = ?2, updated_at_ms = ?3 \
                         WHERE session_id = ?1",
                        params![
                            event.session_id().as_str(),
                            to_sql_sequence(event.sequence().get())?,
                            event.occurred_at().get()
                        ],
                    )
                    .map_err(map_sqlite_error)?;
            }
        }

        let unprojected_sessions = transaction
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE last_sequence = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sqlite_error)?;
        if unprojected_sessions != 0 {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::SessionProjection,
            });
        }

        transaction.commit().map_err(map_sqlite_error)
    }

    fn delete_session(
        &mut self,
        session_id: &SessionId,
        expected_last_sequence: u64,
    ) -> Result<DeletionDisposition, StoreError> {
        if expected_last_sequence > MAX_SQLITE_SEQUENCE {
            return Err(StoreError::SequenceOutOfRange);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;

        let existing: Option<(i64, String, i64)> = transaction
            .query_row(
                "SELECT last_sequence, status, updated_at_ms FROM sessions WHERE session_id = ?1",
                params![session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((last_sequence, _status, updated_at_ms)) = existing else {
            return Ok(DeletionDisposition::NotFound);
        };
        let actual_version = u64::try_from(last_sequence).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::SessionProjection,
        })?;
        if actual_version != expected_last_sequence {
            return Err(StoreError::VersionConflict {
                session_id: session_id.clone(),
                expected: expected_last_sequence,
                actual: actual_version,
            });
        }
        let unsettled = transaction
            .query_row(
                "SELECT COUNT(*) FROM provider_attempts \
                 WHERE session_id = ?1 \
                   AND state IN ('prepared', 'in_flight')",
                params![session_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sqlite_error)?;
        if unsettled != 0 {
            return Err(StoreError::InvalidSessionTransition);
        }

        crate::memory_store::erase_session_memory_and_evidence(
            &transaction,
            session_id,
            actual_version,
            updated_at_ms,
        )?;

        // Delete every dependent row inside this transaction, deepest first,
        // so a crash can never leave orphaned projections or events behind.
        // The retained event stream is authoritative history; deletion is an
        // explicit user request and removes replay data irreversibly.
        for statement in [
            "DELETE FROM context_turn_bindings WHERE session_id = ?1",
            "DELETE FROM context_admission_reasons WHERE admission_id IN (\
                SELECT a.admission_id FROM context_admissions AS a \
                JOIN context_turns AS t ON t.context_turn_id = a.context_turn_id \
                WHERE t.session_id = ?1\
             )",
            "DELETE FROM context_admissions WHERE context_turn_id IN (\
                SELECT context_turn_id FROM context_turns WHERE session_id = ?1\
             )",
            "DELETE FROM context_turn_sources WHERE context_turn_id IN (\
                SELECT context_turn_id FROM context_turns WHERE session_id = ?1\
             )",
            "DELETE FROM context_turns WHERE session_id = ?1",
            "DELETE FROM context_compaction_boundaries WHERE session_id = ?1",
            "DELETE FROM context_epochs WHERE session_id = ?1",
            "DELETE FROM transcript_segments WHERE session_id = ?1",
            "DELETE FROM transcript_messages WHERE session_id = ?1",
            "DELETE FROM provider_attempts WHERE session_id = ?1",
            "DELETE FROM admitted_inputs WHERE session_id = ?1",
        ] {
            transaction
                .execute(statement, params![session_id.as_str()])
                .map_err(map_sqlite_error)?;
        }

        // Event causation is a same-session DAG enforced with RESTRICT.
        // Remove leaves before their causes so multi-event command batches
        // remain deletable without weakening foreign-key enforcement.
        let event_ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT event_id FROM session_events \
                     WHERE session_id = ?1 ORDER BY sequence DESC",
                )
                .map_err(map_sqlite_error)?;
            let rows = statement
                .query_map(params![session_id.as_str()], |row| row.get::<_, String>(0))
                .map_err(map_sqlite_error)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlite_error)?
        };
        for event_id in event_ids {
            let changed = transaction
                .execute(
                    "DELETE FROM session_events WHERE session_id = ?1 AND event_id = ?2",
                    params![session_id.as_str(), event_id],
                )
                .map_err(map_sqlite_error)?;
            if changed != 1 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::Event,
                });
            }
        }
        let changed = transaction
            .execute(
                "DELETE FROM sessions WHERE session_id = ?1",
                params![session_id.as_str()],
            )
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::SessionProjection,
            });
        }

        transaction.commit().map_err(map_sqlite_error)?;
        Ok(DeletionDisposition::Deleted)
    }
}

#[derive(Debug)]
struct EncodedEvent<'event> {
    event: &'event EventEnvelope,
    envelope_json: Vec<u8>,
}

fn validate_and_encode(request: &AppendRequest) -> Result<Vec<EncodedEvent<'_>>, StoreError> {
    if request.events().is_empty() {
        return Err(StoreError::EmptyAppend);
    }
    if request.expected_last_sequence() > MAX_SQLITE_SEQUENCE {
        return Err(StoreError::SequenceOutOfRange);
    }

    let mut event_ids = BTreeSet::new();
    let mut command_ids = BTreeSet::new();
    let mut preceding_event_ids = BTreeSet::new();
    let mut expected = request
        .expected_last_sequence()
        .checked_add(1)
        .ok_or(StoreError::SequenceOutOfRange)?;
    let mut encoded = Vec::with_capacity(request.events().len());

    for (index, event) in request.events().iter().enumerate() {
        if event.session_id() != request.session_id() {
            return Err(StoreError::MixedSessions);
        }
        if event.schema_version() != EVENT_SCHEMA_V1 {
            return Err(StoreError::UnsupportedEventSchema {
                found: event.schema_version(),
            });
        }
        if event.sequence().get() != expected || expected > MAX_SQLITE_SEQUENCE {
            return Err(StoreError::NonContiguousBatch);
        }
        if !event_ids.insert(event.event_id().clone()) {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::Event,
            });
        }
        match event.causation() {
            Causation::Command(command_id) => {
                if !command_ids.insert(command_id.clone()) {
                    return Err(StoreError::IdentityConflict {
                        kind: IdentityKind::Command,
                    });
                }
            }
            Causation::Event(event_id) if event_ids.contains(event_id) => {
                if !preceding_event_ids.contains(event_id) {
                    return Err(StoreError::InvalidCausation);
                }
            }
            Causation::Event(_) => {}
        }
        if matches!(event.payload(), EventPayload::SessionCreated)
            && (request.expected_last_sequence() > 0 || index > 0)
        {
            return Err(StoreError::InvalidSessionTransition);
        }
        if request.expected_last_sequence() == 0
            && index == 0
            && event.sequence() != SessionSequence::FIRST
        {
            return Err(StoreError::InvalidSessionTransition);
        }

        let envelope_json = serde_json::to_vec(event).map_err(|_| StoreError::Backend)?;
        encoded.push(EncodedEvent {
            event,
            envelope_json,
        });
        preceding_event_ids.insert(event.event_id().clone());
        expected = expected
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
    }

    if request.expected_last_sequence() == 0
        && !matches!(
            request.events().first().map(EventEnvelope::payload),
            Some(EventPayload::SessionCreated)
        )
    {
        return Err(StoreError::InvalidSessionTransition);
    }

    Ok(encoded)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingBatchState {
    None,
    Partial,
    All,
}

fn existing_batch_state(
    transaction: &Transaction<'_>,
    events: &[EncodedEvent<'_>],
) -> Result<ExistingBatchState, StoreError> {
    let mut found = 0_usize;
    for event in events {
        let existing = transaction
            .query_row(
                "SELECT \
                    e.event_id, e.session_id, e.sequence, e.schema_version, e.occurred_at_ms, \
                    e.caused_by_command_id, e.caused_by_event_id, e.correlation_id, \
                    e.event_kind, e.envelope_json, cause.sequence \
                 FROM session_events AS e \
                 LEFT JOIN session_events AS cause \
                   ON cause.session_id = e.session_id AND cause.event_id = e.caused_by_event_id \
                 WHERE e.event_id = ?1",
                params![event.event.event_id().as_str()],
                StoredEventRow::from_row,
            )
            .optional()
            .map_err(map_sqlite_error)?;
        if let Some(existing) = existing {
            if existing.envelope_json.as_slice() != event.envelope_json.as_slice() {
                return Err(StoreError::IdentityConflict {
                    kind: IdentityKind::Event,
                });
            }
            let existing_event = existing.validate()?;
            if &existing_event != event.event {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::Event,
                });
            }
            found += 1;
        }
    }

    Ok(if found == 0 {
        ExistingBatchState::None
    } else if found == events.len() {
        ExistingBatchState::All
    } else {
        ExistingBatchState::Partial
    })
}

fn current_session_version(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
) -> Result<u64, StoreError> {
    let sequence = transaction
        .query_row(
            "SELECT last_sequence FROM sessions WHERE session_id = ?1",
            params![session_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .unwrap_or(0);
    u64::try_from(sequence).map_err(|_| StoreError::CorruptData {
        area: CorruptionArea::SessionProjection,
    })
}

fn validate_durable_causation(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
) -> Result<(), StoreError> {
    match event.causation() {
        Causation::Command(command_id) => {
            let exists = transaction
                .query_row(
                    "SELECT 1 FROM session_events WHERE caused_by_command_id = ?1",
                    params![command_id.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sqlite_error)?
                .is_some();
            if exists {
                return Err(StoreError::IdentityConflict {
                    kind: IdentityKind::Command,
                });
            }
        }
        Causation::Event(cause_id) => {
            let cause_sequence = transaction
                .query_row(
                    "SELECT sequence FROM session_events \
                     WHERE session_id = ?1 AND event_id = ?2",
                    params![event.session_id().as_str(), cause_id.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(map_sqlite_error)?;
            let Some(cause_sequence) = cause_sequence else {
                return Err(StoreError::InvalidCausation);
            };
            if cause_sequence >= to_sql_sequence(event.sequence().get())? {
                return Err(StoreError::InvalidCausation);
            }
        }
    }
    Ok(())
}

fn insert_event(
    transaction: &Transaction<'_>,
    encoded: &EncodedEvent<'_>,
) -> Result<(), StoreError> {
    let (caused_by_command_id, caused_by_event_id) = match encoded.event.causation() {
        Causation::Command(command_id) => (Some(command_id.as_str()), None),
        Causation::Event(event_id) => (None, Some(event_id.as_str())),
    };
    transaction
        .execute(
            "INSERT INTO session_events (\
                event_id, session_id, sequence, schema_version, occurred_at_ms, \
                caused_by_command_id, caused_by_event_id, correlation_id, event_kind, envelope_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                encoded.event.event_id().as_str(),
                encoded.event.session_id().as_str(),
                to_sql_sequence(encoded.event.sequence().get())?,
                i64::from(encoded.event.schema_version()),
                encoded.event.occurred_at().get(),
                caused_by_command_id,
                caused_by_event_id,
                encoded.event.correlation_id().as_str(),
                event_kind(encoded.event.payload()),
                &encoded.envelope_json
            ],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionMode {
    Append,
    Rebuild,
}

fn apply_projection(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
    mode: ProjectionMode,
) -> Result<(), StoreError> {
    match event.payload() {
        EventPayload::SessionCreated => {
            if mode == ProjectionMode::Rebuild {
                let changed = transaction
                    .execute(
                        "UPDATE sessions \
                         SET status = 'active', created_at_ms = ?2, updated_at_ms = ?2 \
                         WHERE session_id = ?1",
                        params![event.session_id().as_str(), event.occurred_at().get()],
                    )
                    .map_err(map_sqlite_error)?;
                if changed != 1 {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::SessionProjection,
                    });
                }
            }
        }
        EventPayload::SessionRenamed { title } => {
            let changed = transaction
                .execute(
                    "UPDATE sessions SET title = ?2 WHERE session_id = ?1",
                    params![event.session_id().as_str(), title.as_str()],
                )
                .map_err(map_sqlite_error)?;
            if changed != 1 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::SessionProjection,
                });
            }
        }
        EventPayload::SessionArchived => {
            let changed = transaction
                .execute(
                    "UPDATE sessions SET status = 'archived' WHERE session_id = ?1",
                    params![event.session_id().as_str()],
                )
                .map_err(map_sqlite_error)?;
            if changed != 1 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::SessionProjection,
                });
            }
        }
        EventPayload::SessionUnarchived => {
            let changed = transaction
                .execute(
                    "UPDATE sessions SET status = 'active' WHERE session_id = ?1",
                    params![event.session_id().as_str()],
                )
                .map_err(map_sqlite_error)?;
            if changed != 1 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::SessionProjection,
                });
            }
        }
        EventPayload::ModelSelected { model } => {
            let changed = transaction
                .execute(
                    "UPDATE sessions \
                     SET selected_provider_id = ?2, selected_model_id = ?3 \
                     WHERE session_id = ?1",
                    params![
                        event.session_id().as_str(),
                        model.provider_id().as_str(),
                        model.model_id().as_str()
                    ],
                )
                .map_err(map_sqlite_error)?;
            if changed != 1 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::SessionProjection,
                });
            }
        }
        EventPayload::InputAdmitted {
            input_id,
            prompt,
            delivery_mode,
        } => {
            let input_exists = transaction
                .query_row(
                    "SELECT 1 FROM admitted_inputs WHERE session_id = ?1 AND input_id = ?2",
                    params![event.session_id().as_str(), input_id.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sqlite_error)?
                .is_some();
            if input_exists && mode == ProjectionMode::Append {
                return Err(identity_projection_error(mode, IdentityKind::Input));
            }
            let content = prompt.as_str().as_bytes();
            let content_hash = Sha256::digest(content);
            if input_exists {
                let changed = transaction
                    .execute(
                        "UPDATE admitted_inputs SET admitted_event_id = ?3, \
                            admitted_sequence = ?4, delivery_mode = ?5, state = 'admitted', \
                            prompt_utf8 = ?6, content_sha256 = ?7, admitted_at_ms = ?8, \
                            promoted_at_ms = NULL \
                         WHERE session_id = ?1 AND input_id = ?2",
                        params![
                            event.session_id().as_str(),
                            input_id.as_str(),
                            event.event_id().as_str(),
                            to_sql_sequence(event.sequence().get())?,
                            delivery_mode_name(*delivery_mode),
                            content,
                            content_hash.as_slice(),
                            event.occurred_at().get()
                        ],
                    )
                    .map_err(map_sqlite_error)?;
                require_transition(changed, mode)?;
            } else {
                transaction
                    .execute(
                        "INSERT INTO admitted_inputs (\
                            session_id, input_id, admitted_event_id, admitted_sequence, \
                            delivery_mode, state, prompt_utf8, content_sha256, admitted_at_ms, \
                            promoted_at_ms\
                         ) VALUES (?1, ?2, ?3, ?4, ?5, 'admitted', ?6, ?7, ?8, NULL)",
                        params![
                            event.session_id().as_str(),
                            input_id.as_str(),
                            event.event_id().as_str(),
                            to_sql_sequence(event.sequence().get())?,
                            delivery_mode_name(*delivery_mode),
                            content,
                            content_hash.as_slice(),
                            event.occurred_at().get()
                        ],
                    )
                    .map_err(map_sqlite_error)?;
            }
            transaction
                .execute(
                    "INSERT INTO transcript_messages (\
                        session_id, source_kind, source_id, role, state, first_sequence, last_sequence\
                     ) VALUES (?1, 'input', ?2, 'user', 'complete', ?3, ?3)",
                    params![
                        event.session_id().as_str(),
                        input_id.as_str(),
                        to_sql_sequence(event.sequence().get())?
                    ],
                )
                .map_err(map_sqlite_error)?;
            transaction
                .execute(
                    "INSERT INTO transcript_segments (\
                        session_id, source_kind, source_id, event_sequence, source_event_id, \
                        content_utf8, content_sha256\
                     ) VALUES (?1, 'input', ?2, ?3, ?4, ?5, ?6)",
                    params![
                        event.session_id().as_str(),
                        input_id.as_str(),
                        to_sql_sequence(event.sequence().get())?,
                        event.event_id().as_str(),
                        content,
                        content_hash.as_slice()
                    ],
                )
                .map_err(map_sqlite_error)?;
        }
        EventPayload::AttemptPrepared {
            attempt_id,
            input_id,
            model,
            retry_of,
        } => {
            let attempt_exists = transaction
                .query_row(
                    "SELECT 1 FROM provider_attempts WHERE attempt_id = ?1",
                    params![attempt_id.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sqlite_error)?
                .is_some();
            if attempt_exists && mode == ProjectionMode::Append {
                return Err(identity_projection_error(mode, IdentityKind::Attempt));
            }
            if let Some(retry_of) = retry_of {
                let retry_state = transaction
                    .query_row(
                        "SELECT state FROM provider_attempts \
                         WHERE session_id = ?1 AND attempt_id = ?2",
                        params![event.session_id().as_str(), retry_of.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(map_sqlite_error)?;
                if !retry_state.is_some_and(|state| {
                    matches!(
                        state.as_str(),
                        "completed" | "failed" | "cancelled" | "unknown"
                    )
                }) {
                    return Err(transition_error(mode));
                }
            }
            let promoted = transaction
                .execute(
                    "UPDATE admitted_inputs \
                     SET state = 'promoted', promoted_at_ms = COALESCE(promoted_at_ms, ?3) \
                     WHERE session_id = ?1 AND input_id = ?2",
                    params![
                        event.session_id().as_str(),
                        input_id.as_str(),
                        event.occurred_at().get()
                    ],
                )
                .map_err(map_sqlite_error)?;
            require_transition(promoted, mode)?;
            if attempt_exists {
                let changed = transaction
                    .execute(
                        "UPDATE provider_attempts SET session_id = ?2, input_id = ?3, \
                            provider_id = ?4, model_id = ?5, retry_of_attempt_id = ?6, \
                            state = 'prepared', prepared_event_id = ?7, prepared_sequence = ?8, \
                            started_event_id = NULL, settled_event_id = NULL, \
                            cancellation_requested_event_id = NULL, usage_event_id = NULL, \
                            prepared_at_ms = ?9, started_at_ms = NULL, settled_at_ms = NULL, \
                            cancellation_requested_at_ms = NULL, usage_json = NULL, \
                            failure_json = NULL WHERE attempt_id = ?1",
                        params![
                            attempt_id.as_str(),
                            event.session_id().as_str(),
                            input_id.as_str(),
                            model.provider_id().as_str(),
                            model.model_id().as_str(),
                            retry_of.as_ref().map(AttemptId::as_str),
                            event.event_id().as_str(),
                            to_sql_sequence(event.sequence().get())?,
                            event.occurred_at().get()
                        ],
                    )
                    .map_err(map_sqlite_error)?;
                require_transition(changed, mode)?;
            } else {
                transaction
                    .execute(
                        "INSERT INTO provider_attempts (\
                            attempt_id, session_id, input_id, provider_id, model_id, \
                            retry_of_attempt_id, state, prepared_event_id, prepared_sequence, \
                            started_event_id, settled_event_id, cancellation_requested_event_id, \
                            usage_event_id, prepared_at_ms, started_at_ms, settled_at_ms, \
                            cancellation_requested_at_ms, usage_json, failure_json\
                         ) VALUES (\
                            ?1, ?2, ?3, ?4, ?5, ?6, 'prepared', ?7, ?8, \
                            NULL, NULL, NULL, NULL, ?9, NULL, NULL, NULL, NULL, NULL\
                         )",
                        params![
                            attempt_id.as_str(),
                            event.session_id().as_str(),
                            input_id.as_str(),
                            model.provider_id().as_str(),
                            model.model_id().as_str(),
                            retry_of.as_ref().map(AttemptId::as_str),
                            event.event_id().as_str(),
                            to_sql_sequence(event.sequence().get())?,
                            event.occurred_at().get()
                        ],
                    )
                    .map_err(map_sqlite_error)?;
            }
        }
        EventPayload::AttemptStarted { attempt_id } => {
            let changed = transaction
                .execute(
                    "UPDATE provider_attempts \
                     SET state = 'in_flight', started_event_id = ?3, started_at_ms = ?4 \
                     WHERE session_id = ?1 AND attempt_id = ?2 AND state = 'prepared'",
                    params![
                        event.session_id().as_str(),
                        attempt_id.as_str(),
                        event.event_id().as_str(),
                        event.occurred_at().get()
                    ],
                )
                .map_err(map_sqlite_error)?;
            require_transition(changed, mode)?;
        }
        EventPayload::AttemptTextAppended { attempt_id, text } => {
            require_attempt_in_flight(transaction, event.session_id(), attempt_id, mode)?;
            let message_exists = transaction
                .query_row(
                    "SELECT 1 FROM transcript_messages \
                     WHERE session_id = ?1 AND source_kind = 'attempt' AND source_id = ?2",
                    params![event.session_id().as_str(), attempt_id.as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sqlite_error)?
                .is_some();
            if message_exists {
                let changed = transaction
                    .execute(
                        "UPDATE transcript_messages \
                         SET last_sequence = ?3 \
                         WHERE session_id = ?1 AND source_kind = 'attempt' AND source_id = ?2 \
                           AND state = 'streaming'",
                        params![
                            event.session_id().as_str(),
                            attempt_id.as_str(),
                            to_sql_sequence(event.sequence().get())?
                        ],
                    )
                    .map_err(map_sqlite_error)?;
                require_transition(changed, mode)?;
            } else {
                transaction
                    .execute(
                        "INSERT INTO transcript_messages (\
                            session_id, source_kind, source_id, role, state, \
                            first_sequence, last_sequence\
                         ) VALUES (?1, 'attempt', ?2, 'assistant', 'streaming', ?3, ?3)",
                        params![
                            event.session_id().as_str(),
                            attempt_id.as_str(),
                            to_sql_sequence(event.sequence().get())?
                        ],
                    )
                    .map_err(map_sqlite_error)?;
            }
            let content = text.as_str().as_bytes();
            let content_hash = Sha256::digest(content);
            transaction
                .execute(
                    "INSERT INTO transcript_segments (\
                        session_id, source_kind, source_id, event_sequence, source_event_id, \
                        content_utf8, content_sha256\
                     ) VALUES (?1, 'attempt', ?2, ?3, ?4, ?5, ?6)",
                    params![
                        event.session_id().as_str(),
                        attempt_id.as_str(),
                        to_sql_sequence(event.sequence().get())?,
                        event.event_id().as_str(),
                        content,
                        content_hash.as_slice()
                    ],
                )
                .map_err(map_sqlite_error)?;
        }
        EventPayload::AttemptUsageRecorded { attempt_id, usage } => {
            let usage_json = serde_json::to_vec(usage).map_err(|_| StoreError::Backend)?;
            let changed = transaction
                .execute(
                    "UPDATE provider_attempts \
                     SET usage_json = ?3, usage_event_id = ?4 \
                     WHERE session_id = ?1 AND attempt_id = ?2 AND state = 'in_flight'",
                    params![
                        event.session_id().as_str(),
                        attempt_id.as_str(),
                        usage_json,
                        event.event_id().as_str()
                    ],
                )
                .map_err(map_sqlite_error)?;
            require_transition(changed, mode)?;
        }
        EventPayload::AttemptCancellationRequested { attempt_id } => {
            let changed = transaction
                .execute(
                    "UPDATE provider_attempts \
                     SET cancellation_requested_at_ms = ?3, \
                         cancellation_requested_event_id = ?4 \
                     WHERE session_id = ?1 AND attempt_id = ?2 AND state = 'in_flight' \
                       AND cancellation_requested_at_ms IS NULL",
                    params![
                        event.session_id().as_str(),
                        attempt_id.as_str(),
                        event.occurred_at().get(),
                        event.event_id().as_str()
                    ],
                )
                .map_err(map_sqlite_error)?;
            require_transition(changed, mode)?;
        }
        EventPayload::AttemptCompleted { attempt_id } => {
            settle_attempt(transaction, event, attempt_id, "completed", None, mode)?;
        }
        EventPayload::AttemptFailed {
            attempt_id,
            failure,
        } => {
            let failure_json = serde_json::to_vec(failure).map_err(|_| StoreError::Backend)?;
            settle_attempt(
                transaction,
                event,
                attempt_id,
                "failed",
                Some(failure_json),
                mode,
            )?;
        }
        EventPayload::AttemptCancelled { attempt_id } => {
            settle_attempt(transaction, event, attempt_id, "cancelled", None, mode)?;
        }
        EventPayload::AttemptMarkedUnknown { attempt_id } => {
            settle_attempt(transaction, event, attempt_id, "unknown", None, mode)?;
        }
        EventPayload::ContextTurnBound { .. } => {
            crate::context_store::apply_context_turn_binding(transaction, event)?;
        }
        EventPayload::RunTurnStarted { .. } => {
            crate::context_store::validate_run_turn_binding(transaction, event)?;
        }
        EventPayload::RunBudgetConfigured { .. }
        | EventPayload::ToolCallProposed { .. }
        | EventPayload::ToolPermissionRecorded { .. }
        | EventPayload::ToolPermissionAnswered { .. }
        | EventPayload::ToolCallStarted { .. }
        | EventPayload::ToolCallCompleted { .. }
        | EventPayload::ToolCallFailed { .. }
        | EventPayload::ToolCallDenied { .. }
        | EventPayload::ToolCallCancelled { .. }
        | EventPayload::ToolCallMarkedUnknown { .. }
        | EventPayload::AttemptPausedForTools { .. }
        | EventPayload::AttemptResumedAfterTools { .. } => {
            // Schema-v1 tool state remains authoritative in the event stream.
            // SessionAggregate is the first read projection for this slice.
        }
    }
    Ok(())
}

fn require_attempt_in_flight(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
    attempt_id: &AttemptId,
    mode: ProjectionMode,
) -> Result<(), StoreError> {
    let state = transaction
        .query_row(
            "SELECT state FROM provider_attempts \
             WHERE session_id = ?1 AND attempt_id = ?2",
            params![session_id.as_str(), attempt_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    if state.as_deref() == Some("in_flight") {
        Ok(())
    } else {
        Err(transition_error(mode))
    }
}

fn settle_attempt(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
    attempt_id: &AttemptId,
    state: &'static str,
    failure_json: Option<Vec<u8>>,
    mode: ProjectionMode,
) -> Result<(), StoreError> {
    let allow_prepared = i64::from(state == "failed");
    let changed = transaction
        .execute(
            "UPDATE provider_attempts \
             SET state = ?3, settled_event_id = ?4, settled_at_ms = ?5, failure_json = ?6 \
             WHERE session_id = ?1 AND attempt_id = ?2 \
               AND (state = 'in_flight' OR (?7 = 1 AND state = 'prepared'))",
            params![
                event.session_id().as_str(),
                attempt_id.as_str(),
                state,
                event.event_id().as_str(),
                event.occurred_at().get(),
                failure_json,
                allow_prepared
            ],
        )
        .map_err(map_sqlite_error)?;
    require_transition(changed, mode)?;
    let transcript_state = if state == "completed" {
        "complete"
    } else {
        state
    };
    transaction
        .execute(
            "UPDATE transcript_messages \
             SET state = ?3, last_sequence = ?4 \
             WHERE session_id = ?1 AND source_kind = 'attempt' AND source_id = ?2",
            params![
                event.session_id().as_str(),
                attempt_id.as_str(),
                transcript_state,
                to_sql_sequence(event.sequence().get())?
            ],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn require_transition(changed: usize, mode: ProjectionMode) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(transition_error(mode))
    }
}

fn transition_error(mode: ProjectionMode) -> StoreError {
    match mode {
        ProjectionMode::Append => StoreError::InvalidSessionTransition,
        ProjectionMode::Rebuild => StoreError::CorruptData {
            area: CorruptionArea::Event,
        },
    }
}

fn identity_projection_error(mode: ProjectionMode, kind: IdentityKind) -> StoreError {
    match mode {
        ProjectionMode::Append => StoreError::IdentityConflict { kind },
        ProjectionMode::Rebuild => StoreError::CorruptData {
            area: CorruptionArea::Event,
        },
    }
}

fn validate_complete_streams(
    streams: &BTreeMap<SessionId, Vec<EventEnvelope>>,
) -> Result<(), StoreError> {
    for events in streams.values() {
        let mut expected = 1_u64;
        let mut created = false;
        let mut seen_inputs = BTreeSet::new();
        let mut seen_event_ids = BTreeSet::new();
        let mut seen_command_ids = BTreeSet::new();
        for event in events {
            if event.sequence().get() != expected {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::EventSequence,
                });
            }
            match event.causation() {
                Causation::Command(command_id) if !seen_command_ids.insert(command_id.clone()) => {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::Event,
                    });
                }
                Causation::Event(event_id) if !seen_event_ids.contains(event_id) => {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::Event,
                    });
                }
                Causation::Command(_) | Causation::Event(_) => {}
            }
            match event.payload() {
                EventPayload::SessionCreated if !created => created = true,
                EventPayload::SessionCreated => {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::Event,
                    });
                }
                EventPayload::SessionRenamed { .. }
                | EventPayload::SessionArchived
                | EventPayload::SessionUnarchived
                | EventPayload::ModelSelected { .. }
                | EventPayload::AttemptPrepared { .. }
                | EventPayload::AttemptStarted { .. }
                | EventPayload::AttemptTextAppended { .. }
                | EventPayload::AttemptUsageRecorded { .. }
                | EventPayload::AttemptCancellationRequested { .. }
                | EventPayload::AttemptCompleted { .. }
                | EventPayload::AttemptFailed { .. }
                | EventPayload::AttemptCancelled { .. }
                | EventPayload::AttemptMarkedUnknown { .. }
                | EventPayload::RunBudgetConfigured { .. }
                | EventPayload::ContextTurnBound { .. }
                | EventPayload::RunTurnStarted { .. }
                | EventPayload::ToolCallProposed { .. }
                | EventPayload::ToolPermissionRecorded { .. }
                | EventPayload::ToolPermissionAnswered { .. }
                | EventPayload::ToolCallStarted { .. }
                | EventPayload::ToolCallCompleted { .. }
                | EventPayload::ToolCallFailed { .. }
                | EventPayload::ToolCallDenied { .. }
                | EventPayload::ToolCallCancelled { .. }
                | EventPayload::ToolCallMarkedUnknown { .. }
                | EventPayload::AttemptPausedForTools { .. }
                | EventPayload::AttemptResumedAfterTools { .. }
                    if created => {}
                EventPayload::InputAdmitted { input_id, .. }
                    if created && seen_inputs.insert(input_id.clone()) => {}
                EventPayload::SessionRenamed { .. }
                | EventPayload::SessionArchived
                | EventPayload::SessionUnarchived
                | EventPayload::ModelSelected { .. }
                | EventPayload::InputAdmitted { .. }
                | EventPayload::AttemptPrepared { .. }
                | EventPayload::AttemptStarted { .. }
                | EventPayload::AttemptTextAppended { .. }
                | EventPayload::AttemptUsageRecorded { .. }
                | EventPayload::AttemptCancellationRequested { .. }
                | EventPayload::AttemptCompleted { .. }
                | EventPayload::AttemptFailed { .. }
                | EventPayload::AttemptCancelled { .. }
                | EventPayload::AttemptMarkedUnknown { .. }
                | EventPayload::RunBudgetConfigured { .. }
                | EventPayload::ContextTurnBound { .. }
                | EventPayload::RunTurnStarted { .. }
                | EventPayload::ToolCallProposed { .. }
                | EventPayload::ToolPermissionRecorded { .. }
                | EventPayload::ToolPermissionAnswered { .. }
                | EventPayload::ToolCallStarted { .. }
                | EventPayload::ToolCallCompleted { .. }
                | EventPayload::ToolCallFailed { .. }
                | EventPayload::ToolCallDenied { .. }
                | EventPayload::ToolCallCancelled { .. }
                | EventPayload::ToolCallMarkedUnknown { .. }
                | EventPayload::AttemptPausedForTools { .. }
                | EventPayload::AttemptResumedAfterTools { .. } => {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::Event,
                    });
                }
            }
            seen_event_ids.insert(event.event_id().clone());
            expected = expected
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
        }
        if !created {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::Event,
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
struct StoredEventRow {
    event_id: String,
    session_id: String,
    sequence: i64,
    schema_version: i64,
    occurred_at_ms: i64,
    caused_by_command_id: Option<String>,
    caused_by_event_id: Option<String>,
    correlation_id: String,
    event_kind: String,
    envelope_json: Vec<u8>,
    cause_sequence: Option<i64>,
}

impl StoredEventRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: row.get(0)?,
            session_id: row.get(1)?,
            sequence: row.get(2)?,
            schema_version: row.get(3)?,
            occurred_at_ms: row.get(4)?,
            caused_by_command_id: row.get(5)?,
            caused_by_event_id: row.get(6)?,
            correlation_id: row.get(7)?,
            event_kind: row.get(8)?,
            envelope_json: row.get(9)?,
            cause_sequence: row.get(10)?,
        })
    }

    fn validate(self) -> Result<EventEnvelope, StoreError> {
        let event: EventEnvelope =
            serde_json::from_slice(&self.envelope_json).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::Event,
            })?;
        if event.schema_version() != EVENT_SCHEMA_V1 {
            return Err(StoreError::UnsupportedEventSchema {
                found: event.schema_version(),
            });
        }
        let sequence = u64::try_from(self.sequence).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::EventSequence,
        })?;
        let causation_matches = match event.causation() {
            Causation::Command(command_id) => {
                self.caused_by_command_id.as_deref() == Some(command_id.as_str())
                    && self.caused_by_event_id.is_none()
                    && self.cause_sequence.is_none()
            }
            Causation::Event(event_id) => {
                self.caused_by_command_id.is_none()
                    && self.caused_by_event_id.as_deref() == Some(event_id.as_str())
                    && self
                        .cause_sequence
                        .is_some_and(|cause_sequence| cause_sequence < self.sequence)
            }
        };
        if self.schema_version != i64::from(event.schema_version())
            || self.event_id != event.event_id().as_str()
            || self.session_id != event.session_id().as_str()
            || sequence != event.sequence().get()
            || self.occurred_at_ms != event.occurred_at().get()
            || self.correlation_id != event.correlation_id().as_str()
            || self.event_kind != event_kind(event.payload())
            || !causation_matches
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::Event,
            });
        }
        Ok(event)
    }
}

fn load_all_stored_event_rows(
    transaction: &Transaction<'_>,
) -> Result<Vec<StoredEventRow>, StoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT \
                e.event_id, e.session_id, e.sequence, e.schema_version, e.occurred_at_ms, \
                e.caused_by_command_id, e.caused_by_event_id, e.correlation_id, \
                e.event_kind, e.envelope_json, cause.sequence \
             FROM session_events AS e \
             LEFT JOIN session_events AS cause \
               ON cause.session_id = e.session_id AND cause.event_id = e.caused_by_event_id \
             ORDER BY e.session_id ASC, e.sequence ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], StoredEventRow::from_row)
        .map_err(map_sqlite_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_error)
}

pub(crate) fn load_session_events_through(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
    expected_last_sequence: SessionSequence,
) -> Result<Vec<EventEnvelope>, StoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT \
                e.event_id, e.session_id, e.sequence, e.schema_version, e.occurred_at_ms, \
                e.caused_by_command_id, e.caused_by_event_id, e.correlation_id, \
                e.event_kind, e.envelope_json, cause.sequence \
             FROM session_events AS e \
             LEFT JOIN session_events AS cause \
               ON cause.session_id = e.session_id AND cause.event_id = e.caused_by_event_id \
             WHERE e.session_id = ?1 AND e.sequence <= ?2 \
             ORDER BY e.sequence ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(
            params![
                session_id.as_str(),
                to_sql_sequence(expected_last_sequence.get())?
            ],
            StoredEventRow::from_row,
        )
        .map_err(map_sqlite_error)?;
    let mut events = Vec::new();
    for (index, row) in rows.enumerate() {
        let event = row.map_err(map_sqlite_error)?.validate()?;
        let expected = u64::try_from(index)
            .map_err(|_| StoreError::SequenceOutOfRange)?
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
        if event.sequence().get() != expected {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::EventSequence,
            });
        }
        events.push(event);
    }
    if u64::try_from(events.len()).ok() != Some(expected_last_sequence.get()) {
        return Err(StoreError::CorruptData {
            area: CorruptionArea::EventSequence,
        });
    }
    Ok(events)
}

#[derive(Debug)]
struct SessionProjectionRow {
    session_id: String,
    status: String,
    title: Option<String>,
    selected_provider_id: Option<String>,
    selected_model_id: Option<String>,
    last_sequence: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    minimum_event_sequence: Option<i64>,
    maximum_event_sequence: Option<i64>,
    event_count: i64,
    message_count: i64,
}

impl SessionProjectionRow {
    fn validate(self) -> Result<SessionSummary, StoreError> {
        let corrupt = || StoreError::CorruptData {
            area: CorruptionArea::SessionProjection,
        };
        let session_id = SessionId::new(self.session_id).map_err(|_| corrupt())?;
        let status = match self.status.as_str() {
            "active" => SessionStatus::Active,
            "archived" => SessionStatus::Archived,
            _ => return Err(corrupt()),
        };
        let title = match &self.title {
            None => None,
            Some(raw) if raw.is_empty() => return Err(corrupt()),
            Some(raw) => Some(SessionTitle::new(raw.clone()).map_err(|_| corrupt())?),
        };
        let selected_model = match (self.selected_provider_id, self.selected_model_id) {
            (None, None) => None,
            (Some(provider), Some(model)) => Some(ModelRef::new(
                ProviderId::new(provider).map_err(|_| corrupt())?,
                ModelId::new(model).map_err(|_| corrupt())?,
            )),
            _ => return Err(corrupt()),
        };
        let sequence_value = u64::try_from(self.last_sequence).map_err(|_| corrupt())?;
        let last_sequence = SessionSequence::new(sequence_value).map_err(|_| corrupt())?;
        let message_count = u64::try_from(self.message_count).map_err(|_| corrupt())?;
        if self.minimum_event_sequence != Some(1)
            || self.maximum_event_sequence != Some(self.last_sequence)
            || self.event_count != self.last_sequence
        {
            return Err(corrupt());
        }
        Ok(SessionSummary::new(
            session_id,
            status,
            title,
            selected_model,
            message_count,
            last_sequence,
            TimestampMillis::new(self.created_at_ms),
            TimestampMillis::new(self.updated_at_ms),
        ))
    }
}

#[derive(Debug)]
struct InputProjectionRow {
    input_id: String,
    event_id: String,
    sequence: i64,
    delivery_mode: String,
    state: String,
    prompt_utf8: Vec<u8>,
    content_sha256: Vec<u8>,
    admitted_at_ms: i64,
    promoted_at_ms: Option<i64>,
    event_json: Vec<u8>,
}

impl InputProjectionRow {
    fn validate(self, session_id: SessionId) -> Result<AdmittedInputRecord, StoreError> {
        if Sha256::digest(&self.prompt_utf8).as_slice() != self.content_sha256.as_slice() {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::InputProjection,
            });
        }
        let prompt_string =
            String::from_utf8(self.prompt_utf8).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::InputProjection,
            })?;
        let prompt = PromptText::new(prompt_string).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::InputProjection,
        })?;
        let input_id = InputId::new(self.input_id).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::InputProjection,
        })?;
        let sequence_value = u64::try_from(self.sequence).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::InputProjection,
        })?;
        let sequence =
            SessionSequence::new(sequence_value).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::InputProjection,
            })?;
        let delivery_mode = parse_delivery_mode(&self.delivery_mode)?;
        let state = match (self.state.as_str(), self.promoted_at_ms) {
            ("admitted", None) => InputState::Admitted,
            ("promoted", Some(_)) => InputState::Promoted,
            _ => {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::InputProjection,
                });
            }
        };

        let event: EventEnvelope =
            serde_json::from_slice(&self.event_json).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::InputProjection,
            })?;
        let event_matches = event.event_id().as_str() == self.event_id
            && event.session_id() == &session_id
            && event.sequence() == sequence
            && event.occurred_at().get() == self.admitted_at_ms
            && matches!(
                event.payload(),
                EventPayload::InputAdmitted {
                    input_id: event_input_id,
                    prompt: event_prompt,
                    delivery_mode: event_delivery_mode,
                } if event_input_id == &input_id
                    && event_prompt == &prompt
                    && event_delivery_mode == &delivery_mode
            );
        if !event_matches {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::InputProjection,
            });
        }

        Ok(AdmittedInputRecord::new(
            session_id,
            input_id,
            sequence,
            prompt,
            delivery_mode,
            state,
            TimestampMillis::new(self.admitted_at_ms),
        ))
    }
}

#[derive(Debug)]
struct AttemptProjectionRow {
    attempt_id: String,
    input_id: String,
    provider_id: String,
    model_id: String,
    retry_of_attempt_id: Option<String>,
    state: String,
    prepared_event_id: String,
    prepared_sequence: i64,
    started_event_id: Option<String>,
    settled_event_id: Option<String>,
    cancellation_requested_event_id: Option<String>,
    usage_event_id: Option<String>,
    prepared_at_ms: i64,
    started_at_ms: Option<i64>,
    settled_at_ms: Option<i64>,
    cancellation_requested_at_ms: Option<i64>,
    usage_json: Option<Vec<u8>>,
    failure_json: Option<Vec<u8>>,
    prepared_event_json: Vec<u8>,
    started_event_json: Option<Vec<u8>>,
    settled_event_json: Option<Vec<u8>>,
    cancellation_requested_event_json: Option<Vec<u8>>,
    usage_event_json: Option<Vec<u8>>,
}

impl AttemptProjectionRow {
    fn validate(self, session_id: SessionId) -> Result<AttemptRecord, StoreError> {
        let attempt_id =
            AttemptId::new(self.attempt_id).map_err(|_| attempt_projection_corrupt())?;
        let input_id = InputId::new(self.input_id).map_err(|_| attempt_projection_corrupt())?;
        let model = ModelRef::new(
            ProviderId::new(self.provider_id).map_err(|_| attempt_projection_corrupt())?,
            ModelId::new(self.model_id).map_err(|_| attempt_projection_corrupt())?,
        );
        let retry_of = self
            .retry_of_attempt_id
            .map(AttemptId::new)
            .transpose()
            .map_err(|_| attempt_projection_corrupt())?;
        let prepared_sequence =
            parse_projection_sequence(self.prepared_sequence, CorruptionArea::AttemptProjection)?;
        let state = match self.state.as_str() {
            "prepared" if self.started_at_ms.is_none() && self.settled_at_ms.is_none() => {
                AttemptState::Prepared
            }
            "in_flight" if self.started_at_ms.is_some() && self.settled_at_ms.is_none() => {
                AttemptState::InFlight
            }
            "completed" if self.started_at_ms.is_some() && self.settled_at_ms.is_some() => {
                AttemptState::Completed
            }
            "failed" if self.settled_at_ms.is_some() => AttemptState::Failed,
            "cancelled" if self.started_at_ms.is_some() && self.settled_at_ms.is_some() => {
                AttemptState::Cancelled
            }
            "unknown" if self.started_at_ms.is_some() && self.settled_at_ms.is_some() => {
                AttemptState::Unknown
            }
            _ => return Err(attempt_projection_corrupt()),
        };
        let usage = self
            .usage_json
            .as_deref()
            .map(serde_json::from_slice::<UsageSnapshot>)
            .transpose()
            .map_err(|_| attempt_projection_corrupt())?;
        let failure = self
            .failure_json
            .as_deref()
            .map(serde_json::from_slice::<AttemptFailure>)
            .transpose()
            .map_err(|_| attempt_projection_corrupt())?;
        if (state == AttemptState::Failed) != failure.is_some() {
            return Err(attempt_projection_corrupt());
        }

        let prepared_event: EventEnvelope = serde_json::from_slice(&self.prepared_event_json)
            .map_err(|_| attempt_projection_corrupt())?;
        let prepared_matches = prepared_event.event_id().as_str() == self.prepared_event_id
            && prepared_event.session_id() == &session_id
            && prepared_event.sequence() == prepared_sequence
            && prepared_event.occurred_at().get() == self.prepared_at_ms
            && matches!(
                prepared_event.payload(),
                EventPayload::AttemptPrepared {
                    attempt_id: event_attempt_id,
                    input_id: event_input_id,
                    model: event_model,
                    retry_of: event_retry_of,
                } if event_attempt_id == &attempt_id
                    && event_input_id == &input_id
                    && event_model == &model
                    && event_retry_of == &retry_of
            );
        if !prepared_matches {
            return Err(attempt_projection_corrupt());
        }

        let started_event = decode_optional_projection_event(
            self.started_event_id.as_deref(),
            self.started_event_json.as_deref(),
        )?;
        match (started_event.as_ref(), self.started_at_ms) {
            (None, None) => {}
            (Some(started_event), Some(started_at_ms))
                if started_event.session_id() == &session_id
                    && started_event.occurred_at().get() == started_at_ms
                    && matches!(
                        started_event.payload(),
                        EventPayload::AttemptStarted {
                            attempt_id: event_attempt_id
                        } if event_attempt_id == &attempt_id
                    ) => {}
            _ => return Err(attempt_projection_corrupt()),
        }

        let settled_event = decode_optional_projection_event(
            self.settled_event_id.as_deref(),
            self.settled_event_json.as_deref(),
        )?;
        match (settled_event.as_ref(), self.settled_at_ms, state) {
            (None, None, AttemptState::Prepared | AttemptState::InFlight) => {}
            (Some(settled_event), Some(settled_at_ms), terminal_state)
                if settled_event.session_id() == &session_id
                    && settled_event.occurred_at().get() == settled_at_ms
                    && settled_payload_matches(
                        settled_event.payload(),
                        &attempt_id,
                        terminal_state,
                        failure.as_ref(),
                    ) => {}
            _ => return Err(attempt_projection_corrupt()),
        }

        let cancellation_event = decode_optional_projection_event(
            self.cancellation_requested_event_id.as_deref(),
            self.cancellation_requested_event_json.as_deref(),
        )?;
        match (
            cancellation_event.as_ref(),
            self.cancellation_requested_at_ms,
        ) {
            (None, None) => {}
            (Some(cancellation_event), Some(requested_at_ms))
                if cancellation_event.session_id() == &session_id
                    && cancellation_event.occurred_at().get() == requested_at_ms
                    && matches!(
                        cancellation_event.payload(),
                        EventPayload::AttemptCancellationRequested {
                            attempt_id: event_attempt_id
                        } if event_attempt_id == &attempt_id
                    ) => {}
            _ => return Err(attempt_projection_corrupt()),
        }

        let usage_event = decode_optional_projection_event(
            self.usage_event_id.as_deref(),
            self.usage_event_json.as_deref(),
        )?;
        match (usage_event.as_ref(), usage.as_ref()) {
            (None, None) => {}
            (Some(usage_event), Some(usage))
                if usage_event.session_id() == &session_id
                    && matches!(
                        usage_event.payload(),
                        EventPayload::AttemptUsageRecorded {
                            attempt_id: event_attempt_id,
                            usage: event_usage,
                        } if event_attempt_id == &attempt_id && event_usage == usage
                    ) => {}
            _ => return Err(attempt_projection_corrupt()),
        }

        Ok(AttemptRecord::new(
            session_id,
            attempt_id,
            input_id,
            model,
            retry_of,
            state,
            prepared_sequence,
            TimestampMillis::new(self.prepared_at_ms),
            self.started_at_ms.map(TimestampMillis::new),
            self.settled_at_ms.map(TimestampMillis::new),
            self.cancellation_requested_at_ms.map(TimestampMillis::new),
            usage,
            failure,
        ))
    }
}

fn decode_optional_projection_event(
    event_id: Option<&str>,
    event_json: Option<&[u8]>,
) -> Result<Option<EventEnvelope>, StoreError> {
    match (event_id, event_json) {
        (None, None) => Ok(None),
        (Some(event_id), Some(event_json)) => {
            let event: EventEnvelope =
                serde_json::from_slice(event_json).map_err(|_| attempt_projection_corrupt())?;
            if event.event_id().as_str() == event_id {
                Ok(Some(event))
            } else {
                Err(attempt_projection_corrupt())
            }
        }
        _ => Err(attempt_projection_corrupt()),
    }
}

fn settled_payload_matches(
    payload: &EventPayload,
    attempt_id: &AttemptId,
    state: AttemptState,
    failure: Option<&AttemptFailure>,
) -> bool {
    match (state, payload) {
        (
            AttemptState::Completed,
            EventPayload::AttemptCompleted {
                attempt_id: event_id,
            },
        )
        | (
            AttemptState::Cancelled,
            EventPayload::AttemptCancelled {
                attempt_id: event_id,
            },
        )
        | (
            AttemptState::Unknown,
            EventPayload::AttemptMarkedUnknown {
                attempt_id: event_id,
            },
        ) => event_id == attempt_id,
        (
            AttemptState::Failed,
            EventPayload::AttemptFailed {
                attempt_id: event_id,
                failure: event_failure,
            },
        ) => event_id == attempt_id && Some(event_failure) == failure,
        (AttemptState::Prepared | AttemptState::InFlight, _)
        | (
            AttemptState::Completed
            | AttemptState::Failed
            | AttemptState::Cancelled
            | AttemptState::Unknown,
            _,
        ) => false,
    }
}

fn attempt_projection_corrupt() -> StoreError {
    StoreError::CorruptData {
        area: CorruptionArea::AttemptProjection,
    }
}

#[derive(Debug)]
struct TranscriptMessageRow {
    source_kind: String,
    source_id: String,
    role: String,
    state: String,
    first_sequence: i64,
    last_sequence: i64,
}

impl TranscriptMessageRow {
    fn validate(
        self,
        connection: &Connection,
        session_id: &SessionId,
    ) -> Result<TranscriptEntry, StoreError> {
        let source = match self.source_kind.as_str() {
            "input" => {
                TranscriptSource::Input(InputId::new(self.source_id.clone()).map_err(|_| {
                    StoreError::CorruptData {
                        area: CorruptionArea::TranscriptProjection,
                    }
                })?)
            }
            "attempt" => {
                TranscriptSource::Attempt(AttemptId::new(self.source_id.clone()).map_err(|_| {
                    StoreError::CorruptData {
                        area: CorruptionArea::TranscriptProjection,
                    }
                })?)
            }
            _ => {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::TranscriptProjection,
                });
            }
        };
        let role = match self.role.as_str() {
            "user" => TranscriptRole::User,
            "assistant" => TranscriptRole::Assistant,
            _ => {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::TranscriptProjection,
                });
            }
        };
        let state = match self.state.as_str() {
            "complete" => TranscriptState::Complete,
            "streaming" => TranscriptState::Streaming,
            "failed" => TranscriptState::Failed,
            "cancelled" => TranscriptState::Cancelled,
            "unknown" => TranscriptState::Unknown,
            _ => {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::TranscriptProjection,
                });
            }
        };
        let first_sequence =
            parse_projection_sequence(self.first_sequence, CorruptionArea::TranscriptProjection)?;
        let last_sequence =
            parse_projection_sequence(self.last_sequence, CorruptionArea::TranscriptProjection)?;
        if first_sequence > last_sequence {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            });
        }

        let mut statement = connection
            .prepare(
                "SELECT \
                    event_sequence, source_event_id, content_utf8, content_sha256, \
                    (SELECT envelope_json FROM session_events AS e \
                     WHERE e.event_id = transcript_segments.source_event_id) \
                 FROM transcript_segments \
                 WHERE session_id = ?1 AND source_kind = ?2 AND source_id = ?3 \
                 ORDER BY event_sequence ASC",
            )
            .map_err(map_sqlite_error)?;
        let segments = statement
            .query_map(
                params![session_id.as_str(), &self.source_kind, &self.source_id],
                |row| {
                    Ok(TranscriptSegmentRow {
                        sequence: row.get(0)?,
                        event_id: row.get(1)?,
                        content_utf8: row.get(2)?,
                        content_sha256: row.get(3)?,
                        event_json: row.get(4)?,
                    })
                },
            )
            .map_err(map_sqlite_error)?;

        let mut content = Vec::new();
        let mut previous_sequence = None;
        let mut segment_count = 0_usize;
        for segment in segments {
            let segment = segment.map_err(map_sqlite_error)?;
            let sequence = segment.validate(session_id, &source, first_sequence, last_sequence)?;
            if previous_sequence.is_some_and(|previous| previous >= sequence) {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::TranscriptProjection,
                });
            }
            previous_sequence = Some(sequence);
            content.extend_from_slice(&segment.content_utf8);
            segment_count += 1;
        }
        if segment_count == 0 {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            });
        }
        let content = String::from_utf8(content).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::TranscriptProjection,
        })?;
        let content = TranscriptText::new(content).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::TranscriptProjection,
        })?;

        Ok(TranscriptEntry::new(
            session_id.clone(),
            source,
            role,
            state,
            first_sequence,
            last_sequence,
            content,
        ))
    }
}

#[derive(Debug)]
struct TranscriptSegmentRow {
    sequence: i64,
    event_id: String,
    content_utf8: Vec<u8>,
    content_sha256: Vec<u8>,
    event_json: Vec<u8>,
}

impl TranscriptSegmentRow {
    fn validate(
        &self,
        session_id: &SessionId,
        source: &TranscriptSource,
        first_sequence: SessionSequence,
        last_sequence: SessionSequence,
    ) -> Result<SessionSequence, StoreError> {
        if Sha256::digest(&self.content_utf8).as_slice() != self.content_sha256.as_slice() {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            });
        }
        let sequence =
            parse_projection_sequence(self.sequence, CorruptionArea::TranscriptProjection)?;
        if sequence < first_sequence || sequence > last_sequence {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            });
        }
        let event: EventEnvelope =
            serde_json::from_slice(&self.event_json).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            })?;
        let event_matches = event.event_id().as_str() == self.event_id
            && event.session_id() == session_id
            && event.sequence() == sequence
            && match (source, event.payload()) {
                (
                    TranscriptSource::Input(input_id),
                    EventPayload::InputAdmitted {
                        input_id: event_input_id,
                        prompt,
                        ..
                    },
                ) => {
                    input_id == event_input_id
                        && prompt.as_str().as_bytes() == self.content_utf8.as_slice()
                }
                (
                    TranscriptSource::Attempt(attempt_id),
                    EventPayload::AttemptTextAppended {
                        attempt_id: event_attempt_id,
                        text,
                    },
                ) => {
                    attempt_id == event_attempt_id
                        && text.as_str().as_bytes() == self.content_utf8.as_slice()
                }
                _ => false,
            };
        if !event_matches {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::TranscriptProjection,
            });
        }
        Ok(sequence)
    }
}

fn load_transcript_messages(
    connection: &Connection,
    session_id: &SessionId,
) -> Result<Vec<TranscriptMessageRow>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT source_kind, source_id, role, state, first_sequence, last_sequence \
             FROM transcript_messages \
             WHERE session_id = ?1 \
             ORDER BY first_sequence ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params![session_id.as_str()], |row| {
            Ok(TranscriptMessageRow {
                source_kind: row.get(0)?,
                source_id: row.get(1)?,
                role: row.get(2)?,
                state: row.get(3)?,
                first_sequence: row.get(4)?,
                last_sequence: row.get(5)?,
            })
        })
        .map_err(map_sqlite_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_error)
}

fn parse_projection_sequence(
    value: i64,
    area: CorruptionArea,
) -> Result<SessionSequence, StoreError> {
    let value = u64::try_from(value).map_err(|_| StoreError::CorruptData { area })?;
    SessionSequence::new(value).map_err(|_| StoreError::CorruptData { area })
}

fn event_kind(payload: &EventPayload) -> &'static str {
    match payload {
        EventPayload::SessionCreated => "session_created",
        EventPayload::SessionRenamed { .. } => "session_renamed",
        EventPayload::SessionArchived => "session_archived",
        EventPayload::SessionUnarchived => "session_unarchived",
        EventPayload::ModelSelected { .. } => "model_selected",
        EventPayload::InputAdmitted { .. } => "input_admitted",
        EventPayload::AttemptPrepared { .. } => "attempt_prepared",
        EventPayload::AttemptStarted { .. } => "attempt_started",
        EventPayload::AttemptTextAppended { .. } => "attempt_text_appended",
        EventPayload::AttemptUsageRecorded { .. } => "attempt_usage_recorded",
        EventPayload::AttemptCancellationRequested { .. } => "attempt_cancellation_requested",
        EventPayload::AttemptCompleted { .. } => "attempt_completed",
        EventPayload::AttemptFailed { .. } => "attempt_failed",
        EventPayload::AttemptCancelled { .. } => "attempt_cancelled",
        EventPayload::AttemptMarkedUnknown { .. } => "attempt_marked_unknown",
        EventPayload::RunBudgetConfigured { .. } => "run_budget_configured",
        EventPayload::ContextTurnBound { .. } => "context_turn_bound",
        EventPayload::RunTurnStarted { .. } => "run_turn_started",
        EventPayload::ToolCallProposed { .. } => "tool_call_proposed",
        EventPayload::ToolPermissionRecorded { .. } => "tool_permission_recorded",
        EventPayload::ToolPermissionAnswered { .. } => "tool_permission_answered",
        EventPayload::ToolCallStarted { .. } => "tool_call_started",
        EventPayload::ToolCallCompleted { .. } => "tool_call_completed",
        EventPayload::ToolCallFailed { .. } => "tool_call_failed",
        EventPayload::ToolCallDenied { .. } => "tool_call_denied",
        EventPayload::ToolCallCancelled { .. } => "tool_call_cancelled",
        EventPayload::ToolCallMarkedUnknown { .. } => "tool_call_marked_unknown",
        EventPayload::AttemptPausedForTools { .. } => "attempt_paused_for_tools",
        EventPayload::AttemptResumedAfterTools { .. } => "attempt_resumed_after_tools",
    }
}

fn delivery_mode_name(delivery_mode: DeliveryMode) -> &'static str {
    match delivery_mode {
        DeliveryMode::NextTurn => "next_turn",
    }
}

fn parse_delivery_mode(value: &str) -> Result<DeliveryMode, StoreError> {
    match value {
        "next_turn" => Ok(DeliveryMode::NextTurn),
        _ => Err(StoreError::CorruptData {
            area: CorruptionArea::InputProjection,
        }),
    }
}

pub(crate) fn to_sql_sequence(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::SequenceOutOfRange)
}

fn configure(
    connection: &Connection,
    options: SqliteStoreOptions,
) -> Result<SqliteConfiguration, StoreError> {
    let timeout_ms =
        u64::try_from(options.busy_timeout().as_millis()).map_err(|_| StoreError::Configuration)?;
    if timeout_ms > i64::MAX as u64 {
        return Err(StoreError::Configuration);
    }

    connection
        .busy_timeout(options.busy_timeout())
        .map_err(map_sqlite_error)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite_error)?;
    let journal_mode = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(map_sqlite_error)?
        .to_ascii_lowercase();
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(map_sqlite_error)?;
    connection
        .pragma_update(None, "trusted_schema", "OFF")
        .map_err(map_sqlite_error)?;

    let synchronous_level = connection
        .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
        .map_err(map_sqlite_error)?;
    let foreign_keys = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
        .map_err(map_sqlite_error)?;
    let trusted_schema = connection
        .pragma_query_value(None, "trusted_schema", |row| row.get::<_, i64>(0))
        .map_err(map_sqlite_error)?;
    let fts5 = connection
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    let verified_timeout_ms = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))
        .map_err(map_sqlite_error)?;

    let verified_timeout_ms =
        u64::try_from(verified_timeout_ms).map_err(|_| StoreError::Configuration)?;
    if journal_mode != "wal"
        || synchronous_level != FULL_SYNCHRONOUS_LEVEL
        || foreign_keys != 1
        || trusted_schema != 0
        || fts5 != 1
        || verified_timeout_ms != timeout_ms
    {
        return Err(StoreError::Configuration);
    }

    Ok(SqliteConfiguration {
        journal_mode,
        synchronous_level,
        foreign_keys: true,
        trusted_schema: false,
        fts5: true,
        busy_timeout_ms: verified_timeout_ms,
    })
}

pub(crate) fn map_sqlite_error(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(details, _)
            if matches!(
                details.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            StoreError::Busy
        }
        rusqlite::Error::SqliteFailure(details, _)
            if details.code == ErrorCode::ConstraintViolation =>
        {
            StoreError::IdentityConflict {
                kind: IdentityKind::Constraint,
            }
        }
        _ => StoreError::Backend,
    }
}

#[cfg(test)]
#[path = "sqlite_store_tests.rs"]
mod tests;
