use std::fmt::Write as _;

use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, Layout as PreferenceLayout,
    TerminalTimestampStyle, ThemePreset,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    AttemptStatus, COMMANDS, CatalogProjection, Focus, Model, ModelSummary, MouseAction, Notice,
    OverlayKind, PendingKind, ProfileConnectionState, ProfileCredentialAction, ProfileEditorMode,
    ProviderKindLabel, ProviderProfileProjection, RetryPolicy, Route, SettingsPreference,
    TranscriptItem,
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
    Success,
    Warning,
    Field,
}

#[derive(Clone, Copy)]
struct Presentation {
    color_mode: ColorMode,
    theme: ThemePreset,
    ascii: bool,
    reduced_motion: bool,
    compact: bool,
    single_column: bool,
}

fn presentation(model: &Model) -> Presentation {
    let preferences = model.settings().local_profile.preferences();
    Presentation {
        color_mode: *preferences.color_mode().value(),
        theme: *preferences.theme_preset().value(),
        ascii: *preferences.glyph_mode().value() == GlyphMode::Ascii,
        reduced_motion: *preferences.reduced_motion().value(),
        compact: *preferences.density().value() == Density::Compact,
        single_column: *preferences.layout().value() == PreferenceLayout::SingleColumn,
    }
}

fn visual_style(model: &Model, role: VisualRole) -> Style {
    let presentation = presentation(model);
    match presentation.color_mode {
        ColorMode::Color => match presentation.theme {
            ThemePreset::System => match role {
                VisualRole::Normal => Style::default(),
                VisualRole::Header => Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Muted | VisualRole::Border => Style::new().fg(Color::DarkGray),
                VisualRole::User => Style::new()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Assistant => Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Error => Style::new()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Tool | VisualRole::Warning => Style::new().fg(Color::Yellow),
                VisualRole::Selected => Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Success => Style::new().fg(Color::LightGreen),
                VisualRole::Field => Style::new().fg(Color::White).bg(Color::DarkGray),
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
                VisualRole::Success => Style::new().fg(Color::Green).bg(Color::White),
                VisualRole::Field => Style::new().fg(Color::Black).bg(Color::Gray),
            },
            ThemePreset::Dark => match role {
                VisualRole::Normal => Style::new().fg(Color::White).bg(Color::Black),
                VisualRole::Header | VisualRole::Selected => Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Muted | VisualRole::Border => {
                    Style::new().fg(Color::Gray).bg(Color::Black)
                }
                VisualRole::User => Style::new()
                    .fg(Color::LightBlue)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Assistant => Style::new()
                    .fg(Color::LightCyan)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Error => Style::new()
                    .fg(Color::LightRed)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
                VisualRole::Tool | VisualRole::Warning => {
                    Style::new().fg(Color::LightYellow).bg(Color::Black)
                }
                VisualRole::Success => Style::new().fg(Color::LightGreen).bg(Color::Black),
                VisualRole::Field => Style::new().fg(Color::White).bg(Color::DarkGray),
            },
        },
        ColorMode::NoColor => match role {
            VisualRole::Header | VisualRole::Selected | VisualRole::Field => {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            }
            VisualRole::Muted => Style::default().add_modifier(Modifier::DIM),
            VisualRole::User
            | VisualRole::Assistant
            | VisualRole::Tool
            | VisualRole::Success
            | VisualRole::Warning => Style::default().add_modifier(Modifier::BOLD),
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
            VisualRole::Success => Style::default()
                .fg(Color::LightGreen)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
            VisualRole::Warning => Style::default()
                .fg(Color::LightYellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        },
    }
}

fn app_block(model: &Model) -> Block<'static> {
    let block = Block::default();
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

/// Renders the complete terminal client from local state only.
pub fn view(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let content = render_shell(frame, area, model);
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

    match model.overlay() {
        Some(OverlayKind::Permission) => render_permission(frame, area, model),
        Some(OverlayKind::CommandPalette) => render_palette(frame, area, model),
        Some(OverlayKind::SessionCredential) => render_credential(frame, area, model),
        Some(OverlayKind::ModelPicker) => render_picker(frame, area, model),
        Some(OverlayKind::Confirmation) => render_confirmation(frame, area, model),
        Some(OverlayKind::UserProfile) => render_user_profile(frame, area, model),
        Some(OverlayKind::TranscriptSearch | OverlayKind::ProfileCredential) | None => {}
    }
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
    let area = Rect::new(0, 0, width, height);
    if model.overlay() == Some(OverlayKind::UserProfile) {
        let popup = user_profile_rect(area);
        if row == popup.bottom().saturating_sub(2) {
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
    if model.route() == Route::Profiles
        && (model.profile_center.editor.is_some() || model.profile_center.credential.is_some())
    {
        return None;
    }
    if model.overlay().is_some() {
        return None;
    }

    let wide = !presentation(model).single_column && width >= 100 && height >= 16;
    let content_x = if wide { 28 } else { 0 };
    if wide && column < 28 {
        if row == 1 {
            return Some(MouseAction::OpenUserProfile);
        }
        let route_start = 4 + u16::from(active_session_title(model).is_some()) * 3;
        if row >= route_start && row < route_start + 5 {
            return Some(MouseAction::Route(
                Route::ALL[usize::from(row - route_start)],
            ));
        }
        return None;
    }
    if !wide && row == 0 {
        return route_at_column(width, column).map(MouseAction::Route);
    }

    let relative_column = column.saturating_sub(content_x);
    match model.route() {
        Route::Chat if row == height.saturating_sub(1) && height >= 7 => {
            if relative_column < 12 {
                Some(MouseAction::ChatSend)
            } else if relative_column < 30 {
                Some(MouseAction::ChatModels)
            } else if relative_column < 45 {
                Some(MouseAction::ChatNewSession)
            } else if relative_column < 62 {
                Some(MouseAction::ChatSessions)
            } else if relative_column < 82 {
                Some(MouseAction::ChatCredential)
            } else {
                Some(MouseAction::ChatHelp)
            }
        }
        Route::Sessions if row == height.saturating_sub(2) => {
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
        Route::Profiles if row == 1 => Some(MouseAction::OpenUserProfile),
        Route::Profiles
            if profile_detail_button_rows(model, width, height)
                .is_some_and(|(first, _)| row == first) =>
        {
            profile_action_at_column(relative_column)
        }
        Route::Profiles
            if profile_detail_button_rows(model, width, height)
                .is_some_and(|(_, second)| row == second) =>
        {
            profile_secondary_action_at_column(relative_column)
        }
        Route::Profiles if row == height.saturating_sub(2) => {
            profile_action_at_column(relative_column)
        }
        Route::Profiles => profile_at_row(model, width, height, column, row),
        _ => None,
    }
}

fn profile_action_at_column(column: u16) -> Option<MouseAction> {
    match column {
        0..=6 => Some(MouseAction::ProfileNew),
        8..=14 => Some(MouseAction::ProfileCredential),
        16..=22 => Some(MouseAction::ProfileTest),
        24..=34 => Some(MouseAction::ProfileDefaultModel),
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
fn profile_detail_button_rows(model: &Model, width: u16, height: u16) -> Option<(u16, u16)> {
    let selected = model.selected_profile()?;
    let outer = Rect::new(0, 0, width, height);
    let inner = Rect::new(
        outer.x.saturating_add(1),
        outer.y.saturating_add(1),
        outer.width.saturating_sub(2),
        outer.height.saturating_sub(2),
    );
    let compact = presentation(model).compact;
    let user_height = if compact {
        2
    } else if inner.height >= 12 {
        4
    } else {
        2
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(user_height),
            Constraint::Min(1),
            Constraint::Length(0),
            Constraint::Length(1),
        ])
        .split(inner);
    let detail = if !presentation(model).single_column && rows[1].width >= 78 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(rows[1])[1]
    } else if rows[1].height >= 9 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[1])[1]
    } else {
        return None;
    };
    let mut lines = 8_u16;
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

fn route_at_column(width: u16, column: u16) -> Option<Route> {
    let mut offset = 0_u16;
    for (index, route) in Route::ALL.into_iter().enumerate() {
        let segment = if width >= 72 {
            u16::try_from(route.label().len() + 5).unwrap_or(u16::MAX)
        } else if width >= 48 {
            u16::try_from(route.label().len() + 4).unwrap_or(u16::MAX)
        } else {
            return Some(route);
        };
        if column < offset.saturating_add(segment) {
            return Some(Route::ALL[index]);
        }
        offset = offset.saturating_add(segment);
    }
    None
}

fn profile_at_row(
    model: &Model,
    width: u16,
    height: u16,
    column: u16,
    row: u16,
) -> Option<MouseAction> {
    let content_x = if !presentation(model).single_column && width >= 100 && height >= 16 {
        28
    } else {
        0
    };
    if column < content_x {
        return None;
    }
    let list_start = if width >= 78 && height >= 7 { 5 } else { 3 };
    let index = usize::from(row.saturating_sub(list_start));
    model
        .filtered_profiles()
        .nth(index)
        .map(|profile| MouseAction::SelectProfile(profile.id.clone()))
}

fn render_shell(frame: &mut Frame<'_>, area: Rect, model: &Model) -> Rect {
    if !presentation(model).single_column && area.width >= 100 && area.height >= 16 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(1)])
            .split(area);
        render_navigation_rail(frame, columns[0], model);
        columns[1]
    } else {
        let navigation_height = if area.height >= 3 { 2 } else { 1 };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(navigation_height), Constraint::Min(0)])
            .split(area);
        render_compact_navigation(frame, rows[0], model);
        rows[1]
    }
}

fn render_navigation_rail(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let block = app_block(model)
        .borders(Borders::RIGHT)
        .title(" AutoHarness ")
        .title_style(visual_style(model, VisualRole::Header))
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let user = &model.profiles().user;
    let local_label = user.display_label.as_deref().unwrap_or("Local user");
    let mut lines = vec![
        Line::styled(
            display_safe(local_label),
            visual_style(model, VisualRole::User),
        ),
        Line::styled(
            workspace_label(&user.workspace),
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
    ];
    if let Some(title) = active_session_title(model) {
        lines.push(Line::styled(
            "SESSION",
            visual_style(model, VisualRole::Muted),
        ));
        lines.push(Line::styled(
            display_safe(&title),
            visual_style(model, VisualRole::Normal),
        ));
        lines.push(Line::from(""));
    }

    for (index, route) in Route::ALL.into_iter().enumerate() {
        let label = format!(" {}  {:<10}", index + 1, route.label());
        let style = if route == model.route() {
            visual_style(model, VisualRole::Selected)
        } else {
            visual_style(model, VisualRole::Normal)
        };
        lines.push(Line::styled(label, style));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "CONNECTION",
        visual_style(model, VisualRole::Muted),
    ));
    lines.push(Line::from(display_safe(&model.settings_provider_label())));
    lines.push(Line::from(display_safe(&header_credential_label(model))));
    lines.push(Line::from(display_safe(&selected_model_label(model))));
    let state = attempt_state_label(model);
    let state_style = if state == "ready" {
        visual_style(model, VisualRole::Success)
    } else if state == "failed" || state == "cancelled" {
        visual_style(model, VisualRole::Error)
    } else {
        visual_style(model, VisualRole::Warning)
    };
    lines.push(Line::styled(state, state_style));
    let usage = session_usage(model);
    if !usage.is_empty() {
        lines.push(Line::styled(usage, visual_style(model, VisualRole::Muted)));
    }
    if let Some(catalog) = catalog_status_label(model) {
        lines.push(Line::styled(
            catalog,
            visual_style(model, VisualRole::Warning),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Ctrl+/ commands",
        visual_style(model, VisualRole::Muted),
    ));
    lines.push(Line::styled(
        "F1 contextual help",
        visual_style(model, VisualRole::Muted),
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn active_session_title(model: &Model) -> Option<String> {
    model
        .sessions
        .sessions
        .iter()
        .find(|entry| entry.active || entry.session_id == model.session.session_id)
        .map(|entry| display_safe(&entry.title))
}

fn conversation_title(model: &Model) -> String {
    active_session_title(model).map_or_else(
        || " Conversation ".to_owned(),
        |title| format!(" Conversation{}{} ", chrome_separator(model), title),
    )
}

fn onboarding_step(model: &Model) -> (&'static str, &'static str) {
    if model.session.selected_model.is_none() {
        ("NEXT", "Ctrl+P choose a model")
    } else if model.settings().provider_status.credential_connected {
        ("READY", "Write a prompt below")
    } else {
        ("NEXT", "Ctrl+K connect a session-only key")
    }
}

fn render_onboarding(lines: &mut Vec<Line<'static>>, model: &Model) {
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "GET STARTED",
        visual_style(model, VisualRole::User),
    ));
    lines.push(Line::from("1  Connect a provider in Alt+3 or Ctrl+K"));
    lines.push(Line::from("2  Choose a compatible model with Ctrl+P"));
    let (label, action) = onboarding_step(model);
    lines.push(Line::styled(
        format!("3  {label} · {action}"),
        visual_style(model, VisualRole::Assistant),
    ));
}

fn render_compact_navigation(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.height == 0 {
        return;
    }
    let route_line = if area.width >= 72 {
        let spans = Route::ALL
            .into_iter()
            .enumerate()
            .flat_map(|(index, route)| {
                let style = if route == model.route() {
                    visual_style(model, VisualRole::Selected)
                } else {
                    visual_style(model, VisualRole::Muted)
                };
                [
                    Span::styled(format!(" {} {} ", index + 1, route.label()), style),
                    Span::raw(" "),
                ]
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    } else if area.width >= 48 {
        Line::from(
            Route::ALL
                .into_iter()
                .enumerate()
                .map(|(index, route)| {
                    let style = if route == model.route() {
                        visual_style(model, VisualRole::Selected)
                    } else {
                        visual_style(model, VisualRole::Muted)
                    };
                    Span::styled(format!(" {}{} ", index + 1, route.label()), style)
                })
                .collect::<Vec<_>>(),
        )
    } else {
        Line::from(vec![
            Span::styled(
                format!(
                    " {} {} ",
                    route_number(model.route()),
                    model.route().label()
                ),
                visual_style(model, VisualRole::Selected),
            ),
            Span::raw("  Alt+1..5 routes"),
        ])
    };
    frame.render_widget(Paragraph::new(route_line), area);
    if area.height >= 2 {
        let status = Rect::new(area.x, area.y + 1, area.width, 1);
        render_header(frame, status, model);
    }
}

fn route_number(route: Route) -> usize {
    Route::ALL
        .iter()
        .position(|candidate| *candidate == route)
        .map_or(1, |index| index + 1)
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
        detail_line(model, "Mode", default_mode),
        Line::from(""),
        Line::from(vec![
            Span::styled("[ Save ]", visual_style(model, VisualRole::Selected)),
            Span::raw("  "),
            Span::styled("[ Cancel ]", visual_style(model, VisualRole::Field)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn workspace_label(workspace: &str) -> String {
    workspace
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map_or_else(|| "workspace".to_owned(), display_safe)
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
        "{prefix} {}  /{} - {}",
        display_safe(entry.label),
        entry.id,
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

    let status = &model.settings().provider_status;
    let active_profile = status.active_profile.as_deref().unwrap_or("none");
    let connection = if status.credential_connected {
        "connected"
    } else if status.active_profile.is_some() {
        "disconnected"
    } else {
        "session only"
    };
    let connection_style = if status.credential_connected {
        visual_style(model, VisualRole::Success)
    } else {
        visual_style(model, VisualRole::Warning)
    };
    let mut lines = vec![
        Line::styled(
            "LOCAL PROFILE DEFAULTS",
            visual_style(model, VisualRole::User),
        ),
        settings_preference_line(model, SettingsPreference::DisplayLabel),
        Line::from(""),
        Line::styled("PROVIDERS", visual_style(model, VisualRole::User)),
        detail_line(model, "Provider", &model.settings().provider_label()),
        detail_line(model, "Profile", active_profile),
        Line::from(vec![
            Span::styled("Credential  ", visual_style(model, VisualRole::Muted)),
            Span::styled(connection, connection_style),
        ]),
        detail_line(model, "Source", status.credential_source.as_str()),
        Line::styled(
            "Read-only here: use P or Alt+3 to manage profiles and credentials.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("MODEL & MODE", visual_style(model, VisualRole::User)),
        detail_line(model, "Model", &selected_model_label(model)),
        detail_line(model, "Mode", "safe agent"),
        Line::styled(
            "Read-only here: choose models from Ctrl+P; mode is application policy.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("APPROVALS", visual_style(model, VisualRole::User)),
        Line::styled(
            "Read-only policy: every external capability requires an exact per-call decision.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::styled("RETENTION", visual_style(model, VisualRole::User)),
        Line::styled(
            "Read-only policy: durable sessions retain provider-neutral history; deletion confirms.",
            visual_style(model, VisualRole::Muted),
        ),
        Line::from(""),
        Line::styled("APPEARANCE", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::ThemePreset),
        settings_preference_line(model, SettingsPreference::ColorMode),
        settings_preference_line(model, SettingsPreference::GlyphMode),
        Line::styled("ACCESSIBILITY", visual_style(model, VisualRole::User)),
        settings_preference_line(model, SettingsPreference::ReducedMotion),
        settings_preference_line(model, SettingsPreference::Density),
        Line::styled("LOGGING", visual_style(model, VisualRole::User)),
        Line::styled(
            "Read-only policy: credentials and model content are excluded from settings diagnostics.",
            visual_style(model, VisualRole::Muted),
        ),
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
    let hint_height = u16::from(inner.height >= 2);
    let content_height = inner.height.saturating_sub(hint_height);
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
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
    if hint_height > 0 {
        let hint = Rect::new(inner.x, inner.y + content_height, inner.width, hint_height);
        frame.render_widget(
            Paragraph::new(format!(
                "{} select/PgUp/PgDn  Left/Right change  Enter edit label  R inherit  D user default  Esc chat",
                navigation_keys(model)
            ))
            .style(visual_style(model, VisualRole::Muted)),
            hint,
        );
    }
}

fn settings_preference_line(model: &Model, preference: SettingsPreference) -> Line<'static> {
    let profile = &model.settings().local_profile;
    let (label, value, source, explanation) = match preference {
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
    let selected = SettingsPreference::at(model.settings_workspace.selected) == preference;
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
    Line::styled(
        format!("{marker} {label:<18} {value}  Source: {source}  {explanation}{suffix}"),
        style,
    )
}

fn settings_preference_label(preference: SettingsPreference) -> &'static str {
    match preference {
        SettingsPreference::DisplayLabel => "Display label",
        SettingsPreference::ThemePreset => "Theme preset",
        SettingsPreference::ColorMode => "Color mode",
        SettingsPreference::GlyphMode => "Glyph mode",
        SettingsPreference::ReducedMotion => "Reduced motion",
        SettingsPreference::Density => "Density",
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
    }
}

fn color_mode_label(value: ColorMode) -> &'static str {
    match value {
        ColorMode::Color => "color",
        ColorMode::NoColor => "no color",
        ColorMode::HighContrast => "high contrast",
    }
}

fn glyph_mode_label(value: GlyphMode) -> &'static str {
    match value {
        GlyphMode::Unicode => "unicode",
        GlyphMode::Ascii => "ASCII",
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
/// Renders the full-screen local profile and provider connection center.
fn render_profile_center(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    frame.render_widget(Clear, area);
    let outer = app_block(model)
        .borders(Borders::ALL)
        .title(" Profiles & Providers ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let compact = presentation(model).compact;
    let notice_height = if model.notice.is_some() && inner.height >= 8 {
        if compact { 1 } else { 2 }
    } else {
        0
    };
    let user_height = if compact {
        2
    } else if inner.height >= 12 {
        4
    } else {
        2
    };
    let help_height = u16::from(inner.height >= 4);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(user_height),
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(help_height),
        ])
        .split(inner);
    render_local_profile(frame, rows[0], model);

    if !presentation(model).single_column && rows[1].width >= 78 && rows[1].height >= 7 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(rows[1]);
        render_profile_list(frame, columns[0], model);
        render_profile_detail(frame, columns[1], model);
    } else if rows[1].height >= 9 {
        let panes = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[1]);
        render_profile_list(frame, panes[0], model);
        render_profile_detail(frame, panes[1], model);
    } else {
        render_profile_list(frame, rows[1], model);
    }

    if notice_height > 0 {
        render_notice(frame, rows[2], model);
    }
    if help_height > 0 {
        let hints = if model.profile_center.confirming_disconnect.is_some() {
            "Y disconnect credential  N/Esc cancel"
        } else if model.profile_center.confirming_delete.is_some() {
            "Y delete profile and credential  N/Esc cancel"
        } else if rows[3].width >= 100 {
            if presentation(model).ascii {
                "Up/Down choose Enter active Alt+N new Alt+E edit Alt+K key Alt+T test Alt+M default Esc"
            } else {
                "↑/↓ choose Enter active Alt+N new Alt+E edit Alt+K key Alt+T test Alt+M default Esc"
            }
        } else if rows[3].width >= 70 {
            if presentation(model).ascii {
                "Up/Down choose  Enter active  Alt+N new  Alt+K key  Alt+T test  Esc"
            } else {
                "↑/↓ choose  Enter active  Alt+N new  Alt+K key  Alt+T test  Esc"
            }
        } else if rows[3].width >= 50 {
            if presentation(model).ascii {
                "Up/Down choose  Enter active  Alt+N new  Alt+K key  Esc"
            } else {
                "↑/↓ choose  Enter active  Alt+N new  Alt+K key  Esc"
            }
        } else if presentation(model).ascii {
            "Up/Down  Enter active  Alt+N new  Esc"
        } else {
            "↑/↓  Enter active  Alt+N new  Esc"
        };
        frame.render_widget(
            Paragraph::new(hints).style(visual_style(model, VisualRole::Muted)),
            rows[3],
        );
    }

    if model.profile_center.editor.is_some() {
        render_profile_editor(frame, area, model);
    } else if model.profile_center.credential.is_some() {
        render_profile_credential(frame, area, model);
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
        Span::raw("  "),
        Span::styled("Default ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(default_profile)),
        Span::styled("  Model ", visual_style(model, VisualRole::Muted)),
        Span::raw(display_safe(default_model)),
        Span::styled("  Mode ", visual_style(model, VisualRole::Muted)),
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

fn render_profile_list(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let title = format!(
        " Provider profiles - filter: {} ",
        display_safe(&model.profile_center.query)
    );
    let block = app_block(model).borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let profiles = model.filtered_profiles().collect::<Vec<_>>();
    if profiles.is_empty() {
        let empty = if model.profiles().profiles.is_empty() {
            "No provider profiles yet.\nPress Alt+N to create Gemini or router access."
        } else {
            "No profiles match this filter."
        };
        frame.render_widget(
            Paragraph::new(empty)
                .style(visual_style(model, VisualRole::Muted))
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    let selected_index = model
        .profile_selection()
        .and_then(|selected| profiles.iter().position(|profile| profile.id == selected))
        .unwrap_or(0);
    let visible = usize::from(inner.height);
    let start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(profiles.len().saturating_sub(visible));
    let items = profiles
        .iter()
        .skip(start)
        .take(visible)
        .map(|profile| profile_list_item(profile, model, inner.width))
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), inner);
}

fn profile_list_item(
    profile: &ProviderProfileProjection,
    model: &Model,
    width: u16,
) -> ListItem<'static> {
    let selected = model.profile_selection() == Some(profile.id.as_str());
    let marker = if selected {
        selection_marker(model)
    } else {
        " "
    };
    let active = if profile.active { "*" } else { " " };
    let style = if selected {
        visual_style(model, VisualRole::Selected)
    } else if profile.active {
        visual_style(model, VisualRole::Assistant)
    } else {
        visual_style(model, VisualRole::Normal)
    };
    let id = display_safe(&profile.id);
    let default_model = profile
        .default_model
        .as_deref()
        .map(display_safe)
        .unwrap_or_else(|| "no default".to_owned());
    let label = if width >= 56 {
        format!(
            "{marker}{active} {id}  {}  {}  {}  {}",
            profile.kind.as_str(),
            profile.credential_state.as_str(),
            profile.connection.label(),
            default_model,
        )
    } else if width >= 44 {
        format!(
            "{marker}{active} {id}  {}  {}  {}",
            profile.kind.as_str(),
            profile.credential_state.as_str(),
            profile.connection.label(),
        )
    } else if width >= 32 {
        format!(
            "{marker}{active} {id}  {}  {}",
            profile.kind.as_str(),
            profile.credential_state.as_str(),
        )
    } else {
        format!("{marker}{active} {id}  {}", profile.kind.as_str())
    };
    ListItem::new(Line::from(label)).style(style)
}

fn render_profile_detail(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let block = app_block(model).borders(Borders::ALL).title(" Connection ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let Some(profile) = model.selected_profile() else {
        frame.render_widget(
            Paragraph::new("Select or create a profile.")
                .style(visual_style(model, VisualRole::Muted)),
            inner,
        );
        return;
    };
    let default_model = profile.default_model.as_deref().unwrap_or("not set");
    let default_mode = if profile.default_mode.is_empty() {
        "safe agent"
    } else {
        profile.default_mode.as_str()
    };
    let mut lines = vec![
        detail_line(model, "Name", &profile.id),
        detail_line(model, "Provider", profile.kind.as_str()),
    ];
    if profile.kind == ProviderKindLabel::Router {
        lines.push(detail_line(model, "Base URL", &profile.base_url));
        if !profile.project.is_empty() {
            lines.push(detail_line(model, "Project", &profile.project));
        }
        if !profile.auth_header.is_empty() {
            lines.push(detail_line(model, "Auth header", &profile.auth_header));
        }
    }
    lines.extend([
        detail_line(model, "Active", if profile.active { "yes" } else { "no" }),
        detail_line(model, "Credential", profile.credential_state.as_str()),
        detail_line(model, "Source", profile.credential_source.as_str()),
        detail_line(model, "Connection", profile.connection.label()),
        detail_line(model, "Default model", default_model),
        detail_line(model, "Mode", default_mode),
    ]);
    if let ProfileConnectionState::Failed(reason) = &profile.connection {
        lines.push(Line::from(vec![
            Span::styled("Reason      ", visual_style(model, VisualRole::Error)),
            Span::raw(display_safe(reason)),
        ]));
    }
    if model.profiles().pending_recovery > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} credential repair operation(s) pending",
                model.profiles().pending_recovery
            ),
            visual_style(model, VisualRole::Warning),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[ New ]", visual_style(model, VisualRole::Field)),
        Span::raw(" "),
        Span::styled("[ Key ]", visual_style(model, VisualRole::Field)),
        Span::raw(" "),
        Span::styled("[ Test ]", visual_style(model, VisualRole::Field)),
        Span::raw(" "),
        Span::styled("[ Default ]", visual_style(model, VisualRole::Field)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("[ Disconnect ]", visual_style(model, VisualRole::Warning)),
        Span::raw(" "),
        Span::styled("[ Delete ]", visual_style(model, VisualRole::Error)),
    ]));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
    let title = match editor.mode {
        ProfileEditorMode::Create => " Create provider profile ",
        ProfileEditorMode::Edit => " Edit provider profile ",
        ProfileEditorMode::Duplicate => " Duplicate provider profile ",
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
    lines.push(Line::from(Span::styled(
        "Tab next field  Left/Right provider  Enter save  Esc cancel",
        visual_style(model, VisualRole::Muted),
    )));
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
    let lines = vec![
        Line::from(Span::styled(
            masked,
            visual_style(model, VisualRole::Warning),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Stored only in the operating-system vault. Enter save  Esc cancel",
            visual_style(model, VisualRole::Muted),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
    let help = if pending {
        "Saving answer..."
    } else {
        "Y allow  N/Esc deny"
    };
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
    lines.push(Line::styled(help, visual_style(model, VisualRole::Warning)));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((model.permission_scroll, 0)),
        inner,
    );
}

fn render_standard(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let compact = presentation(model).compact;
    let composer_height = u16::try_from(model.composer.lines().len())
        .unwrap_or(u16::MAX)
        .saturating_add(if compact { 1 } else { 2 })
        .clamp(if compact { 2 } else { 3 }, if compact { 5 } else { 8 });
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
            Constraint::Length(1),
        ])
        .split(area);

    render_transcript(frame, chunks[0], model, true);
    if notice_height > 0 {
        render_notice(frame, chunks[1], model);
    }
    if search_height > 0 {
        render_search_bar(frame, chunks[2], model);
    }
    let mut composer = model.composer.editor.clone();
    composer.set_block(
        app_block(model)
            .borders(Borders::ALL)
            .title(" Prompt ")
            .border_style(visual_style(model, VisualRole::Border)),
    );
    composer.set_cursor_style(visual_style(model, VisualRole::Selected));
    frame.render_widget(&composer, chunks[3]);
    render_footer(frame, chunks[4], model);
    set_composer_cursor(frame, chunks[3], model, true);
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
    let composer_height = area.height.min(2);
    let transcript_height = area.height.saturating_sub(composer_height);
    let transcript = Rect::new(area.x, area.y, area.width, transcript_height);
    let composer = Rect::new(
        area.x,
        area.y + transcript_height,
        area.width,
        composer_height,
    );
    if transcript.height > 0 {
        render_transcript(frame, transcript, model, false);
    }
    if composer.height > 0 {
        let mut editor = model.composer.editor.clone();
        editor.remove_block();
        editor.set_cursor_line_style(visual_style(model, VisualRole::Normal));
        editor.set_cursor_style(visual_style(model, VisualRole::Selected));
        frame.render_widget(&editor, composer);
        set_composer_cursor(frame, composer, model, false);
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

fn attempt_state_label(model: &Model) -> String {
    if let Some((attempt_id, status)) = model.session.active_attempt() {
        if matches!(status, AttemptStatus::Cancelling) || model.cancelling.contains(attempt_id) {
            format!("{} cancelling", spinner(model))
        } else {
            format!("{} streaming", spinner(model))
        }
    } else if let Some((attempt_id, _)) = model.session.retryable_attempt() {
        if model.retry_requested(attempt_id) {
            "retrying".to_owned()
        } else if model.session.failed_attempt().is_some() {
            "failed".to_owned()
        } else {
            "cancelled".to_owned()
        }
    } else {
        "ready".to_owned()
    }
}

fn catalog_status_label(model: &Model) -> Option<String> {
    let label = match &*model.catalog {
        CatalogProjection::CredentialRequired => "offline credential needed",
        CatalogProjection::Loading => "catalog loading",
        CatalogProjection::Ready { stale: true, .. } => "catalog stale",
        CatalogProjection::Failed(_) => "catalog error",
        CatalogProjection::Ready { stale: false, .. } => return None,
    };
    Some(if presentation(model).ascii {
        label.to_owned()
    } else if matches!(&*model.catalog, CatalogProjection::CredentialRequired) {
        "offline · credential needed".to_owned()
    } else {
        label.to_owned()
    })
}

fn render_header(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let selected = selected_model_label(model);

    let state = attempt_state_label(model);

    // Status surface segments degrade left to right: identity and work state
    // survive at every width; provider, credential, catalog, and usage detail
    // appear as space allows. Credential wording never claims a connection
    // that is not effective.
    let provider = model.settings.provider_label();
    let credential = header_credential_label(model);
    let catalog = catalog_status_label(model).unwrap_or_default();
    let usage = session_usage(model);
    let usage_segment = (!usage.is_empty()).then(|| format!(" | {usage}"));
    let usage = usage_segment.as_deref().unwrap_or_default();

    let title = if area.width < 50 {
        format!(" AutoHarness | {state} ")
    } else if area.width < 72 {
        format!(" AutoHarness  |  {selected}  |  {state} ")
    } else {
        let mut title =
            format!(" AutoHarness  |  {provider}  |  {credential}  |  {selected}  |  {state}");
        if !catalog.is_empty() {
            title.push_str(&format!("  |  {catalog}"));
        }
        title.push_str(usage);
        title.push(' ');
        title
    };
    frame.render_widget(
        Paragraph::new(display_safe(&title)).style(visual_style(model, VisualRole::Header)),
        area,
    );
}

/// Safe credential label for the status surface.
///
/// A vault or environment source only displays when a credential is actually
/// connected; otherwise the disconnected state is named explicitly so the
/// status line can never overclaim.
fn header_credential_label(model: &Model) -> String {
    let status = &model.settings.provider_status;
    if status.credential_connected {
        status.credential_source.as_str().to_owned()
    } else if status.active_profile.is_some() {
        // A profile exists but no credential resolved from any source.
        "disconnected".to_owned()
    } else {
        // The documented default: nothing persisted, session-only entry.
        "session only".to_owned()
    }
}

/// Aggregate token usage across completed attempts in the active session.
fn session_usage(model: &Model) -> String {
    let (input, output) = model
        .session
        .transcript
        .iter()
        .fold((0_u64, 0_u64), |acc, item| match item {
            TranscriptItem::Assistant {
                usage: Some(usage), ..
            } => (
                acc.0.saturating_add(usage.input_tokens),
                acc.1.saturating_add(usage.output_tokens),
            ),
            _ => acc,
        });
    if input == 0 && output == 0 {
        return String::new();
    }
    format!("{} tok", input.saturating_add(output))
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, model: &Model, bordered: bool) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let text = transcript_text(model);
    let block = bordered.then(|| {
        app_block(model)
            .borders(Borders::ALL)
            .title(conversation_title(model))
            .border_style(visual_style(model, VisualRole::Border))
    });
    let inner = block.as_ref().map_or(area, |block| block.inner(area));
    if let Some(block) = block {
        frame.render_widget(block, area);
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
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
                    "Alt+3 manage providers or Ctrl+K use a session-only key",
                    visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Loading => {
                lines.push(Line::styled(
                    "CONNECTING",
                    visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::from("Loading compatible models..."));
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
        return Text::from(lines);
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
                            "{}{} streaming",
                            chrome_separator(model),
                            spinner(model)
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
    Text::from(lines)
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
            visual_style(model, VisualRole::Warning),
        ),
        Notice::Failure(failure) => (
            format!(
                "Error [{}]: {}",
                display_safe(&failure.code),
                display_safe(&failure.message)
            ),
            visual_style(model, VisualRole::Error),
        ),
    };
    frame.render_widget(
        Paragraph::new(label)
            .block(app_block(model).borders(Borders::TOP).border_style(style))
            .style(style),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width < 50 {
        render_narrow_footer(frame, area, model);
        return;
    }

    let control_s = *model
        .settings()
        .local_profile
        .preferences()
        .composer_submit_behavior()
        .value()
        == ComposerSubmitBehavior::ControlS;
    let submit_chord = if control_s { " Ctrl+S " } else { " Enter " };
    let newline_chord = if control_s {
        " Enter "
    } else {
        " Ctrl+S/Ctrl+Enter "
    };
    let mut spans = vec![
        Span::styled(submit_chord, visual_style(model, VisualRole::Selected)),
        Span::raw("send  "),
    ];
    if area.width >= 72 {
        spans.push(Span::styled(
            newline_chord,
            visual_style(model, VisualRole::Muted),
        ));
        spans.push(Span::raw("newline  "));
    }
    spans.extend([
        Span::styled(" Ctrl+P ", visual_style(model, VisualRole::Field)),
        Span::raw("models  "),
        Span::styled(" Ctrl+N ", visual_style(model, VisualRole::Field)),
        Span::raw("new"),
    ]);
    if area.width >= 88 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " Ctrl+L ",
            visual_style(model, VisualRole::Field),
        ));
        spans.push(Span::raw("sessions"));
    }
    if area.width >= 100 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " Ctrl+K ",
            visual_style(model, VisualRole::Field),
        ));
        spans.push(Span::raw("API key"));
    }
    if area.width >= 104 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(" F1 ", visual_style(model, VisualRole::Field)));
        spans.push(Span::raw("help"));
    }
    if let Some((attempt_id, status)) = model.session.active_attempt() {
        spans.push(Span::raw("  "));
        if model.cancelling.contains(attempt_id) || matches!(status, AttemptStatus::Cancelling) {
            spans.push(Span::styled(
                "cancelling...",
                visual_style(model, VisualRole::Warning),
            ));
        } else {
            spans.push(Span::styled(
                " Esc ",
                visual_style(model, VisualRole::Warning),
            ));
            spans.push(Span::raw("cancel"));
        }
    } else if let Some((attempt_id, retry)) = model.session.retryable_attempt() {
        spans.push(Span::raw("  "));
        if model.retry_requested(attempt_id) {
            spans.push(Span::styled(
                "retry requested",
                visual_style(model, VisualRole::Warning),
            ));
        } else if model.retry_available(attempt_id, retry) {
            spans.push(Span::styled(
                " Ctrl+R ",
                visual_style(model, VisualRole::Warning),
            ));
            spans.push(Span::raw("retry"));
        } else if let Some(remaining_ms) = model.retry_remaining_ms(attempt_id, retry) {
            spans.push(Span::styled(
                retry_countdown(remaining_ms),
                visual_style(model, VisualRole::Warning),
            ));
        } else {
            spans.push(Span::raw("retry unavailable"));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(visual_style(model, VisualRole::Muted)),
        area,
    );
}

fn render_narrow_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let submit = if *model
        .settings()
        .local_profile
        .preferences()
        .composer_submit_behavior()
        .value()
        == ComposerSubmitBehavior::ControlS
    {
        " ^S "
    } else {
        " Enter "
    };
    let mut spans = vec![
        Span::styled(submit, visual_style(model, VisualRole::Selected)),
        Span::raw("send  "),
        Span::styled(" ^P ", visual_style(model, VisualRole::Field)),
        Span::raw("models  "),
        Span::styled(" ^N ", visual_style(model, VisualRole::Field)),
        Span::raw("new"),
    ];
    if let Some((attempt_id, status)) = model.session.active_attempt() {
        spans.push(Span::raw("  "));
        if model.cancelling.contains(attempt_id) || matches!(status, AttemptStatus::Cancelling) {
            spans.push(Span::styled(
                "cancelling",
                visual_style(model, VisualRole::Warning),
            ));
        } else {
            spans.push(Span::styled(
                " Esc ",
                visual_style(model, VisualRole::Warning),
            ));
            spans.push(Span::raw("cancel"));
        }
    } else if let Some((attempt_id, retry)) = model.session.retryable_attempt() {
        spans.push(Span::raw("  "));
        if model.retry_requested(attempt_id) {
            spans.push(Span::styled(
                "retrying",
                visual_style(model, VisualRole::Warning),
            ));
        } else if model.retry_available(attempt_id, retry) {
            spans.push(Span::styled(
                " ^R ",
                visual_style(model, VisualRole::Warning),
            ));
            spans.push(Span::raw("retry"));
        } else if let Some(remaining_ms) = model.retry_remaining_ms(attempt_id, retry) {
            spans.push(Span::styled(
                retry_countdown(remaining_ms),
                visual_style(model, VisualRole::Warning),
            ));
        } else {
            spans.push(Span::raw("no retry"));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(visual_style(model, VisualRole::Muted)),
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
                "{} choose  Enter select  Esc close",
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
    let text = if inner.height < 8 || inner.width < 36 {
        Text::from(vec![
            Line::from("API key required"),
            Line::styled(format!(" {mask} "), visual_style(model, VisualRole::Field)),
            Line::styled(
                "Enter connect  Esc later",
                visual_style(model, VisualRole::Muted),
            ),
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
            Line::from(""),
            Line::styled(
                "Enter connect  Backspace edit  Esc later",
                visual_style(model, VisualRole::Muted),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
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

fn credential_rect(area: Rect) -> Rect {
    if area.width <= 40 || area.height <= 12 {
        return area;
    }
    let width = area.width.saturating_sub(4).min(68);
    let height = area.height.saturating_sub(2).min(10);
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
