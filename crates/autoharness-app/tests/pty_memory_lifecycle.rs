//! Complete Memory lifecycle journey through the real terminal and durable store.

mod pty_support;

use autoharness_domain::{
    MemoryContent, MemoryId, MemoryOperationPayload, MemoryOrigin, MemoryScope, Sensitivity,
    TimestampMillis, TrustClass, WorkspaceId,
};
use autoharness_store::{MemorySearchQuery, MemoryStore};
use autoharness_store_sqlite::SqliteStore;
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const ALT_6: [u8; 2] = [0x1b, b'6'];
const ALT_I: [u8; 2] = [0x1b, b'i'];
const ALT_N: [u8; 2] = [0x1b, b'n'];
const ALT_E: [u8; 2] = [0x1b, b'e'];
const ALT_V: [u8; 2] = [0x1b, b'v'];
const ALT_S: [u8; 2] = [0x1b, b's'];
const ALT_X: [u8; 2] = [0x1b, b'x'];
const ALT_D: [u8; 2] = [0x1b, b'd'];
const CTRL_S: [u8; 1] = [0x13];
const TAB: [u8; 1] = *b"\t";
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];
const RIGHT: [u8; 3] = [0x1b, b'[', b'C'];

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn memory_can_be_created_restarted_corrected_exported_retracted_and_deleted() {
    let environment = ScenarioEnvironment::prepare();
    environment.seed_completed_session("memory journey seed", "memory journey ready");
    let original = "PTY durable preference: use compact verified summaries.";
    let correction = " Corrected after restart.";
    let import_path = "import.txt";
    let imported = "PTY imported decision: preserve exact provider evidence across restart.";
    std::fs::write(environment.data_dir().join(import_path), imported)
        .expect("safe UTF-8 workspace document");

    let mut first = PtySession::start(&environment, 30, 100);
    first.wait_for(
        |screen| screen.contents().contains("memory journey ready"),
        "seeded session should make the offline terminal ready",
    );
    first.send_bytes(&ALT_6);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Memory")
                && text.contains("Memory index")
                && text.contains("Search all memory")
        },
        "Alt+6 should open the complete Memory workspace",
    );
    first.send_bytes(&ALT_N);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Remember")
                && text.contains("Scope: Workspace")
                && text.contains("Ctrl+S saves")
        },
        "Alt+N should open the explicit-memory editor",
    );
    first.type_text(original);
    first.send_bytes(&CTRL_S);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY durable preference") && !text.contains("Ctrl+S saves")
        },
        "saving should close the editor and publish the committed memory",
    );

    first.send_bytes(&ALT_I);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Import document")
                && text.contains("Workspace-relative UTF-8 text")
                && text.contains("Copies exact bytes, up to 16 KiB")
                && text.contains("Review-only until separate approval")
                && text.contains("Enter imports; Esc cancels")
        },
        "Alt+I should explain the bounded review-only workspace import",
    );
    first.type_text(import_path);
    first.send_bytes(b"\r");
    first.wait_for(
        |screen| {
            let text = screen.contents();
            !text.contains("Import document") && text.contains("2 on page")
        },
        "submitting a relative path should create a review-only proposal",
    );
    first.send_bytes(b"/");
    first.send_bytes(&TAB);
    for _ in 0..3 {
        first.send_bytes(&RIGHT);
    }
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("State: proposed")
                && text.contains("PTY imported decision")
                && text.contains("proposed")
                && text.contains("imported document")
                && text.contains("imported")
        },
        "the imported document should remain a visibly unapproved proposal",
    );
    first.send_bytes(&ALT_V);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Review proposal")
                && text.contains("Exact proposed content")
                && text.contains("PTY imported decision")
                && text.contains("State: proposed")
                && text.contains("Trust: imported")
                && text.contains("Origin: imported document")
        },
        "proposal review should expose exact content, provenance, and authority",
    );
    for _ in 0..5 {
        first.send_bytes(&DOWN);
    }
    first.wait_for(
        |screen| screen.contents().contains("Approval is deliberate"),
        "the scrollable review should explain that approval affects future turns",
    );
    first.send_bytes(b"a");
    first.wait_for(
        |screen| {
            let text = screen.contents();
            !text.contains("Review proposal") && text.contains("State: proposed")
        },
        "approval should commit before the first process exits",
    );
    first.send_bytes(&ctrl_c());
    assert_eq!(first.wait_for_exit(), 0, "first terminal exits cleanly");
    first.wait_for_raw(
        b"\x1b[?1049l",
        "first lifecycle leg must leave the alternate screen",
    );
    drop(first);

    let mut restarted = PtySession::start(&environment, 30, 100);
    restarted.send_bytes(&ALT_6);
    restarted.wait_for(
        |screen| screen.contents().contains("Memory index"),
        "the Memory workspace should reopen after a real process restart",
    );
    restarted.send_bytes(b"/");
    restarted.type_text("PTY imported decision");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "import search should enter its debounced authoritative refresh",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY imported decision")
                && text.contains("user approved")
                && text.contains("explicit user")
                && text.contains("active")
                && !text.contains("refreshing view")
        },
        "the approved imported revision should remain eligible after restart",
    );
    restarted.send_bytes(b"\x1b");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "clearing import search should request the authoritative eligible view",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Search all memory") && !text.contains("refreshing view")
        },
        "clearing import search should restore the authoritative eligible view",
    );
    restarted.send_bytes(b"/");
    restarted.type_text("PTY durable preference");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "remembered-preference search should enter its debounced refresh",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY durable preference") && !text.contains("refreshing view")
        },
        "literal search should restore the separately remembered preference",
    );
    restarted.send_bytes(b"\x1b");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "clearing remembered-preference search should refresh the full view",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY durable preference")
                && text.contains("Search all memory")
                && !text.contains("refreshing view")
        },
        "clearing the focused search should keep its selected durable row",
    );
    restarted.send_bytes(b"/");
    restarted.type_text("missing memory sentinel");
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("0 visible") && text.contains("No memories match")
        },
        "literal Memory search should present an explicit no-match state",
    );
    restarted.send_bytes(b"\x1b");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "clearing the no-match search should refresh the complete eligible view",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Search all memory")
                && text.contains("2 visible")
                && !text.contains("refreshing view")
        },
        "Esc should clear Memory search and restore the complete eligible view",
    );
    restarted.send_bytes(b"/");
    restarted.type_text("PTY durable preference");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "the correction target search should enter its authoritative refresh",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("PTY durable preference") && !text.contains("refreshing view")
        },
        "the separately remembered preference should be selected for correction",
    );
    restarted.resize(18, 60);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Memory index") && text.contains("PTY durable preference")
        },
        "the Memory workspace should remain complete after a compact resize",
    );
    restarted.send_bytes(b"\r");
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Revision detail") && text.contains("PTY durable preference")
        },
        "compact Enter should drill into the selected Memory detail",
    );
    restarted.resize(30, 100);
    restarted.wait_for(
        |screen| screen.contents().contains("Memory index"),
        "the Memory workspace should restore its wide layout after resize",
    );
    restarted.send_bytes(&ALT_E);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Correct memory") && text.contains("PTY durable preference")
        },
        "Alt+E should preload the exact retained revision for correction",
    );
    restarted.type_text(correction);
    restarted.send_bytes(&CTRL_S);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Revision")
                && text.contains("restart.")
                && !text.contains("Correct memory")
        },
        "the corrected revision should become the visible active head",
    );

    restarted.send_bytes(&ALT_S);
    restarted.wait_for(
        |screen| screen.contents().contains("Export memory"),
        "Alt+S should show the exact standalone export review",
    );
    restarted.send_bytes(b"\r");
    restarted.wait_for(
        |screen| {
            !screen.contents().contains("Export memory")
                && std::fs::read_dir(environment.data_dir()).is_ok_and(|entries| {
                    entries.filter_map(Result::ok).any(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("autoharness-memory-")
                    })
                })
        },
        "confirmed export should write a user-owned artifact",
    );

    restarted.send_bytes(&ALT_X);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Retract memory")
                && text.contains("future admission")
                && text.contains("cannot be recalled")
        },
        "retraction should explain its forward-only effect",
    );
    restarted.send_bytes(b"y");
    restarted.wait_for(
        |screen| !screen.contents().contains("Retract memory"),
        "confirmed retraction should settle durably",
    );

    let workspace_id = persisted_workspace_id(&environment);
    let eligibility_query = MemorySearchQuery::new(
        MemoryContent::new("durable").expect("eligibility query"),
        vec![MemoryScope::Workspace(workspace_id.clone())],
        Sensitivity::Sensitive,
        TimestampMillis::new(i64::MAX),
        8,
    )
    .expect("bounded eligibility query");
    let eligible_after_retraction = SqliteStore::open(environment.database())
        .expect("open store after retraction")
        .search_memory(&eligibility_query)
        .expect("search future-eligible memory");
    assert!(
        eligible_after_retraction.candidates().is_empty(),
        "retraction must remove the remembered preference from every future retrieval batch: {:?}",
        eligible_after_retraction.candidates()
    );

    restarted.send_bytes(b"/");
    restarted.send_bytes(&TAB);
    for _ in 0..4 {
        restarted.send_bytes(&RIGHT);
    }
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("State: inactive")
                && text.contains("PTY durable preference")
                && text.contains("retracted")
        },
        "the inactive filter should keep the retracted audit row inspectable",
    );

    restarted.send_bytes(&ALT_D);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Delete memory")
                && text.contains("Logical delete")
                && text.contains("Audit history remains")
        },
        "logical deletion should explain its tombstone semantics",
    );
    restarted.send_bytes(b"y");
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            !text.contains("Delete memory")
                && text.contains("0 visible")
                && text.contains("No memories match")
        },
        "confirmed deletion should immediately erase the revision from content search",
    );
    restarted.send_bytes(b"/");
    restarted.send_bytes(b"\x1b");
    restarted.wait_for(
        |screen| screen.contents().contains("refreshing view"),
        "clearing deleted content search should refresh the inactive audit view",
    );
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("State: inactive")
                && text.contains("deleted")
                && !text.contains("refreshing view")
        },
        "confirmed deletion should leave a visible content-free audit row",
    );
    restarted.send_bytes(&ctrl_c());
    assert_eq!(
        restarted.wait_for_exit(),
        0,
        "second terminal exits cleanly"
    );
    restarted.wait_for_raw(
        b"\x1b[?2004l",
        "final lifecycle exit must disable bracketed paste",
    );
    restarted.wait_for_raw(
        b"\x1b[?1049l",
        "final lifecycle exit must leave the alternate screen",
    );
    restarted.wait_for_raw(b"\x1b[?25h", "final lifecycle exit must show the cursor");
    drop(restarted);

    let export_path = std::fs::read_dir(environment.data_dir())
        .expect("scenario data directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("autoharness-memory-"))
        })
        .expect("standalone memory export");
    let export: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).expect("read standalone memory export"),
    )
    .expect("parse standalone memory export");
    assert_eq!(export["schema_version"], 2);
    let memory_id = MemoryId::new(
        export["memory_id"]
            .as_str()
            .expect("exported memory identity"),
    )
    .expect("valid exported memory identity");
    let exported_bytes = serde_json::to_vec(&export).expect("serialize parsed export");
    assert!(
        String::from_utf8_lossy(&exported_bytes).contains(original),
        "the pre-deletion user export should preserve the selected exact revision"
    );
    assert!(String::from_utf8_lossy(&exported_bytes).contains(correction));

    let store = SqliteStore::open(environment.database()).expect("open deleted memory store");
    let imported_query = MemorySearchQuery::new(
        MemoryContent::new("provider").expect("import eligibility query"),
        vec![MemoryScope::Workspace(workspace_id)],
        Sensitivity::Sensitive,
        TimestampMillis::new(i64::MAX),
        8,
    )
    .expect("bounded import eligibility query");
    let imported_candidates = store
        .search_memory(&imported_query)
        .expect("search approved imported memory");
    let imported_candidate = imported_candidates
        .candidates()
        .first()
        .expect("approved import remains future-eligible");
    assert_eq!(imported_candidate.content().as_str(), imported);
    assert_eq!(
        imported_candidate.revision().origin(),
        MemoryOrigin::ExplicitUser
    );
    assert_eq!(
        imported_candidate.revision().trust_class(),
        TrustClass::UserApproved
    );
    let operations = store
        .load_memory_operations(&memory_id, 0, 64)
        .expect("load durable memory audit stream");
    assert!(matches!(
        operations.last().map(|operation| operation.payload()),
        Some(MemoryOperationPayload::MemoryDeleted { .. })
    ));
    for revision in store
        .load_memory_revisions(&memory_id)
        .expect("load deleted memory revisions")
    {
        assert!(
            store
                .load_memory_content(revision.revision_id())
                .expect("load erased revision content")
                .is_none(),
            "logical deletion must erase every application-owned revision sidecar"
        );
    }
}

fn persisted_workspace_id(environment: &ScenarioEnvironment) -> WorkspaceId {
    let binding_directory = environment.data_dir().join("workspace-bindings");
    let binding = std::fs::read_dir(binding_directory)
        .expect("workspace binding directory")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "id")
        })
        .expect("persisted opaque workspace binding");
    let value = std::fs::read_to_string(binding.path()).expect("read opaque workspace binding");
    WorkspaceId::new(value.trim().to_owned()).expect("valid opaque workspace identity")
}
