//! Multi-session switching and destructive-confirmation scenarios through the real terminal.

mod pty_support;

use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const CTRL_A: [u8; 1] = [0x01];
const CTRL_D: [u8; 1] = [0x04];
const CTRL_L: [u8; 1] = [0x0c];
const CTRL_N: [u8; 1] = [0x0e];
const CTRL_R: [u8; 1] = [0x12];
const CTRL_Z: [u8; 1] = [0x1a];

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn sessions_switch_and_destructive_actions_require_confirmation() {
    let environment = ScenarioEnvironment::prepare();
    environment.seed_completed_session("durable lifecycle prompt", "durable lifecycle response");
    let mut session = PtySession::start(&environment, 30, 100);
    session.wait_for(
        |screen| screen.contents().contains("durable lifecycle response"),
        "seeded session should replay before lifecycle actions",
    );

    session.send_bytes(&CTRL_N);
    session.wait_for(
        |screen| screen.contents().contains("New session created"),
        "Ctrl+N should create and activate a second durable session",
    );
    session.send_bytes(&CTRL_L);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Sessions") && text.contains("Offline seed")
        },
        "Ctrl+L should list both durable sessions",
    );
    session.type_text("Offline seed");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Offline seed")
                && text.contains("Session details")
                && text.contains("Messages")
        },
        "session filter should select the seeded session",
    );
    session.send_bytes(b"\r");
    session.wait_for(
        |screen| screen.contents().contains("durable lifecycle response"),
        "Enter should switch by replaying the selected session",
    );

    session.send_bytes(&CTRL_N);
    session.wait_for(
        |screen| screen.contents().contains("New session created"),
        "a fresh active session should make the seeded session destructible",
    );
    session.send_bytes(&CTRL_L);
    session.wait_for(
        |screen| screen.contents().contains("Offline seed"),
        "the browser query should continue to identify the seeded session",
    );

    session.send_bytes(&CTRL_R);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Rename session")
                && text.contains("Enter save")
                && text.contains("Esc cancel")
        },
        "Ctrl+R should open the selected session title editor",
    );
    session.type_text(" renamed");
    session.send_bytes(b"\r");
    session.wait_for(
        |screen| screen.contents().contains("Offline seed renamed"),
        "renaming should commit through the real coordinator",
    );

    session.send_bytes(&CTRL_A);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Archive session")
                && text.contains("Cancel (N/Esc)")
                && text.contains("Confirm (Y)")
        },
        "archiving should own the confirmation overlay before changing durable state",
    );
    session.send_bytes(b"n");
    session.wait_for(
        |screen| !screen.contents().contains("Confirm (Y)"),
        "N should cancel the archive confirmation",
    );
    session.send_bytes(&CTRL_A);
    session.send_bytes(b"y");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Offline seed renamed") && text.contains("archived")
        },
        "Y should commit the armed archive",
    );
    session.send_bytes(&CTRL_Z);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Offline seed renamed")
                && text.contains("State")
                && text.contains("active")
                && !text.contains("archived")
        },
        "Ctrl+Z should reverse the most recent lifecycle transition once",
    );

    session.send_bytes(&ctrl_c());
    assert_eq!(
        session.wait_for_exit(),
        0,
        "the lifecycle restart boundary should shut down cleanly"
    );
    session = PtySession::start(&environment, 30, 100);
    session.send_bytes(&CTRL_L);
    session.wait_for(
        |screen| screen.contents().contains("Sessions"),
        "the session workspace should reopen after restart",
    );
    session.type_text("renamed");
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Offline seed renamed")
                && text.contains("State")
                && text.contains("active")
                && !text.contains("archived")
        },
        "rename and restored lifecycle state should survive process restart",
    );

    session.send_bytes(&CTRL_D);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Delete session")
                && text.contains("Cancel (N/Esc)")
                && text.contains("Confirm (Y)")
        },
        "deletion should own the confirmation overlay before export or removal",
    );
    session.send_bytes(b"n");
    session.wait_for(
        |screen| !screen.contents().contains("Confirm (Y)"),
        "N should cancel the deletion confirmation",
    );
    session.send_bytes(&CTRL_D);
    session.send_bytes(b"y");
    session.wait_for(
        |screen| !screen.contents().contains("Offline seed renamed"),
        "Y should export then delete the selected session",
    );
    let exported = std::fs::read_dir(environment.data_dir())
        .expect("scenario data directory")
        .filter_map(Result::ok)
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("autoharness-session-pty-offline-session.export.v")
                && name.ends_with(".json")
        });
    assert!(exported, "confirmed deletion must leave its JSON archive");

    // Clear the retained filter and delete the currently open conversation.
    // Another fresh conversation exists, so the coordinator should switch to
    // it after the current session is safely exported and removed.
    session.send_bytes(&[0x7f; 12]);
    session.wait_for(
        |screen| screen.contents().contains("Session details"),
        "clearing the filter should reveal the remaining conversations",
    );
    session.send_bytes(&CTRL_R);
    session.wait_for(
        |screen| screen.contents().contains("Rename session"),
        "the current conversation should remain directly manageable",
    );
    session.type_text(" XDEL");
    session.send_bytes(b"\r");
    session.wait_for(
        |screen| screen.contents().contains("XDEL"),
        "the current conversation should receive its temporary title",
    );
    session.send_bytes(&CTRL_D);
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Delete session") && text.contains("XDEL")
        },
        "Ctrl+D should allow deletion of the current conversation",
    );
    session.send_bytes(b"y");
    session.wait_for(
        |screen| !screen.contents().contains("XDEL"),
        "deleting the current conversation should open the next one",
    );

    session.send_bytes(&ctrl_c());
    assert_eq!(
        session.wait_for_exit(),
        0,
        "lifecycle scenario exits cleanly"
    );
}
