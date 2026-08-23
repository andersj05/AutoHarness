//! Returning-profile, offline-replay, settings, and resize scenarios through the real terminal.

mod pty_support;

use autoharness_app::profiles::ProfileStore;
use autoharness_settings::ProfileId;
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

#[test]
#[ignore = "runs in the cross-platform terminal PTY CI gate"]
fn returning_profile_replays_offline_and_survives_resize_and_restart() {
    let environment = ScenarioEnvironment::prepare();
    let model =
        environment.seed_completed_session("durable offline prompt", "durable offline response");
    let profiles = ProfileStore::open(&environment.profiles_document()).expect("profile store");
    profiles
        .upsert_profile(
            "home-router",
            r#"{"kind":"router","base_url":"http://127.0.0.1:9/","project":"pty-fixture"}"#,
        )
        .expect("returning profile");
    profiles
        .set_active_profile(Some(&ProfileId::new("home-router").expect("profile ID")))
        .expect("activate returning profile");

    let mut first = PtySession::start(&environment, 24, 80);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("durable offline prompt")
                && text.contains("durable offline response")
                && text.contains("home-router")
                && text.contains("disconnected")
                && text.contains(model.model_id().as_str())
        },
        "returning profile should replay its selected model and transcript offline",
    );
    first.send_bytes(b"\x1b");
    first.wait_for(
        |screen| screen.contents().contains("An API key is still required"),
        "offline profile should permit deferring credential entry",
    );
    first.resize(18, 60);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("AutoHarness")
                && text.contains("durable offline response")
                && text.contains("Ask AutoHarness")
        },
        "resize should redraw the complete active surface without artifacts",
    );
    first.send_bytes(&ctrl_c());
    assert_eq!(first.wait_for_exit(), 0, "first restart leg exits cleanly");
    drop(first);

    let mut restarted = PtySession::start(&environment, 24, 80);
    restarted.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("durable offline response")
                && text.contains("home-router")
                && text.contains("disconnected")
        },
        "offline transcript and profile provenance should survive process restart",
    );
    restarted.send_bytes(b"\x1b");
    restarted.wait_for(
        |screen| screen.contents().contains("An API key is still required"),
        "restarted offline profile should remain dismissible",
    );
    restarted.type_text("offline draft remains editable");
    restarted.wait_for(
        |screen| screen.contents().contains("offline draft remains editable"),
        "composer should accept a draft while the provider is offline",
    );
    restarted.send_bytes(&ctrl_c());
    assert_eq!(restarted.wait_for_exit(), 0, "restart leg exits cleanly");
}
