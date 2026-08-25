//! Full profile-management journey through the real terminal binary.

mod pty_support;

use autoharness_settings::{LayerKind, SettingsBuilder};
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const CTRL_G: [u8; 1] = [0x07];
const ALT_D: [u8; 2] = [0x1b, b'd'];
const TAB: [u8; 1] = *b"\t";
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];
const DELETE: [u8; 4] = [0x1b, b'[', b'3', b'~'];

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn profiles_are_created_switched_duplicated_and_deleted_without_shell_setup() {
    let environment = ScenarioEnvironment::prepare();
    let mut terminal = PtySession::start(&environment, 30, 100);

    terminal.send_bytes(&CTRL_G);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Providers & Connections") && text.contains("Google AI Studio")
        },
        "provider catalog should open from credential-free first run",
    );

    terminal.send_bytes(b"\r");
    terminal.wait_for(
        |screen| screen.contents().contains("Create provider profile"),
        "Google AI Studio setup form should open",
    );
    terminal.submit_line("");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("google-ai-studio") && text.contains("Provider profile saved")
        },
        "Google AI Studio profile should save inside the terminal",
    );

    terminal.send_bytes(&DOWN);
    terminal.send_bytes(b"\r");
    terminal.wait_for(
        |screen| screen.contents().contains("Create provider profile"),
        "OpenAI and Codex setup form should open",
    );
    terminal.submit_line("");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("openai-codex") && text.contains("api.openai.com")
        },
        "OpenAI and Codex profile should save with its non-secret connection fields",
    );

    terminal.send_bytes(&TAB);
    terminal.send_bytes(&DOWN);

    terminal.send_bytes(&ALT_D);
    terminal.wait_for(
        |screen| screen.contents().contains("Duplicate provider profile"),
        "duplicate form should open",
    );
    terminal.submit_line("router-copy");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("router-copy") && text.contains("Profile duplicated without a credential")
        },
        "duplicate should copy configuration without a credential",
    );

    terminal.send_bytes(b"\r");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Active provider switched") && text.contains("router-copy")
        },
        "the duplicated profile should become active without leaving the terminal",
    );

    terminal.send_bytes(&DELETE);
    terminal.wait_for(
        |screen| screen.contents().contains("Delete profile 'router-copy'"),
        "profile deletion should require explicit confirmation",
    );
    terminal.send_bytes(b"n");
    terminal.wait_for(
        |screen| !screen.contents().contains("Delete profile 'router-copy'"),
        "cancelled deletion should leave the profile intact",
    );
    terminal.send_bytes(&DELETE);
    terminal.send_bytes(b"y");
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Provider profile deleted") && !text.contains("router-copy")
        },
        "confirmed deletion should remove only the selected profile",
    );

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);

    let document = std::fs::read_to_string(environment.profiles_document())
        .expect("profile document after PTY journey");
    let settings = SettingsBuilder::new()
        .with_layer(LayerKind::UserFile, document)
        .resolve()
        .expect("resolved profiles");
    let names = settings
        .profiles()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["google-ai-studio", "openai-codex"]);
    assert_eq!(settings.active_profile(), None);
}
