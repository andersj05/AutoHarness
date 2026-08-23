use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    AttemptStatus, CatalogProjection, Focus, Model, ModelSummary, Notice, OverlayKind, PendingKind,
    ProfileConnectionState, ProfileCredentialAction, ProfileEditorMode, ProviderKindLabel,
    ProviderProfileProjection, RetryPolicy, Route, TranscriptItem,
};
use crate::text::display_safe;

const HEADER_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Cyan)
    .add_modifier(Modifier::BOLD);
const MUTED_STYLE: Style = Style::new().fg(Color::DarkGray);
const USER_STYLE: Style = Style::new()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::BOLD);
const ASSISTANT_STYLE: Style = Style::new()
    .fg(Color::LightCyan)
    .add_modifier(Modifier::BOLD);
const ERROR_STYLE: Style = Style::new()
    .fg(Color::LightRed)
    .add_modifier(Modifier::BOLD);
const TOOL_STYLE: Style = Style::new().fg(Color::Yellow);

/// Renders the complete terminal client from local state only.
pub fn view(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    match model.route() {
        Route::Chat => {
            if area.width < 24 || area.height < 7 {
                render_compact(frame, area, model);
            } else {
                render_standard(frame, area, model);
            }
        }
        Route::Sessions => render_browser(frame, area, model),
        Route::Profiles => render_profile_center(frame, area, model),
        Route::Settings => render_settings(frame, area, model),
        Route::Help => render_help(frame, area, model),
    }

    match model.overlay() {
        Some(OverlayKind::Permission) => render_permission(frame, area, model),
        Some(OverlayKind::CommandPalette) => render_palette(frame, area, model),
        Some(OverlayKind::SessionCredential) => render_credential(frame, area, model),
        Some(OverlayKind::ModelPicker) => render_picker(frame, area, model),
        Some(OverlayKind::TranscriptSearch | OverlayKind::ProfileCredential) | None => {}
    }
}

/// Renders the searchable command-palette overlay from local state only.
fn render_palette(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Commands ")
        .border_style(Style::default().fg(Color::Cyan));
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
            .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        search,
    );

    let entries = model.palette_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No commands match this filter.").style(MUTED_STYLE),
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
            .map(|entry| palette_item(entry, selected))
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), list);
    }

    if help.height > 0 {
        frame.render_widget(
            Paragraph::new("↑/↓ choose  Enter run  Esc close").style(MUTED_STYLE),
            help,
        );
    }
}

fn palette_item(
    entry: &crate::model::CommandEntry,
    selected: Option<&'static str>,
) -> ListItem<'static> {
    let is_selected = selected == Some(entry.id);
    let prefix = if is_selected { "›" } else { " " };
    let mut label = format!(
        "{prefix} /{} - {}",
        entry.id,
        display_safe(entry.description)
    );
    if let Some(hint) = entry.key_hint {
        let _ = write!(label, "  [{hint}]");
    }
    let style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    ListItem::new(Line::styled(label, style))
}

/// Renders the contextual help overlay from local state only.
///
/// The section matching the surface help was requested from is rendered
/// first and highlighted, and content scrolls without clipping the frame.
fn render_help(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .border_style(Style::default().fg(Color::Cyan));
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
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::styled(section.title.to_owned(), style));
        for (key, description) in section.rows {
            lines.push(Line::from(vec![
                Span::styled(format!("  {key}"), MUTED_STYLE),
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
            Paragraph::new("↑/↓ scroll  Esc close").style(MUTED_STYLE),
            hint,
        );
    }
}

/// Renders the non-modal settings overlay from local state only.
fn render_settings(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Settings ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let credential_line = model.settings().credential_label();
    let lines = vec![
        Line::from(vec![
            Span::styled("Provider   ", MUTED_STYLE),
            Span::raw(display_safe(&model.settings().provider_label())),
        ]),
        Line::from(vec![
            Span::styled("Credential ", MUTED_STYLE),
            Span::raw(display_safe(&credential_line)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Ctrl+, close - Ctrl+G manage profiles - Ctrl+K session-only key",
            MUTED_STYLE,
        )),
    ];
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}
/// Renders the full-screen local profile and provider connection center.
fn render_profile_center(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    frame.render_widget(Clear, area);
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" Profiles & Providers ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let notice_height = if model.notice.is_some() && inner.height >= 8 {
        2
    } else {
        0
    };
    let user_height = if inner.height >= 12 { 4 } else { 2 };
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

    if rows[1].width >= 78 && rows[1].height >= 7 {
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
            "↑/↓ choose Enter active Alt+N new Alt+E edit Alt+K key Alt+T test Alt+M default Esc"
        } else if rows[3].width >= 70 {
            "↑/↓ choose  Enter active  Alt+N new  Alt+K key  Alt+T test  Esc"
        } else if rows[3].width >= 50 {
            "↑/↓ choose  Enter active  Alt+N new  Alt+K key  Esc"
        } else {
            "↑/↓  Enter active  Alt+N new  Esc"
        };
        frame.render_widget(Paragraph::new(hints).style(MUTED_STYLE), rows[3]);
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
    let first = Line::from(vec![
        Span::styled(format!(" {} ", display_safe(label)), HEADER_STYLE),
        Span::raw("  "),
        Span::styled("Default ", MUTED_STYLE),
        Span::raw(display_safe(default_profile)),
        Span::styled("  Mode ", MUTED_STYLE),
        Span::raw(display_safe(&user.default_mode)),
    ]);
    let workspace = Line::from(vec![
        Span::styled(" Workspace ", MUTED_STYLE),
        Span::raw(display_safe(&user.workspace)),
    ]);
    let mut lines = vec![first];
    if area.height >= 2 {
        lines.push(workspace);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::BOTTOM))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_profile_list(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let title = format!(
        " Provider profiles - filter: {} ",
        display_safe(&model.profile_center.query)
    );
    let block = Block::default().borders(Borders::ALL).title(title);
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
                .style(MUTED_STYLE)
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
    let marker = if selected { ">" } else { " " };
    let active = if profile.active { "*" } else { " " };
    let style = if selected {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else if profile.active {
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let id = display_safe(&profile.id);
    let label = if width >= 44 {
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
    let block = Block::default().borders(Borders::ALL).title(" Connection ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let Some(profile) = model.selected_profile() else {
        frame.render_widget(
            Paragraph::new("Select or create a profile.").style(MUTED_STYLE),
            inner,
        );
        return;
    };
    let mut lines = vec![
        detail_line("Name", &profile.id),
        detail_line("Provider", profile.kind.as_str()),
        detail_line("Credential", profile.credential_state.as_str()),
        detail_line("Source", profile.credential_source.as_str()),
        detail_line("Connection", profile.connection.label()),
    ];
    if profile.kind == ProviderKindLabel::Router {
        lines.push(detail_line("Base URL", &profile.base_url));
        if !profile.project.is_empty() {
            lines.push(detail_line("Project", &profile.project));
        }
        if !profile.auth_header.is_empty() {
            lines.push(detail_line("Auth header", &profile.auth_header));
        }
    }
    if let ProfileConnectionState::Failed(reason) = &profile.connection {
        lines.push(Line::from(vec![
            Span::styled("Reason      ", ERROR_STYLE),
            Span::raw(display_safe(reason)),
        ]));
    }
    if model.profiles().pending_recovery > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "{} credential repair operation(s) pending",
                model.profiles().pending_recovery
            ),
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Alt+N new  Alt+E edit  Alt+D duplicate  Alt+K save/replace",
        MUTED_STYLE,
    )));
    lines.push(Line::from(Span::styled(
        "Alt+T test  Alt+M set current model default",
        MUTED_STYLE,
    )));
    lines.push(Line::from(Span::styled(
        "Alt+X disconnect  Delete remove",
        MUTED_STYLE,
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn detail_line(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), MUTED_STYLE),
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
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
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default()
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
        MUTED_STYLE,
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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " {action} credential - {} ",
            display_safe(&editor.profile_id)
        ))
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let masked = if editor.has_value() {
        "••••••••"
    } else {
        "paste or type API key"
    };
    let lines = vec![
        Line::from(Span::styled(
            masked,
            Style::default().fg(Color::LightYellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Stored only in the operating-system vault. Enter save  Esc cancel",
            MUTED_STYLE,
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Renders the searchable session-browser overlay from local state only.
fn render_browser(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Sessions ")
        .border_style(Style::default().fg(Color::Cyan));
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
        Paragraph::new(search_line).style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        search,
    );

    let entries = model.browser_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No sessions match this filter.")
                .style(MUTED_STYLE)
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
        let confirming = model.browser.confirming_delete.is_some();
        let hints = if confirming {
            "Y delete permanently  N/Esc cancel"
        } else {
            "↑/↓ choose  Enter open  Ctrl+R rename  Ctrl+A archive  Ctrl+D delete  Esc close"
        };
        frame.render_widget(Paragraph::new(hints).style(MUTED_STYLE), help);
    }
}

fn browser_item(entry: &crate::model::SessionBrowserEntry, model: &Model) -> ListItem<'static> {
    let selected = model
        .browser
        .selected
        .as_ref()
        .is_some_and(|candidate| candidate == &entry.session_id);
    let prefix = if selected { "›" } else { " " };
    let mut label = format!("{prefix} {}", display_safe(&entry.title));
    if entry.active {
        label.push_str("  [active]");
    }
    if entry.archived {
        label.push_str("  [archived]");
    }
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if entry.archived {
        MUTED_STYLE
    } else {
        Style::default().fg(Color::White)
    };
    ListItem::new(Line::styled(label, style))
}

fn render_permission(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = credential_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Tool permission ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let Some(request) = model.session.permission_requests.first() else {
        return;
    };
    let pending = model.answering_permissions.contains(&request.tool_call_id);
    let help = if pending {
        "Saving answer..."
    } else {
        "Up/Down inspect  Y allow this exact call once  N/Esc deny"
    };
    let mut lines = vec![
        Line::styled(
            "A model requested an external capability.",
            Style::default().fg(Color::White),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", MUTED_STYLE),
            Span::raw(display_safe(&request.tool_name)),
        ]),
        Line::from(vec![
            Span::styled("Capability: ", MUTED_STYLE),
            Span::raw(display_safe(&request.capability)),
        ]),
        Line::from(vec![
            Span::styled("Resource: ", MUTED_STYLE),
            Span::raw(display_safe(&request.resource)),
        ]),
        Line::from(""),
    ];
    lines.extend(request.details.iter().map(|detail| {
        Line::from(vec![
            Span::styled(format!("{}: ", display_safe(&detail.label)), MUTED_STYLE),
            Span::raw(display_safe(&detail.value)),
        ])
    }));
    lines.push(Line::from(""));
    lines.push(Line::styled(help, Style::default().fg(Color::Yellow)));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((model.permission_scroll, 0)),
        inner,
    );
}

fn render_standard(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let composer_height = u16::try_from(model.composer.lines().len())
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .clamp(3, 8);
    let notice_height = if model.notice.is_some() { 2 } else { 0 };
    let search_height = u16::from(model.search_open());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(search_height),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], model);
    render_transcript(frame, chunks[1], model, true);
    if notice_height > 0 {
        render_notice(frame, chunks[2], model);
    }
    if search_height > 0 {
        render_search_bar(frame, chunks[3], model);
    }
    frame.render_widget(&model.composer.editor, chunks[4]);
    render_footer(frame, chunks[5], model);
    set_composer_cursor(frame, chunks[4], model, true);
}

/// Renders the one-row transcript search bar.
fn render_search_bar(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let status = model.search_status_label();
    let query = display_safe(&model.search.query);
    frame.render_widget(
        Paragraph::new(format!(" Search: /{query} - {status} "))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        area,
    );
}

fn render_compact(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let header = Rect::new(area.x, area.y, area.width, 1);
    render_header(frame, header, model);
    if area.height == 1 {
        return;
    }

    let remaining = area.height - 1;
    let composer_height = remaining.min(2);
    let transcript_height = remaining.saturating_sub(composer_height);
    let transcript = Rect::new(area.x, area.y + 1, area.width, transcript_height);
    let composer = Rect::new(
        area.x,
        area.y + 1 + transcript_height,
        area.width,
        composer_height,
    );

    if transcript.height > 0 {
        render_transcript(frame, transcript, model, false);
    }
    if composer.height > 0 {
        let mut editor = model.composer.editor.clone();
        editor.remove_block();
        frame.render_widget(&editor, composer);
        set_composer_cursor(frame, composer, model, false);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let selected = model
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
        .unwrap_or_else(|| "no model".to_owned());

    let state = if let Some((attempt_id, status)) = model.session.active_attempt() {
        if matches!(status, AttemptStatus::Cancelling) || model.cancelling.contains(attempt_id) {
            format!("{} cancelling", spinner(model.now))
        } else {
            format!("{} streaming", spinner(model.now))
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
    };

    // Status surface segments degrade left to right: identity and work state
    // survive at every width; provider, credential, catalog, and usage detail
    // appear as space allows. Credential wording never claims a connection
    // that is not effective.
    let provider = model.settings.provider_label();
    let credential = header_credential_label(model);
    let catalog = match &*model.catalog {
        CatalogProjection::Ready { stale: true, .. } => "catalog stale",
        CatalogProjection::Failed(_) => "catalog error",
        CatalogProjection::Ready { stale: false, .. }
        | CatalogProjection::Loading
        | CatalogProjection::CredentialRequired => "",
    };
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
        Paragraph::new(display_safe(&title)).style(HEADER_STYLE),
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
        Block::default()
            .borders(Borders::ALL)
            .title(" Transcript ")
            .border_style(MUTED_STYLE)
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
        lines.push(Line::styled(
            "Choose a model and write a prompt to begin.",
            MUTED_STYLE,
        ));
        return Text::from(lines);
    }

    for (index, item) in model.session.transcript.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        match item {
            TranscriptItem::User { text, .. } => {
                lines.push(Line::styled("you", USER_STYLE));
                push_safe_lines(&mut lines, text, Style::default());
            }
            TranscriptItem::Tool(row) => {
                let mut heading = String::from("tool ");
                heading.push_str(&display_safe(&row.tool_name));
                if !row.status.is_empty() {
                    heading.push_str(" · ");
                    heading.push_str(&display_safe(&row.status));
                }
                if let Some(summary) = &row.summary {
                    heading.push_str(" · ");
                    heading.push_str(&display_safe(summary));
                }
                if model.tools_expanded {
                    heading.push_str("  [");
                    heading.push_str(&display_safe(&row.resource));
                    heading.push(']');
                }
                lines.push(Line::styled(heading, TOOL_STYLE));
            }
            TranscriptItem::Assistant {
                attempt_id,
                text,
                status,
                usage,
                retry_of,
            } => {
                let mut heading = String::from("assistant");
                if retry_of.is_some() {
                    heading.push_str(" · retry");
                }
                match status {
                    AttemptStatus::Streaming => {
                        let _ = write!(heading, " · {} streaming", spinner(model.now));
                    }
                    AttemptStatus::Cancelling => {
                        let _ = write!(heading, " · {} cancelling", spinner(model.now));
                    }
                    AttemptStatus::Completed => heading.push_str(" · complete"),
                    AttemptStatus::Cancelled => heading.push_str(" · cancelled"),
                    AttemptStatus::Failed(_) => heading.push_str(" · failed"),
                }
                if matches!(status, AttemptStatus::Streaming)
                    && (model.pending.values().any(|pending| {
                        matches!(pending, PendingKind::CancelAttempt(candidate) if candidate == attempt_id)
                    }) || model.cancelling.contains(attempt_id))
                {
                    heading.push_str(" · cancelling");
                }
                if model.retry_requested(attempt_id) {
                    heading.push_str(" · retrying");
                }
                let style = if matches!(status, AttemptStatus::Failed(_)) {
                    ERROR_STYLE
                } else {
                    ASSISTANT_STYLE
                };
                lines.push(Line::styled(heading, style));
                if text.is_empty() && matches!(status, AttemptStatus::Streaming) {
                    lines.push(Line::styled("Waiting for the first token...", MUTED_STYLE));
                } else {
                    push_safe_lines(&mut lines, text, Style::default());
                }
                if let AttemptStatus::Failed(failure) = status {
                    lines.push(Line::styled(
                        format!("Error: {}", display_safe(&failure.message)),
                        ERROR_STYLE,
                    ));
                    lines.push(Line::styled(
                        format!(
                            "{} | {} | ref {}",
                            display_safe(&failure.code),
                            retry_label(model, attempt_id, failure.retry),
                            diagnostic_reference(attempt_id)
                        ),
                        MUTED_STYLE,
                    ));
                }
                if let Some(usage) = usage {
                    lines.push(Line::styled(
                        format!(
                            "{} input tokens · {} output tokens",
                            usage.input_tokens, usage.output_tokens
                        ),
                        MUTED_STYLE,
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
        Notice::Info(message) => (display_safe(message), Style::default().fg(Color::Yellow)),
        Notice::Failure(failure) => (
            format!(
                "Error [{}]: {}",
                display_safe(&failure.code),
                display_safe(&failure.message)
            ),
            ERROR_STYLE,
        ),
    };
    frame.render_widget(
        Paragraph::new(label)
            .block(Block::default().borders(Borders::TOP).border_style(style))
            .style(style),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width < 50 {
        render_narrow_footer(frame, area, model);
        return;
    }

    let mut spans = vec![
        Span::styled(
            " Ctrl+S ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw("send  "),
    ];
    if area.width >= 72 {
        spans.push(Span::styled(
            " Enter ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::raw("newline  "));
    }
    spans.extend([
        Span::styled(
            " Ctrl+P ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw("models  "),
        Span::styled(
            " Ctrl+N ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw("new"),
    ]);
    if area.width >= 88 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " Ctrl+L ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::raw("sessions"));
    }
    if area.width >= 100 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " Ctrl+K ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::raw("API key"));
    }
    if area.width >= 104 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " F1 ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ));
        spans.push(Span::raw("help"));
    }
    if let Some((attempt_id, status)) = model.session.active_attempt() {
        spans.push(Span::raw("  "));
        if model.cancelling.contains(attempt_id) || matches!(status, AttemptStatus::Cancelling) {
            spans.push(Span::styled(
                "cancelling...",
                Style::default().fg(Color::Yellow),
            ));
        } else {
            spans.push(Span::styled(
                " Esc ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
            spans.push(Span::raw("cancel"));
        }
    } else if let Some((attempt_id, retry)) = model.session.retryable_attempt() {
        spans.push(Span::raw("  "));
        if model.retry_requested(attempt_id) {
            spans.push(Span::styled(
                "retry requested",
                Style::default().fg(Color::Yellow),
            ));
        } else if model.retry_available(attempt_id, retry) {
            spans.push(Span::styled(
                " Ctrl+R ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
            spans.push(Span::raw("retry"));
        } else if let Some(remaining_ms) = model.retry_remaining_ms(attempt_id, retry) {
            spans.push(Span::styled(
                retry_countdown(remaining_ms),
                Style::default().fg(Color::Yellow),
            ));
        } else {
            spans.push(Span::raw("retry unavailable"));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(MUTED_STYLE), area);
}

fn render_narrow_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let mut spans = vec![
        Span::styled(" ^S ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw("send  "),
        Span::styled(
            " ^P ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw("models  "),
        Span::styled(
            " ^N ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::raw("new"),
    ];
    if let Some((attempt_id, status)) = model.session.active_attempt() {
        spans.push(Span::raw("  "));
        if model.cancelling.contains(attempt_id) || matches!(status, AttemptStatus::Cancelling) {
            spans.push(Span::styled(
                "cancelling",
                Style::default().fg(Color::Yellow),
            ));
        } else {
            spans.push(Span::styled(
                " Esc ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
            spans.push(Span::raw("cancel"));
        }
    } else if let Some((attempt_id, retry)) = model.session.retryable_attempt() {
        spans.push(Span::raw("  "));
        if model.retry_requested(attempt_id) {
            spans.push(Span::styled("retrying", Style::default().fg(Color::Yellow)));
        } else if model.retry_available(attempt_id, retry) {
            spans.push(Span::styled(
                " ^R ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ));
            spans.push(Span::raw("retry"));
        } else if let Some(remaining_ms) = model.retry_remaining_ms(attempt_id, retry) {
            spans.push(Span::styled(
                retry_countdown(remaining_ms),
                Style::default().fg(Color::Yellow),
            ));
        } else {
            spans.push(Span::raw("no retry"));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(MUTED_STYLE), area);
}

fn render_picker(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = popup_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Models ")
        .border_style(Style::default().fg(Color::Cyan));
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
            .style(Style::default().fg(Color::White).bg(Color::DarkGray)),
        search,
    );

    match &*model.catalog {
        CatalogProjection::CredentialRequired => {
            frame.render_widget(
                Paragraph::new("A provider API key is required. Press Ctrl+K to connect.")
                    .style(MUTED_STYLE)
                    .wrap(Wrap { trim: false }),
                list,
            );
        }
        CatalogProjection::Loading => {
            frame.render_widget(Paragraph::new("Loading models...").style(MUTED_STYLE), list);
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
                .style(ERROR_STYLE)
                .wrap(Wrap { trim: false }),
                list,
            );
        }
        CatalogProjection::Ready { stale, .. } => {
            render_picker_models(frame, list, model);
            if *stale && stale_height > 0 {
                frame.render_widget(
                    Paragraph::new("stale catalog - Ctrl+R refresh")
                        .style(Style::default().fg(Color::Yellow)),
                    stale_area,
                );
            }
        }
    }
    if help.height > 0 {
        frame.render_widget(
            Paragraph::new("↑/↓ choose  Enter select  Esc close").style(MUTED_STYLE),
            help,
        );
    }
}

fn render_credential(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = credential_rect(area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Provider API key ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mask = if model.credential.has_value() {
        "••••••••••••"
    } else {
        "paste or type key"
    };
    let text = if inner.height < 8 || inner.width < 36 {
        Text::from(vec![
            Line::from("API key required"),
            Line::styled(
                format!(" {mask} "),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Line::styled("Enter connect  Esc later", MUTED_STYLE),
        ])
    } else {
        Text::from(vec![
            Line::from("Paste your provider API key below."),
            Line::from("It is kept only in memory for this run and is never saved."),
            Line::from(""),
            Line::styled(
                format!("  {mask}  "),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Line::from(""),
            Line::styled("Enter connect  Backspace edit  Esc later", MUTED_STYLE),
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
            Paragraph::new("No models match this filter.").style(MUTED_STYLE),
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
    let prefix = if selected { "›" } else { " " };
    let suffix = if summary.detail.is_empty() {
        String::new()
    } else {
        format!("  {}", display_safe(&summary.detail))
    };
    let label = format!("{prefix} {}{suffix}", display_safe(&summary.display_name));
    let style = if !summary.selectable {
        MUTED_STYLE
    } else if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    ListItem::new(Line::styled(label, style))
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
    if area.width < 30 || area.height < 9 {
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

fn spinner(now: u64) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    FRAMES[usize::try_from((now / 100) % 8).unwrap_or(0)]
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
