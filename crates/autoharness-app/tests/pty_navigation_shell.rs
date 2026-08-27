//! Route, overlay, draft, confirmation, and responsive-shell journey through the real binary.

mod pty_support;

use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const ALT_1: [u8; 2] = [0x1b, b'1'];
const ALT_2: [u8; 2] = [0x1b, b'2'];
const ALT_3: [u8; 2] = [0x1b, b'3'];
const ALT_4: [u8; 2] = [0x1b, b'4'];
const ALT_5: [u8; 2] = [0x1b, b'5'];
const CTRL_N: [u8; 1] = [0x0e];
const CTRL_P: [u8; 1] = [0x10];
const CTRL_D: [u8; 1] = [0x04];
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];
const RIGHT: [u8; 3] = [0x1b, b'[', b'C'];

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn routed_shell_restores_focus_drafts_confirmations_and_terminal_state() {
    let environment = ScenarioEnvironment::prepare();
    environment.seed_completed_session("seeded navigation prompt", "seeded navigation response");
    let mut terminal = PtySession::start(&environment, 40, 120);

    terminal.send_bytes(&ALT_2);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("AutoHarness")
                && text.contains("Sessions")
                && text.contains("Session details")
                && text.contains("Offline seed")
                && text.contains("active")
                && text.contains("[ Open ]")
        },
        "Alt+2 should leave first-run credential entry for the Sessions route",
    );

    terminal.send_bytes(&ALT_3);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Providers")
                && text.contains("Choose a provider on the left")
                && text.contains("Provider catalog")
                && text.contains("Saved connections")
        },
        "Alt+3 should open Providers",
    );
    terminal.send_bytes(&ALT_4);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Settings")
                && text.contains("Appearance")
                && text.contains("Glyph mode")
                && text.contains("Ctrl+F search")
        },
        "Alt+4 should open Settings",
    );
    terminal.send_bytes(&CTRL_P);
    terminal.wait_for(
        |screen| screen.contents().contains("Models"),
        "model picker should overlay Settings",
    );
    terminal.send_bytes(b"\x1b");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Settings") && text.contains("Appearance")
        },
        "Esc should restore the exact Settings route",
    );
    terminal.send_bytes(b"\r");
    for _ in 0..2 {
        terminal.send_bytes(&DOWN);
    }
    terminal.send_bytes(&RIGHT);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Glyph mode")
                && text.contains("Nerd Font")
                && text.contains("2/3")
                && !text.contains("[saving]")
        },
        "Settings should persist Nerd Font mode before accepting another option",
    );
    terminal.send_bytes(&RIGHT);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Glyph mode")
                && text.contains("ASCII")
                && text.contains("3/3")
                && !text.contains("[saving]")
        },
        "Settings should persist an ASCII chrome preference without leaving the route",
    );
    terminal.send_bytes(&ALT_5);
    terminal.wait_for(
        |screen| screen.contents().contains("Help"),
        "Alt+5 should open Help",
    );
    terminal.send_bytes(&ALT_1);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("seeded navigation response")
                && text.contains("Ask AutoHarness")
                && !text.contains("Conversation")
        },
        "Alt+1 should return to replayable offline Chat",
    );

    terminal.type_text("draft survives routed shell");
    terminal.send_bytes(&ALT_2);
    terminal.send_bytes(&ALT_1);
    terminal.wait_for(
        |screen| screen.contents().contains("draft survives routed shell"),
        "composer draft must survive route navigation",
    );

    terminal.send_bytes(&CTRL_N);
    terminal.wait_for(
        |screen| screen.contents().contains("New session created"),
        "global new session should remain available",
    );
    terminal.send_bytes(&ALT_2);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Untitled session") && text.contains("Offline seed")
        },
        "Sessions should list both durable conversations",
    );
    terminal.type_text("Offline seed");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("/ Offline seed") && text.contains("Session details")
        },
        "session filter should select the non-active session",
    );
    terminal.send_bytes(&CTRL_D);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Delete session")
                && text.contains("Cancel (N/Esc)")
                && text.contains("Confirm (Y)")
        },
        "destructive confirmation should own one modal slot",
    );
    terminal.send_bytes(b"n");
    terminal.wait_for(
        |screen| !screen.contents().contains("Confirm (Y)"),
        "N should cancel and restore Sessions",
    );

    terminal.send_bytes(&ALT_1);
    terminal.resize(12, 40);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("AutoHarness")
                && text.contains("Offline")
                && text.contains("Connect a provider key")
                && text.contains("/settings")
                && text.contains("Ask AutoHarness")
                && !text.contains("Profile")
        },
        "narrow layout should retain the primary recovery path without a redundant footer",
    );

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);
    let settings = std::fs::read_to_string(environment.profiles_document())
        .expect("persisted Settings preference");
    assert!(settings.contains("\"glyph_mode\": \"ascii\""));
}
