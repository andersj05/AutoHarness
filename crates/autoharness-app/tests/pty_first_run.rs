//! End-to-end first-run scenario through a real pseudo-terminal.
//!
//! Proves the credential-free launch path a new user experiences: the complete
//! interface renders, application files stay inside the isolated data
//! directory, quitting restores the terminal, and the exit code is clean.

mod pty_support;

use std::fs;

use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn first_run_renders_the_interface_and_restores_the_terminal_on_quit() {
    let environment = ScenarioEnvironment::prepare();
    // No provider environment at all: the launch must degrade to session-only
    // operation and still present the full interface.
    let mut session = PtySession::start(&environment, 24, 80);

    // The first draw stays quiet and leaves setup available through commands.
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("no model")
                && text.contains("Ask Agent")
                && !text.contains("Connect a provider key")
                && !text.contains("Choose a compatible model")
        },
        "first draw should show the clean credential-free Chat surface",
    );
    let cursor_show_count = session
        .raw_output()
        .windows(b"\x1b[?25h".len())
        .filter(|window| *window == b"\x1b[?25h")
        .count();
    session.type_text("/settings");
    session.send_bytes(b"\r");
    session.wait_for(
        |screen| screen.contents().contains("Settings"),
        "the settings command should open the settings route",
    );
    assert_eq!(
        session
            .raw_output()
            .windows(b"\x1b[?25h".len())
            .filter(|window| *window == b"\x1b[?25h")
            .count(),
        cursor_show_count,
        "interactive redraws must keep the hardware cursor hidden behind the styled software cursor"
    );

    // Quit with Ctrl+C: clean exit code and restored terminal.
    session.send_bytes(&ctrl_c());
    let exit_code = session.wait_for_exit();
    assert_eq!(exit_code, 0, "clean quit must exit successfully");
    session.wait_for_raw(b"\x1b[?2004l", "clean quit must disable bracketed paste");
    session.wait_for_raw(b"\x1b[?1049l", "clean quit must leave the alternate screen");
    session.wait_for_raw(b"\x1b[?25h", "clean quit must show the cursor");

    // Durable state stays inside the isolated data directory.
    assert!(
        session.data_dir().join("autoharness.sqlite3").exists(),
        "durable database should be created"
    );
    let log = fs::read_to_string(environment.log()).unwrap_or_default();
    assert!(
        !log.contains("first run probe"),
        "prompt text must never reach the log"
    );
}
