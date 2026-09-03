//! Terminal-capability smokes through the real binary and pseudo-terminal.
//!
//! These scenarios exercise the exact glyph and color-depth paths selected at
//! process startup. A human font review still verifies glyph shape, while this
//! gate proves the candidate emits the intended characters and escape classes.

mod pty_support;

use autoharness_app::profiles::ProfileStore;
use autoharness_settings::{GlyphMode, LocalPreferences, LocalProfile};
use pty_support::{PtySession, ScenarioEnvironment, ctrl_c};

const ALT_4: [u8; 2] = [0x1b, b'4'];
const DOWN: [u8; 3] = [0x1b, b'[', b'B'];

fn persist_glyph_mode(environment: &ScenarioEnvironment, glyph_mode: GlyphMode) {
    let store = ProfileStore::open(&environment.profiles_document()).expect("profile store");
    let mut preferences = LocalPreferences::new();
    preferences.set_glyph_mode(Some(glyph_mode));
    let mut local_profile = LocalProfile::new();
    local_profile.set_preferences(preferences);
    store
        .set_local_profile(local_profile)
        .expect("persist glyph mode");
}

fn wait_for_chat(session: &mut PtySession) {
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("AutoHarness") && text.contains("no model") && text.contains("Ask Agent")
        },
        "the terminal-capability smoke should reach the complete Chat surface",
    );
}

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn nerd_font_mode_emits_reserved_private_use_glyphs_without_replacement_characters() {
    let environment = ScenarioEnvironment::prepare();
    persist_glyph_mode(&environment, GlyphMode::NerdFont);
    let mut session = PtySession::start(&environment, 30, 100);
    wait_for_chat(&mut session);

    let text = session.screen_text();
    assert!(
        text.chars()
            .any(|character| ('\u{e000}'..='\u{f8ff}').contains(&character)),
        "Nerd Font mode should emit at least one BMP private-use glyph"
    );
    assert!(
        !text.contains('\u{fffd}'),
        "the terminal stream must not replace a Nerd Font glyph"
    );

    session.send_bytes(&ctrl_c());
    assert_eq!(session.wait_for_exit(), 0);
}

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn unicode_mode_emits_portable_glyphs_without_private_use_dependencies() {
    let environment = ScenarioEnvironment::prepare();
    persist_glyph_mode(&environment, GlyphMode::Unicode);
    let mut session = PtySession::start(&environment, 30, 100);
    wait_for_chat(&mut session);

    let text = session.screen_text();
    assert!(text.contains("▣ Chat"));
    assert!(
        !text
            .chars()
            .any(|character| ('\u{e000}'..='\u{f8ff}').contains(&character)),
        "Unicode mode must not depend on a private-use font glyph"
    );
    assert!(!text.contains('\u{fffd}'));

    session.send_bytes(&ctrl_c());
    assert_eq!(session.wait_for_exit(), 0);
}

#[test]
#[ignore = "legacy terminal migration reference; run deliberately"]
fn basic_sixteen_color_mode_reports_capability_and_avoids_extended_color_sequences() {
    let mut environment = ScenarioEnvironment::prepare();
    environment.remove("COLORTERM");
    environment.insert("TERM", "xterm");
    let mut session = PtySession::start(&environment, 30, 100);
    wait_for_chat(&mut session);

    session.send_bytes(&ALT_4);
    for _ in 0..8 {
        session.send_bytes(&DOWN);
    }
    session.wait_for(
        |screen| {
            let text = screen.contents();
            text.contains("About") && text.contains("Terminal colors") && text.contains("16 colors")
        },
        "Settings should report the detected sixteen-color terminal capability",
    );

    let raw = session.raw_output();
    for forbidden in [
        b"\x1b[38;2;".as_slice(),
        b"\x1b[48;2;",
        b"\x1b[38;5;",
        b"\x1b[48;5;",
    ] {
        assert!(
            !raw.windows(forbidden.len())
                .any(|window| window == forbidden),
            "Basic16 output must not emit truecolor or indexed-color escapes"
        );
    }

    session.send_bytes(&ctrl_c());
    assert_eq!(session.wait_for_exit(), 0);
}
