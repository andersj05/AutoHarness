use std::fmt::Write as _;

use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, Layout as PreferenceLayout,
    PromptStatusDetail, TerminalTimestampStyle, ThemePreset,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    AttemptStatus, COMMANDS, CatalogProjection, Focus, Model, ModelDefaultStep, ModelSummary,
    MouseAction, Notice, OverlayKind, PROVIDER_CHOICES, PendingKind, ProfileCenterFocus,
    ProfileConnectionState, ProfileCredentialAction, ProfileEditorMode, ProviderKindLabel,
    RetryPolicy, Route, SettingsPreference, TranscriptItem, UsageView,
};
use crate::text::display_safe;
use crate::ui::Token;
use crate::ui::layout::{self as ui_layout, Layout as UiLayout, Presentation, SETTINGS_NAV};
use crate::ui::metrics::{
    COMPACT_CHAT_MIN_HEIGHT, COMPACT_CHAT_MIN_WIDTH, CREDENTIAL_COMPACT_WIDTH,
    PAGE_HEADER_TALL_MIN, PAGE_HELP_COMFORTABLE, PAGE_HELP_MIN, PROFILE_COMPACT_WIDTH,
    PROFILE_HELP_MEDIUM, PROFILE_HELP_NARROW, PROFILE_HELP_WIDE, ROW, SESSION_HELP_WIDE,
    SETTINGS_NAV_COMPACT_WIDTH, SIDEBAR_LABEL_INSET, SIDEBAR_SESSION_CHROME, STATUS_BRANCH_CHARS,
    STATUS_BRANCH_MIN, STATUS_COMPACT_WIDTH, STATUS_CONTEXT_COMPACT, STATUS_CONTEXT_MIN,
    STATUS_MODEL_CHARS_MID, STATUS_MODEL_CHARS_NARROW, STATUS_MODEL_CHARS_WIDE,
    STATUS_MODEL_WIDE_MIN, STATUS_PATH_CHARS_NARROW, STATUS_PATH_CHARS_WIDE, STATUS_THINKING_MIN,
    STATUS_TOKENS_MIN, STATUS_WORKSPACE_MIN, STATUS_WORKSPACE_WIDE, TWO_ROWS,
};
use crate::ui::normalized_t;

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

fn presentation(model: &Model) -> Presentation {
    ui_layout::presentation(model)
}

fn visual_token(role: VisualRole) -> Token {
    match role {
        VisualRole::Normal => Token::TextPrimary,
        VisualRole::Header => Token::Accent,
        VisualRole::Selected => Token::SurfaceSelected,
        VisualRole::Muted => Token::TextMuted,
        VisualRole::User => Token::RoleUser,
        VisualRole::Assistant => Token::RoleAssistant,
        VisualRole::Error => Token::Danger,
        VisualRole::Tool => Token::RoleTool,
        VisualRole::Warning => Token::Warning,
        VisualRole::Border => Token::BorderSubtle,
        VisualRole::Field => Token::SurfaceRaised,
    }
}

fn visual_style(model: &Model, role: VisualRole) -> Style {
    let theme = model.theme();
    match role {
        VisualRole::Header => theme.filled(Token::Accent),
        other => theme.style(visual_token(other)),
    }
}

fn chat_visual_style(model: &Model, role: VisualRole) -> Style {
    let theme = model.theme();
    match role {
        VisualRole::Header => theme.filled(Token::Accent),
        other => theme.style_transparent(visual_token(other)),
    }
}

fn gradient_style(model: &Model, index: u16, count: u16) -> Style {
    model.theme().gradient_cell(index, count)
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

/// Renders the complete terminal client from local state only.
pub fn view(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let layout = UiLayout::compute(area, model);
    frame.render_widget(Clear, area);
    let shell = render_shell(frame, &layout.regions, model);
    let content = shell.content;
    if content.width > 0 && content.height > 0 {
        match model.route() {
            Route::Chat => {
                if content.width < COMPACT_CHAT_MIN_WIDTH
                    || content.height < COMPACT_CHAT_MIN_HEIGHT
                {
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
    let popup = ui_layout::startup_rect(area);
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
/// Hit testing reverse-scans the paint-order regions produced by `Layout::compute`.
pub fn hit_test(
    model: &Model,
    width: u16,
    height: u16,
    column: u16,
    row: u16,
) -> Option<MouseAction> {
    UiLayout::compute(Rect::new(0, 0, width, height), model).hit_at(column, row)
}

fn render_shell(
    frame: &mut Frame<'_>,
    regions: &ui_layout::NamedRects,
    model: &Model,
) -> ui_layout::NamedRects {
    if let Some(sidebar) = regions.sidebar {
        render_navigation_rail(frame, sidebar, model);
    } else {
        render_shell_footer(frame, regions.footer, model);
    }
    *regions
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
    usize::from(area.height.saturating_sub(SIDEBAR_SESSION_CHROME)).max(1)
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
    let session_width = content.width.saturating_sub(SIDEBAR_LABEL_INSET);
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
        chat_visual_style(model, VisualRole::User),
    ));
    lines.push(Line::from("1  /settings connect a provider key"));
    lines.push(Line::from("2  /models choose a compatible model"));
    let (label, action) = onboarding_step(model);
    lines.push(Line::styled(
        format!("3  {label} · {action}"),
        chat_visual_style(model, VisualRole::Assistant),
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
    ui_layout::inline_palette_rect(area, model)
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
    ui_layout::settings_body_area(area)
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

    let header_height = if body.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(body.height >= PAGE_HELP_MIN);
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
    let compact = area.width < SETTINGS_NAV_COMPACT_WIDTH;
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
    let header_height = if area.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(area.height >= PAGE_HELP_MIN);
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

    let compact = presentation(model).compact || inner.width < PROFILE_COMPACT_WIDTH;
    let notice_height = if model.notice.is_some() && inner.height >= PAGE_HEADER_TALL_MIN {
        if compact { ROW } else { TWO_ROWS }
    } else {
        0
    };
    let header_height = if inner.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(inner.height >= PAGE_HELP_COMFORTABLE);
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
        let help = if inner.width < PROFILE_HELP_NARROW {
            format!("↑/↓ choose  Enter open  Esc {return_to}")
        } else if inner.width < PROFILE_HELP_MEDIUM {
            format!("↑/↓ choose  ←/→ section  Enter open  Esc {return_to}")
        } else if inner.width < PROFILE_HELP_WIDE {
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
    ui_layout::profile_list_detail_areas(area, model)
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
    let header_height = if area.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(area.height >= PAGE_HELP_MIN);
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
        } else if help.width >= SESSION_HELP_WIDE {
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

fn thinking_level(value: &str) -> (&'static str, usize) {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => ("none", 0),
        "minimal" => ("minimal", 1),
        "low" => ("low", 2),
        "medium" => ("medium", 3),
        "high" => ("high", 4),
        "xhigh" => ("xhigh", 5),
        "max" => ("max", 6),
        _ => ("auto", 0),
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
    } else if parts.len() > 3 {
        format!("…/{}", parts[parts.len() - 3..].join("/"))
    } else {
        normalized
    }
}

fn metadata_separator(model: &Model) -> &'static str {
    if presentation(model).ascii {
        " | "
    } else {
        " │ "
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
    metric_index: u16,
) {
    if !spans.is_empty() {
        spans.push(Span::styled(
            metadata_separator(model),
            gradient_style(model, metric_index, 6),
        ));
    }
    spans.push(Span::styled(value, chat_visual_style(model, role)));
}

fn push_thinking_piece(spans: &mut Vec<Span<'static>>, model: &Model, value: &str, compact: bool) {
    if !spans.is_empty() {
        spans.push(Span::styled(
            metadata_separator(model),
            gradient_style(model, 1, 6),
        ));
    }
    let (label, filled) = thinking_level(value);
    spans.push(Span::styled(
        if compact {
            label.chars().next().unwrap_or('a').to_string()
        } else {
            format!("{label} ")
        },
        chat_visual_style(model, VisualRole::User),
    ));
    let (open, full, empty, close) = if presentation(model).ascii {
        ("(", "o", ".", ")")
    } else {
        ("", "●", "○", "")
    };
    spans.push(Span::styled(
        open.to_owned(),
        chat_visual_style(model, VisualRole::Muted),
    ));
    for index in 0..6 {
        let (glyph, style) = if index < filled {
            (
                full,
                model
                    .theme()
                    .gradient_emphasis_style(normalized_t(u16::try_from(index).unwrap_or(5), 6)),
            )
        } else {
            (empty, chat_visual_style(model, VisualRole::Muted))
        };
        spans.push(Span::styled(glyph.to_owned(), style));
    }
    spans.push(Span::styled(
        close.to_owned(),
        chat_visual_style(model, VisualRole::Muted),
    ));
}

fn selected_context_window(model: &Model) -> Option<u64> {
    let selected = model.session.selected_model.as_ref()?;
    model
        .catalog
        .models()
        .iter()
        .find(|summary| &summary.model == selected)
        .and_then(|summary| summary.context_window_tokens)
}

fn latest_turn_usage(model: &Model) -> Option<UsageView> {
    model
        .session
        .transcript
        .iter()
        .rev()
        .find_map(|item| match item {
            TranscriptItem::Assistant { usage, .. } => *usage,
            TranscriptItem::User { .. } | TranscriptItem::Tool(_) => None,
        })
}

fn context_percentage(used: u64, limit: u64) -> String {
    if limit == 0 || used == 0 {
        return "0%".to_owned();
    }
    let tenths = u128::from(used)
        .saturating_mul(1_000)
        .checked_div(u128::from(limit))
        .unwrap_or_default()
        .min(1_000);
    if tenths == 0 {
        return "<0.1%".to_owned();
    }
    if tenths < 100 {
        format!("{}.{:01}%", tenths / 10, tenths % 10)
    } else {
        format!("{}%", (tenths + 5) / 10)
    }
}

fn context_metric(model: &Model, compact: bool) -> (String, VisualRole) {
    let Some(limit) = selected_context_window(model) else {
        return (
            if compact { "--" } else { "ctx --" }.to_owned(),
            VisualRole::Muted,
        );
    };
    let used = latest_turn_usage(model)
        .map(|usage| usage.input_tokens.saturating_add(usage.output_tokens))
        .unwrap_or_default();
    let percentage = context_percentage(used, limit);
    let role = if u128::from(used).saturating_mul(100) >= u128::from(limit).saturating_mul(90) {
        VisualRole::Error
    } else if u128::from(used).saturating_mul(100) >= u128::from(limit).saturating_mul(70) {
        VisualRole::Warning
    } else {
        VisualRole::Tool
    };
    (
        if compact {
            percentage
        } else {
            format!("ctx {percentage}")
        },
        role,
    )
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn prompt_metadata_line(model: &Model, width: u16) -> Line<'static> {
    let mut spans = Vec::new();
    let compact = width < STATUS_COMPACT_WIDTH;
    let model_width = if width >= STATUS_MODEL_WIDE_MIN {
        STATUS_MODEL_CHARS_WIDE
    } else if compact {
        STATUS_MODEL_CHARS_NARROW
    } else {
        STATUS_MODEL_CHARS_MID
    };
    push_metadata_piece(
        &mut spans,
        model,
        single_line_label(&selected_model_name(model), model_width),
        VisualRole::Assistant,
        0,
    );
    if width >= STATUS_THINKING_MIN {
        push_thinking_piece(
            &mut spans,
            model,
            &model.profiles().user.default_mode,
            compact,
        );
    }
    if width >= STATUS_CONTEXT_MIN {
        let (context, role) = context_metric(model, width < STATUS_CONTEXT_COMPACT);
        push_metadata_piece(&mut spans, model, context, role, 2);
    }
    let detail = *model
        .settings()
        .local_profile
        .preferences()
        .prompt_status_detail()
        .value();
    if detail != PromptStatusDetail::Essential && width >= STATUS_WORKSPACE_MIN {
        push_metadata_piece(
            &mut spans,
            model,
            format!(
                "{}{}",
                path_marker(model),
                single_line_label(
                    &workspace_display_path(&model.profiles().user.workspace),
                    if width >= STATUS_WORKSPACE_WIDE {
                        STATUS_PATH_CHARS_WIDE
                    } else {
                        STATUS_PATH_CHARS_NARROW
                    }
                )
            ),
            VisualRole::Normal,
            3,
        );
    }
    if detail != PromptStatusDetail::Essential
        && width >= STATUS_BRANCH_MIN
        && let Some(branch) = model.settings().git_branch.as_deref()
    {
        push_metadata_piece(
            &mut spans,
            model,
            format!(
                "{}{}",
                branch_marker(model),
                single_line_label(branch, STATUS_BRANCH_CHARS)
            ),
            VisualRole::Tool,
            4,
        );
    }
    if detail == PromptStatusDetail::Detailed
        && width >= STATUS_TOKENS_MIN
        && let Some(usage) = latest_turn_usage(model)
    {
        push_metadata_piece(
            &mut spans,
            model,
            format!(
                "in {} / out {}",
                compact_token_count(usage.input_tokens),
                compact_token_count(usage.output_tokens)
            ),
            VisualRole::Muted,
            5,
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
    ui_layout::prompt_surface_height(area, model)
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
    let text = transcript_text(model);
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
                    chat_visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::from("No provider credential is available."));
                lines.push(Line::styled(
                    "Provider API key: use /settings",
                    chat_visual_style(model, VisualRole::Assistant),
                ));
                lines.push(Line::styled(
                    "An API key is still required. Ask AutoHarness after setup.",
                    chat_visual_style(model, VisualRole::Muted),
                ));
            }
            CatalogProjection::Loading => {
                lines.push(Line::styled(
                    format!("{}  CONNECTING", spinner(model)),
                    chat_visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::from("Loading provider models..."));
            }
            CatalogProjection::Failed(failure) => {
                lines.push(Line::styled(
                    "CONNECTION ERROR",
                    chat_visual_style(model, VisualRole::Error),
                ));
                lines.push(Line::from(display_safe(&failure.message)));
                lines.push(Line::styled(
                    "Ctrl+R retry or Alt+3 inspect provider settings",
                    chat_visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { models, .. } if models.is_empty() => {
                lines.push(Line::styled(
                    "NO COMPATIBLE MODELS",
                    chat_visual_style(model, VisualRole::Warning),
                ));
                lines.push(Line::styled(
                    "Ctrl+R refresh the catalog",
                    chat_visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { .. } if model.session.selected_model.is_none() => {
                lines.push(Line::styled(
                    "CHOOSE A MODEL",
                    chat_visual_style(model, VisualRole::User),
                ));
                lines.push(Line::styled(
                    "Ctrl+P opens the searchable model catalog",
                    chat_visual_style(model, VisualRole::Assistant),
                ));
            }
            CatalogProjection::Ready { .. } => {
                lines.push(Line::styled(
                    "NEW CONVERSATION",
                    chat_visual_style(model, VisualRole::User),
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
                lines.push(Line::styled(
                    send,
                    chat_visual_style(model, VisualRole::Muted),
                ));
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
                lines.push(Line::styled(
                    "YOU",
                    chat_visual_style(model, VisualRole::User),
                ));
                push_safe_lines(
                    &mut lines,
                    text,
                    chat_visual_style(model, VisualRole::Normal),
                );
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
                lines.push(Line::styled(
                    heading,
                    chat_visual_style(model, VisualRole::Tool),
                ));
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
                    chat_visual_style(model, VisualRole::Error)
                } else {
                    chat_visual_style(model, VisualRole::Assistant)
                };
                lines.push(Line::styled(heading, style));
                if text.is_empty() && matches!(status, AttemptStatus::Streaming) {
                    lines.push(Line::styled(
                        "Waiting for the first token...",
                        chat_visual_style(model, VisualRole::Muted),
                    ));
                } else {
                    push_safe_lines(
                        &mut lines,
                        text,
                        chat_visual_style(model, VisualRole::Normal),
                    );
                }
                if let AttemptStatus::Failed(failure) = status {
                    lines.push(Line::styled(
                        format!("Error: {}", display_safe(&failure.message)),
                        chat_visual_style(model, VisualRole::Error),
                    ));
                    lines.push(Line::styled(
                        format!(
                            "{} | {} | ref {}",
                            display_safe(&failure.code),
                            retry_label(model, attempt_id, failure.retry),
                            diagnostic_reference(attempt_id)
                        ),
                        chat_visual_style(model, VisualRole::Muted),
                    ));
                }
                if let Some(usage) = usage {
                    lines.push(Line::styled(
                        format!(
                            "{} input tokens · {} output tokens",
                            usage.input_tokens, usage.output_tokens
                        ),
                        chat_visual_style(model, VisualRole::Muted),
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
    let compact = inner.height < PAGE_HEADER_TALL_MIN || inner.width < CREDENTIAL_COMPACT_WIDTH;
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
    ui_layout::confirmation_rect(area)
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
    ui_layout::popup_rect(area)
}

fn codex_auth_rect(area: Rect) -> Rect {
    ui_layout::codex_auth_rect(area)
}

fn credential_rect(area: Rect) -> Rect {
    ui_layout::credential_rect(area)
}

fn user_profile_rect(area: Rect) -> Rect {
    ui_layout::user_profile_rect(area)
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
    model.motion().pending_glyph()
}

fn generation_animation(model: &Model) -> &'static str {
    model.motion().generation_scanner()
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
