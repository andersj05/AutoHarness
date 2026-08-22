//! Provider-neutral session export for pre-deletion archival.
//!
//! The export format is documented in `docs/architecture/SESSION_EXPORT.md`
//! and is intentionally derived only from authoritative durable events so an
//! exported file replays identically against any future schema-v1 consumer.

use std::path::{Path, PathBuf};

use autoharness_domain::EventEnvelope;
use autoharness_store::SessionSummary;
use autoharness_store_sqlite::SqliteStore;

use crate::error::AppError;

/// Schema version of the exported JSON document.
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

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
    });

    let encoded = serde_json::to_vec_pretty(&document).map_err(|_| AppError::FileSystem)?;
    let file_name = format!(
        "autoharness-session-{}.export.v{EXPORT_SCHEMA_VERSION}.json",
        summary.session_id().as_str()
    );
    let destination = output_directory.join(file_name);
    let temporary = output_directory.join(format!(
        "{}.tmp",
        destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "autoharness-export".to_owned())
    ));
    std::fs::write(&temporary, encoded).map_err(|_| AppError::FileSystem)?;
    std::fs::rename(&temporary, &destination).map_err(|_| AppError::FileSystem)?;
    Ok(destination)
}

fn load_all_events(
    store: &mut SqliteStore,
    session_id: &autoharness_domain::SessionId,
) -> Result<Vec<EventEnvelope>, AppError> {
    use autoharness_store::{DEFAULT_EVENT_PAGE_SIZE, SessionStore as _};

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
