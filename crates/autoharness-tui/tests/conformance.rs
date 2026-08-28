use std::fmt::Write as _;
use std::sync::Arc;

use autoharness_domain::{ErrorClass, ModelId, ModelRef, ProviderId};
use autoharness_settings::{LayerKind, SettingsBuilder};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, ColorDepth, CredentialSourceLabel,
    LocalUserProfileProjection, Message, Model, ModelSummary, PermissionDetailView,
    PermissionRequestView, ProfileConnectionState, ProfileCredentialStateLabel, ProfilesProjection,
    ProviderKindLabel, ProviderProfileProjection, RetryPolicy, SessionBrowserEntry,
    SessionProjection, SessionsProjection, SettingsProjection, ToolCallKey, ToolRowView,
    TranscriptItem, UiClock, UiFailure, UsageView, update, view,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui_textarea::{Input, Key};

const SIZES: [(u16, u16); 5] = [(120, 50), (120, 40), (80, 24), (60, 18), (40, 12)];
const THEMES: [&str; 9] = [
    "system", "light", "dark", "aurora", "ember", "midnight", "ocean", "forest", "rose",
];
const COLOR_MODES: [&str; 5] = ["color", "soft", "vivid", "no_color", "high_contrast"];

#[derive(Clone, Copy, Debug)]
enum Surface {
    Chat,
    StreamingChat,
    Sessions,
    Profiles,
    Settings,
    Help,
    ModelPicker,
    SessionCredential,
    InlineCommandPalette,
    ModalCommandPalette,
    TranscriptSearch,
    Permission,
    ProfileCredential,
    UserProfile,
    Confirmation,
    ProfileEditor,
    CodexLogin,
    CodexOpening,
    Startup,
}

impl Surface {
    const ALL: [Self; 19] = [
        Self::Chat,
        Self::StreamingChat,
        Self::Sessions,
        Self::Profiles,
        Self::Settings,
        Self::Help,
        Self::ModelPicker,
        Self::SessionCredential,
        Self::InlineCommandPalette,
        Self::ModalCommandPalette,
        Self::TranscriptSearch,
        Self::Permission,
        Self::ProfileCredential,
        Self::UserProfile,
        Self::Confirmation,
        Self::ProfileEditor,
        Self::CodexLogin,
        Self::CodexOpening,
        Self::Startup,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::StreamingChat => "chat-streaming",
            Self::Sessions => "sessions",
            Self::Profiles => "profiles",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::ModelPicker => "model-picker",
            Self::SessionCredential => "session-credential",
            Self::InlineCommandPalette => "command-palette-inline",
            Self::ModalCommandPalette => "command-palette-modal",
            Self::TranscriptSearch => "transcript-search",
            Self::Permission => "permission",
            Self::ProfileCredential => "profile-credential",
            Self::UserProfile => "user-profile",
            Self::Confirmation => "confirmation",
            Self::ProfileEditor => "profile-editor",
            Self::CodexLogin => "codex-login",
            Self::CodexOpening => "codex-opening",
            Self::Startup => "startup",
        }
    }

    const fn anchor(self) -> &'static str {
        match self {
            Self::Chat => "Agent",
            Self::StreamingChat => "Waiting for the first token",
            Self::Sessions => "Sessions",
            Self::Profiles => "Providers",
            Self::Settings => "Settings",
            Self::Help => "Help",
            Self::ModelPicker => "Models",
            Self::SessionCredential => "Provider API key",
            Self::InlineCommandPalette | Self::ModalCommandPalette => "Commands",
            Self::TranscriptSearch => "matches",
            Self::Permission => "Tool permission",
            Self::ProfileCredential => "credential",
            Self::UserProfile => "User profile",
            Self::Confirmation => "Delete session",
            Self::ProfileEditor => "Connect Gemini",
            Self::CodexLogin => "Sign in to Codex",
            Self::CodexOpening => "Opening your browser",
            Self::Startup => "Starting",
        }
    }
}

#[derive(Clone)]
struct MatrixProfile {
    name: String,
    theme: &'static str,
    color_mode: &'static str,
    glyph_mode: &'static str,
    reduced_motion: bool,
    density: &'static str,
    layout: &'static str,
    depth: ColorDepth,
}

fn model_ref(id: &str) -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("provider"),
        ModelId::new(id).expect("model"),
    )
}

fn selected_model() -> ModelRef {
    model_ref("models/gemini-conformance-pro")
}

fn catalog() -> Arc<CatalogProjection> {
    Arc::new(CatalogProjection::Ready {
        models: vec![
            ModelSummary {
                model: selected_model(),
                display_name: "Gemini Conformance Pro with a deliberately long display name"
                    .to_owned(),
                detail: "reasoning | tools | multimodal".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            },
            ModelSummary {
                model: model_ref("models/gemini-conformance-flash"),
                display_name: "Gemini Conformance Flash".to_owned(),
                detail: "fast | text".to_owned(),
                context_window_tokens: Some(1_000_000),
                selectable: true,
            },
        ],
        stale: false,
    })
}

fn session(transcript: Vec<TranscriptItem>) -> Arc<SessionProjection> {
    Arc::new(SessionProjection {
        session_id: "conformance-active".to_owned(),
        revision: 12,
        selected_model: Some(selected_model()),
        transcript,
        permission_requests: Vec::new(),
    })
}

fn sessions() -> Arc<SessionsProjection> {
    Arc::new(SessionsProjection {
        sessions: vec![
            SessionBrowserEntry {
                session_id: "conformance-active".to_owned(),
                title: "Responsive conformance review with a deliberately long title".to_owned(),
                archived: false,
                selected_model: Some(selected_model()),
                message_count: 14,
                updated_at_ms: 1_700_000_095_000,
                active: true,
            },
            SessionBrowserEntry {
                session_id: "conformance-archived".to_owned(),
                title: "Archived accessibility evidence".to_owned(),
                archived: true,
                selected_model: None,
                message_count: 3,
                updated_at_ms: 1_699_900_000_000,
                active: false,
            },
        ],
    })
}

fn profiles() -> Arc<ProfilesProjection> {
    Arc::new(ProfilesProjection {
        user: LocalUserProfileProjection {
            display_label: Some("Conformance user".to_owned()),
            workspace: "C:/work/autoharness".to_owned(),
            default_profile: Some("personal-gemini".to_owned()),
            default_model: Some("gemini-conformance-pro".to_owned()),
            default_mode: "high".to_owned(),
        },
        profiles: vec![
            ProviderProfileProjection {
                id: "personal-gemini".to_owned(),
                kind: ProviderKindLabel::Gemini,
                active: true,
                base_url: String::new(),
                project: String::new(),
                auth_header: String::new(),
                credential_state: ProfileCredentialStateLabel::Stored,
                credential_source: CredentialSourceLabel::CredentialVault,
                connection: ProfileConnectionState::Ready,
                default_model: Some("gemini-conformance-pro".to_owned()),
                default_mode: "high".to_owned(),
            },
            ProviderProfileProjection {
                id: "work-router".to_owned(),
                kind: ProviderKindLabel::Router,
                active: false,
                base_url: "https://router.example.test/v1/".to_owned(),
                project: "conformance".to_owned(),
                auth_header: "x-router-key".to_owned(),
                credential_state: ProfileCredentialStateLabel::Disconnected,
                credential_source: CredentialSourceLabel::SessionOnly,
                connection: ProfileConnectionState::Untested,
                default_model: None,
                default_mode: "safe agent".to_owned(),
            },
        ],
        pending_recovery: 0,
    })
}

fn transcript() -> Vec<TranscriptItem> {
    vec![
        TranscriptItem::User {
            input_id: "input-conformance".to_owned(),
            text: "Review every responsive breakpoint without clipping important content."
                .to_owned(),
        },
        TranscriptItem::Tool(ToolRowView {
            tool_call_id: ToolCallKey::new("tool-conformance").expect("tool call"),
            tool_name: "fs_read".to_owned(),
            resource: "workspace:docs/design/TUI_REDESIGN_PLAN.md".to_owned(),
            status: "completed".to_owned(),
            summary: Some("Phase 3.10 step 9 loaded".to_owned()),
        }),
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-complete").expect("attempt"),
            text: "The matrix covers routes, overlays, themes, color modes, glyphs, motion, density, and layout."
                .to_owned(),
            status: AttemptStatus::Completed,
            usage: Some(UsageView {
                input_tokens: 2_048,
                output_tokens: 512,
            }),
            retry_of: None,
        },
        TranscriptItem::Assistant {
            attempt_id: AttemptKey::new("attempt-failed").expect("attempt"),
            text: "A deliberately visible failure state.".to_owned(),
            status: AttemptStatus::Failed(UiFailure::new(
                ErrorClass::Unavailable,
                "Provider temporarily unavailable",
                RetryPolicy::Now,
            )),
            usage: None,
            retry_of: Some(AttemptKey::new("attempt-complete").expect("attempt")),
        },
    ]
}

fn base_model() -> Model {
    let mut model = Model::new(session(transcript()), sessions(), catalog());
    model.apply_profiles(profiles());
    let _ = update(
        &mut model,
        Message::Tick(UiClock::new(1_400, 1_700_000_100_000)),
    );
    model
}

fn key(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn ctrl(character: char) -> Input {
    Input {
        ctrl: true,
        ..key(Key::Char(character))
    }
}

fn alt(character: char) -> Input {
    Input {
        alt: true,
        ..key(Key::Char(character))
    }
}

fn press(model: &mut Model, input: Input) {
    let _ = update(model, Message::Input(input));
}

fn surface_model(surface: Surface) -> Model {
    if matches!(surface, Surface::Startup) {
        return Model::new(
            Arc::new(SessionProjection {
                session_id: "conformance-startup".to_owned(),
                revision: 1,
                selected_model: None,
                transcript: Vec::new(),
                permission_requests: Vec::new(),
            }),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        );
    }

    let mut model = base_model();
    match surface {
        Surface::Chat => {}
        Surface::StreamingChat => {
            let mut projection = (*model.session).clone();
            projection.revision += 1;
            projection.transcript.push(TranscriptItem::Assistant {
                attempt_id: AttemptKey::new("attempt-streaming").expect("attempt"),
                text: String::new(),
                status: AttemptStatus::Streaming,
                usage: None,
                retry_of: None,
            });
            let _ = update(&mut model, Message::SessionChanged(Arc::new(projection)));
        }
        Surface::Sessions => press(&mut model, ctrl('2')),
        Surface::Profiles => press(&mut model, ctrl('3')),
        Surface::Settings => press(&mut model, ctrl('4')),
        Surface::Help => press(&mut model, ctrl('5')),
        Surface::ModelPicker => press(&mut model, ctrl('p')),
        Surface::SessionCredential => press(&mut model, ctrl('k')),
        Surface::InlineCommandPalette => press(&mut model, ctrl('/')),
        Surface::ModalCommandPalette => {
            press(&mut model, ctrl('2'));
            press(&mut model, ctrl('/'));
        }
        Surface::TranscriptSearch => {
            press(&mut model, ctrl('f'));
            for character in "matrix".chars() {
                press(&mut model, key(Key::Char(character)));
            }
        }
        Surface::Permission => {
            let mut projection = (*model.session).clone();
            projection.revision += 1;
            projection.permission_requests.push(PermissionRequestView {
                tool_call_id: ToolCallKey::new("permission-conformance").expect("tool call"),
                tool_name: "fs_write".to_owned(),
                capability: "filesystem write".to_owned(),
                resource: "workspace:src/lib.rs".to_owned(),
                details: vec![PermissionDetailView {
                    label: "Path".to_owned(),
                    value: "src/lib.rs".to_owned(),
                }],
            });
            let _ = update(&mut model, Message::SessionChanged(Arc::new(projection)));
        }
        Surface::ProfileCredential => {
            press(&mut model, ctrl('g'));
            press(&mut model, key(Key::Down));
            press(&mut model, alt('k'));
        }
        Surface::UserProfile => press(&mut model, alt('u')),
        Surface::Confirmation => {
            press(&mut model, ctrl('2'));
            press(&mut model, key(Key::Down));
            press(&mut model, ctrl('d'));
        }
        Surface::ProfileEditor => {
            press(&mut model, ctrl('g'));
            press(&mut model, key(Key::Enter));
        }
        Surface::CodexLogin | Surface::CodexOpening => {
            press(&mut model, ctrl('g'));
            for _ in 0..3 {
                press(&mut model, key(Key::Down));
            }
            press(&mut model, key(Key::Enter));
            if matches!(surface, Surface::CodexOpening) {
                press(&mut model, key(Key::Enter));
            }
        }
        Surface::Startup => unreachable!(),
    }
    model
}

fn apply_profile(model: &mut Model, profile: &MatrixProfile) {
    let settings = SettingsBuilder::new()
        .with_layer(
            LayerKind::UserFile,
            format!(
                r#"{{"schema_version":4,"local_profile":{{"preferences":{{"theme_preset":"{}","color_mode":"{}","glyph_mode":"{}","reduced_motion":{},"density":"{}","layout":"{}","terminal_timestamp_style":"relative"}}}}}}"#,
                profile.theme,
                profile.color_mode,
                profile.glyph_mode,
                profile.reduced_motion,
                profile.density,
                profile.layout,
            ),
        )
        .resolve()
        .expect("conformance preferences");
    model.apply_settings(Arc::new(SettingsProjection {
        local_profile: settings.local_profile().clone(),
        git_branch: Some("feat/tui-conformance".to_owned()),
        ..SettingsProjection::default()
    }));
    model.set_color_depth(profile.depth);
}

fn profiles_under_review() -> Vec<MatrixProfile> {
    let mut profiles = Vec::new();
    for theme in THEMES {
        for color_mode in COLOR_MODES {
            profiles.push(MatrixProfile {
                name: format!("{theme}-{color_mode}"),
                theme,
                color_mode,
                glyph_mode: "unicode",
                reduced_motion: false,
                density: "comfortable",
                layout: "responsive",
                depth: ColorDepth::TrueColor,
            });
        }
    }
    profiles.extend([
        MatrixProfile {
            name: "glyph-unicode".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "glyph-ascii".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "ascii",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "glyph-nerd-font".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "nerd_font",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "reduced-motion".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: true,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "compact-density".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "compact",
            layout: "responsive",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "single-column".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "single_column",
            depth: ColorDepth::TrueColor,
        },
        MatrixProfile {
            name: "indexed-256".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::Indexed256,
        },
        MatrixProfile {
            name: "basic-16".to_owned(),
            theme: "system",
            color_mode: "color",
            glyph_mode: "unicode",
            reduced_motion: false,
            density: "comfortable",
            layout: "responsive",
            depth: ColorDepth::Basic16,
        },
    ]);
    profiles
}

fn render(model: &Model, width: u16, height: u16) -> TestBackend {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| view(frame, model)).expect("draw");
    terminal.backend().clone()
}

fn buffer_text(buffer: &Buffer) -> String {
    let mut rendered = String::new();
    for y in buffer.area.y..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            rendered.push_str(buffer.cell((x, y)).expect("cell").symbol());
        }
        rendered.push('\n');
    }
    rendered
}

fn update_hash(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

fn hash_color(hash: &mut u64, color: Color) {
    let stable = match color {
        Color::Reset => "reset".to_owned(),
        Color::Black => "black".to_owned(),
        Color::Red => "red".to_owned(),
        Color::Green => "green".to_owned(),
        Color::Yellow => "yellow".to_owned(),
        Color::Blue => "blue".to_owned(),
        Color::Magenta => "magenta".to_owned(),
        Color::Cyan => "cyan".to_owned(),
        Color::Gray => "gray".to_owned(),
        Color::DarkGray => "dark-gray".to_owned(),
        Color::LightRed => "light-red".to_owned(),
        Color::LightGreen => "light-green".to_owned(),
        Color::LightYellow => "light-yellow".to_owned(),
        Color::LightBlue => "light-blue".to_owned(),
        Color::LightMagenta => "light-magenta".to_owned(),
        Color::LightCyan => "light-cyan".to_owned(),
        Color::White => "white".to_owned(),
        Color::Indexed(value) => format!("indexed-{value}"),
        Color::Rgb(red, green, blue) => format!("rgb-{red}-{green}-{blue}"),
    };
    update_hash(hash, stable.as_bytes());
}

fn style_digest(buffer: &Buffer) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325;
    update_hash(&mut hash, &buffer.area.width.to_le_bytes());
    update_hash(&mut hash, &buffer.area.height.to_le_bytes());
    for cell in buffer.content() {
        update_hash(&mut hash, cell.symbol().as_bytes());
        update_hash(&mut hash, &[0]);
        hash_color(&mut hash, cell.fg);
        hash_color(&mut hash, cell.bg);
        update_hash(&mut hash, &cell.modifier.bits().to_le_bytes());
    }
    hash
}

fn assert_surface_contract(
    surface: Surface,
    profile: &MatrixProfile,
    width: u16,
    height: u16,
    buffer: &Buffer,
) {
    let rendered = buffer_text(buffer);
    let rendered_folded = rendered.to_lowercase();
    assert!(
        rendered_folded.contains(&surface.anchor().to_lowercase()),
        "{} missing anchor {:?} in {} at {width}x{height}\n{rendered}",
        surface.name(),
        surface.anchor(),
        profile.name,
    );
    assert!(
        !rendered.contains('\u{fffd}'),
        "{} emitted a replacement glyph in {} at {width}x{height}",
        surface.name(),
        profile.name,
    );
    if profile.glyph_mode == "ascii" {
        let non_ascii = rendered
            .chars()
            .filter(|character| !character.is_ascii())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            non_ascii.is_empty(),
            "{} emitted non-ASCII {non_ascii:?} in {} at {width}x{height}",
            surface.name(),
            profile.name,
        );
    }
}

fn matrix_manifest() -> String {
    let mut manifest = String::from("# AutoHarness TUI conformance matrix v1\n");
    for surface in Surface::ALL {
        for profile in profiles_under_review() {
            for (width, height) in SIZES {
                let mut model = surface_model(surface);
                apply_profile(&mut model, &profile);
                let backend = render(&model, width, height);
                assert_surface_contract(surface, &profile, width, height, backend.buffer());
                writeln!(
                    manifest,
                    "{}|{}|{}x{}|{:016x}",
                    surface.name(),
                    profile.name,
                    width,
                    height,
                    style_digest(backend.buffer()),
                )
                .expect("manifest line");
            }
        }
    }
    manifest
}

#[test]
fn full_visual_conformance_matrix_matches_reviewed_manifest() {
    let actual = matrix_manifest();
    let expected = include_str!("conformance/matrix-v1.txt");
    if std::env::var("AUTOHARNESS_UPDATE_CONFORMANCE").as_deref() == Ok("1") {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("conformance")
            .join("matrix-v1.txt");
        std::fs::write(path, &actual).expect("write conformance manifest");
    } else {
        assert_eq!(actual, expected, "conformance manifest changed");
    }
}

#[test]
fn reduced_motion_freezes_every_surface() {
    let profile = MatrixProfile {
        name: "reduced-motion".to_owned(),
        theme: "system",
        color_mode: "color",
        glyph_mode: "unicode",
        reduced_motion: true,
        density: "comfortable",
        layout: "responsive",
        depth: ColorDepth::TrueColor,
    };
    for surface in Surface::ALL {
        let mut model = surface_model(surface);
        apply_profile(&mut model, &profile);
        if matches!(surface, Surface::Startup) {
            let _ = update(&mut model, Message::Tick(UiClock::new(100, 0)));
        }
        let first = render(&model, 80, 24);
        let next_tick = if matches!(surface, Surface::Startup) {
            UiClock::new(200, 0)
        } else {
            UiClock::new(1_900, 1_700_000_100_000)
        };
        let _ = update(&mut model, Message::Tick(next_tick));
        let second = render(&model, 80, 24);
        assert_eq!(
            style_digest(first.buffer()),
            style_digest(second.buffer()),
            "{} changed with reduced motion enabled",
            surface.name(),
        );
    }
}
