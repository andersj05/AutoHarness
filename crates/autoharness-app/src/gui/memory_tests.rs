//! Native GUI command, channel, coordinator, SQLite, and restart journey.
use super::*;
use autoharness_client::{MemoryCommand, MemoryProjection, MemoryQuery, MemoryRow, MemoryText};
use std::sync::Mutex;

struct Harness {
    actor: crate::engine_actor::EngineActor,
    coordinator: tokio::task::JoinHandle<Result<(), AppError>>,
    bridge: BridgeActor,
    frames: Arc<Mutex<Vec<ServerFrame>>>,
    shutdown: CancellationToken,
}

impl Harness {
    fn start(database: std::path::PathBuf) -> Self {
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).unwrap();
        let (ui, app) = autoharness_tui::bounded_ports(
            Arc::new(crate::projection::session(&session)),
            Arc::new(TuiSessionsProjection::default()),
            Arc::new(TuiCatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = tokio::spawn(
            crate::coordinator::Coordinator::new(
                session_id,
                session,
                actor.handle(),
                None,
                app,
                shutdown.clone(),
            )
            .run(),
        );
        let (_, request_rx) = mpsc::channel(32);
        let (_, ack_rx) = mpsc::channel(1);
        let mut bridge = BridgeActor::new(ui, request_rx, ack_rx, shutdown.clone());
        let frames = Arc::new(Mutex::new(Vec::new()));
        let output = frames.clone();
        bridge
            .connect(Channel::new(move |body| {
                output
                    .lock()
                    .unwrap()
                    .push(body.deserialize::<ServerFrame>().unwrap());
                Ok(())
            }))
            .unwrap();
        Self {
            actor,
            coordinator,
            bridge,
            frames,
            shutdown,
        }
    }

    async fn command(&mut self, command: MemoryCommand, committed: bool) {
        // Exercise the exact serialized native ingress, including its schema version.
        let wire =
            serde_json::to_value(CommandEnvelope::new(ClientCommand::Memory { command })).unwrap();
        let receipt = self.bridge.dispatch(decode_command(wire).unwrap()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                self.bridge.drain_available_notices().unwrap();
                self.bridge.observe_pending_projections().unwrap();
                if let Some(frame) = &self.bridge.in_flight {
                    self.bridge.acknowledge_frame(frame.revision).unwrap();
                }
                self.bridge.pump_outbound().unwrap();
                let outcome =
                    self.frames
                        .lock()
                        .unwrap()
                        .iter()
                        .find_map(|frame| match &frame.payload {
                            autoharness_client::FramePayload::Notice(
                                ClientNotice::CommandCommitted { request_id },
                            ) if *request_id == receipt.request_id => Some(true),
                            autoharness_client::FramePayload::Notice(
                                ClientNotice::CommandRejected { request_id, .. },
                            ) if *request_id == receipt.request_id => Some(false),
                            _ => None,
                        });
                if let Some(outcome) = outcome {
                    assert_eq!(outcome, committed);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native command settlement");
    }

    async fn page(&mut self) -> MemoryProjection {
        self.command(
            MemoryCommand::Query(MemoryQuery {
                view_generation: DecimalU64::new(self.bridge.next_request_id),
                literal: MemoryText::new("").unwrap(),
                status: autoharness_client::MemoryStatusFilter::All,
                scope: autoharness_client::MemoryScopeFilter::All,
                direction: autoharness_client::MemoryPageDirection::First,
                before: None,
            }),
            true,
        )
        .await;
        self.bridge.snapshot().unwrap().memory
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.coordinator.await.unwrap().unwrap();
        self.actor.shutdown().await.unwrap();
    }
}

fn context(row: &MemoryRow) -> &autoharness_client::MemoryRevisionContext {
    row.detail
        .as_ref()
        .unwrap()
        .revision_context
        .as_ref()
        .unwrap()
}

#[tokio::test]
async fn gui_memory_lifecycle_replays_through_native_frames_and_sqlite() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("gui-memory.sqlite3");
    // A workspace-relative test document exercises the same confined import path as the GUI.
    let document = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    std::fs::write(
        document.path().join("decision.txt"),
        "Prefer measured evidence for every durable decision.",
    )
    .unwrap();
    let import_path = format!(
        "{}/decision.txt",
        document.path().file_name().unwrap().to_str().unwrap()
    );
    let mut harness = Harness::start(database.clone());
    harness
        .command(
            MemoryCommand::Remember {
                content: MemoryText::new("Use Rust for durable runtime state.").unwrap(),
            },
            true,
        )
        .await;
    harness
        .command(
            MemoryCommand::Import {
                path: MemoryText::new(import_path).unwrap(),
            },
            true,
        )
        .await;
    let page = harness.page().await;
    assert_eq!(page.rows.len(), 2);
    let proposal = page
        .rows
        .iter()
        .find(|row| row.status == autoharness_client::MemoryStatus::Proposed)
        .unwrap();
    assert_eq!(
        proposal.detail.as_ref().unwrap().trust,
        autoharness_client::MemoryTrust::Imported
    );
    let proposal_id = context(proposal).proposal_revision_id.clone().unwrap();
    let memory_id = proposal.memory_id.clone();
    let approval = MemoryCommand::Approve {
        memory_id: memory_id.clone(),
        expected_last_sequence: context(proposal).expected_last_sequence,
        proposal_revision_id: proposal_id.clone(),
    };
    harness.command(approval.clone(), true).await;
    harness.command(approval, false).await;
    let approved = harness.page().await;
    let row = approved
        .rows
        .iter()
        .find(|row| row.memory_id == memory_id)
        .unwrap();
    assert_eq!(row.status, autoharness_client::MemoryStatus::Active);
    assert_ne!(context(row).revision_id, proposal_id);
    assert_eq!(
        row.detail.as_ref().unwrap().trust,
        autoharness_client::MemoryTrust::UserApproved
    );
    harness.stop().await;

    let mut harness = Harness::start(database.clone());
    let restarted = harness.page().await;
    assert_eq!(
        restarted.rows, approved.rows,
        "GUI projection is replay equivalent after restart"
    );
    let row = restarted
        .rows
        .iter()
        .find(|row| row.memory_id == memory_id)
        .unwrap();
    harness
        .command(
            MemoryCommand::Revise {
                memory_id: memory_id.clone(),
                expected_last_sequence: context(row).expected_last_sequence,
                content: MemoryText::new("Use measured, replayable evidence for every decision.")
                    .unwrap(),
            },
            true,
        )
        .await;
    harness
        .command(
            MemoryCommand::Export {
                memory_id: memory_id.clone(),
            },
            true,
        )
        .await;
    assert!(std::fs::read_dir(directory.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("autoharness-memory-")
    }));
    let corrected = harness.page().await;
    let row = corrected
        .rows
        .iter()
        .find(|row| row.memory_id == memory_id)
        .unwrap();
    harness
        .command(
            MemoryCommand::Retract {
                memory_id: memory_id.clone(),
                expected_last_sequence: context(row).expected_last_sequence,
                revision_id: context(row).revision_id.clone(),
            },
            true,
        )
        .await;
    let retracted = harness.page().await;
    let row = retracted
        .rows
        .iter()
        .find(|row| row.memory_id == memory_id)
        .unwrap();
    assert_eq!(row.status, autoharness_client::MemoryStatus::Retracted);
    harness
        .command(
            MemoryCommand::Delete {
                memory_id: memory_id.clone(),
                expected_last_sequence: context(row).expected_last_sequence,
            },
            true,
        )
        .await;
    let deleted = harness.page().await;
    let row = deleted
        .rows
        .iter()
        .find(|row| row.memory_id == memory_id)
        .unwrap();
    assert_eq!(row.status, autoharness_client::MemoryStatus::Deleted);
    assert!(row.detail.as_ref().unwrap().content.is_none());
    harness.stop().await;

    let mut harness = Harness::start(database);
    assert_eq!(
        harness.page().await.rows,
        deleted.rows,
        "deletion remains replay equivalent"
    );
    harness.stop().await;
}

#[test]
fn gui_memory_rejects_invalid_imports_and_query_bounds_before_admission() {
    let request = TuiRequestId::new(1);
    for path in ["../secret.txt", "C:/secret.txt", "https://example.com/file"] {
        assert!(
            memory::map_command(
                MemoryCommand::Import {
                    path: MemoryText::new(path).unwrap()
                },
                request
            )
            .is_err()
        );
    }
    let query = MemoryQuery {
        view_generation: 1.into(),
        literal: MemoryText::new("x".repeat(257)).unwrap(),
        status: autoharness_client::MemoryStatusFilter::All,
        scope: autoharness_client::MemoryScopeFilter::All,
        direction: autoharness_client::MemoryPageDirection::First,
        before: None,
    };
    assert!(memory::map_command(MemoryCommand::Query(query), request).is_err());
}

#[test]
fn unrepresentable_memory_is_a_recoverable_workspace_failure() {
    let row = autoharness_tui::MemorySummary::new(
        "界".repeat(512),
        "safe preview",
        autoharness_tui::MemoryStatus::Active,
        autoharness_tui::MemoryScope::Workspace,
        1,
        None,
        0,
    )
    .unwrap();
    let source = autoharness_tui::MemoryProjection::ready(7, vec![row], vec![], 1, false)
        .unwrap()
        .with_view_page(9, None);
    let page = memory::map_projection(&source).unwrap();
    assert_eq!(page.view_generation.get(), 9);
    assert!(matches!(
        page.state,
        autoharness_client::MemoryLoadState::Failed { .. }
    ));
    assert!(page.rows.is_empty());
    page.validate().unwrap();
}
