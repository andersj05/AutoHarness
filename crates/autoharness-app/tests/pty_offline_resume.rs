//! Returning-profile, offline-replay, settings, and resize scenarios through the real terminal.

mod pty_support;

use autoharness_app::profiles::ProfileStore;
use autoharness_settings::{ProfileId, ProviderProfile};
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn returning_profile_replays_offline_and_survives_resize_and_restart() {
    let environment = ScenarioEnvironment::prepare();
    environment.seed_completed_session("durable offline prompt", "durable offline response");
    let profiles = ProfileStore::open(&environment.profiles_document()).expect("profile store");
    let id = ProfileId::new("home-router").expect("profile ID");
    profiles
        .upsert_profile(
            &id,
            &ProviderProfile::router("http://127.0.0.1:9/", Some("pty-fixture".to_owned()), None)
                .expect("router profile"),
        )
        .expect("returning profile");
    profiles
        .set_active_profile(Some(&id))
        .expect("activate returning profile");

    let mut first = PtySession::start(&environment, 24, 80);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("router-offline-mo")
                && text.contains("durable offline prompt")
                && text.contains("durable offline response")
        },
        "returning profile should show the complete offline replay without a credential editor",
    );
    first.resize(18, 60);
    first.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("durable offline response") && text.contains("Ask Agent")
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
            text.contains("router-offline-mo") && text.contains("durable offline response")
        },
        "offline replay should survive process restart without opening a credential editor",
    );
    restarted.type_text("offline draft remains editable");
    restarted.wait_for(
        |screen| screen.contents().contains("offline draft remains editable"),
        "composer should accept a draft while the provider is offline",
    );
    restarted.send_bytes(&ctrl_c());
    assert_eq!(restarted.wait_for_exit(), 0, "restart leg exits cleanly");
}
