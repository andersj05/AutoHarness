use std::collections::{BTreeMap, BTreeSet};

use autoharness_domain::{
    AgentId, ContextAdmission, ContextTurnManifest, MEMORY_SCHEMA_V1, MemoryCausation,
    MemoryContent, MemoryEvidenceSource, MemoryGeneration, MemoryId, MemoryKind,
    MemoryOperationEnvelope, MemoryOperationPayload, MemoryOrigin, MemoryRevision,
    MemoryRevisionId, MemoryRevisionStatus, MemoryScope, MemoryValidationStatus, MemoryValidity,
    Sensitivity, SessionId, TrustClass, UserId, WorkspaceId,
};
use autoharness_memory::{
    normalized_content_hash, verify_admission_rendered_hash, verify_context_manifest_hash,
};
use autoharness_store::{
    ActiveMemoryHead, ActiveMemoryHeadPageQuery, ActiveMemoryHeadQuery, CorruptionArea,
    IdentityKind, MAX_MEMORY_SEARCH_CANDIDATES, MemoryAdmissionKey, MemoryAdmissionQuery,
    MemoryAdmissionRecord, MemoryAppendBatchRequest, MemoryAppendDisposition,
    MemoryAppendOperation, MemoryAppendReceipt, MemoryAppendRequest, MemoryCandidateBatch,
    MemoryContentState, MemoryInspectionQuery, MemoryInspectionRecord, MemoryMutationGeneration,
    MemorySearchCandidate, MemorySearchQuery, MemoryStore, StoreError, StoredMemoryCandidate,
};
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter};
use sha2::{Digest, Sha256};

use crate::sqlite_store::{SqliteStore, map_sqlite_error, to_sql_sequence};

const MAX_MEMORY_OPERATION_BYTES: usize = 1024 * 1024;
const MAX_FTS_QUERY_TERMS: usize = 32;

impl MemoryStore for SqliteStore {
    fn append_memory(
        &mut self,
        request: &MemoryAppendRequest,
    ) -> Result<MemoryAppendReceipt, StoreError> {
        let batch = MemoryAppendBatchRequest::new(
            request.expected_last_sequence(),
            vec![MemoryAppendOperation::new(
                request.operation().clone(),
                request.content().cloned(),
            )],
        );
        self.append_memory_batch(&batch)
    }

    fn append_memory_batch(
        &mut self,
        request: &MemoryAppendBatchRequest,
    ) -> Result<MemoryAppendReceipt, StoreError> {
        let encoded = encode_and_validate_batch(request)?;
        let first = request
            .operations()
            .first()
            .ok_or(StoreError::EmptyAppend)?;
        let last = request.operations().last().ok_or(StoreError::EmptyAppend)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;

        let mut existing_count = 0_usize;
        for (entry, encoded) in request.operations().iter().zip(&encoded) {
            if let Some(existing_json) = transaction
                .query_row(
                    "SELECT envelope_json FROM memory_operations WHERE operation_id = ?1",
                    params![entry.operation().operation_id().as_str()],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(map_sqlite_error)?
            {
                if existing_json != encoded.envelope_json {
                    return Err(StoreError::IdentityConflict {
                        kind: IdentityKind::MemoryOperation,
                    });
                }
                existing_count += 1;
            }
        }
        if existing_count == request.operations().len() {
            for entry in request.operations() {
                validate_existing_memory_sidecar(&transaction, entry)?;
            }
            let last_sequence =
                current_memory_sequence(&transaction, first.operation().memory_id().as_str())?;
            let generation = read_generation(&transaction)?;
            let mutation_generation = read_mutation_generation(&transaction)?;
            transaction.commit().map_err(map_sqlite_error)?;
            return Ok(MemoryAppendReceipt::new(
                MemoryAppendDisposition::AlreadyCommitted,
                last_sequence,
                generation,
                mutation_generation,
            ));
        }
        if existing_count != 0 {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::MemoryOperation,
            });
        }

        let actual_sequence =
            current_memory_sequence(&transaction, first.operation().memory_id().as_str())?;
        if actual_sequence != request.expected_last_sequence() {
            return Err(StoreError::MemoryVersionConflict {
                memory_id: first.operation().memory_id().clone(),
                expected: request.expected_last_sequence(),
                actual: actual_sequence,
            });
        }

        let active_before =
            current_active_revision(&transaction, first.operation().memory_id().as_str())?;
        let creates_item = matches!(
            first.operation().payload(),
            MemoryOperationPayload::MemoryCreated { .. }
        );
        if (actual_sequence == 0) != creates_item {
            return Err(StoreError::InvalidMemoryTransition);
        }
        if creates_item {
            insert_memory_item_shell(&transaction, first.operation())?;
        }

        for (entry, encoded) in request.operations().iter().zip(&encoded) {
            validate_memory_causation(&transaction, entry.operation())?;
            insert_operation(&transaction, entry.operation(), encoded)?;
            let single = MemoryAppendRequest::new(
                entry.operation().sequence().get().saturating_sub(1),
                entry.operation().clone(),
                entry.content().cloned(),
            );
            apply_operation(&transaction, &single, false)?;
            transaction
                .execute(
                    "UPDATE memory_items SET last_sequence = ?2, updated_at_ms = ?3 \
                     WHERE memory_id = ?1",
                    params![
                        entry.operation().memory_id().as_str(),
                        to_sql_sequence(entry.operation().sequence().get())?,
                        entry.operation().occurred_at().get(),
                    ],
                )
                .map_err(map_sqlite_error)?;
        }
        let active_after =
            current_active_revision(&transaction, first.operation().memory_id().as_str())?;
        let generation = if active_before != active_after {
            increment_generation(&transaction, last.operation().occurred_at().get())?
        } else {
            read_generation(&transaction)?
        };
        let mutation_generation =
            increment_mutation_generation(&transaction, last.operation().occurred_at().get())?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(MemoryAppendReceipt::new(
            MemoryAppendDisposition::Committed,
            last.operation().sequence().get(),
            generation,
            mutation_generation,
        ))
    }

    fn load_memory_operations(
        &self,
        memory_id: &autoharness_domain::MemoryId,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<MemoryOperationEnvelope>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if limit > autoharness_store::DEFAULT_MEMORY_PAGE_SIZE {
            return Err(StoreError::LimitExceeded);
        }
        let after_sequence = to_sql_sequence(after_sequence)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT sequence, envelope_json, operation_sha256 \
                 FROM memory_operations WHERE memory_id = ?1 AND sequence > ?2 \
                 ORDER BY sequence ASC LIMIT ?3",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(
                params![memory_id.as_str(), after_sequence, i64::from(limit)],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .map_err(map_sqlite_error)?;
        let mut operations = Vec::new();
        let mut expected = u64::try_from(after_sequence)
            .map_err(|_| StoreError::SequenceOutOfRange)?
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
        for row in rows {
            let (sequence, json, hash) = row.map_err(map_sqlite_error)?;
            let sequence = u64::try_from(sequence).map_err(|_| corrupt_memory_ledger())?;
            if sequence != expected || Sha256::digest(&json).as_slice() != hash.as_slice() {
                return Err(corrupt_memory_ledger());
            }
            let operation: MemoryOperationEnvelope =
                serde_json::from_slice(&json).map_err(|_| corrupt_memory_ledger())?;
            if operation.memory_id() != memory_id || operation.sequence().get() != sequence {
                return Err(corrupt_memory_ledger());
            }
            operations.push(operation);
            expected = expected
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
        }
        Ok(operations)
    }

    fn load_memory_revisions(
        &self,
        memory_id: &autoharness_domain::MemoryId,
    ) -> Result<Vec<MemoryRevision>, StoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT state, metadata_json, metadata_sha256 \
                 FROM memory_revisions WHERE memory_id = ?1 ORDER BY revision ASC",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![memory_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| {
            let (state, json, hash) = row.map_err(map_sqlite_error)?;
            decode_projected_revision(&state, &json, &hash)
        })
        .collect()
    }

    fn load_memory_content(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<MemoryContent>, StoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT b.content_utf8, b.content_sha256, r.metadata_json, r.metadata_sha256 \
                 FROM memory_revisions AS r \
                 JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
                 WHERE r.revision_id = ?1",
                params![revision_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((content, content_hash, metadata_json, metadata_hash)) = row else {
            return Ok(None);
        };
        if Sha256::digest(&content).as_slice() != content_hash.as_slice()
            || Sha256::digest(&metadata_json).as_slice() != metadata_hash.as_slice()
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            });
        }
        let metadata: MemoryRevision =
            serde_json::from_slice(&metadata_json).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            })?;
        let content_text = String::from_utf8(content).map_err(|_| StoreError::CorruptData {
            area: CorruptionArea::MemoryProjection,
        })?;
        if normalized_content_hash(&content_text).map_err(|_| corrupt_memory_projection())?
            != *metadata.content_hash()
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            });
        }
        MemoryContent::new(content_text)
            .map(Some)
            .map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            })
    }

    fn load_memory_candidate(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<StoredMemoryCandidate>, StoreError> {
        let row = self
            .connection
            .query_row(
                "SELECT i.memory_id, i.scope_type, i.scope_id, i.kind, r.state, \
                        r.metadata_json, r.metadata_sha256, r.content_hash_sha256, \
                        r.content_id, b.content_utf8, b.content_sha256 \
                 FROM memory_revisions AS r \
                 JOIN memory_items AS i ON i.memory_id = r.memory_id \
                 LEFT JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
                 WHERE r.revision_id = ?1",
                params![revision_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                        row.get::<_, Vec<u8>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<Vec<u8>>>(9)?,
                        row.get::<_, Option<Vec<u8>>>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((
            memory_id,
            scope_type,
            scope_id,
            kind,
            state,
            metadata_json,
            metadata_hash,
            indexed_content_hash,
            content_id,
            content,
            raw_content_hash,
        )) = row
        else {
            return Ok(None);
        };
        let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
        if revision.revision_id() != revision_id
            || digest_bytes(revision.content_hash().as_str())?.as_slice()
                != indexed_content_hash.as_slice()
        {
            return Err(corrupt_memory_projection());
        }
        let content = match (content_id, content, raw_content_hash) {
            (None, None, None) => MemoryContentState::Erased,
            (Some(_), Some(content), Some(raw_hash))
                if Sha256::digest(&content).as_slice() == raw_hash.as_slice() =>
            {
                let content =
                    String::from_utf8(content).map_err(|_| corrupt_memory_projection())?;
                if normalized_content_hash(&content).map_err(|_| corrupt_memory_projection())?
                    != *revision.content_hash()
                {
                    return Err(corrupt_memory_projection());
                }
                MemoryContentState::Retained(
                    MemoryContent::new(content).map_err(|_| corrupt_memory_projection())?,
                )
            }
            _ => return Err(corrupt_memory_projection()),
        };
        Ok(Some(StoredMemoryCandidate::new(
            MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
            decode_scope(&scope_type, scope_id)?,
            decode_memory_kind(&kind)?,
            revision,
            content,
        )))
    }

    fn inspect_memories(
        &self,
        query: &MemoryInspectionQuery,
    ) -> Result<Vec<MemoryInspectionRecord>, StoreError> {
        let mut sql = String::from(
            "SELECT i.memory_id, i.scope_type, i.scope_id, i.kind, i.lifecycle, \
                    i.active_revision_id, i.last_sequence, i.created_at_ms, i.updated_at_ms, \
                    r.state, r.metadata_json, r.metadata_sha256, b.content_utf8, b.content_sha256 \
             FROM memory_items AS i \
             JOIN memory_revisions AS r ON r.revision_id = i.latest_revision_id \
             LEFT JOIN memory_content_blobs AS b ON b.content_id = r.content_id WHERE (",
        );
        let mut values = Vec::new();
        for (index, scope) in query.eligible_scopes().iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            let parameter = values.len() + 1;
            sql.push_str(&format!(
                "(i.scope_type = ?{parameter} AND i.scope_id = ?{})",
                parameter + 1
            ));
            let (scope_type, scope_id) = encode_scope(scope);
            values.push(Value::Text(scope_type.to_owned()));
            values.push(Value::Text(scope_id.to_owned()));
        }
        sql.push(')');
        if !query.statuses().is_empty() {
            sql.push_str(" AND i.lifecycle IN (");
            for (index, status) in query.statuses().iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                values.push(Value::Text(encode_revision_status(*status).to_owned()));
                sql.push('?');
                sql.push_str(&values.len().to_string());
            }
            sql.push(')');
        }
        if let Some(memory_kind) = query.memory_kind() {
            values.push(Value::Text(encode_memory_kind(memory_kind).to_owned()));
            sql.push_str(" AND i.kind = ?");
            sql.push_str(&values.len().to_string());
        }
        if let Some(subject_key) = query.subject_key() {
            values.push(Value::Text(subject_key.as_str().to_owned()));
            sql.push_str(" AND r.subject_key = ?");
            sql.push_str(&values.len().to_string());
        }
        if let Some(before) = query.before() {
            let time_parameter = values.len() + 1;
            values.push(Value::Integer(before.updated_at().get()));
            let id_parameter = values.len() + 1;
            values.push(Value::Text(before.memory_id().as_str().to_owned()));
            sql.push_str(&format!(
                " AND (i.updated_at_ms < ?{time_parameter} OR \
                 (i.updated_at_ms = ?{time_parameter} AND i.memory_id < ?{id_parameter}))"
            ));
        }
        sql.push_str(" ORDER BY i.updated_at_ms DESC, i.memory_id DESC LIMIT ?");
        sql.push_str(&(values.len() + 1).to_string());
        values.push(Value::Integer(i64::from(query.limit())));

        let mut statement = self.connection.prepare(&sql).map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Vec<u8>>(10)?,
                    row.get::<_, Vec<u8>>(11)?,
                    row.get::<_, Option<Vec<u8>>>(12)?,
                    row.get::<_, Option<Vec<u8>>>(13)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| {
            let (
                memory_id,
                scope_type,
                scope_id,
                memory_kind,
                lifecycle,
                active_revision_id,
                last_sequence,
                created_at_ms,
                updated_at_ms,
                revision_state,
                metadata_json,
                metadata_hash,
                content,
                content_hash,
            ) = row.map_err(map_sqlite_error)?;
            let latest_revision =
                decode_projected_revision(&revision_state, &metadata_json, &metadata_hash)?;
            let content = decode_optional_content(&latest_revision, content, content_hash)?;
            let lifecycle = decode_revision_status(&lifecycle)?;
            if lifecycle != MemoryRevisionStatus::Deleted && content.is_none() {
                return Err(corrupt_memory_projection());
            }
            Ok(MemoryInspectionRecord::new(
                MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
                decode_scope(&scope_type, scope_id)?,
                decode_memory_kind(&memory_kind)?,
                lifecycle,
                latest_revision,
                content,
                active_revision_id
                    .map(MemoryRevisionId::new)
                    .transpose()
                    .map_err(|_| corrupt_memory_projection())?,
                u64::try_from(last_sequence).map_err(|_| corrupt_memory_projection())?,
                autoharness_domain::TimestampMillis::new(created_at_ms),
                autoharness_domain::TimestampMillis::new(updated_at_ms),
            ))
        })
        .collect()
    }

    fn load_memory_admissions(
        &self,
        query: &MemoryAdmissionQuery,
    ) -> Result<Vec<MemoryAdmissionRecord>, StoreError> {
        let mut values = Vec::new();
        let key_predicate = match query.key() {
            MemoryAdmissionKey::Memory(memory_id) => {
                values.push(Value::Text(memory_id.as_str().to_owned()));
                "r.memory_id = ?1"
            }
            MemoryAdmissionKey::Revision(revision_id) => {
                values.push(Value::Text(revision_id.as_str().to_owned()));
                "a.memory_revision_id = ?1"
            }
        };
        let mut sql = format!(
            "SELECT a.admission_json, a.admission_json_sha256, a.rendered_state, \
                    t.manifest_json, t.manifest_json_sha256, a.rendered_utf8, \
                    a.rendered_content_sha256, r.memory_id \
             FROM context_admissions AS a \
             JOIN context_turns AS t ON t.context_turn_id = a.context_turn_id \
             JOIN memory_revisions AS r ON r.revision_id = a.memory_revision_id \
             WHERE {key_predicate}"
        );
        if let Some(before) = query.before() {
            let time_parameter = values.len() + 1;
            values.push(Value::Integer(before.admitted_at().get()));
            let id_parameter = values.len() + 1;
            values.push(Value::Text(before.admission_id().as_str().to_owned()));
            sql.push_str(&format!(
                " AND (a.admitted_at_ms < ?{time_parameter} OR \
                 (a.admitted_at_ms = ?{time_parameter} AND a.admission_id < ?{id_parameter}))"
            ));
        }
        sql.push_str(" ORDER BY a.admitted_at_ms DESC, a.admission_id DESC LIMIT ?");
        sql.push_str(&(values.len() + 1).to_string());
        values.push(Value::Integer(i64::from(query.limit())));
        let mut statement = self.connection.prepare(&sql).map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                    row.get::<_, Option<Vec<u8>>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| {
            let (
                admission_json,
                admission_hash,
                rendered_state,
                turn_json,
                turn_hash,
                rendered_bytes,
                rendered_content_hash,
                memory_id,
            ) = row.map_err(map_sqlite_error)?;
            let admission: ContextAdmission = decode_json(
                &admission_json,
                &admission_hash,
                CorruptionArea::ContextLedger,
            )?;
            let turn: ContextTurnManifest =
                decode_json(&turn_json, &turn_hash, CorruptionArea::ContextLedger)?;
            if !verify_context_manifest_hash(&turn).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::ContextLedger,
            })? {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::ContextLedger,
                });
            }
            let revision_id = admission
                .memory_revision_id()
                .cloned()
                .ok_or_else(corrupt_memory_projection)?;
            if admission.context_turn_id() != turn.context_turn_id() {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::ContextLedger,
                });
            }
            let rendered_content_available = match (
                rendered_state.as_str(),
                rendered_bytes,
                rendered_content_hash,
            ) {
                ("erased", None, None) => false,
                ("retained", Some(bytes), Some(hash))
                    if Sha256::digest(&bytes).as_slice() == hash.as_slice() =>
                {
                    let rendered =
                        String::from_utf8(bytes).map_err(|_| StoreError::CorruptData {
                            area: CorruptionArea::ContextLedger,
                        })?;
                    let memory_id =
                        MemoryId::new(memory_id).map_err(|_| StoreError::CorruptData {
                            area: CorruptionArea::ContextLedger,
                        })?;
                    if !verify_admission_rendered_hash(&admission, Some(&memory_id), &rendered)
                        .map_err(|_| StoreError::CorruptData {
                            area: CorruptionArea::ContextLedger,
                        })?
                    {
                        return Err(StoreError::CorruptData {
                            area: CorruptionArea::ContextLedger,
                        });
                    }
                    true
                }
                _ => {
                    return Err(StoreError::CorruptData {
                        area: CorruptionArea::ContextLedger,
                    });
                }
            };
            Ok(MemoryAdmissionRecord::new(
                admission.admission_id().clone(),
                revision_id,
                turn.context_turn_id().clone(),
                turn.epoch_id().clone(),
                turn.session_id().clone(),
                turn.attempt_id().clone(),
                turn.run_turn(),
                turn.model().clone(),
                admission.admitted_at(),
                admission.rank(),
                admission.rank_score(),
                admission.token_count(),
                admission.renderer_version(),
                admission.reasons().to_vec(),
                rendered_content_available,
            ))
        })
        .collect()
    }

    fn load_active_memory_heads(
        &self,
        query: &ActiveMemoryHeadQuery,
    ) -> Result<Vec<ActiveMemoryHead>, StoreError> {
        let mut sql = String::from(
            "SELECT i.memory_id, i.scope_type, i.scope_id, i.kind, \
                    r.state, r.metadata_json, r.metadata_sha256, r.content_hash_sha256 \
             FROM memory_items AS i \
             JOIN memory_revisions AS r ON r.revision_id = i.active_revision_id \
             WHERE i.lifecycle = 'active' AND r.state = 'active' AND i.kind = ?1 AND (",
        );
        let mut values = vec![Value::Text(
            encode_memory_kind(query.memory_kind()).to_owned(),
        )];
        for (index, scope) in query.eligible_scopes().iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            let parameter = values.len() + 1;
            sql.push_str(&format!(
                "(i.scope_type = ?{parameter} AND i.scope_id = ?{})",
                parameter + 1
            ));
            let (scope_type, scope_id) = encode_scope(scope);
            values.push(Value::Text(scope_type.to_owned()));
            values.push(Value::Text(scope_id.to_owned()));
        }
        sql.push(')');
        match query.subject_key() {
            Some(subject_key) => {
                values.push(Value::Text(subject_key.as_str().to_owned()));
                sql.push_str(" AND r.subject_key = ?");
                sql.push_str(&values.len().to_string());
            }
            None => sql.push_str(" AND r.subject_key IS NULL"),
        }
        if let Some(content_hash) = query.content_hash() {
            values.push(Value::Blob(digest_bytes(content_hash.as_str())?.to_vec()));
            sql.push_str(" AND r.content_hash_sha256 = ?");
            sql.push_str(&values.len().to_string());
        }
        sql.push_str(" ORDER BY i.scope_type ASC, i.scope_id ASC, i.memory_id ASC LIMIT ?");
        sql.push_str(&(values.len() + 1).to_string());
        values.push(Value::Integer(i64::from(query.limit())));

        let mut statement = self.connection.prepare(&sql).map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| {
            let (
                memory_id,
                scope_type,
                scope_id,
                kind,
                state,
                metadata_json,
                metadata_hash,
                content_hash,
            ) = row.map_err(map_sqlite_error)?;
            let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
            if revision.status() != MemoryRevisionStatus::Active
                || digest_bytes(revision.content_hash().as_str())?.as_slice()
                    != content_hash.as_slice()
            {
                return Err(corrupt_memory_projection());
            }
            Ok(ActiveMemoryHead::new(
                MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
                decode_scope(&scope_type, scope_id)?,
                decode_memory_kind(&kind)?,
                revision,
            ))
        })
        .collect()
    }

    fn page_active_memory_heads(
        &self,
        query: &ActiveMemoryHeadPageQuery,
    ) -> Result<Vec<ActiveMemoryHead>, StoreError> {
        let mut sql = String::from(
            "SELECT i.memory_id, i.scope_type, i.scope_id, i.kind, \
                    r.state, r.metadata_json, r.metadata_sha256 \
             FROM memory_items AS i \
             JOIN memory_revisions AS r ON r.revision_id = i.active_revision_id \
             WHERE i.lifecycle = 'active' AND r.state = 'active' AND (",
        );
        let mut values = Vec::new();
        for (index, scope) in query.eligible_scopes().iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            let parameter = values.len() + 1;
            sql.push_str(&format!(
                "(i.scope_type = ?{parameter} AND i.scope_id = ?{})",
                parameter + 1
            ));
            let (scope_type, scope_id) = encode_scope(scope);
            values.push(Value::Text(scope_type.to_owned()));
            values.push(Value::Text(scope_id.to_owned()));
        }
        sql.push(')');
        if let Some(after) = query.after() {
            values.push(Value::Text(after.memory_id().as_str().to_owned()));
            sql.push_str(" AND i.memory_id > ?");
            sql.push_str(&values.len().to_string());
        }
        sql.push_str(" ORDER BY i.memory_id ASC LIMIT ?");
        sql.push_str(&(values.len() + 1).to_string());
        values.push(Value::Integer(i64::from(query.limit())));

        let mut statement = self.connection.prepare(&sql).map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.map(|row| {
            let (memory_id, scope_type, scope_id, kind, state, metadata_json, metadata_hash) =
                row.map_err(map_sqlite_error)?;
            let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
            if revision.status() != MemoryRevisionStatus::Active {
                return Err(corrupt_memory_projection());
            }
            Ok(ActiveMemoryHead::new(
                MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
                decode_scope(&scope_type, scope_id)?,
                decode_memory_kind(&kind)?,
                revision,
            ))
        })
        .collect()
    }

    fn search_memory(&self, query: &MemorySearchQuery) -> Result<MemoryCandidateBatch, StoreError> {
        let generation = self.memory_generation()?;
        let Some(match_query) = literal_fts_query(query.query().as_str()) else {
            return Ok(MemoryCandidateBatch::new(generation, Vec::new()));
        };

        let mut sql = String::from(
            "SELECT r.state, r.metadata_json, r.metadata_sha256, \
                    b.content_utf8, b.content_sha256, i.memory_id, i.scope_type, i.scope_id, i.kind \
             FROM memory_revision_fts AS f \
             JOIN memory_revisions AS r ON r.search_rowid = f.rowid \
             JOIN memory_items AS i ON i.memory_id = r.memory_id \
             JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE memory_revision_fts MATCH ?1 AND r.state = 'active' \
               AND (r.valid_from_ms IS NULL OR r.valid_from_ms <= ?2) \
               AND (r.valid_until_ms IS NULL OR r.valid_until_ms > ?2) \
               AND CASE r.sensitivity \
                    WHEN 'public' THEN 0 WHEN 'internal' THEN 1 \
                    WHEN 'sensitive' THEN 2 WHEN 'secret' THEN 3 ELSE 4 END <= ?3 AND (",
        );
        let mut values = vec![
            Value::Text(match_query),
            Value::Integer(query.as_of().get()),
            Value::Integer(sensitivity_rank(query.sensitivity_ceiling())),
        ];
        for (index, scope) in query.eligible_scopes().iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            let parameter = values.len() + 1;
            sql.push_str(&format!(
                "(i.scope_type = ?{parameter} AND i.scope_id = ?{})",
                parameter + 1
            ));
            let (scope_type, scope_id) = encode_scope(scope);
            values.push(Value::Text(scope_type.to_owned()));
            values.push(Value::Text(scope_id.to_owned()));
        }
        sql.push_str(
            ") ORDER BY bm25(memory_revision_fts) ASC, i.memory_id ASC, r.revision ASC LIMIT ?",
        );
        sql.push_str(&(values.len() + 1).to_string());
        values.push(Value::Integer(i64::from(query.limit())));

        let mut statement = self.connection.prepare(&sql).map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        let mut candidates = Vec::new();
        for (rank, row) in rows.enumerate() {
            let (
                state,
                metadata_json,
                metadata_hash,
                content,
                content_hash,
                memory_id,
                scope_type,
                scope_id,
                memory_kind,
            ) = row.map_err(map_sqlite_error)?;
            let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
            if Sha256::digest(&content).as_slice() != content_hash.as_slice() {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::MemorySearch,
                });
            }
            let content_text = String::from_utf8(content).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::MemorySearch,
            })?;
            if normalized_content_hash(&content_text).map_err(|_| StoreError::CorruptData {
                area: CorruptionArea::MemorySearch,
            })? != *revision.content_hash()
            {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::MemorySearch,
                });
            }
            let content =
                MemoryContent::new(content_text).map_err(|_| StoreError::CorruptData {
                    area: CorruptionArea::MemorySearch,
                })?;
            let rank = u32::try_from(rank).map_err(|_| StoreError::LimitExceeded)?;
            candidates.push(MemorySearchCandidate::new(
                MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
                decode_scope(&scope_type, scope_id)?,
                decode_memory_kind(&memory_kind)?,
                revision,
                content,
                rank,
            ));
        }
        if candidates.len()
            > usize::try_from(MAX_MEMORY_SEARCH_CANDIDATES)
                .map_err(|_| StoreError::LimitExceeded)?
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::MemorySearch,
            });
        }
        Ok(MemoryCandidateBatch::new(generation, candidates))
    }

    fn memory_generation(&self) -> Result<MemoryGeneration, StoreError> {
        read_generation(&self.connection)
    }

    fn memory_mutation_generation(&self) -> Result<MemoryMutationGeneration, StoreError> {
        read_mutation_generation(&self.connection)
    }

    fn rebuild_memory_projections(&mut self) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite_error)?;
        transaction
            .execute("DELETE FROM memory_revision_fts", [])
            .map_err(map_sqlite_error)?;

        let revisions = load_revision_introduction_states(&transaction)?;
        for (revision_id, state) in revisions {
            transaction
                .execute(
                    "UPDATE memory_revisions SET state = ?2 WHERE revision_id = ?1",
                    params![revision_id, encode_revision_status(state)],
                )
                .map_err(map_sqlite_error)?;
        }
        transaction
            .execute(
                "UPDATE memory_items SET lifecycle = 'proposed', last_sequence = 0, \
                 latest_revision = 0, latest_revision_id = NULL, active_revision_id = NULL",
                [],
            )
            .map_err(map_sqlite_error)?;

        let operations = load_all_operations(&transaction)?;
        for operation in &operations {
            apply_rebuild_operation(&transaction, operation)?;
            transaction
                .execute(
                    "UPDATE memory_items SET last_sequence = ?2, updated_at_ms = ?3 \
                     WHERE memory_id = ?1",
                    params![
                        operation.memory_id().as_str(),
                        to_sql_sequence(operation.sequence().get())?,
                        operation.occurred_at().get(),
                    ],
                )
                .map_err(map_sqlite_error)?;
        }
        apply_session_erasure_tombstones(&transaction)?;
        rebuild_fts(&transaction)?;
        let incomplete = transaction
            .query_row(
                "SELECT COUNT(*) FROM memory_items WHERE last_sequence = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_sqlite_error)?;
        if incomplete != 0 {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection,
            });
        }
        transaction.commit().map_err(map_sqlite_error)
    }
}

pub(crate) fn erase_session_memory_and_evidence(
    transaction: &Transaction<'_>,
    session_id: &SessionId,
    session_last_sequence: u64,
    erased_at_ms: i64,
) -> Result<(), StoreError> {
    let session_memories = {
        let mut statement = transaction
            .prepare(
                "SELECT memory_id, latest_revision_id, lifecycle = 'active' FROM memory_items \
                 WHERE scope_type = 'session' AND scope_id = ?1",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![session_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?
    };
    let eligibility_changed = session_memories
        .iter()
        .any(|(_, _, was_active)| *was_active);
    let mut mutation_changed = !session_memories.is_empty();
    for (memory_id, latest_revision_id, _) in session_memories {
        let latest_revision_id = latest_revision_id
            .ok_or_else(corrupt_memory_projection)
            .and_then(|id| MemoryRevisionId::new(id).map_err(|_| corrupt_memory_projection()))?;
        delete_memory_content(transaction, &memory_id, &latest_revision_id)?;
        transaction
            .execute(
                "UPDATE memory_revisions SET state = 'deleted' WHERE memory_id = ?1",
                params![memory_id],
            )
            .map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO memory_session_erasure_tombstones (\
                    memory_id, session_id, session_last_sequence, erased_at_ms\
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    memory_id,
                    session_id.as_str(),
                    to_sql_sequence(session_last_sequence)?,
                    erased_at_ms
                ],
            )
            .map_err(map_sqlite_error)?;
    }

    let cross_scope_evidence = {
        let mut statement = transaction
            .prepare(
                "SELECT e.revision_id, e.ordinal, e.source_json, e.excerpt_content_id \
                 FROM memory_evidence AS e \
                 JOIN memory_revisions AS r ON r.revision_id = e.revision_id \
                 JOIN memory_items AS i ON i.memory_id = r.memory_id \
                 WHERE e.excerpt_content_id IS NOT NULL \
                   AND NOT (i.scope_type = 'session' AND i.scope_id = ?1)",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![session_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(map_sqlite_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?
    };
    for (revision_id, ordinal, source_json, content_id) in cross_scope_evidence {
        let source: MemoryEvidenceSource =
            serde_json::from_slice(&source_json).map_err(|_| corrupt_memory_projection())?;
        if !evidence_belongs_to_session(&source, session_id) {
            continue;
        }
        let changed = transaction
            .execute(
                "UPDATE memory_evidence SET excerpt_content_id = NULL, excerpt_sha256 = NULL, \
                    erased_by_session_id = ?3, erased_at_ms = ?4 \
                 WHERE revision_id = ?1 AND ordinal = ?2 AND excerpt_content_id = ?5",
                params![
                    revision_id,
                    ordinal,
                    session_id.as_str(),
                    erased_at_ms,
                    content_id
                ],
            )
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(corrupt_memory_projection());
        }
        transaction
            .execute(
                "DELETE FROM memory_content_blobs WHERE content_id = ?1",
                params![content_id],
            )
            .map_err(map_sqlite_error)?;
        mutation_changed = true;
    }
    if mutation_changed {
        transaction
            .execute(
                "UPDATE memory_store_state SET \
                    generation = generation + ?1, \
                    mutation_generation = mutation_generation + 1, updated_at_ms = ?2 \
                 WHERE singleton = 1",
                params![i64::from(eligibility_changed), erased_at_ms],
            )
            .map_err(map_sqlite_error)?;
    }
    Ok(())
}

fn evidence_belongs_to_session(source: &MemoryEvidenceSource, session_id: &SessionId) -> bool {
    match source {
        MemoryEvidenceSource::UserInput {
            session_id: source_session,
            ..
        }
        | MemoryEvidenceSource::ToolObservation {
            session_id: source_session,
            ..
        }
        | MemoryEvidenceSource::SessionEvent {
            session_id: source_session,
            ..
        } => source_session == session_id,
        MemoryEvidenceSource::ImportedDocument { .. }
        | MemoryEvidenceSource::MemoryRevision { .. } => false,
    }
}

fn apply_session_erasure_tombstones(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE memory_revisions SET state = 'deleted' WHERE memory_id IN (\
                SELECT memory_id FROM memory_session_erasure_tombstones\
             )",
            [],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "UPDATE memory_items SET lifecycle = 'deleted', active_revision_id = NULL \
             WHERE memory_id IN (SELECT memory_id FROM memory_session_erasure_tombstones)",
            [],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

struct EncodedMemoryOperation {
    envelope_json: Vec<u8>,
    operation_hash: [u8; 32],
}

fn encode_and_validate_batch(
    request: &MemoryAppendBatchRequest,
) -> Result<Vec<EncodedMemoryOperation>, StoreError> {
    let first = request
        .operations()
        .first()
        .ok_or(StoreError::EmptyAppend)?;
    let mut expected = request
        .expected_last_sequence()
        .checked_add(1)
        .ok_or(StoreError::SequenceOutOfRange)?;
    let mut operation_ids = BTreeSet::new();
    let mut preceding_operation_ids = BTreeSet::new();
    let mut command_ids = BTreeSet::new();
    let mut encoded = Vec::with_capacity(request.operations().len());
    for (index, entry) in request.operations().iter().enumerate() {
        if entry.operation().memory_id() != first.operation().memory_id()
            || entry.operation().correlation_id() != first.operation().correlation_id()
            || entry.operation().sequence().get() != expected
            || (index > 0
                && matches!(
                    entry.operation().payload(),
                    MemoryOperationPayload::MemoryCreated { .. }
                ))
            || !operation_ids.insert(entry.operation().operation_id().clone())
        {
            return Err(StoreError::NonContiguousBatch);
        }
        if index > 0
            && !matches!(
                entry.operation().causation(),
                MemoryCausation::Operation(cause_id) if preceding_operation_ids.contains(cause_id)
            )
        {
            return Err(StoreError::InvalidCausation);
        }
        if let MemoryCausation::Command(command_id) = entry.operation().causation()
            && !command_ids.insert(command_id.clone())
        {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::Command,
            });
        }
        let single = MemoryAppendRequest::new(
            expected.saturating_sub(1),
            entry.operation().clone(),
            entry.content().cloned(),
        );
        encoded.push(encode_and_validate(&single)?);
        preceding_operation_ids.insert(entry.operation().operation_id().clone());
        expected = expected
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
    }
    Ok(encoded)
}

fn encode_and_validate(
    request: &MemoryAppendRequest,
) -> Result<EncodedMemoryOperation, StoreError> {
    let operation = request.operation();
    if operation.schema_version() != MEMORY_SCHEMA_V1 {
        return Err(StoreError::UnsupportedMemorySchema {
            found: operation.schema_version(),
        });
    }
    if operation.sequence().get() > i64::MAX as u64
        || request.expected_last_sequence() > i64::MAX as u64
    {
        return Err(StoreError::SequenceOutOfRange);
    }
    validate_content_sidecar(request)?;
    let envelope_json = serde_json::to_vec(operation).map_err(|_| StoreError::Backend)?;
    if envelope_json.len() > MAX_MEMORY_OPERATION_BYTES {
        return Err(StoreError::LimitExceeded);
    }
    let operation_hash = Sha256::digest(&envelope_json).into();
    Ok(EncodedMemoryOperation {
        envelope_json,
        operation_hash,
    })
}

fn validate_content_sidecar(request: &MemoryAppendRequest) -> Result<(), StoreError> {
    let metadata = introduced_revision(request.operation().payload());
    match (metadata, request.content()) {
        (None, None) => Ok(()),
        (None, Some(_)) | (Some(_), None) => Err(StoreError::InvalidMemoryTransition),
        (Some(metadata), Some(content)) => {
            if metadata.revision_id() != content.revision_id()
                || metadata.sensitivity() == Sensitivity::Secret
                || normalized_content_hash(content.content().as_str())
                    .map_err(|_| StoreError::InvalidMemoryTransition)?
                    != *metadata.content_hash()
            {
                return Err(StoreError::InvalidMemoryTransition);
            }

            let mut sidecars = BTreeMap::new();
            for evidence in content.evidence() {
                if sidecars
                    .insert(evidence.evidence_id().as_str(), evidence.excerpt())
                    .is_some()
                {
                    return Err(StoreError::IdentityConflict {
                        kind: IdentityKind::MemoryRevision,
                    });
                }
            }
            for evidence in metadata.evidence() {
                match (
                    evidence.excerpt_hash(),
                    sidecars.remove(evidence.evidence_id().as_str()),
                ) {
                    (None, None) => {}
                    (Some(hash), Some(excerpt))
                        if Sha256::digest(excerpt.as_str().as_bytes()).as_slice()
                            == digest_bytes(hash.as_str())?.as_slice() => {}
                    _ => return Err(StoreError::InvalidMemoryTransition),
                }
            }
            if !sidecars.is_empty() {
                return Err(StoreError::InvalidMemoryTransition);
            }
            Ok(())
        }
    }
}

fn validate_existing_memory_sidecar(
    transaction: &Transaction<'_>,
    entry: &MemoryAppendOperation,
) -> Result<(), StoreError> {
    let Some(revision) = introduced_revision(entry.operation().payload()) else {
        return if entry.content().is_none() {
            Ok(())
        } else {
            Err(StoreError::IdentityConflict {
                kind: IdentityKind::MemoryRevision,
            })
        };
    };
    let content = entry.content().ok_or(StoreError::InvalidMemoryTransition)?;
    let row = transaction
        .query_row(
            "SELECT r.content_id, b.content_utf8, b.content_sha256, i.lifecycle \
             FROM memory_revisions AS r \
             JOIN memory_items AS i ON i.memory_id = r.memory_id \
             LEFT JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE r.revision_id = ?1",
            params![revision.revision_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(map_sqlite_error)?
        .ok_or_else(corrupt_memory_projection)?;
    let deleted = match row {
        (None, None, None, lifecycle) if lifecycle == "deleted" => true,
        (Some(_), Some(bytes), Some(hash), _)
            if Sha256::digest(&bytes).as_slice() == hash.as_slice()
                && bytes.as_slice() == content.content().as_str().as_bytes() =>
        {
            false
        }
        (Some(_), Some(_), Some(_), _) => {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::MemoryRevision,
            });
        }
        _ => return Err(corrupt_memory_projection()),
    };
    if deleted {
        return Ok(());
    }

    let expected = content
        .evidence()
        .iter()
        .map(|evidence| (evidence.evidence_id().as_str(), evidence.excerpt().as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut statement = transaction
        .prepare(
            "SELECT e.evidence_id, b.content_utf8, b.content_sha256 \
             FROM memory_evidence AS e \
             JOIN memory_content_blobs AS b ON b.content_id = e.excerpt_content_id \
             WHERE e.revision_id = ?1 ORDER BY e.ordinal",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(params![revision.revision_id().as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    let mut found = 0_usize;
    for row in rows {
        let (evidence_id, bytes, hash) = row.map_err(map_sqlite_error)?;
        if Sha256::digest(&bytes).as_slice() != hash.as_slice()
            || expected
                .get(evidence_id.as_str())
                .map(|value| value.as_bytes())
                != Some(bytes.as_slice())
        {
            return Err(StoreError::IdentityConflict {
                kind: IdentityKind::MemoryRevision,
            });
        }
        found += 1;
    }
    if found != expected.len() {
        return Err(StoreError::IdentityConflict {
            kind: IdentityKind::MemoryRevision,
        });
    }
    Ok(())
}

fn current_memory_sequence(
    connection: &rusqlite::Connection,
    memory_id: &str,
) -> Result<u64, StoreError> {
    let sequence = connection
        .query_row(
            "SELECT last_sequence FROM memory_items WHERE memory_id = ?1",
            params![memory_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .unwrap_or(0);
    u64::try_from(sequence).map_err(|_| corrupt_memory_projection())
}

fn current_active_revision(
    connection: &rusqlite::Connection,
    memory_id: &str,
) -> Result<Option<String>, StoreError> {
    connection
        .query_row(
            "SELECT active_revision_id FROM memory_items WHERE memory_id = ?1",
            params![memory_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(map_sqlite_error)
        .map(Option::flatten)
}

fn read_generation(connection: &rusqlite::Connection) -> Result<MemoryGeneration, StoreError> {
    let generation = connection
        .query_row(
            "SELECT generation FROM memory_store_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    let generation = u64::try_from(generation).map_err(|_| corrupt_memory_projection())?;
    MemoryGeneration::new(generation).map_err(|_| corrupt_memory_projection())
}

fn read_mutation_generation(
    connection: &rusqlite::Connection,
) -> Result<MemoryMutationGeneration, StoreError> {
    let generation = connection
        .query_row(
            "SELECT mutation_generation FROM memory_store_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    let generation = u64::try_from(generation).map_err(|_| corrupt_memory_projection())?;
    Ok(MemoryMutationGeneration::new(generation))
}

fn increment_generation(
    transaction: &Transaction<'_>,
    updated_at_ms: i64,
) -> Result<MemoryGeneration, StoreError> {
    let current = read_generation(transaction)?;
    let next = current
        .checked_next()
        .ok_or(StoreError::SequenceOutOfRange)?;
    let changed = transaction
        .execute(
            "UPDATE memory_store_state SET generation = ?1, updated_at_ms = ?2 \
             WHERE singleton = 1 AND generation = ?3",
            params![
                to_sql_sequence(next.get())?,
                updated_at_ms,
                to_sql_sequence(current.get())?
            ],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(corrupt_memory_projection());
    }
    Ok(next)
}

fn increment_mutation_generation(
    transaction: &Transaction<'_>,
    updated_at_ms: i64,
) -> Result<MemoryMutationGeneration, StoreError> {
    let current = read_mutation_generation(transaction)?;
    let next = current
        .get()
        .checked_add(1)
        .ok_or(StoreError::SequenceOutOfRange)?;
    let changed = transaction
        .execute(
            "UPDATE memory_store_state SET mutation_generation = ?1, updated_at_ms = ?2 \
             WHERE singleton = 1 AND mutation_generation = ?3",
            params![
                to_sql_sequence(next)?,
                updated_at_ms,
                to_sql_sequence(current.get())?
            ],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(corrupt_memory_projection());
    }
    Ok(MemoryMutationGeneration::new(next))
}

fn insert_memory_item_shell(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
) -> Result<(), StoreError> {
    let MemoryOperationPayload::MemoryCreated {
        scope,
        memory_kind,
        revision,
    } = operation.payload()
    else {
        return Err(StoreError::InvalidMemoryTransition);
    };
    if revision.revision().get() != 1
        || !matches!(
            revision.status(),
            MemoryRevisionStatus::Proposed | MemoryRevisionStatus::Active
        )
    {
        return Err(StoreError::InvalidMemoryTransition);
    }
    let (scope_type, scope_id) = encode_scope(scope);
    transaction
        .execute(
            "INSERT INTO memory_items (memory_id, scope_type, scope_id, kind, lifecycle, \
             last_sequence, latest_revision, latest_revision_id, active_revision_id, \
             created_at_ms, updated_at_ms) \
             VALUES (?1, ?2, ?3, ?4, 'proposed', 0, 0, NULL, NULL, ?5, ?5)",
            params![
                operation.memory_id().as_str(),
                scope_type,
                scope_id,
                encode_memory_kind(*memory_kind),
                operation.occurred_at().get(),
            ],
        )
        .map_err(|error| map_identity_error(error, IdentityKind::Memory))?;
    Ok(())
}

fn validate_memory_causation(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
) -> Result<(), StoreError> {
    if let MemoryCausation::Operation(cause_id) = operation.causation() {
        let cause = transaction
            .query_row(
                "SELECT memory_id, sequence FROM memory_operations WHERE operation_id = ?1",
                params![cause_id.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((memory_id, sequence)) = cause else {
            return Err(StoreError::InvalidCausation);
        };
        if memory_id != operation.memory_id().as_str()
            || sequence >= to_sql_sequence(operation.sequence().get())?
        {
            return Err(StoreError::InvalidCausation);
        }
    }
    Ok(())
}

fn insert_operation(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
    encoded: &EncodedMemoryOperation,
) -> Result<(), StoreError> {
    let (command_id, cause_operation_id) = match operation.causation() {
        MemoryCausation::Command(command_id) => (Some(command_id.as_str()), None),
        MemoryCausation::Operation(operation_id) => (None, Some(operation_id.as_str())),
    };
    transaction
        .execute(
            "INSERT INTO memory_operations (operation_id, memory_id, sequence, schema_version, \
             operation_kind, revision_id, caused_by_command_id, caused_by_operation_id, \
             correlation_id, occurred_at_ms, envelope_json, operation_sha256) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                operation.operation_id().as_str(),
                operation.memory_id().as_str(),
                to_sql_sequence(operation.sequence().get())?,
                i64::from(operation.schema_version()),
                operation_kind(operation.payload()),
                payload_revision_id(operation.payload()),
                command_id,
                cause_operation_id,
                operation.correlation_id().as_str(),
                operation.occurred_at().get(),
                &encoded.envelope_json,
                encoded.operation_hash.as_slice(),
            ],
        )
        .map_err(|error| map_identity_error(error, IdentityKind::MemoryOperation))?;
    Ok(())
}

fn apply_operation(
    transaction: &Transaction<'_>,
    request: &MemoryAppendRequest,
    rebuilding: bool,
) -> Result<(), StoreError> {
    let operation = request.operation();
    match operation.payload() {
        MemoryOperationPayload::MemoryCreated { revision, .. }
        | MemoryOperationPayload::RevisionProposed { revision }
        | MemoryOperationPayload::MemoryRevised { revision } => {
            if rebuilding {
                apply_revision_projection(
                    transaction,
                    operation.memory_id().as_str(),
                    revision,
                    false,
                )?;
            } else {
                insert_revision(transaction, operation, revision, request.content())?;
            }
        }
        MemoryOperationPayload::ProposalApproved {
            proposal_revision_id,
            approved_revision,
        } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                proposal_revision_id,
                &[MemoryRevisionStatus::Proposed],
            )?;
            if rebuilding {
                apply_revision_projection(
                    transaction,
                    operation.memory_id().as_str(),
                    approved_revision,
                    false,
                )?;
            } else {
                insert_revision(transaction, operation, approved_revision, request.content())?;
            }
            set_revision_state(
                transaction,
                proposal_revision_id,
                MemoryRevisionStatus::Superseded,
            )?;
        }
        MemoryOperationPayload::RevisionValidated {
            revision_id,
            validation,
        } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[MemoryRevisionStatus::Proposed, MemoryRevisionStatus::Active],
            )?;
            let (metadata_json, metadata_hash) = transaction
                .query_row(
                    "SELECT metadata_json, metadata_sha256 FROM memory_revisions \
                     WHERE revision_id = ?1",
                    params![revision_id.as_str()],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .map_err(map_sqlite_error)?;
            if Sha256::digest(&metadata_json).as_slice() != metadata_hash.as_slice() {
                return Err(corrupt_memory_projection());
            }
            let metadata: MemoryRevision =
                serde_json::from_slice(&metadata_json).map_err(|_| corrupt_memory_projection())?;
            if metadata.content_hash() != validation.content_hash() {
                return Err(StoreError::InvalidMemoryTransition);
            }
            if !rebuilding {
                let json = serde_json::to_vec(validation).map_err(|_| StoreError::Backend)?;
                transaction
                    .execute(
                        "INSERT INTO memory_validations (operation_id, revision_id, \
                         validator_version, content_sha256, outcome, validation_json, created_at_ms) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            operation.operation_id().as_str(),
                            revision_id.as_str(),
                            i64::from(validation.validator_version()),
                            digest_bytes(validation.content_hash().as_str())?.as_slice(),
                            encode_validation_status(validation.status()),
                            json,
                            operation.occurred_at().get(),
                        ],
                    )
                    .map_err(map_sqlite_error)?;
            }
        }
        MemoryOperationPayload::RevisionActivated { revision_id } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[MemoryRevisionStatus::Proposed],
            )?;
            activate_revision(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                !rebuilding,
            )?;
        }
        MemoryOperationPayload::RevisionSuperseded {
            revision_id,
            superseded_by_revision_id,
        } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[
                    MemoryRevisionStatus::Active,
                    MemoryRevisionStatus::Superseded,
                ],
            )?;
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                superseded_by_revision_id,
                &[MemoryRevisionStatus::Active],
            )?;
            set_revision_state(transaction, revision_id, MemoryRevisionStatus::Superseded)?;
            remove_revision_from_fts(transaction, revision_id)?;
        }
        MemoryOperationPayload::RevisionRejected { revision_id, .. } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[MemoryRevisionStatus::Proposed],
            )?;
            set_revision_state(transaction, revision_id, MemoryRevisionStatus::Rejected)?;
            update_item_from_revision(transaction, operation.memory_id().as_str(), revision_id)?;
        }
        MemoryOperationPayload::MemoryRetracted { revision_id } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[MemoryRevisionStatus::Active],
            )?;
            set_revision_state(transaction, revision_id, MemoryRevisionStatus::Retracted)?;
            remove_revision_from_fts(transaction, revision_id)?;
            transaction
                .execute(
                    "UPDATE memory_items SET lifecycle = 'retracted', active_revision_id = NULL \
                     WHERE memory_id = ?1",
                    params![operation.memory_id().as_str()],
                )
                .map_err(map_sqlite_error)?;
        }
        MemoryOperationPayload::MemoryDeleted { revision_id } => {
            require_revision_state(
                transaction,
                operation.memory_id().as_str(),
                revision_id,
                &[
                    MemoryRevisionStatus::Proposed,
                    MemoryRevisionStatus::Active,
                    MemoryRevisionStatus::Rejected,
                    MemoryRevisionStatus::Retracted,
                ],
            )?;
            delete_memory_content(transaction, operation.memory_id().as_str(), revision_id)?;
        }
    }
    Ok(())
}

fn insert_revision(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
    revision: &MemoryRevision,
    content: Option<&autoharness_store::MemoryRevisionContent>,
) -> Result<(), StoreError> {
    let content = content.ok_or(StoreError::InvalidMemoryTransition)?;
    let latest = transaction
        .query_row(
            "SELECT latest_revision FROM memory_items WHERE memory_id = ?1",
            params![operation.memory_id().as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    if revision.revision().get()
        != u64::try_from(latest)
            .map_err(|_| corrupt_memory_projection())?
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?
    {
        return Err(StoreError::InvalidMemoryTransition);
    }
    let content_id = format!("memory-content:{}", revision.revision_id().as_str());
    transaction
        .execute(
            "INSERT INTO memory_content_blobs (content_id, media_type, content_utf8, \
             content_sha256, created_at_ms) VALUES (?1, 'text/plain; charset=utf-8', ?2, ?3, ?4)",
            params![
                content_id,
                content.content().as_str().as_bytes(),
                Sha256::digest(content.content().as_str().as_bytes()).as_slice(),
                revision.created_at().get(),
            ],
        )
        .map_err(|error| map_identity_error(error, IdentityKind::MemoryRevision))?;
    let metadata_json = serde_json::to_vec(revision).map_err(|_| StoreError::Backend)?;
    let metadata_hash = Sha256::digest(&metadata_json);
    let (valid_from, valid_until) = validity_bounds(revision.validity());
    transaction
        .execute(
            "INSERT INTO memory_revisions (revision_id, memory_id, revision, subject_key, \
             introduced_operation_id, state, content_id, content_hash_sha256, metadata_json, metadata_sha256, \
             origin, trust_class, confidence_basis_points, sensitivity, valid_from_ms, \
             valid_until_ms, created_at_ms, supersedes_revision_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                     ?16, ?17, ?18)",
            params![
                revision.revision_id().as_str(),
                operation.memory_id().as_str(),
                to_sql_sequence(revision.revision().get())?,
                revision.subject_key().map(|key| key.as_str()),
                operation.operation_id().as_str(),
                encode_revision_status(revision.status()),
                content_id,
                digest_bytes(revision.content_hash().as_str())?.as_slice(),
                metadata_json,
                metadata_hash.as_slice(),
                encode_origin(revision.origin()),
                encode_trust(revision.trust_class()),
                i64::from(revision.confidence().get()),
                encode_sensitivity(revision.sensitivity()),
                valid_from,
                valid_until,
                revision.created_at().get(),
                revision.supersedes_revision_id().map(|id| id.as_str()),
            ],
        )
        .map_err(|error| map_identity_error(error, IdentityKind::MemoryRevision))?;

    let evidence_sidecars = content
        .evidence()
        .iter()
        .map(|evidence| (evidence.evidence_id().as_str(), evidence.excerpt()))
        .collect::<BTreeMap<_, _>>();
    for (ordinal, evidence) in revision.evidence().iter().enumerate() {
        let excerpt_content_id =
            if let Some(excerpt) = evidence_sidecars.get(evidence.evidence_id().as_str()) {
                let excerpt_id = format!("memory-evidence:{}", evidence.evidence_id().as_str());
                transaction
                    .execute(
                        "INSERT INTO memory_content_blobs (content_id, media_type, content_utf8, \
                     content_sha256, created_at_ms) \
                     VALUES (?1, 'text/plain; charset=utf-8', ?2, ?3, ?4)",
                        params![
                            excerpt_id,
                            excerpt.as_str().as_bytes(),
                            digest_bytes(
                                evidence
                                    .excerpt_hash()
                                    .ok_or(StoreError::InvalidMemoryTransition)?
                                    .as_str()
                            )?
                            .as_slice(),
                            revision.created_at().get(),
                        ],
                    )
                    .map_err(map_sqlite_error)?;
                Some(excerpt_id)
            } else {
                None
            };
        let source_json = serde_json::to_vec(evidence.source()).map_err(|_| StoreError::Backend)?;
        transaction
            .execute(
                "INSERT INTO memory_evidence (revision_id, ordinal, evidence_id, source_json, \
                 relation, excerpt_content_id, excerpt_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    revision.revision_id().as_str(),
                    i64::try_from(ordinal).map_err(|_| StoreError::LimitExceeded)?,
                    evidence.evidence_id().as_str(),
                    source_json,
                    encode_evidence_relation(evidence.relation()),
                    excerpt_content_id,
                    evidence
                        .excerpt_hash()
                        .map(|hash| digest_bytes(hash.as_str()))
                        .transpose()?,
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    for (ordinal, relation) in revision.relations().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO memory_relations (revision_id, ordinal, to_memory_id, relation) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    revision.revision_id().as_str(),
                    i64::try_from(ordinal).map_err(|_| StoreError::LimitExceeded)?,
                    relation.memory_id().as_str(),
                    encode_relation_kind(relation.kind()),
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    apply_revision_projection(transaction, operation.memory_id().as_str(), revision, true)
}

fn apply_revision_projection(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision: &MemoryRevision,
    maintain_fts: bool,
) -> Result<(), StoreError> {
    if revision.status() == MemoryRevisionStatus::Active {
        deactivate_current_revision(transaction, memory_id, Some(revision.revision_id()))?;
        if maintain_fts {
            add_revision_to_fts(transaction, revision.revision_id())?;
        }
    }
    if revision.status() == MemoryRevisionStatus::Active {
        transaction
            .execute(
                "UPDATE memory_items SET latest_revision = ?2, latest_revision_id = ?3, \
                 lifecycle = 'active', active_revision_id = ?3 WHERE memory_id = ?1",
                params![
                    memory_id,
                    to_sql_sequence(revision.revision().get())?,
                    revision.revision_id().as_str(),
                ],
            )
            .map_err(map_sqlite_error)?;
    } else {
        transaction
            .execute(
                "UPDATE memory_items SET latest_revision = ?2, latest_revision_id = ?3, \
                 lifecycle = CASE WHEN active_revision_id IS NULL THEN ?4 ELSE 'active' END \
                 WHERE memory_id = ?1",
                params![
                    memory_id,
                    to_sql_sequence(revision.revision().get())?,
                    revision.revision_id().as_str(),
                    encode_revision_status(revision.status()),
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    Ok(())
}

fn activate_revision(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision_id: &MemoryRevisionId,
    maintain_fts: bool,
) -> Result<(), StoreError> {
    deactivate_current_revision(transaction, memory_id, Some(revision_id))?;
    set_revision_state(transaction, revision_id, MemoryRevisionStatus::Active)?;
    if maintain_fts {
        add_revision_to_fts(transaction, revision_id)?;
    }
    transaction
        .execute(
            "UPDATE memory_items SET lifecycle = 'active', active_revision_id = ?2 \
             WHERE memory_id = ?1",
            params![memory_id, revision_id.as_str()],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn deactivate_current_revision(
    transaction: &Transaction<'_>,
    memory_id: &str,
    except: Option<&MemoryRevisionId>,
) -> Result<(), StoreError> {
    let current = transaction
        .query_row(
            "SELECT active_revision_id FROM memory_items WHERE memory_id = ?1",
            params![memory_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(map_sqlite_error)?;
    if let Some(current) = current
        && except.is_none_or(|except| except.as_str() != current)
    {
        transaction
            .execute(
                "UPDATE memory_revisions SET state = 'superseded' WHERE revision_id = ?1",
                params![current],
            )
            .map_err(map_sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM memory_revision_fts WHERE revision_id = ?1",
                params![current],
            )
            .map_err(map_sqlite_error)?;
    }
    Ok(())
}

fn require_revision_state(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision_id: &MemoryRevisionId,
    allowed: &[MemoryRevisionStatus],
) -> Result<(), StoreError> {
    let state = transaction
        .query_row(
            "SELECT state FROM memory_revisions WHERE memory_id = ?1 AND revision_id = ?2",
            params![memory_id, revision_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .ok_or(StoreError::InvalidMemoryTransition)?;
    let state = decode_revision_status(&state)?;
    if !allowed.contains(&state) {
        return Err(StoreError::InvalidMemoryTransition);
    }
    Ok(())
}

fn set_revision_state(
    transaction: &Transaction<'_>,
    revision_id: &MemoryRevisionId,
    state: MemoryRevisionStatus,
) -> Result<(), StoreError> {
    let changed = transaction
        .execute(
            "UPDATE memory_revisions SET state = ?2 WHERE revision_id = ?1",
            params![revision_id.as_str(), encode_revision_status(state)],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(corrupt_memory_projection());
    }
    Ok(())
}

fn update_item_from_revision(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision_id: &MemoryRevisionId,
) -> Result<(), StoreError> {
    let state = transaction
        .query_row(
            "SELECT state FROM memory_revisions WHERE revision_id = ?1",
            params![revision_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "UPDATE memory_items SET \
             lifecycle = CASE WHEN active_revision_id IS NULL THEN ?2 ELSE 'active' END \
             WHERE memory_id = ?1",
            params![memory_id, state],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn delete_memory_content(
    transaction: &Transaction<'_>,
    memory_id: &str,
    revision_id: &MemoryRevisionId,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE context_turns SET rendered_state = 'erased', rendered_utf8 = NULL, \
             rendered_content_sha256 = NULL WHERE context_turn_id IN ( \
                 SELECT DISTINCT a.context_turn_id FROM context_admissions AS a \
                 JOIN memory_revisions AS r ON r.revision_id = a.memory_revision_id \
                 WHERE r.memory_id = ?1 \
             )",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "UPDATE context_admissions SET rendered_state = 'erased', rendered_utf8 = NULL, \
             rendered_content_sha256 = NULL WHERE memory_revision_id IN ( \
                 SELECT revision_id FROM memory_revisions WHERE memory_id = ?1 \
             )",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "DELETE FROM memory_revision_fts WHERE memory_id = ?1",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    let content_ids = {
        let mut statement = transaction
            .prepare(
                "SELECT content_id FROM memory_revisions \
                 WHERE memory_id = ?1 AND content_id IS NOT NULL \
                 UNION ALL \
                 SELECT e.excerpt_content_id FROM memory_evidence AS e \
                 JOIN memory_revisions AS r ON r.revision_id = e.revision_id \
                 WHERE r.memory_id = ?1 AND e.excerpt_content_id IS NOT NULL",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![memory_id], |row| row.get::<_, String>(0))
            .map_err(map_sqlite_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?
    };
    transaction
        .execute(
            "UPDATE memory_evidence SET excerpt_content_id = NULL, excerpt_sha256 = NULL \
             WHERE revision_id IN \
             (SELECT revision_id FROM memory_revisions WHERE memory_id = ?1)",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    transaction
        .execute(
            "UPDATE memory_revisions SET content_id = NULL WHERE memory_id = ?1",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    for content_id in content_ids {
        transaction
            .execute(
                "DELETE FROM memory_content_blobs WHERE content_id = ?1",
                params![content_id],
            )
            .map_err(map_sqlite_error)?;
    }
    set_revision_state(transaction, revision_id, MemoryRevisionStatus::Deleted)?;
    transaction
        .execute(
            "UPDATE memory_items SET lifecycle = 'deleted', active_revision_id = NULL \
             WHERE memory_id = ?1",
            params![memory_id],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn add_revision_to_fts(
    transaction: &Transaction<'_>,
    revision_id: &MemoryRevisionId,
) -> Result<(), StoreError> {
    let changed = transaction
        .execute(
            "INSERT INTO memory_revision_fts (rowid, content, revision_id, memory_id) \
             SELECT r.search_rowid, CAST(b.content_utf8 AS TEXT), r.revision_id, r.memory_id \
             FROM memory_revisions AS r JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE r.revision_id = ?1",
            params![revision_id.as_str()],
        )
        .map_err(map_sqlite_error)?;
    if changed != 1 {
        return Err(StoreError::InvalidMemoryTransition);
    }
    Ok(())
}

fn remove_revision_from_fts(
    transaction: &Transaction<'_>,
    revision_id: &MemoryRevisionId,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "DELETE FROM memory_revision_fts WHERE revision_id = ?1",
            params![revision_id.as_str()],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn rebuild_fts(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO memory_revision_fts (rowid, content, revision_id, memory_id) \
             SELECT r.search_rowid, CAST(b.content_utf8 AS TEXT), r.revision_id, r.memory_id \
             FROM memory_revisions AS r JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE r.state = 'active' ORDER BY r.memory_id, r.revision",
            [],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn load_all_operations(
    transaction: &Transaction<'_>,
) -> Result<Vec<MemoryOperationEnvelope>, StoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT envelope_json, operation_sha256 FROM memory_operations \
             ORDER BY memory_id, sequence",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(map_sqlite_error)?;
    rows.map(|row| {
        let (json, hash) = row.map_err(map_sqlite_error)?;
        if Sha256::digest(&json).as_slice() != hash.as_slice() {
            return Err(corrupt_memory_ledger());
        }
        serde_json::from_slice(&json).map_err(|_| corrupt_memory_ledger())
    })
    .collect()
}

fn load_revision_introduction_states(
    transaction: &Transaction<'_>,
) -> Result<Vec<(String, MemoryRevisionStatus)>, StoreError> {
    let mut statement = transaction
        .prepare("SELECT revision_id, metadata_json, metadata_sha256 FROM memory_revisions")
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    rows.map(|row| {
        let (id, json, hash) = row.map_err(map_sqlite_error)?;
        if Sha256::digest(&json).as_slice() != hash.as_slice() {
            return Err(corrupt_memory_projection());
        }
        let revision: MemoryRevision =
            serde_json::from_slice(&json).map_err(|_| corrupt_memory_projection())?;
        if revision.revision_id().as_str() != id {
            return Err(corrupt_memory_projection());
        }
        Ok((id, revision.status()))
    })
    .collect()
}

fn apply_rebuild_operation(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
) -> Result<(), StoreError> {
    let request = MemoryAppendRequest::new(
        operation.sequence().get().saturating_sub(1),
        operation.clone(),
        None,
    );
    apply_operation(transaction, &request, true)
}

fn decode_projected_revision(
    state: &str,
    json: &[u8],
    hash: &[u8],
) -> Result<MemoryRevision, StoreError> {
    if Sha256::digest(json).as_slice() != hash {
        return Err(corrupt_memory_projection());
    }
    let metadata: MemoryRevision =
        serde_json::from_slice(json).map_err(|_| corrupt_memory_projection())?;
    let state = decode_revision_status(state)?;
    Ok(metadata.with_status(state))
}

fn decode_optional_content(
    revision: &MemoryRevision,
    content: Option<Vec<u8>>,
    hash: Option<Vec<u8>>,
) -> Result<Option<MemoryContent>, StoreError> {
    match (content, hash) {
        (None, None) => Ok(None),
        (Some(content), Some(hash)) if Sha256::digest(&content).as_slice() == hash.as_slice() => {
            let content = String::from_utf8(content).map_err(|_| corrupt_memory_projection())?;
            if normalized_content_hash(&content).map_err(|_| corrupt_memory_projection())?
                != *revision.content_hash()
            {
                return Err(corrupt_memory_projection());
            }
            MemoryContent::new(content)
                .map(Some)
                .map_err(|_| corrupt_memory_projection())
        }
        _ => Err(corrupt_memory_projection()),
    }
}

fn decode_json<T: serde::de::DeserializeOwned>(
    json: &[u8],
    hash: &[u8],
    area: CorruptionArea,
) -> Result<T, StoreError> {
    if Sha256::digest(json).as_slice() != hash {
        return Err(StoreError::CorruptData { area });
    }
    serde_json::from_slice(json).map_err(|_| StoreError::CorruptData { area })
}

fn decode_scope(scope_type: &str, scope_id: String) -> Result<MemoryScope, StoreError> {
    match scope_type {
        "user" => UserId::new(scope_id).map(MemoryScope::User),
        "workspace" => WorkspaceId::new(scope_id).map(MemoryScope::Workspace),
        "session" => SessionId::new(scope_id).map(MemoryScope::Session),
        "agent" => AgentId::new(scope_id).map(MemoryScope::Agent),
        _ => return Err(corrupt_memory_projection()),
    }
    .map_err(|_| corrupt_memory_projection())
}

fn decode_memory_kind(value: &str) -> Result<MemoryKind, StoreError> {
    match value {
        "fact" => Ok(MemoryKind::Fact),
        "preference" => Ok(MemoryKind::Preference),
        "constraint" => Ok(MemoryKind::Constraint),
        "lesson" => Ok(MemoryKind::Lesson),
        "procedure" => Ok(MemoryKind::Procedure),
        _ => Err(corrupt_memory_projection()),
    }
}

fn introduced_revision(payload: &MemoryOperationPayload) -> Option<&MemoryRevision> {
    match payload {
        MemoryOperationPayload::MemoryCreated { revision, .. }
        | MemoryOperationPayload::RevisionProposed { revision }
        | MemoryOperationPayload::MemoryRevised { revision } => Some(revision),
        MemoryOperationPayload::ProposalApproved {
            approved_revision, ..
        } => Some(approved_revision),
        MemoryOperationPayload::RevisionValidated { .. }
        | MemoryOperationPayload::RevisionActivated { .. }
        | MemoryOperationPayload::RevisionSuperseded { .. }
        | MemoryOperationPayload::RevisionRejected { .. }
        | MemoryOperationPayload::MemoryRetracted { .. }
        | MemoryOperationPayload::MemoryDeleted { .. } => None,
    }
}

fn payload_revision_id(payload: &MemoryOperationPayload) -> Option<&str> {
    match payload {
        MemoryOperationPayload::MemoryCreated { revision, .. }
        | MemoryOperationPayload::RevisionProposed { revision }
        | MemoryOperationPayload::MemoryRevised { revision } => {
            Some(revision.revision_id().as_str())
        }
        MemoryOperationPayload::ProposalApproved {
            approved_revision, ..
        } => Some(approved_revision.revision_id().as_str()),
        MemoryOperationPayload::RevisionValidated { revision_id, .. }
        | MemoryOperationPayload::RevisionActivated { revision_id }
        | MemoryOperationPayload::RevisionSuperseded { revision_id, .. }
        | MemoryOperationPayload::RevisionRejected { revision_id, .. }
        | MemoryOperationPayload::MemoryRetracted { revision_id }
        | MemoryOperationPayload::MemoryDeleted { revision_id } => Some(revision_id.as_str()),
    }
}

const fn operation_kind(payload: &MemoryOperationPayload) -> &'static str {
    match payload {
        MemoryOperationPayload::MemoryCreated { .. } => "created",
        MemoryOperationPayload::RevisionProposed { .. } => "proposed",
        MemoryOperationPayload::MemoryRevised { .. } => "revised",
        MemoryOperationPayload::RevisionValidated { .. } => "validated",
        MemoryOperationPayload::ProposalApproved { .. } => "approved",
        MemoryOperationPayload::RevisionActivated { .. } => "activated",
        MemoryOperationPayload::RevisionSuperseded { .. } => "superseded",
        MemoryOperationPayload::RevisionRejected { .. } => "rejected",
        MemoryOperationPayload::MemoryRetracted { .. } => "retracted",
        MemoryOperationPayload::MemoryDeleted { .. } => "deleted",
    }
}

fn validity_bounds(validity: MemoryValidity) -> (Option<i64>, Option<i64>) {
    match validity {
        MemoryValidity::Indefinite => (None, None),
        MemoryValidity::From { valid_from } => (Some(valid_from.get()), None),
        MemoryValidity::Until { valid_until } => (None, Some(valid_until.get())),
        MemoryValidity::Window(window) => (
            Some(window.valid_from().get()),
            Some(window.valid_until().get()),
        ),
    }
}

fn literal_fts_query(query: &str) -> Option<String> {
    let mut terms = BTreeSet::new();
    for term in query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(MAX_FTS_QUERY_TERMS)
    {
        terms.insert(term.to_lowercase());
    }
    (!terms.is_empty()).then(|| {
        terms
            .into_iter()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn encode_scope(scope: &MemoryScope) -> (&'static str, &str) {
    match scope {
        MemoryScope::User(id) => ("user", id.as_str()),
        MemoryScope::Workspace(id) => ("workspace", id.as_str()),
        MemoryScope::Session(id) => ("session", id.as_str()),
        MemoryScope::Agent(id) => ("agent", id.as_str()),
    }
}

const fn encode_memory_kind(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Fact => "fact",
        MemoryKind::Preference => "preference",
        MemoryKind::Constraint => "constraint",
        MemoryKind::Lesson => "lesson",
        MemoryKind::Procedure => "procedure",
    }
}

const fn encode_revision_status(status: MemoryRevisionStatus) -> &'static str {
    match status {
        MemoryRevisionStatus::Proposed => "proposed",
        MemoryRevisionStatus::Active => "active",
        MemoryRevisionStatus::Superseded => "superseded",
        MemoryRevisionStatus::Rejected => "rejected",
        MemoryRevisionStatus::Retracted => "retracted",
        MemoryRevisionStatus::Deleted => "deleted",
    }
}

fn decode_revision_status(value: &str) -> Result<MemoryRevisionStatus, StoreError> {
    match value {
        "proposed" => Ok(MemoryRevisionStatus::Proposed),
        "active" => Ok(MemoryRevisionStatus::Active),
        "superseded" => Ok(MemoryRevisionStatus::Superseded),
        "rejected" => Ok(MemoryRevisionStatus::Rejected),
        "retracted" => Ok(MemoryRevisionStatus::Retracted),
        "deleted" => Ok(MemoryRevisionStatus::Deleted),
        _ => Err(corrupt_memory_projection()),
    }
}

const fn encode_origin(origin: MemoryOrigin) -> &'static str {
    match origin {
        MemoryOrigin::ExplicitUser => "explicit_user",
        MemoryOrigin::VerifiedTool => "verified_tool",
        MemoryOrigin::ImportedDocument => "imported_document",
        MemoryOrigin::ModelProposal => "model_proposal",
        MemoryOrigin::Compaction => "compaction",
    }
}

const fn encode_trust(trust: TrustClass) -> &'static str {
    match trust {
        TrustClass::UserApproved => "user_approved",
        TrustClass::VerifiedObservation => "verified_observation",
        TrustClass::Imported => "imported",
        TrustClass::UntrustedProposal => "untrusted_proposal",
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

const fn sensitivity_rank(sensitivity: Sensitivity) -> i64 {
    match sensitivity {
        Sensitivity::Public => 0,
        Sensitivity::Internal => 1,
        Sensitivity::Sensitive => 2,
        Sensitivity::Secret => 3,
    }
}

const fn encode_evidence_relation(
    relation: autoharness_domain::MemoryEvidenceRelation,
) -> &'static str {
    match relation {
        autoharness_domain::MemoryEvidenceRelation::Supports => "supports",
        autoharness_domain::MemoryEvidenceRelation::Contradicts => "contradicts",
        autoharness_domain::MemoryEvidenceRelation::DerivedFrom => "derived_from",
    }
}

const fn encode_relation_kind(kind: autoharness_domain::MemoryRelationKind) -> &'static str {
    match kind {
        autoharness_domain::MemoryRelationKind::DuplicateOf => "duplicate_of",
        autoharness_domain::MemoryRelationKind::Contradicts => "contradicts",
        autoharness_domain::MemoryRelationKind::Refines => "refines",
        autoharness_domain::MemoryRelationKind::Supersedes => "supersedes",
        autoharness_domain::MemoryRelationKind::Related => "related",
    }
}

const fn encode_validation_status(status: MemoryValidationStatus) -> &'static str {
    match status {
        MemoryValidationStatus::Accepted => "accepted",
        MemoryValidationStatus::NeedsReview => "needs_review",
        MemoryValidationStatus::Rejected => "rejected",
    }
}

fn digest_bytes(value: &str) -> Result<[u8; 32], StoreError> {
    if value.len() != 64 {
        return Err(corrupt_memory_projection());
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| corrupt_memory_projection())?;
    }
    Ok(output)
}

fn map_identity_error(error: rusqlite::Error, kind: IdentityKind) -> StoreError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => StoreError::IdentityConflict { kind },
        _ => map_sqlite_error(error),
    }
}

const fn corrupt_memory_ledger() -> StoreError {
    StoreError::CorruptData {
        area: CorruptionArea::MemoryLedger,
    }
}

const fn corrupt_memory_projection() -> StoreError {
    StoreError::CorruptData {
        area: CorruptionArea::MemoryProjection,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        CommandId, ConfidenceBasisPoints, CorrelationId, InputId, MemoryEvidence,
        MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryEvidenceRelation, MemoryEvidenceSource,
        MemoryId, MemoryOperationId, MemoryOrigin, MemoryRevisionDraft, MemoryRevisionNumber,
        MemorySequence, MemorySubjectKey, MemoryValidationResult, MemoryValidationStatus,
        SessionId, Sha256Digest, TimestampMillis, TrustClass, UserId,
    };
    use autoharness_store::{
        DeletionDisposition, MemoryContentState, MemoryEvidenceContent, MemoryRevisionContent,
        MemoryStore, SessionStore,
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
            let path = directory.path().join("memory.sqlite3");
            Self {
                _directory: directory,
                path,
            }
        }

        fn open(&self) -> SqliteStore {
            SqliteStore::open(&self.path).expect("open sqlite store")
        }
    }

    fn digest(content: &str) -> Sha256Digest {
        normalized_content_hash(content).expect("valid normalized digest")
    }

    fn revision(
        revision_id: &str,
        revision_number: u64,
        content: &str,
        status: MemoryRevisionStatus,
    ) -> (MemoryRevision, MemoryRevisionContent) {
        revision_with(
            revision_id,
            revision_number,
            content,
            status,
            None,
            Sensitivity::Internal,
            Vec::new(),
        )
    }

    fn revision_with(
        revision_id: &str,
        revision_number: u64,
        content: &str,
        status: MemoryRevisionStatus,
        subject_key: Option<&str>,
        sensitivity: Sensitivity,
        evidence: Vec<MemoryEvidence>,
    ) -> (MemoryRevision, MemoryRevisionContent) {
        let content = MemoryContent::new(content).expect("valid content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new(revision_id).expect("revision ID"),
            MemoryRevisionNumber::new(revision_number).expect("revision number"),
            subject_key.map(|key| MemorySubjectKey::new(key).expect("subject key")),
            content.clone(),
            digest(content.as_str()),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            sensitivity,
            MemoryValidity::Indefinite,
            evidence,
            Vec::new(),
        )
        .expect("revision draft");
        let evidence_content = draft
            .evidence()
            .iter()
            .filter_map(|evidence| {
                evidence.excerpt().cloned().map(|excerpt| {
                    MemoryEvidenceContent::new(evidence.evidence_id().clone(), excerpt)
                })
            })
            .collect();
        let metadata = MemoryRevision::from_draft(status, &draft, TimestampMillis::new(10), None);
        let sidecar = MemoryRevisionContent::new(
            draft.revision_id().clone(),
            draft.content().clone(),
            evidence_content,
        );
        (metadata, sidecar)
    }

    fn create_operation(
        memory_id: &str,
        operation_id: &str,
        revision: MemoryRevision,
    ) -> MemoryOperationEnvelope {
        create_operation_in_scope(
            memory_id,
            operation_id,
            MemoryScope::User(UserId::new("user-1").expect("user ID")),
            revision,
        )
    }

    fn create_operation_in_scope(
        memory_id: &str,
        operation_id: &str,
        scope: MemoryScope,
        revision: MemoryRevision,
    ) -> MemoryOperationEnvelope {
        MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new(operation_id).expect("operation ID"),
            MemoryId::new(memory_id).expect("memory ID"),
            MemorySequence::FIRST,
            TimestampMillis::new(10),
            MemoryCausation::Command(
                CommandId::new(format!("command-{operation_id}")).expect("command ID"),
            ),
            CorrelationId::new(format!("correlation-{operation_id}")).expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope,
                memory_kind: MemoryKind::Fact,
                revision,
            },
        )
    }

    fn raw_digest(content: &str) -> Sha256Digest {
        let encoded = Sha256::digest(content.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Sha256Digest::new(encoded).expect("raw digest")
    }

    #[test]
    fn active_memory_round_trips_restarts_and_uses_literal_scoped_fts() {
        let database = TestDatabase::new();
        let (revision, sidecar) = revision(
            "revision-a",
            1,
            "Alpha release readiness checklist",
            MemoryRevisionStatus::Active,
        );
        let operation = create_operation("memory-a", "operation-a", revision.clone());
        let request = MemoryAppendRequest::new(0, operation.clone(), Some(sidecar));

        let mut store = database.open();
        let receipt = store.append_memory(&request).expect("append memory");
        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(
            store
                .append_memory(&request)
                .expect("exact retry")
                .disposition(),
            MemoryAppendDisposition::AlreadyCommitted
        );

        let query = MemorySearchQuery::new(
            MemoryContent::new("Alpha OR title:\"injection\"").expect("query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Internal,
            TimestampMillis::new(20),
            8,
        )
        .expect("search query");
        let batch = store.search_memory(&query).expect("search memory");
        assert_eq!(batch.generation().get(), 1);
        assert_eq!(batch.candidates().len(), 1);
        assert_eq!(
            batch.candidates()[0].content().as_str(),
            "Alpha release readiness checklist"
        );
        drop(store);

        let reopened = database.open();
        assert_eq!(
            reopened
                .load_memory_operations(operation.memory_id(), 0, 8)
                .expect("load operations"),
            vec![operation]
        );
        assert_eq!(
            reopened
                .load_memory_revisions(&MemoryId::new("memory-a").expect("memory ID"))
                .expect("load revisions"),
            vec![revision]
        );
        assert_eq!(
            reopened
                .load_memory_content(&MemoryRevisionId::new("revision-a").expect("revision ID"))
                .expect("load content")
                .expect("retained content")
                .as_str(),
            "Alpha release readiness checklist"
        );
    }

    #[test]
    fn proposal_activation_and_projection_rebuild_restore_search_deterministically() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (proposed, sidecar) = revision(
            "revision-proposed",
            1,
            "Deterministic context compaction",
            MemoryRevisionStatus::Proposed,
        );
        let create = create_operation("memory-proposed", "operation-proposed", proposed);
        store
            .append_memory(&MemoryAppendRequest::new(0, create.clone(), Some(sidecar)))
            .expect("append proposal");
        assert_eq!(store.memory_generation().expect("generation").get(), 0);
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("mutation generation")
                .get(),
            1
        );

        let activate = MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new("operation-activate").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(11),
            MemoryCausation::Operation(create.operation_id().clone()),
            CorrelationId::new("correlation-activate").expect("correlation ID"),
            MemoryOperationPayload::RevisionActivated {
                revision_id: MemoryRevisionId::new("revision-proposed").expect("revision ID"),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, activate, None))
            .expect("activate proposal");
        assert_eq!(store.memory_generation().expect("generation").get(), 1);
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("mutation generation")
                .get(),
            2
        );

        let query = MemorySearchQuery::new(
            MemoryContent::new("compaction").expect("query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Internal,
            TimestampMillis::new(20),
            8,
        )
        .expect("query");
        assert_eq!(
            store
                .search_memory(&query)
                .expect("search")
                .candidates()
                .len(),
            1
        );
        store
            .connection
            .execute("DELETE FROM memory_revision_fts", [])
            .expect("damage FTS projection");
        assert!(
            store
                .search_memory(&query)
                .expect("empty damaged search")
                .candidates()
                .is_empty()
        );
        store
            .rebuild_memory_projections()
            .expect("rebuild memory projections");
        let rebuilt = store.search_memory(&query).expect("rebuilt search");
        assert_eq!(rebuilt.candidates().len(), 1);
        assert_eq!(
            rebuilt.candidates()[0].revision().status(),
            MemoryRevisionStatus::Active
        );
    }

    #[test]
    fn logical_command_batches_are_atomic_contiguous_and_exactly_idempotent() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (proposed, sidecar) = revision(
            "revision-batch",
            1,
            "Atomic proposed memory",
            MemoryRevisionStatus::Proposed,
        );
        let create = create_operation("memory-batch", "operation-batch-create", proposed.clone());
        let validate = MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new("operation-batch-validate")
                .expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(11),
            MemoryCausation::Operation(create.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::RevisionValidated {
                revision_id: proposed.revision_id().clone(),
                validation: MemoryValidationResult::new(
                    1,
                    proposed.content_hash().clone(),
                    MemoryValidationStatus::Accepted,
                    Vec::new(),
                )
                .expect("validation"),
            },
        );
        let activate = MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new("operation-batch-activate")
                .expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(3).expect("sequence"),
            TimestampMillis::new(12),
            MemoryCausation::Operation(validate.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::RevisionActivated {
                revision_id: proposed.revision_id().clone(),
            },
        );
        let batch = MemoryAppendBatchRequest::new(
            0,
            vec![
                MemoryAppendOperation::new(create.clone(), Some(sidecar.clone())),
                MemoryAppendOperation::new(validate, None),
                MemoryAppendOperation::new(activate, None),
            ],
        );
        let receipt = store.append_memory_batch(&batch).expect("append batch");
        assert_eq!(receipt.last_sequence(), 3);
        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(
            store
                .append_memory_batch(&batch)
                .expect("retry batch")
                .disposition(),
            MemoryAppendDisposition::AlreadyCommitted
        );

        let (invalid_revision, invalid_sidecar) = revision(
            "revision-batch-invalid",
            1,
            "This transaction must roll back",
            MemoryRevisionStatus::Proposed,
        );
        let invalid_create = create_operation(
            "memory-batch-invalid",
            "operation-batch-invalid-create",
            invalid_revision,
        );
        let invalid_activate = MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new("operation-batch-invalid-activate")
                .expect("operation ID"),
            invalid_create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(13),
            MemoryCausation::Operation(invalid_create.operation_id().clone()),
            invalid_create.correlation_id().clone(),
            MemoryOperationPayload::RevisionActivated {
                revision_id: MemoryRevisionId::new("missing-revision").expect("revision ID"),
            },
        );
        let invalid_batch = MemoryAppendBatchRequest::new(
            0,
            vec![
                MemoryAppendOperation::new(invalid_create, Some(invalid_sidecar)),
                MemoryAppendOperation::new(invalid_activate, None),
            ],
        );
        assert_eq!(
            store.append_memory_batch(&invalid_batch),
            Err(StoreError::InvalidMemoryTransition)
        );
        let invalid_count = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_items WHERE memory_id = 'memory-batch-invalid'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("invalid item count");
        assert_eq!(invalid_count, 0);
    }

    #[test]
    fn deletion_erases_all_content_without_erasing_replayable_metadata() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let sentinel = "erase-me-unique-secretless-sentinel";
        let (revision, sidecar) =
            revision("revision-delete", 1, sentinel, MemoryRevisionStatus::Active);
        let create = create_operation("memory-delete", "operation-delete-create", revision);
        store
            .append_memory(&MemoryAppendRequest::new(0, create.clone(), Some(sidecar)))
            .expect("append memory");
        let delete = MemoryOperationEnvelope::new_v1(
            autoharness_domain::MemoryOperationId::new("operation-delete").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(12),
            MemoryCausation::Command(CommandId::new("command-delete").expect("command ID")),
            CorrelationId::new("correlation-delete").expect("correlation ID"),
            MemoryOperationPayload::MemoryDeleted {
                revision_id: MemoryRevisionId::new("revision-delete").expect("revision ID"),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, delete, None))
            .expect("delete memory");

        assert!(
            store
                .load_memory_content(
                    &MemoryRevisionId::new("revision-delete").expect("revision ID")
                )
                .expect("load deleted content")
                .is_none()
        );
        let revisions = store
            .load_memory_revisions(create.memory_id())
            .expect("load tombstone metadata");
        assert_eq!(revisions[0].status(), MemoryRevisionStatus::Deleted);
        let content_count = store
            .connection
            .query_row("SELECT COUNT(*) FROM memory_content_blobs", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("content count");
        assert_eq!(content_count, 0);
        let leaked = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_operations \
                 WHERE instr(CAST(envelope_json AS TEXT), ?1) > 0",
                params![sentinel],
                |row| row.get::<_, i64>(0),
            )
            .expect("sentinel query");
        assert_eq!(leaked, 0);
        store
            .rebuild_memory_projections()
            .expect("rebuild after deletion");
    }

    #[test]
    fn null_subject_identity_and_revision_reconstruction_survive_crlf_and_erasure() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let content = "First line\r\nSecond line";
        let (null_subject, null_sidecar) = revision_with(
            "revision-null-subject",
            1,
            content,
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Internal,
            Vec::new(),
        );
        let null_create = create_operation(
            "memory-null-subject",
            "operation-null-subject",
            null_subject.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                null_create.clone(),
                Some(null_sidecar),
            ))
            .expect("append null-subject memory");

        let (keyed, keyed_sidecar) = revision_with(
            "revision-keyed-subject",
            1,
            content,
            MemoryRevisionStatus::Active,
            Some("release:readiness"),
            Sensitivity::Internal,
            Vec::new(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation("memory-keyed-subject", "operation-keyed-subject", keyed),
                Some(keyed_sidecar),
            ))
            .expect("append keyed memory");

        let null_query = ActiveMemoryHeadQuery::new(
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            MemoryKind::Fact,
            None,
            8,
        )
        .expect("null-subject query")
        .with_content_hash(null_subject.content_hash().clone());
        let heads = store
            .load_active_memory_heads(&null_query)
            .expect("load null-subject identity");
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0].memory_id(), null_create.memory_id());

        let retained = store
            .load_memory_candidate(null_subject.revision_id())
            .expect("load candidate")
            .expect("candidate exists");
        assert_eq!(retained.memory_id(), null_create.memory_id());
        assert_eq!(
            retained.scope(),
            &MemoryScope::User(UserId::new("user-1").expect("user ID"))
        );
        assert_eq!(retained.memory_kind(), MemoryKind::Fact);
        assert_eq!(retained.revision(), &null_subject);
        assert_eq!(
            retained.content(),
            &MemoryContentState::Retained(MemoryContent::new(content).expect("content"))
        );

        let delete = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-null-delete").expect("operation ID"),
            null_create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(12),
            MemoryCausation::Command(CommandId::new("command-null-delete").expect("command ID")),
            CorrelationId::new("correlation-null-delete").expect("correlation ID"),
            MemoryOperationPayload::MemoryDeleted {
                revision_id: null_subject.revision_id().clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, delete, None))
            .expect("delete null-subject memory");
        let erased = store
            .load_memory_candidate(null_subject.revision_id())
            .expect("load erased candidate")
            .expect("erased candidate exists");
        assert_eq!(erased.content(), &MemoryContentState::Erased);
        assert_eq!(erased.revision().status(), MemoryRevisionStatus::Deleted);
    }

    #[test]
    fn search_applies_sensitivity_before_the_bounded_fts_window() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (sensitive, sensitive_sidecar) = revision_with(
            "revision-sensitive",
            1,
            "shared crowdout token",
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Sensitive,
            Vec::new(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation("memory-a-sensitive", "operation-sensitive", sensitive),
                Some(sensitive_sidecar),
            ))
            .expect("append sensitive memory");
        let (public, public_sidecar) = revision_with(
            "revision-public",
            1,
            "shared crowdout token",
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Public,
            Vec::new(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation("memory-z-public", "operation-public", public),
                Some(public_sidecar),
            ))
            .expect("append public memory");

        let query = MemorySearchQuery::new(
            MemoryContent::new("shared crowdout token").expect("query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Public,
            TimestampMillis::new(20),
            1,
        )
        .expect("search query");
        let batch = store.search_memory(&query).expect("search public memory");
        assert_eq!(batch.candidates().len(), 1);
        assert_eq!(
            batch.candidates()[0].memory_id(),
            &MemoryId::new("memory-z-public").expect("memory ID")
        );
    }

    #[test]
    fn session_deletion_erases_scoped_memory_and_cross_scope_evidence() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let session_id = SessionId::new("session-private").expect("session ID");
        store
            .connection
            .execute(
                "INSERT INTO sessions (session_id, status, last_sequence, created_at_ms, updated_at_ms) \
                 VALUES (?1, 'active', 1, 1, 9)",
                params![session_id.as_str()],
            )
            .expect("session");
        store
            .connection
            .execute(
                "INSERT INTO session_events (event_id, session_id, sequence, schema_version, \
                 occurred_at_ms, caused_by_command_id, caused_by_event_id, correlation_id, \
                 event_kind, envelope_json) \
                 VALUES ('event-private', ?1, 1, 1, 1, 'command-private', NULL, \
                         'correlation-private', 'session_created', x'01')",
                params![session_id.as_str()],
            )
            .expect("session event");

        let (session_revision, session_sidecar) = revision(
            "revision-session-private",
            1,
            "session-only retained bytes",
            MemoryRevisionStatus::Active,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation_in_scope(
                    "memory-session-private",
                    "operation-session-private",
                    MemoryScope::Session(session_id.clone()),
                    session_revision.clone(),
                ),
                Some(session_sidecar),
            ))
            .expect("append session memory");

        let excerpt =
            MemoryEvidenceExcerpt::new("private admitted input excerpt").expect("evidence excerpt");
        let evidence = MemoryEvidence::new(
            MemoryEvidenceId::new("evidence-session-input").expect("evidence ID"),
            MemoryEvidenceSource::UserInput {
                session_id: session_id.clone(),
                input_id: InputId::new("input-private").expect("input ID"),
            },
            MemoryEvidenceRelation::Supports,
            Some(excerpt.clone()),
            Some(raw_digest(excerpt.as_str())),
        )
        .expect("evidence");
        let (cross_scope_revision, cross_scope_sidecar) = revision_with(
            "revision-cross-scope",
            1,
            "cross-scope durable fact",
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Internal,
            vec![evidence],
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-cross-scope",
                    "operation-cross-scope",
                    cross_scope_revision.clone(),
                ),
                Some(cross_scope_sidecar),
            ))
            .expect("append cross-scope memory");

        let eligibility_before = store.memory_generation().expect("eligibility generation");
        let mutation_before = store
            .memory_mutation_generation()
            .expect("mutation generation");
        assert_eq!(
            store
                .delete_session(&session_id, 1)
                .expect("delete private session"),
            DeletionDisposition::Deleted
        );
        assert_eq!(
            store
                .memory_generation()
                .expect("eligibility generation")
                .get(),
            eligibility_before.get() + 1
        );
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("mutation generation")
                .get(),
            mutation_before.get() + 1
        );

        let session_candidate = store
            .load_memory_candidate(session_revision.revision_id())
            .expect("load session candidate")
            .expect("session tombstone");
        assert_eq!(session_candidate.content(), &MemoryContentState::Erased);
        assert_eq!(
            session_candidate.revision().status(),
            MemoryRevisionStatus::Deleted
        );
        let cross_scope_candidate = store
            .load_memory_candidate(cross_scope_revision.revision_id())
            .expect("load cross-scope candidate")
            .expect("cross-scope memory");
        assert!(matches!(
            cross_scope_candidate.content(),
            MemoryContentState::Retained(_)
        ));
        let erased_evidence = store
            .connection
            .query_row(
                "SELECT excerpt_content_id IS NULL, excerpt_sha256 IS NULL, erased_by_session_id \
                 FROM memory_evidence WHERE evidence_id = 'evidence-session-input'",
                [],
                |row| {
                    Ok((
                        row.get::<_, bool>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .expect("erased evidence");
        assert_eq!(
            erased_evidence,
            (true, true, Some(session_id.as_str().to_owned()))
        );
        store
            .rebuild_memory_projections()
            .expect("rebuild privacy tombstones");
        assert_eq!(
            store
                .load_memory_candidate(session_revision.revision_id())
                .expect("load rebuilt session candidate")
                .expect("rebuilt session tombstone")
                .content(),
            &MemoryContentState::Erased
        );
    }

    #[test]
    fn content_hash_mismatch_rolls_back_before_creating_an_item() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let content = MemoryContent::new("actual bytes").expect("content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-invalid").expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            digest("different bytes"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(8_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("draft");
        let metadata = MemoryRevision::from_draft(
            MemoryRevisionStatus::Active,
            &draft,
            TimestampMillis::new(1),
            None,
        );
        let operation = create_operation("memory-invalid", "operation-invalid", metadata);
        let sidecar = MemoryRevisionContent::new(draft.revision_id().clone(), content, Vec::new());
        assert_eq!(
            store.append_memory(&MemoryAppendRequest::new(0, operation, Some(sidecar))),
            Err(StoreError::InvalidMemoryTransition)
        );
        let item_count = store
            .connection
            .query_row("SELECT COUNT(*) FROM memory_items", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("item count");
        assert_eq!(item_count, 0);
    }
}
