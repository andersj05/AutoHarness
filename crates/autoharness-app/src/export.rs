//! Provider-neutral session export for pre-deletion archival.
//!
//! The export format is documented in `docs/architecture/SESSION_EXPORT.md`
//! and is intentionally derived from authoritative durable events plus
//! contentless context and memory ledgers with their retained sidecars.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use autoharness_domain::{
    ContextEpochId, EventEnvelope, EventPayload, MemoryId, MemoryScope, Sensitivity, SessionId,
};
use autoharness_store::{
    ContextStore as _, DEFAULT_EVENT_PAGE_SIZE, MAX_MEMORY_INSPECTION_PAGE_SIZE,
    MemoryAdmissionCursor, MemoryAdmissionKey, MemoryAdmissionQuery, MemoryInspectionCursor,
    MemoryInspectionQuery, MemoryStore as _, SessionStore as _, SessionSummary,
};
use autoharness_store_sqlite::SqliteStore;
use uuid::Uuid;

use crate::error::AppError;

/// Schema version of the exported JSON document.
pub const EXPORT_SCHEMA_VERSION: u32 = 2;
/// Schema version of one standalone memory export.
pub const MEMORY_EXPORT_SCHEMA_VERSION: u32 = 1;

/// Writes one provider-neutral JSON export for a session.
///
/// The file is written atomically (temporary file plus rename) so a failed
/// export never leaves a truncated archive beside the durable database, and
/// the caller can delete the session only after this returns successfully.
///
/// Returns the path of the written export.
pub fn export_session(
    store: &mut SqliteStore,
    summary: &SessionSummary,
    output_directory: &Path,
) -> Result<PathBuf, AppError> {
    let events = load_all_events(store, summary.session_id())?;
    let contexts = export_contexts(store, &events)?;
    let memories = export_session_memories(store, summary.session_id())?;
    let document = serde_json::json!({
        "schema_version": EXPORT_SCHEMA_VERSION,
        "session": {
            "session_id": summary.session_id().as_str(),
            "status": match summary.status() {
                autoharness_store::SessionStatus::Active => "active",
                autoharness_store::SessionStatus::Archived => "archived",
            },
            "title": summary.title().map(autoharness_domain::SessionTitle::as_str),
            "selected_provider_id": summary
                .selected_model()
                .map(autoharness_domain::ModelRef::provider_id)
                .map(autoharness_domain::ProviderId::as_str),
            "selected_model_id": summary
                .selected_model()
                .map(autoharness_domain::ModelRef::model_id)
                .map(autoharness_domain::ModelId::as_str),
            "created_at_ms": summary.created_at().get(),
            "updated_at_ms": summary.updated_at().get(),
        },
        "event_count": events.len(),
        "events": events,
        "context_audit": contexts,
        "session_memories": memories,
    });

    let encoded = serde_json::to_vec_pretty(&document).map_err(|_| AppError::FileSystem)?;
    let file_name = format!(
        "autoharness-session-{}.export.v{EXPORT_SCHEMA_VERSION}.json",
        safe_export_key(summary.session_id().as_str())
    );
    write_atomic_unique(output_directory, &file_name, &encoded)
}

/// Writes one authorized memory item as a standalone provider-neutral JSON artifact.
pub fn export_memory(
    store: &mut SqliteStore,
    memory_id: &MemoryId,
    authorized_scopes: &[MemoryScope],
    output_directory: &Path,
) -> Result<PathBuf, AppError> {
    let operations = load_all_memory_operations(store, memory_id)?;
    let scope = operations
        .iter()
        .find_map(|operation| match operation.payload() {
            autoharness_domain::MemoryOperationPayload::MemoryCreated { scope, .. } => {
                Some(scope.clone())
            }
            _ => None,
        });
    if scope
        .as_ref()
        .is_none_or(|scope| !authorized_scopes.contains(scope))
    {
        return Err(AppError::Configuration);
    }
    let revisions = export_memory_revisions(store, memory_id)?;
    let admissions = export_memory_admissions(store, memory_id)?;
    let document = serde_json::json!({
        "schema_version": MEMORY_EXPORT_SCHEMA_VERSION,
        "memory_id": memory_id.as_str(),
        "scope": scope,
        "operation_count": operations.len(),
        "operations": operations,
        "revisions": revisions,
        "admissions": admissions,
    });
    let encoded = serde_json::to_vec_pretty(&document).map_err(|_| AppError::FileSystem)?;
    let file_name = format!(
        "autoharness-memory-{}.export.v{MEMORY_EXPORT_SCHEMA_VERSION}.json",
        safe_export_key(memory_id.as_str())
    );
    write_atomic_unique(output_directory, &file_name, &encoded)
}

fn load_all_events(
    store: &mut SqliteStore,
    session_id: &autoharness_domain::SessionId,
) -> Result<Vec<EventEnvelope>, AppError> {
    let mut events = Vec::new();
    let mut after = 0_u64;
    loop {
        let page = store
            .load_events(session_id, after, DEFAULT_EVENT_PAGE_SIZE)
            .map_err(AppError::from)?;
        let loaded = page.len();
        if let Some(last) = page.last() {
            after = last.sequence().get();
        }
        let complete = loaded < usize::try_from(DEFAULT_EVENT_PAGE_SIZE).unwrap_or(usize::MAX);
        events.extend(page);
        if complete {
            break;
        }
    }
    Ok(events)
}

fn export_contexts(
    store: &mut SqliteStore,
    events: &[EventEnvelope],
) -> Result<serde_json::Value, AppError> {
    let context_turn_ids = events.iter().filter_map(|event| match event.payload() {
        EventPayload::ContextTurnBound {
            context_turn_id, ..
        } => Some(context_turn_id.clone()),
        _ => None,
    });
    let mut epochs = BTreeMap::<ContextEpochId, autoharness_domain::ContextEpochManifest>::new();
    let mut turns = Vec::new();
    for context_turn_id in context_turn_ids {
        let manifest = store
            .load_context_turn(&context_turn_id)?
            .ok_or(AppError::Configuration)?;
        let epoch = store
            .load_context_epoch(manifest.epoch_id())?
            .ok_or(AppError::Configuration)?;
        epochs.insert(epoch.epoch_id().clone(), epoch);
        let admissions = store.load_context_admissions(&context_turn_id)?;
        let admission_rows = admissions
            .iter()
            .map(|admission| {
                let rendered = store
                    .load_context_admission_content(admission.admission_id())?
                    .map(|content| content.as_str().to_owned());
                Ok(serde_json::json!({
                    "manifest": admission,
                    "rendered_content_state": retained_state(rendered.is_some()),
                    "rendered_content": rendered,
                }))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let prelude = store
            .load_context_turn_content(&context_turn_id)?
            .map(|content| content.as_str().to_owned());
        turns.push(serde_json::json!({
            "manifest": manifest,
            "rendered_prelude_state": retained_state(prelude.is_some()),
            "rendered_prelude": prelude,
            "admissions": admission_rows,
        }));
    }
    let epochs = epochs.into_values().collect::<Vec<_>>();
    Ok(serde_json::json!({
        "epoch_count": epochs.len(),
        "turn_count": turns.len(),
        "epochs": epochs,
        "turns": turns,
    }))
}

fn export_session_memories(
    store: &mut SqliteStore,
    session_id: &SessionId,
) -> Result<Vec<serde_json::Value>, AppError> {
    let scope = MemoryScope::Session(session_id.clone());
    let mut before = None;
    let mut rows = Vec::new();
    loop {
        let query = MemoryInspectionQuery::new(
            vec![scope.clone()],
            Vec::new(),
            before,
            MAX_MEMORY_INSPECTION_PAGE_SIZE,
        )?;
        let page = store.inspect_memories(&query)?;
        let loaded = page.len();
        before = page.last().map(|record| {
            MemoryInspectionCursor::new(record.updated_at(), record.memory_id().clone())
        });
        for record in page {
            rows.push(serde_json::json!({
                "memory_id": record.memory_id().as_str(),
                "scope": record.scope(),
                "memory_kind": record.memory_kind(),
                "lifecycle": record.lifecycle(),
                "last_sequence": record.last_sequence(),
                "created_at_ms": record.created_at().get(),
                "updated_at_ms": record.updated_at().get(),
                "revisions": export_memory_revisions(store, record.memory_id())?,
                "admissions": export_memory_admissions(store, record.memory_id())?,
            }));
        }
        if loaded < usize::try_from(MAX_MEMORY_INSPECTION_PAGE_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(rows)
}

fn export_memory_revisions(
    store: &mut SqliteStore,
    memory_id: &MemoryId,
) -> Result<Vec<serde_json::Value>, AppError> {
    store
        .load_memory_revisions(memory_id)?
        .into_iter()
        .map(|revision| {
            let (content_state, content) = if revision.sensitivity() == Sensitivity::Secret {
                ("redacted_by_policy", None)
            } else {
                let content = store
                    .load_memory_content(revision.revision_id())?
                    .map(|content| content.as_str().to_owned());
                (retained_state(content.is_some()), content)
            };
            Ok(serde_json::json!({
                "metadata": revision,
                "content_state": content_state,
                "content": content,
            }))
        })
        .collect()
}

fn export_memory_admissions(
    store: &mut SqliteStore,
    memory_id: &MemoryId,
) -> Result<Vec<serde_json::Value>, AppError> {
    let mut before = None;
    let mut rows = Vec::new();
    loop {
        let query = MemoryAdmissionQuery::new(
            MemoryAdmissionKey::Memory(memory_id.clone()),
            before,
            MAX_MEMORY_INSPECTION_PAGE_SIZE,
        )?;
        let page = store.load_memory_admissions(&query)?;
        let loaded = page.len();
        before = page.last().map(|record| {
            MemoryAdmissionCursor::new(record.admitted_at(), record.admission_id().clone())
        });
        for record in page {
            let rendered = if record.rendered_content_available() {
                store
                    .load_context_admission_content(record.admission_id())?
                    .map(|content| content.as_str().to_owned())
            } else {
                None
            };
            rows.push(serde_json::json!({
                "admission_id": record.admission_id().as_str(),
                "memory_revision_id": record.memory_revision_id().as_str(),
                "context_turn_id": record.context_turn_id().as_str(),
                "epoch_id": record.epoch_id().as_str(),
                "session_id": record.session_id().as_str(),
                "attempt_id": record.attempt_id().as_str(),
                "run_turn": record.run_turn(),
                "model": record.model(),
                "admitted_at_ms": record.admitted_at().get(),
                "rank": record.rank(),
                "rank_score": record.rank_score(),
                "token_count": record.token_count().get(),
                "renderer_version": record.renderer_version(),
                "reasons": record.reasons(),
                "rendered_content_state": retained_state(rendered.is_some()),
                "rendered_content": rendered,
            }));
        }
        if loaded < usize::try_from(MAX_MEMORY_INSPECTION_PAGE_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(rows)
}

const fn retained_state(retained: bool) -> &'static str {
    if retained { "retained" } else { "unavailable" }
}

fn load_all_memory_operations(
    store: &mut SqliteStore,
    memory_id: &MemoryId,
) -> Result<Vec<autoharness_domain::MemoryOperationEnvelope>, AppError> {
    let mut operations = Vec::new();
    let mut after = 0_u64;
    loop {
        let page = store.load_memory_operations(
            memory_id,
            after,
            autoharness_store::DEFAULT_MEMORY_PAGE_SIZE,
        )?;
        let loaded = page.len();
        if let Some(last) = page.last() {
            after = last.sequence().get();
        }
        operations.extend(page);
        if loaded
            < usize::try_from(autoharness_store::DEFAULT_MEMORY_PAGE_SIZE).unwrap_or(usize::MAX)
        {
            break;
        }
    }
    Ok(operations)
}

fn safe_export_key(value: &str) -> String {
    let key = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(96)
        .collect::<String>();
    if key.is_empty() {
        "item".to_owned()
    } else {
        key
    }
}

fn write_atomic_unique(
    output_directory: &Path,
    requested_name: &str,
    bytes: &[u8],
) -> Result<PathBuf, AppError> {
    let suffix = Uuid::new_v4().simple();
    let stem = requested_name
        .strip_suffix(".json")
        .unwrap_or(requested_name);
    let destination = output_directory.join(format!("{stem}-{suffix}.json"));
    let temporary = output_directory.join(format!(".{stem}-{suffix}.tmp"));
    std::fs::write(&temporary, bytes).map_err(|_| AppError::FileSystem)?;
    if std::fs::rename(&temporary, &destination).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(AppError::FileSystem);
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        AttemptId, Causation, CommandId, ConfidenceBasisPoints, ContextAdmission,
        ContextAdmissionFactor, ContextAdmissionId, ContextAdmissionReason,
        ContextBudgetAllocation, ContextEligibility, ContextEpochHashes, ContextEpochManifest,
        ContextEpochReason, ContextEpochVersions, ContextSection, ContextSourceKey,
        ContextTokenBudget, ContextTurnId, ContextTurnManifest, CorrelationId, DeliveryMode,
        EstimatedTokens, EventEnvelope, EventId, EventPayload, InputId, MemoryContent,
        MemoryEvidence, MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryEvidenceRelation,
        MemoryEvidenceSource, MemoryKind, MemoryOperationEnvelope, MemoryOperationId,
        MemoryOperationPayload, MemoryOrigin, MemoryRevision, MemoryRevisionDraft,
        MemoryRevisionId, MemoryRevisionNumber, MemoryRevisionStatus, MemorySequence,
        MemoryValidity, ModelId, ModelRef, PromptText, ProviderId, SessionSequence, Sha256Digest,
        TimestampMillis, TrustClass, UserId, WorkspaceId,
    };
    use autoharness_memory::{
        CONTEXT_RENDERER_VERSION, CanonicalEncoder, MEMORY_RENDERER_V1, context_manifest_hash,
        normalized_content_hash, rendered_context_hash,
    };
    use autoharness_store::{
        AppendRequest, BoundContextTurnCommitRequest, ContextAdmissionContent, ContextStore as _,
        ContextTurnCommitRequest, ContextTurnContent, DeletionDisposition, MemoryAppendRequest,
        MemoryEvidenceContent, MemoryRevisionContent, MemoryStore as _, RenderedContextText,
        SessionStore as _,
    };
    use sha2::{Digest as _, Sha256};

    use super::*;

    fn envelope(
        event: &str,
        sequence: u64,
        payload: EventPayload,
    ) -> Result<EventEnvelope, autoharness_domain::ValueError> {
        Ok(EventEnvelope::new_v1(
            EventId::new(event).map_err(|_| autoharness_domain::ValueError::EmptyIdentifier)?,
            autoharness_domain::SessionId::new("session-x")
                .map_err(|_| autoharness_domain::ValueError::EmptyIdentifier)?,
            SessionSequence::new(sequence)
                .map_err(|_| autoharness_domain::ValueError::ZeroSequence)?,
            TimestampMillis::new(100),
            Causation::Command(
                CommandId::new(format!("command-{sequence}"))
                    .map_err(|_| autoharness_domain::ValueError::EmptyIdentifier)?,
            ),
            CorrelationId::new("correlation-1")
                .map_err(|_| autoharness_domain::ValueError::EmptyIdentifier)?,
            payload,
        ))
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
    }

    fn raw_digest(content: &str) -> Sha256Digest {
        Sha256Digest::new(
            Sha256::digest(content.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .expect("raw digest")
    }

    fn model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider ID"),
            ModelId::new("models/gemini-export").expect("model ID"),
        )
    }

    fn seed_dispatch_ready_session(store: &mut SqliteStore) -> SessionId {
        let session = SessionId::new("session-x").expect("session ID");
        let events = vec![
            envelope("event-1", 1, EventPayload::SessionCreated).expect("session event"),
            envelope("event-2", 2, EventPayload::ModelSelected { model: model() })
                .expect("model event"),
            envelope(
                "event-3",
                3,
                EventPayload::InputAdmitted {
                    input_id: InputId::new("input-1").expect("input ID"),
                    prompt: PromptText::new("export privacy evidence").expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            )
            .expect("input event"),
            envelope(
                "event-4",
                4,
                EventPayload::AttemptPrepared {
                    attempt_id: AttemptId::new("attempt-export").expect("attempt ID"),
                    input_id: InputId::new("input-1").expect("input ID"),
                    model: model(),
                    retry_of: None,
                },
            )
            .expect("attempt prepared event"),
            envelope(
                "event-5",
                5,
                EventPayload::AttemptStarted {
                    attempt_id: AttemptId::new("attempt-export").expect("attempt ID"),
                },
            )
            .expect("attempt started event"),
        ];
        store
            .append(&AppendRequest::new(session.clone(), 0, events))
            .expect("seed dispatch-ready session");
        session
    }

    fn append_created_memory(
        store: &mut SqliteStore,
        memory_id: &str,
        scope: MemoryScope,
        status: MemoryRevisionStatus,
        content: &str,
        evidence: Vec<MemoryEvidence>,
        occurred_at: i64,
    ) -> MemoryRevision {
        let content = MemoryContent::new(content).expect("memory content");
        let origin = if status == MemoryRevisionStatus::Proposed {
            MemoryOrigin::ModelProposal
        } else {
            MemoryOrigin::ExplicitUser
        };
        let trust = if status == MemoryRevisionStatus::Proposed {
            TrustClass::UntrustedProposal
        } else {
            TrustClass::UserApproved
        };
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new(format!("revision-{memory_id}")).expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            origin,
            trust,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            autoharness_domain::Sensitivity::Internal,
            MemoryValidity::Indefinite,
            evidence,
            Vec::new(),
        )
        .expect("revision draft");
        let revision =
            MemoryRevision::from_draft(status, &draft, TimestampMillis::new(occurred_at), None);
        let evidence_content = draft
            .evidence()
            .iter()
            .filter_map(|evidence| {
                evidence.excerpt().cloned().map(|excerpt| {
                    MemoryEvidenceContent::new(evidence.evidence_id().clone(), excerpt)
                })
            })
            .collect();
        let operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new(format!("operation-{memory_id}-1")).expect("operation ID"),
            MemoryId::new(memory_id).expect("memory ID"),
            MemorySequence::FIRST,
            TimestampMillis::new(occurred_at),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new(format!("command-{memory_id}-1")).expect("command ID"),
            ),
            CorrelationId::new(format!("correlation-{memory_id}")).expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope,
                memory_kind: MemoryKind::Fact,
                revision: revision.clone(),
            },
        );
        store
            .append_memory(&MemoryAppendRequest::new(
                0,
                operation,
                Some(MemoryRevisionContent::new(
                    revision.revision_id().clone(),
                    content,
                    evidence_content,
                )),
            ))
            .expect("append memory");
        revision
    }

    fn append_memory_lifecycle(
        store: &mut SqliteStore,
        memory_id: &str,
        revision_id: &MemoryRevisionId,
        payload: MemoryOperationPayload,
        occurred_at: i64,
    ) {
        let operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new(format!("operation-{memory_id}-2")).expect("operation ID"),
            MemoryId::new(memory_id).expect("memory ID"),
            MemorySequence::new(2).expect("memory sequence"),
            TimestampMillis::new(occurred_at),
            autoharness_domain::MemoryCausation::Command(
                CommandId::new(format!("command-{memory_id}-2")).expect("command ID"),
            ),
            CorrelationId::new(format!("correlation-{memory_id}")).expect("correlation ID"),
            payload,
        );
        store
            .append_memory(&MemoryAppendRequest::new(1, operation, None))
            .expect("append memory lifecycle");
        assert_eq!(
            store
                .load_memory_revisions(&MemoryId::new(memory_id).expect("memory ID"))
                .expect("load lifecycle revision")[0]
                .revision_id(),
            revision_id
        );
    }

    fn rendered_admission_hash(
        memory_id: &MemoryId,
        revision_id: &MemoryRevisionId,
        rendered: &str,
    ) -> Sha256Digest {
        let mut encoder = CanonicalEncoder::new();
        encoder
            .field("renderer", MEMORY_RENDERER_V1.as_bytes())
            .expect("renderer field");
        encoder
            .field("memory_id", memory_id.as_str().as_bytes())
            .expect("memory ID field");
        encoder
            .field("revision_id", revision_id.as_str().as_bytes())
            .expect("revision ID field");
        encoder
            .field("rendered", rendered.as_bytes())
            .expect("rendered field");
        encoder.finish().expect("rendered admission hash")
    }

    fn commit_export_context(store: &mut SqliteStore, admitted: &[(&str, &MemoryRevision, &str)]) {
        let session_id = SessionId::new("session-x").expect("session ID");
        let context_turn_id = ContextTurnId::new("context-turn-export").expect("context turn ID");
        let admissions = admitted
            .iter()
            .enumerate()
            .map(|(index, (memory_id, revision, rendered))| {
                let memory_id = MemoryId::new(*memory_id).expect("memory ID");
                ContextAdmission::new(
                    ContextAdmissionId::new(format!("admission-{}", index + 1))
                        .expect("admission ID"),
                    context_turn_id.clone(),
                    ContextSection::DurableMemory,
                    ContextSourceKey::new(format!("memory:{memory_id}")).expect("source key"),
                    revision.content_hash().clone(),
                    Some(revision.revision_id().clone()),
                    CONTEXT_RENDERER_VERSION,
                    rendered_admission_hash(&memory_id, revision.revision_id(), rendered),
                    u32::try_from(index + 1).expect("rank"),
                    500 - i64::try_from(index).expect("score"),
                    EstimatedTokens::new(4).expect("token count"),
                    TimestampMillis::new(100),
                    vec![
                        ContextAdmissionReason::new(1, ContextAdmissionFactor::Authority, 300)
                            .expect("authority reason"),
                        ContextAdmissionReason::new(2, ContextAdmissionFactor::ExactMatch, 200)
                            .expect("exact-match reason"),
                    ],
                )
                .expect("context admission")
            })
            .collect::<Vec<_>>();
        let prelude = admitted
            .iter()
            .map(|(_, _, rendered)| *rendered)
            .collect::<Vec<_>>()
            .join("\n");
        let memory_generation = store.memory_generation().expect("memory generation");
        let placeholder = ContextTurnManifest::new(
            context_turn_id,
            ContextEpochId::new("epoch-export").expect("epoch ID"),
            session_id.clone(),
            AttemptId::new("attempt-export").expect("attempt ID"),
            1,
            SessionSequence::new(5).expect("session sequence"),
            memory_generation,
            model(),
            digest('1'),
            rendered_context_hash(&prelude).expect("prelude hash"),
            digest('2'),
            ContextEligibility::new(
                UserId::new("user-1").expect("user ID"),
                WorkspaceId::new("workspace-1").expect("workspace ID"),
                session_id.clone(),
                None,
                autoharness_domain::Sensitivity::Internal,
            ),
            ContextBudgetAllocation::new(
                ContextTokenBudget::new(4_096).expect("token budget"),
                EstimatedTokens::new(0).expect("reserved tokens"),
                EstimatedTokens::new(2_048).expect("memory budget"),
            )
            .expect("budget allocation"),
            EstimatedTokens::new(16).expect("rendered token count"),
            TimestampMillis::new(100),
            Vec::new(),
            admissions,
        )
        .expect("placeholder context manifest");
        let manifest = ContextTurnManifest::new(
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
        .expect("context manifest");
        let epoch = ContextEpochManifest::new(
            manifest.epoch_id().clone(),
            session_id,
            memory_generation,
            ContextEpochReason::NewAttempt,
            None,
            digest('3'),
            ContextEpochVersions::new(1, 1, 1, 1, 1).expect("epoch versions"),
            ContextEpochHashes::new(digest('4'), digest('5'), digest('6'), digest('7')),
            ContextTokenBudget::new(4_096).expect("token budget"),
            TimestampMillis::new(90),
        )
        .expect("context epoch");
        let content = ContextTurnContent::new(
            Some(RenderedContextText::new(prelude).expect("rendered prelude")),
            manifest
                .admissions()
                .iter()
                .zip(admitted.iter())
                .map(|(admission, (_, _, rendered))| {
                    ContextAdmissionContent::new(
                        admission.admission_id().clone(),
                        RenderedContextText::new(*rendered).expect("rendered admission"),
                    )
                })
                .collect(),
        );
        let binding = envelope(
            "event-6",
            6,
            EventPayload::ContextTurnBound {
                attempt_id: manifest.attempt_id().clone(),
                run_turn: manifest.run_turn(),
                context_turn_id: manifest.context_turn_id().clone(),
                manifest_hash: manifest.manifest_hash().clone(),
            },
        )
        .expect("binding event");
        store
            .commit_context_turn_and_bind(&BoundContextTurnCommitRequest::new(
                ContextTurnCommitRequest::new(Some(epoch), manifest, content),
                binding,
            ))
            .expect("commit and bind context");
        store
            .append(&AppendRequest::new(
                SessionId::new("session-x").expect("session ID"),
                6,
                vec![
                    envelope(
                        "event-7",
                        7,
                        EventPayload::RunTurnStarted {
                            attempt_id: AttemptId::new("attempt-export").expect("attempt ID"),
                            turn: 1,
                        },
                    )
                    .expect("run turn event"),
                    envelope(
                        "event-8",
                        8,
                        EventPayload::AttemptCompleted {
                            attempt_id: AttemptId::new("attempt-export").expect("attempt ID"),
                        },
                    )
                    .expect("attempt completed event"),
                ],
            ))
            .expect("settle bound provider turn");
    }

    fn read_export(path: &Path) -> (Vec<u8>, serde_json::Value) {
        let bytes = std::fs::read(path).expect("read export");
        let document = serde_json::from_slice(&bytes).expect("parse export");
        (bytes, document)
    }

    fn session_memory<'a>(
        document: &'a serde_json::Value,
        memory_id: &str,
    ) -> &'a serde_json::Value {
        document["session_memories"]
            .as_array()
            .expect("session memories")
            .iter()
            .find(|memory| memory["memory_id"] == memory_id)
            .expect("memory export")
    }

    fn user_input_evidence(evidence_id: &str, excerpt: &str) -> MemoryEvidence {
        let excerpt = MemoryEvidenceExcerpt::new(excerpt).expect("evidence excerpt");
        MemoryEvidence::new(
            MemoryEvidenceId::new(evidence_id).expect("evidence ID"),
            MemoryEvidenceSource::UserInput {
                session_id: SessionId::new("session-x").expect("session ID"),
                input_id: InputId::new("input-1").expect("input ID"),
            },
            MemoryEvidenceRelation::Supports,
            Some(excerpt.clone()),
            Some(raw_digest(excerpt.as_str())),
        )
        .expect("memory evidence")
    }

    #[test]
    fn export_writes_complete_history_and_survives_deletion() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("export.sqlite3");
        let mut store = SqliteStore::open(&database).expect("open store");
        let session = autoharness_domain::SessionId::new("session-x").expect("valid session ID");

        let events = vec![
            envelope("event-1", 1, EventPayload::SessionCreated).expect("valid event"),
            envelope(
                "event-2",
                2,
                EventPayload::InputAdmitted {
                    input_id: autoharness_domain::InputId::new("input-1").expect("input ID"),
                    prompt: PromptText::new("archived prompt").expect("prompt"),
                    delivery_mode: autoharness_domain::DeliveryMode::NextTurn,
                },
            )
            .expect("valid event"),
        ];
        store
            .append(&AppendRequest::new(session.clone(), 0, events))
            .expect("append history");

        let summaries = store.list_sessions().expect("list sessions");
        assert_eq!(summaries.len(), 1);
        let destination =
            export_session(&mut store, &summaries[0], directory.path()).expect("export");

        assert!(destination.exists());
        assert!(destination.to_string_lossy().contains("session-x"));

        // Delete the session; the export remains independently readable and
        // carries the full authoritative history.
        assert_eq!(
            store
                .delete_session(&session, summaries[0].last_sequence().get())
                .expect("delete"),
            DeletionDisposition::Deleted
        );
        let bytes = std::fs::read(&destination).expect("read export");
        let document: serde_json::Value = serde_json::from_slice(&bytes).expect("parse export");
        assert_eq!(document["schema_version"], EXPORT_SCHEMA_VERSION);
        assert_eq!(document["event_count"], 2);
        assert_eq!(
            document["session"]["session_id"],
            summaries[0].session_id().as_str()
        );
        let events = document["events"].as_array().expect("events array");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["payload"]["kind"], "session_created");
    }

    #[test]
    fn schema_v2_exports_context_memory_lifecycles_and_privacy_states() {
        // Seed the evidence sidecar directly to model a configured credential that an ingress
        // guard must reject. The exporter has no evidence-content read and must not leak it.
        const CONFIGURED_SECRET_SENTINEL: &str = "configured-credential-never-export";

        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("composed-export.sqlite3");
        let mut store = SqliteStore::open(&database).expect("open store");
        let session = seed_dispatch_ready_session(&mut store);
        let session_scope = MemoryScope::Session(session.clone());

        let active = append_created_memory(
            &mut store,
            "memory-active",
            session_scope.clone(),
            MemoryRevisionStatus::Active,
            "Retain exact context bytes for this fact.",
            Vec::new(),
            10,
        );
        let deleted = append_created_memory(
            &mut store,
            "memory-deleted",
            session_scope.clone(),
            MemoryRevisionStatus::Active,
            "Erase this fact and every derived rendering.",
            Vec::new(),
            20,
        );
        append_created_memory(
            &mut store,
            "memory-proposed",
            session_scope.clone(),
            MemoryRevisionStatus::Proposed,
            "Review this model-authored proposal.",
            vec![user_input_evidence(
                "evidence-proposal",
                CONFIGURED_SECRET_SENTINEL,
            )],
            30,
        );
        let retracted = append_created_memory(
            &mut store,
            "memory-retracted",
            session_scope.clone(),
            MemoryRevisionStatus::Active,
            "This fact is historically retained but no longer eligible.",
            Vec::new(),
            40,
        );
        append_memory_lifecycle(
            &mut store,
            "memory-retracted",
            retracted.revision_id(),
            MemoryOperationPayload::MemoryRetracted {
                revision_id: retracted.revision_id().clone(),
            },
            50,
        );
        let cross_scope = append_created_memory(
            &mut store,
            "memory-cross-scope",
            MemoryScope::User(UserId::new("user-1").expect("user ID")),
            MemoryRevisionStatus::Active,
            "A user-scoped fact survives deletion of its evidence session.",
            vec![user_input_evidence(
                "evidence-cross-scope",
                CONFIGURED_SECRET_SENTINEL,
            )],
            60,
        );

        commit_export_context(
            &mut store,
            &[
                (
                    "memory-active",
                    &active,
                    "<memory>retained admission rendering</memory>",
                ),
                (
                    "memory-deleted",
                    &deleted,
                    "<memory>erasable admission rendering</memory>",
                ),
            ],
        );
        append_memory_lifecycle(
            &mut store,
            "memory-deleted",
            deleted.revision_id(),
            MemoryOperationPayload::MemoryDeleted {
                revision_id: deleted.revision_id().clone(),
            },
            110,
        );

        let summaries = store.list_sessions().expect("list sessions");
        let summary = summaries
            .iter()
            .find(|summary| summary.session_id() == &session)
            .expect("session summary");
        let first = export_session(&mut store, summary, directory.path()).expect("first export");
        let second = export_session(&mut store, summary, directory.path()).expect("second export");
        let (first_bytes, document) = read_export(&first);
        let (second_bytes, _) = read_export(&second);

        assert_eq!(
            first_bytes, second_bytes,
            "archive bytes must be deterministic"
        );
        assert!(
            !first_bytes
                .windows(CONFIGURED_SECRET_SENTINEL.len())
                .any(|window| window == CONFIGURED_SECRET_SENTINEL.as_bytes())
        );
        assert_eq!(document["schema_version"], EXPORT_SCHEMA_VERSION);
        assert_eq!(document["event_count"], 8);
        assert_eq!(document["context_audit"]["epoch_count"], 1);
        assert_eq!(document["context_audit"]["turn_count"], 1);

        let context_turn = &document["context_audit"]["turns"][0];
        assert_eq!(
            context_turn["manifest"]["context_turn_id"],
            "context-turn-export"
        );
        assert_eq!(context_turn["rendered_prelude_state"], "unavailable");
        assert!(context_turn["rendered_prelude"].is_null());
        assert_eq!(context_turn["admissions"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            context_turn["admissions"][0]["rendered_content_state"],
            "retained"
        );
        assert_eq!(
            context_turn["admissions"][0]["manifest"]["reasons"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            context_turn["admissions"][1]["rendered_content_state"],
            "unavailable"
        );
        assert!(context_turn["admissions"][1]["rendered_content"].is_null());

        let active_row = session_memory(&document, "memory-active");
        assert_eq!(active_row["lifecycle"], "active");
        assert_eq!(active_row["revisions"][0]["content_state"], "retained");
        assert_eq!(
            active_row["admissions"][0]["rendered_content_state"],
            "retained"
        );
        assert_eq!(active_row["revisions"][0]["metadata"]["status"], "active");
        let proposed_row = session_memory(&document, "memory-proposed");
        assert_eq!(proposed_row["lifecycle"], "proposed");
        assert_eq!(
            proposed_row["revisions"][0]["metadata"]["status"],
            "proposed"
        );
        assert_eq!(
            proposed_row["revisions"][0]["metadata"]["origin"],
            "model_proposal"
        );
        assert_eq!(
            proposed_row["revisions"][0]["metadata"]["trust_class"],
            "untrusted_proposal"
        );
        let retracted_row = session_memory(&document, "memory-retracted");
        assert_eq!(retracted_row["lifecycle"], "retracted");
        assert_eq!(
            retracted_row["revisions"][0]["metadata"]["status"],
            "retracted"
        );
        let deleted_row = session_memory(&document, "memory-deleted");
        assert_eq!(deleted_row["lifecycle"], "deleted");
        assert_eq!(deleted_row["revisions"][0]["metadata"]["status"], "deleted");
        assert_eq!(deleted_row["revisions"][0]["content_state"], "unavailable");
        assert!(deleted_row["revisions"][0]["content"].is_null());
        assert_eq!(
            deleted_row["admissions"][0]["rendered_content_state"],
            "unavailable"
        );
        assert!(
            document["session_memories"]
                .as_array()
                .expect("session memories")
                .iter()
                .all(|memory| memory["memory_id"] != "memory-cross-scope")
        );

        let mutation_before = store
            .memory_mutation_generation()
            .expect("memory mutation generation");
        assert_eq!(
            store
                .delete_session(&session, summary.last_sequence().get())
                .expect("delete session"),
            DeletionDisposition::Deleted
        );
        assert!(
            store
                .memory_mutation_generation()
                .expect("memory mutation generation")
                .get()
                > mutation_before.get()
        );
        assert_eq!(
            std::fs::read(&first).expect("read preserved archive"),
            first_bytes,
            "session deletion must not mutate the independent archive"
        );

        let cross_scope_path = export_memory(
            &mut store,
            &MemoryId::new("memory-cross-scope").expect("memory ID"),
            &[MemoryScope::User(UserId::new("user-1").expect("user ID"))],
            directory.path(),
        )
        .expect("cross-scope export");
        let (cross_scope_bytes, cross_scope_document) = read_export(&cross_scope_path);
        assert!(
            !cross_scope_bytes
                .windows(CONFIGURED_SECRET_SENTINEL.len())
                .any(|window| window == CONFIGURED_SECRET_SENTINEL.as_bytes())
        );
        assert_eq!(cross_scope_document["memory_id"], "memory-cross-scope");
        assert_eq!(
            cross_scope_document["revisions"][0]["content_state"],
            "retained"
        );
        assert_eq!(
            cross_scope_document["revisions"][0]["metadata"]["evidence"][0]["source"]["payload"]["session_id"],
            "session-x"
        );
        assert_eq!(
            cross_scope_document["revisions"][0]["metadata"]["evidence"][0]["excerpt_hash"],
            raw_digest(CONFIGURED_SECRET_SENTINEL).as_str()
        );
        assert_eq!(
            cross_scope.revision_id().as_str(),
            "revision-memory-cross-scope"
        );

        let session_tombstone_path = export_memory(
            &mut store,
            &MemoryId::new("memory-active").expect("memory ID"),
            &[MemoryScope::Session(session)],
            directory.path(),
        )
        .expect("session tombstone export");
        let (_, session_tombstone) = read_export(&session_tombstone_path);
        assert_eq!(
            session_tombstone["revisions"][0]["metadata"]["status"],
            "deleted"
        );
        assert_eq!(
            session_tombstone["revisions"][0]["content_state"],
            "unavailable"
        );
    }

    #[test]
    fn standalone_memory_export_requires_exact_scope_authority() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut store =
            SqliteStore::open(directory.path().join("scope-export.sqlite3")).expect("open store");
        let memory_id = MemoryId::new("memory-user-private").expect("memory ID");
        append_created_memory(
            &mut store,
            memory_id.as_str(),
            MemoryScope::User(UserId::new("user-1").expect("user ID")),
            MemoryRevisionStatus::Active,
            "Only the owning scope may export this content.",
            Vec::new(),
            1,
        );

        assert!(matches!(
            export_memory(
                &mut store,
                &memory_id,
                &[MemoryScope::User(UserId::new("user-2").expect("user ID"),)],
                directory.path(),
            ),
            Err(AppError::Configuration)
        ));
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("read directory")
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "json"))
                .count(),
            0
        );
    }
}
