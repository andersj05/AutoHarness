//! Forced-process termination and restart recovery through the real terminal.

mod pty_support;

use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const CTRL_N: [u8; 1] = [0x0e];

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn forced_shutdown_leaves_a_recoverable_store() {
    let environment = ScenarioEnvironment::prepare();
    let mut interrupted = PtySession::start(&environment, 24, 80);
    interrupted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("no model") && text.contains("Ask Agent")
        },
        "application should draw before forced termination",
    );
    interrupted.send_bytes(&CTRL_N);
    interrupted.wait_for(
        |screen| screen.contents().contains("New session created"),
        "durable mutation should commit before forced termination",
    );
    interrupted.kill();
    assert_ne!(
        interrupted.wait_for_exit(),
        0,
        "forced termination must not look like a clean exit"
    );
    drop(interrupted);

    let mut recovered = PtySession::start(&environment, 24, 80);
    recovered.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("no model") && text.contains("Ask Agent")
        },
        "restart should reopen and render the store left by forced termination",
    );
    recovered.send_bytes(&ctrl_c());
    assert_eq!(
        recovered.wait_for_exit(),
        0,
        "recovered process exits cleanly"
    );
}
