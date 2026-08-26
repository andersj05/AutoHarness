//! Full profile-management journey through the real terminal binary.

mod pty_support;

use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const CTRL_G: [u8; 1] = [0x07];
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn providers_open_the_official_codex_subscription_authentication_page() {
    let environment = ScenarioEnvironment::prepare();
    let mut terminal = PtySession::start(&environment, 30, 100);

    terminal.send_bytes(&CTRL_G);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Providers")
                && text.contains("Gemini")
                && text.contains("Google AI Studio API")
                && text.contains("Cursor")
                && text.contains("Codex")
                && text.contains("Claude Code")
        },
        "provider choices should open from credential-free first run",
    );

    for _ in 0..3 {
        terminal.send_bytes(&DOWN);
    }
    terminal.send_bytes(b"\r");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Connect Codex subscription") && text.contains("codex login")
        },
        "Codex should open its subscription authentication page",
    );

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);
}
