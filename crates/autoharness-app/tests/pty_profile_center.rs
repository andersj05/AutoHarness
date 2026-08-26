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
fn unavailable_saved_codex_provider_keeps_recovery_ui_open() {
    let mut environment = ScenarioEnvironment::prepare();
    let unavailable_codex = environment.data_dir().join("codex.exe");
    std::fs::create_dir(&unavailable_codex).expect("unlaunchable Codex path");
    environment.insert(
        "AUTOHARNESS_CODEX_EXECUTABLE",
        unavailable_codex.as_os_str(),
    );

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
            text.contains("CONNECTION ERROR") && text.contains("provider")
        },
        "a Codex probe failure should render a recoverable app state",
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
        "provider recovery should remain reachable after a Codex probe failure",
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
        let fake_codex = environment.data_dir().join("codex.cmd");
        std::fs::write(
            &fake_codex,
            "@echo off\r\nif \"%~1\"==\"login\" type nul > \"%AUTOHARNESS_DATA_DIR%\\codex-login-launched\"\r\n",
        )
        .expect("write fake Codex CLI");
        environment.insert("AUTOHARNESS_CODEX_EXECUTABLE", fake_codex.as_os_str());
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
            text.contains("Sign in to Codex") && text.contains("Open browser sign-in")
        },
        "Codex should open its subscription authentication page",
    );

    #[cfg(windows)]
    {
        terminal.send_bytes(b"\r");
        terminal.wait_for(
            |screen| screen.contents().contains("Browser sign-in started"),
            "Codex login should be dispatched from the real terminal",
        );
        let marker = environment.data_dir().join("codex-login-launched");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            marker.exists(),
            "the Codex login subprocess should receive 'login'"
        );
    }

    terminal.send_bytes(&ctrl_c());
    assert_eq!(terminal.wait_for_exit(), 0);
}
