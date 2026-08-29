use std::collections::{BTreeMap, BTreeSet};

use autoharness_domain::{
    AgentId, ContextAdmission, ContextTurnManifest, MEMORY_SCHEMA_V1, MemoryCausation,
    MemoryContent, MemoryEvidenceExcerpt, MemoryEvidenceSource, MemoryGeneration, MemoryId,
    MemoryKind, MemoryOperationEnvelope, MemoryOperationPayload, MemoryOrigin, MemoryRevision,
    MemoryRevisionId, MemoryRevisionStatus, MemoryScope, MemoryValidationResult,
    MemoryValidationStatus, MemoryValidity, Sensitivity, SessionId, TrustClass, UserId,
    WorkspaceId,
};
use autoharness_memory::{
    normalized_content_hash, verify_admission_rendered_hash, verify_context_manifest_hash,
};
use autoharness_store::{
    ActiveMemoryHead, ActiveMemoryHeadPageQuery, ActiveMemoryHeadQuery, CorruptionArea,
    IdentityKind, MAX_MEMORY_SEARCH_CANDIDATES, MemoryAdmissionKey, MemoryAdmissionQuery,
    MemoryAdmissionRecord, MemoryAppendBatchRequest, MemoryAppendDisposition,
    MemoryAppendOperation, MemoryAppendReceipt, MemoryAppendRequest, MemoryCandidateBatch,
    MemoryContentState, MemoryEvidenceExcerptState, MemoryInspectionPage, MemoryInspectionQuery,
    MemoryInspectionRecord, MemoryInspectionStatus, MemoryMutationGeneration,
    MemorySearchCandidate, MemorySearchQuery, MemoryStore, StoreError, StoredMemoryCandidate,
    StoredMemoryEvidenceContent,
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

    fn load_memory_evidence_content(
        &self,
        revision_id: &MemoryRevisionId,
    ) -> Result<Option<Vec<StoredMemoryEvidenceContent>>, StoreError> {
        let revision_row = self
            .connection
            .query_row(
                "SELECT r.state, r.metadata_json, r.metadata_sha256, i.lifecycle \
                 FROM memory_revisions AS r \
                 JOIN memory_items AS i ON i.memory_id = r.memory_id \
                 WHERE r.revision_id = ?1",
                params![revision_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?;
        let Some((state, metadata_json, metadata_hash, item_lifecycle)) = revision_row else {
            return Ok(None);
        };
        let revision = decode_projected_revision(&state, &metadata_json, &metadata_hash)?;
        if revision.revision_id() != revision_id {
            return Err(corrupt_memory_projection());
        }

        let mut statement = self
            .connection
            .prepare(
                "SELECT e.ordinal, e.evidence_id, e.source_json, e.relation, \
                        e.excerpt_content_id, e.excerpt_sha256, e.erased_by_session_id, \
                        e.erased_at_ms, b.content_utf8, b.content_sha256 \
                 FROM memory_evidence AS e \
                 LEFT JOIN memory_content_blobs AS b ON b.content_id = e.excerpt_content_id \
                 WHERE e.revision_id = ?1 ORDER BY e.ordinal",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map(params![revision_id.as_str()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<Vec<u8>>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<Vec<u8>>>(8)?,
                    row.get::<_, Option<Vec<u8>>>(9)?,
                ))
            })
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        if rows.len() != revision.evidence().len() {
            return Err(corrupt_memory_projection());
        }

        revision
            .evidence()
            .iter()
            .zip(rows)
            .enumerate()
            .map(|(index, (metadata, row))| {
                let (
                    ordinal,
                    evidence_id,
                    source_json,
                    relation,
                    content_id,
                    excerpt_hash,
                    erased_by_session_id,
                    erased_at_ms,
                    content,
                    content_hash,
                ) = row;
                let expected_ordinal =
                    i64::try_from(index).map_err(|_| StoreError::LimitExceeded)?;
                let expected_source =
                    serde_json::to_vec(metadata.source()).map_err(|_| StoreError::Backend)?;
                if ordinal != expected_ordinal
                    || evidence_id != metadata.evidence_id().as_str()
                    || source_json != expected_source
                    || relation != encode_evidence_relation(metadata.relation())
                {
                    return Err(corrupt_memory_projection());
                }

                let excerpt = match (
                    metadata.excerpt_hash(),
                    content_id,
                    excerpt_hash,
                    erased_by_session_id,
                    erased_at_ms,
                    content,
                    content_hash,
                ) {
                    (None, None, None, None, None, None, None) => {
                        MemoryEvidenceExcerptState::Absent
                    }
                    (Some(_), None, None, erased_by, erased_at, None, None)
                        if item_lifecycle == "deleted"
                            || (erased_by.is_some() && erased_at.is_some()) =>
                    {
                        MemoryEvidenceExcerptState::Erased
                    }
                    (
                        Some(expected_hash),
                        Some(content_id),
                        Some(indexed_hash),
                        None,
                        None,
                        Some(content),
                        Some(blob_hash),
                    ) if content_id == format!("memory-evidence:{evidence_id}")
                        && indexed_hash.as_slice()
                            == digest_bytes(expected_hash.as_str())?.as_slice()
                        && blob_hash == indexed_hash
                        && Sha256::digest(&content).as_slice() == indexed_hash.as_slice() =>
                    {
                        let excerpt =
                            String::from_utf8(content).map_err(|_| corrupt_memory_projection())?;
                        MemoryEvidenceExcerptState::Retained(
                            MemoryEvidenceExcerpt::new(excerpt)
                                .map_err(|_| corrupt_memory_projection())?,
                        )
                    }
                    _ => return Err(corrupt_memory_projection()),
                };
                Ok(StoredMemoryEvidenceContent::new(
                    metadata.evidence_id().clone(),
                    excerpt,
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()
            .map(Some)
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

    fn inspect_memory_page(
        &self,
        query: &MemoryInspectionQuery,
    ) -> Result<MemoryInspectionPage, StoreError> {
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
        append_effective_status_filter(&mut sql, &mut values, query)?;
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
        values.push(Value::Integer(sensitivity_rank(
            query.sensitivity_ceiling(),
        )));
        sql.push_str(
            " AND CASE r.sensitivity \
                WHEN 'public' THEN 0 WHEN 'internal' THEN 1 \
                WHEN 'sensitive' THEN 2 WHEN 'secret' THEN 3 ELSE 4 END <= ?",
        );
        sql.push_str(&values.len().to_string());
        if let Some(literal_search) = query.literal_search() {
            values.push(Value::Text(literal_search.as_str().to_owned()));
            sql.push_str(" AND (instr(i.memory_id, ?");
            sql.push_str(&values.len().to_string());
            sql.push_str(") > 0 OR instr(CAST(b.content_utf8 AS TEXT), ?");
            sql.push_str(&values.len().to_string());
            sql.push_str(") > 0)");
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
        values.push(Value::Integer(i64::from(query.limit()) + 1));

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
        let mut records = rows
            .map(|row| {
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
                let evidence_content = self
                    .load_memory_evidence_content(latest_revision.revision_id())?
                    .ok_or_else(corrupt_memory_projection)?;
                let latest_validation = load_latest_revision_validation(
                    &self.connection,
                    &MemoryId::new(memory_id.clone()).map_err(|_| corrupt_memory_projection())?,
                    &latest_revision,
                )?;
                Ok(MemoryInspectionRecord::new(
                    MemoryId::new(memory_id).map_err(|_| corrupt_memory_projection())?,
                    decode_scope(&scope_type, scope_id)?,
                    decode_memory_kind(&memory_kind)?,
                    lifecycle,
                    latest_revision,
                    content,
                    evidence_content,
                    latest_validation,
                    active_revision_id
                        .map(MemoryRevisionId::new)
                        .transpose()
                        .map_err(|_| corrupt_memory_projection())?,
                    u64::try_from(last_sequence).map_err(|_| corrupt_memory_projection())?,
                    autoharness_domain::TimestampMillis::new(created_at_ms),
                    autoharness_domain::TimestampMillis::new(updated_at_ms),
                ))
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let has_more = records.len()
            > usize::try_from(query.limit()).map_err(|_| StoreError::LimitExceeded)?;
        if has_more {
            records.pop();
        }
        Ok(MemoryInspectionPage::new(records, has_more))
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
        let operations = load_all_operations(&transaction)?;
        let erasures = load_rebuild_erasures(&transaction, &operations)?;
        let privacy_blobs = verify_rebuild_sidecars(&transaction, &operations, &erasures)?;
        transaction
            .execute_batch("PRAGMA defer_foreign_keys = ON;")
            .map_err(map_sqlite_error)?;
        clear_memory_projections(&transaction)?;
        for content_id in privacy_blobs {
            transaction
                .execute(
                    "DELETE FROM memory_content_blobs WHERE content_id = ?1",
                    params![content_id],
                )
                .map_err(map_sqlite_error)?;
        }
        let replay = replay_memory_operations(&transaction, &operations, &erasures)?;
        rebuild_fts(&transaction)?;
        replace_memory_store_state(&transaction, replay)?;
        verify_rebuilt_projection(&transaction, &operations)?;
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
    struct OperationRow {
        operation_id: String,
        memory_id: String,
        sequence: i64,
        schema_version: i64,
        operation_kind: String,
        revision_id: Option<String>,
        caused_by_command_id: Option<String>,
        caused_by_operation_id: Option<String>,
        correlation_id: String,
        occurred_at_ms: i64,
        envelope_json: Vec<u8>,
        operation_sha256: Vec<u8>,
    }

    let mut statement = transaction
        .prepare(
            "SELECT operation_id, memory_id, sequence, schema_version, operation_kind, \
                    revision_id, caused_by_command_id, caused_by_operation_id, correlation_id, \
                    occurred_at_ms, envelope_json, operation_sha256 \
             FROM memory_operations \
             ORDER BY memory_id, sequence",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(OperationRow {
                operation_id: row.get(0)?,
                memory_id: row.get(1)?,
                sequence: row.get(2)?,
                schema_version: row.get(3)?,
                operation_kind: row.get(4)?,
                revision_id: row.get(5)?,
                caused_by_command_id: row.get(6)?,
                caused_by_operation_id: row.get(7)?,
                correlation_id: row.get(8)?,
                occurred_at_ms: row.get(9)?,
                envelope_json: row.get(10)?,
                operation_sha256: row.get(11)?,
            })
        })
        .map_err(map_sqlite_error)?;

    let mut operations = Vec::new();
    let mut current_memory_id = None::<String>;
    let mut expected_sequence = 1_u64;
    let mut seen_operations = BTreeMap::<String, (String, u64)>::new();
    let mut seen_commands = BTreeSet::new();
    for row in rows {
        let row = row.map_err(map_sqlite_error)?;
        if Sha256::digest(&row.envelope_json).as_slice() != row.operation_sha256.as_slice() {
            return Err(corrupt_memory_ledger());
        }
        let operation: MemoryOperationEnvelope =
            serde_json::from_slice(&row.envelope_json).map_err(|_| corrupt_memory_ledger())?;
        let sequence = u64::try_from(row.sequence).map_err(|_| corrupt_memory_ledger())?;
        let scalar_causation_matches = match operation.causation() {
            MemoryCausation::Command(command_id) => {
                row.caused_by_command_id.as_deref() == Some(command_id.as_str())
                    && row.caused_by_operation_id.is_none()
            }
            MemoryCausation::Operation(operation_id) => {
                row.caused_by_command_id.is_none()
                    && row.caused_by_operation_id.as_deref() == Some(operation_id.as_str())
            }
        };
        if operation.operation_id().as_str() != row.operation_id
            || operation.memory_id().as_str() != row.memory_id
            || operation.sequence().get() != sequence
            || i64::from(operation.schema_version()) != row.schema_version
            || operation_kind(operation.payload()) != row.operation_kind
            || payload_revision_id(operation.payload()) != row.revision_id.as_deref()
            || operation.correlation_id().as_str() != row.correlation_id
            || operation.occurred_at().get() != row.occurred_at_ms
            || !scalar_causation_matches
        {
            return Err(corrupt_memory_ledger());
        }
        if operation.schema_version() != MEMORY_SCHEMA_V1 {
            return Err(StoreError::UnsupportedMemorySchema {
                found: operation.schema_version(),
            });
        }

        if current_memory_id.as_deref() != Some(operation.memory_id().as_str()) {
            current_memory_id = Some(operation.memory_id().as_str().to_owned());
            expected_sequence = 1;
        }
        if operation.sequence().get() != expected_sequence
            || (expected_sequence == 1)
                != matches!(
                    operation.payload(),
                    MemoryOperationPayload::MemoryCreated { .. }
                )
        {
            return Err(corrupt_memory_ledger());
        }
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;

        match operation.causation() {
            MemoryCausation::Command(command_id) => {
                if !seen_commands.insert(command_id.as_str().to_owned()) {
                    return Err(corrupt_memory_ledger());
                }
            }
            MemoryCausation::Operation(cause_id) => {
                let Some((cause_memory_id, cause_sequence)) =
                    seen_operations.get(cause_id.as_str())
                else {
                    return Err(corrupt_memory_ledger());
                };
                if cause_memory_id != operation.memory_id().as_str()
                    || *cause_sequence >= operation.sequence().get()
                {
                    return Err(corrupt_memory_ledger());
                }
            }
        }
        if seen_operations
            .insert(
                operation.operation_id().as_str().to_owned(),
                (
                    operation.memory_id().as_str().to_owned(),
                    operation.sequence().get(),
                ),
            )
            .is_some()
        {
            return Err(corrupt_memory_ledger());
        }
        operations.push(operation);
    }
    Ok(operations)
}

#[derive(Clone)]
struct RebuildEvidenceMetadata {
    memory_id: String,
    source: MemoryEvidenceSource,
    has_excerpt: bool,
}

#[derive(Clone)]
struct RebuildEvidenceErasure {
    session_id: SessionId,
    erased_at_ms: i64,
}

#[derive(Default)]
struct RebuildSessionErasure {
    session_last_sequence: Option<u64>,
    erased_at_ms: i64,
    memory_ids: BTreeSet<String>,
    evidence_keys: BTreeSet<(String, String)>,
}

#[derive(Default)]
struct RebuildErasures {
    deleted_memories: BTreeSet<String>,
    evidence: BTreeMap<(String, String), RebuildEvidenceErasure>,
    sessions: BTreeMap<String, RebuildSessionErasure>,
}

fn record_rebuild_session_erasure(
    erasures: &mut RebuildErasures,
    session_id: &SessionId,
    session_last_sequence: Option<u64>,
    erased_at_ms: i64,
) -> Result<(), StoreError> {
    if let Some(existing) = erasures.sessions.get_mut(session_id.as_str()) {
        if existing.erased_at_ms != erased_at_ms
            || (existing.session_last_sequence.is_some()
                && session_last_sequence.is_some()
                && existing.session_last_sequence != session_last_sequence)
        {
            return Err(corrupt_memory_projection());
        }
        if existing.session_last_sequence.is_none() {
            existing.session_last_sequence = session_last_sequence;
        }
    } else {
        erasures.sessions.insert(
            session_id.as_str().to_owned(),
            RebuildSessionErasure {
                session_last_sequence,
                erased_at_ms,
                memory_ids: BTreeSet::new(),
                evidence_keys: BTreeSet::new(),
            },
        );
    }
    Ok(())
}

fn load_rebuild_erasures(
    transaction: &Transaction<'_>,
    operations: &[MemoryOperationEnvelope],
) -> Result<RebuildErasures, StoreError> {
    let mut erasures = RebuildErasures::default();
    let mut memory_scopes = BTreeMap::<String, MemoryScope>::new();
    let mut evidence_metadata = BTreeMap::<(String, String), RebuildEvidenceMetadata>::new();
    let mut evidence_ids = BTreeSet::new();
    for operation in operations {
        if let MemoryOperationPayload::MemoryCreated { scope, .. } = operation.payload()
            && memory_scopes
                .insert(operation.memory_id().as_str().to_owned(), scope.clone())
                .is_some()
        {
            return Err(corrupt_memory_ledger());
        }
        if matches!(
            operation.payload(),
            MemoryOperationPayload::MemoryDeleted { .. }
        ) {
            erasures
                .deleted_memories
                .insert(operation.memory_id().as_str().to_owned());
        }
        if let Some(revision) = introduced_revision(operation.payload()) {
            for evidence in revision.evidence() {
                if !evidence_ids.insert(evidence.evidence_id().as_str().to_owned())
                    || evidence_metadata
                        .insert(
                            (
                                revision.revision_id().as_str().to_owned(),
                                evidence.evidence_id().as_str().to_owned(),
                            ),
                            RebuildEvidenceMetadata {
                                memory_id: operation.memory_id().as_str().to_owned(),
                                source: evidence.source().clone(),
                                has_excerpt: evidence.excerpt_hash().is_some(),
                            },
                        )
                        .is_some()
                {
                    return Err(corrupt_memory_ledger());
                }
            }
        }
    }

    let mut statement = transaction
        .prepare(
            "SELECT memory_id, session_id, session_last_sequence, erased_at_ms \
             FROM memory_session_erasure_tombstones ORDER BY session_id, memory_id",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    for row in rows {
        let (memory_id, session_id, last_sequence, erased_at_ms) = row.map_err(map_sqlite_error)?;
        let session_id = SessionId::new(session_id).map_err(|_| corrupt_memory_projection())?;
        let last_sequence =
            u64::try_from(last_sequence).map_err(|_| corrupt_memory_projection())?;
        if !matches!(
            memory_scopes.get(&memory_id),
            Some(MemoryScope::Session(scope_session_id)) if scope_session_id == &session_id
        ) {
            return Err(corrupt_memory_projection());
        }
        record_rebuild_session_erasure(
            &mut erasures,
            &session_id,
            Some(last_sequence),
            erased_at_ms,
        )?;
        erasures.deleted_memories.insert(memory_id.clone());
        erasures
            .sessions
            .get_mut(session_id.as_str())
            .ok_or_else(corrupt_memory_projection)?
            .memory_ids
            .insert(memory_id);
    }
    drop(statement);

    let mut statement = transaction
        .prepare(
            "SELECT revision_id, evidence_id, erased_by_session_id, erased_at_ms \
             FROM memory_evidence \
             WHERE erased_by_session_id IS NOT NULL OR erased_at_ms IS NOT NULL \
             ORDER BY revision_id, ordinal",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .map_err(map_sqlite_error)?;
    for row in rows {
        let (revision_id, evidence_id, session_id, erased_at_ms) = row.map_err(map_sqlite_error)?;
        let (Some(session_id), Some(erased_at_ms)) = (session_id, erased_at_ms) else {
            return Err(corrupt_memory_projection());
        };
        let session_id = SessionId::new(session_id).map_err(|_| corrupt_memory_projection())?;
        let key = (revision_id, evidence_id);
        let metadata = evidence_metadata
            .get(&key)
            .ok_or_else(corrupt_memory_projection)?;
        if !metadata.has_excerpt || !evidence_belongs_to_session(&metadata.source, &session_id) {
            return Err(corrupt_memory_projection());
        }
        record_rebuild_session_erasure(&mut erasures, &session_id, None, erased_at_ms)?;
        if erasures
            .evidence
            .insert(
                key.clone(),
                RebuildEvidenceErasure {
                    session_id: session_id.clone(),
                    erased_at_ms,
                },
            )
            .is_some()
        {
            return Err(corrupt_memory_projection());
        }
        erasures
            .sessions
            .get_mut(session_id.as_str())
            .ok_or_else(corrupt_memory_projection)?
            .evidence_keys
            .insert(key);
    }

    let deleted_sessions = erasures
        .sessions
        .iter()
        .filter_map(|(session_id, erasure)| {
            erasure
                .session_last_sequence
                .map(|_| (session_id.clone(), erasure.erased_at_ms))
        })
        .collect::<Vec<_>>();
    for ((revision_id, evidence_id), metadata) in evidence_metadata {
        if !metadata.has_excerpt || erasures.deleted_memories.contains(&metadata.memory_id) {
            continue;
        }
        let Some((session_id, erased_at_ms)) = deleted_sessions.iter().find(|(session_id, _)| {
            SessionId::new(session_id.clone())
                .is_ok_and(|session_id| evidence_belongs_to_session(&metadata.source, &session_id))
        }) else {
            continue;
        };
        let session_id =
            SessionId::new(session_id.clone()).map_err(|_| corrupt_memory_projection())?;
        let key = (revision_id, evidence_id);
        if let Some(existing) = erasures.evidence.get(&key) {
            if existing.session_id != session_id || existing.erased_at_ms != *erased_at_ms {
                return Err(corrupt_memory_projection());
            }
            continue;
        }
        erasures.evidence.insert(
            key.clone(),
            RebuildEvidenceErasure {
                session_id: session_id.clone(),
                erased_at_ms: *erased_at_ms,
            },
        );
        erasures
            .sessions
            .get_mut(session_id.as_str())
            .ok_or_else(corrupt_memory_projection)?
            .evidence_keys
            .insert(key);
    }
    Ok(erasures)
}

struct RebuildBlob {
    media_type: String,
    content: Vec<u8>,
    content_sha256: Vec<u8>,
    created_at_ms: i64,
}

fn verify_retained_rebuild_blob(blob: &RebuildBlob, created_at_ms: i64) -> Result<(), StoreError> {
    if blob.media_type != "text/plain; charset=utf-8"
        || blob.created_at_ms != created_at_ms
        || Sha256::digest(&blob.content).as_slice() != blob.content_sha256.as_slice()
    {
        return Err(corrupt_memory_projection());
    }
    Ok(())
}

fn verify_rebuild_sidecars(
    transaction: &Transaction<'_>,
    operations: &[MemoryOperationEnvelope],
    erasures: &RebuildErasures,
) -> Result<BTreeSet<String>, StoreError> {
    let mut statement = transaction
        .prepare(
            "SELECT content_id, media_type, content_utf8, content_sha256, created_at_ms \
             FROM memory_content_blobs ORDER BY content_id",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                RebuildBlob {
                    media_type: row.get(1)?,
                    content: row.get(2)?,
                    content_sha256: row.get(3)?,
                    created_at_ms: row.get(4)?,
                },
            ))
        })
        .map_err(map_sqlite_error)?;
    let mut blobs = rows
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(map_sqlite_error)?;
    let mut privacy_blobs = BTreeSet::new();
    let mut revision_ids = BTreeSet::new();
    let mut evidence_ids = BTreeSet::new();
    for operation in operations {
        let Some(revision) = introduced_revision(operation.payload()) else {
            continue;
        };
        if revision.sensitivity() == Sensitivity::Secret
            || !revision_ids.insert(revision.revision_id().as_str().to_owned())
        {
            return Err(corrupt_memory_ledger());
        }
        let content_id = format!("memory-content:{}", revision.revision_id().as_str());
        if erasures
            .deleted_memories
            .contains(operation.memory_id().as_str())
        {
            if blobs.remove(&content_id).is_some() {
                privacy_blobs.insert(content_id);
            }
        } else {
            let blob = blobs
                .remove(&content_id)
                .ok_or_else(corrupt_memory_projection)?;
            verify_retained_rebuild_blob(&blob, revision.created_at().get())?;
            let text =
                std::str::from_utf8(&blob.content).map_err(|_| corrupt_memory_projection())?;
            MemoryContent::new(text.to_owned()).map_err(|_| corrupt_memory_projection())?;
            if normalized_content_hash(text).map_err(|_| corrupt_memory_projection())?
                != *revision.content_hash()
            {
                return Err(corrupt_memory_projection());
            }
        }

        for evidence in revision.evidence() {
            if !evidence_ids.insert(evidence.evidence_id().as_str().to_owned()) {
                return Err(corrupt_memory_ledger());
            }
            let evidence_content_id =
                format!("memory-evidence:{}", evidence.evidence_id().as_str());
            let key = (
                revision.revision_id().as_str().to_owned(),
                evidence.evidence_id().as_str().to_owned(),
            );
            let is_erased = erasures
                .deleted_memories
                .contains(operation.memory_id().as_str())
                || erasures.evidence.contains_key(&key);
            match (evidence.excerpt_hash(), is_erased) {
                (None, _) | (Some(_), true) => {
                    if blobs.remove(&evidence_content_id).is_some() {
                        privacy_blobs.insert(evidence_content_id);
                    }
                }
                (Some(expected_hash), false) => {
                    let blob = blobs
                        .remove(&evidence_content_id)
                        .ok_or_else(corrupt_memory_projection)?;
                    verify_retained_rebuild_blob(&blob, revision.created_at().get())?;
                    let expected_hash = digest_bytes(expected_hash.as_str())
                        .map_err(|_| corrupt_memory_ledger())?;
                    if blob.content_sha256.as_slice() != expected_hash.as_slice() {
                        return Err(corrupt_memory_projection());
                    }
                    let text = std::str::from_utf8(&blob.content)
                        .map_err(|_| corrupt_memory_projection())?;
                    MemoryEvidenceExcerpt::new(text.to_owned())
                        .map_err(|_| corrupt_memory_projection())?;
                }
            }
        }
    }
    privacy_blobs.extend(blobs.into_keys());
    Ok(privacy_blobs)
}

fn clear_memory_projections(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    transaction
        .execute_batch(
            "DELETE FROM memory_revision_fts; \
             DELETE FROM memory_validations; \
             DELETE FROM memory_evidence; \
             DELETE FROM memory_relations; \
             DELETE FROM memory_revisions; \
             DELETE FROM memory_items; \
             DELETE FROM memory_store_state;",
        )
        .map_err(map_sqlite_error)
}

fn insert_rebuilt_revision(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
    revision: &MemoryRevision,
    erasures: &RebuildErasures,
) -> Result<(), StoreError> {
    let latest_revision = transaction
        .query_row(
            "SELECT latest_revision FROM memory_items WHERE memory_id = ?1",
            params![operation.memory_id().as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    let expected_revision = u64::try_from(latest_revision)
        .map_err(|_| corrupt_memory_projection())?
        .checked_add(1)
        .ok_or(StoreError::SequenceOutOfRange)?;
    if revision.revision().get() != expected_revision {
        return Err(StoreError::InvalidMemoryTransition);
    }
    let memory_is_deleted = erasures
        .deleted_memories
        .contains(operation.memory_id().as_str());
    let content_id =
        (!memory_is_deleted).then(|| format!("memory-content:{}", revision.revision_id().as_str()));
    let metadata_json = serde_json::to_vec(revision).map_err(|_| StoreError::Backend)?;
    let metadata_hash = Sha256::digest(&metadata_json);
    let (valid_from, valid_until) = validity_bounds(revision.validity());
    transaction
        .execute(
            "INSERT INTO memory_revisions (revision_id, memory_id, revision, subject_key, \
             introduced_operation_id, state, content_id, content_hash_sha256, metadata_json, \
             metadata_sha256, origin, trust_class, confidence_basis_points, sensitivity, \
             valid_from_ms, valid_until_ms, created_at_ms, supersedes_revision_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                     ?15, ?16, ?17, ?18)",
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
        .map_err(map_sqlite_error)?;

    for (ordinal, evidence) in revision.evidence().iter().enumerate() {
        let key = (
            revision.revision_id().as_str().to_owned(),
            evidence.evidence_id().as_str().to_owned(),
        );
        let erasure = erasures.evidence.get(&key);
        let retains_excerpt =
            evidence.excerpt_hash().is_some() && !memory_is_deleted && erasure.is_none();
        let excerpt_content_id =
            retains_excerpt.then(|| format!("memory-evidence:{}", evidence.evidence_id().as_str()));
        let excerpt_sha256 = if retains_excerpt {
            evidence
                .excerpt_hash()
                .map(|hash| digest_bytes(hash.as_str()))
                .transpose()?
        } else {
            None
        };
        let source_json = serde_json::to_vec(evidence.source()).map_err(|_| StoreError::Backend)?;
        transaction
            .execute(
                "INSERT INTO memory_evidence (revision_id, ordinal, evidence_id, source_json, \
                 relation, excerpt_content_id, excerpt_sha256, erased_by_session_id, erased_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    revision.revision_id().as_str(),
                    i64::try_from(ordinal).map_err(|_| StoreError::LimitExceeded)?,
                    evidence.evidence_id().as_str(),
                    source_json,
                    encode_evidence_relation(evidence.relation()),
                    excerpt_content_id,
                    excerpt_sha256,
                    erasure.map(|erasure| erasure.session_id.as_str()),
                    erasure.map(|erasure| erasure.erased_at_ms),
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
    Ok(())
}

fn insert_rebuilt_validation(
    transaction: &Transaction<'_>,
    operation: &MemoryOperationEnvelope,
) -> Result<(), StoreError> {
    let MemoryOperationPayload::RevisionValidated {
        revision_id,
        validation,
    } = operation.payload()
    else {
        return Ok(());
    };
    let json = serde_json::to_vec(validation).map_err(|_| StoreError::Backend)?;
    transaction
        .execute(
            "INSERT INTO memory_validations (operation_id, revision_id, validator_version, \
             content_sha256, outcome, validation_json, created_at_ms) \
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
    Ok(())
}

#[derive(Clone, Copy)]
struct RebuildStoreState {
    generation: u64,
    mutation_generation: u64,
    updated_at_ms: i64,
}

fn replay_batch_end(operations: &[MemoryOperationEnvelope], start: usize) -> usize {
    let first = &operations[start];
    let mut preceding_ids = BTreeSet::from([first.operation_id().as_str()]);
    let mut end = start + 1;
    while let Some(operation) = operations.get(end) {
        if operation.memory_id() != first.memory_id()
            || operation.correlation_id() != first.correlation_id()
            || !matches!(
                operation.causation(),
                MemoryCausation::Operation(cause_id) if preceding_ids.contains(cause_id.as_str())
            )
        {
            break;
        }
        preceding_ids.insert(operation.operation_id().as_str());
        end += 1;
    }
    end
}

fn replay_memory_operations(
    transaction: &Transaction<'_>,
    operations: &[MemoryOperationEnvelope],
    erasures: &RebuildErasures,
) -> Result<RebuildStoreState, StoreError> {
    let mut replay = RebuildStoreState {
        generation: 0,
        mutation_generation: 0,
        updated_at_ms: 0,
    };
    let mut start = 0;
    while start < operations.len() {
        let end = replay_batch_end(operations, start);
        let first = &operations[start];
        let active_before = current_active_revision(transaction, first.memory_id().as_str())?;
        for operation in &operations[start..end] {
            if matches!(
                operation.payload(),
                MemoryOperationPayload::MemoryCreated { .. }
            ) {
                insert_memory_item_shell(transaction, operation)?;
            }
            if let Some(revision) = introduced_revision(operation.payload()) {
                insert_rebuilt_revision(transaction, operation, revision, erasures)?;
            }
            insert_rebuilt_validation(transaction, operation)?;
            apply_rebuild_operation(transaction, operation)?;
            let changed = transaction
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
            if changed != 1 {
                return Err(StoreError::InvalidMemoryTransition);
            }
        }
        let active_after = current_active_revision(transaction, first.memory_id().as_str())?;
        replay.mutation_generation = replay
            .mutation_generation
            .checked_add(1)
            .ok_or(StoreError::SequenceOutOfRange)?;
        if active_before != active_after {
            replay.generation = replay
                .generation
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
        }
        replay.updated_at_ms = replay
            .updated_at_ms
            .max(operations[end - 1].occurred_at().get());
        start = end;
    }

    for erasure in erasures.sessions.values() {
        let eligibility_changed =
            erasure
                .memory_ids
                .iter()
                .try_fold(false, |changed, memory_id| {
                    current_active_revision(transaction, memory_id)
                        .map(|active| changed || active.is_some())
                })?;
        if eligibility_changed {
            replay.generation = replay
                .generation
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
        }
        if !erasure.memory_ids.is_empty() || !erasure.evidence_keys.is_empty() {
            replay.mutation_generation = replay
                .mutation_generation
                .checked_add(1)
                .ok_or(StoreError::SequenceOutOfRange)?;
            replay.updated_at_ms = replay.updated_at_ms.max(erasure.erased_at_ms);
        }
    }
    apply_session_erasure_tombstones(transaction)?;
    Ok(replay)
}

fn replace_memory_store_state(
    transaction: &Transaction<'_>,
    replay: RebuildStoreState,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO memory_store_state (singleton, generation, mutation_generation, updated_at_ms) \
             VALUES (1, ?1, ?2, ?3)",
            params![
                to_sql_sequence(replay.generation)?,
                to_sql_sequence(replay.mutation_generation)?,
                replay.updated_at_ms,
            ],
        )
        .map_err(map_sqlite_error)?;
    Ok(())
}

fn verify_rebuilt_projection(
    transaction: &Transaction<'_>,
    operations: &[MemoryOperationEnvelope],
) -> Result<(), StoreError> {
    let expected_items = operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.payload(),
                MemoryOperationPayload::MemoryCreated { .. }
            )
        })
        .count();
    let expected_revisions = operations
        .iter()
        .filter(|operation| introduced_revision(operation.payload()).is_some())
        .count();
    let expected_validations = operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.payload(),
                MemoryOperationPayload::RevisionValidated { .. }
            )
        })
        .count();
    let expected_evidence = operations
        .iter()
        .filter_map(|operation| introduced_revision(operation.payload()))
        .map(|revision| revision.evidence().len())
        .sum::<usize>();
    let expected_relations = operations
        .iter()
        .filter_map(|operation| introduced_revision(operation.payload()))
        .map(|revision| revision.relations().len())
        .sum::<usize>();
    for (table, expected) in [
        ("memory_items", expected_items),
        ("memory_revisions", expected_revisions),
        ("memory_validations", expected_validations),
        ("memory_evidence", expected_evidence),
        ("memory_relations", expected_relations),
    ] {
        let count = transaction
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(map_sqlite_error)?;
        if usize::try_from(count).ok() != Some(expected) {
            return Err(corrupt_memory_projection());
        }
    }
    let fts_mismatch = transaction
        .query_row(
            "SELECT ( \
                 (SELECT COUNT(*) FROM memory_revision_fts) != \
                 (SELECT COUNT(*) FROM memory_revisions WHERE state = 'active' AND content_id IS NOT NULL) \
                 OR EXISTS ( \
                     SELECT 1 FROM memory_revision_fts AS f \
                     LEFT JOIN memory_revisions AS r ON r.search_rowid = f.rowid \
                     LEFT JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
                     WHERE r.state IS NULL OR r.state != 'active' \
                        OR f.revision_id != r.revision_id OR f.memory_id != r.memory_id \
                        OR f.content != CAST(b.content_utf8 AS TEXT) \
                 ) \
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(map_sqlite_error)?;
    if fts_mismatch {
        return Err(corrupt_memory_projection());
    }
    let mut foreign_keys = transaction
        .prepare("PRAGMA foreign_key_check")
        .map_err(map_sqlite_error)?;
    if foreign_keys.exists([]).map_err(map_sqlite_error)? {
        return Err(corrupt_memory_projection());
    }
    Ok(())
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

pub(crate) fn decode_projected_revision(
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

fn load_latest_revision_validation(
    connection: &rusqlite::Connection,
    memory_id: &MemoryId,
    revision: &MemoryRevision,
) -> Result<Option<MemoryValidationResult>, StoreError> {
    struct ValidationRow {
        sequence: i64,
        operation_id: String,
        envelope_json: Vec<u8>,
        operation_hash: Vec<u8>,
        projected_operation_id: Option<String>,
        projected_revision_id: Option<String>,
        validator_version: Option<i64>,
        content_hash: Option<Vec<u8>>,
        outcome: Option<String>,
        validation_json: Option<Vec<u8>>,
        created_at_ms: Option<i64>,
    }

    let mut statement = connection
        .prepare(
            "SELECT o.sequence, o.operation_id, o.envelope_json, o.operation_sha256, \
                    v.operation_id, v.revision_id, v.validator_version, v.content_sha256, \
                    v.outcome, v.validation_json, v.created_at_ms \
             FROM memory_operations AS o \
             LEFT JOIN memory_validations AS v ON v.operation_id = o.operation_id \
             WHERE o.memory_id = ?1 AND o.revision_id = ?2 \
                   AND o.operation_kind = 'validated' \
             ORDER BY o.sequence ASC",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map(
            params![memory_id.as_str(), revision.revision_id().as_str()],
            |row| {
                Ok(ValidationRow {
                    sequence: row.get(0)?,
                    operation_id: row.get(1)?,
                    envelope_json: row.get(2)?,
                    operation_hash: row.get(3)?,
                    projected_operation_id: row.get(4)?,
                    projected_revision_id: row.get(5)?,
                    validator_version: row.get(6)?,
                    content_hash: row.get(7)?,
                    outcome: row.get(8)?,
                    validation_json: row.get(9)?,
                    created_at_ms: row.get(10)?,
                })
            },
        )
        .map_err(map_sqlite_error)?;

    let mut latest = None;
    let mut authoritative_count = 0_usize;
    for row in rows {
        let row = row.map_err(map_sqlite_error)?;
        if Sha256::digest(&row.envelope_json).as_slice() != row.operation_hash.as_slice() {
            return Err(corrupt_memory_ledger());
        }
        let operation: MemoryOperationEnvelope =
            serde_json::from_slice(&row.envelope_json).map_err(|_| corrupt_memory_ledger())?;
        let sequence = u64::try_from(row.sequence).map_err(|_| corrupt_memory_ledger())?;
        let MemoryOperationPayload::RevisionValidated {
            revision_id,
            validation,
        } = operation.payload()
        else {
            return Err(corrupt_memory_ledger());
        };
        if operation.operation_id().as_str() != row.operation_id
            || operation.memory_id() != memory_id
            || operation.sequence().get() != sequence
            || operation.schema_version() != MEMORY_SCHEMA_V1
            || revision_id != revision.revision_id()
            || validation.content_hash() != revision.content_hash()
        {
            return Err(corrupt_memory_ledger());
        }

        let (
            Some(projected_operation_id),
            Some(projected_revision_id),
            Some(validator_version),
            Some(content_hash),
            Some(outcome),
            Some(validation_json),
            Some(created_at_ms),
        ) = (
            row.projected_operation_id,
            row.projected_revision_id,
            row.validator_version,
            row.content_hash,
            row.outcome,
            row.validation_json,
            row.created_at_ms,
        )
        else {
            return Err(corrupt_memory_projection());
        };
        let projected: MemoryValidationResult =
            serde_json::from_slice(&validation_json).map_err(|_| corrupt_memory_projection())?;
        let expected_json = serde_json::to_vec(validation).map_err(|_| StoreError::Backend)?;
        if projected_operation_id != row.operation_id
            || projected_revision_id != revision.revision_id().as_str()
            || validator_version != i64::from(validation.validator_version())
            || content_hash.as_slice()
                != digest_bytes(validation.content_hash().as_str())?.as_slice()
            || outcome != encode_validation_status(validation.status())
            || validation_json != expected_json
            || created_at_ms != operation.occurred_at().get()
            || projected != *validation
        {
            return Err(corrupt_memory_projection());
        }
        latest = Some(projected);
        authoritative_count = authoritative_count
            .checked_add(1)
            .ok_or(StoreError::LimitExceeded)?;
    }
    drop(statement);

    let projected_count = connection
        .query_row(
            "SELECT COUNT(*) FROM memory_validations WHERE revision_id = ?1",
            params![revision.revision_id().as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(map_sqlite_error)?;
    if usize::try_from(projected_count).map_err(|_| corrupt_memory_projection())?
        != authoritative_count
    {
        return Err(corrupt_memory_projection());
    }

    Ok(latest)
}

fn append_effective_status_filter(
    sql: &mut String,
    values: &mut Vec<Value>,
    query: &MemoryInspectionQuery,
) -> Result<(), StoreError> {
    if query.effective_statuses().is_empty() {
        return Ok(());
    }
    let as_of = query.as_of().ok_or(StoreError::Backend)?;
    values.push(Value::Integer(as_of.get()));
    let as_of_parameter = values.len();
    let valid = format!(
        "((r.valid_from_ms IS NULL OR r.valid_from_ms <= ?{as_of_parameter}) AND \
          (r.valid_until_ms IS NULL OR r.valid_until_ms > ?{as_of_parameter}))"
    );
    let conflicting = "(instr(CAST(r.metadata_json AS TEXT), '\"kind\":\"contradicts\"') > 0 OR \
         COALESCE(instr(CAST(( \
             SELECT v.validation_json FROM memory_validations AS v \
             JOIN memory_operations AS vo ON vo.operation_id = v.operation_id \
             WHERE v.revision_id = r.revision_id \
             ORDER BY vo.sequence DESC LIMIT 1 \
         ) AS TEXT), '\"contradiction\"'), 0) > 0)";

    sql.push_str(" AND (");
    for (index, status) in query.effective_statuses().iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        match status {
            MemoryInspectionStatus::Active => {
                sql.push_str("(i.lifecycle = 'active' AND ");
                sql.push_str(&valid);
                sql.push_str(" AND NOT ");
                sql.push_str(conflicting);
                sql.push(')');
            }
            MemoryInspectionStatus::Proposed => {
                sql.push_str("(i.lifecycle = 'proposed' AND ");
                sql.push_str(&valid);
                sql.push_str(" AND NOT ");
                sql.push_str(conflicting);
                sql.push(')');
            }
            MemoryInspectionStatus::Conflicting => {
                sql.push_str("(i.lifecycle IN ('active', 'proposed') AND ");
                sql.push_str(conflicting);
                sql.push(')');
            }
            MemoryInspectionStatus::Expired => {
                sql.push_str("(i.lifecycle IN ('active', 'proposed') AND NOT ");
                sql.push_str(conflicting);
                sql.push_str(" AND NOT ");
                sql.push_str(&valid);
                sql.push(')');
            }
            MemoryInspectionStatus::Superseded => sql.push_str("i.lifecycle = 'superseded'"),
            MemoryInspectionStatus::Rejected => sql.push_str("i.lifecycle = 'rejected'"),
            MemoryInspectionStatus::Retracted => sql.push_str("i.lifecycle = 'retracted'"),
            MemoryInspectionStatus::Deleted => sql.push_str("i.lifecycle = 'deleted'"),
        }
    }
    sql.push(')');
    Ok(())
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
        AttemptId, CommandId, ConfidenceBasisPoints, ContextEpochId, ContextTokenBudget,
        ContextTurnId, CorrelationId, EstimatedTokens, InputId, MemoryEvidence,
        MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryEvidenceRelation, MemoryEvidenceSource,
        MemoryId, MemoryOperationId, MemoryOrigin, MemoryRelation, MemoryRelationKind,
        MemoryRevisionDraft, MemoryRevisionNumber, MemorySequence, MemorySubjectKey,
        MemoryValidationIssue, MemoryValidationResult, MemoryValidationStatus, ModelId, ModelRef,
        ProviderId, SessionId, SessionSequence, Sha256Digest, TimestampMillis, TrustClass, UserId,
    };
    use autoharness_memory::{
        ContextBuildRequest, ContextBuilder, MemoryCandidate, RetrievalScope,
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
        revision_with_relations(
            revision_id,
            revision_number,
            content,
            status,
            subject_key,
            sensitivity,
            evidence,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn revision_with_relations(
        revision_id: &str,
        revision_number: u64,
        content: &str,
        status: MemoryRevisionStatus,
        subject_key: Option<&str>,
        sensitivity: Sensitivity,
        evidence: Vec<MemoryEvidence>,
        relations: Vec<MemoryRelation>,
    ) -> (MemoryRevision, MemoryRevisionContent) {
        revision_with_validity(
            revision_id,
            revision_number,
            content,
            status,
            subject_key,
            sensitivity,
            MemoryValidity::Indefinite,
            evidence,
            relations,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn revision_with_validity(
        revision_id: &str,
        revision_number: u64,
        content: &str,
        status: MemoryRevisionStatus,
        subject_key: Option<&str>,
        sensitivity: Sensitivity,
        validity: MemoryValidity,
        evidence: Vec<MemoryEvidence>,
        relations: Vec<MemoryRelation>,
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
            validity,
            evidence,
            relations,
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

    fn rebuild_fts_in_physical_order(store: &SqliteStore, descending: bool) {
        store
            .connection
            .execute("DELETE FROM memory_revision_fts", [])
            .expect("clear FTS projection");
        let sql = if descending {
            "INSERT INTO memory_revision_fts (rowid, content, revision_id, memory_id) \
             SELECT r.search_rowid, CAST(b.content_utf8 AS TEXT), r.revision_id, r.memory_id \
             FROM memory_revisions AS r \
             JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE r.state = 'active' ORDER BY r.search_rowid DESC"
        } else {
            "INSERT INTO memory_revision_fts (rowid, content, revision_id, memory_id) \
             SELECT r.search_rowid, CAST(b.content_utf8 AS TEXT), r.revision_id, r.memory_id \
             FROM memory_revisions AS r \
             JOIN memory_content_blobs AS b ON b.content_id = r.content_id \
             WHERE r.state = 'active' ORDER BY r.search_rowid ASC"
        };
        store
            .connection
            .execute(sql, [])
            .expect("rebuild FTS projection");
    }

    fn context_candidate(candidate: &MemorySearchCandidate) -> MemoryCandidate {
        let revision = candidate.revision();
        let lexical_basis_points = match candidate.memory_id().as_str() {
            "memory-m-near" => 10_000,
            "memory-a-tie" | "memory-b-tie" => 9_000,
            "memory-z-verbose" => 8_000,
            _ => 0,
        };
        MemoryCandidate {
            memory_id: candidate.memory_id().clone(),
            revision_id: revision.revision_id().clone(),
            status: revision.status(),
            scope: candidate.scope().clone(),
            kind: candidate.memory_kind(),
            trust: revision.trust_class(),
            confidence: revision.confidence(),
            sensitivity: revision.sensitivity(),
            validity: revision.validity(),
            content: candidate.content().clone(),
            content_hash: revision.content_hash().clone(),
            created_at: revision.created_at(),
            exact_match: false,
            lexical_basis_points,
            conflicted: false,
        }
    }

    fn context_request(
        generation: MemoryGeneration,
        memory_candidates: Vec<MemoryCandidate>,
        durable_memory_limit: u64,
    ) -> ContextBuildRequest {
        ContextBuildRequest {
            context_turn_id: ContextTurnId::new("context-turn-fts-order").expect("turn ID"),
            epoch_id: ContextEpochId::new("context-epoch-fts-order").expect("epoch ID"),
            session_id: SessionId::new("session-fts-order").expect("session ID"),
            attempt_id: AttemptId::new("attempt-fts-order").expect("attempt ID"),
            run_turn: 1,
            expected_session_sequence: SessionSequence::FIRST,
            memory_generation: generation,
            model: ModelRef::new(
                ProviderId::new("provider-fts-order").expect("provider ID"),
                ModelId::new("model-fts-order").expect("model ID"),
            ),
            token_budget: ContextTokenBudget::new(100_000).expect("token budget"),
            reserved_tokens: EstimatedTokens::new(0).expect("reserved tokens"),
            durable_memory_limit: EstimatedTokens::new(durable_memory_limit).expect("memory limit"),
            committed_at: TimestampMillis::new(20),
            retrieval_scope: RetrievalScope {
                user_id: UserId::new("user-1").expect("user ID"),
                workspace_id: WorkspaceId::new("workspace-fts-order").expect("workspace ID"),
                session_id: SessionId::new("session-fts-order").expect("session ID"),
                agent_id: None,
                as_of: TimestampMillis::new(20),
                sensitivity_ceiling: Sensitivity::Internal,
            },
            observed_sources: Vec::new(),
            memory_candidates,
        }
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
    fn evidence_content_read_distinguishes_absent_retained_and_erased_without_debug_leaks() {
        const SECRET_SENTINEL: &str = "authorization: bearer configured-secret-sentinel";

        let database = TestDatabase::new();
        let mut store = database.open();
        let session_id = SessionId::new("session-evidence").expect("session ID");
        let excerpt = MemoryEvidenceExcerpt::new(SECRET_SENTINEL).expect("evidence excerpt");
        let retained_evidence = MemoryEvidence::new(
            MemoryEvidenceId::new("evidence-retained").expect("evidence ID"),
            MemoryEvidenceSource::UserInput {
                session_id: session_id.clone(),
                input_id: InputId::new("input-retained").expect("input ID"),
            },
            MemoryEvidenceRelation::Supports,
            Some(excerpt.clone()),
            Some(raw_digest(excerpt.as_str())),
        )
        .expect("retained evidence");
        let absent_evidence = MemoryEvidence::new(
            MemoryEvidenceId::new("evidence-absent").expect("evidence ID"),
            MemoryEvidenceSource::UserInput {
                session_id,
                input_id: InputId::new("input-absent").expect("input ID"),
            },
            MemoryEvidenceRelation::DerivedFrom,
            None,
            None,
        )
        .expect("evidence without excerpt");
        let (revision, sidecar) = revision_with(
            "revision-evidence-content",
            1,
            "Evidence read models do not follow source references.",
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Internal,
            vec![retained_evidence, absent_evidence],
        );
        let create = create_operation(
            "memory-evidence-content",
            "operation-evidence-content",
            revision.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(0, create, Some(sidecar)))
            .expect("append evidence memory");

        let retained = store
            .load_memory_evidence_content(revision.revision_id())
            .expect("load evidence content")
            .expect("known revision");
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[0].evidence_id().as_str(), "evidence-retained");
        assert_eq!(
            retained[0]
                .excerpt()
                .retained()
                .expect("retained excerpt")
                .as_str(),
            SECRET_SENTINEL
        );
        assert_eq!(retained[1].excerpt(), &MemoryEvidenceExcerptState::Absent);
        assert!(!format!("{retained:?}").contains(SECRET_SENTINEL));
        let inspection_query = MemoryInspectionQuery::new(
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Vec::new(),
            None,
            8,
        )
        .expect("inspection query");
        let inspected = store
            .inspect_memory_page(&inspection_query)
            .expect("inspect retained evidence");
        assert_eq!(inspected.records().len(), 1);
        assert_eq!(inspected.records()[0].evidence_content(), retained);
        let inspected_debug = format!("{:?}", inspected.records()[0]);
        assert!(!inspected_debug.contains(SECRET_SENTINEL));
        assert!(!inspected_debug.contains("Evidence read models do not follow"));
        assert!(
            store
                .load_memory_evidence_content(
                    &MemoryRevisionId::new("revision-unknown").expect("revision ID")
                )
                .expect("load unknown evidence")
                .is_none()
        );

        let delete = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-evidence-delete").expect("operation ID"),
            MemoryId::new("memory-evidence-content").expect("memory ID"),
            MemorySequence::new(2).expect("memory sequence"),
            TimestampMillis::new(20),
            MemoryCausation::Command(
                CommandId::new("command-evidence-delete").expect("command ID"),
            ),
            CorrelationId::new("correlation-evidence-delete").expect("correlation ID"),
            MemoryOperationPayload::MemoryDeleted {
                revision_id: revision.revision_id().clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, delete, None))
            .expect("delete evidence memory");
        let erased = store
            .load_memory_evidence_content(revision.revision_id())
            .expect("load erased evidence")
            .expect("known tombstone");
        assert_eq!(erased[0].excerpt(), &MemoryEvidenceExcerptState::Erased);
        assert_eq!(erased[1].excerpt(), &MemoryEvidenceExcerptState::Absent);
        let inspected = store
            .inspect_memory_page(&inspection_query)
            .expect("inspect erased evidence");
        assert_eq!(inspected.records()[0].evidence_content(), erased);
    }

    #[test]
    fn inspection_reads_latest_validation_from_the_ledger_and_fails_closed_on_damage() {
        const CONTENT_SENTINEL: &str = "private validation projection sentinel";

        let database = TestDatabase::new();
        let mut store = database.open();
        let (revision, sidecar) = revision(
            "revision-inspection-validation",
            1,
            CONTENT_SENTINEL,
            MemoryRevisionStatus::Active,
        );
        let create = create_operation(
            "memory-inspection-validation",
            "operation-inspection-validation-create",
            revision.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(0, create.clone(), Some(sidecar)))
            .expect("append validated memory");
        let validation = MemoryValidationResult::new(
            9,
            revision.content_hash().clone(),
            MemoryValidationStatus::NeedsReview,
            vec![
                MemoryValidationIssue::Contradiction,
                MemoryValidationIssue::InjectionPattern,
            ],
        )
        .expect("validation result");
        let validation_operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-inspection-validation-result").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(11),
            MemoryCausation::Operation(create.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::RevisionValidated {
                revision_id: revision.revision_id().clone(),
                validation: validation.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, validation_operation, None))
            .expect("append validation");
        let query = MemoryInspectionQuery::new(
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Vec::new(),
            None,
            8,
        )
        .expect("inspection query");

        let inspected = store
            .inspect_memory_page(&query)
            .expect("inspect validation");
        assert_eq!(
            inspected.records()[0].latest_validation(),
            Some(&validation)
        );
        assert!(!format!("{:?}", inspected.records()[0]).contains(CONTENT_SENTINEL));

        store
            .connection
            .execute(
                "DELETE FROM memory_validations \
                 WHERE operation_id = 'operation-inspection-validation-result'",
                [],
            )
            .expect("remove validation projection");
        assert!(matches!(
            store.inspect_memory_page(&query),
            Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection
            })
        ));

        store
            .rebuild_memory_projections()
            .expect("rebuild validation projection");
        assert_eq!(
            store
                .inspect_memory_page(&query)
                .expect("inspect rebuilt validation")
                .records()[0]
                .latest_validation(),
            Some(&validation)
        );

        store
            .connection
            .execute(
                "UPDATE memory_validations SET validation_json = x'7b7d' \
                 WHERE operation_id = 'operation-inspection-validation-result'",
                [],
            )
            .expect("damage validation JSON");
        assert!(matches!(
            store.inspect_memory_page(&query),
            Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection
            })
        ));
    }

    #[test]
    fn effective_status_filters_conflicts_and_expiry_before_the_page_limit() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let scope = MemoryScope::User(UserId::new("user-1").expect("user ID"));
        let as_of = TimestampMillis::new(20);

        let (conflicting_revision, conflicting_sidecar) = revision(
            "revision-effective-conflicting",
            1,
            "Older contradictory proposal remains discoverable.",
            MemoryRevisionStatus::Proposed,
        );
        let conflicting_create = create_operation(
            "memory-effective-conflicting",
            "operation-effective-conflicting-create",
            conflicting_revision.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                conflicting_create.clone(),
                Some(conflicting_sidecar),
            ))
            .expect("append conflicting proposal");
        let contradiction = MemoryValidationResult::new(
            1,
            conflicting_revision.content_hash().clone(),
            MemoryValidationStatus::NeedsReview,
            vec![MemoryValidationIssue::Contradiction],
        )
        .expect("contradiction validation");
        store
            .append_memory(&MemoryAppendRequest::new(
                1,
                MemoryOperationEnvelope::new_v1(
                    MemoryOperationId::new("operation-effective-conflicting-validation")
                        .expect("operation ID"),
                    conflicting_create.memory_id().clone(),
                    MemorySequence::new(2).expect("sequence"),
                    TimestampMillis::new(11),
                    MemoryCausation::Operation(conflicting_create.operation_id().clone()),
                    conflicting_create.correlation_id().clone(),
                    MemoryOperationPayload::RevisionValidated {
                        revision_id: conflicting_revision.revision_id().clone(),
                        validation: contradiction,
                    },
                ),
                None,
            ))
            .expect("append contradiction validation");

        let (expired_revision, expired_sidecar) = revision_with_validity(
            "revision-effective-expired",
            1,
            "Older expired fact remains discoverable.",
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Internal,
            MemoryValidity::Until { valid_until: as_of },
            Vec::new(),
            Vec::new(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-effective-expired",
                    "operation-effective-expired",
                    expired_revision,
                ),
                Some(expired_sidecar),
            ))
            .expect("append expired memory");

        for index in 0..4 {
            let (revision, sidecar) = revision(
                &format!("revision-effective-active-{index}"),
                1,
                &format!("Newer active distraction {index}"),
                MemoryRevisionStatus::Active,
            );
            store
                .append_memory(&MemoryAppendRequest::new(
                    0,
                    create_operation(
                        &format!("memory-effective-active-{index}"),
                        &format!("operation-effective-active-{index}"),
                        revision,
                    ),
                    Some(sidecar),
                ))
                .expect("append active distractor");
        }
        store
            .connection
            .execute(
                "UPDATE memory_items SET updated_at_ms = CASE memory_id \
                     WHEN 'memory-effective-conflicting' THEN 2 \
                     WHEN 'memory-effective-expired' THEN 1 \
                     ELSE 100 END",
                [],
            )
            .expect("establish physical order");

        let conflicting = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(vec![scope.clone()], Vec::new(), None, 1)
                    .expect("conflicting query")
                    .with_effective_statuses(vec![MemoryInspectionStatus::Conflicting], as_of),
            )
            .expect("inspect conflicting state");
        assert_eq!(conflicting.records().len(), 1);
        assert_eq!(
            conflicting.records()[0].memory_id().as_str(),
            "memory-effective-conflicting"
        );

        let expired = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(vec![scope.clone()], Vec::new(), None, 1)
                    .expect("expired query")
                    .with_effective_statuses(vec![MemoryInspectionStatus::Expired], as_of),
            )
            .expect("inspect expired state");
        assert_eq!(expired.records().len(), 1);
        assert_eq!(
            expired.records()[0].memory_id().as_str(),
            "memory-effective-expired"
        );

        let active = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(vec![scope], Vec::new(), None, 8)
                    .expect("active query")
                    .with_effective_statuses(vec![MemoryInspectionStatus::Active], as_of),
            )
            .expect("inspect eligible active state");
        assert_eq!(active.records().len(), 4);
        assert!(active.records().iter().all(|record| {
            record
                .memory_id()
                .as_str()
                .starts_with("memory-effective-active-")
        }));
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
    fn full_rebuild_recreates_every_projection_family_and_is_idempotent() {
        let database = TestDatabase::new();
        let mut store = database.open();

        let (target_revision, target_sidecar) = revision(
            "revision-z-rebuild-target",
            1,
            "Target memory inserted before the lexically earlier item.",
            MemoryRevisionStatus::Active,
        );
        let target_create = create_operation(
            "memory-z-rebuild-target",
            "operation-z-rebuild-target",
            target_revision.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                target_create.clone(),
                Some(target_sidecar),
            ))
            .expect("append target memory first");

        let excerpt =
            MemoryEvidenceExcerpt::new("retained exact rebuild excerpt").expect("evidence excerpt");
        let retained_evidence = MemoryEvidence::new(
            MemoryEvidenceId::new("evidence-rebuild-retained").expect("evidence ID"),
            MemoryEvidenceSource::UserInput {
                session_id: SessionId::new("session-rebuild-source").expect("session ID"),
                input_id: InputId::new("input-rebuild-source").expect("input ID"),
            },
            MemoryEvidenceRelation::Supports,
            Some(excerpt.clone()),
            Some(raw_digest(excerpt.as_str())),
        )
        .expect("retained evidence");
        let absent_evidence = MemoryEvidence::new(
            MemoryEvidenceId::new("evidence-rebuild-absent").expect("evidence ID"),
            MemoryEvidenceSource::ImportedDocument {
                source_key: autoharness_domain::ContextSourceKey::new("document-rebuild")
                    .expect("source key"),
                source_revision: raw_digest("document revision"),
            },
            MemoryEvidenceRelation::DerivedFrom,
            None,
            None,
        )
        .expect("absent evidence");
        let (proposed_revision, proposed_sidecar) = revision_with_relations(
            "revision-a-rebuild",
            1,
            "Full replay repairs item revision evidence relation validation generation and FTS.",
            MemoryRevisionStatus::Proposed,
            Some("rebuild:all-projections"),
            Sensitivity::Internal,
            vec![retained_evidence, absent_evidence],
            vec![MemoryRelation::new(
                target_create.memory_id().clone(),
                MemoryRelationKind::Related,
            )],
        );
        let create = create_operation(
            "memory-a-rebuild",
            "operation-a-rebuild-create",
            proposed_revision.clone(),
        );
        let validate = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-a-rebuild-validate").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(11),
            MemoryCausation::Operation(create.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::RevisionValidated {
                revision_id: proposed_revision.revision_id().clone(),
                validation: MemoryValidationResult::new(
                    7,
                    proposed_revision.content_hash().clone(),
                    MemoryValidationStatus::Accepted,
                    Vec::new(),
                )
                .expect("validation"),
            },
        );
        let activate = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-a-rebuild-activate").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(3).expect("sequence"),
            TimestampMillis::new(12),
            MemoryCausation::Operation(validate.operation_id().clone()),
            create.correlation_id().clone(),
            MemoryOperationPayload::RevisionActivated {
                revision_id: proposed_revision.revision_id().clone(),
            },
        );
        store
            .append_memory_batch(&MemoryAppendBatchRequest::new(
                0,
                vec![
                    MemoryAppendOperation::new(create.clone(), Some(proposed_sidecar)),
                    MemoryAppendOperation::new(validate, None),
                    MemoryAppendOperation::new(activate, None),
                ],
            ))
            .expect("append logical proposal batch after target");
        let expected_generation = store.memory_generation().expect("generation");
        let expected_mutation = store
            .memory_mutation_generation()
            .expect("mutation generation");
        assert_eq!(expected_generation.get(), 2);
        assert_eq!(expected_mutation.get(), 2);

        store
            .connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF; \
                 DELETE FROM memory_items WHERE memory_id = 'memory-z-rebuild-target'; \
                 DELETE FROM memory_revisions WHERE revision_id = 'revision-z-rebuild-target'; \
                 UPDATE memory_items SET lifecycle = 'rejected', latest_revision = 77, \
                     latest_revision_id = NULL, active_revision_id = NULL, last_sequence = 91 \
                     WHERE memory_id = 'memory-a-rebuild'; \
                 UPDATE memory_revisions SET state = 'rejected', content_id = NULL, \
                     metadata_json = x'00', metadata_sha256 = zeroblob(32) \
                     WHERE revision_id = 'revision-a-rebuild'; \
                 DELETE FROM memory_evidence; \
                 DELETE FROM memory_relations; \
                 DELETE FROM memory_validations; \
                 DELETE FROM memory_revision_fts; \
                 INSERT INTO memory_revision_fts (rowid, content, revision_id, memory_id) \
                     VALUES (900001, 'bogus damaged index row', 'bogus-revision', 'bogus-memory'); \
                 DELETE FROM memory_store_state; \
                 PRAGMA foreign_keys = ON;",
            )
            .expect("damage every derived projection family");

        store
            .rebuild_memory_projections()
            .expect("full projection replay");
        assert_eq!(
            store.memory_generation().expect("rebuilt generation"),
            expected_generation
        );
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("rebuilt mutation generation"),
            expected_mutation
        );
        assert_eq!(
            store
                .load_memory_revisions(target_create.memory_id())
                .expect("rebuilt target revisions"),
            vec![target_revision]
        );
        let rebuilt = store
            .load_memory_candidate(proposed_revision.revision_id())
            .expect("load rebuilt candidate")
            .expect("rebuilt candidate");
        assert_eq!(rebuilt.revision().status(), MemoryRevisionStatus::Active);
        assert!(matches!(rebuilt.content(), MemoryContentState::Retained(_)));
        let evidence = store
            .load_memory_evidence_content(proposed_revision.revision_id())
            .expect("load rebuilt evidence")
            .expect("rebuilt evidence");
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence[0]
                .excerpt()
                .retained()
                .expect("retained rebuilt excerpt")
                .as_str(),
            excerpt.as_str()
        );
        assert_eq!(evidence[1].excerpt(), &MemoryEvidenceExcerptState::Absent);
        let relation = store
            .connection
            .query_row(
                "SELECT to_memory_id, relation FROM memory_relations \
                 WHERE revision_id = 'revision-a-rebuild' AND ordinal = 0",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("rebuilt relation");
        assert_eq!(
            relation,
            ("memory-z-rebuild-target".to_owned(), "related".to_owned())
        );
        let validation = store
            .connection
            .query_row(
                "SELECT validator_version, outcome FROM memory_validations \
                 WHERE operation_id = 'operation-a-rebuild-validate'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("rebuilt validation");
        assert_eq!(validation, (7, "accepted".to_owned()));
        let bogus_fts_rows = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_revision_fts WHERE revision_id = 'bogus-revision'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("bogus FTS count");
        assert_eq!(bogus_fts_rows, 0);
        let query = MemorySearchQuery::new(
            MemoryContent::new("full replay repairs").expect("search query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Internal,
            TimestampMillis::new(30),
            8,
        )
        .expect("memory search query");
        assert_eq!(
            store
                .search_memory(&query)
                .expect("rebuilt FTS search")
                .candidates()
                .len(),
            1
        );

        store
            .rebuild_memory_projections()
            .expect("idempotent repeated replay");
        assert_eq!(
            store.memory_generation().expect("stable generation"),
            expected_generation
        );
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("stable mutation generation"),
            expected_mutation
        );
        assert_eq!(
            store
                .load_memory_evidence_content(proposed_revision.revision_id())
                .expect("load evidence after repeated replay")
                .expect("known revision"),
            evidence
        );
        assert_eq!(
            store
                .search_memory(&query)
                .expect("search after repeated replay")
                .candidates()
                .len(),
            1
        );
    }

    #[test]
    fn rebuild_fails_closed_when_a_retained_sidecar_is_missing() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (revision, sidecar) = revision(
            "revision-missing-rebuild-sidecar",
            1,
            "Retained bytes must never be invented by replay.",
            MemoryRevisionStatus::Active,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-missing-rebuild-sidecar",
                    "operation-missing-rebuild-sidecar",
                    revision.clone(),
                ),
                Some(sidecar),
            ))
            .expect("append retained memory");
        store
            .connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF; \
                 DELETE FROM memory_content_blobs \
                     WHERE content_id = 'memory-content:revision-missing-rebuild-sidecar'; \
                 PRAGMA foreign_keys = ON;",
            )
            .expect("remove authoritative retained sidecar");

        assert!(matches!(
            store.rebuild_memory_projections(),
            Err(StoreError::CorruptData {
                area: CorruptionArea::MemoryProjection
            })
        ));
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT lifecycle FROM memory_items \
                     WHERE memory_id = 'memory-missing-rebuild-sidecar'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("original projection remains"),
            "active"
        );
    }

    #[test]
    fn replay_failure_rolls_back_projection_clear_atomically() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let (revision, sidecar) = revision(
            "revision-replay-rollback",
            1,
            "Replay rollback retains the pre-rebuild projection on failure.",
            MemoryRevisionStatus::Active,
        );
        let create = create_operation(
            "memory-replay-rollback",
            "operation-replay-rollback-create",
            revision,
        );
        store
            .append_memory(&MemoryAppendRequest::new(0, create.clone(), Some(sidecar)))
            .expect("append active memory");
        let invalid = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-replay-rollback-invalid").expect("operation ID"),
            create.memory_id().clone(),
            MemorySequence::new(2).expect("sequence"),
            TimestampMillis::new(20),
            MemoryCausation::Operation(create.operation_id().clone()),
            CorrelationId::new("correlation-replay-rollback-invalid").expect("correlation ID"),
            MemoryOperationPayload::RevisionActivated {
                revision_id: MemoryRevisionId::new("revision-never-introduced")
                    .expect("revision ID"),
            },
        );
        let encoded = encode_and_validate(&MemoryAppendRequest::new(1, invalid.clone(), None))
            .expect("encode structurally valid invalid transition");
        let transaction = store.connection.transaction().expect("ledger transaction");
        insert_operation(&transaction, &invalid, &encoded).expect("inject invalid ledger fact");
        transaction
            .commit()
            .expect("commit injected replay failure");
        store
            .connection
            .execute(
                "UPDATE memory_items SET updated_at_ms = 777 \
                 WHERE memory_id = 'memory-replay-rollback'",
                [],
            )
            .expect("mark pre-rebuild projection");
        store
            .connection
            .execute("DELETE FROM memory_revision_fts", [])
            .expect("damage FTS before replay");

        assert_eq!(
            store.rebuild_memory_projections(),
            Err(StoreError::InvalidMemoryTransition)
        );
        let preserved = store
            .connection
            .query_row(
                "SELECT last_sequence, updated_at_ms, lifecycle FROM memory_items \
                 WHERE memory_id = 'memory-replay-rollback'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("projection survives failed replay");
        assert_eq!(preserved, (1, 777, "active".to_owned()));
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM memory_revision_fts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("damaged FTS remains after rollback"),
            0
        );
        assert_eq!(store.memory_generation().expect("generation").get(), 1);
        assert_eq!(
            store
                .memory_mutation_generation()
                .expect("mutation generation")
                .get(),
            1
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
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM memory_content_blobs", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("content count after rebuild"),
            0
        );
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
    fn workspace_inspection_searches_literal_content_and_ids_before_pagination() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let literal = "needle '\" OR NOT * Ω <system>\n-- %_ [x]";
        let (matching, matching_sidecar) = revision(
            "revision-old-literal-match",
            1,
            &format!("older exact content: {literal}"),
            MemoryRevisionStatus::Active,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-old-literal-match",
                    "operation-old-literal-match",
                    matching,
                ),
                Some(matching_sidecar),
            ))
            .expect("append older match");
        store
            .connection
            .execute(
                "UPDATE memory_items SET updated_at_ms = 1 \
                 WHERE memory_id = 'memory-old-literal-match'",
                [],
            )
            .expect("age matching item");

        for index in 0..=autoharness_store::MAX_MEMORY_INSPECTION_PAGE_SIZE {
            let revision_id = format!("revision-newer-distractor-{index:03}");
            let memory_id = format!("memory-newer-distractor-{index:03}");
            let operation_id = format!("operation-newer-distractor-{index:03}");
            let (revision, sidecar) = revision(
                &revision_id,
                1,
                &format!("ordinary newer distractor {index}"),
                MemoryRevisionStatus::Active,
            );
            store
                .append_memory(&MemoryAppendRequest::new(
                    0,
                    create_operation(&memory_id, &operation_id, revision),
                    Some(sidecar),
                ))
                .expect("append distractor");
            store
                .connection
                .execute(
                    "UPDATE memory_items SET updated_at_ms = ?2 WHERE memory_id = ?1",
                    params![memory_id, i64::from(index) + 100],
                )
                .expect("order distractor");
        }

        let (sensitive, sensitive_sidecar) = revision_with(
            "revision-sensitive-inspection",
            1,
            literal,
            MemoryRevisionStatus::Active,
            None,
            Sensitivity::Sensitive,
            Vec::new(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-sensitive-inspection",
                    "operation-sensitive-inspection",
                    sensitive,
                ),
                Some(sensitive_sidecar),
            ))
            .expect("append sensitive literal match");

        let proposed_literal = "untrusted proposal literal Ω";
        let (proposed, proposed_sidecar) = revision(
            "revision-proposed-inspection",
            1,
            proposed_literal,
            MemoryRevisionStatus::Proposed,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-proposed-inspection",
                    "operation-proposed-inspection",
                    proposed,
                ),
                Some(proposed_sidecar),
            ))
            .expect("append proposed literal match");

        let scope = MemoryScope::User(UserId::new("user-1").expect("user ID"));
        let query = MemoryInspectionQuery::new(
            vec![scope.clone()],
            vec![MemoryRevisionStatus::Active],
            None,
            8,
        )
        .expect("inspection query")
        .with_memory_kind(MemoryKind::Fact)
        .with_sensitivity_ceiling(Sensitivity::Internal)
        .with_literal_search(MemoryContent::new(literal).expect("literal search"));
        let page = store
            .inspect_memory_page(&query)
            .expect("literal inspection page");
        assert!(!page.has_more());
        assert_eq!(page.records().len(), 1);
        assert_eq!(
            page.records()[0].memory_id(),
            &MemoryId::new("memory-old-literal-match").expect("memory ID")
        );

        let id_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(
                    vec![scope.clone()],
                    vec![MemoryRevisionStatus::Active],
                    None,
                    4,
                )
                .expect("ID query")
                .with_literal_search(MemoryContent::new("old-literal-match").expect("ID literal")),
            )
            .expect("ID inspection page");
        assert_eq!(id_page.records().len(), 1);
        assert_eq!(
            id_page.records()[0].memory_id().as_str(),
            "memory-old-literal-match"
        );

        let proposed_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(
                    vec![scope.clone()],
                    vec![MemoryRevisionStatus::Proposed],
                    None,
                    4,
                )
                .expect("proposal query")
                .with_literal_search(
                    MemoryContent::new(proposed_literal).expect("proposal literal"),
                ),
            )
            .expect("proposed inspection page");
        assert_eq!(proposed_page.records().len(), 1);
        assert_eq!(
            proposed_page.records()[0].lifecycle(),
            MemoryRevisionStatus::Proposed
        );
        assert_eq!(
            proposed_page.records()[0]
                .content()
                .expect("retained proposal")
                .as_str(),
            proposed_literal
        );

        let first_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(
                    vec![scope.clone()],
                    vec![MemoryRevisionStatus::Active],
                    None,
                    8,
                )
                .expect("first page query")
                .with_sensitivity_ceiling(Sensitivity::Internal),
            )
            .expect("first page");
        assert_eq!(first_page.records().len(), 8);
        assert!(first_page.has_more());
        let last = first_page.records().last().expect("last first-page row");
        let cursor = autoharness_store::MemoryInspectionCursor::new(
            last.updated_at(),
            last.memory_id().clone(),
        );
        let second_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(
                    vec![scope.clone()],
                    vec![MemoryRevisionStatus::Active],
                    Some(cursor),
                    8,
                )
                .expect("second page query")
                .with_sensitivity_ceiling(Sensitivity::Internal),
            )
            .expect("second page");
        assert!(first_page.records().iter().all(|first| {
            second_page
                .records()
                .iter()
                .all(|second| first.memory_id() != second.memory_id())
        }));

        let deleted_sentinel = "erased-only workspace search sentinel";
        let (deleted_revision, deleted_sidecar) = revision(
            "revision-deleted-inspection",
            1,
            deleted_sentinel,
            MemoryRevisionStatus::Active,
        );
        let deleted_create = create_operation(
            "memory-deleted-inspection",
            "operation-deleted-inspection",
            deleted_revision.clone(),
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                deleted_create.clone(),
                Some(deleted_sidecar),
            ))
            .expect("append deletable inspection item");
        store
            .append_memory(&MemoryAppendRequest::new(
                1,
                MemoryOperationEnvelope::new_v1(
                    MemoryOperationId::new("operation-delete-inspection").expect("operation ID"),
                    deleted_create.memory_id().clone(),
                    MemorySequence::new(2).expect("memory sequence"),
                    TimestampMillis::new(20),
                    MemoryCausation::Command(
                        CommandId::new("command-delete-inspection").expect("command ID"),
                    ),
                    CorrelationId::new("correlation-delete-inspection").expect("correlation ID"),
                    MemoryOperationPayload::MemoryDeleted {
                        revision_id: deleted_revision.revision_id().clone(),
                    },
                ),
                None,
            ))
            .expect("delete inspection item");
        let erased_content_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(vec![scope.clone()], Vec::new(), None, 8)
                    .expect("erased-content query")
                    .with_literal_search(
                        MemoryContent::new(deleted_sentinel).expect("deleted literal"),
                    ),
            )
            .expect("erased-content page");
        assert!(erased_content_page.records().is_empty());
        let tombstone_page = store
            .inspect_memory_page(
                &MemoryInspectionQuery::new(
                    vec![scope],
                    vec![MemoryRevisionStatus::Deleted],
                    None,
                    8,
                )
                .expect("tombstone query")
                .with_literal_search(
                    MemoryContent::new("memory-deleted-inspection").expect("tombstone ID"),
                ),
            )
            .expect("tombstone page");
        assert_eq!(tombstone_page.records().len(), 1);
        assert!(tombstone_page.records()[0].content().is_none());
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
            .connection
            .execute(
                "DELETE FROM memory_evidence WHERE evidence_id = 'evidence-session-input'",
                [],
            )
            .expect("damage erased evidence projection");
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
        assert_eq!(
            store
                .load_memory_evidence_content(cross_scope_revision.revision_id())
                .expect("load rebuilt cross-scope evidence")
                .expect("known cross-scope revision")[0]
                .excerpt(),
            &MemoryEvidenceExcerptState::Erased
        );
        assert!(matches!(
            store
                .load_memory_candidate(cross_scope_revision.revision_id())
                .expect("load rebuilt cross-scope candidate")
                .expect("known cross-scope memory")
                .content(),
            MemoryContentState::Retained(_)
        ));
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

    #[test]
    fn fts_physical_rebuild_order_cannot_change_ranking_or_context_fit() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let memories = [
            (
                "memory-z-verbose",
                "revision-z-verbose",
                "operation-z-verbose",
                "nexus candidate with verbose unrelated filler words for lower relevance",
            ),
            (
                "memory-b-tie",
                "revision-b-tie",
                "operation-b-tie",
                "nexus stable tie context",
            ),
            (
                "memory-m-near",
                "revision-m-near",
                "operation-m-near",
                "nexus nexus stable near context",
            ),
            (
                "memory-a-tie",
                "revision-a-tie",
                "operation-a-tie",
                "nexus stable tie context",
            ),
        ];
        for (memory_id, revision_id, operation_id, content) in memories {
            let (revision, sidecar) =
                revision(revision_id, 1, content, MemoryRevisionStatus::Active);
            store
                .append_memory(&MemoryAppendRequest::new(
                    0,
                    create_operation(memory_id, operation_id, revision),
                    Some(sidecar),
                ))
                .expect("append FTS candidate");
        }
        store
            .rebuild_memory_projections()
            .expect("rebuild authoritative projections");

        let query = MemorySearchQuery::new(
            MemoryContent::new("nexus").expect("query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Internal,
            TimestampMillis::new(20),
            8,
        )
        .expect("search query");
        rebuild_fts_in_physical_order(&store, false);
        let ascending = store.search_memory(&query).expect("ascending search");
        rebuild_fts_in_physical_order(&store, true);
        let descending = store.search_memory(&query).expect("descending search");

        let signatures = |batch: &MemoryCandidateBatch| {
            batch
                .candidates()
                .iter()
                .map(|candidate| {
                    (
                        candidate.memory_id().as_str().to_owned(),
                        candidate.revision().revision_id().as_str().to_owned(),
                        candidate.fts_rank(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let ascending_signatures = signatures(&ascending);
        assert_eq!(ascending_signatures, signatures(&descending));
        assert_eq!(ascending_signatures.len(), 4);
        let tie_a = ascending_signatures
            .iter()
            .position(|(memory_id, _, _)| memory_id == "memory-a-tie")
            .expect("first tied candidate");
        let tie_b = ascending_signatures
            .iter()
            .position(|(memory_id, _, _)| memory_id == "memory-b-tie")
            .expect("second tied candidate");
        assert_eq!(tie_b, tie_a + 1);

        let ascending_candidates = ascending
            .candidates()
            .iter()
            .map(context_candidate)
            .collect::<Vec<_>>();
        let mut reversed_candidates = descending
            .candidates()
            .iter()
            .map(context_candidate)
            .collect::<Vec<_>>();
        reversed_candidates.reverse();
        let generous = ContextBuilder::default()
            .build(context_request(
                ascending.generation(),
                ascending_candidates.clone(),
                100_000,
            ))
            .expect("generous context build");
        let expected_order = [
            "revision-m-near",
            "revision-a-tie",
            "revision-b-tie",
            "revision-z-verbose",
        ];
        assert_eq!(
            generous
                .selected_memories()
                .iter()
                .map(|memory| memory.revision_id.as_str())
                .collect::<Vec<_>>(),
            expected_order
        );
        let three_item_limit = generous.selected_memories()[..3]
            .iter()
            .map(|memory| memory.estimated_tokens.get())
            .sum();
        let ascending_fit = ContextBuilder::default()
            .build(context_request(
                ascending.generation(),
                ascending_candidates,
                three_item_limit,
            ))
            .expect("ascending context fit");
        let reversed_fit = ContextBuilder::default()
            .build(context_request(
                descending.generation(),
                reversed_candidates,
                three_item_limit,
            ))
            .expect("reversed context fit");
        let selected = |built: &autoharness_memory::BuiltContext| {
            built
                .selected_memories()
                .iter()
                .map(|memory| memory.revision_id.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            selected(&ascending_fit),
            vec![
                "revision-m-near".to_owned(),
                "revision-a-tie".to_owned(),
                "revision-b-tie".to_owned(),
            ]
        );
        assert_eq!(selected(&ascending_fit), selected(&reversed_fit));
        assert_eq!(ascending_fit.prelude(), reversed_fit.prelude());
        assert_eq!(ascending_fit.rendered_hash(), reversed_fit.rendered_hash());
    }

    #[test]
    fn active_fts_literalizes_operator_control_and_unicode_query_text() {
        let database = TestDatabase::new();
        let mut store = database.open();
        let target_content =
            "quoted OR NOT NEAR prefix column value system control 雪 Ω café naïve target";
        let (target, target_sidecar) = revision(
            "revision-unicode-literal",
            1,
            target_content,
            MemoryRevisionStatus::Active,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-unicode-literal",
                    "operation-unicode-literal",
                    target,
                ),
                Some(target_sidecar),
            ))
            .expect("append Unicode target");
        let (distractor, distractor_sidecar) = revision(
            "revision-unrelated-literal",
            1,
            "completely unrelated searchable memory",
            MemoryRevisionStatus::Active,
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                create_operation(
                    "memory-unrelated-literal",
                    "operation-unrelated-literal",
                    distractor,
                ),
                Some(distractor_sidecar),
            ))
            .expect("append distractor");

        let hostile_literal =
            "\"quoted OR NOT NEAR(prefix*) column:value <system>\u{0007}\ncontrol 雪 Ω café naïve";
        let query = MemorySearchQuery::new(
            MemoryContent::new(hostile_literal).expect("hostile literal query"),
            vec![MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            Sensitivity::Internal,
            TimestampMillis::new(20),
            8,
        )
        .expect("search query");
        let batch = store
            .search_memory(&query)
            .expect("safe literal FTS search");

        assert_eq!(batch.candidates().len(), 1);
        assert_eq!(
            batch.candidates()[0].memory_id(),
            &MemoryId::new("memory-unicode-literal").expect("memory ID")
        );
        assert_eq!(batch.candidates()[0].content().as_str(), target_content);
    }
}
