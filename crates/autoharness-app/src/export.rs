//! Provider-neutral session export for pre-deletion archival.
//!
//! The export format is documented in `docs/architecture/SESSION_EXPORT.md`
//! and is intentionally derived only from authoritative durable events so an
//! exported file replays identically against any future schema-v1 consumer.

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
                    "rendered_content": rendered,
                }))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let prelude = store
            .load_context_turn_content(&context_turn_id)?
            .map(|content| content.as_str().to_owned());
        turns.push(serde_json::json!({
            "manifest": manifest,
            "rendered_prelude": prelude,
            "admissions": admission_rows,
        }));
    }
    Ok(serde_json::json!({
        "epochs": epochs.into_values().collect::<Vec<_>>(),
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
            let content = if revision.sensitivity() == Sensitivity::Secret {
                None
            } else {
                store
                    .load_memory_content(revision.revision_id())?
                    .map(|content| content.as_str().to_owned())
            };
            Ok(serde_json::json!({
                "metadata": revision,
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
                "rendered_content": rendered,
            }));
        }
        if loaded < usize::try_from(MAX_MEMORY_INSPECTION_PAGE_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(rows)
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
        Causation, CommandId, CorrelationId, EventEnvelope, EventId, EventPayload, PromptText,
        SessionSequence, TimestampMillis,
    };
    use autoharness_store::{AppendRequest, DeletionDisposition, SessionStore as _};

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
}
