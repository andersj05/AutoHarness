use autoharness_domain::{
    AgentId, Causation, ContextAdmission, ContextAdmissionFactor, ContextEpochId,
    ContextEpochManifest, ContextEpochReason, ContextObservationState, ContextSection,
    ContextTurnId, ContextTurnManifest, EVENT_SCHEMA_V1, EventEnvelope, EventPayload,
    MemoryContent, MemoryId, MemoryKind, MemoryRelationKind, MemoryRevisionStatus, MemoryScope,
    MemoryValidity, Sensitivity, SessionId, SessionSequence, Sha256Digest, UserId, WorkspaceId,
};
use autoharness_memory::{
    COMPACTION_FACTS_VERSION, EffectiveDurableFactsFingerprint, MemoryCandidate, RetrievalScope,
    effective_durable_facts, normalized_content_hash, pending_session_facts_from_events,
    verify_admission_rendered_hash, verify_context_manifest_hash, verify_rendered_context_hash,
};
use autoharness_store::{
    BoundContextTurnCommitReceipt, BoundContextTurnCommitRequest, ContextCommitDisposition,
    ContextCompactionBoundary, ContextStore, ContextTurnCommitRequest, CorruptionArea,
    IdentityKind, RenderedContextText, StoreError,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::memory_store::decode_projected_revision;
use crate::sqlite_store::{
    SqliteStore, load_session_events_through, map_sqlite_error, to_sql_sequence,
};

impl ContextStore for SqliteStore {
    fn commit_context_turn(
        &mut self,
        request: &ContextTurnCommitRequest,
    ) -> Result<ContextCommitDisposition, StoreError> {
        validate_context_sidecars(request)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;
        let disposition = commit_context_in_transaction(&transaction, request)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(disposition)
    }

    fn commit_context_turn_and_bind(
        &mut self,
        request: &BoundContextTurnCommitRequest,
    ) -> Result<BoundContextTurnCommitReceipt, StoreError> {
        validate_context_sidecars(request.context())?;
        validate_binding_request(request)?;
        let event_json =
            serde_json::to_vec(request.binding_event()).map_err(|_| StoreError::Backend)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;

        bind_session_workspace(&transaction, request.context().turn())?;
        reconcile_unbound_turn_conflict(&transaction, request.context().turn())?;
        if staged_turn_requires_boundary_revalidation(
            &transaction,
            request.context().turn().context_turn_id(),
        )? {
            validate_context_boundary(&transaction, request.context())?;
        }
        let context_disposition = commit_context_in_transaction(&transaction, request.context())?;
        persist_compaction_boundary(
            &transaction,
            request.context().turn(),
            request.compaction_boundary(),
        )?;
        let binding_disposition = append_context_binding_event(
            &transaction,
            request.context().turn(),
            request.binding_event(),
            &event_json,
        )?;
        transaction.commit().map_err(map_sqlite_error)?;

        let disposition = if context_disposition == ContextCommitDisposition::AlreadyCommitted
            && binding_disposition == ContextCommitDisposition::AlreadyCommitted
        {
            ContextCommitDisposition::AlreadyCommitted
        } else {
            ContextCommitDisposition::Committed
        };
        Ok(BoundContextTurnCommitReceipt::new(
            disposition,
            request.binding_event().sequence().get(),
        ))
    }

    fn load_context_epoch(
        &self,
        epoch_id: &ContextEpochId,
    ) -> Result<Option<ContextEpochManifest>, StoreError> {
        load_json_record(
            &self.connection,
            "SELECT manifest_json, manifest_json_sha256 FROM context_epochs WHERE epoch_id = ?1",
            epoch_id.as_str(),
        )
    }

    fn load_compaction_boundary(
        &self,
        epoch_id: &ContextEpochId,
    ) -> Result<Option<ContextCompactionBoundary>, StoreError> {
        load_compaction_boundary_record(&self.connection, epoch_id)
    }

    fn load_context_turn(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Option<ContextTurnManifest>, StoreError> {
        load_json_record(
            &self.connection,
            "SELECT manifest_json, manifest_json_sha256 FROM context_turns \
             WHERE context_turn_id = ?1",
            context_turn_id.as_str(),
        )
    }

    fn load_context_admissions(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Vec<ContextAdmission>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT admission_json, admission_json_sha256 FROM context_admissions \
                 WHERE context_turn_id = ?1 ORDER BY rank ASC",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![context_turn_id.as_str()], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(map_sqlite_error)?;
        let mut admissions = Vec::new();
        for (index, row) in rows.enumerate() {
            let (json, hash) = row.map_err(map_sqlite_error)?;
            let admission: ContextAdmission = decode_context_json(&json, &hash)?;
            if admission.context_turn_id() != context_turn_id
                || usize::try_from(admission.rank()).ok() != Some(index + 1)
            {
                return Err(corrupt_context());
            }
            admissions.push(admission);
        }
        Ok(admissions)
    }

    fn load_context_turn_content(
        &self,
        context_turn_id: &ContextTurnId,
    ) -> Result<Option<RenderedContextText>, StoreError> {
        load_context_turn_rendered_content(&self.connection, context_turn_id)
    }

    fn load_context_admission_content(
        &self,
        admission_id: &autoharness_domain::ContextAdmissionId,
    ) -> Result<Option<RenderedContextText>, StoreError> {
        load_context_admission_rendered_content(&self.connection, admission_id)
    }

    fn load_attempt_context_turn(
        &self,
        attempt_id: &autoharness_domain::AttemptId,
        turn: u32,
    ) -> Result<Option<ContextTurnManifest>, StoreError> {
        if turn == 0 {
            return Err(StoreError::InvalidContextTransition);
        }
        let row = self
            .connection
            .query_row(
                "SELECT manifest_json, manifest_json_sha256 FROM context_turns \
                 WHERE attempt_id = ?1 AND run_turn = ?2",
                params![attempt_id.as_str(), i64::from(turn)],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        row.map(|(json, hash)| decode_context_json(&json, &hash))
            .transpose()
    }
}

fn staged_turn_requires_boundary_revalidation(
    transaction: &Transaction<'_>,
    context_turn_id: &ContextTurnId,
) -> Result<bool, StoreError> {
    let state = transaction
        .query_row(
            "SELECT b.context_turn_id IS NOT NULL \
             FROM context_turns AS t \
             LEFT JOIN context_turn_bindings AS b ON b.context_turn_id = t.context_turn_id \
             WHERE t.context_turn_id = ?1",
            params![context_turn_id.as_str()],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    Ok(matches!(state, Some(false)))
}

fn bind_session_workspace(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
) -> Result<(), StoreError> {
    let workspace_id = turn.eligibility().workspace_id();
    let existing = transaction
        .query_row(
            "SELECT workspace_id FROM sessions WHERE session_id = ?1",
            params![turn.session_id().as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .ok_or(StoreError::InvalidContextTransition)?;
    match existing {
        Some(existing) if existing == workspace_id.as_str() => Ok(()),
        Some(_) => Err(StoreError::InvalidContextTransition),
        None => {
            let changed = transaction
                .execute(
                    "UPDATE sessions SET workspace_id = ?2 \
                     WHERE session_id = ?1 AND workspace_id IS NULL",
                    params![turn.session_id().as_str(), workspace_id.as_str()],
                )
                .map_err(map_sqlite_error)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(StoreError::InvalidContextTransition)
            }
        }
    }
}

struct EncodedContext {
    turn_json: Vec<u8>,
    turn_json_hash: Vec<u8>,
    epoch: Option<(Vec<u8>, Vec<u8>)>,
}

fn encode_context(request: &ContextTurnCommitRequest) -> Result<EncodedContext, StoreError> {
    let turn_json = serde_json::to_vec(request.turn()).map_err(|_| StoreError::Backend)?;
    let turn_json_hash = Sha256::digest(&turn_json).to_vec();
    let epoch = request
        .epoch()
        .map(|epoch| {
            serde_json::to_vec(epoch)
                .map(|json| {
                    let hash = Sha256::digest(&json).to_vec();
                    (json, hash)
                })
                .map_err(|_| StoreError::Backend)
        })
        .transpose()?;
    Ok(EncodedContext {
        turn_json,
        turn_json_hash,
        epoch,
    })
}

fn commit_context_in_transaction(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
) -> Result<ContextCommitDisposition, StoreError> {
    let turn = request.turn();
    let encoded = encode_context(request)?;
    if let Some((existing_json, existing_hash)) = transaction
        .query_row(
            "SELECT manifest_json, manifest_json_sha256 FROM context_turns \
             WHERE context_turn_id = ?1",
            params![turn.context_turn_id().as_str()],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(map_sqlite_error)?
    {
        if Sha256::digest(&existing_json).as_slice() != existing_hash.as_slice() {
            return Err(corrupt_context());
        }
        if existing_json != encoded.turn_json {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::ContextTurn,
            });
        }
        if let (Some(epoch), Some((epoch_json, _))) = (request.epoch(), encoded.epoch.as_ref()) {
            let existing_epoch_json = transaction
                .query_row(
                    "SELECT manifest_json FROM context_epochs WHERE epoch_id = ?1",
                    params![epoch.epoch_id().as_str()],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(map_sqlite_error)?;
            if existing_epoch_json.as_deref() != Some(epoch_json.as_slice()) {
                return Err(StoreError::IdentityConflict {
                    kind: IdentityKind::ContextEpoch,
                });
            }
        }
        validate_existing_context_children(transaction, request)?;
        return Ok(ContextCommitDisposition::AlreadyCommitted);
    }

    validate_context_boundary(transaction, request)?;
    if let (Some(epoch), Some((epoch_json, epoch_json_hash))) =
        (request.epoch(), encoded.epoch.as_ref())
    {
        insert_or_reconcile_epoch(transaction, epoch, epoch_json, epoch_json_hash)?;
    }
    validate_durable_epoch(transaction, turn)?;
    insert_context_turn(
        transaction,
        request,
        &encoded.turn_json,
        &encoded.turn_json_hash,
    )?;
    insert_context_children(transaction, request)?;
    Ok(ContextCommitDisposition::Committed)
}

fn persist_compaction_boundary(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
    boundary: Option<&ContextCompactionBoundary>,
) -> Result<(), StoreError> {
    let (epoch_json, epoch_hash) = transaction
        .query_row(
            "SELECT manifest_json, manifest_json_sha256 FROM context_epochs WHERE epoch_id = ?1",
            params![turn.epoch_id().as_str()],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .map_err(map_sqlite_error)?;
    let epoch: ContextEpochManifest = decode_context_json(&epoch_json, &epoch_hash)?;
    if epoch.reason() != ContextEpochReason::Compaction {
        return if boundary.is_none() {
            Ok(())
        } else {
            Err(StoreError::InvalidContextTransition)
        };
    }
    let boundary = boundary.ok_or(StoreError::InvalidContextTransition)?;
    if boundary.epoch_id() != epoch.epoch_id()
        || Some(boundary.predecessor_epoch_id()) != epoch.predecessor_epoch_id()
        || boundary.session_id() != turn.session_id()
        || boundary.expected_session_sequence() != turn.expected_session_sequence()
        || boundary.memory_generation() != turn.memory_generation()
        || boundary.facts_version() != COMPACTION_FACTS_VERSION
    {
        return Err(StoreError::InvalidContextTransition);
    }

    if let Some(existing) = load_compaction_boundary_record(transaction, boundary.epoch_id())? {
        return if &existing == boundary {
            Ok(())
        } else {
            Err(StoreError::IdentityConflict {
                kind: IdentityKind::ContextEpoch,
            })
        };
    }

    if let Some(summary_revision_id) = boundary.summary_revision_id() {
        let row = transaction
            .query_row(
                "SELECT state, metadata_json, metadata_sha256 FROM memory_revisions \
                 WHERE revision_id = ?1",
                params![summary_revision_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((state, json, hash)) = row else {
            return Err(StoreError::InvalidContextTransition);
        };
        let revision = decode_projected_revision(&state, &json, &hash)?;
        if revision.origin() != autoharness_domain::MemoryOrigin::Compaction
            || revision.status() != MemoryRevisionStatus::Proposed
        {
            return Err(StoreError::InvalidContextTransition);
        }
    }

    let fingerprint = compute_compaction_fingerprint(transaction, &epoch, turn)?;
    if fingerprint.hash() != boundary.facts_hash()
        || fingerprint.memory_fact_count() != boundary.memory_fact_count()
        || fingerprint.pending_session_fact_count() != boundary.pending_session_fact_count()
    {
        return Err(StoreError::InvalidContextTransition);
    }
    transaction
        .execute(
            "INSERT INTO context_compaction_boundaries (\
                epoch_id, predecessor_epoch_id, session_id, expected_session_sequence, \
                memory_generation, facts_version, facts_sha256, memory_fact_count, \
                pending_session_fact_count, summary_revision_id, verified_at_ms\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                boundary.epoch_id().as_str(),
                boundary.predecessor_epoch_id().as_str(),
                boundary.session_id().as_str(),
                to_sql_sequence(boundary.expected_session_sequence().get())?,
                to_sql_sequence(boundary.memory_generation().get())?,
                i64::from(boundary.facts_version()),
                decode_digest(boundary.facts_hash().as_str())?.as_slice(),
                i64::from(boundary.memory_fact_count()),
                i64::from(boundary.pending_session_fact_count()),
                boundary.summary_revision_id().map(|id| id.as_str()),
                boundary.verified_at().get(),
            ],
        )
        .map_err(|error| map_context_identity_error(error, IdentityKind::ContextEpoch))?;
    Ok(())
}

fn compute_compaction_fingerprint(
    transaction: &Transaction<'_>,
    epoch: &ContextEpochManifest,
    turn: &ContextTurnManifest,
) -> Result<EffectiveDurableFactsFingerprint, StoreError> {
    let events = load_session_events_through(
        transaction,
        turn.session_id(),
        turn.expected_session_sequence(),
    )?;
    let pending = pending_session_facts_from_events(
        turn.session_id(),
        turn.expected_session_sequence(),
        &events,
    )
    .map_err(|_| corrupt_context())?;
    let candidates = load_compaction_memory_candidates(transaction, turn)?;
    let eligibility = turn.eligibility();
    let scope = RetrievalScope {
        user_id: eligibility.user_id().clone(),
        workspace_id: eligibility.workspace_id().clone(),
        session_id: eligibility.session_id().clone(),
        agent_id: eligibility.agent_id().cloned(),
        as_of: epoch.started_at(),
        sensitivity_ceiling: eligibility.sensitivity_ceiling(),
    };
    effective_durable_facts(&scope, &candidates, &pending).map_err(|_| corrupt_context())
}

fn load_compaction_memory_candidates(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
) -> Result<Vec<MemoryCandidate>, StoreError> {
    let eligibility = turn.eligibility();
    let mut statement = transaction
        .prepare(
            "SELECT i.memory_id, i.scope_type, i.scope_id, i.kind, \
                    r.state, r.metadata_json, r.metadata_sha256, r.content_id, \
                    r.content_hash_sha256, b.content_utf8, b.content_sha256 \
             FROM memory_items AS i \
             JOIN memory_revisions AS r ON r.revision_id = i.active_revision_id \
             LEFT JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE i.lifecycle = 'active' AND r.state = 'active' AND (\
                    (i.scope_type = 'user' AND i.scope_id = ?1) OR \
                    (i.scope_type = 'workspace' AND i.scope_id = ?2) OR \
                    (i.scope_type = 'session' AND i.scope_id = ?3) OR \
                    (?4 IS NOT NULL AND i.scope_type = 'agent' AND i.scope_id = ?4)\
             ) ORDER BY i.memory_id ASC, r.revision_id ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(
            params![
                eligibility.user_id().as_str(),
                eligibility.workspace_id().as_str(),
                eligibility.session_id().as_str(),
                eligibility.agent_id().map(AgentId::as_str),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, Option<Vec<u8>>>(9)?,
                    row.get::<_, Option<Vec<u8>>>(10)?,
                ))
            },
        )
        .map_err(map_sqlite_error)?;

    let mut candidates = Vec::new();
    for row in rows {
        let (
            memory_id,
            scope_type,
            scope_id,
            memory_kind,
            state,
            metadata_json,
            metadata_hash,
            content_id,
            indexed_content_hash,
            content_bytes,
            content_hash,
        ) = row.map_err(map_sqlite_error)?;
        let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
        let (Some(_), Some(content_bytes), Some(content_hash)) =
            (content_id, content_bytes, content_hash)
        else {
            return Err(compaction_memory_corruption());
        };
        if revision.status() != MemoryRevisionStatus::Active
            || Sha256::digest(&content_bytes).as_slice() != content_hash.as_slice()
            || decode_digest(revision.content_hash().as_str())?.as_slice()
                != indexed_content_hash.as_slice()
        {
            return Err(compaction_memory_corruption());
        }
        let content_text =
            String::from_utf8(content_bytes).map_err(|_| compaction_memory_corruption())?;
        if normalized_content_hash(&content_text).map_err(|_| compaction_memory_corruption())?
            != *revision.content_hash()
        {
            return Err(compaction_memory_corruption());
        }
        let conflicted = revision
            .relations()
            .iter()
            .any(|relation| relation.kind() == MemoryRelationKind::Contradicts);
        candidates.push(MemoryCandidate {
            memory_id: MemoryId::new(memory_id).map_err(|_| compaction_memory_corruption())?,
            revision_id: revision.revision_id().clone(),
            status: revision.status(),
            scope: decode_scope(&scope_type, scope_id)
                .map_err(|_| compaction_memory_corruption())?,
            kind: decode_compaction_memory_kind(&memory_kind)?,
            trust: revision.trust_class(),
            confidence: revision.confidence(),
            sensitivity: revision.sensitivity(),
            validity: revision.validity(),
            content: MemoryContent::new(content_text)
                .map_err(|_| compaction_memory_corruption())?,
            content_hash: revision.content_hash().clone(),
            created_at: revision.created_at(),
            exact_match: false,
            lexical_basis_points: 0,
            conflicted,
        });
    }
    Ok(candidates)
}

fn decode_compaction_memory_kind(value: &str) -> Result<MemoryKind, StoreError> {
    match value {
        "fact" => Ok(MemoryKind::Fact),
        "preference" => Ok(MemoryKind::Preference),
        "constraint" => Ok(MemoryKind::Constraint),
        "lesson" => Ok(MemoryKind::Lesson),
        "procedure" => Ok(MemoryKind::Procedure),
        _ => Err(compaction_memory_corruption()),
    }
}

const fn compaction_memory_corruption() -> StoreError {
    StoreError::CorruptData {
        area: CorruptionArea::MemoryProjection,
    }
}

fn load_compaction_boundary_record(
    connection: &rusqlite::Connection,
    epoch_id: &ContextEpochId,
) -> Result<Option<ContextCompactionBoundary>, StoreError> {
    let row = connection
        .query_row(
            "SELECT predecessor_epoch_id, session_id, expected_session_sequence, \
                    memory_generation, facts_version, facts_sha256, memory_fact_count, \
                    pending_session_fact_count, summary_revision_id, verified_at_ms \
             FROM context_compaction_boundaries WHERE epoch_id = ?1",
            params![epoch_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((
        predecessor_epoch_id,
        session_id,
        expected_session_sequence,
        memory_generation,
        facts_version,
        facts_hash,
        memory_fact_count,
        pending_session_fact_count,
        summary_revision_id,
        verified_at,
    )) = row
    else {
        return Ok(None);
    };
    let facts_version = u16::try_from(facts_version).map_err(|_| corrupt_context())?;
    let memory_fact_count = u32::try_from(memory_fact_count).map_err(|_| corrupt_context())?;
    let pending_session_fact_count =
        u32::try_from(pending_session_fact_count).map_err(|_| corrupt_context())?;
    Ok(Some(ContextCompactionBoundary::new(
        epoch_id.clone(),
        ContextEpochId::new(predecessor_epoch_id).map_err(|_| corrupt_context())?,
        SessionId::new(session_id).map_err(|_| corrupt_context())?,
        SessionSequence::new(
            u64::try_from(expected_session_sequence).map_err(|_| corrupt_context())?,
        )
        .map_err(|_| corrupt_context())?,
        autoharness_domain::MemoryGeneration::new(
            u64::try_from(memory_generation).map_err(|_| corrupt_context())?,
        )
        .map_err(|_| corrupt_context())?,
        facts_version,
        digest_from_bytes(&facts_hash)?,
        memory_fact_count,
        pending_session_fact_count,
        summary_revision_id
            .map(autoharness_domain::MemoryRevisionId::new)
            .transpose()
            .map_err(|_| corrupt_context())?,
        autoharness_domain::TimestampMillis::new(verified_at),
    )))
}

fn digest_from_bytes(bytes: &[u8]) -> Result<Sha256Digest, StoreError> {
    if bytes.len() != 32 {
        return Err(corrupt_context());
    }
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Sha256Digest::new(encoded).map_err(|_| corrupt_context())
}

fn validate_binding_request(request: &BoundContextTurnCommitRequest) -> Result<(), StoreError> {
    let turn = request.context().turn();
    let event = request.binding_event();
    let expected_sequence = turn
        .expected_session_sequence()
        .get()
        .checked_add(1)
        .ok_or(StoreError::SequenceOutOfRange)?;
    if event.schema_version() != EVENT_SCHEMA_V1
        || event.session_id() != turn.session_id()
        || event.sequence().get() != expected_sequence
    {
        return Err(StoreError::InvalidContextTransition);
    }
    match event.payload() {
        EventPayload::ContextTurnBound {
            attempt_id,
            run_turn,
            context_turn_id,
            manifest_hash,
        } if attempt_id == turn.attempt_id()
            && *run_turn == turn.run_turn()
            && context_turn_id == turn.context_turn_id()
            && manifest_hash == turn.manifest_hash() =>
        {
            Ok(())
        }
        _ => Err(StoreError::InvalidContextTransition),
    }
}

fn reconcile_unbound_turn_conflict(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
) -> Result<(), StoreError> {
    let conflict = transaction
        .query_row(
            "SELECT t.context_turn_id, t.epoch_id, b.context_turn_id IS NOT NULL \
             FROM context_turns AS t \
             LEFT JOIN context_turn_bindings AS b ON b.context_turn_id = t.context_turn_id \
             WHERE t.attempt_id = ?1 AND t.run_turn = ?2",
            params![turn.attempt_id().as_str(), i64::from(turn.run_turn())],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((context_turn_id, epoch_id, is_bound)) = conflict else {
        return Ok(());
    };
    if context_turn_id == turn.context_turn_id().as_str() {
        return Ok(());
    }
    if is_bound {
        return Err(StoreError::IdentityConflict {
            kind: IdentityKind::ContextTurn,
        });
    }

    transaction
        .execute(
            "DELETE FROM context_admission_reasons WHERE admission_id IN (\
                SELECT admission_id FROM context_admissions WHERE context_turn_id = ?1\
             )",
            params![context_turn_id],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "DELETE FROM context_admissions WHERE context_turn_id = ?1",
            params![context_turn_id],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "DELETE FROM context_turn_sources WHERE context_turn_id = ?1",
            params![context_turn_id],
        )
        .map_err(map_sqlite_error)?;
    let changed = transaction
        .execute(
            "DELETE FROM context_turns WHERE context_turn_id = ?1",
            params![context_turn_id],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(corrupt_context());
    }
    transaction
        .execute(
            "DELETE FROM context_epochs \
             WHERE epoch_id = ?1 \
               AND NOT EXISTS (SELECT 1 FROM context_turns WHERE epoch_id = ?1) \
               AND NOT EXISTS (\
                    SELECT 1 FROM context_epochs WHERE predecessor_epoch_id = ?1\
               )",
            params![epoch_id],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn append_context_binding_event(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
    event: &EventEnvelope,
    event_json: &[u8],
) -> Result<ContextCommitDisposition, StoreError> {
    if let Some(existing_json) = transaction
        .query_row(
            "SELECT envelope_json FROM session_events WHERE event_id = ?1",
            params![event.event_id().as_str()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
    {
        if existing_json.as_slice() != event_json {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::Event,
            });
        }
        let existing_binding = validate_existing_binding(transaction, turn, event)?;
        if !existing_binding {
            apply_context_turn_binding(transaction, event)?;
        }
        let actual = current_context_session_version(transaction, turn.session_id())?;
        if actual < event.sequence().get() {
            return Err(corrupt_context());
        }
        return Ok(if existing_binding {
            ContextCommitDisposition::AlreadyCommitted
        } else {
            ContextCommitDisposition::Committed
        });
    }

    let actual = current_context_session_version(transaction, turn.session_id())?;
    if actual != turn.expected_session_sequence().get() {
        return Err(StoreError::VersionConflict {
            session_id: turn.session_id().clone(),
            expected: turn.expected_session_sequence().get(),
            actual,
        });
    }
    validate_binding_boundary(transaction, turn, event)?;
    insert_binding_event(transaction, event, event_json)?;
    apply_context_turn_binding(transaction, event)?;
    let changed = transaction
        .execute(
            "UPDATE sessions SET last_sequence = ?2, updated_at_ms = ?3 \
             WHERE session_id = ?1 AND last_sequence = ?4",
            params![
                event.session_id().as_str(),
                to_sql_sequence(event.sequence().get())?,
                event.occurred_at().get(),
                to_sql_sequence(turn.expected_session_sequence().get())?
            ],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(StoreError::InvalidContextTransition);
    }
    Ok(ContextCommitDisposition::Committed)
}

fn current_context_session_version(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
) -> Result<u64, StoreError> {
    let value = transaction
        .query_row(
            "SELECT last_sequence FROM sessions WHERE session_id = ?1",
            params![session_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .ok_or(StoreError::InvalidContextTransition)?;
    u64::try_from(value).map_err(|_| corrupt_context())
}

fn insert_binding_event(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
    event_json: &[u8],
) -> Result<(), StoreError> {
    let (caused_by_command_id, caused_by_event_id) = match event.causation() {
        Causation::Command(command_id) => (Some(command_id.as_str()), None),
        Causation::Event(event_id) => (None, Some(event_id.as_str())),
    };
    transaction
        .execute(
            "INSERT INTO session_events (\
                event_id, session_id, sequence, schema_version, occurred_at_ms, \
                caused_by_command_id, caused_by_event_id, correlation_id, event_kind, envelope_json\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'context_turn_bound', ?9)",
            params![
                event.event_id().as_str(),
                event.session_id().as_str(),
                to_sql_sequence(event.sequence().get())?,
                i64::from(event.schema_version()),
                event.occurred_at().get(),
                caused_by_command_id,
                caused_by_event_id,
                event.correlation_id().as_str(),
                event_json
            ],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn validate_existing_binding(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
    event: &EventEnvelope,
) -> Result<bool, StoreError> {
    let row = transaction
        .query_row(
            "SELECT session_id, attempt_id, run_turn, bound_event_id, manifest_sha256 \
             FROM context_turn_bindings WHERE context_turn_id = ?1",
            params![turn.context_turn_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((session_id, attempt_id, run_turn, bound_event_id, manifest_hash)) = row else {
        return Ok(false);
    };
    if session_id != turn.session_id().as_str()
        || attempt_id != turn.attempt_id().as_str()
        || u32::try_from(run_turn).ok() != Some(turn.run_turn())
        || bound_event_id != event.event_id().as_str()
        || manifest_hash.as_slice() != decode_digest(turn.manifest_hash().as_str())?.as_slice()
    {
        return Err(corrupt_context());
    }
    Ok(true)
}

struct AttemptBindingState {
    turns_started: u32,
    dispatch_ready: bool,
    pending_binding: bool,
    max_turns: Option<u32>,
}

fn load_attempt_binding_state(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
    attempt_id: &autoharness_domain::AttemptId,
    before_sequence: u64,
) -> Result<AttemptBindingState, StoreError> {
    let state = transaction
        .query_row(
            "SELECT state FROM provider_attempts WHERE session_id = ?1 AND attempt_id = ?2",
            params![session_id.as_str(), attempt_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    if state.as_deref() != Some("in_flight") {
        return Err(StoreError::InvalidContextTransition);
    }

    let mut statement = transaction
        .prepare(
            "SELECT envelope_json FROM session_events \
             WHERE session_id = ?1 AND sequence < ?2 ORDER BY sequence ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(
            params![session_id.as_str(), to_sql_sequence(before_sequence)?],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(map_sqlite_error)?;
    let mut turns_started = 0_u32;
    let mut dispatch_ready = false;
    let mut pending_binding = false;
    let mut max_turns = None;
    for row in rows {
        let json = row.map_err(map_sqlite_error)?;
        let event: EventEnvelope = serde_json::from_slice(&json).map_err(|_| corrupt_context())?;
        match event.payload() {
            EventPayload::RunBudgetConfigured {
                attempt_id: found,
                limits,
            } if found == attempt_id => max_turns = Some(limits.max_turns),
            EventPayload::AttemptStarted { attempt_id: found } if found == attempt_id => {
                dispatch_ready = true;
            }
            EventPayload::ContextTurnBound {
                attempt_id: found, ..
            } if found == attempt_id => pending_binding = true,
            EventPayload::RunTurnStarted {
                attempt_id: found,
                turn,
            } if found == attempt_id => {
                turns_started = *turn;
                dispatch_ready = false;
                pending_binding = false;
            }
            EventPayload::AttemptPausedForTools { attempt_id: found } if found == attempt_id => {
                dispatch_ready = false;
            }
            EventPayload::AttemptResumedAfterTools { attempt_id: found } if found == attempt_id => {
                dispatch_ready = true;
            }
            _ => {}
        }
    }
    Ok(AttemptBindingState {
        turns_started,
        dispatch_ready,
        pending_binding,
        max_turns,
    })
}

fn validate_binding_boundary(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
    event: &EventEnvelope,
) -> Result<(), StoreError> {
    let state = load_attempt_binding_state(
        transaction,
        turn.session_id(),
        turn.attempt_id(),
        event.sequence().get(),
    )?;
    let expected_turn = state
        .turns_started
        .checked_add(1)
        .ok_or(StoreError::InvalidContextTransition)?;
    if !state.dispatch_ready
        || state.pending_binding
        || expected_turn != turn.run_turn()
        || state.max_turns.is_some_and(|limit| expected_turn > limit)
    {
        return Err(StoreError::InvalidContextTransition);
    }
    match event.causation() {
        Causation::Command(command_id) => {
            let duplicate = transaction
                .query_row(
                    "SELECT 1 FROM session_events \
                     WHERE caused_by_command_id = ?1 AND event_id <> ?2",
                    params![command_id.as_str(), event.event_id().as_str()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(map_sqlite_error)?
                .is_some();
            if duplicate {
                return Err(StoreError::IdentityConflict {
                    kind: IdentityKind::Command,
                });
            }
        }
        Causation::Event(cause_id) => {
            let sequence = transaction
                .query_row(
                    "SELECT sequence FROM session_events \
                     WHERE session_id = ?1 AND event_id = ?2",
                    params![event.session_id().as_str(), cause_id.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(map_sqlite_error)?;
            if sequence.is_none_or(|sequence| {
                u64::try_from(sequence).map_or(true, |sequence| sequence >= event.sequence().get())
            }) {
                return Err(StoreError::InvalidCausation);
            }
        }
    }
    Ok(())
}

pub(crate) fn apply_context_turn_binding(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
) -> Result<(), StoreError> {
    let EventPayload::ContextTurnBound {
        attempt_id,
        run_turn,
        context_turn_id,
        manifest_hash,
    } = event.payload()
    else {
        return Err(StoreError::InvalidContextTransition);
    };
    let row = transaction
        .query_row(
            "SELECT manifest_json, manifest_json_sha256 FROM context_turns \
             WHERE context_turn_id = ?1 AND session_id = ?2 AND attempt_id = ?3 AND run_turn = ?4",
            params![
                context_turn_id.as_str(),
                event.session_id().as_str(),
                attempt_id.as_str(),
                i64::from(*run_turn)
            ],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((json, hash)) = row else {
        return Err(StoreError::InvalidContextTransition);
    };
    let turn: ContextTurnManifest = decode_context_json(&json, &hash)?;
    if turn.manifest_hash() != manifest_hash
        || !verify_context_manifest_hash(&turn).map_err(|_| corrupt_context())?
    {
        return Err(StoreError::InvalidContextTransition);
    }
    validate_binding_boundary(transaction, &turn, event)?;
    transaction
        .execute(
            "INSERT INTO context_turn_bindings (\
                context_turn_id, session_id, attempt_id, run_turn, bound_event_id, \
                manifest_sha256, bound_at_ms\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                context_turn_id.as_str(),
                event.session_id().as_str(),
                attempt_id.as_str(),
                i64::from(*run_turn),
                event.event_id().as_str(),
                decode_digest(manifest_hash.as_str())?.as_slice(),
                event.occurred_at().get()
            ],
        )
        .map_err(|error| map_context_identity_error(error, IdentityKind::ContextTurn))?;
    Ok(())
}

pub(crate) fn validate_run_turn_binding(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
) -> Result<(), StoreError> {
    let EventPayload::RunTurnStarted { attempt_id, turn } = event.payload() else {
        return Err(StoreError::InvalidSessionTransition);
    };
    let prior_sequence = event
        .sequence()
        .get()
        .checked_sub(1)
        .ok_or(StoreError::InvalidSessionTransition)?;
    let valid = transaction
        .query_row(
            "SELECT 1 FROM context_turn_bindings AS b \
             JOIN session_events AS e ON e.session_id = b.session_id \
                                      AND e.event_id = b.bound_event_id \
             WHERE b.session_id = ?1 AND b.attempt_id = ?2 AND b.run_turn = ?3 \
               AND e.sequence = ?4",
            params![
                event.session_id().as_str(),
                attempt_id.as_str(),
                i64::from(*turn),
                to_sql_sequence(prior_sequence)?
            ],
            |_| Ok(()),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .is_some();
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidSessionTransition)
    }
}

fn validate_existing_context_children(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
) -> Result<(), StoreError> {
    let turn = request.turn();
    let source_count = transaction
        .query_row(
            "SELECT COUNT(*) FROM context_turn_sources WHERE context_turn_id = ?1",
            params![turn.context_turn_id().as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    let admission_count = transaction
        .query_row(
            "SELECT COUNT(*) FROM context_admissions WHERE context_turn_id = ?1",
            params![turn.context_turn_id().as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    if usize::try_from(source_count).ok() != Some(turn.sources().len())
        || usize::try_from(admission_count).ok() != Some(turn.admissions().len())
    {
        return Err(corrupt_context());
    }
    let mut statement = transaction
        .prepare(
            "SELECT admission_json, admission_json_sha256 FROM context_admissions \
             WHERE context_turn_id = ?1 ORDER BY rank ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params![turn.context_turn_id().as_str()], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(map_sqlite_error)?;
    for (expected, row) in turn.admissions().iter().zip(rows) {
        let (json, hash) = row.map_err(map_sqlite_error)?;
        let actual: ContextAdmission = decode_context_json(&json, &hash)?;
        if &actual != expected {
            return Err(corrupt_context());
        }
    }
    let persisted_prelude = transaction
        .query_row(
            "SELECT rendered_state, rendered_utf8, rendered_content_sha256 FROM context_turns \
             WHERE context_turn_id = ?1",
            params![turn.context_turn_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                ))
            },
        )
        .map_err(map_sqlite_error)?;
    validate_rendered_retry(persisted_prelude, request.content().prelude())?;
    let mut statement = transaction
        .prepare(
            "SELECT rendered_state, rendered_utf8, rendered_content_sha256 FROM context_admissions \
             WHERE context_turn_id = ?1 ORDER BY rank ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params![turn.context_turn_id().as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    for (expected, row) in request.content().admissions().iter().zip(rows) {
        validate_rendered_retry(row.map_err(map_sqlite_error)?, Some(expected.rendered()))?;
    }
    Ok(())
}

fn validate_context_sidecars(request: &ContextTurnCommitRequest) -> Result<(), StoreError> {
    let turn = request.turn();
    if turn.admissions().is_empty() != request.content().prelude().is_none()
        || turn.admissions().len() != request.content().admissions().len()
        || turn
            .admissions()
            .iter()
            .zip(request.content().admissions())
            .any(|(metadata, content)| metadata.admission_id() != content.admission_id())
    {
        return Err(StoreError::InvalidContextTransition);
    }
    if !verify_context_manifest_hash(turn).map_err(|_| StoreError::InvalidContextTransition)? {
        return Err(StoreError::InvalidContextTransition);
    }
    let prelude = request
        .content()
        .prelude()
        .map_or("", RenderedContextText::as_str);
    if !verify_rendered_context_hash(prelude, turn.rendered_hash())
        .map_err(|_| StoreError::InvalidContextTransition)?
    {
        return Err(StoreError::InvalidContextTransition);
    }
    for (admission, content) in turn.admissions().iter().zip(request.content().admissions()) {
        if admission.memory_revision_id().is_none()
            && !verify_admission_rendered_hash(admission, None, content.rendered().as_str())
                .map_err(|_| StoreError::InvalidContextTransition)?
        {
            return Err(StoreError::InvalidContextTransition);
        }
    }
    Ok(())
}

fn validate_rendered_retry(
    persisted: (String, Option<Vec<u8>>, Option<Vec<u8>>),
    expected: Option<&RenderedContextText>,
) -> Result<(), StoreError> {
    match (persisted, expected) {
        ((state, None, None), None) if state == "absent" => Ok(()),
        ((state, None, None), Some(_)) if state == "erased" => Ok(()),
        ((state, Some(bytes), Some(hash)), Some(expected))
            if state == "retained"
                && Sha256::digest(&bytes).as_slice() == hash.as_slice()
                && bytes.as_slice() == expected.as_str().as_bytes() =>
        {
            Ok(())
        }
        ((state, Some(_), Some(_)), Some(_)) if state == "retained" => {
            Err(StoreError::IdentityConflict {
                kind: IdentityKind::ContextTurn,
            })
        }
        _ => Err(corrupt_context()),
    }
}

fn validate_context_boundary(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
) -> Result<(), StoreError> {
    let turn = request.turn();
    let frozen_baseline = load_frozen_epoch_baseline(transaction, turn)?;
    let budget = turn.budget();
    let durable_tokens = turn
        .admissions()
        .iter()
        .filter(|admission| admission.section() == ContextSection::DurableMemory)
        .try_fold(0_u64, |total, admission| {
            total.checked_add(admission.token_count().get())
        })
        .ok_or(StoreError::InvalidContextTransition)?;
    if turn.rendered_token_count().get() > budget.rendered_limit()
        || durable_tokens > budget.durable_memory_limit().get()
        || turn.eligibility().session_id() != turn.session_id()
    {
        return Err(StoreError::InvalidContextTransition);
    }
    if request
        .epoch()
        .is_some_and(|epoch| epoch.epoch_id() != turn.epoch_id())
        || request
            .epoch()
            .is_some_and(|epoch| epoch.session_id() != turn.session_id())
        || request
            .epoch()
            .is_some_and(|epoch| epoch.memory_generation() != turn.memory_generation())
    {
        return Err(StoreError::InvalidContextTransition);
    }

    if frozen_baseline.is_none() {
        let generation = transaction
            .query_row(
                "SELECT generation FROM memory_store_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sqlite_error)?;
        let generation = u64::try_from(generation).map_err(|_| corrupt_context())?;
        if generation != turn.memory_generation().get() {
            return Err(StoreError::ContextGenerationConflict {
                expected: turn.memory_generation().get(),
                actual: generation,
            });
        }
    }

    let session = transaction
        .query_row(
            "SELECT s.last_sequence, a.provider_id, a.model_id \
             FROM sessions AS s JOIN provider_attempts AS a ON a.session_id = s.session_id \
             WHERE s.session_id = ?1 AND a.attempt_id = ?2",
            params![turn.session_id().as_str(), turn.attempt_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((last_sequence, provider_id, model_id)) = session else {
        return Err(StoreError::InvalidContextTransition);
    };
    let last_sequence = u64::try_from(last_sequence).map_err(|_| corrupt_context())?;
    if last_sequence != turn.expected_session_sequence().get() {
        return Err(StoreError::VersionConflict {
            session_id: turn.session_id().clone(),
            expected: turn.expected_session_sequence().get(),
            actual: last_sequence,
        });
    }
    if provider_id != turn.model().provider_id().as_str()
        || model_id != turn.model().model_id().as_str()
    {
        return Err(StoreError::InvalidContextTransition);
    }

    let eligibility_at_ms = match request.epoch() {
        Some(epoch) => epoch.started_at().get(),
        None => transaction
            .query_row(
                "SELECT started_at_ms FROM context_epochs WHERE epoch_id = ?1",
                params![turn.epoch_id().as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .ok_or(StoreError::InvalidContextTransition)?,
    };

    if let Some(frozen_baseline) = frozen_baseline {
        return validate_frozen_epoch_continuation(transaction, request, &frozen_baseline);
    }

    for admission in turn.admissions() {
        let Some(revision_id) = admission.memory_revision_id() else {
            continue;
        };
        let row = transaction
            .query_row(
                "SELECT r.state, r.metadata_json, r.metadata_sha256, r.content_id, \
                        i.scope_type, i.scope_id, r.memory_id \
                 FROM memory_revisions AS r \
                 JOIN memory_items AS i ON i.memory_id = r.memory_id \
                 WHERE r.revision_id = ?1",
                params![revision_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((state, json, hash, content_id, scope_type, scope_id, memory_id)) = row else {
            return Err(StoreError::InvalidContextTransition);
        };
        let metadata = decode_projected_revision(&state, &json, &hash)?;
        let scope = decode_scope(&scope_type, scope_id)?;
        let memory_id = MemoryId::new(memory_id).map_err(|_| corrupt_context())?;
        let rendered = request
            .content()
            .admissions()
            .iter()
            .find(|content| content.admission_id() == admission.admission_id())
            .ok_or(StoreError::InvalidContextTransition)?;
        if state != "active"
            || content_id.is_none()
            || metadata.status() != MemoryRevisionStatus::Active
            || metadata.content_hash() != admission.source_revision()
            || metadata.created_at().get() > eligibility_at_ms
            || !valid_at(metadata.validity(), eligibility_at_ms)
            || !turn.eligibility().permits_scope(&scope)
            || !turn
                .eligibility()
                .permits_sensitivity(metadata.sensitivity())
            || !verify_admission_rendered_hash(
                admission,
                Some(&memory_id),
                rendered.rendered().as_str(),
            )
            .map_err(|_| StoreError::InvalidContextTransition)?
        {
            return Err(StoreError::InvalidContextTransition);
        }
    }
    Ok(())
}

struct FrozenEpochBaseline {
    turn: ContextTurnManifest,
    prelude: Option<RenderedContextText>,
    admissions: Vec<(ContextAdmission, RenderedContextText)>,
}

fn load_frozen_epoch_baseline(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
) -> Result<Option<FrozenEpochBaseline>, StoreError> {
    let row = transaction
        .query_row(
            "SELECT t.manifest_json, t.manifest_json_sha256 \
             FROM context_turns AS t \
             JOIN context_turn_bindings AS b ON b.context_turn_id = t.context_turn_id \
             WHERE t.epoch_id = ?1 AND t.session_id = ?2 AND t.attempt_id = ?3 \
               AND t.run_turn < ?4 \
             ORDER BY t.run_turn ASC LIMIT 1",
            params![
                turn.epoch_id().as_str(),
                turn.session_id().as_str(),
                turn.attempt_id().as_str(),
                i64::from(turn.run_turn())
            ],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((turn_json, turn_hash)) = row else {
        return Ok(None);
    };
    let baseline_turn: ContextTurnManifest = decode_context_json(&turn_json, &turn_hash)?;
    if !verify_context_manifest_hash(&baseline_turn).map_err(|_| corrupt_context())? {
        return Err(corrupt_context());
    }
    let prelude = load_context_turn_rendered_content(transaction, baseline_turn.context_turn_id())?;
    let mut statement = transaction
        .prepare(
            "SELECT admission_json, admission_json_sha256, rendered_state, rendered_utf8, \
                    rendered_content_sha256 \
             FROM context_admissions WHERE context_turn_id = ?1 ORDER BY rank ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params![baseline_turn.context_turn_id().as_str()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<Vec<u8>>>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    let mut admissions = Vec::new();
    for row in rows {
        let (json, hash, state, rendered, rendered_hash) = row.map_err(map_sqlite_error)?;
        let admission: ContextAdmission = decode_context_json(&json, &hash)?;
        let rendered = decode_rendered_content(&state, rendered, rendered_hash)?
            .ok_or(StoreError::InvalidContextTransition)?;
        admissions.push((admission, rendered));
    }
    if admissions.len() != baseline_turn.admissions().len()
        || (admissions.is_empty() != prelude.is_none())
    {
        return Err(corrupt_context());
    }
    Ok(Some(FrozenEpochBaseline {
        turn: baseline_turn,
        prelude,
        admissions,
    }))
}

fn validate_frozen_epoch_continuation(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
    baseline: &FrozenEpochBaseline,
) -> Result<(), StoreError> {
    let turn = request.turn();
    if turn.epoch_id() != baseline.turn.epoch_id()
        || turn.session_id() != baseline.turn.session_id()
        || turn.attempt_id() != baseline.turn.attempt_id()
        || turn.memory_generation() != baseline.turn.memory_generation()
        || turn.model() != baseline.turn.model()
        || turn.eligibility() != baseline.turn.eligibility()
        || turn.budget().token_budget() != baseline.turn.budget().token_budget()
        || turn.budget().durable_memory_limit() != baseline.turn.budget().durable_memory_limit()
        || baseline.turn.rendered_token_count().get() > turn.budget().rendered_limit()
        || turn.sources() != baseline.turn.sources()
        || turn.rendered_hash() != baseline.turn.rendered_hash()
        || turn.rendered_token_count() != baseline.turn.rendered_token_count()
        || request.content().prelude() != baseline.prelude.as_ref()
        || turn.admissions().len() != baseline.admissions.len()
    {
        return Err(StoreError::InvalidContextTransition);
    }

    for ((admission, content), (baseline_admission, baseline_content)) in turn
        .admissions()
        .iter()
        .zip(request.content().admissions())
        .zip(&baseline.admissions)
    {
        if admission.section() != baseline_admission.section()
            || admission.source_key() != baseline_admission.source_key()
            || admission.source_revision() != baseline_admission.source_revision()
            || admission.memory_revision_id() != baseline_admission.memory_revision_id()
            || admission.renderer_version() != baseline_admission.renderer_version()
            || admission.rendered_hash() != baseline_admission.rendered_hash()
            || admission.rank() != baseline_admission.rank()
            || admission.rank_score() != baseline_admission.rank_score()
            || admission.token_count() != baseline_admission.token_count()
            || admission.reasons() != baseline_admission.reasons()
            || content.rendered() != baseline_content
        {
            return Err(StoreError::InvalidContextTransition);
        }
        let memory_id = admission
            .memory_revision_id()
            .map(|revision_id| {
                transaction
                    .query_row(
                        "SELECT memory_id FROM memory_revisions WHERE revision_id = ?1",
                        params![revision_id.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(map_sqlite_error)?
                    .map(MemoryId::new)
                    .transpose()
                    .map_err(|_| corrupt_context())?
                    .ok_or(StoreError::InvalidContextTransition)
            })
            .transpose()?;
        if !verify_admission_rendered_hash(
            admission,
            memory_id.as_ref(),
            content.rendered().as_str(),
        )
        .map_err(|_| StoreError::InvalidContextTransition)?
        {
            return Err(StoreError::InvalidContextTransition);
        }
    }
    Ok(())
}

fn valid_at(validity: MemoryValidity, at_ms: i64) -> bool {
    match validity {
        MemoryValidity::Indefinite => true,
        MemoryValidity::From { valid_from } => valid_from.get() <= at_ms,
        MemoryValidity::Until { valid_until } => at_ms < valid_until.get(),
        MemoryValidity::Window(window) => {
            window.valid_from().get() <= at_ms && at_ms < window.valid_until().get()
        }
    }
}

fn decode_scope(scope_type: &str, scope_id: String) -> Result<MemoryScope, StoreError> {
    match scope_type {
        "user" => UserId::new(scope_id).map(MemoryScope::User),
        "workspace" => WorkspaceId::new(scope_id).map(MemoryScope::Workspace),
        "session" => SessionId::new(scope_id).map(MemoryScope::Session),
        "agent" => AgentId::new(scope_id).map(MemoryScope::Agent),
        _ => return Err(corrupt_context()),
    }
    .map_err(|_| corrupt_context())
}

fn insert_or_reconcile_epoch(
    transaction: &Transaction<'_>,
    epoch: &ContextEpochManifest,
    json: &[u8],
    json_hash: &[u8],
) -> Result<(), StoreError> {
    if let Some(existing_json) = transaction
        .query_row(
            "SELECT manifest_json FROM context_epochs WHERE epoch_id = ?1",
            params![epoch.epoch_id().as_str()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
    {
        return if existing_json == json {
            Ok(())
        } else {
            Err(StoreError::IdentityConflict {
                kind: IdentityKind::ContextEpoch,
            })
        };
    }
    if let Some(predecessor_id) = epoch.predecessor_epoch_id() {
        let predecessor_session = transaction
            .query_row(
                "SELECT session_id FROM context_epochs WHERE epoch_id = ?1",
                params![predecessor_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        if predecessor_session.as_deref() != Some(epoch.session_id().as_str()) {
            return Err(StoreError::InvalidContextTransition);
        }
    }
    let versions = epoch.versions();
    let hashes = epoch.hashes();
    transaction
        .execute(
            "INSERT INTO context_epochs (epoch_id, session_id, memory_generation, reason, \
             predecessor_epoch_id, baseline_sha256, builder_version, registry_version, \
             ranker_version, renderer_version, sizer_version, config_sha256, catalog_sha256, \
             model_capability_sha256, tool_registry_sha256, token_budget, started_at_ms, \
             manifest_json, manifest_json_sha256) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                     ?16, ?17, ?18, ?19)",
            params![
                epoch.epoch_id().as_str(),
                epoch.session_id().as_str(),
                to_sql_sequence(epoch.memory_generation().get())?,
                encode_epoch_reason(epoch.reason()),
                epoch.predecessor_epoch_id().map(|id| id.as_str()),
                decode_digest(epoch.baseline_hash().as_str())?.as_slice(),
                i64::from(versions.builder_version()),
                i64::from(versions.registry_version()),
                i64::from(versions.ranker_version()),
                i64::from(versions.renderer_version()),
                i64::from(versions.sizer_version()),
                decode_digest(hashes.config_hash().as_str())?.as_slice(),
                decode_digest(hashes.catalog_hash().as_str())?.as_slice(),
                decode_digest(hashes.model_capability_hash().as_str())?.as_slice(),
                decode_digest(hashes.tool_registry_hash().as_str())?.as_slice(),
                to_sql_sequence(epoch.token_budget().get())?,
                epoch.started_at().get(),
                json,
                json_hash,
            ],
        )
        .map_err(|error| map_context_identity_error(error, IdentityKind::ContextEpoch))?;
    Ok(())
}

fn validate_durable_epoch(
    transaction: &Transaction<'_>,
    turn: &ContextTurnManifest,
) -> Result<(), StoreError> {
    let epoch = transaction
        .query_row(
            "SELECT session_id, memory_generation, token_budget FROM context_epochs \
             WHERE epoch_id = ?1",
            params![turn.epoch_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((session_id, memory_generation, token_budget)) = epoch else {
        return Err(StoreError::InvalidContextTransition);
    };
    if session_id != turn.session_id().as_str()
        || u64::try_from(memory_generation).ok() != Some(turn.memory_generation().get())
        || u64::try_from(token_budget).ok() != Some(turn.token_budget().get())
    {
        return Err(StoreError::InvalidContextTransition);
    }
    Ok(())
}

fn insert_context_turn(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
    json: &[u8],
    json_hash: &[u8],
) -> Result<(), StoreError> {
    let turn = request.turn();
    let eligibility = turn.eligibility();
    let budget = turn.budget();
    let (rendered_state, rendered_utf8, rendered_content_hash) = match request.content().prelude() {
        Some(prelude) => (
            "retained",
            Some(prelude.as_str().as_bytes()),
            Some(Sha256::digest(prelude.as_str().as_bytes())),
        ),
        None => ("absent", None, None),
    };
    transaction
        .execute(
            "INSERT INTO context_turns (context_turn_id, session_id, attempt_id, run_turn, \
             epoch_id, expected_session_sequence, memory_generation, provider_id, model_id, \
             request_sha256, rendered_sha256, manifest_sha256, eligibility_user_id, \
             eligibility_workspace_id, eligibility_agent_id, sensitivity_ceiling, token_budget, \
             reserved_tokens, durable_memory_limit, rendered_token_count, committed_at_ms, \
             rendered_state, rendered_utf8, \
             rendered_content_sha256, manifest_json, manifest_json_sha256) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                     ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
            params![
                turn.context_turn_id().as_str(),
                turn.session_id().as_str(),
                turn.attempt_id().as_str(),
                i64::from(turn.run_turn()),
                turn.epoch_id().as_str(),
                to_sql_sequence(turn.expected_session_sequence().get())?,
                to_sql_sequence(turn.memory_generation().get())?,
                turn.model().provider_id().as_str(),
                turn.model().model_id().as_str(),
                decode_digest(turn.request_hash().as_str())?.as_slice(),
                decode_digest(turn.rendered_hash().as_str())?.as_slice(),
                decode_digest(turn.manifest_hash().as_str())?.as_slice(),
                eligibility.user_id().as_str(),
                eligibility.workspace_id().as_str(),
                eligibility.agent_id().map(|id| id.as_str()),
                encode_sensitivity(eligibility.sensitivity_ceiling()),
                to_sql_sequence(budget.token_budget().get())?,
                to_sql_sequence(budget.reserved_tokens().get())?,
                to_sql_sequence(budget.durable_memory_limit().get())?,
                to_sql_sequence(turn.rendered_token_count().get())?,
                turn.committed_at().get(),
                rendered_state,
                rendered_utf8,
                rendered_content_hash.as_ref().map(|hash| hash.as_slice()),
                json,
                json_hash,
            ],
        )
        .map_err(|error| map_context_identity_error(error, IdentityKind::ContextTurn))?;
    Ok(())
}

fn insert_context_children(
    transaction: &Transaction<'_>,
    request: &ContextTurnCommitRequest,
) -> Result<(), StoreError> {
    let turn = request.turn();
    for (ordinal, snapshot) in turn.sources().iter().enumerate() {
        let json = serde_json::to_vec(snapshot).map_err(|_| StoreError::Backend)?;
        let json_hash = Sha256::digest(&json);
        transaction
            .execute(
                "INSERT INTO context_turn_sources (context_turn_id, ordinal, source_key, \
                 observation_state, source_revision_sha256, value_sha256, observed_at_ms, \
                 snapshot_json, snapshot_json_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    turn.context_turn_id().as_str(),
                    i64::try_from(ordinal).map_err(|_| StoreError::LimitExceeded)?,
                    snapshot.source_key().as_str(),
                    encode_observation_state(snapshot.observation_state()),
                    snapshot
                        .source_revision()
                        .map(|hash| decode_digest(hash.as_str()))
                        .transpose()?,
                    snapshot
                        .value_hash()
                        .map(|hash| decode_digest(hash.as_str()))
                        .transpose()?,
                    snapshot.observed_at().get(),
                    json,
                    json_hash.as_slice(),
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    for (admission, content) in turn.admissions().iter().zip(request.content().admissions()) {
        insert_admission(transaction, admission, content.rendered())?;
    }
    Ok(())
}

fn insert_admission(
    transaction: &Transaction<'_>,
    admission: &ContextAdmission,
    rendered: &RenderedContextText,
) -> Result<(), StoreError> {
    let json = serde_json::to_vec(admission).map_err(|_| StoreError::Backend)?;
    let json_hash = Sha256::digest(&json);
    let rendered_content_hash = Sha256::digest(rendered.as_str().as_bytes());
    transaction
        .execute(
            "INSERT INTO context_admissions (admission_id, context_turn_id, rank, section, \
             source_key, source_revision_sha256, memory_revision_id, renderer_version, \
             rendered_sha256, rank_score, token_count, admitted_at_ms, rendered_state, \
             rendered_utf8, rendered_content_sha256, admission_json, admission_json_sha256) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                    ?16, ?17)",
            params![
                admission.admission_id().as_str(),
                admission.context_turn_id().as_str(),
                i64::from(admission.rank()),
                encode_section(admission.section()),
                admission.source_key().as_str(),
                decode_digest(admission.source_revision().as_str())?.as_slice(),
                admission.memory_revision_id().map(|id| id.as_str()),
                i64::from(admission.renderer_version()),
                decode_digest(admission.rendered_hash().as_str())?.as_slice(),
                admission.rank_score(),
                to_sql_sequence(admission.token_count().get())?,
                admission.admitted_at().get(),
                "retained",
                rendered.as_str().as_bytes(),
                rendered_content_hash.as_slice(),
                json,
                json_hash.as_slice(),
            ],
        )
        .map_err(|error| map_context_identity_error(error, IdentityKind::ContextAdmission))?;
    for reason in admission.reasons() {
        transaction
            .execute(
                "INSERT INTO context_admission_reasons \
                 (admission_id, ordinal, factor, contribution) VALUES (?1, ?2, ?3, ?4)",
                params![
                    admission.admission_id().as_str(),
                    i64::from(reason.ordinal()),
                    encode_admission_factor(reason.factor()),
                    reason.contribution(),
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    Ok(())
}

fn load_json_record<T: serde::de::DeserializeOwned>(
    connection: &rusqlite::Connection,
    sql: &str,
    identity: &str,
) -> Result<Option<T>, StoreError> {
    let row = connection
        .query_row(sql, params![identity], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .optional()
        .map_err(map_sqlite_error)?;
    row.map(|(json, hash)| decode_context_json(&json, &hash))
        .transpose()
}

fn load_context_turn_rendered_content(
    connection: &rusqlite::Connection,
    context_turn_id: &ContextTurnId,
) -> Result<Option<RenderedContextText>, StoreError> {
    let row = connection
        .query_row(
            "SELECT manifest_json, manifest_json_sha256, rendered_state, rendered_utf8, \
                    rendered_content_sha256 \
             FROM context_turns WHERE context_turn_id = ?1",
            params![context_turn_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((json, json_hash, state, content, content_hash)) = row else {
        return Ok(None);
    };
    let turn: ContextTurnManifest = decode_context_json(&json, &json_hash)?;
    if turn.context_turn_id() != context_turn_id
        || !verify_context_manifest_hash(&turn).map_err(|_| corrupt_context())?
    {
        return Err(corrupt_context());
    }
    let rendered = decode_rendered_content(&state, content, content_hash)?;
    if let Some(rendered) = rendered.as_ref()
        && !verify_rendered_context_hash(rendered.as_str(), turn.rendered_hash())
            .map_err(|_| corrupt_context())?
    {
        return Err(corrupt_context());
    }
    Ok(rendered)
}

fn load_context_admission_rendered_content(
    connection: &rusqlite::Connection,
    admission_id: &autoharness_domain::ContextAdmissionId,
) -> Result<Option<RenderedContextText>, StoreError> {
    let row = connection
        .query_row(
            "SELECT a.admission_json, a.admission_json_sha256, a.rendered_state, \
                    a.rendered_utf8, a.rendered_content_sha256, r.memory_id \
             FROM context_admissions AS a \
             LEFT JOIN memory_revisions AS r ON r.revision_id = a.memory_revision_id \
             WHERE a.admission_id = ?1",
            params![admission_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?;
    let Some((json, json_hash, state, content, content_hash, memory_id)) = row else {
        return Ok(None);
    };
    let admission: ContextAdmission = decode_context_json(&json, &json_hash)?;
    if admission.admission_id() != admission_id {
        return Err(corrupt_context());
    }
    let memory_id = memory_id
        .map(MemoryId::new)
        .transpose()
        .map_err(|_| corrupt_context())?;
    let rendered = decode_rendered_content(&state, content, content_hash)?;
    if let Some(rendered) = rendered.as_ref()
        && !verify_admission_rendered_hash(&admission, memory_id.as_ref(), rendered.as_str())
            .map_err(|_| corrupt_context())?
    {
        return Err(corrupt_context());
    }
    Ok(rendered)
}

fn decode_rendered_content(
    state: &str,
    content: Option<Vec<u8>>,
    hash: Option<Vec<u8>>,
) -> Result<Option<RenderedContextText>, StoreError> {
    match (state, content, hash) {
        ("absent" | "erased", None, None) => Ok(None),
        ("retained", Some(content), Some(hash))
            if Sha256::digest(&content).as_slice() == hash.as_slice() =>
        {
            let content = String::from_utf8(content).map_err(|_| corrupt_context())?;
            RenderedContextText::new(content)
                .map(Some)
                .map_err(|_| corrupt_context())
        }
        _ => Err(corrupt_context()),
    }
}

fn decode_context_json<T: serde::de::DeserializeOwned>(
    json: &[u8],
    hash: &[u8],
) -> Result<T, StoreError> {
    if Sha256::digest(json).as_slice() != hash {
        return Err(corrupt_context());
    }
    serde_json::from_slice(json).map_err(|_| corrupt_context())
}

const fn encode_epoch_reason(reason: ContextEpochReason) -> &'static str {
    match reason {
        ContextEpochReason::NewAttempt => "new_attempt",
        ContextEpochReason::ExplicitRetry => "explicit_retry",
        ContextEpochReason::Compaction => "compaction",
        ContextEpochReason::SourceIncompatibility => "source_incompatibility",
        ContextEpochReason::PolicyChange => "policy_change",
        ContextEpochReason::Recovery => "recovery",
    }
}

const fn encode_observation_state(state: ContextObservationState) -> &'static str {
    match state {
        ContextObservationState::Available => "available",
        ContextObservationState::RetainedStale => "retained_stale",
        ContextObservationState::ObservedAbsent => "observed_absent",
        ContextObservationState::Unavailable => "unavailable",
    }
}

const fn encode_section(section: ContextSection) -> &'static str {
    match section {
        ContextSection::SafetyPolicy => "safety_policy",
        ContextSection::CurrentInstruction => "current_instruction",
        ContextSection::AuthorizedInstruction => "authorized_instruction",
        ContextSection::ToolContract => "tool_contract",
        ContextSection::ConversationHistory => "conversation_history",
        ContextSection::DurableMemory => "durable_memory",
    }
}

const fn encode_admission_factor(factor: ContextAdmissionFactor) -> &'static str {
    match factor {
        ContextAdmissionFactor::Pin => "pin",
        ContextAdmissionFactor::Authority => "authority",
        ContextAdmissionFactor::ExactMatch => "exact_match",
        ContextAdmissionFactor::ScopeSpecificity => "scope_specificity",
        ContextAdmissionFactor::LexicalOverlap => "lexical_overlap",
        ContextAdmissionFactor::Freshness => "freshness",
        ContextAdmissionFactor::Confidence => "confidence",
        ContextAdmissionFactor::PriorUtility => "prior_utility",
        ContextAdmissionFactor::Diversity => "diversity",
        ContextAdmissionFactor::BudgetFit => "budget_fit",
    }
}

const fn encode_sensitivity(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Sensitive => "sensitive",
        Sensitivity::Secret => "secret",
    }
}

fn decode_digest(value: &str) -> Result<[u8; 32], StoreError> {
    if value.len() != 64 {
        return Err(corrupt_context());
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *byte =
            u8::from_str_radix(&value[offset..offset + 2], 16).map_err(|_| corrupt_context())?;
    }
    Ok(output)
}

fn map_context_identity_error(error: rusqlite::Error, kind: IdentityKind) -> StoreError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => StoreError::IdentityConflict { kind },
        _ => map_sqlite_error(error),
    }
}

const fn corrupt_context() -> StoreError {
    StoreError::CorruptData {
        area: CorruptionArea::ContextLedger,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        AttemptId, CapabilityKind, CapabilityRequest, Causation, CommandId, ConfidenceBasisPoints,
        ContextAdmissionId, ContextAdmissionReason, ContextBudgetAllocation, ContextEligibility,
        ContextEpochHashes, ContextEpochVersions, ContextSourceKey, ContextSourceSnapshot,
        ContextTokenBudget, CorrelationId, DeliveryMode, EstimatedTokens, EventEnvelope, EventId,
        EventPayload, InputId, MemoryContent, MemoryGeneration, MemoryId, MemoryKind,
        MemoryOperationEnvelope, MemoryOperationId, MemoryOperationPayload, MemoryOrigin,
        MemoryRelation, MemoryRelationKind, MemoryRevision, MemoryRevisionDraft, MemoryRevisionId,
        MemoryRevisionNumber, MemoryScope, MemorySequence, MemoryValidity, ModelId, ModelRef,
        PermissionAnswer, PermissionDecisionId, PermissionOutcome, PromptText, ProviderCallId,
        ProviderId, ResourceRef, SessionId, SessionSequence, Sha256Digest, TimestampMillis,
        ToolArguments, ToolCallId, ToolCallSpec, ToolName, TrustClass, UserId, WorkspaceId,
    };
    use autoharness_memory::{
        COMPACTION_FACTS_VERSION, CONTEXT_RENDERER_VERSION, CanonicalEncoder, MEMORY_RENDERER_V1,
        SOURCE_RENDERER_V1, context_manifest_hash, normalized_content_hash, rendered_context_hash,
    };
    use autoharness_store::{
        AppendRequest, BoundContextTurnCommitRequest, ContextAdmissionContent,
        ContextCompactionBoundary, ContextTurnContent, MemoryAdmissionKey, MemoryAdmissionQuery,
        MemoryAppendRequest, MemoryRevisionContent, MemoryStore, RenderedContextText, SessionStore,
    };
    use tempfile::TempDir;

    use super::*;

    struct TestDatabase {
        _directory: TempDir,
        path: std::path::PathBuf,
    }

    impl TestDatabase {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("temporary directory");
            let path = directory.path().join("context.sqlite3");
            Self {
                _directory: directory,
                path,
            }
        }

        fn open(&self) -> SqliteStore {
            SqliteStore::open(&self.path).expect("open sqlite store")
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
    }

    fn session_event(sequence: u64, payload: EventPayload) -> EventEnvelope {
        EventEnvelope::new_v1(
            EventId::new(format!("event-context-{sequence}")).expect("event ID"),
            SessionId::new("session-context").expect("session ID"),
            SessionSequence::new(sequence).expect("session sequence"),
            TimestampMillis::new(i64::try_from(sequence).expect("timestamp")),
            Causation::Command(
                CommandId::new(format!("command-context-{sequence}")).expect("command ID"),
            ),
            CorrelationId::new(format!("correlation-context-{sequence}")).expect("correlation ID"),
            payload,
        )
    }

    fn seed_dispatch_ready_attempt(store: &mut SqliteStore) {
        let session_id = SessionId::new("session-context").expect("session ID");
        let input_id = InputId::new("input-context").expect("input ID");
        let attempt_id = AttemptId::new("attempt-context").expect("attempt ID");
        let events = vec![
            session_event(1, EventPayload::SessionCreated),
            session_event(2, EventPayload::ModelSelected { model: model() }),
            session_event(
                3,
                EventPayload::InputAdmitted {
                    input_id: input_id.clone(),
                    prompt: PromptText::new("context binding test").expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            ),
            session_event(
                4,
                EventPayload::AttemptPrepared {
                    attempt_id: attempt_id.clone(),
                    input_id,
                    model: model(),
                    retry_of: None,
                },
            ),
            session_event(5, EventPayload::AttemptStarted { attempt_id }),
        ];
        store
            .append(&AppendRequest::new(session_id, 0, events))
            .expect("seed dispatch-ready attempt");
    }

    fn seed_expiring_active_memory(store: &mut SqliteStore) -> (MemoryId, MemoryRevision) {
        let memory_id = MemoryId::new("memory-expiring").expect("memory ID");
        let content = MemoryContent::new("Fact frozen at the epoch boundary").expect("content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-expiring").expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Until {
                valid_until: TimestampMillis::new(10),
            },
            Vec::new(),
            Vec::new(),
        )
        .expect("revision draft");
        let revision = MemoryRevision::from_draft(
            MemoryRevisionStatus::Active,
            &draft,
            TimestampMillis::new(3),
            None,
        );
        let operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-expiring").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::FIRST,
            TimestampMillis::new(3),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new("command-expiring").expect("command ID"),
            ),
            CorrelationId::new("correlation-expiring").expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope: MemoryScope::User(UserId::new("user-1").expect("user ID")),
                memory_kind: MemoryKind::Fact,
                revision: revision.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                operation,
                Some(MemoryRevisionContent::new(
                    draft.revision_id().clone(),
                    content,
                    Vec::new(),
                )),
            ))
            .expect("append expiring memory");
        (memory_id, revision)
    }

    fn seed_active_memory(
        store: &mut SqliteStore,
        memory_id: &str,
        revision_id: &str,
        content_text: &str,
        relations: Vec<MemoryRelation>,
    ) -> (MemoryId, MemoryRevision) {
        let memory_id = MemoryId::new(memory_id).expect("memory ID");
        let content = MemoryContent::new(content_text).expect("content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new(revision_id).expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            relations,
        )
        .expect("revision draft");
        let revision = MemoryRevision::from_draft(
            MemoryRevisionStatus::Active,
            &draft,
            TimestampMillis::new(3),
            None,
        );
        let operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new(format!("operation-{memory_id}")).expect("operation ID"),
            memory_id.clone(),
            MemorySequence::FIRST,
            TimestampMillis::new(3),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new(format!("command-{memory_id}")).expect("command ID"),
            ),
            CorrelationId::new(format!("correlation-{memory_id}")).expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope: MemoryScope::User(UserId::new("user-1").expect("user ID")),
                memory_kind: MemoryKind::Fact,
                revision: revision.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                operation,
                Some(MemoryRevisionContent::new(
                    draft.revision_id().clone(),
                    content,
                    Vec::new(),
                )),
            ))
            .expect("append active memory");
        (memory_id, revision)
    }

    fn seed_approved_proposal_memory(store: &mut SqliteStore) -> (MemoryId, MemoryRevision) {
        let memory_id = MemoryId::new("memory-approved-proposal").expect("memory ID");
        let content = MemoryContent::new("Approved proposal can enter context").expect("content");
        let proposal_draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-original-proposal").expect("proposal revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ModelProposal,
            TrustClass::UntrustedProposal,
            ConfidenceBasisPoints::new(5_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("proposal draft");
        let proposal = MemoryRevision::from_draft(
            MemoryRevisionStatus::Proposed,
            &proposal_draft,
            TimestampMillis::new(6),
            None,
        );
        let create = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-create-proposal").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::FIRST,
            TimestampMillis::new(6),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new("command-create-proposal").expect("command ID"),
            ),
            CorrelationId::new("correlation-approved-proposal").expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope: MemoryScope::User(UserId::new("user-1").expect("user ID")),
                memory_kind: MemoryKind::Fact,
                revision: proposal.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create.clone(),
                Some(MemoryRevisionContent::new(
                    proposal.revision_id().clone(),
                    content.clone(),
                    Vec::new(),
                )),
            ))
            .expect("append proposal");

        let approved_draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-approved-proposal").expect("approved revision ID"),
            MemoryRevisionNumber::new(2).expect("revision number"),
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("approved draft");
        let approved = MemoryRevision::from_draft(
            MemoryRevisionStatus::Proposed,
            &approved_draft,
            TimestampMillis::new(7),
            Some(proposal.revision_id().clone()),
        );
        let approve = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-approve-proposal").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(7),
            autoharness_domain::MemoryCausation::Operation(create.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::ProposalApproved {
                proposal_revision_id: proposal.revision_id().clone(),
                approved_revision: approved.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                1,
                approve.clone(),
                Some(MemoryRevisionContent::new(
                    approved.revision_id().clone(),
                    content,
                    Vec::new(),
                )),
            ))
            .expect("append approved revision");
        let activate = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-activate-approved").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::new(3).expect("sequence"),
            TimestampMillis::new(8),
            autoharness_domain::MemoryCausation::Operation(approve.operation_id().clone()),
            approve.correlation_id().clone(),
            MemoryOperationPayload::RevisionActivated {
                revision_id: approved.revision_id().clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(2, activate, None))
            .expect("activate approved revision");
        (
            memory_id,
            approved.with_status(MemoryRevisionStatus::Active),
        )
    }

    fn memory_turn(
        turn_id: &str,
        epoch_id: &str,
        run_turn: u32,
        expected_sequence: u64,
        committed_at_ms: i64,
        memory_id: &MemoryId,
        revision: &MemoryRevision,
    ) -> (ContextTurnManifest, ContextTurnContent) {
        const RENDERED: &str = "<memory>frozen epoch fact</memory>";
        let context_turn_id = ContextTurnId::new(turn_id).expect("turn ID");
        let mut rendered_encoder = CanonicalEncoder::new();
        rendered_encoder
            .field("renderer", MEMORY_RENDERER_V1.as_bytes())
            .expect("renderer field");
        rendered_encoder
            .field("memory_id", memory_id.as_str().as_bytes())
            .expect("memory ID field");
        rendered_encoder
            .field("revision_id", revision.revision_id().as_str().as_bytes())
            .expect("revision ID field");
        rendered_encoder
            .field("rendered", RENDERED.as_bytes())
            .expect("rendered field");
        let admission = ContextAdmission::new(
            ContextAdmissionId::new(format!("memory-admission-{run_turn}")).expect("admission ID"),
            context_turn_id.clone(),
            ContextSection::DurableMemory,
            ContextSourceKey::new("memory:expiring").expect("source key"),
            revision.content_hash().clone(),
            Some(revision.revision_id().clone()),
            CONTEXT_RENDERER_VERSION,
            rendered_encoder.finish().expect("rendered hash"),
            1,
            100,
            EstimatedTokens::new(8).expect("tokens"),
            TimestampMillis::new(committed_at_ms),
            vec![
                ContextAdmissionReason::new(1, ContextAdmissionFactor::ExactMatch, 100)
                    .expect("reason"),
            ],
        )
        .expect("admission");
        let placeholder = ContextTurnManifest::new(
            context_turn_id,
            ContextEpochId::new(epoch_id).expect("epoch ID"),
            SessionId::new("session-context").expect("session ID"),
            AttemptId::new("attempt-context").expect("attempt ID"),
            run_turn,
            SessionSequence::new(expected_sequence).expect("sequence"),
            MemoryGeneration::new(1).expect("generation"),
            model(),
            digest('3'),
            rendered_context_hash(RENDERED).expect("context hash"),
            digest('5'),
            ContextEligibility::new(
                UserId::new("user-1").expect("user ID"),
                WorkspaceId::new("workspace-1").expect("workspace ID"),
                SessionId::new("session-context").expect("session ID"),
                None,
                Sensitivity::Internal,
            ),
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("budget"),
                EstimatedTokens::new(0).expect("reserved tokens"),
                EstimatedTokens::new(2_048).expect("memory limit"),
            )
            .expect("budget allocation"),
            EstimatedTokens::new(8).expect("tokens"),
            TimestampMillis::new(committed_at_ms),
            Vec::new(),
            vec![admission],
        )
        .expect("turn placeholder");
        let turn = ContextTurnManifest::new(
            placeholder.context_turn_id().clone(),
            placeholder.epoch_id().clone(),
            placeholder.session_id().clone(),
            placeholder.attempt_id().clone(),
            placeholder.run_turn(),
            placeholder.expected_session_sequence(),
            placeholder.memory_generation(),
            placeholder.model().clone(),
            placeholder.request_hash().clone(),
            placeholder.rendered_hash().clone(),
            context_manifest_hash(&placeholder).expect("manifest hash"),
            placeholder.eligibility().clone(),
            placeholder.budget(),
            placeholder.rendered_token_count(),
            placeholder.committed_at(),
            placeholder.sources().to_vec(),
            placeholder.admissions().to_vec(),
        )
        .expect("turn");
        let content = ContextTurnContent::new(
            Some(RenderedContextText::new(RENDERED).expect("prelude")),
            vec![ContextAdmissionContent::new(
                turn.admissions()[0].admission_id().clone(),
                RenderedContextText::new(RENDERED).expect("rendered memory"),
            )],
        );
        (turn, content)
    }

    fn seed_attempt(store: &SqliteStore) {
        let zero_hash = [0_u8; 32];
        store
            .connection
            .execute(
                "INSERT INTO sessions (session_id, status, selected_provider_id, \
                 selected_model_id, last_sequence, created_at_ms, updated_at_ms) \
                 VALUES ('session-context', 'active', 'google-ai-studio', \
                         'models/gemini-test', 2, 1, 2)",
                [],
            )
            .expect("session");
        store
            .connection
            .execute(
                "INSERT INTO session_events (event_id, session_id, sequence, schema_version, \
                 occurred_at_ms, caused_by_command_id, caused_by_event_id, correlation_id, \
                 event_kind, envelope_json) \
                 VALUES ('event-input', 'session-context', 1, 1, 1, 'command-input', NULL, \
                         'correlation-input', 'input_admitted', x'01')",
                [],
            )
            .expect("input event");
        store
            .connection
            .execute(
                "INSERT INTO admitted_inputs (session_id, input_id, admitted_event_id, \
                 admitted_sequence, delivery_mode, state, prompt_utf8, content_sha256, admitted_at_ms) \
                 VALUES ('session-context', 'input-context', 'event-input', 1, 'next_turn', \
                         'admitted', x'61', ?1, 1)",
                params![zero_hash.as_slice()],
            )
            .expect("input projection");
        store
            .connection
            .execute(
                "INSERT INTO session_events (event_id, session_id, sequence, schema_version, \
                 occurred_at_ms, caused_by_command_id, caused_by_event_id, correlation_id, \
                 event_kind, envelope_json) \
                 VALUES ('event-attempt', 'session-context', 2, 1, 2, 'command-attempt', NULL, \
                         'correlation-attempt', 'attempt_prepared', x'02')",
                [],
            )
            .expect("attempt event");
        store
            .connection
            .execute(
                "INSERT INTO provider_attempts (attempt_id, session_id, input_id, provider_id, \
                 model_id, state, prepared_event_id, prepared_sequence, prepared_at_ms) \
                 VALUES ('attempt-context', 'session-context', 'input-context', \
                         'google-ai-studio', 'models/gemini-test', 'prepared', \
                         'event-attempt', 2, 2)",
                [],
            )
            .expect("attempt projection");
    }

    fn model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider ID"),
            ModelId::new("models/gemini-test").expect("model ID"),
        )
    }

    fn epoch(
        epoch_id: &str,
        reason: ContextEpochReason,
        predecessor: Option<&str>,
    ) -> ContextEpochManifest {
        epoch_at(epoch_id, reason, predecessor, MemoryGeneration::INITIAL, 3)
    }

    fn epoch_at(
        epoch_id: &str,
        reason: ContextEpochReason,
        predecessor: Option<&str>,
        generation: MemoryGeneration,
        started_at_ms: i64,
    ) -> ContextEpochManifest {
        ContextEpochManifest::new(
            ContextEpochId::new(epoch_id).expect("epoch ID"),
            SessionId::new("session-context").expect("session ID"),
            generation,
            reason,
            predecessor.map(|id| ContextEpochId::new(id).expect("predecessor ID")),
            digest('a'),
            ContextEpochVersions::new(1, 1, 1, 1, 1).expect("versions"),
            ContextEpochHashes::new(digest('b'), digest('c'), digest('d'), digest('e')),
            ContextTokenBudget::new(4_096).expect("budget"),
            TimestampMillis::new(started_at_ms),
        )
        .expect("epoch")
    }

    fn turn(turn_id: &str, epoch_id: &str, run_turn: u32) -> ContextTurnManifest {
        turn_at(turn_id, epoch_id, run_turn, 2, "workspace-1")
    }

    fn turn_at(
        turn_id: &str,
        epoch_id: &str,
        run_turn: u32,
        expected_sequence: u64,
        workspace_id: &str,
    ) -> ContextTurnManifest {
        turn_at_with_budget(
            turn_id,
            epoch_id,
            run_turn,
            expected_sequence,
            workspace_id,
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("budget"),
                EstimatedTokens::new(0).expect("reserved tokens"),
                EstimatedTokens::new(2_048).expect("memory limit"),
            )
            .expect("budget allocation"),
        )
    }

    fn turn_at_generation(
        turn_id: &str,
        epoch_id: &str,
        run_turn: u32,
        expected_sequence: u64,
        generation: MemoryGeneration,
    ) -> ContextTurnManifest {
        let original = turn_at(
            turn_id,
            epoch_id,
            run_turn,
            expected_sequence,
            "workspace-1",
        );
        let placeholder = ContextTurnManifest::new(
            original.context_turn_id().clone(),
            original.epoch_id().clone(),
            original.session_id().clone(),
            original.attempt_id().clone(),
            original.run_turn(),
            original.expected_session_sequence(),
            generation,
            original.model().clone(),
            original.request_hash().clone(),
            original.rendered_hash().clone(),
            digest('0'),
            original.eligibility().clone(),
            original.budget(),
            original.rendered_token_count(),
            original.committed_at(),
            original.sources().to_vec(),
            original.admissions().to_vec(),
        )
        .expect("turn placeholder with generation");
        ContextTurnManifest::new(
            placeholder.context_turn_id().clone(),
            placeholder.epoch_id().clone(),
            placeholder.session_id().clone(),
            placeholder.attempt_id().clone(),
            placeholder.run_turn(),
            placeholder.expected_session_sequence(),
            placeholder.memory_generation(),
            placeholder.model().clone(),
            placeholder.request_hash().clone(),
            placeholder.rendered_hash().clone(),
            context_manifest_hash(&placeholder).expect("manifest hash"),
            placeholder.eligibility().clone(),
            placeholder.budget(),
            placeholder.rendered_token_count(),
            placeholder.committed_at(),
            placeholder.sources().to_vec(),
            placeholder.admissions().to_vec(),
        )
        .expect("turn with generation")
    }

    struct CompactionFixture {
        predecessor: ContextEpochManifest,
        epoch: ContextEpochManifest,
        turn: ContextTurnManifest,
        context: ContextTurnCommitRequest,
        binding: EventEnvelope,
    }

    fn seed_compaction_fixture(
        store: &mut SqliteStore,
        generation: MemoryGeneration,
    ) -> CompactionFixture {
        seed_dispatch_ready_attempt(store);
        let predecessor = epoch_at(
            "epoch-proof-predecessor",
            ContextEpochReason::NewAttempt,
            None,
            generation,
            3,
        );
        let first_turn = turn_at_generation(
            "turn-proof-predecessor",
            predecessor.epoch_id().as_str(),
            1,
            5,
            generation,
        );
        let first_binding = session_event(
            6,
            EventPayload::ContextTurnBound {
                attempt_id: first_turn.attempt_id().clone(),
                run_turn: first_turn.run_turn(),
                context_turn_id: first_turn.context_turn_id().clone(),
                manifest_hash: first_turn.manifest_hash().clone(),
            },
        );
        store
            .commit_context_turn_and_bind(&BoundContextTurnCommitRequest::new(
                ContextTurnCommitRequest::new(
                    Some(predecessor.clone()),
                    first_turn.clone(),
                    turn_content(&first_turn),
                ),
                first_binding,
            ))
            .expect("bind predecessor context");
        store
            .append(&AppendRequest::new(
                first_turn.session_id().clone(),
                6,
                vec![
                    session_event(
                        7,
                        EventPayload::RunTurnStarted {
                            attempt_id: first_turn.attempt_id().clone(),
                            turn: 1,
                        },
                    ),
                    session_event(
                        8,
                        EventPayload::AttemptPausedForTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                    session_event(
                        9,
                        EventPayload::AttemptResumedAfterTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                ],
            ))
            .expect("advance to compaction boundary");

        let epoch = epoch_at(
            "epoch-proof-compaction",
            ContextEpochReason::Compaction,
            Some(predecessor.epoch_id().as_str()),
            generation,
            9,
        );
        let turn = turn_at_generation(
            "turn-proof-compaction",
            epoch.epoch_id().as_str(),
            2,
            9,
            generation,
        );
        let context =
            ContextTurnCommitRequest::new(Some(epoch.clone()), turn.clone(), turn_content(&turn));
        let binding = session_event(
            10,
            EventPayload::ContextTurnBound {
                attempt_id: turn.attempt_id().clone(),
                run_turn: turn.run_turn(),
                context_turn_id: turn.context_turn_id().clone(),
                manifest_hash: turn.manifest_hash().clone(),
            },
        );
        CompactionFixture {
            predecessor,
            epoch,
            turn,
            context,
            binding,
        }
    }

    fn compaction_fingerprint(
        store: &mut SqliteStore,
        fixture: &CompactionFixture,
    ) -> EffectiveDurableFactsFingerprint {
        let transaction = store
            .connection
            .transaction()
            .expect("open proof transaction");
        let fingerprint =
            compute_compaction_fingerprint(&transaction, &fixture.epoch, &fixture.turn)
                .expect("compute compaction fingerprint");
        transaction.rollback().expect("roll back proof transaction");
        fingerprint
    }

    fn compaction_boundary(
        fixture: &CompactionFixture,
        fingerprint: &EffectiveDurableFactsFingerprint,
    ) -> ContextCompactionBoundary {
        compaction_boundary_with_claims(
            fixture,
            fingerprint.hash().clone(),
            fingerprint.memory_fact_count(),
            fingerprint.pending_session_fact_count(),
        )
    }

    fn compaction_boundary_with_claims(
        fixture: &CompactionFixture,
        facts_hash: Sha256Digest,
        memory_fact_count: u32,
        pending_session_fact_count: u32,
    ) -> ContextCompactionBoundary {
        ContextCompactionBoundary::new(
            fixture.epoch.epoch_id().clone(),
            fixture.predecessor.epoch_id().clone(),
            fixture.turn.session_id().clone(),
            fixture.turn.expected_session_sequence(),
            fixture.turn.memory_generation(),
            COMPACTION_FACTS_VERSION,
            facts_hash,
            memory_fact_count,
            pending_session_fact_count,
            None,
            TimestampMillis::new(9),
        )
    }

    fn turn_at_with_budget(
        turn_id: &str,
        epoch_id: &str,
        run_turn: u32,
        expected_sequence: u64,
        workspace_id: &str,
        budget: ContextBudgetAllocation,
    ) -> ContextTurnManifest {
        try_turn_at_with_budget(
            turn_id,
            epoch_id,
            run_turn,
            expected_sequence,
            workspace_id,
            budget,
        )
        .expect("turn")
    }

    fn try_turn_at_with_budget(
        turn_id: &str,
        epoch_id: &str,
        run_turn: u32,
        expected_sequence: u64,
        workspace_id: &str,
        budget: ContextBudgetAllocation,
    ) -> Result<ContextTurnManifest, autoharness_domain::ValueError> {
        const RENDERED_ADMISSION: &str = "<source>workspace agents</source>";
        const RENDERED_PRELUDE: &str = "<context>workspace agents</context>";
        let snapshot = ContextSourceSnapshot::new(
            ContextSourceKey::new("workspace:agents").expect("source key"),
            ContextObservationState::Available,
            Some(digest('f')),
            Some(digest('1')),
            TimestampMillis::new(4),
        )
        .expect("snapshot");
        let mut rendered_encoder = CanonicalEncoder::new();
        rendered_encoder
            .field("renderer", SOURCE_RENDERER_V1.as_bytes())
            .expect("renderer field");
        rendered_encoder
            .field("source_key", b"workspace:agents")
            .expect("source key field");
        rendered_encoder
            .field("source_revision", digest('f').as_str().as_bytes())
            .expect("source revision field");
        rendered_encoder
            .field("section", b"authorized_instruction")
            .expect("section field");
        rendered_encoder
            .field("rendered", RENDERED_ADMISSION.as_bytes())
            .expect("rendered field");
        let admission = ContextAdmission::new(
            ContextAdmissionId::new(format!("admission-{run_turn}")).expect("admission ID"),
            ContextTurnId::new(turn_id).expect("turn ID"),
            ContextSection::AuthorizedInstruction,
            ContextSourceKey::new("workspace:agents").expect("source key"),
            digest('f'),
            None,
            CONTEXT_RENDERER_VERSION,
            rendered_encoder.finish().expect("rendered hash"),
            1,
            100,
            EstimatedTokens::new(32).expect("tokens"),
            TimestampMillis::new(5),
            vec![
                ContextAdmissionReason::new(1, ContextAdmissionFactor::Authority, 100)
                    .expect("reason"),
            ],
        )
        .expect("admission");
        let placeholder = ContextTurnManifest::new(
            ContextTurnId::new(turn_id).expect("turn ID"),
            ContextEpochId::new(epoch_id).expect("epoch ID"),
            SessionId::new("session-context").expect("session ID"),
            AttemptId::new("attempt-context").expect("attempt ID"),
            run_turn,
            SessionSequence::new(expected_sequence).expect("sequence"),
            MemoryGeneration::INITIAL,
            model(),
            digest('3'),
            rendered_context_hash(RENDERED_PRELUDE).expect("context hash"),
            digest('5'),
            ContextEligibility::new(
                UserId::new("user-1").expect("user ID"),
                WorkspaceId::new(workspace_id).expect("workspace ID"),
                SessionId::new("session-context").expect("session ID"),
                None,
                Sensitivity::Internal,
            ),
            budget,
            EstimatedTokens::new(32).expect("tokens"),
            TimestampMillis::new(6 + i64::from(run_turn)),
            vec![snapshot],
            vec![admission],
        )?;
        let manifest_hash = context_manifest_hash(&placeholder).expect("manifest hash");
        ContextTurnManifest::new(
            placeholder.context_turn_id().clone(),
            placeholder.epoch_id().clone(),
            placeholder.session_id().clone(),
            placeholder.attempt_id().clone(),
            placeholder.run_turn(),
            placeholder.expected_session_sequence(),
            placeholder.memory_generation(),
            placeholder.model().clone(),
            placeholder.request_hash().clone(),
            placeholder.rendered_hash().clone(),
            manifest_hash,
            placeholder.eligibility().clone(),
            placeholder.budget(),
            placeholder.rendered_token_count(),
            placeholder.committed_at(),
            placeholder.sources().to_vec(),
            placeholder.admissions().to_vec(),
        )
    }

    fn turn_content(turn: &ContextTurnManifest) -> ContextTurnContent {
        ContextTurnContent::new(
            Some(RenderedContextText::new("<context>workspace agents</context>").expect("prelude")),
            turn.admissions()
                .iter()
                .map(|admission| {
                    ContextAdmissionContent::new(
                        admission.admission_id().clone(),
                        RenderedContextText::new("<source>workspace agents</source>")
                            .expect("rendered admission"),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn context_turn_commit_is_atomic_idempotent_and_restart_safe() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_attempt(&store);
        let epoch = epoch("epoch-1", ContextEpochReason::NewAttempt, None);
        let turn = turn("turn-1", "epoch-1", 1);
        let request =
            ContextTurnCommitRequest::new(Some(epoch.clone()), turn.clone(), turn_content(&turn));
        assert_eq!(
            store.commit_context_turn(&request).expect("commit context"),
            ContextCommitDisposition::Committed
        );
        assert_eq!(
            store.commit_context_turn(&request).expect("retry context"),
            ContextCommitDisposition::AlreadyCommitted
        );
        drop(store);

        let reopened = database.open();
        assert_eq!(
            reopened
                .load_context_epoch(epoch.epoch_id())
                .expect("load epoch"),
            Some(epoch)
        );
        assert_eq!(
            reopened
                .load_context_turn(turn.context_turn_id())
                .expect("load turn"),
            Some(turn.clone())
        );
        assert_eq!(
            reopened
                .load_attempt_context_turn(turn.attempt_id(), 1)
                .expect("load attempt turn"),
            Some(turn.clone())
        );
        assert_eq!(
            reopened
                .load_context_admissions(turn.context_turn_id())
                .expect("load admissions"),
            turn.admissions()
        );
        assert_eq!(
            reopened
                .load_context_turn_content(turn.context_turn_id())
                .expect("load rendered turn")
                .expect("retained rendered turn")
                .as_str(),
            "<context>workspace agents</context>"
        );
    }

    #[test]
    fn first_binding_revalidates_a_staged_turn_but_bound_retry_remains_idempotent() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_dispatch_ready_attempt(&mut store);
        let epoch = epoch("epoch-staged", ContextEpochReason::NewAttempt, None);
        let turn = turn_at("turn-staged", "epoch-staged", 1, 5, "workspace-1");
        let context = ContextTurnCommitRequest::new(Some(epoch), turn.clone(), turn_content(&turn));
        store
            .commit_context_turn(&context)
            .expect("stage unbound context");
        let binding = session_event(
            6,
            EventPayload::ContextTurnBound {
                attempt_id: turn.attempt_id().clone(),
                run_turn: 1,
                context_turn_id: turn.context_turn_id().clone(),
                manifest_hash: turn.manifest_hash().clone(),
            },
        );
        let request = BoundContextTurnCommitRequest::new(context, binding);

        store
            .connection
            .execute(
                "UPDATE memory_store_state SET generation = 1 WHERE singleton = 1",
                [],
            )
            .expect("advance generation");
        assert_eq!(
            store.commit_context_turn_and_bind(&request),
            Err(StoreError::ContextGenerationConflict {
                expected: 0,
                actual: 1,
            })
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM context_turn_bindings", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("unbound count"),
            0
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT last_sequence, workspace_id FROM sessions \
                     WHERE session_id = 'session-context'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .expect("unchanged session"),
            (5, None)
        );

        store
            .connection
            .execute(
                "UPDATE memory_store_state SET generation = 0 WHERE singleton = 1",
                [],
            )
            .expect("restore generation");
        assert_eq!(
            store
                .commit_context_turn_and_bind(&request)
                .expect("bind staged context")
                .disposition(),
            ContextCommitDisposition::Committed
        );
        store
            .connection
            .execute(
                "UPDATE memory_store_state SET generation = 1 WHERE singleton = 1",
                [],
            )
            .expect("advance after binding");
        assert_eq!(
            store
                .commit_context_turn_and_bind(&request)
                .expect("exact bound retry")
                .disposition(),
            ContextCommitDisposition::AlreadyCommitted
        );
    }

    #[test]
    fn frozen_epoch_allows_dynamic_reservation_only_while_exact_baseline_still_fits() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_dispatch_ready_attempt(&mut store);
        let first_epoch = epoch("epoch-dynamic-budget", ContextEpochReason::NewAttempt, None);
        let first_turn = turn_at_with_budget(
            "turn-dynamic-budget-1",
            "epoch-dynamic-budget",
            1,
            5,
            "workspace-1",
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("budget"),
                EstimatedTokens::new(0).expect("reserved"),
                EstimatedTokens::new(0).expect("durable memory"),
            )
            .expect("first budget"),
        );
        let first_request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(
                Some(first_epoch),
                first_turn.clone(),
                turn_content(&first_turn),
            ),
            session_event(
                6,
                EventPayload::ContextTurnBound {
                    attempt_id: first_turn.attempt_id().clone(),
                    run_turn: 1,
                    context_turn_id: first_turn.context_turn_id().clone(),
                    manifest_hash: first_turn.manifest_hash().clone(),
                },
            ),
        );
        store
            .commit_context_turn_and_bind(&first_request)
            .expect("bind first baseline");
        store
            .append(&AppendRequest::new(
                first_turn.session_id().clone(),
                6,
                vec![
                    session_event(
                        7,
                        EventPayload::RunTurnStarted {
                            attempt_id: first_turn.attempt_id().clone(),
                            turn: 1,
                        },
                    ),
                    session_event(
                        8,
                        EventPayload::AttemptPausedForTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                    session_event(
                        9,
                        EventPayload::AttemptResumedAfterTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                ],
            ))
            .expect("prepare second turn");

        let second_turn = turn_at_with_budget(
            "turn-dynamic-budget-2",
            "epoch-dynamic-budget",
            2,
            9,
            "workspace-1",
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("budget"),
                EstimatedTokens::new(512).expect("reserved"),
                EstimatedTokens::new(0).expect("durable memory"),
            )
            .expect("second budget"),
        );
        let second_request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(None, second_turn.clone(), turn_content(&second_turn)),
            session_event(
                10,
                EventPayload::ContextTurnBound {
                    attempt_id: second_turn.attempt_id().clone(),
                    run_turn: 2,
                    context_turn_id: second_turn.context_turn_id().clone(),
                    manifest_hash: second_turn.manifest_hash().clone(),
                },
            ),
        );
        store
            .commit_context_turn_and_bind(&second_request)
            .expect("changed dynamic history budget still fits frozen baseline");
        store
            .append(&AppendRequest::new(
                second_turn.session_id().clone(),
                10,
                vec![
                    session_event(
                        11,
                        EventPayload::RunTurnStarted {
                            attempt_id: second_turn.attempt_id().clone(),
                            turn: 2,
                        },
                    ),
                    session_event(
                        12,
                        EventPayload::AttemptPausedForTools {
                            attempt_id: second_turn.attempt_id().clone(),
                        },
                    ),
                    session_event(
                        13,
                        EventPayload::AttemptResumedAfterTools {
                            attempt_id: second_turn.attempt_id().clone(),
                        },
                    ),
                ],
            ))
            .expect("prepare third turn");

        let too_small = try_turn_at_with_budget(
            "turn-dynamic-budget-3",
            "epoch-dynamic-budget",
            3,
            13,
            "workspace-1",
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("budget"),
                EstimatedTokens::new(4_080).expect("reserved"),
                EstimatedTokens::new(0).expect("durable memory"),
            )
            .expect("third budget"),
        );
        assert_eq!(
            too_small,
            Err(autoharness_domain::ValueError::InvalidContextManifest)
        );
    }

    #[test]
    fn compaction_bind_rejects_forged_hash_and_counts_without_partial_writes() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let fixture = seed_compaction_fixture(&mut store, MemoryGeneration::INITIAL);
        let fingerprint = compaction_fingerprint(&mut store, &fixture);
        assert_eq!(fingerprint.memory_fact_count(), 0);
        assert_eq!(fingerprint.pending_session_fact_count(), 0);

        let forged_boundaries = [
            compaction_boundary_with_claims(
                &fixture,
                digest('9'),
                fingerprint.memory_fact_count(),
                fingerprint.pending_session_fact_count(),
            ),
            compaction_boundary_with_claims(
                &fixture,
                fingerprint.hash().clone(),
                fingerprint.memory_fact_count() + 1,
                fingerprint.pending_session_fact_count(),
            ),
            compaction_boundary_with_claims(
                &fixture,
                fingerprint.hash().clone(),
                fingerprint.memory_fact_count(),
                fingerprint.pending_session_fact_count() + 1,
            ),
        ];
        for forged in forged_boundaries {
            let request = BoundContextTurnCommitRequest::new(
                fixture.context.clone(),
                fixture.binding.clone(),
            )
            .with_compaction_boundary(forged);
            assert_eq!(
                store.commit_context_turn_and_bind(&request),
                Err(StoreError::InvalidContextTransition)
            );
            assert!(
                store
                    .load_context_turn(fixture.turn.context_turn_id())
                    .expect("load rejected turn")
                    .is_none()
            );
            assert!(
                store
                    .load_compaction_boundary(fixture.epoch.epoch_id())
                    .expect("load rejected boundary")
                    .is_none()
            );
            assert_eq!(
                store
                    .connection
                    .query_row(
                        "SELECT COUNT(*) FROM session_events WHERE sequence = 10",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .expect("binding event count"),
                0
            );
        }

        let valid =
            BoundContextTurnCommitRequest::new(fixture.context.clone(), fixture.binding.clone())
                .with_compaction_boundary(compaction_boundary(&fixture, &fingerprint));
        assert_eq!(
            store
                .commit_context_turn_and_bind(&valid)
                .expect("retry with the verified proof")
                .disposition(),
            ContextCommitDisposition::Committed
        );
    }

    #[test]
    fn compaction_retry_survives_restart_and_later_authoritative_mutation() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let fixture = seed_compaction_fixture(&mut store, MemoryGeneration::INITIAL);
        let fingerprint = compaction_fingerprint(&mut store, &fixture);
        let boundary = compaction_boundary(&fixture, &fingerprint);
        let request =
            BoundContextTurnCommitRequest::new(fixture.context.clone(), fixture.binding.clone())
                .with_compaction_boundary(boundary.clone());
        store
            .commit_context_turn_and_bind(&request)
            .expect("commit verified compaction");

        seed_active_memory(
            &mut store,
            "memory-after-compaction",
            "revision-after-compaction",
            "This mutation happened after the frozen proof",
            Vec::new(),
        );
        drop(store);

        let mut reopened = database.open();
        assert_eq!(
            reopened
                .commit_context_turn_and_bind(&request)
                .expect("retry exact committed proof after restart and mutation")
                .disposition(),
            ContextCommitDisposition::AlreadyCommitted
        );
        assert_eq!(
            reopened
                .load_compaction_boundary(fixture.epoch.epoch_id())
                .expect("load immutable proof"),
            Some(boundary)
        );
    }

    #[test]
    fn compaction_bind_rejects_memory_mutation_after_the_caller_snapshot() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_active_memory(
            &mut store,
            "memory-before-snapshot",
            "revision-before-snapshot",
            "Fact present in the caller snapshot",
            Vec::new(),
        );
        let fixture = seed_compaction_fixture(
            &mut store,
            MemoryGeneration::new(1).expect("memory generation"),
        );
        let fingerprint = compaction_fingerprint(&mut store, &fixture);
        let request =
            BoundContextTurnCommitRequest::new(fixture.context.clone(), fixture.binding.clone())
                .with_compaction_boundary(compaction_boundary(&fixture, &fingerprint));

        seed_active_memory(
            &mut store,
            "memory-raced-snapshot",
            "revision-raced-snapshot",
            "Fact committed before the atomic bind began",
            Vec::new(),
        );
        assert_eq!(
            store.commit_context_turn_and_bind(&request),
            Err(StoreError::ContextGenerationConflict {
                expected: 1,
                actual: 2,
            })
        );
        assert!(
            store
                .load_context_turn(fixture.turn.context_turn_id())
                .expect("load raced turn")
                .is_none()
        );
        assert!(
            store
                .load_compaction_boundary(fixture.epoch.epoch_id())
                .expect("load raced boundary")
                .is_none()
        );
    }

    #[test]
    fn compaction_proof_is_order_independent_and_excludes_conflicted_active_heads() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_active_memory(
            &mut store,
            "memory-zeta",
            "revision-zeta",
            "Zeta fact inserted first",
            Vec::new(),
        );
        seed_active_memory(
            &mut store,
            "memory-conflicted",
            "revision-conflicted",
            "Conflicted active head must not survive compaction",
            vec![MemoryRelation::new(
                MemoryId::new("memory-contradiction-target").expect("relation target"),
                MemoryRelationKind::Contradicts,
            )],
        );
        seed_active_memory(
            &mut store,
            "memory-alpha",
            "revision-alpha",
            "Alpha fact inserted last",
            Vec::new(),
        );
        let fixture = seed_compaction_fixture(
            &mut store,
            MemoryGeneration::new(3).expect("memory generation"),
        );
        let fingerprint = compaction_fingerprint(&mut store, &fixture);
        assert_eq!(fingerprint.memory_fact_count(), 2);

        let transaction = store
            .connection
            .transaction()
            .expect("open ordering transaction");
        let mut candidates = load_compaction_memory_candidates(&transaction, &fixture.turn)
            .expect("load candidates");
        assert_eq!(candidates.len(), 3);
        assert!(
            candidates
                .iter()
                .find(|candidate| candidate.memory_id.as_str() == "memory-conflicted")
                .is_some_and(|candidate| candidate.conflicted)
        );
        candidates.reverse();
        let events = load_session_events_through(
            &transaction,
            fixture.turn.session_id(),
            fixture.turn.expected_session_sequence(),
        )
        .expect("load event prefix");
        let pending = pending_session_facts_from_events(
            fixture.turn.session_id(),
            fixture.turn.expected_session_sequence(),
            &events,
        )
        .expect("derive pending facts");
        let eligibility = fixture.turn.eligibility();
        let reordered = effective_durable_facts(
            &RetrievalScope {
                user_id: eligibility.user_id().clone(),
                workspace_id: eligibility.workspace_id().clone(),
                session_id: eligibility.session_id().clone(),
                agent_id: eligibility.agent_id().cloned(),
                as_of: fixture.epoch.started_at(),
                sensitivity_ceiling: eligibility.sensitivity_ceiling(),
            },
            &candidates,
            &pending,
        )
        .expect("hash reordered candidates");
        transaction
            .rollback()
            .expect("roll back ordering transaction");
        assert_eq!(reordered, fingerprint);
    }

    #[test]
    fn compaction_bind_fails_closed_when_eligible_content_was_erased() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (_, revision) = seed_active_memory(
            &mut store,
            "memory-erased-proof",
            "revision-erased-proof",
            "Eligible content that must be independently verified",
            Vec::new(),
        );
        let fixture = seed_compaction_fixture(
            &mut store,
            MemoryGeneration::new(1).expect("memory generation"),
        );
        let fingerprint = compaction_fingerprint(&mut store, &fixture);
        assert_eq!(fingerprint.memory_fact_count(), 1);
        let request =
            BoundContextTurnCommitRequest::new(fixture.context.clone(), fixture.binding.clone())
                .with_compaction_boundary(compaction_boundary(&fixture, &fingerprint));

        store
            .connection
            .execute(
                "DELETE FROM memory_content_blobs WHERE content_id = (\
                    SELECT content_id FROM memory_revisions WHERE revision_id = ?1\
                 )",
                [revision.revision_id().as_str()],
            )
            .expect("erase eligible content sidecar");
        assert_eq!(
            store.commit_context_turn_and_bind(&request),
            Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            })
        );
        assert!(
            store
                .load_context_turn(fixture.turn.context_turn_id())
                .expect("load rejected turn")
                .is_none()
        );
    }

    #[test]
    fn compaction_proof_tracks_unpromoted_input_and_permission_state_changes() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let fixture = seed_compaction_fixture(&mut store, MemoryGeneration::INITIAL);
        let tool_call_id = ToolCallId::new("tool-proof-pending").expect("tool call ID");
        let decision_id = PermissionDecisionId::new("decision-proof-pending").expect("decision ID");
        let call = ToolCallSpec {
            tool_call_id: tool_call_id.clone(),
            provider_call_id: ProviderCallId::new("provider-call-proof-pending")
                .expect("provider call ID"),
            tool_name: ToolName::new("read_file").expect("tool name"),
            schema_version: 1,
            arguments: ToolArguments::new(serde_json::json!({"path": "notes.txt"}))
                .expect("tool arguments"),
            capability: CapabilityRequest {
                kind: CapabilityKind::FilesystemRead,
                resource: ResourceRef::new("notes.txt").expect("resource"),
            },
        };
        store
            .append(&AppendRequest::new(
                fixture.turn.session_id().clone(),
                9,
                vec![
                    session_event(
                        10,
                        EventPayload::InputAdmitted {
                            input_id: InputId::new("input-proof-pending")
                                .expect("pending input ID"),
                            prompt: PromptText::new("Keep this queued input exact")
                                .expect("pending prompt"),
                            delivery_mode: DeliveryMode::NextTurn,
                        },
                    ),
                    session_event(
                        11,
                        EventPayload::ToolCallProposed {
                            attempt_id: fixture.turn.attempt_id().clone(),
                            call,
                        },
                    ),
                    session_event(
                        12,
                        EventPayload::ToolPermissionRecorded {
                            tool_call_id: tool_call_id.clone(),
                            decision_id: decision_id.clone(),
                            outcome: PermissionOutcome::Ask,
                        },
                    ),
                ],
            ))
            .expect("append pending input and permission");

        let pending_turn = turn_at_generation(
            "turn-proof-pending",
            fixture.epoch.epoch_id().as_str(),
            2,
            12,
            MemoryGeneration::INITIAL,
        );
        let pending_fixture = CompactionFixture {
            predecessor: fixture.predecessor.clone(),
            epoch: fixture.epoch.clone(),
            context: ContextTurnCommitRequest::new(
                Some(fixture.epoch.clone()),
                pending_turn.clone(),
                turn_content(&pending_turn),
            ),
            binding: session_event(
                13,
                EventPayload::ContextTurnBound {
                    attempt_id: pending_turn.attempt_id().clone(),
                    run_turn: pending_turn.run_turn(),
                    context_turn_id: pending_turn.context_turn_id().clone(),
                    manifest_hash: pending_turn.manifest_hash().clone(),
                },
            ),
            turn: pending_turn,
        };
        let before_answer = compaction_fingerprint(&mut store, &pending_fixture);
        assert_eq!(before_answer.memory_fact_count(), 0);
        assert_eq!(before_answer.pending_session_fact_count(), 3);

        store
            .append(&AppendRequest::new(
                pending_fixture.turn.session_id().clone(),
                12,
                vec![session_event(
                    13,
                    EventPayload::ToolPermissionAnswered {
                        tool_call_id,
                        decision_id,
                        answer: PermissionAnswer::AllowOnce,
                    },
                )],
            ))
            .expect("answer pending permission");
        let answered_turn = turn_at_generation(
            "turn-proof-answered",
            fixture.epoch.epoch_id().as_str(),
            2,
            13,
            MemoryGeneration::INITIAL,
        );
        let answered_fixture = CompactionFixture {
            predecessor: fixture.predecessor,
            epoch: fixture.epoch,
            context: ContextTurnCommitRequest::new(
                Some(pending_fixture.epoch.clone()),
                answered_turn.clone(),
                turn_content(&answered_turn),
            ),
            binding: session_event(
                14,
                EventPayload::ContextTurnBound {
                    attempt_id: answered_turn.attempt_id().clone(),
                    run_turn: answered_turn.run_turn(),
                    context_turn_id: answered_turn.context_turn_id().clone(),
                    manifest_hash: answered_turn.manifest_hash().clone(),
                },
            ),
            turn: answered_turn,
        };
        let stale_claim = compaction_boundary_with_claims(
            &answered_fixture,
            before_answer.hash().clone(),
            before_answer.memory_fact_count(),
            before_answer.pending_session_fact_count(),
        );
        assert_eq!(
            store.commit_context_turn_and_bind(
                &BoundContextTurnCommitRequest::new(
                    answered_fixture.context.clone(),
                    answered_fixture.binding.clone(),
                )
                .with_compaction_boundary(stale_claim),
            ),
            Err(StoreError::InvalidContextTransition)
        );

        let after_answer = compaction_fingerprint(&mut store, &answered_fixture);
        assert_eq!(after_answer.pending_session_fact_count(), 2);
        assert_ne!(after_answer.hash(), before_answer.hash());
        assert_eq!(
            store
                .commit_context_turn_and_bind(
                    &BoundContextTurnCommitRequest::new(
                        answered_fixture.context.clone(),
                        answered_fixture.binding.clone(),
                    )
                    .with_compaction_boundary(compaction_boundary(
                        &answered_fixture,
                        &after_answer,
                    )),
                )
                .expect("bind current pending-state proof")
                .disposition(),
            ContextCommitDisposition::Committed
        );
    }

    #[test]
    fn atomic_binding_freezes_workspace_and_persists_verified_compaction_boundary() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_dispatch_ready_attempt(&mut store);

        let first_epoch = epoch("epoch-bound", ContextEpochReason::NewAttempt, None);
        let first_turn = turn_at("turn-bound", "epoch-bound", 1, 5, "workspace-1");
        let first_context = ContextTurnCommitRequest::new(
            Some(first_epoch.clone()),
            first_turn.clone(),
            turn_content(&first_turn),
        );
        let first_binding = session_event(
            6,
            EventPayload::ContextTurnBound {
                attempt_id: first_turn.attempt_id().clone(),
                run_turn: 1,
                context_turn_id: first_turn.context_turn_id().clone(),
                manifest_hash: first_turn.manifest_hash().clone(),
            },
        );
        let first_request =
            BoundContextTurnCommitRequest::new(first_context, first_binding.clone());
        let receipt = store
            .commit_context_turn_and_bind(&first_request)
            .expect("atomically bind first turn");
        assert_eq!(receipt.disposition(), ContextCommitDisposition::Committed);
        assert_eq!(receipt.last_sequence(), 6);
        assert_eq!(
            store
                .commit_context_turn_and_bind(&first_request)
                .expect("retry atomic bind")
                .disposition(),
            ContextCommitDisposition::AlreadyCommitted
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT workspace_id FROM sessions WHERE session_id = 'session-context'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .expect("session workspace"),
            Some("workspace-1".to_owned())
        );

        let mismatched_turn = turn_at(
            "turn-workspace-mismatch",
            "epoch-bound",
            2,
            6,
            "workspace-2",
        );
        let mismatched_request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(
                None,
                mismatched_turn.clone(),
                turn_content(&mismatched_turn),
            ),
            session_event(
                7,
                EventPayload::ContextTurnBound {
                    attempt_id: mismatched_turn.attempt_id().clone(),
                    run_turn: 2,
                    context_turn_id: mismatched_turn.context_turn_id().clone(),
                    manifest_hash: mismatched_turn.manifest_hash().clone(),
                },
            ),
        );
        assert_eq!(
            store.commit_context_turn_and_bind(&mismatched_request),
            Err(StoreError::InvalidContextTransition)
        );
        assert!(
            store
                .load_context_turn(mismatched_turn.context_turn_id())
                .expect("load mismatched turn")
                .is_none()
        );

        store
            .append(&AppendRequest::new(
                SessionId::new("session-context").expect("session ID"),
                6,
                vec![
                    session_event(
                        7,
                        EventPayload::RunTurnStarted {
                            attempt_id: first_turn.attempt_id().clone(),
                            turn: 1,
                        },
                    ),
                    session_event(
                        8,
                        EventPayload::AttemptPausedForTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                    session_event(
                        9,
                        EventPayload::AttemptResumedAfterTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                ],
            ))
            .expect("advance to second dispatch boundary");

        let compacted_epoch = epoch(
            "epoch-compaction-bound",
            ContextEpochReason::Compaction,
            Some("epoch-bound"),
        );
        let compacted_turn = turn_at(
            "turn-compaction-bound",
            "epoch-compaction-bound",
            2,
            9,
            "workspace-1",
        );
        let compacted_context = ContextTurnCommitRequest::new(
            Some(compacted_epoch.clone()),
            compacted_turn.clone(),
            turn_content(&compacted_turn),
        );
        let compacted_binding = session_event(
            10,
            EventPayload::ContextTurnBound {
                attempt_id: compacted_turn.attempt_id().clone(),
                run_turn: 2,
                context_turn_id: compacted_turn.context_turn_id().clone(),
                manifest_hash: compacted_turn.manifest_hash().clone(),
            },
        );
        let missing_boundary = BoundContextTurnCommitRequest::new(
            compacted_context.clone(),
            compacted_binding.clone(),
        );
        assert_eq!(
            store.commit_context_turn_and_bind(&missing_boundary),
            Err(StoreError::InvalidContextTransition)
        );
        assert!(
            store
                .load_context_turn(compacted_turn.context_turn_id())
                .expect("load rolled-back compacted turn")
                .is_none()
        );

        let fingerprint = {
            let transaction = store
                .connection
                .transaction()
                .expect("open proof transaction");
            let fingerprint =
                compute_compaction_fingerprint(&transaction, &compacted_epoch, &compacted_turn)
                    .expect("compute independent compaction proof");
            transaction.rollback().expect("roll back proof transaction");
            fingerprint
        };
        let boundary = ContextCompactionBoundary::new(
            compacted_epoch.epoch_id().clone(),
            first_epoch.epoch_id().clone(),
            compacted_turn.session_id().clone(),
            compacted_turn.expected_session_sequence(),
            compacted_turn.memory_generation(),
            COMPACTION_FACTS_VERSION,
            fingerprint.hash().clone(),
            fingerprint.memory_fact_count(),
            fingerprint.pending_session_fact_count(),
            None,
            TimestampMillis::new(9),
        );
        let compacted_request =
            BoundContextTurnCommitRequest::new(compacted_context, compacted_binding)
                .with_compaction_boundary(boundary.clone());
        assert_eq!(
            store
                .commit_context_turn_and_bind(&compacted_request)
                .expect("commit compacted context")
                .last_sequence(),
            10
        );
        assert_eq!(
            store
                .load_compaction_boundary(compacted_epoch.epoch_id())
                .expect("load compaction boundary"),
            Some(boundary.clone())
        );
        assert_eq!(
            store
                .commit_context_turn_and_bind(&compacted_request)
                .expect("retry compacted context")
                .disposition(),
            ContextCommitDisposition::AlreadyCommitted
        );
        store
            .rebuild_projections()
            .expect("rebuild session projections around retained context audit rows");
        assert_eq!(
            store
                .load_context_turn(compacted_turn.context_turn_id())
                .expect("load compacted turn after rebuild"),
            Some(compacted_turn)
        );
        assert_eq!(
            store
                .load_compaction_boundary(compacted_epoch.epoch_id())
                .expect("load boundary after rebuild"),
            Some(boundary)
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM context_turn_bindings", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("binding count after rebuild"),
            2
        );
    }

    #[test]
    fn frozen_epoch_continuation_survives_retraction_but_not_content_deletion() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_dispatch_ready_attempt(&mut store);
        let (memory_id, revision) = seed_expiring_active_memory(&mut store);
        let epoch = epoch_at(
            "epoch-frozen-memory",
            ContextEpochReason::NewAttempt,
            None,
            MemoryGeneration::new(1).expect("generation"),
            4,
        );
        let (first_turn, first_content) = memory_turn(
            "turn-frozen-memory-1",
            "epoch-frozen-memory",
            1,
            5,
            20,
            &memory_id,
            &revision,
        );
        let first_request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(Some(epoch.clone()), first_turn.clone(), first_content),
            session_event(
                6,
                EventPayload::ContextTurnBound {
                    attempt_id: first_turn.attempt_id().clone(),
                    run_turn: 1,
                    context_turn_id: first_turn.context_turn_id().clone(),
                    manifest_hash: first_turn.manifest_hash().clone(),
                },
            ),
        );
        store
            .commit_context_turn_and_bind(&first_request)
            .expect("bind memory after its wall-clock expiry but inside epoch eligibility");

        store
            .append(&AppendRequest::new(
                SessionId::new("session-context").expect("session ID"),
                6,
                vec![
                    session_event(
                        7,
                        EventPayload::RunTurnStarted {
                            attempt_id: first_turn.attempt_id().clone(),
                            turn: 1,
                        },
                    ),
                    session_event(
                        8,
                        EventPayload::AttemptPausedForTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                    session_event(
                        9,
                        EventPayload::AttemptResumedAfterTools {
                            attempt_id: first_turn.attempt_id().clone(),
                        },
                    ),
                ],
            ))
            .expect("advance frozen attempt");

        let retract = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-expiring-retract").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::new(2).expect("memory sequence"),
            TimestampMillis::new(25),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new("command-expiring-retract").expect("command ID"),
            ),
            CorrelationId::new("correlation-expiring-retract").expect("correlation ID"),
            MemoryOperationPayload::MemoryRetracted {
                revision_id: revision.revision_id().clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, retract, None))
            .expect("retract memory outside frozen epoch");
        assert_eq!(store.memory_generation().expect("generation").get(), 2);

        let (second_turn, second_content) = memory_turn(
            "turn-frozen-memory-2",
            "epoch-frozen-memory",
            2,
            9,
            30,
            &memory_id,
            &revision,
        );
        let second_request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(None, second_turn.clone(), second_content),
            session_event(
                10,
                EventPayload::ContextTurnBound {
                    attempt_id: second_turn.attempt_id().clone(),
                    run_turn: 2,
                    context_turn_id: second_turn.context_turn_id().clone(),
                    manifest_hash: second_turn.manifest_hash().clone(),
                },
            ),
        );
        store
            .commit_context_turn_and_bind(&second_request)
            .expect("continue exact frozen baseline after retraction");

        let history = store
            .load_memory_admissions(
                &MemoryAdmissionQuery::new(MemoryAdmissionKey::Memory(memory_id.clone()), None, 8)
                    .expect("history query"),
            )
            .expect("load admission history");
        assert_eq!(history.len(), 2);
        assert!(
            history
                .iter()
                .all(|record| record.rendered_content_available())
        );

        let delete = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-expiring-delete").expect("operation ID"),
            memory_id.clone(),
            MemorySequence::new(3).expect("memory sequence"),
            TimestampMillis::new(35),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new("command-expiring-delete").expect("command ID"),
            ),
            CorrelationId::new("correlation-expiring-delete").expect("correlation ID"),
            MemoryOperationPayload::MemoryDeleted {
                revision_id: revision.revision_id().clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(2, delete, None))
            .expect("delete memory and frozen sidecars");
        let erased_history = store
            .load_memory_admissions(
                &MemoryAdmissionQuery::new(
                    MemoryAdmissionKey::Revision(revision.revision_id().clone()),
                    None,
                    8,
                )
                .expect("erased history query"),
            )
            .expect("load erased admission history");
        assert_eq!(erased_history.len(), 2);
        assert!(
            erased_history
                .iter()
                .all(|record| !record.rendered_content_available())
        );
        assert!(
            store
                .load_context_turn_content(first_turn.context_turn_id())
                .expect("load erased first prelude")
                .is_none()
        );

        let (third_turn, third_content) = memory_turn(
            "turn-frozen-memory-3",
            "epoch-frozen-memory",
            3,
            10,
            40,
            &memory_id,
            &revision,
        );
        assert_eq!(
            store.commit_context_turn(&ContextTurnCommitRequest::new(
                None,
                third_turn.clone(),
                third_content,
            )),
            Err(StoreError::InvalidContextTransition)
        );
        assert!(
            store
                .load_context_turn(third_turn.context_turn_id())
                .expect("load rejected third turn")
                .is_none()
        );
    }

    #[test]
    fn first_turn_admits_an_approved_proposal_using_its_projected_active_state() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_dispatch_ready_attempt(&mut store);
        let (memory_id, revision) = seed_approved_proposal_memory(&mut store);
        let epoch = epoch_at(
            "epoch-approved-proposal",
            ContextEpochReason::NewAttempt,
            None,
            MemoryGeneration::new(1).expect("generation"),
            20,
        );
        let (turn, content) = memory_turn(
            "turn-approved-proposal",
            "epoch-approved-proposal",
            1,
            5,
            20,
            &memory_id,
            &revision,
        );
        let request = BoundContextTurnCommitRequest::new(
            ContextTurnCommitRequest::new(Some(epoch), turn.clone(), content),
            session_event(
                6,
                EventPayload::ContextTurnBound {
                    attempt_id: turn.attempt_id().clone(),
                    run_turn: 1,
                    context_turn_id: turn.context_turn_id().clone(),
                    manifest_hash: turn.manifest_hash().clone(),
                },
            ),
        );

        assert_eq!(
            store
                .commit_context_turn_and_bind(&request)
                .expect("bind approved proposal")
                .disposition(),
            ContextCommitDisposition::Committed
        );
    }

    #[test]
    fn compaction_epoch_links_predecessor_and_failed_generation_writes_nothing() {
        let database = TestDatabase::new();
        let mut store = database.open();
        seed_attempt(&store);
        let first_epoch = epoch("epoch-first", ContextEpochReason::NewAttempt, None);
        let first_turn = turn("turn-first", "epoch-first", 1);
        store
            .commit_context_turn(&ContextTurnCommitRequest::new(
                Some(first_epoch),
                first_turn.clone(),
                turn_content(&first_turn),
            ))
            .expect("first context");

        let compacted = epoch(
            "epoch-compacted",
            ContextEpochReason::Compaction,
            Some("epoch-first"),
        );
        let compacted_turn = turn("turn-compacted", "epoch-compacted", 2);
        store
            .commit_context_turn(&ContextTurnCommitRequest::new(
                Some(compacted.clone()),
                compacted_turn.clone(),
                turn_content(&compacted_turn),
            ))
            .expect("compacted context");
        assert_eq!(
            store
                .load_context_epoch(compacted.epoch_id())
                .expect("load compacted epoch"),
            Some(compacted)
        );

        store
            .connection
            .execute(
                "UPDATE memory_store_state SET generation = 1 WHERE singleton = 1",
                [],
            )
            .expect("advance generation");
        let stale_epoch = epoch("epoch-stale", ContextEpochReason::Recovery, None);
        let stale_turn = turn("turn-stale", "epoch-stale", 3);
        assert_eq!(
            store.commit_context_turn(&ContextTurnCommitRequest::new(
                Some(stale_epoch),
                stale_turn.clone(),
                turn_content(&stale_turn),
            )),
            Err(StoreError::ContextGenerationConflict {
                expected: 0,
                actual: 1,
            })
        );
        let stale_count = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM context_epochs WHERE epoch_id = 'epoch-stale'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("stale epoch count");
        assert_eq!(stale_count, 0);
    }
}
