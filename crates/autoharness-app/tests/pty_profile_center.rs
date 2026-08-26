//! Full profile-management journey through the real terminal binary.

mod pty_support;

#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
use autoharness_app::profiles::{ProfileManager, ProfileStore};
#[cfg(windows)]
use autoharness_app::vault::FakeVault;
#[cfg(windows)]
use autoharness_settings::{ProfileId, ProviderProfile};
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const CTRL_G: [u8; 1] = [0x07];
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];

#[cfg(windows)]
#[test]
#[ignore = "runs in the Windows terminal PTY CI gate"]
fn saved_codex_profile_without_login_keeps_recovery_ui_open() {
    let environment = ScenarioEnvironment::prepare();
    let store = ProfileStore::open(&environment.profiles_document()).expect("profile store");
    let manager = ProfileManager::new(store, Arc::new(FakeVault::new()));
    let profile_id = ProfileId::new("codex-subscription").expect("profile ID");
    manager
        .upsert(&profile_id, &ProviderProfile::codex_cli())
        .expect("save Codex profile");
    manager
        .activate(Some(&profile_id))
        .expect("activate Codex profile");

    let mut terminal = PtySession::start(&environment, 30, 100);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("OFFLINE") && text.contains("credential")
        },
        "a Codex profile without login should render a recoverable app state",
    );

    terminal.send_bytes(&CTRL_G);
    terminal.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("Providers")
                && text.contains("Gemini")
                && text.contains("Codex")
                && text.contains("Claude Code")
        },
        "provider recovery should remain reachable without a Codex login",
    );

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);
}

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn providers_open_the_official_codex_subscription_authentication_page() {
    let environment = ScenarioEnvironment::prepare();
    #[cfg(windows)]
    let mut environment = environment;
    #[cfg(windows)]
    {
        environment.insert("AUTOHARNESS_BROWSER_EXECUTABLE", "where.exe");
    }
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
            text.contains("Sign in to Codex") && text.contains("Sign in with ChatGPT")
        },
        "Codex should open its subscription authentication page",
    );

    #[cfg(windows)]
    {
        terminal.send_bytes(b"\r");
        terminal.wait_for(
            |screen| screen.contents().contains("Browser opened"),
            "Codex login should be dispatched from the real terminal",
        );
        terminal.send_bytes(&[0x1b]);
        terminal.wait_for(
            |screen| !screen.contents().contains("Sign in to Codex"),
            "Escape should cancel the pending browser sign-in",
        );
    }

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);
}
