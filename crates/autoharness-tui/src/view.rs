use std::fmt::Write as _;

use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, Layout as PreferenceLayout,
    PromptStatusDetail, TerminalTimestampStyle, ThemePreset,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    AttemptStatus, COMMANDS, CatalogProjection, Focus, Model, ModelDefaultStep, ModelSummary,
    MouseAction, Notice, OverlayKind, PROVIDER_CHOICES, PendingKind, ProfileCenterFocus,
    ProfileConnectionState, ProfileCredentialAction, ProfileEditorMode, ProviderKindLabel,
    RetryPolicy, Route, SettingsPreference, TranscriptItem,
};
use crate::text::display_safe;

const ASCII_BORDER: ratatui::symbols::border::Set<'static> = ratatui::symbols::border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

#[derive(Clone, Copy)]
enum VisualRole {
    Normal,
    Header,
    Muted,
    User,
    Assistant,
    Error,
    Tool,
    Selected,
    Border,
    Warning,
    Field,
}

#[derive(Clone, Copy)]
struct Presentation {
    color_mode: ColorMode,
    theme: ThemePreset,
    ascii: bool,
    nerd_font: bool,
    reduced_motion: bool,
    compact: bool,
    single_column: bool,
}

#[derive(Clone, Copy)]
struct ShellLayout {
    sidebar: Option<Rect>,
    content: Rect,
    footer: Rect,
}
fn presentation(model: &Model) -> Presentation {
    let preferences = model.settings().local_profile.preferences();
    Presentation {
        color_mode: *preferences.color_mode().value(),
        theme: *preferences.theme_preset().value(),
        ascii: *preferences.glyph_mode().value() == GlyphMode::Ascii,
        nerd_font: *preferences.glyph_mode().value() == GlyphMode::NerdFont,
        reduced_motion: *preferences.reduced_motion().value(),
        compact: *preferences.density().value() == Density::Compact,
        single_column: *preferences.layout().value() == PreferenceLayout::SingleColumn,
    }
}

fn extra_theme_style(theme: ThemePreset, role: VisualRole) -> Style {
    let (background, normal, header, selected, user, assistant, error, tool, warning, field) =
        match theme {
            ThemePreset::Aurora => (
                Color::Rgb(4, 15, 30),
                Color::Rgb(224, 242, 254),
                Color::Rgb(45, 212, 191),
                Color::Rgb(129, 140, 248),
                Color::Rgb(56, 189, 248),
                Color::Rgb(94, 234, 212),
                Color::Rgb(251, 113, 133),
                Color::Rgb(167, 139, 250),
                Color::Rgb(250, 204, 21),
                Color::Rgb(15, 35, 60),
            ),
            ThemePreset::Ember => (
                Color::Rgb(26, 10, 10),
                Color::Rgb(255, 241, 232),
                Color::Rgb(251, 146, 60),
                Color::Rgb(244, 63, 94),
                Color::Rgb(253, 186, 116),
                Color::Rgb(251, 191, 36),
                Color::Rgb(248, 113, 113),
                Color::Rgb(232, 121, 249),
                Color::Rgb(251, 146, 60),
                Color::Rgb(62, 24, 20),
            ),
            ThemePreset::Midnight => (
                Color::Rgb(3, 7, 18),
                Color::Rgb(226, 232, 240),
                Color::Rgb(96, 165, 250),
                Color::Rgb(99, 102, 241),
                Color::Rgb(147, 197, 253),
                Color::Rgb(129, 140, 248),
                Color::Rgb(248, 113, 113),
                Color::Rgb(192, 132, 252),
                Color::Rgb(250, 204, 21),
                Color::Rgb(17, 24, 39),
            ),
            ThemePreset::Ocean => (
                Color::Rgb(2, 20, 32),
                Color::Rgb(224, 247, 250),
                Color::Rgb(34, 211, 238),
                Color::Rgb(14, 165, 233),
                Color::Rgb(56, 189, 248),
                Color::Rgb(45, 212, 191),
                Color::Rgb(251, 113, 133),
                Color::Rgb(103, 232, 249),
                Color::Rgb(253, 224, 71),
                Color::Rgb(8, 47, 73),
            ),
            ThemePreset::Forest => (
                Color::Rgb(7, 20, 13),
                Color::Rgb(236, 253, 245),
                Color::Rgb(74, 222, 128),
                Color::Rgb(34, 197, 94),
                Color::Rgb(134, 239, 172),
                Color::Rgb(45, 212, 191),
                Color::Rgb(251, 113, 133),
                Color::Rgb(163, 230, 53),
                Color::Rgb(251, 191, 36),
                Color::Rgb(20, 48, 31),
            ),
            ThemePreset::Rose => (
                Color::Rgb(29, 8, 20),
                Color::Rgb(255, 241, 246),
                Color::Rgb(244, 114, 182),
                Color::Rgb(236, 72, 153),
                Color::Rgb(251, 113, 133),
                Color::Rgb(232, 121, 249),
                Color::Rgb(251, 113, 133),
                Color::Rgb(216, 180, 254),
                Color::Rgb(253, 186, 116),
                Color::Rgb(66, 20, 45),
            ),
            ThemePreset::System | ThemePreset::Light | ThemePreset::Dark => {
                unreachable!("base themes use dedicated palettes")
            }
        };
    match role {
        VisualRole::Normal => Style::new().fg(normal).bg(background),
        VisualRole::Header => Style::new()
            .fg(Color::Rgb(8, 12, 24))
            .bg(header)
            .add_modifier(Modifier::BOLD),
        VisualRole::Selected => Style::new()
            .fg(Color::Rgb(8, 12, 24))
            .bg(selected)
            .add_modifier(Modifier::BOLD),
        VisualRole::Muted | VisualRole::Border => Style::new().fg(Color::Gray).bg(background),
        VisualRole::User => Style::new()
            .fg(user)
            .bg(background)
            .add_modifier(Modifier::BOLD),
        VisualRole::Assistant => Style::new()
            .fg(assistant)
            .bg(background)
            .add_modifier(Modifier::BOLD),
        VisualRole::Error => Style::new()
            .fg(error)
            .bg(background)
            .add_modifier(Modifier::BOLD),
        VisualRole::Tool => Style::new().fg(tool).bg(background),
        VisualRole::Warning => Style::new().fg(warning).bg(background),
        VisualRole::Field => Style::new().fg(Color::White).bg(field),
    }
}

fn visual_style(model: &Model, role: VisualRole) -> Style {
    let presentation = presentation(model);
    let style = match presentation.color_mode {
        ColorMode::Color | ColorMode::Soft | ColorMode::Vivid => {
            let style = match presentation.theme {
                ThemePreset::System => match role {
                    VisualRole::Normal => Style::new()
                        .fg(Color::Rgb(226, 232, 240))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Header => Style::new()
                        .fg(Color::Rgb(5, 10, 20))
                        .bg(Color::Rgb(34, 211, 238))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Selected => Style::new()
                        .fg(Color::Rgb(8, 12, 24))
                        .bg(Color::Rgb(167, 139, 250))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Muted | VisualRole::Border => Style::new()
                        .fg(Color::Rgb(100, 116, 139))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::User => Style::new()
                        .fg(Color::Rgb(96, 165, 250))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Assistant => Style::new()
                        .fg(Color::Rgb(45, 212, 191))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Error => Style::new()
                        .fg(Color::Rgb(251, 113, 133))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Tool => Style::new()
                        .fg(Color::Rgb(192, 132, 252))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Warning => Style::new()
                        .fg(Color::Rgb(251, 191, 36))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Field => Style::new()
                        .fg(Color::Rgb(226, 232, 240))
                        .bg(Color::Rgb(30, 41, 59)),
                },
                ThemePreset::Light => match role {
                    VisualRole::Normal => Style::new().fg(Color::Black).bg(Color::White),
                    VisualRole::Header | VisualRole::Selected => Style::new()
                        .fg(Color::White)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Muted | VisualRole::Border => {
                        Style::new().fg(Color::DarkGray).bg(Color::White)
                    }
                    VisualRole::User => Style::new()
                        .fg(Color::Blue)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Assistant => Style::new()
                        .fg(Color::Cyan)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Error => Style::new()
                        .fg(Color::Red)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Tool | VisualRole::Warning => {
                        Style::new().fg(Color::Yellow).bg(Color::White)
                    }
                    VisualRole::Field => Style::new().fg(Color::Black).bg(Color::Gray),
                },
                ThemePreset::Dark => match role {
                    VisualRole::Normal => Style::new()
                        .fg(Color::Rgb(226, 232, 240))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Header => Style::new()
                        .fg(Color::Rgb(5, 10, 20))
                        .bg(Color::Rgb(34, 211, 238))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Selected => Style::new()
                        .fg(Color::Rgb(8, 12, 24))
                        .bg(Color::Rgb(167, 139, 250))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Muted | VisualRole::Border => Style::new()
                        .fg(Color::Rgb(100, 116, 139))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::User => Style::new()
                        .fg(Color::Rgb(96, 165, 250))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Assistant => Style::new()
                        .fg(Color::Rgb(45, 212, 191))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Error => Style::new()
                        .fg(Color::Rgb(251, 113, 133))
                        .bg(Color::Rgb(8, 12, 24))
                        .add_modifier(Modifier::BOLD),
                    VisualRole::Tool => Style::new()
                        .fg(Color::Rgb(192, 132, 252))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Warning => Style::new()
                        .fg(Color::Rgb(251, 191, 36))
                        .bg(Color::Rgb(8, 12, 24)),
                    VisualRole::Field => Style::new()
                        .fg(Color::Rgb(226, 232, 240))
                        .bg(Color::Rgb(30, 41, 59)),
                },
                ThemePreset::Aurora
                | ThemePreset::Ember
                | ThemePreset::Midnight
                | ThemePreset::Ocean
                | ThemePreset::Forest
                | ThemePreset::Rose => extra_theme_style(presentation.theme, role),
            };
            match presentation.color_mode {
                ColorMode::Soft if !matches!(role, VisualRole::Header | VisualRole::Selected) => {
                    style.add_modifier(Modifier::DIM)
                }
                ColorMode::Vivid if !matches!(role, VisualRole::Muted | VisualRole::Border) => {
                    style.add_modifier(Modifier::BOLD)
                }
                _ => style,
            }
        }
        ColorMode::NoColor => match role {
            VisualRole::Header | VisualRole::Selected | VisualRole::Field => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            }
            VisualRole::Muted => Style::default().add_modifier(Modifier::DIM),
            VisualRole::User | VisualRole::Assistant | VisualRole::Tool | VisualRole::Warning => {
                Style::default().add_modifier(Modifier::BOLD)
            }
            VisualRole::Error => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            }
            VisualRole::Normal | VisualRole::Border => Style::default(),
        },
        ColorMode::HighContrast => match role {
            VisualRole::Header | VisualRole::Selected | VisualRole::Field => Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
            VisualRole::Normal | VisualRole::Muted | VisualRole::Border => {
                Style::default().fg(Color::White).bg(Color::Black)
            }
            VisualRole::User | VisualRole::Assistant | VisualRole::Tool => Style::default()
                .fg(Color::LightCyan)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
            VisualRole::Error => Style::default()
                .fg(Color::LightRed)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            VisualRole::Warning => Style::default()
                .fg(Color::LightYellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        },
    };
    if matches!(role, VisualRole::Border) {
        style.bg(Color::Reset)
    } else {
        style
    }
}
fn chat_visual_style(model: &Model, role: VisualRole) -> Style {
    visual_style(model, role).bg(Color::Reset)
}

fn theme_gradient(theme: ThemePreset) -> ((u8, u8, u8), (u8, u8, u8)) {
    match theme {
        ThemePreset::System | ThemePreset::Dark | ThemePreset::Midnight => {
            ((34, 211, 238), (167, 139, 250))
        }
        ThemePreset::Light => ((37, 99, 235), (219, 39, 119)),
        ThemePreset::Aurora => ((45, 212, 191), (129, 140, 248)),
        ThemePreset::Ember => ((251, 146, 60), (244, 63, 94)),
        ThemePreset::Ocean => ((34, 211, 238), (14, 165, 233)),
        ThemePreset::Forest => ((74, 222, 128), (250, 204, 21)),
        ThemePreset::Rose => ((244, 114, 182), (192, 132, 252)),
    }
}

fn gradient_style(model: &Model, index: u16, count: u16) -> Style {
    let presentation = presentation(model);
    if matches!(
        presentation.color_mode,
        ColorMode::NoColor | ColorMode::HighContrast
    ) {
        return chat_visual_style(model, VisualRole::Border);
    }
    let (start, end) = theme_gradient(presentation.theme);
    let denominator = count.saturating_sub(1).max(1);
    let blend = |from: u8, to: u8| {
        let from = u32::from(from);
        let to = u32::from(to);
        let index = u32::from(index.min(denominator));
        let denominator = u32::from(denominator);
        u8::try_from((from * (denominator - index) + to * index) / denominator).unwrap_or(u8::MAX)
    };
    let mut style = Style::new()
        .fg(Color::Rgb(
            blend(start.0, end.0),
            blend(start.1, end.1),
            blend(start.2, end.2),
        ))
        .bg(Color::Reset);
    if presentation.color_mode == ColorMode::Soft {
        style = style.add_modifier(Modifier::DIM);
    } else if presentation.color_mode == ColorMode::Vivid {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn gradient_text(model: &Model, value: &str) -> Line<'static> {
    let count = u16::try_from(value.chars().count()).unwrap_or(u16::MAX);
    Line::from(
        value
            .chars()
            .enumerate()
            .map(|(index, character)| {
                Span::styled(
                    character.to_string(),
                    gradient_style(model, u16::try_from(index).unwrap_or(u16::MAX), count),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn render_vertical_gradient(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let glyph = if presentation(model).ascii {
        "|"
    } else {
        "│"
    };
    let lines = (0..area.height)
        .map(|row| Line::styled(glyph, gradient_style(model, row, area.height)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_horizontal_gradient(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let glyph = if presentation(model).ascii {
        "-"
    } else {
        "─"
    };
    let spans = (0..area.width)
        .map(|column| Span::styled(glyph, gradient_style(model, column, area.width)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn app_block(model: &Model) -> Block<'static> {
    let block = Block::default().border_set(ratatui::symbols::border::ROUNDED);
    if presentation(model).ascii {
        block.border_set(ASCII_BORDER)
    } else {
        block
    }
}

fn chrome_separator(model: &Model) -> &'static str {
    if presentation(model).ascii {
        " | "
    } else {
        " · "
    }
}

fn selection_marker(model: &Model) -> &'static str {
    if presentation(model).ascii {
        ">"
    } else {
        "›"
    }
}

fn navigation_keys(model: &Model) -> &'static str {
    if presentation(model).ascii {
        "Up/Down"
    } else {
        "↑/↓"
    }
}

const SETTINGS_NAV: [&str; 4] = ["Settings", "Providers", "Profile", "Models"];

/// Renders the complete terminal client from local state only.
pub fn view(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(Clear, area);
    let shell = render_shell(frame, area, model);
    let content = shell.content;
    if content.width > 0 && content.height > 0 {
        match model.route() {
            Route::Chat => {
                if content.width < 24 || content.height < 7 {
                    render_compact(frame, content, model);
                } else {
                    render_standard(frame, content, model);
                }
            }
            Route::Sessions => render_browser(frame, content, model),
            Route::Profiles => render_profile_center(frame, content, model),
            Route::Settings => render_settings(frame, content, model),
            Route::Help => render_help(frame, content, model),
        }
    }

    if model.startup_active() {
        render_startup(frame, area, model);
        return;
    }

    if model.overlay() == Some(OverlayKind::CommandPalette) && model.route() == Route::Chat {
        render_inline_palette(frame, content, model);
    }
    match model.overlay() {
        Some(OverlayKind::Permission) => render_permission(frame, area, model),
        Some(OverlayKind::CommandPalette) if model.route() != Route::Chat => {
            render_palette(frame, area, model)
        }
        Some(OverlayKind::CommandPalette) => {}
        Some(OverlayKind::SessionCredential) => render_credential(frame, area, model),
        Some(OverlayKind::ModelPicker) => render_picker(frame, area, model),
        Some(OverlayKind::Confirmation) => render_confirmation(frame, area, model),
        Some(OverlayKind::UserProfile) => render_user_profile(frame, area, model),
        Some(OverlayKind::TranscriptSearch | OverlayKind::ProfileCredential) | None => {}
    }
}

fn render_startup(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    frame.render_widget(Clear, area);
    let width = area.width.clamp(28, 48);
    let height = area.height.clamp(4, 5);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" AutoHarness ")
        .title_style(visual_style(model, VisualRole::Header))
        .border_style(visual_style(model, VisualRole::Selected));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let lines = vec![
        Line::styled(
            format!("{}  Starting", spinner(model)),
            visual_style(model, VisualRole::Assistant),
        ),
        Line::styled(
            "Loading provider models...",
            visual_style(model, VisualRole::Muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: false }),
        inner,
    );
}
/// Resolves a left-click coordinate against the currently visible controls.
///
/// Hit testing is derived from the same responsive layout thresholds as the
/// renderer and returns semantic actions for the deterministic update layer.
pub fn hit_test(
    model: &Model,
    width: u16,
    height: u16,
    column: u16,
    row: u16,
) -> Option<MouseAction> {
    if model.startup_active() {
        return None;
    }
    let area = Rect::new(0, 0, width, height);
    if model.overlay() == Some(OverlayKind::UserProfile) {
        let popup = user_profile_rect(area);
        let button_row = popup.y.saturating_add(10);
        if button_row < popup.bottom() && row == button_row {
            return (column < popup.x + popup.width / 2)
                .then_some(MouseAction::UserProfileSave)
                .or(Some(MouseAction::UserProfileCancel));
        }
        return None;
    }
    if model.overlay() == Some(OverlayKind::Confirmation) {
        let popup = confirmation_rect(area);
        if row == popup.bottom().saturating_sub(2) {
            return (column < popup.x + popup.width / 2)
                .then_some(MouseAction::Confirm)
                .or(Some(MouseAction::Cancel));
        }
        return None;
    }
    if model.overlay() == Some(OverlayKind::ModelPicker) {
        return picker_mouse_target(area, model, row);
    }
    if model.overlay() == Some(OverlayKind::CommandPalette) {
        if model.route() == Route::Chat {
            let content = shell_layout(area, model).content;
            return inline_palette_mouse_target(content, model, column, row);
        }
        return palette_mouse_target(area, model, row);
    }
    if model.overlay() == Some(OverlayKind::SessionCredential) {
        return modal_button_target(
            credential_rect(area),
            row,
            column,
            MouseAction::CredentialSubmit,
            MouseAction::CredentialCancel,
        );
    }
    if model.overlay() == Some(OverlayKind::ProfileCredential) {
        return modal_button_target(
            popup_rect(area),
            row,
            column,
            MouseAction::ProfileCredentialSubmit,
            MouseAction::ProfileCredentialCancel,
        );
    }
    if model.overlay() == Some(OverlayKind::Permission) {
        return modal_button_target(
            credential_rect(area),
            row,
            column,
            MouseAction::PermissionAllow,
            MouseAction::PermissionDeny,
        );
    }
    if model.profile_center.auth_page == Some(crate::model::ProviderChoice::Codex) {
        let popup = codex_auth_rect(area);
        let action_row = popup.y.saturating_add(3);
        return (row == action_row && column > popup.x && column < popup.right())
            .then_some(MouseAction::CodexLogin);
    }
    let layout = shell_layout(area, model);
    if let Some(sidebar) = layout.sidebar {
        if column < sidebar.right() {
            let footer_row = sidebar.bottom().saturating_sub(1);
            if row == footer_row {
                return shell_footer_action(column.saturating_sub(sidebar.x));
            }
            let sessions_start = sidebar.y.saturating_add(1);
            let session_count = model
                .sessions
                .sessions
                .len()
                .min(sidebar_session_limit(sidebar));
            let sessions_end =
                sessions_start.saturating_add(u16::try_from(session_count).unwrap_or(u16::MAX));
            if row >= sessions_start && row < sessions_end {
                return Some(MouseAction::Route(Route::Sessions));
            }
            return None;
        }
    } else if row == layout.footer.y {
        return shell_footer_action(column);
    }
    let content = layout.content;
    if model.route() == Route::Settings && row == content.y.saturating_add(1) {
        return settings_nav_action(content, column);
    }
    if model.route() == Route::Settings && model.settings_workspace.nav_selected == 1 {
        let profile_area = settings_body_area(content);
        if profile_local_hit_row(profile_area, model)
            .is_some_and(|local| local.contains(Position::new(column, row)))
        {
            return Some(MouseAction::OpenUserProfile);
        }
        if let Some(action) = provider_choice_at_row(model, profile_area, column, row) {
            return Some(action);
        }
        if profile_detail_button_rows(model, profile_area).is_some_and(|(first, _)| row == first) {
            return profile_detail_action_at_column(model, profile_area, column, false);
        }
        if profile_detail_button_rows(model, profile_area).is_some_and(|(_, second)| row == second)
        {
            return profile_detail_action_at_column(model, profile_area, column, true);
        }
        if row == profile_area.bottom().saturating_sub(2) {
            return profile_action_at_column(column.saturating_sub(profile_area.x));
        }
        return profile_at_row(model, profile_area, column, row);
    }
    if model.route() == Route::Settings && model.settings_workspace.nav_selected == 2 {
        let profile_area = settings_body_area(content);
        if profile_area.contains(Position::new(column, row)) {
            return Some(MouseAction::OpenUserProfile);
        }
    }
    match model.route() {
        Route::Sessions if row == height.saturating_sub(2) => {
            let relative_column = column.saturating_sub(content.x);
            if relative_column < 18 {
                Some(MouseAction::SessionOpen)
            } else if relative_column < 38 {
                Some(MouseAction::SessionRename)
            } else if relative_column < 58 {
                Some(MouseAction::SessionArchive)
            } else {
                Some(MouseAction::SessionDelete)
            }
        }
        Route::Profiles
            if profile_local_hit_row(content, model)
                .is_some_and(|local| local.contains(Position::new(column, row))) =>
        {
            Some(MouseAction::OpenUserProfile)
        }
        Route::Profiles if provider_choice_at_row(model, content, column, row).is_some() => {
            provider_choice_at_row(model, content, column, row)
        }
        Route::Profiles
            if profile_detail_button_rows(model, content)
                .is_some_and(|(first, _)| row == first) =>
        {
            profile_detail_action_at_column(model, content, column, false)
        }
        Route::Profiles
            if profile_detail_button_rows(model, content)
                .is_some_and(|(_, second)| row == second) =>
        {
            profile_detail_action_at_column(model, content, column, true)
        }
        Route::Profiles => profile_at_row(model, content, column, row),
        _ => None,
    }
}

fn profile_local_hit_row(area: Rect, model: &Model) -> Option<Rect> {
    let _ = (area, model);
    None
}

fn profile_center_content_area(model: &Model, area: Rect) -> Rect {
    let inner = area;
    let compact = presentation(model).compact || inner.width < 72;
    let notice_height = if model.notice.is_some() && inner.height >= 8 {
        if compact { 1 } else { 2 }
    } else {
        0
    };
    let header_height = if inner.height >= 8 { 2 } else { 1 };
    let help_height = u16::from(inner.height >= 4);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(help_height),
        ])
        .split(inner);
    rows[1]
}

fn profile_list_inner_rect(model: &Model, area: Rect) -> Option<Rect> {
    let content = profile_center_content_area(model, area);
    let (list_area, _) = profile_list_detail_areas(content, model);
    Some(
        Block::default()
            .border_set(ratatui::symbols::border::ROUNDED)
            .borders(Borders::ALL)
            .inner(list_area),
    )
}

fn profile_detail_area(model: &Model, area: Rect) -> Option<Rect> {
    let content = profile_center_content_area(model, area);
    profile_list_detail_areas(content, model).1
}

fn profile_detail_action_at_column(
    model: &Model,
    area: Rect,
    column: u16,
    secondary: bool,
) -> Option<MouseAction> {
    let detail = profile_detail_area(model, area)?;
    let relative = column.saturating_sub(detail.x.saturating_add(1));
    if secondary {
        profile_secondary_action_at_column(relative)
    } else {
        profile_action_at_column(relative)
    }
}

fn profile_detail_button_rows(model: &Model, area: Rect) -> Option<(u16, u16)> {
    let selected = model.selected_profile()?;
    let detail = profile_detail_area(model, area)?;
    let mut lines = u16::try_from(model.filtered_profiles().count())
        .unwrap_or(u16::MAX)
        .saturating_add(9);
    if selected.kind == ProviderKindLabel::Router {
        lines = lines.saturating_add(1);
        if !selected.project.is_empty() {
            lines = lines.saturating_add(1);
        }
        if !selected.auth_header.is_empty() {
            lines = lines.saturating_add(1);
        }
    }
    if matches!(selected.connection, ProfileConnectionState::Failed(_)) {
        lines = lines.saturating_add(1);
    }
    if model.profiles().pending_recovery > 0 {
        lines = lines.saturating_add(1);
    }
    let first = detail
        .y
        .saturating_add(1)
        .saturating_add(lines)
        .saturating_add(1);
    Some((first, first.saturating_add(1)))
}

fn provider_choice_at_row(model: &Model, area: Rect, column: u16, row: u16) -> Option<MouseAction> {
    let list = profile_list_inner_rect(model, area)?;
    if !list.contains(Position::new(column, row)) {
        return None;
    }
    let selected = model
        .profile_center
        .choice_selected
        .min(PROVIDER_CHOICES.len().saturating_sub(1));
    let index = usize::from(row.saturating_sub(list.y)).saturating_add(usize::from(
        profile_list_scroll(selected, PROVIDER_CHOICES.len(), list.height),
    ));
    (index < PROVIDER_CHOICES.len()).then_some(MouseAction::SelectProviderChoice(index))
}

fn profile_at_row(model: &Model, area: Rect, column: u16, row: u16) -> Option<MouseAction> {
    let list = Block::default()
        .border_set(ratatui::symbols::border::ROUNDED)
        .borders(Borders::ALL)
        .inner(profile_detail_area(model, area)?);
    if !list.contains(Position::new(column, row)) {
        return None;
    }
    let profiles = model.filtered_profiles().collect::<Vec<_>>();
    let index = usize::from(row.saturating_sub(list.y));
    profiles
        .get(index)
        .map(|profile| MouseAction::SelectProfile(profile.id.clone()))
}

fn picker_mouse_target(area: Rect, model: &Model, row: u16) -> Option<MouseAction> {
    let popup = popup_rect(area);
    let inner_height = popup.height.saturating_sub(2);
    let stale_height = u16::from(
        matches!(
            &*model.catalog,
            CatalogProjection::Ready { stale: true, .. }
        ) && inner_height >= 3,
    );
    let help_height = u16::from(inner_height >= 4);
    let list_height = inner_height.saturating_sub(1 + stale_height + help_height);
    let list_start = popup.y.saturating_add(2);
    if row < list_start || row >= list_start.saturating_add(list_height) {
        return None;
    }
    let models = filtered_models(model);
    let selected_index = model
        .picker
        .selected
        .as_ref()
        .and_then(|selected| models.iter().position(|summary| &summary.model == selected))
        .unwrap_or(0);
    let visible = usize::from(list_height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(models.len().saturating_sub(visible));
    models
        .get(start + usize::from(row - list_start))
        .map(|summary| MouseAction::PickerSelect(summary.model.clone()))
}
fn modal_button_target(
    popup: Rect,
    row: u16,
    column: u16,
    primary: MouseAction,
    secondary: MouseAction,
) -> Option<MouseAction> {
    let action_row = popup.bottom().saturating_sub(2);
    if row != action_row {
        return None;
    }
    if column < popup.x + popup.width / 2 {
        Some(primary)
    } else {
        Some(secondary)
    }
}

fn inline_palette_mouse_target(
    area: Rect,
    model: &Model,
    column: u16,
    row: u16,
) -> Option<MouseAction> {
    let list = inline_palette_rect(area, model);
    if !list.contains(Position::new(column, row)) {
        return None;
    }
    let entries = model.palette_entries();
    let selected_index = model
        .palette
        .selected
        .and_then(|selected| entries.iter().position(|entry| entry.id == selected))
        .unwrap_or(0);
    let visible = usize::from(list.height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(entries.len().saturating_sub(visible));
    entries
        .get(start + usize::from(row.saturating_sub(list.y)))
        .map(|entry| MouseAction::PaletteRun(entry.id.to_owned()))
}

fn palette_mouse_target(area: Rect, model: &Model, row: u16) -> Option<MouseAction> {
    let popup = popup_rect(area);
    let inner_height = popup.height.saturating_sub(2);
    let help_height = u16::from(inner_height >= 3);
    let list_height = inner_height.saturating_sub(1 + help_height);
    let list_start = popup.y.saturating_add(2);
    if row < list_start || row >= list_start.saturating_add(list_height) {
        return None;
    }
    let entries = model.palette_entries();
    let selected_index = model
        .palette
        .selected
        .and_then(|selected| entries.iter().position(|entry| entry.id == selected))
        .unwrap_or(0);
    let visible = usize::from(list_height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(entries.len().saturating_sub(visible));
    entries
        .get(start + usize::from(row - list_start))
        .map(|entry| MouseAction::PaletteRun(entry.id.to_owned()))
}
fn profile_action_at_column(column: u16) -> Option<MouseAction> {
    match column {
        0..=10 => Some(MouseAction::ProfileCredential),
        12..=19 => Some(MouseAction::ProfileTest),
        21..=29 => Some(MouseAction::ProfileDefaultModel),
        _ => None,
    }
}

fn profile_secondary_action_at_column(column: u16) -> Option<MouseAction> {
    match column {
        0..=13 => Some(MouseAction::ProfileDisconnect),
        15..=24 => Some(MouseAction::ProfileDelete),
        _ => None,
    }
}

fn settings_nav_action(area: Rect, column: u16) -> Option<MouseAction> {
    let compact = area.width < 48;
    let padding = if compact { 0 } else { 2 };
    let gap = if compact { 1 } else { 2 };
    let mut offset = area.x;
    for (index, label) in SETTINGS_NAV.iter().enumerate() {
        let width = u16::try_from(label.len().saturating_add(padding)).unwrap_or(u16::MAX);
        if column >= offset && column < offset.saturating_add(width) {
            return Some(MouseAction::SettingsTab(index));
        }
        offset = offset.saturating_add(width).saturating_add(gap);
    }
    None
}

fn render_shell(frame: &mut Frame<'_>, area: Rect, model: &Model) -> ShellLayout {
    let layout = shell_layout(area, model);
    if let Some(sidebar) = layout.sidebar {
        render_navigation_rail(frame, sidebar, model);
    } else {
        render_shell_footer(frame, layout.footer, model);
    }
    layout
}

fn shell_footer_action(column: u16) -> Option<MouseAction> {
    if column < 10 {
        Some(MouseAction::SettingsTab(2))
    } else if column < 22 {
        Some(MouseAction::SettingsTab(0))
    } else {
        None
    }
}

fn shell_layout(area: Rect, model: &Model) -> ShellLayout {
    let wide = !presentation(model).single_column && area.width >= 100 && area.height >= 16;
    if wide {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(1)])
            .split(area);
        ShellLayout {
            sidebar: Some(columns[0]),
            content: columns[1],
            footer: Rect::default(),
        }
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);
        ShellLayout {
            sidebar: None,
            content: rows[0],
            footer: rows[1],
        }
    }
}

fn render_shell_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let profile_style =
        if model.route() == Route::Settings && model.settings_workspace.nav_selected == 2 {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
    let settings_style =
        if model.route() == Route::Settings && model.settings_workspace.nav_selected == 0 {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
    let line = Line::from(vec![
        Span::styled(" Profile ", profile_style),
        Span::styled(" | ", visual_style(model, VisualRole::Muted)),
        Span::styled(" Settings ", settings_style),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(chat_visual_style(model, VisualRole::Normal)),
        area,
    );
}
fn sidebar_session_limit(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(5)).max(1)
}

fn single_line_label(value: &str, width: u16) -> String {
    let safe = display_safe(value);
    let width = usize::from(width);
    if safe.chars().count() <= width {
        return safe;
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut truncated = safe.chars().take(width - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn render_navigation_rail(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let divider = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
    render_vertical_gradient(frame, divider, model);
    let inner = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let footer_height = u16::from(inner.height >= 2);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(footer_height),
        ])
        .split(inner);
    let brand = if presentation(model).nerd_font {
        "   AutoHarness"
    } else {
        " AutoHarness"
    };
    frame.render_widget(Paragraph::new(gradient_text(model, brand)), sections[0]);
    let content = sections[1];
    let mut lines = Vec::new();
    let session_limit = sidebar_session_limit(area);
    let session_width = content.width.saturating_sub(4);
    for entry in model.sessions.sessions.iter().take(session_limit) {
        let marker = if entry.active || entry.session_id == model.session.session_id {
            selection_marker(model)
        } else {
            " "
        };
        let style = if entry.active || entry.session_id == model.session.session_id {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
        lines.push(Line::styled(
            format!(
                " {marker} {}",
                single_line_label(&entry.title, session_width)
            ),
            style,
        ));
    }
    if model.sessions.sessions.is_empty() {
        lines.push(Line::styled(
            " No sessions yet",
            visual_style(model, VisualRole::Muted),
        ));
    }
    lines.extend([
        Line::from(""),
        Line::styled("PROJECTS", visual_style(model, VisualRole::Muted)),
        Line::styled(
            format!(" {} ", workspace_label(&model.profiles().user.workspace)),
            visual_style(model, VisualRole::Normal),
        ),
    ]);
    frame.render_widget(Paragraph::new(lines), content);
    if footer_height > 0 {
        render_shell_footer(frame, sections[2], model);
    }
}

fn onboarding_step(model: &Model) -> (&'static str, &'static str) {
    if model.session.selected_model.is_none() {
        ("NEXT", "/models choose a model")
    } else if model.settings().provider_status.credential_connected {
        ("READY", "Write a prompt below")
    } else {
        ("NEXT", "/settings set a provider key")
    }
}

fn render_onboarding(lines: &mut Vec<Line<'static>>, model: &Model) {
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "GET STARTED",
        visual_style(model, VisualRole::User),
    ));
    lines.push(Line::from("1  /settings connect a provider key"));
    lines.push(Line::from("2  /models choose a compatible model"));
    let (label, action) = onboarding_step(model);
    lines.push(Line::styled(
        format!("3  {label} · {action}"),
        visual_style(model, VisualRole::Assistant),
    ));
}

fn render_confirmation(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let confirmation = if let Some(session_id) = &model.browser.confirming_archive {
        Some((
            " Archive session ",
            format!("Archive session '{}'?", display_safe(session_id)),
            "The session remains durable and can be unarchived.",
        ))
    } else if let Some(session_id) = &model.browser.confirming_delete {
        Some((
            " Delete session ",
            format!("Permanently delete session '{}'?", display_safe(session_id)),
            "A complete provider-neutral archive is written before deletion.",
        ))
    } else if let Some(profile_id) = &model.profile_center.confirming_disconnect {
        Some((
            " Disconnect credential ",
            format!(
                "Disconnect the stored credential for '{}'?",
                display_safe(profile_id)
            ),
            "The profile remains and environment overrides are unchanged.",
        ))
    } else {
        model
            .profile_center
            .confirming_delete
            .as_ref()
            .map(|profile_id| {
                (
                    " Delete provider profile ",
                    format!(
                        "Delete profile '{}' and its stored credential?",
                        display_safe(profile_id)
                    ),
                    "Other provider profiles and credentials are unaffected.",
                )
            })
    };
    let Some((title, question, consequence)) = confirmation else {
        return;
    };
    let popup = confirmation_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(title)
        .border_style(visual_style(model, VisualRole::Error));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let lines = vec![
        Line::styled(question, visual_style(model, VisualRole::User)),
        Line::from(""),
        Line::styled(consequence, visual_style(model, VisualRole::Warning)),
        Line::from(""),
        Line::styled(
            "Y confirm  N or Esc cancel",
            visual_style(model, VisualRole::Assistant),
        ),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_user_profile(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = user_profile_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" User profile ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let label = model
        .user_profile
        .display_label_editor
        .as_deref()
        .unwrap_or_default();
    let user = &model.profiles().user;
    let default_profile = user.default_profile.as_deref().unwrap_or("session only");
    let default_model = user.default_model.as_deref().unwrap_or("not set");
    let default_mode = if user.default_mode.is_empty() {
        "safe agent"
    } else {
        user.default_mode.as_str()
    };
    let lines = vec![
        Line::styled("LOCAL IDENTITY", visual_style(model, VisualRole::User)),
        Line::styled(
            format!("Display name  > {}", display_safe(label)),
            visual_style(model, VisualRole::Selected),
        ),
        Line::styled(
            format!("Workspace     {}", workspace_label(&user.workspace)),
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("DEFAULTS", visual_style(model, VisualRole::User)),
        detail_line(model, "Provider", default_profile),
        detail_line(model, "Model", default_model),
        detail_line(model, "Thinking", default_mode),
        Line::from(""),
        Line::from(vec![
            Span::styled("[ Save ]", visual_style(model, VisualRole::Selected)),
            Span::raw("  "),
            Span::styled("[ Cancel ]", visual_style(model, VisualRole::Field)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn workspace_label(workspace: &str) -> String {
    workspace
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map_or_else(|| "workspace".to_owned(), display_safe)
}

fn inline_palette_rect(area: Rect, model: &Model) -> Rect {
    let prompt_height = prompt_surface_height(area, model);
    let height = u16::try_from(model.palette_entries().len())
        .unwrap_or(u16::MAX)
        .min(8)
        .min(area.height.saturating_sub(prompt_height));
    Rect::new(
        area.x.saturating_add(2),
        area.bottom()
            .saturating_sub(prompt_height.saturating_add(height)),
        area.width.saturating_sub(4),
        height,
    )
}

fn render_inline_palette(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let list = inline_palette_rect(area, model);
    if list.width == 0 || list.height == 0 {
        return;
    }
    frame.render_widget(Clear, Rect::new(area.x, list.y, area.width, list.height));
    let entries = model.palette_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching commands").style(visual_style(model, VisualRole::Muted)),
            list,
        );
        return;
    }
    let selected_index = model
        .palette
        .selected
        .and_then(|selected| entries.iter().position(|entry| entry.id == selected))
        .unwrap_or(0);
    let visible = usize::from(list.height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(entries.len().saturating_sub(visible));
    let items = entries
        .iter()
        .skip(start)
        .take(visible)
        .map(|entry| inline_palette_item(entry, model.palette.selected, model))
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), list);
}

fn inline_palette_item(
    entry: &crate::model::CommandEntry,
    selected: Option<&'static str>,
    model: &Model,
) -> ListItem<'static> {
    let is_selected = selected == Some(entry.id);
    let prefix = if is_selected {
        selection_marker(model)
    } else {
        " "
    };
    let mut label = format!(
        "{prefix} /{}  {} - {}",
        entry.id,
        display_safe(entry.label),
        display_safe(entry.description)
    );
    if let Some(hint) = entry.key_hint {
        let _ = write!(label, "  [{hint}]");
    }
    let style = if is_selected {
        chat_visual_style(model, VisualRole::Assistant)
    } else {
        chat_visual_style(model, VisualRole::Normal)
    };
    ListItem::new(Line::styled(label, style))
}

/// Renders the searchable command-palette overlay from local state only.
fn render_palette(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Commands ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let search_height = 1.min(inner.height);
    let help_height = u16::from(inner.height >= 3);
    let list_height = inner.height.saturating_sub(search_height + help_height);
    let search = Rect::new(inner.x, inner.y, inner.width, search_height);
    let list = Rect::new(inner.x, inner.y + search_height, inner.width, list_height);
    let help = Rect::new(
        inner.x,
        inner.y + search_height + list_height,
        inner.width,
        help_height,
    );

    frame.render_widget(
        Paragraph::new(format!("Filter: {}", display_safe(&model.palette.query)))
            .style(visual_style(model, VisualRole::Field)),
        search,
    );

    let entries = model.palette_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No commands match this filter.")
                .style(visual_style(model, VisualRole::Muted)),
            list,
        );
    } else {
        let selected = model.palette.selected;
        let selected_index = selected
            .and_then(|selected| entries.iter().position(|entry| entry.id == selected))
            .unwrap_or(0);
        let visible = usize::from(list.height);
        let start = selected_index
            .saturating_add(1)
            .saturating_sub(visible)
            .min(entries.len().saturating_sub(visible));
        let items = entries
            .iter()
            .skip(start)
            .take(visible)
            .map(|entry| palette_item(entry, selected, model))
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), list);
    }

    if help.height > 0 {
        frame.render_widget(
            Paragraph::new(format!(
                "{} choose  Enter run  Esc close",
                navigation_keys(model)
            ))
            .style(visual_style(model, VisualRole::Muted)),
            help,
        );
    }
}

fn palette_item(
    entry: &crate::model::CommandEntry,
    selected: Option<&'static str>,
    model: &Model,
) -> ListItem<'static> {
    let is_selected = selected == Some(entry.id);
    let prefix = if is_selected {
        selection_marker(model)
    } else {
        " "
    };
    let mut label = format!(
        "{prefix} /{}  {} - {}",
        entry.id,
        display_safe(entry.label),
        display_safe(entry.description)
    );
    if let Some(hint) = entry.key_hint {
        let _ = write!(label, "  [{hint}]");
    }
    let style = if is_selected {
        visual_style(model, VisualRole::Selected)
    } else {
        visual_style(model, VisualRole::Normal)
    };
    ListItem::new(Line::styled(label, style))
}

/// Renders the contextual help overlay from local state only.
///
/// The section matching the surface help was requested from is rendered
/// first and highlighted, and content scrolls without clipping the frame.
fn render_help(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = area;
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Help ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // The surface help was opened from leads; everything else follows in
    // table order so context is never below the fold on small terminals.
    // The always-true Global section never leads.
    let origin = model.navigation.previous_route.focus();
    let mut ordered: Vec<&crate::model::HelpSection> = Vec::new();
    if let Some(first) = crate::model::HELP_SECTIONS
        .iter()
        .find(|section| section.title != "Global" && section.matches_focus(origin))
    {
        ordered.push(first);
    }
    for section in crate::model::HELP_SECTIONS {
        if !ordered.iter().any(|placed| std::ptr::eq(*placed, section)) {
            ordered.push(section);
        }
    }

    let mut lines = Vec::new();
    for (position, section) in ordered.iter().enumerate() {
        let style = if position == 0 {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
        lines.push(Line::styled(section.title.to_owned(), style));
        for (key, description) in section.rows {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key}"), visual_style(model, VisualRole::Muted)),
                Span::raw(format!("  {description}")),
            ]));
        }
    }

    let hint_height = u16::from(inner.height >= 2);
    let content_height = inner.height - hint_height;
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph.scroll((model.help.scroll, 0)), content);

    if hint_height > 0 {
        let hint = Rect::new(inner.x, inner.y + content_height, inner.width, hint_height);
        frame.render_widget(
            Paragraph::new(format!("{} scroll  Esc close", navigation_keys(model)))
                .style(visual_style(model, VisualRole::Muted)),
            hint,
        );
    }
}

fn settings_body_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(2),
        area.width.saturating_sub(2),
        area.height.saturating_sub(3),
    )
}

/// Renders resolved runtime settings and safe provenance as a primary route.
fn render_settings(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    frame.render_widget(Clear, area);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Settings & Provenance ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let nav_height = u16::from(inner.height >= 2);
    if nav_height > 0 {
        render_settings_nav(
            frame,
            Rect::new(inner.x, inner.y, inner.width, nav_height),
            model,
        );
    }
    let body = if nav_height > 0 {
        settings_body_area(area)
    } else {
        inner
    };
    match model.settings_workspace.nav_selected {
        1 => render_profile_center(frame, body, model),
        2 => render_settings_profile(frame, body, model),
        3 => render_model_defaults(frame, body, model),
        _ => {}
    }
    if model.settings_workspace.nav_selected != 0 {
        return;
    }

    let header_height = if body.height >= 8 { 2 } else { 1 };
    let help_height = u16::from(body.height >= 3);
    let page_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(help_height),
        ])
        .split(body);
    render_settings_page_header(
        frame,
        page_rows[0],
        model,
        "General",
        "Review runtime state, preferences, appearance, and terminal behavior.",
    );

    let mut lines = vec![
        Line::styled("PROFILE DEFAULTS", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::DisplayLabel),
        Line::from(""),
        settings_preference_line(model, SettingsPreference::Provider),
        settings_preference_line(model, SettingsPreference::Profile),
        settings_preference_line(model, SettingsPreference::Credential),
        settings_preference_line(model, SettingsPreference::Source),
        Line::styled(
            "API KEY  /connect or press K",
            visual_style(model, VisualRole::Field),
        ),
        Line::styled(
            "Stored provider credentials remain managed from Providers.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("MODEL & THINKING", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::Model),
        settings_preference_line(model, SettingsPreference::Mode),
        Line::styled(
            "Read-only here: choose a model from /models; thinking follows its advertised capability.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("APPROVALS", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::Approvals),
        Line::styled("RETENTION", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::Retention),
        Line::from(""),
        Line::styled("APPEARANCE", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::ThemePreset),
        settings_preference_line(model, SettingsPreference::ColorMode),
        settings_preference_line(model, SettingsPreference::GlyphMode),
        Line::styled("PROMPT BAR", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::PromptStatusDetail),
        Line::styled("ACCESSIBILITY", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::ReducedMotion),
        settings_preference_line(model, SettingsPreference::Density),
        Line::styled("LOGGING", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::Logging),
        Line::styled("TERMINAL BEHAVIOR", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::Layout),
        settings_preference_line(model, SettingsPreference::TerminalTimestampStyle),
        settings_preference_line(model, SettingsPreference::ComposerSubmitBehavior),
        Line::from(""),
        Line::styled("SHORTCUT REFERENCE", visual_style(model, VisualRole::User)),
    ];
    lines.extend(COMMANDS.iter().filter_map(|command| {
        command.key_hint.map(|hint| {
            Line::styled(
                format!(" {hint:<24} /{} - {}", command.id, command.description),
                visual_style(model, VisualRole::Muted),
            )
        })
    }));
    let content = page_rows[1];
    let scroll = settings_scroll(
        &lines,
        SettingsPreference::at(model.settings_workspace.selected),
        content,
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        content,
    );
    if help_height > 0 {
        let controls = if model.settings_workspace.nav_focus {
            "←/→ page  Down open  Esc Chat"
        } else {
            "↑/↓ setting  ←/→ option  Enter edit  R inherit  D reset  Up return"
        };
        frame.render_widget(
            Paragraph::new(format!("{} {controls}", navigation_keys(model)))
                .style(visual_style(model, VisualRole::Muted)),
            page_rows[2],
        );
    }
}

fn render_settings_page_header(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    title: &'static str,
    description: &'static str,
) {
    if area.height > 1 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(title, visual_style(model, VisualRole::User)),
                Line::styled(description, visual_style(model, VisualRole::Muted)),
            ]),
            area,
        );
    } else if area.height == 1 {
        frame.render_widget(
            Paragraph::new(format!("{title}  {description}"))
                .style(visual_style(model, VisualRole::User)),
            area,
        );
    }
}

fn render_settings_nav(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let compact = area.width < 48;
    let mut spans = Vec::new();
    for (index, label) in SETTINGS_NAV.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(if compact { " " } else { "  " }));
        }
        let style = if index == model.settings_workspace.nav_selected {
            if model.settings_workspace.nav_focus {
                visual_style(model, VisualRole::Selected)
            } else {
                visual_style(model, VisualRole::User)
            }
        } else {
            visual_style(model, VisualRole::Muted)
        };
        spans.push(Span::styled(
            if compact {
                (*label).to_owned()
            } else {
                format!(" {label} ")
            },
            style,
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_settings_profile(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let header_height = if area.height >= 8 { 2 } else { 1 };
    let help_height = u16::from(area.height >= 3);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(help_height),
        ])
        .split(area);
    render_settings_page_header(
        frame,
        rows[0],
        model,
        "Profile",
        "Manage the local identity and workspace defaults used across sessions.",
    );
    let card_height = rows[1].height.min(4);
    render_local_profile(
        frame,
        Rect::new(rows[1].x, rows[1].y, rows[1].width, card_height),
        model,
    );
    if help_height > 0 {
        frame.render_widget(
            Paragraph::new("Enter edit  Up return  Left/Right pages  Esc Settings")
                .style(visual_style(model, VisualRole::Muted)),
            rows[2],
        );
    }
}

fn settings_preference_line(model: &Model, preference: SettingsPreference) -> Line<'static> {
    let profile = &model.settings().local_profile;
    let (label, value, source, explanation) = match preference {
        SettingsPreference::Provider => (
            "Provider",
            model.settings().provider_label(),
            "runtime",
            "active provider adapter",
        ),
        SettingsPreference::Profile => (
            "Profile",
            model
                .settings()
                .provider_status
                .active_profile
                .as_deref()
                .unwrap_or("none")
                .to_owned(),
            "runtime",
            "active provider profile",
        ),
        SettingsPreference::Credential => (
            "Credential",
            if model.settings().provider_status.credential_connected {
                "connected".to_owned()
            } else {
                "disconnected".to_owned()
            },
            "runtime",
            "safe connection state",
        ),
        SettingsPreference::Source => (
            "Source",
            model
                .settings()
                .provider_status
                .credential_source
                .as_str()
                .to_owned(),
            "runtime",
            "credential provenance",
        ),
        SettingsPreference::Model => (
            "Model",
            selected_model_label(model),
            "runtime",
            "active session model",
        ),
        SettingsPreference::Mode => (
            "Thinking",
            if model.profiles().user.default_mode.is_empty() {
                "provider default".to_owned()
            } else {
                model.profiles().user.default_mode.clone()
            },
            "profile",
            "new-session thinking default",
        ),
        SettingsPreference::Approvals => (
            "Approvals",
            "per-call".to_owned(),
            "policy",
            "exact capability decisions",
        ),
        SettingsPreference::Retention => (
            "Retention",
            "durable".to_owned(),
            "policy",
            "session history and deletion controls",
        ),
        SettingsPreference::Logging => (
            "Logging",
            "redacted".to_owned(),
            "policy",
            "credentials and content excluded",
        ),
        SettingsPreference::DisplayLabel => (
            "Display label",
            model
                .settings_workspace
                .display_label_editor
                .as_ref()
                .map_or_else(
                    || {
                        profile.display_label().value().as_ref().map_or_else(
                            || "not set".to_owned(),
                            |label| display_safe(label.as_str()),
                        )
                    },
                    |value| format!("{} [editing]", display_safe(value)),
                ),
            profile.display_label().source().as_str(),
            "local identity shown only in this terminal",
        ),
        SettingsPreference::ThemePreset => (
            "Theme preset",
            theme_preset_label(*profile.preferences().theme_preset().value()).to_owned(),
            profile.preferences().theme_preset().source().as_str(),
            "terminal palette preference",
        ),
        SettingsPreference::ColorMode => (
            "Color mode",
            color_mode_label(*profile.preferences().color_mode().value()).to_owned(),
            profile.preferences().color_mode().source().as_str(),
            "status and focus contrast",
        ),
        SettingsPreference::GlyphMode => (
            "Glyph mode",
            glyph_mode_label(*profile.preferences().glyph_mode().value()).to_owned(),
            profile.preferences().glyph_mode().source().as_str(),
            "application chrome only",
        ),
        SettingsPreference::PromptStatusDetail => (
            "Prompt detail",
            prompt_status_detail_label(*profile.preferences().prompt_status_detail().value())
                .to_owned(),
            profile
                .preferences()
                .prompt_status_detail()
                .source()
                .as_str(),
            "essential, workspace, or token metrics",
        ),
        SettingsPreference::ReducedMotion => (
            "Reduced motion",
            bool_label(*profile.preferences().reduced_motion().value()).to_owned(),
            profile.preferences().reduced_motion().source().as_str(),
            "stops animated status indicators",
        ),
        SettingsPreference::Density => (
            "Density",
            density_label(*profile.preferences().density().value()).to_owned(),
            profile.preferences().density().source().as_str(),
            "spacing between terminal elements",
        ),
        SettingsPreference::Layout => (
            "Layout",
            layout_label(*profile.preferences().layout().value()).to_owned(),
            profile.preferences().layout().source().as_str(),
            "panel arrangement",
        ),
        SettingsPreference::TerminalTimestampStyle => (
            "Timestamp style",
            timestamp_style_label(*profile.preferences().terminal_timestamp_style().value())
                .to_owned(),
            profile
                .preferences()
                .terminal_timestamp_style()
                .source()
                .as_str(),
            "terminal timestamp display",
        ),
        SettingsPreference::ComposerSubmitBehavior => (
            "Composer submit",
            composer_submit_label(*profile.preferences().composer_submit_behavior().value())
                .to_owned(),
            profile
                .preferences()
                .composer_submit_behavior()
                .source()
                .as_str(),
            "prompt submission chord",
        ),
    };
    let selected = !model.settings_workspace.nav_focus
        && SettingsPreference::at(model.settings_workspace.selected) == preference;
    let wheel = selected
        .then(|| settings_preference_wheel(model, preference))
        .flatten();
    let marker = if selected {
        selection_marker(model)
    } else {
        " "
    };
    let saving = model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::UpdateLocalPreference(_)));
    let suffix = if selected && saving { " [saving]" } else { "" };
    let style = if selected {
        visual_style(model, VisualRole::Selected)
    } else {
        visual_style(model, VisualRole::Normal)
    };
    let content = wheel.map_or_else(
        || format!("{marker} {label:<18} {value}  Source: {source}  {explanation}{suffix}"),
        |wheel| format!("{marker} {label:<14} {wheel}{suffix}"),
    );
    Line::styled(content, style)
}

fn wheel_value<T: Copy + PartialEq>(
    current: T,
    values: &[T],
    label: fn(T) -> &'static str,
) -> String {
    let index = values
        .iter()
        .position(|candidate| *candidate == current)
        .unwrap_or_default();
    let previous = values[index.checked_sub(1).unwrap_or(values.len() - 1)];
    let next = values[(index + 1) % values.len()];
    format!(
        "‹{}  [{}]  {}›",
        label(previous),
        label(current),
        label(next)
    )
}

fn settings_preference_wheel(model: &Model, preference: SettingsPreference) -> Option<String> {
    let preferences = model.settings().local_profile.preferences();
    match preference {
        SettingsPreference::ThemePreset => Some(wheel_value(
            *preferences.theme_preset().value(),
            &[
                ThemePreset::System,
                ThemePreset::Light,
                ThemePreset::Dark,
                ThemePreset::Aurora,
                ThemePreset::Ember,
                ThemePreset::Midnight,
                ThemePreset::Ocean,
                ThemePreset::Forest,
                ThemePreset::Rose,
            ],
            theme_preset_label,
        )),
        SettingsPreference::ColorMode => Some(wheel_value(
            *preferences.color_mode().value(),
            &[
                ColorMode::Color,
                ColorMode::Soft,
                ColorMode::Vivid,
                ColorMode::NoColor,
                ColorMode::HighContrast,
            ],
            color_mode_label,
        )),
        SettingsPreference::GlyphMode => Some(wheel_value(
            *preferences.glyph_mode().value(),
            &[GlyphMode::Unicode, GlyphMode::NerdFont, GlyphMode::Ascii],
            glyph_mode_label,
        )),
        SettingsPreference::PromptStatusDetail => Some(wheel_value(
            *preferences.prompt_status_detail().value(),
            &[
                PromptStatusDetail::Essential,
                PromptStatusDetail::Workspace,
                PromptStatusDetail::Detailed,
            ],
            prompt_status_detail_label,
        )),
        SettingsPreference::ReducedMotion => Some(wheel_value(
            *preferences.reduced_motion().value(),
            &[false, true],
            bool_label,
        )),
        SettingsPreference::Density => Some(wheel_value(
            *preferences.density().value(),
            &[Density::Comfortable, Density::Compact],
            density_label,
        )),
        SettingsPreference::Layout => Some(wheel_value(
            *preferences.layout().value(),
            &[PreferenceLayout::Responsive, PreferenceLayout::SingleColumn],
            layout_label,
        )),
        SettingsPreference::TerminalTimestampStyle => Some(wheel_value(
            *preferences.terminal_timestamp_style().value(),
            &[
                TerminalTimestampStyle::Relative,
                TerminalTimestampStyle::Absolute,
                TerminalTimestampStyle::Hidden,
            ],
            timestamp_style_label,
        )),
        SettingsPreference::ComposerSubmitBehavior => Some(wheel_value(
            *preferences.composer_submit_behavior().value(),
            &[
                ComposerSubmitBehavior::ControlS,
                ComposerSubmitBehavior::Enter,
            ],
            composer_submit_label,
        )),
        _ => None,
    }
}

fn settings_preference_label(preference: SettingsPreference) -> &'static str {
    match preference {
        SettingsPreference::DisplayLabel => "Display label",
        SettingsPreference::Provider => "Provider",
        SettingsPreference::Profile => "Profile",
        SettingsPreference::Credential => "Credential",
        SettingsPreference::Source => "Source",
        SettingsPreference::Model => "Model",
        SettingsPreference::Mode => "Thinking",
        SettingsPreference::ThemePreset => "Theme preset",
        SettingsPreference::ColorMode => "Color mode",
        SettingsPreference::GlyphMode => "Glyph mode",
        SettingsPreference::PromptStatusDetail => "Prompt detail",
        SettingsPreference::ReducedMotion => "Reduced motion",
        SettingsPreference::Density => "Density",
        SettingsPreference::Approvals => "Approvals",
        SettingsPreference::Retention => "Retention",
        SettingsPreference::Logging => "Logging",
        SettingsPreference::Layout => "Layout",
        SettingsPreference::TerminalTimestampStyle => "Timestamp style",
        SettingsPreference::ComposerSubmitBehavior => "Composer submit",
    }
}

fn settings_scroll(lines: &[Line<'static>], selected: SettingsPreference, area: Rect) -> u16 {
    let needle = settings_preference_label(selected);
    let target = lines
        .iter()
        .position(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains(needle)
        })
        .unwrap_or_default();
    let prefix_rows = Paragraph::new(Text::from(lines[..target].to_vec()))
        .wrap(Wrap { trim: false })
        .line_count(area.width);
    let visible_rows = usize::from(area.height.max(1));
    u16::try_from(prefix_rows.saturating_sub(visible_rows / 3)).unwrap_or(u16::MAX)
}

fn theme_preset_label(value: ThemePreset) -> &'static str {
    match value {
        ThemePreset::System => "system",
        ThemePreset::Light => "light",
        ThemePreset::Dark => "dark",
        ThemePreset::Aurora => "aurora",
        ThemePreset::Ember => "ember",
        ThemePreset::Midnight => "midnight",
        ThemePreset::Ocean => "ocean",
        ThemePreset::Forest => "forest",
        ThemePreset::Rose => "rose",
    }
}

fn color_mode_label(value: ColorMode) -> &'static str {
    match value {
        ColorMode::Color => "color",
        ColorMode::Soft => "soft",
        ColorMode::Vivid => "vivid",
        ColorMode::NoColor => "no color",
        ColorMode::HighContrast => "high contrast",
    }
}

fn glyph_mode_label(value: GlyphMode) -> &'static str {
    match value {
        GlyphMode::Unicode => "unicode",
        GlyphMode::NerdFont => "Nerd Font",
        GlyphMode::Ascii => "ASCII",
    }
}

fn prompt_status_detail_label(value: PromptStatusDetail) -> &'static str {
    match value {
        PromptStatusDetail::Essential => "essential",
        PromptStatusDetail::Workspace => "workspace",
        PromptStatusDetail::Detailed => "detailed",
    }
}

fn bool_label(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn density_label(value: Density) -> &'static str {
    match value {
        Density::Comfortable => "comfortable",
        Density::Compact => "compact",
    }
}

fn layout_label(value: PreferenceLayout) -> &'static str {
    match value {
        PreferenceLayout::Responsive => "responsive",
        PreferenceLayout::SingleColumn => "single column",
    }
}

fn timestamp_style_label(value: TerminalTimestampStyle) -> &'static str {
    match value {
        TerminalTimestampStyle::Relative => "relative",
        TerminalTimestampStyle::Absolute => "absolute",
        TerminalTimestampStyle::Hidden => "hidden",
    }
}

fn session_timestamp_label(model: &Model, updated_at_ms: i64) -> Option<String> {
    match *model
        .settings()
        .local_profile
        .preferences()
        .terminal_timestamp_style()
        .value()
    {
        TerminalTimestampStyle::Relative => Some("updated".to_owned()),
        TerminalTimestampStyle::Absolute => Some(format!("updated {updated_at_ms}")),
        TerminalTimestampStyle::Hidden => None,
    }
}

fn composer_submit_label(value: ComposerSubmitBehavior) -> &'static str {
    match value {
        ComposerSubmitBehavior::ControlS => "Ctrl+S / Ctrl+Enter",
        ComposerSubmitBehavior::Enter => "Enter",
    }
}
/// Renders provider choices, connected profiles, and provider-specific setup.
fn render_profile_center(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let inner = area;
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let compact = presentation(model).compact || inner.width < 72;
    let notice_height = if model.notice.is_some() && inner.height >= 8 {
        if compact { 1 } else { 2 }
    } else {
        0
    };
    let header_height = if inner.height >= 8 { 2 } else { 1 };
    let help_height = u16::from(inner.height >= 4);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(help_height),
        ])
        .split(inner);

    let subtitle = if compact {
        "Add or manage provider connections."
    } else {
        "Choose a provider on the left. Manage connected providers on the right."
    };
    render_settings_page_header(frame, rows[0], model, "Providers", subtitle);
    let (list_area, detail_area) = profile_list_detail_areas(rows[1], model);
    render_connected_profiles(frame, list_area, model);
    if let Some(detail_area) = detail_area {
        render_profile_detail(frame, detail_area, model);
    }
    if notice_height > 0 {
        render_notice(frame, rows[2], model);
    }
    if help_height > 0 {
        let return_to = if model.route() == Route::Settings {
            "Settings"
        } else {
            "Chat"
        };
        let help = if inner.width < 48 {
            format!("↑/↓ choose  Enter open  Esc {return_to}")
        } else if inner.width < 72 {
            format!("↑/↓ choose  ←/→ section  Enter open  Esc {return_to}")
        } else if inner.width < 96 {
            format!("←/→ section  ↑/↓ choose  Enter open  Alt+K sign-in  Esc {return_to}")
        } else {
            format!(
                "←/→ section  ↑/↓ choose  Enter open  Alt+K sign-in  Alt+T test  Del remove  Esc {return_to}"
            )
        };
        frame.render_widget(
            Paragraph::new(help).style(visual_style(model, VisualRole::Muted)),
            rows[3],
        );
    }
    if model.profile_center.auth_page == Some(crate::model::ProviderChoice::Codex) {
        render_codex_authentication(frame, area, model);
    } else if model.profile_center.editor.is_some() {
        render_profile_editor(frame, area, model);
    } else if model.profile_center.credential.is_some() {
        render_profile_credential(frame, area, model);
    }
}

fn profile_list_detail_areas(area: Rect, model: &Model) -> (Rect, Option<Rect>) {
    if !presentation(model).single_column && area.width >= 60 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(area);
        (columns[0], Some(columns[1]))
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area);
        (rows[0], Some(rows[1]))
    }
}

fn render_connected_profiles(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let focused = model.profile_center.focus == ProfileCenterFocus::ProviderChoices;
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Add provider ")
        .border_style(visual_style(
            model,
            if focused {
                VisualRole::Selected
            } else {
                VisualRole::Border
            },
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let selected = model
        .profile_center
        .choice_selected
        .min(PROVIDER_CHOICES.len().saturating_sub(1));
    let lines = PROVIDER_CHOICES
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let selected = index == selected;
            let style = if selected && focused {
                visual_style(model, VisualRole::Selected)
            } else {
                visual_style(model, VisualRole::Normal)
            };
            let marker = if selected {
                selection_marker(model)
            } else {
                " "
            };
            Line::styled(
                format!(
                    "{marker} {:<22} {}",
                    choice.label(),
                    provider_choice_status(model, *choice)
                ),
                style,
            )
        })
        .collect::<Vec<_>>();
    let scroll = profile_list_scroll(selected, lines.len(), inner.height);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn profile_list_scroll(selected: usize, count: usize, visible: u16) -> u16 {
    let visible = usize::from(visible.max(1));
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(count.saturating_sub(visible));
    u16::try_from(start).unwrap_or(u16::MAX)
}

fn render_profile_detail(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let focused = model.profile_center.focus == ProfileCenterFocus::ConnectedProfiles;
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Connected providers ")
        .border_style(visual_style(
            model,
            if focused {
                VisualRole::Selected
            } else {
                VisualRole::Border
            },
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let profiles = model.filtered_profiles().collect::<Vec<_>>();
    let Some(profile) = model.selected_profile() else {
        if inner.width > 0 && inner.height > 0 {
            frame.render_widget(
                Paragraph::new(
                    "No saved connections yet. Choose a provider on the left to connect.",
                )
                .style(visual_style(model, VisualRole::Muted)),
                inner,
            );
        }
        return;
    };
    let mut lines = profiles
        .iter()
        .map(|candidate| {
            let selected = candidate.id == profile.id;
            let marker = if selected {
                selection_marker(model)
            } else {
                " "
            };
            let state = if candidate.active {
                "active"
            } else {
                candidate.connection.label()
            };
            Line::styled(
                format!("{marker} {:<20} {state}", display_safe(&candidate.id)),
                if selected && focused {
                    visual_style(model, VisualRole::Selected)
                } else {
                    visual_style(model, VisualRole::Normal)
                },
            )
        })
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    let credential_stored = matches!(
        profile.credential_state,
        crate::model::ProfileCredentialStateLabel::Stored
    );
    let (sign_in, managed_by) = if profile.kind == ProviderKindLabel::CodexCli {
        (
            "ChatGPT browser",
            if credential_stored {
                "operating-system vault"
            } else {
                "not connected"
            },
        )
    } else {
        (
            "API key",
            match profile.credential_source {
                crate::model::CredentialSourceLabel::Environment => "environment variable",
                crate::model::CredentialSourceLabel::CredentialVault => "operating-system vault",
                crate::model::CredentialSourceLabel::SessionOnly => "not saved",
            },
        )
    };
    lines.extend([
        detail_line(model, "Profile", &profile.id),
        detail_line(model, "Provider", provider_display_name(profile.kind)),
        detail_line(
            model,
            "Status",
            if profile.active && credential_stored {
                "active"
            } else if profile.active {
                "selected - sign-in required"
            } else {
                "saved"
            },
        ),
        detail_line(model, "Connection", profile.connection.label()),
        detail_line(model, "Sign-in", sign_in),
        detail_line(model, "Credential", managed_by),
        detail_line(
            model,
            "Default model",
            profile
                .default_model
                .as_deref()
                .unwrap_or("provider default"),
        ),
        detail_line(model, "Thinking", &profile.default_mode),
    ]);
    if profile.kind == ProviderKindLabel::Router {
        lines.push(detail_line(model, "Base URL", &profile.base_url));
        if !profile.project.is_empty() {
            lines.push(detail_line(model, "Project", &profile.project));
        }
        if !profile.auth_header.is_empty() {
            lines.push(detail_line(model, "Auth header", &profile.auth_header));
        }
    }
    if let ProfileConnectionState::Failed(reason) = &profile.connection {
        lines.push(detail_line(model, "Failure", reason));
    }
    if model.profiles().pending_recovery > 0 {
        lines.push(detail_line(model, "Recovery", "pending"));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        if profile.kind == ProviderKindLabel::CodexCli {
            "[ Sign in ] [ Test ] [ Model ]"
        } else {
            "[ API key ] [ Test ] [ Model ]"
        },
        visual_style(model, VisualRole::Assistant),
    ));
    lines.push(Line::styled(
        "[ Disconnect ] [ Remove ]",
        visual_style(model, VisualRole::Warning),
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_codex_authentication(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = codex_auth_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Sign in to Codex ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let (action, status) = match model.profile_center.codex_login {
        crate::model::CodexLoginState::Idle => (
            format!("{} Sign in with ChatGPT", selection_marker(model)),
            "The browser will open for secure sign-in.",
        ),
        crate::model::CodexLoginState::Starting => (
            format!("{} Opening your browser...", spinner(model)),
            "AutoHarness is preparing a secure local callback.",
        ),
        crate::model::CodexLoginState::BrowserOpened => (
            "Browser opened".to_owned(),
            "Finish signing in there. AutoHarness will connect automatically.",
        ),
        crate::model::CodexLoginState::Failed => (
            format!("{} Try sign-in again", selection_marker(model)),
            "Sign-in did not finish. Press Enter to retry.",
        ),
    };
    let mut lines = vec![
        Line::styled(
            "Connect your Codex subscription in your default browser.",
            visual_style(model, VisualRole::User),
        ),
        Line::from(""),
        Line::styled(action, visual_style(model, VisualRole::Selected)),
        Line::styled(status, visual_style(model, VisualRole::Muted)),
        Line::from(""),
        Line::styled(
            "Enter sign in or retry  Esc cancel",
            visual_style(model, VisualRole::Muted),
        ),
    ];
    lines.extend([
        Line::from(""),
        Line::styled(
            "Sign-in tokens are kept in your operating-system credential vault.",
            visual_style(model, VisualRole::Muted),
        ),
    ]);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn provider_display_name(kind: ProviderKindLabel) -> &'static str {
    match kind {
        ProviderKindLabel::Gemini => "Google AI Studio",
        ProviderKindLabel::Router => "OpenAI-compatible API",
        ProviderKindLabel::CodexCli => "Codex subscription",
    }
}

fn provider_choice_status(model: &Model, choice: crate::model::ProviderChoice) -> &'static str {
    match choice {
        crate::model::ProviderChoice::Gemini | crate::model::ProviderChoice::GoogleAiStudio => {
            "API key"
        }
        crate::model::ProviderChoice::Codex => {
            if model.profiles().profiles.iter().any(|profile| {
                profile.kind == ProviderKindLabel::CodexCli
                    && matches!(
                        profile.credential_state,
                        crate::model::ProfileCredentialStateLabel::Stored
                    )
            }) {
                "Connected"
            } else {
                "Subscription"
            }
        }
        crate::model::ProviderChoice::Cursor => "Unavailable",
        crate::model::ProviderChoice::ClaudeCode => "Unavailable",
        crate::model::ProviderChoice::OpenAiCompatible => "API key",
    }
}

fn render_model_defaults(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let header_height = if area.height >= 8 { 2 } else { 1 };
    let help_height = u16::from(area.height >= 3);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(help_height),
        ])
        .split(area);
    render_settings_page_header(
        frame,
        rows[0],
        model,
        "Models",
        "Choose the model and thinking mode used by every new session.",
    );
    let inner = rows[1];
    let step = model.model_defaults.step;
    let step_chip = |label, candidate| {
        Span::styled(
            format!(" {label} "),
            if step == candidate {
                visual_style(model, VisualRole::Selected)
            } else {
                visual_style(model, VisualRole::Muted)
            },
        )
    };
    let mut lines = vec![
        Line::from(vec![
            step_chip("1  MODEL", ModelDefaultStep::Model),
            Span::raw("  "),
            step_chip("2  THINKING", ModelDefaultStep::Thinking),
        ]),
        Line::from(""),
    ];
    match step {
        ModelDefaultStep::Model => {
            let active_profile = model
                .profiles()
                .profiles
                .iter()
                .find(|profile| profile.active);
            if let Some(profile) = active_profile {
                lines.push(Line::from(vec![
                    Span::styled("Active profile  ", visual_style(model, VisualRole::Muted)),
                    Span::styled(
                        display_safe(&profile.id),
                        visual_style(model, VisualRole::User),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Current default ", visual_style(model, VisualRole::Muted)),
                    Span::raw(display_safe(
                        profile.default_model.as_deref().unwrap_or("Not set"),
                    )),
                    Span::styled("  Thinking  ", visual_style(model, VisualRole::Muted)),
                    Span::raw(display_safe(&profile.default_mode)),
                ]));
                lines.push(Line::from(""));
            }
            lines.push(Line::styled(
                "Select the default model",
                visual_style(model, VisualRole::User),
            ));
            let mut selectable_count = 0;
            for (index, summary) in model
                .catalog
                .models()
                .iter()
                .filter(|summary| summary.selectable)
                .enumerate()
            {
                selectable_count += 1;
                let selected = index == model.model_defaults.model_selected;
                let prefix = if selected {
                    selection_marker(model)
                } else {
                    " "
                };
                let is_default = active_profile
                    .and_then(|profile| profile.default_model.as_deref())
                    == Some(summary.model.model_id().as_str());
                let style = if selected {
                    visual_style(model, VisualRole::Selected)
                } else {
                    visual_style(model, VisualRole::Normal)
                };
                lines.push(Line::styled(
                    format!(
                        "{prefix} {}{}  {}",
                        display_safe(&summary.display_name),
                        if is_default { "  DEFAULT" } else { "" },
                        display_safe(&summary.detail)
                    ),
                    style,
                ));
            }
            if selectable_count == 0 {
                lines.push(Line::styled(
                    if active_profile.is_some() {
                        "Waiting for the active provider's compatible model catalog."
                    } else {
                        "Connect and activate a provider from the Providers tab first."
                    },
                    visual_style(model, VisualRole::Muted),
                ));
            }
        }
        ModelDefaultStep::Thinking => {
            lines.push(Line::styled(
                "Thinking mode",
                visual_style(model, VisualRole::User),
            ));
            for (index, effort) in [
                "Provider default",
                "None",
                "Low",
                "Medium",
                "High",
                "Extra high",
                "Maximum",
            ]
            .iter()
            .enumerate()
            {
                let selected = index == model.model_defaults.thinking_selected;
                let marker = if selected {
                    selection_marker(model)
                } else {
                    " "
                };
                let style = if selected {
                    visual_style(model, VisualRole::Selected)
                } else {
                    visual_style(model, VisualRole::Normal)
                };
                lines.push(Line::styled(format!("{marker} {effort}"), style));
            }
            lines.push(Line::styled(
                "Enter saves this model and thinking mode as the new-session default.",
                visual_style(model, VisualRole::Muted),
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    if help_height > 0 {
        frame.render_widget(
            Paragraph::new("↑/↓ select  Enter continue/save  Esc Settings")
                .style(visual_style(model, VisualRole::Muted)),
            rows[2],
        );
    }
}

fn render_local_profile(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let user = &model.profiles().user;
    let label = user.display_label.as_deref().unwrap_or("Local user");
    let default_profile = user.default_profile.as_deref().unwrap_or("session only");
    let default_model = user.default_model.as_deref().unwrap_or("not set");
    let default_mode = if user.default_mode.is_empty() {
        "safe agent"
    } else {
        user.default_mode.as_str()
    };
    let workspace_value = if user.workspace.is_empty() {
        "current workspace"
    } else {
        user.workspace.as_str()
    };
    let first = Line::from(vec![
        Span::styled(
            format!(" {} ", display_safe(label)),
            visual_style(model, VisualRole::Header),
        ),
        Span::styled("Default ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(default_profile)),
        Span::styled("  Model ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(default_model)),
        Span::styled("  Thinking ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(default_mode)),
    ]);
    let workspace = Line::from(vec![
        Span::styled(" Workspace ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(workspace_value)),
    ]);
    let mut lines = vec![first];
    if area.height >= 2 {
        lines.push(workspace);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(app_block(model).borders(Borders::BOTTOM))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn detail_line(model: &Model, label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<14}"),
            visual_style(model, VisualRole::Muted),
        ),
        Span::raw(display_safe(value)),
    ])
}

fn render_profile_editor(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let editor = model
        .profile_center
        .editor
        .as_ref()
        .expect("profile editor is open");
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let title = match (editor.mode, editor.kind) {
        (ProfileEditorMode::Create, ProviderKindLabel::CodexCli) => " Connect Codex subscription ",
        (ProfileEditorMode::Create, ProviderKindLabel::Gemini) => " Connect Gemini ",
        (ProfileEditorMode::Create, ProviderKindLabel::Router) => " Connect compatible API ",
        (ProfileEditorMode::Edit, _) => " Edit provider profile ",
        (ProfileEditorMode::Duplicate, _) => " Duplicate provider profile ",
    };
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(title)
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let rows = [
        ("Name", editor.id.as_str()),
        ("Provider", editor.kind.as_str()),
        ("Base URL", editor.base_url.as_str()),
        ("Project", editor.project.as_str()),
        ("Auth header", editor.auth_header.as_str()),
    ];
    let visible = if editor.mode == ProfileEditorMode::Duplicate {
        1
    } else {
        editor.field_count()
    };
    let mut lines = Vec::new();
    for (index, (label, value)) in rows.into_iter().take(visible).enumerate() {
        let selected = editor.field == index;
        let style = if selected {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
        let marker = if selected { ">" } else { " " };
        lines.push(Line::styled(
            format!("{marker} {label:<12} {}", display_safe(value)),
            style,
        ));
    }
    lines.push(Line::from(""));
    if editor.mode == ProfileEditorMode::Create && editor.kind == ProviderKindLabel::CodexCli {
        lines.push(Line::styled(
            "Use the Codex provider card instead. AutoHarness opens browser sign-in directly.",
            visual_style(model, VisualRole::Muted),
        ));
    } else {
        lines.push(Line::from(Span::styled(
            "↑/↓ next field  Left/Right provider  Enter save  Esc cancel",
            visual_style(model, VisualRole::Muted),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_profile_credential(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let editor = model
        .profile_center
        .credential
        .as_ref()
        .expect("profile credential editor is open");
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let action = match editor.action {
        ProfileCredentialAction::Save => "Save",
        ProfileCredentialAction::Replace => "Replace",
    };
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(format!(
            " {action} credential - {} ",
            display_safe(&editor.profile_id)
        ))
        .border_style(visual_style(model, VisualRole::Warning));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let masked = if editor.has_value() {
        if presentation(model).ascii {
            "********"
        } else {
            "••••••••"
        }
    } else {
        "paste or type API key"
    };
    let content_height = inner.height.saturating_sub(1);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{masked}\n\nStored only in the operating-system vault.",),
            visual_style(model, VisualRole::Warning),
        )))
        .wrap(Wrap { trim: false }),
        content,
    );
    let actions = Rect::new(inner.x, inner.y + content_height, inner.width, 1);
    frame.render_widget(
        Paragraph::new("[ Save ] Enter    [ Cancel ] Esc")
            .style(visual_style(model, VisualRole::Assistant)),
        actions,
    );
}

/// Renders the searchable session-browser overlay from local state only.
fn render_browser(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = area;
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Sessions ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let search_height = 1.min(inner.height);
    let help_height = u16::from(inner.height >= 3);
    let list_height = inner.height.saturating_sub(search_height + help_height);
    let search = Rect::new(inner.x, inner.y, inner.width, search_height);
    let list = Rect::new(inner.x, inner.y + search_height, inner.width, list_height);
    let help = Rect::new(
        inner.x,
        inner.y + search_height + list_height,
        inner.width,
        help_height,
    );

    let search_line = if model.browser.renaming {
        format!("Rename: {}", display_safe(&model.browser.rename_buffer))
    } else {
        format!("Filter: {}", display_safe(&model.browser.query))
    };
    frame.render_widget(
        Paragraph::new(search_line).style(visual_style(model, VisualRole::Field)),
        search,
    );

    let entries = model.browser_entries();
    if entries.is_empty() {
        let empty = if model.sessions.sessions.is_empty() {
            "No durable sessions yet.\nCtrl+N creates and opens the first session."
        } else {
            "No sessions match this filter.\nBackspace clears the filter."
        };
        frame.render_widget(
            Paragraph::new(empty)
                .style(visual_style(model, VisualRole::Muted))
                .wrap(Wrap { trim: false }),
            list,
        );
    } else {
        let selected_index = model
            .browser
            .selected
            .as_ref()
            .and_then(|selected| {
                entries
                    .iter()
                    .position(|entry| &entry.session_id == selected)
            })
            .unwrap_or(0);
        let visible = usize::from(list.height);
        let start = selected_index
            .saturating_add(1)
            .saturating_sub(visible)
            .min(entries.len().saturating_sub(visible));
        let items = entries
            .iter()
            .skip(start)
            .take(visible)
            .map(|entry| browser_item(entry, model))
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), list);
    }

    if help.height > 0 && !model.browser.renaming {
        let hints = if model.overlay() == Some(OverlayKind::Confirmation) {
            "[ Y Confirm ]  [ N Cancel ]"
        } else if help.width >= 50 {
            "[ Open ] Enter  [ Rename ] Ctrl+R  [ Archive ] Ctrl+A  [ Delete ] Ctrl+D  Esc"
        } else {
            "[ Open ]  [ Rename ]  [ Delete ]  Esc"
        };
        frame.render_widget(
            Paragraph::new(hints).style(visual_style(model, VisualRole::Muted)),
            help,
        );
    }
}

fn browser_item(entry: &crate::model::SessionBrowserEntry, model: &Model) -> ListItem<'static> {
    let selected = model
        .browser
        .selected
        .as_ref()
        .is_some_and(|candidate| candidate == &entry.session_id);
    let prefix = if selected {
        selection_marker(model)
    } else {
        " "
    };
    let mut label = format!("{prefix} {}", display_safe(&entry.title));
    if entry.active {
        label.push_str("  [active]");
    }
    if entry.archived {
        label.push_str("  [archived]");
    }
    if let Some(timestamp) = session_timestamp_label(model, entry.updated_at_ms) {
        let _ = write!(label, "  [{timestamp}]");
    }
    let style = if selected {
        visual_style(model, VisualRole::Selected)
    } else if entry.archived {
        visual_style(model, VisualRole::Muted)
    } else {
        visual_style(model, VisualRole::Normal)
    };
    ListItem::new(Line::styled(label, style))
}

fn render_permission(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = credential_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Tool permission ")
        .border_style(visual_style(model, VisualRole::Warning));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let Some(request) = model.session.permission_requests.first() else {
        return;
    };
    let pending = model.answering_permissions.contains(&request.tool_call_id);
    let mut lines = vec![
        Line::styled(
            "A model requested an external capability.",
            visual_style(model, VisualRole::Normal),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", visual_style(model, VisualRole::Muted)),
            Span::raw(display_safe(&request.tool_name)),
        ]),
        Line::from(vec![
            Span::styled("Capability: ", visual_style(model, VisualRole::Muted)),
            Span::raw(display_safe(&request.capability)),
        ]),
        Line::from(vec![
            Span::styled("Resource: ", visual_style(model, VisualRole::Muted)),
            Span::raw(display_safe(&request.resource)),
        ]),
        Line::from(""),
    ];
    lines.extend(request.details.iter().map(|detail| {
        Line::from(vec![
            Span::styled(
                format!("{}: ", display_safe(&detail.label)),
                visual_style(model, VisualRole::Muted),
            ),
            Span::raw(display_safe(&detail.value)),
        ])
    }));
    let content_height = inner.height.saturating_sub(1);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((model.permission_scroll, 0)),
        content,
    );
    let actions = Rect::new(inner.x, inner.y + content_height, inner.width, 1);
    let action_text = if pending {
        "Saving answer..."
    } else {
        "[ Allow ] Y    [ Deny ] N/Esc deny"
    };
    frame.render_widget(
        Paragraph::new(action_text).style(visual_style(model, VisualRole::Warning)),
        actions,
    );
}

fn selected_model_name(model: &Model) -> String {
    let Some(selected) = model.session.selected_model.as_ref() else {
        return "not selected".to_owned();
    };
    model
        .catalog
        .models()
        .iter()
        .find(|summary| &summary.model == selected)
        .map_or_else(
            || selected.model_id().as_str().to_owned(),
            |summary| summary.display_name.clone(),
        )
}

fn thinking_meter(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => "[......]",
        "minimal" => "[#.....]",
        "low" => "[##....]",
        "medium" => "[###...]",
        "high" => "[####..]",
        "xhigh" => "[#####.]",
        "max" => "[######]",
        _ => "[auto]",
    }
}

fn workspace_display_path(workspace: &str) -> String {
    let normalized = display_safe(workspace.trim()).replace('\\', "/");
    if normalized.is_empty() {
        return ".".to_owned();
    }
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let home_suffix = if parts.len() >= 3
        && (parts[0].eq_ignore_ascii_case("home") || parts[0].eq_ignore_ascii_case("users"))
    {
        Some(&parts[2..])
    } else if parts.len() >= 4 && parts[0].ends_with(':') && parts[1].eq_ignore_ascii_case("users")
    {
        Some(&parts[3..])
    } else {
        None
    };
    if let Some(suffix) = home_suffix {
        if suffix.is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", suffix.join("/"))
        }
    } else {
        normalized
    }
}

fn metadata_separator(model: &Model) -> &'static str {
    if presentation(model).nerd_font {
        "  "
    } else {
        "  "
    }
}

fn path_marker(model: &Model) -> &'static str {
    if presentation(model).nerd_font {
        " "
    } else {
        ""
    }
}

fn branch_marker(model: &Model) -> &'static str {
    if presentation(model).nerd_font {
        " "
    } else if presentation(model).ascii {
        "* "
    } else {
        "⑂ "
    }
}

fn push_metadata_piece(
    spans: &mut Vec<Span<'static>>,
    model: &Model,
    value: String,
    role: VisualRole,
) {
    if !spans.is_empty() {
        spans.push(Span::styled(
            metadata_separator(model),
            chat_visual_style(model, VisualRole::Muted),
        ));
    }
    spans.push(Span::styled(value, chat_visual_style(model, role)));
}

fn prompt_metadata_line(model: &Model, width: u16) -> Line<'static> {
    let mut spans = Vec::new();
    let model_width = if width >= 64 { 22 } else { 14 };
    push_metadata_piece(
        &mut spans,
        model,
        single_line_label(&selected_model_name(model), model_width),
        VisualRole::Assistant,
    );
    if width >= 24 {
        push_metadata_piece(
            &mut spans,
            model,
            thinking_meter(&model.profiles().user.default_mode).to_owned(),
            VisualRole::User,
        );
    }
    if width >= 48 {
        push_metadata_piece(
            &mut spans,
            model,
            format!(
                "{}{}",
                path_marker(model),
                single_line_label(
                    &workspace_display_path(&model.profiles().user.workspace),
                    if width >= 84 { 28 } else { 18 }
                )
            ),
            VisualRole::Normal,
        );
    }
    if width >= 76
        && let Some(branch) = model.settings().git_branch.as_deref()
    {
        push_metadata_piece(
            &mut spans,
            model,
            format!("{}{}", branch_marker(model), single_line_label(branch, 18)),
            VisualRole::Tool,
        );
    }
    Line::from(spans)
}

fn render_prompt_bar(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let horizontal_inset = u16::from(area.width >= 4);
    let surface = Rect::new(
        area.x.saturating_add(horizontal_inset),
        area.y,
        area.width
            .saturating_sub(horizontal_inset.saturating_mul(2)),
        area.height,
    );
    let rule = Rect::new(
        surface.x,
        surface.bottom().saturating_sub(1),
        surface.width,
        u16::from(surface.height > 0),
    );
    let inner = Rect::new(
        surface.x,
        surface.y,
        surface.width,
        surface.height.saturating_sub(1),
    );
    render_horizontal_gradient(frame, rule, model);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let metadata = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(prompt_metadata_line(model, inner.width))
            .style(chat_visual_style(model, VisualRole::Normal)),
        metadata,
    );
    if inner.height < 2 {
        return;
    }
    let editor_area = Rect::new(
        inner.x.saturating_add(2),
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height - 1,
    );
    let prompt = if presentation(model).ascii {
        "> "
    } else {
        "❯ "
    };
    frame.render_widget(
        Paragraph::new(prompt).style(chat_visual_style(model, VisualRole::Assistant)),
        Rect::new(inner.x, inner.y + 1, inner.width.min(2), inner.height - 1),
    );
    if model.palette_open() {
        frame.render_widget(
            Paragraph::new(format!("/{}", display_safe(&model.palette.query)))
                .style(chat_visual_style(model, VisualRole::Assistant)),
            editor_area,
        );
        set_palette_cursor(frame, editor_area, model);
        return;
    }
    let mut composer = model.composer.editor.clone();
    composer.remove_block();
    composer.set_cursor_line_style(chat_visual_style(model, VisualRole::Normal));
    composer.set_cursor_style(visual_style(model, VisualRole::Selected));
    frame.render_widget(&composer, editor_area);
    set_composer_cursor(frame, editor_area, model, false);
}

fn prompt_surface_height(area: Rect, model: &Model) -> u16 {
    u16::try_from(model.composer.lines().len())
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .clamp(3, if presentation(model).compact { 4 } else { 5 })
        .min(area.height)
}

fn render_standard(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let compact = presentation(model).compact;
    let composer_height = prompt_surface_height(area, model);
    let notice_height = if model.notice.is_some() {
        if compact { 1 } else { 2 }
    } else {
        0
    };
    let search_height = u16::from(model.search_open());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(search_height),
            Constraint::Length(composer_height),
        ])
        .split(area);

    render_transcript(frame, chunks[0], model);
    if notice_height > 0 {
        render_notice(frame, chunks[1], model);
    }
    if search_height > 0 {
        render_search_bar(frame, chunks[2], model);
    }
    render_prompt_bar(frame, chunks[3], model);
}

/// Renders the one-row transcript search bar.
fn render_search_bar(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let status = model.search_status_label();
    let query = display_safe(&model.search.query);
    frame.render_widget(
        Paragraph::new(format!(" Search: /{query} - {status} "))
            .style(visual_style(model, VisualRole::Field)),
        area,
    );
}

fn render_compact(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let composer_height = prompt_surface_height(area, model);
    let transcript_height = area.height.saturating_sub(composer_height);
    let transcript = Rect::new(area.x, area.y, area.width, transcript_height);
    let composer = Rect::new(
        area.x,
        area.y.saturating_add(transcript_height),
        area.width,
        composer_height,
    );
    if transcript.height > 0 {
        render_transcript(frame, transcript, model);
    }
    if composer.height > 0 {
        render_prompt_bar(frame, composer, model);
    }
}
fn selected_model_label(model: &Model) -> String {
    model
        .session
        .selected_model
        .as_ref()
        .map(|selected| {
            model
                .catalog
                .models()
                .iter()
                .find(|summary| &summary.model == selected)
                .map_or_else(
                    || selected.model_id().as_str().to_owned(),
                    |summary| summary.display_name.clone(),
                )
        })
        .unwrap_or_else(|| "no model".to_owned())
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let text = transparent_chat_text(transcript_text(model));
    let horizontal_inset = u16::from(area.width >= 4);
    let inner = Rect::new(
        area.x.saturating_add(horizontal_inset),
        area.y,
        area.width
            .saturating_sub(horizontal_inset.saturating_mul(2)),
        area.height,
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let paragraph = Paragraph::new(text)
        .style(chat_visual_style(model, VisualRole::Normal))
        .wrap(Wrap { trim: false });
    let total_rows = paragraph.line_count(inner.width);
    let viewport_rows = usize::from(inner.height);
    let maximum_scroll = total_rows.saturating_sub(viewport_rows);

    // An active search jump pins its row into view; ordinary scrolling and
    // tail-follow apply otherwise.
    let top: usize = if let Some(pinned) = model.search_pinned_row.filter(|_| model.search_open()) {
        pinned.saturating_sub(viewport_rows / 4).min(maximum_scroll)
    } else if model.transcript.follow_tail {
        maximum_scroll
    } else {
        maximum_scroll.saturating_sub(model.transcript.rows_from_bottom.min(maximum_scroll))
    };
    let top = u16::try_from(top).unwrap_or(u16::MAX);
    frame.render_widget(paragraph.scroll((top, 0)), inner);
}
fn transparent_chat_text(mut text: Text<'static>) -> Text<'static> {
    for line in &mut text.lines {
        for span in &mut line.spans {
            span.style = span.style.bg(Color::Reset);
        }
    }
    text
}

/// Plain text of the whole transcript for clipboard copy.
///
/// The output is display-safe (control characters escaped) and identical to
/// what search matches against.
pub(crate) fn transcript_plain_text(model: &Model) -> String {
    let mut lines = transcript_display_lines(model);
    lines.push(String::new());
    lines.join("\n")
}

/// Plain display text of every transcript line, shared by rendering and
/// search so match counts always describe what is actually visible.
pub(crate) fn transcript_display_lines(model: &Model) -> Vec<String> {
    let text = transcript_text(model);
    text.lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        })
        .collect()
}

fn transcript_text(model: &Model) -> Text<'static> {
    let mut lines = Vec::new();
    if model.session.transcript.is_empty() {
        match &*model.catalog {
            CatalogProjection::CredentialRequired => {
                lines.push(Line::styled(
                    "OFFLINE",
                    visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::from("No provider credential is available."));
                lines.push(Line::styled(
                    "Provider API key: use /settings",
                    visual_style(model, VisualRole::Assistant),
                ));
                lines.push(Line::styled(
                    "An API key is still required. Ask AutoHarness after setup.",
                    visual_style(model, VisualRole::Muted),
                ));
            }
            CatalogProjection::Loading => {
                lines.push(Line::styled(
                    format!("{}  CONNECTING", spinner(model)),
                    visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::from("Loading provider models..."));
            }
            CatalogProjection::Failed(failure) => {
                lines.push(Line::styled(
                    "CONNECTION ERROR",
                    visual_style(model, VisualRole::Error),
                ));
                lines.push(Line::from(display_safe(&failure.message)));
                lines.push(Line::styled(
                    "Ctrl+R retry or Alt+3 inspect provider settings",
                    visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { models, .. } if models.is_empty() => {
                lines.push(Line::styled(
                    "NO COMPATIBLE MODELS",
                    visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::styled(
                    "Ctrl+R refresh the catalog",
                    visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { .. } if model.session.selected_model.is_none() => {
                lines.push(Line::styled(
                    "CHOOSE A MODEL",
                    visual_style(model, VisualRole::User),
                ));
                lines.push(Line::styled(
                    "Ctrl+P opens the searchable model catalog",
                    visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { .. } => {
                lines.push(Line::styled(
                    "NEW CONVERSATION",
                    visual_style(model, VisualRole::User),
                ));
                lines.push(Line::from("Write a prompt below."));
                let send = if *model
                    .settings()
                    .local_profile
                    .preferences()
                    .composer_submit_behavior()
                    .value()
                    == ComposerSubmitBehavior::ControlS
                {
                    "Ctrl+S sends"
                } else {
                    "Enter sends"
                };
                lines.push(Line::styled(send, visual_style(model, VisualRole::Muted)));
                render_onboarding(&mut lines, model);
            }
        }
        return transparent_chat_text(Text::from(lines));
    }

    for (index, item) in model.session.transcript.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        match item {
            TranscriptItem::User { text, .. } => {
                lines.push(Line::styled("YOU", visual_style(model, VisualRole::User)));
                push_safe_lines(&mut lines, text, visual_style(model, VisualRole::Normal));
            }
            TranscriptItem::Tool(row) => {
                let mut heading = format!("TOOL{}", chrome_separator(model));
                heading.push_str(&display_safe(&row.tool_name));
                if !row.status.is_empty() {
                    heading.push_str(chrome_separator(model));
                    heading.push_str(&display_safe(&row.status));
                }
                if let Some(summary) = &row.summary {
                    heading.push_str(chrome_separator(model));
                    heading.push_str(&display_safe(summary));
                }
                if model.tools_expanded {
                    heading.push_str("  [");
                    heading.push_str(&display_safe(&row.resource));
                    heading.push(']');
                }
                lines.push(Line::styled(heading, visual_style(model, VisualRole::Tool)));
            }
            TranscriptItem::Assistant {
                attempt_id,
                text,
                status,
                usage,
                retry_of,
            } => {
                let mut heading = String::from("AUTOHARNESS");
                if retry_of.is_some() {
                    heading.push_str(chrome_separator(model));
                    heading.push_str("retry");
                }
                match status {
                    AttemptStatus::Streaming => {
                        let _ = write!(
                            heading,
                            "{}{} generating",
                            chrome_separator(model),
                            generation_animation(model)
                        );
                    }
                    AttemptStatus::Cancelling => {
                        let _ = write!(
                            heading,
                            "{}{} cancelling",
                            chrome_separator(model),
                            spinner(model)
                        );
                    }
                    AttemptStatus::Completed => {
                        heading.push_str(chrome_separator(model));
                        heading.push_str("complete");
                    }
                    AttemptStatus::Cancelled => {
                        heading.push_str(chrome_separator(model));
                        heading.push_str("cancelled");
                    }
                    AttemptStatus::Failed(_) => {
                        heading.push_str(chrome_separator(model));
                        heading.push_str("failed");
                    }
                }
                if matches!(status, AttemptStatus::Streaming)
                    && (model.pending.values().any(|pending| {
                        matches!(pending, PendingKind::CancelAttempt(candidate) if candidate == attempt_id)
                    }) || model.cancelling.contains(attempt_id))
                {
                    heading.push_str(chrome_separator(model));
                    heading.push_str("cancelling");
                }
                if model.retry_requested(attempt_id) {
                    heading.push_str(chrome_separator(model));
                    heading.push_str("retrying");
                }
                let style = if matches!(status, AttemptStatus::Failed(_)) {
                    visual_style(model, VisualRole::Error)
                } else {
                    visual_style(model, VisualRole::Assistant)
                };
                lines.push(Line::styled(heading, style));
                if text.is_empty() && matches!(status, AttemptStatus::Streaming) {
                    lines.push(Line::styled(
                        "Waiting for the first token...",
                        visual_style(model, VisualRole::Muted),
                    ));
                } else {
                    push_safe_lines(&mut lines, text, visual_style(model, VisualRole::Normal));
                }
                if let AttemptStatus::Failed(failure) = status {
                    lines.push(Line::styled(
                        format!("Error: {}", display_safe(&failure.message)),
                        visual_style(model, VisualRole::Error),
                    ));
                    lines.push(Line::styled(
                        format!(
                            "{} | {} | ref {}",
                            display_safe(&failure.code),
                            retry_label(model, attempt_id, failure.retry),
                            diagnostic_reference(attempt_id)
                        ),
                        visual_style(model, VisualRole::Muted),
                    ));
                }
                if let Some(usage) = usage {
                    lines.push(Line::styled(
                        format!(
                            "{} input tokens · {} output tokens",
                            usage.input_tokens, usage.output_tokens
                        ),
                        visual_style(model, VisualRole::Muted),
                    ));
                }
            }
        }
    }
    transparent_chat_text(Text::from(lines))
}

fn push_safe_lines(lines: &mut Vec<Line<'static>>, text: &str, style: Style) {
    let safe = display_safe(text);
    for line in safe.split('\n') {
        lines.push(Line::styled(line.to_owned(), style));
    }
}

fn render_notice(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let Some(notice) = &model.notice else {
        return;
    };
    let (label, style) = match notice {
        Notice::Info(message) => (
            display_safe(message),
            chat_visual_style(model, VisualRole::Warning),
        ),
        Notice::Failure(failure) => (
            format!(
                "Error [{}]: {}",
                display_safe(&failure.code),
                display_safe(&failure.message)
            ),
            chat_visual_style(model, VisualRole::Error),
        ),
    };
    frame.render_widget(
        Paragraph::new(label)
            .block(app_block(model).borders(Borders::TOP).border_style(style))
            .style(style),
        area,
    );
}

fn render_picker(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Models ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let search_height = 1.min(inner.height);
    let help_height = u16::from(inner.height >= 4);
    let stale_height = u16::from(
        matches!(
            &*model.catalog,
            CatalogProjection::Ready { stale: true, .. }
        ) && inner.height >= 3,
    );
    let list_height = inner
        .height
        .saturating_sub(search_height + stale_height + help_height);
    let search = Rect::new(inner.x, inner.y, inner.width, search_height);
    let list = Rect::new(inner.x, inner.y + search_height, inner.width, list_height);
    let stale_area = Rect::new(
        inner.x,
        inner.y + search_height + list_height,
        inner.width,
        stale_height,
    );
    let help = Rect::new(
        inner.x,
        inner.y + search_height + list_height + stale_height,
        inner.width,
        help_height,
    );
    frame.render_widget(
        Paragraph::new(format!("Filter: {}", display_safe(&model.picker.query)))
            .style(visual_style(model, VisualRole::Field)),
        search,
    );

    match &*model.catalog {
        CatalogProjection::CredentialRequired => {
            frame.render_widget(
                Paragraph::new("A provider API key is required. Press Ctrl+K to connect.")
                    .style(visual_style(model, VisualRole::Muted))
                    .wrap(Wrap { trim: false }),
                list,
            );
        }
        CatalogProjection::Loading => {
            frame.render_widget(
                Paragraph::new("Loading models...").style(visual_style(model, VisualRole::Muted)),
                list,
            );
        }
        CatalogProjection::Failed(failure) => {
            let refresh = if model.catalog_retry_available(failure.retry) {
                "Press Ctrl+R to refresh models.".to_owned()
            } else {
                model.catalog_retry_remaining_ms(failure.retry).map_or_else(
                    || "Catalog refresh is unavailable for this failure.".to_owned(),
                    |remaining| {
                        format!(
                            "Catalog refresh available in {}.",
                            retry_countdown(remaining)
                        )
                    },
                )
            };
            frame.render_widget(
                Paragraph::new(format!(
                    "Model discovery failed: {}\n{}",
                    display_safe(&failure.message),
                    refresh
                ))
                .style(visual_style(model, VisualRole::Error))
                .wrap(Wrap { trim: false }),
                list,
            );
        }
        CatalogProjection::Ready { stale, .. } => {
            render_picker_models(frame, list, model);
            if *stale && stale_height > 0 {
                frame.render_widget(
                    Paragraph::new("stale catalog - Ctrl+R refresh")
                        .style(visual_style(model, VisualRole::Warning)),
                    stale_area,
                );
            }
        }
    }
    if help.height > 0 {
        frame.render_widget(
            Paragraph::new(format!(
                "{} choose  Enter select  D default  Esc close",
                navigation_keys(model)
            ))
            .style(visual_style(model, VisualRole::Muted)),
            help,
        );
    }
}

fn render_credential(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = credential_rect(area);
    frame.render_widget(Clear, popup);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Provider API key ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mask = if model.credential.has_value() {
        if presentation(model).ascii {
            "************"
        } else {
            "••••••••••••"
        }
    } else {
        "paste or type key"
    };
    let compact = inner.height < 8 || inner.width < 36;
    let text = if compact {
        Text::from(vec![
            Line::from("API key required"),
            Line::styled(format!(" {mask} "), visual_style(model, VisualRole::Field)),
        ])
    } else {
        Text::from(vec![
            Line::from("Paste your provider API key below."),
            Line::from("It is kept only in memory for this run and is never saved."),
            Line::from(""),
            Line::styled(
                format!("  {mask}  "),
                visual_style(model, VisualRole::Field),
            ),
        ])
    };
    let content_height = inner.height.saturating_sub(1);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), content);
    let actions = Rect::new(inner.x, inner.y + content_height, inner.width, 1);
    frame.render_widget(
        Paragraph::new("[ Connect ] Enter    [ Cancel ] Esc")
            .style(visual_style(model, VisualRole::Assistant)),
        actions,
    );
}

fn render_picker_models(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let models = filtered_models(model);
    if models.is_empty() {
        frame.render_widget(
            Paragraph::new("No models match this filter.")
                .style(visual_style(model, VisualRole::Muted)),
            area,
        );
        return;
    }

    let selected_index = model
        .picker
        .selected
        .as_ref()
        .and_then(|selected| models.iter().position(|summary| &summary.model == selected))
        .unwrap_or(0);
    let visible = usize::from(area.height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(models.len().saturating_sub(visible));
    let items = models
        .iter()
        .skip(start)
        .take(visible)
        .map(|summary| picker_item(summary, model))
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), area);
}

fn picker_item(summary: &ModelSummary, model: &Model) -> ListItem<'static> {
    let selected = model
        .picker
        .selected
        .as_ref()
        .is_some_and(|candidate| candidate == &summary.model);
    let prefix = if selected {
        selection_marker(model)
    } else {
        " "
    };
    let suffix = if summary.detail.is_empty() {
        String::new()
    } else {
        format!("  {}", display_safe(&summary.detail))
    };
    let label = format!("{prefix} {}{suffix}", display_safe(&summary.display_name));
    let style = if !summary.selectable {
        visual_style(model, VisualRole::Muted)
    } else if selected {
        visual_style(model, VisualRole::Selected)
    } else {
        visual_style(model, VisualRole::Normal)
    };
    ListItem::new(Line::styled(label, style))
}

fn confirmation_rect(area: Rect) -> Rect {
    if area.width <= 40 || area.height <= 12 {
        return area;
    }
    let width = area.width.saturating_sub(4).clamp(1, 72);
    let height = area.height.saturating_sub(2).clamp(1, 9);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn filtered_models(model: &Model) -> Vec<&ModelSummary> {
    let query = model.picker.query.to_lowercase();
    model
        .catalog
        .models()
        .iter()
        .filter(|summary| {
            query.is_empty()
                || summary.display_name.to_lowercase().contains(&query)
                || summary
                    .model
                    .model_id()
                    .as_str()
                    .to_lowercase()
                    .contains(&query)
                || summary
                    .model
                    .provider_id()
                    .as_str()
                    .to_lowercase()
                    .contains(&query)
        })
        .collect()
}

fn popup_rect(area: Rect) -> Rect {
    if area.width < 30 || area.height < 10 {
        return area;
    }
    let width = area.width.saturating_mul(4) / 5;
    let height = area.height.saturating_mul(3) / 4;
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.max(1),
        height.max(1),
    )
}

fn codex_auth_rect(area: Rect) -> Rect {
    if area.width <= 40 || area.height <= 12 {
        return area;
    }
    let width = area.width.saturating_sub(4).min(72);
    let height = area.height.saturating_sub(2).min(14);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.max(1),
        height.max(1),
    )
}

fn credential_rect(area: Rect) -> Rect {
    if area.width <= 40 || area.height <= 12 {
        return area;
    }
    let width = area.width.saturating_sub(4).min(68);
    let height = area.height.saturating_sub(2).min(11);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.max(1),
        height.max(1),
    )
}

fn user_profile_rect(area: Rect) -> Rect {
    if area.width <= 44 || area.height <= 14 {
        return area;
    }
    let width = area.width.saturating_sub(4).min(72);
    let height = area.height.saturating_sub(4).min(16);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width.max(1),
        height.max(1),
    )
}

fn set_composer_cursor(frame: &mut Frame<'_>, area: Rect, model: &Model, bordered: bool) {
    if model.focus != Focus::Composer
        || model.overlay().is_some()
        || model
            .pending
            .values()
            .any(|pending| matches!(pending, PendingKind::SubmitPrompt(_)))
    {
        return;
    }
    let cursor = model.composer.editor.screen_cursor();
    let inset = u16::from(bordered);
    let x = area
        .x
        .saturating_add(inset)
        .saturating_add(u16::try_from(cursor.col).unwrap_or(u16::MAX));
    let y = area
        .y
        .saturating_add(inset)
        .saturating_add(u16::try_from(cursor.row).unwrap_or(u16::MAX));
    if x < area.right() && y < area.bottom() {
        frame.set_cursor_position((x, y));
    }
}

fn set_palette_cursor(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if !model.palette_open() || area.width == 0 || area.height == 0 {
        return;
    }
    let x = area.x.saturating_add(1).saturating_add(
        u16::try_from(display_safe(&model.palette.query).chars().count()).unwrap_or(u16::MAX),
    );
    if x < area.right() {
        frame.set_cursor_position((x, area.y));
    }
}

fn spinner(model: &Model) -> &'static str {
    if presentation(model).reduced_motion {
        return "-";
    }
    if presentation(model).ascii {
        const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
        return FRAMES[usize::try_from((model.now / 100) % 4).unwrap_or(0)];
    }
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    FRAMES[usize::try_from((model.now / 100) % 8).unwrap_or(0)]
}

fn generation_animation(model: &Model) -> &'static str {
    if presentation(model).reduced_motion {
        return "[--------]";
    }
    const FRAMES: [&str; 16] = [
        "[>-------]",
        "[=>------]",
        "[==>-----]",
        "[===>----]",
        "[-===>---]",
        "[--===>--]",
        "[---===>-]",
        "[----===>]",
        "[----<===]",
        "[---<===-]",
        "[--<===--]",
        "[-<===---]",
        "[<===----]",
        "[<==-----]",
        "[<=------]",
        "[<-------]",
    ];
    FRAMES[usize::try_from((model.now / 100) % 16).unwrap_or(0)]
}

fn retry_label(model: &Model, attempt_id: &crate::model::AttemptKey, retry: RetryPolicy) -> String {
    match retry {
        RetryPolicy::Never => "Ctrl+N new".to_owned(),
        RetryPolicy::Now => "Ctrl+R retry | Ctrl+N new".to_owned(),
        RetryPolicy::After { .. } | RetryPolicy::At(_)
            if model.retry_available(attempt_id, retry) =>
        {
            "Ctrl+R retry | Ctrl+N new".to_owned()
        }
        RetryPolicy::After { .. } | RetryPolicy::At(_) => {
            model.retry_remaining_ms(attempt_id, retry).map_or_else(
                || "retry pending | Ctrl+N new".to_owned(),
                |remaining_ms| format!("retry in {} | Ctrl+N new", retry_countdown(remaining_ms)),
            )
        }
    }
}

fn diagnostic_reference(attempt_id: &crate::model::AttemptKey) -> String {
    let value = attempt_id.as_str();
    let characters = value.chars().count();
    if characters <= 16 {
        return display_safe(value);
    }
    value
        .chars()
        .skip(characters.saturating_sub(12))
        .collect::<String>()
}

fn retry_countdown(remaining_ms: u64) -> String {
    let seconds = remaining_ms.saturating_add(999) / 1_000;
    format!("{seconds}s")
}
