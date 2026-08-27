use std::fmt::Write as _;

use autoharness_settings::{Source, TerminalTimestampStyle, ThemePreset};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    COMMANDS, CatalogProjection, MODEL_THINKING_LEVELS, Model, ModelDefaultStep, ModelSummary,
    MouseAction, Notice, OverlayKind, PROVIDER_CHOICES, ProfileCenterFocus, ProfileConnectionState,
    ProfileCredentialAction, ProfileEditorMode, ProviderKindLabel, Route, SettingsCategory,
    SettingsPreference,
};
use crate::text::display_safe;
use crate::time::{AgeBucket, age_bucket, format_relative_age, relative_age};
use crate::ui::component::{
    Chip, ChipVariant, KeyValue, KeyValueTable, ListBadge, ListItem as PresentationListItem,
    ListView, Panel, Provenance, SearchField, SegmentedControl, SettingKind, SettingRow,
};
use crate::ui::layout::{self as ui_layout, Layout as UiLayout, Presentation};
use crate::ui::metrics::{
    CREDENTIAL_COMPACT_WIDTH, PAGE_HEADER_TALL_MIN, PAGE_HELP_COMFORTABLE, PAGE_HELP_MIN,
    PROFILE_COMPACT_WIDTH, PROFILE_HELP_MEDIUM, PROFILE_HELP_NARROW, PROFILE_HELP_WIDE, ROW,
    SESSION_DETAIL_PERCENT, SESSION_HELP_WIDE, SESSION_LIST_PERCENT, SESSION_TWO_PANE_MIN_WIDTH,
    SETTINGS_CATEGORY_RAIL_XS, SETTINGS_THEME_LABEL_WIDTH, SETTINGS_THEME_PREVIEW_CELLS,
    SETTINGS_THEME_PREVIEW_INSET, TWO_ROWS,
};
use crate::ui::{Icon, Theme, Token, normalized_t};

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

fn app_block(model: &Model) -> Block<'static> {
    let block = Block::default().border_set(ratatui::symbols::border::ROUNDED);
    if presentation(model).ascii {
        block.border_set(ASCII_BORDER)
    } else {
        block
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
            Route::Chat => crate::ui::page::render_chat(frame, &layout.regions, model),
            Route::Sessions => render_browser(frame, content, model),
            Route::Profiles => render_profile_center(frame, content, model),
            Route::Settings => render_settings(frame, &layout.regions, model),
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
        crate::ui::page::render_rail(frame, sidebar, model);
    } else if regions.footer.height > 0 {
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

/// Renders resolved runtime settings and safe provenance as a primary route.
fn render_settings(frame: &mut Frame<'_>, regions: &ui_layout::NamedRects, model: &Model) {
    let area = regions.content;
    frame.render_widget(Clear, area);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Settings ")
        .border_style(visual_style(model, VisualRole::Border));
    frame.render_widget(block, area);
    let (Some(rail), Some(body), Some(footer)) = (
        regions.settings_nav,
        regions.settings_body,
        regions.settings_footer,
    ) else {
        return;
    };
    render_settings_nav(frame, rail, model);
    if model.settings_workspace.search_active {
        render_settings_search(frame, body, model);
    } else if model.settings_workspace.choice_picker_open {
        render_settings_theme_picker(frame, body, model);
    } else if model.settings_workspace.detail_open {
        render_model_defaults(frame, body, model);
    } else {
        render_settings_category(frame, body, model);
    }
    render_settings_footer(frame, footer, model);
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
    let theme = model.theme();
    let icons = theme.icons();
    let visible = usize::from(area.height);
    let scroll = model
        .settings_workspace
        .nav_selected
        .saturating_sub(visible.saturating_sub(1));
    for (index, category) in SettingsCategory::ALL
        .iter()
        .copied()
        .enumerate()
        .skip(scroll)
    {
        let Ok(row) = u16::try_from(index.saturating_sub(scroll)) else {
            continue;
        };
        if row >= area.height {
            break;
        }
        let style = if index == model.settings_workspace.nav_selected {
            if model.settings_workspace.nav_focus {
                theme.filled(Token::SurfaceSelected)
            } else {
                theme.style(Token::Accent)
            }
        } else {
            theme.style(Token::TextSecondary)
        };
        let icon = settings_category_icon(category);
        let text = if area.width <= SETTINGS_CATEGORY_RAIL_XS {
            icons.glyph(icon).to_owned()
        } else {
            format!("{} {}", icons.glyph(icon), category.label())
        };
        crate::ui::component::paint::put(
            frame.buffer_mut(),
            area.x,
            area.y.saturating_add(row),
            area.width,
            &text,
            style,
        );
    }
}

const THEME_OPTIONS: [&str; 9] = [
    "system", "light", "dark", "aurora", "ember", "midnight", "ocean", "forest", "rose",
];
const COLOR_OPTIONS: [&str; 5] = ["color", "soft", "vivid", "no color", "high contrast"];
const GLYPH_OPTIONS: [&str; 3] = ["unicode", "Nerd Font", "ASCII"];
const PROMPT_OPTIONS: [&str; 3] = ["essential", "workspace", "detailed"];
const DENSITY_OPTIONS: [&str; 2] = ["comfortable", "compact"];
const LAYOUT_OPTIONS: [&str; 2] = ["responsive", "single column"];
const TIMESTAMP_OPTIONS: [&str; 3] = ["relative", "absolute", "hidden"];
const SUBMIT_OPTIONS: [&str; 2] = ["Ctrl+S", "Enter"];

fn settings_category_icon(category: SettingsCategory) -> Icon {
    match category {
        SettingsCategory::Appearance => Icon::RouteSettings,
        SettingsCategory::ChatComposer => Icon::RouteChat,
        SettingsCategory::Accessibility => Icon::Success,
        SettingsCategory::Providers => Icon::RouteProviders,
        SettingsCategory::ModelsThinking => Icon::RouteModels,
        SettingsCategory::Profile => Icon::User,
        SettingsCategory::SessionsData => Icon::RouteSessions,
        SettingsCategory::Shortcuts => Icon::PromptCaret,
        SettingsCategory::About => Icon::Info,
    }
}

fn render_settings_category(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let title_height = if area.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW.min(area.height)
    };
    render_settings_page_header(
        frame,
        Rect::new(area.x, area.y, area.width, title_height),
        model,
        category.label(),
        settings_category_description(category),
    );
    let content = Rect::new(
        area.x,
        area.y.saturating_add(title_height),
        area.width,
        area.height.saturating_sub(title_height),
    );
    if category == SettingsCategory::Shortcuts {
        render_settings_shortcuts(frame, content, model);
        return;
    }
    let rows = SettingsPreference::rows(category);
    let label_width = rows
        .iter()
        .map(|row| u16::try_from(row.label().len()).unwrap_or(0))
        .max()
        .unwrap_or(0);
    let visible = usize::from(content.height.max(ROW));
    let scroll = model
        .settings_workspace
        .selected
        .saturating_sub(visible.saturating_div(3));
    let mut y = content.y;
    for (index, preference) in rows.iter().copied().enumerate().skip(scroll) {
        if y >= content.bottom() {
            break;
        }
        let focused = !model.settings_workspace.nav_focus
            && index == model.settings_workspace.selected
            && preference.editable();
        let remaining = content.bottom().saturating_sub(y);
        let used = render_typed_settings_row(
            frame,
            Rect::new(content.x, y, content.width, remaining),
            model,
            preference,
            focused,
            label_width,
        );
        y = y.saturating_add(used.max(ROW));
    }
}

fn settings_category_description(category: SettingsCategory) -> &'static str {
    match category {
        SettingsCategory::Appearance => {
            "Nerd Font needs a patched font. Diamonds mean it is missing."
        }
        SettingsCategory::ChatComposer => "Prompt metadata, timestamps, layout, and submission.",
        SettingsCategory::Accessibility => "Motion, color treatment, and redundant state cues.",
        SettingsCategory::Providers => "Safe connection facts and provider actions.",
        SettingsCategory::ModelsThinking => "Active model and profile thinking defaults.",
        SettingsCategory::Profile => "Local identity and workspace defaults.",
        SettingsCategory::SessionsData => "Durability, redaction, and session access.",
        SettingsCategory::Shortcuts => "Keyboard reference generated from the command table.",
        SettingsCategory::About => "Runtime capabilities and policy facts.",
    }
}

fn render_typed_settings_row(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    preference: SettingsPreference,
    focused: bool,
    label_width: u16,
) -> u16 {
    let value = model.settings_row_value(preference);
    let provenance = settings_provenance(model, preference);
    let description = settings_row_description(preference);
    let kind = match preference {
        SettingsPreference::ReducedMotion => SettingKind::Toggle {
            on: *model
                .settings()
                .local_profile
                .preferences()
                .reduced_motion()
                .value(),
        },
        SettingsPreference::ThemePreset => choice_kind(&THEME_OPTIONS, &value),
        SettingsPreference::ColorMode => choice_kind(&COLOR_OPTIONS, &value),
        SettingsPreference::GlyphMode => choice_kind(&GLYPH_OPTIONS, &value),
        SettingsPreference::PromptStatusDetail => choice_kind(&PROMPT_OPTIONS, &value),
        SettingsPreference::Density => choice_kind(&DENSITY_OPTIONS, &value),
        SettingsPreference::Layout => choice_kind(&LAYOUT_OPTIONS, &value),
        SettingsPreference::TerminalTimestampStyle => choice_kind(&TIMESTAMP_OPTIONS, &value),
        SettingsPreference::ComposerSubmitBehavior => choice_kind(&SUBMIT_OPTIONS, &value),
        SettingsPreference::DisplayLabel => SettingKind::Text {
            value: &value,
            max_len: 64,
        },
        SettingsPreference::ManageProviders
        | SettingsPreference::ConnectCredential
        | SettingsPreference::ConfigureModels
        | SettingsPreference::OpenSessions => SettingKind::Action { label: &value },
        _ => SettingKind::Info { value: &value },
    };
    let row = SettingRow::new(
        model.theme(),
        preference.label(),
        kind,
        provenance,
        Some(if preference == SettingsPreference::ThemePreset {
            ""
        } else {
            description
        }),
        focused,
        label_width,
    );
    let used = row.measure().min(area.height);
    row.render(
        frame.buffer_mut(),
        Rect::new(area.x, area.y, area.width, used),
    );
    if focused && preference == SettingsPreference::ThemePreset && used > ROW {
        render_theme_preview(
            frame.buffer_mut(),
            Rect::new(
                area.x.saturating_add(label_width).saturating_add(ROW),
                area.y.saturating_add(ROW),
                area.width.saturating_sub(label_width.saturating_add(ROW)),
                ROW,
            ),
            model.theme(),
        );
    }
    used
}

fn choice_kind<'a>(options: &'a [&'a str], value: &str) -> SettingKind<'a> {
    let selected = options
        .iter()
        .position(|option| option.eq_ignore_ascii_case(value))
        .unwrap_or_default();
    SettingKind::Choice { options, selected }
}

fn render_theme_preview(buf: &mut ratatui::buffer::Buffer, area: Rect, theme: &Theme) {
    let sample_width = area.width.min(SETTINGS_THEME_PREVIEW_CELLS);
    for index in 0..sample_width {
        crate::ui::component::paint::put(
            buf,
            area.x.saturating_add(index),
            area.y,
            ROW,
            theme.icons().horizontal_rule(),
            theme.gradient_style(normalized_t(index, sample_width.max(ROW))),
        );
    }
    for (offset, token) in [
        Token::SurfaceSunken,
        Token::SurfaceBase,
        Token::SurfaceRaised,
    ]
    .into_iter()
    .enumerate()
    {
        let x = area
            .x
            .saturating_add(sample_width)
            .saturating_add(ROW)
            .saturating_add(u16::try_from(offset).unwrap_or(0));
        crate::ui::component::paint::put(buf, x, area.y, ROW, " ", theme.style(token));
    }
}

fn render_settings_theme_picker(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    render_settings_page_header(
        frame,
        Rect::new(area.x, area.y, area.width, TWO_ROWS.min(area.height)),
        model,
        "Choose theme",
        "Each option previews its gradient and three surface levels.",
    );
    let mut y = area.y.saturating_add(TWO_ROWS);
    for (index, preset) in [
        ThemePreset::System,
        ThemePreset::Light,
        ThemePreset::Dark,
        ThemePreset::Aurora,
        ThemePreset::Ember,
        ThemePreset::Midnight,
        ThemePreset::Ocean,
        ThemePreset::Forest,
        ThemePreset::Rose,
    ]
    .into_iter()
    .enumerate()
    {
        if y >= area.bottom() {
            break;
        }
        let selected = index == model.settings_workspace.choice_picker_selected;
        let marker = if selected {
            model.theme().icons().glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        let preview_theme = Theme::from_preset_with_icons(
            preset,
            model.theme().mode(),
            model.theme().depth(),
            model.theme().icons().mode(),
        );
        crate::ui::component::paint::put(
            frame.buffer_mut(),
            area.x,
            y,
            SETTINGS_THEME_LABEL_WIDTH.min(area.width),
            &format!("{marker} {}", THEME_OPTIONS[index]),
            if selected {
                model.theme().style(Token::FocusRing)
            } else {
                model.theme().style(Token::TextSecondary)
            },
        );
        render_theme_preview(
            frame.buffer_mut(),
            Rect::new(
                area.x.saturating_add(SETTINGS_THEME_PREVIEW_INSET),
                y,
                area.width.saturating_sub(SETTINGS_THEME_PREVIEW_INSET),
                ROW,
            ),
            &preview_theme,
        );
        y = y.saturating_add(ROW);
    }
}

fn render_settings_shortcuts(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let rows = COMMANDS
        .iter()
        .filter_map(|command| {
            command.key_hint.map(|key| KeyValue {
                label: key,
                value: command.description,
                chip: None,
            })
        })
        .collect::<Vec<_>>();
    KeyValueTable::new(model.theme(), &rows).render(frame.buffer_mut(), area);
}

fn settings_provenance(model: &Model, preference: SettingsPreference) -> Provenance {
    let local = &model.settings().local_profile;
    let source = match preference {
        SettingsPreference::DisplayLabel => Some(local.display_label().source()),
        SettingsPreference::ThemePreset => Some(local.preferences().theme_preset().source()),
        SettingsPreference::ColorMode => Some(local.preferences().color_mode().source()),
        SettingsPreference::GlyphMode => Some(local.preferences().glyph_mode().source()),
        SettingsPreference::PromptStatusDetail => {
            Some(local.preferences().prompt_status_detail().source())
        }
        SettingsPreference::ReducedMotion => Some(local.preferences().reduced_motion().source()),
        SettingsPreference::Density => Some(local.preferences().density().source()),
        SettingsPreference::Layout => Some(local.preferences().layout().source()),
        SettingsPreference::TerminalTimestampStyle => {
            Some(local.preferences().terminal_timestamp_style().source())
        }
        SettingsPreference::ComposerSubmitBehavior => {
            Some(local.preferences().composer_submit_behavior().source())
        }
        _ => None,
    };
    match source {
        Some(Source::Default) => Provenance::Default,
        Some(Source::UserFile) => Provenance::User,
        Some(Source::WorkspaceFile) => Provenance::Workspace,
        Some(Source::Environment | Source::CommandLine) => Provenance::Env,
        None => match preference {
            SettingsPreference::Mode | SettingsPreference::Profile => Provenance::Profile,
            SettingsPreference::Approvals
            | SettingsPreference::Retention
            | SettingsPreference::Logging
            | SettingsPreference::StateIndicators => Provenance::Policy,
            SettingsPreference::ColorDepth | SettingsPreference::Version => Provenance::System,
            _ => Provenance::Runtime,
        },
    }
}

fn settings_row_description(preference: SettingsPreference) -> &'static str {
    match preference {
        SettingsPreference::DisplayLabel => "Local identity shown only in this terminal.",
        SettingsPreference::Provider => "Active provider adapter.",
        SettingsPreference::Profile => "Named provider profile used by new sessions.",
        SettingsPreference::Credential => "Safe connection state; secret material is never shown.",
        SettingsPreference::Source => {
            "Credential precedence is environment, vault, then session only."
        }
        SettingsPreference::Model => "Active session model.",
        SettingsPreference::Mode => "Provider-native reasoning default for new sessions.",
        SettingsPreference::ThemePreset => "Palette preview uses the resolved three-stop gradient.",
        SettingsPreference::ColorMode => "Color treatment preserves documented contrast floors.",
        SettingsPreference::GlyphMode => "Nerd Font needs a patched terminal font.",
        SettingsPreference::PromptStatusDetail => {
            "Chooses essential, workspace, or token metadata."
        }
        SettingsPreference::ReducedMotion => "Freezes animated status indicators.",
        SettingsPreference::Density => "Controls spacing between terminal elements.",
        SettingsPreference::Approvals => "Every capability decision remains explicit and per call.",
        SettingsPreference::Retention => "Sessions remain replayable until explicitly deleted.",
        SettingsPreference::Logging => "Credentials and prompt content stay out of diagnostics.",
        SettingsPreference::Layout => "Responsive or forced single-column panel arrangement.",
        SettingsPreference::TerminalTimestampStyle => {
            "Relative, absolute, or hidden transcript times."
        }
        SettingsPreference::ComposerSubmitBehavior => "Selects the key that submits a prompt.",
        SettingsPreference::GlyphCheck => "Every icon in the currently selected glyph mode.",
        SettingsPreference::KeyboardNavigation => "No Settings action requires a mouse.",
        SettingsPreference::StateIndicators => "Important state always uses a glyph plus fill.",
        SettingsPreference::Workspace => "Current safe workspace label.",
        SettingsPreference::ColorDepth => "Detected once from terminal capability variables.",
        SettingsPreference::Version => "Running AutoHarness package version.",
        SettingsPreference::ManageProviders => "Open the full provider profile workspace.",
        SettingsPreference::ConnectCredential => "Open the existing zeroizing credential editor.",
        SettingsPreference::ConfigureModels => "Choose a profile default model and thinking level.",
        SettingsPreference::OpenSessions => "Open durable session search and lifecycle controls.",
    }
}

fn render_settings_search(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let results = model.settings_search_results();
    let field = Rect::new(area.x, area.y, area.width, ROW.min(area.height));
    SearchField::new(
        model.theme(),
        model.theme().icons(),
        &model.settings_workspace.search_query,
        model.settings_workspace.search_query.chars().count(),
        Some(u32::try_from(results.len()).unwrap_or(u32::MAX)),
        true,
    )
    .render(frame.buffer_mut(), field);
    let mut y = area.y.saturating_add(2).min(area.bottom());
    let mut previous = None;
    for (result_index, (category, _, preference)) in results.into_iter().enumerate() {
        if y >= area.bottom() {
            break;
        }
        if previous != Some(category) {
            crate::ui::component::paint::put(
                frame.buffer_mut(),
                area.x,
                y,
                area.width,
                category.label(),
                model.theme().style(Token::Accent),
            );
            y = y.saturating_add(ROW);
            previous = Some(category);
        }
        if y >= area.bottom() {
            break;
        }
        let selected = result_index == model.settings_workspace.search_selected;
        let marker = if selected {
            model.theme().icons().glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        let text = format!(
            "{marker} {}  {}",
            preference.label(),
            model.settings_row_value(preference)
        );
        let style = if selected {
            model.theme().filled(Token::SurfaceSelected)
        } else {
            model.theme().style(Token::TextSecondary)
        };
        crate::ui::component::paint::put(frame.buffer_mut(), area.x, y, area.width, &text, style);
        y = y.saturating_add(ROW);
    }
}

fn render_settings_footer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.height == 0 {
        return;
    }
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let selected = SettingsPreference::rows(category)
        .get(model.settings_workspace.selected)
        .copied();
    let first = if model.settings_workspace.search_active {
        "Search Settings by label or current value".to_owned()
    } else if let Some(preference) = selected.filter(|row| row.editable()) {
        format!(
            "{}  {}  {}",
            preference.label(),
            model.settings_row_value(preference),
            settings_provenance(model, preference).label()
        )
    } else {
        category.label().to_owned()
    };
    frame.render_widget(
        Paragraph::new(first).style(model.theme().style(Token::TextSecondary)),
        Rect::new(area.x, area.y, area.width, ROW),
    );
    if area.height < TWO_ROWS {
        return;
    }
    let controls = settings_footer_controls(model, selected);
    frame.render_widget(
        Paragraph::new(controls).style(model.theme().style(Token::TextMuted)),
        Rect::new(area.x, area.y.saturating_add(ROW), area.width, ROW),
    );
}

fn settings_footer_controls(model: &Model, selected: Option<SettingsPreference>) -> String {
    if model.settings_workspace.search_active {
        return "Type filter  Up/Down select  Enter open  Esc close".to_owned();
    }
    if model.settings_workspace.choice_picker_open {
        return "Up/Down choose  Enter apply  Esc cancel".to_owned();
    }
    if model.settings_workspace.nav_focus {
        return "Up/Down category  Tab/Enter rows  Ctrl+F search  Esc Chat".to_owned();
    }
    let Some(preference) = selected.filter(|row| row.editable()) else {
        return "Tab categories  Ctrl+F search  Esc categories".to_owned();
    };
    let mut controls = match preference {
        SettingsPreference::DisplayLabel => "Enter edit".to_owned(),
        SettingsPreference::ManageProviders
        | SettingsPreference::ConnectCredential
        | SettingsPreference::ConfigureModels
        | SettingsPreference::OpenSessions => "Enter activate".to_owned(),
        SettingsPreference::ReducedMotion => "Left/Right or Space toggle".to_owned(),
        _ => "Left/Right change".to_owned(),
    };
    let provenance = settings_provenance(model, preference);
    if provenance == Provenance::User {
        controls.push_str(&format!(
            "  Backspace inherit -> {}",
            settings_default_value(preference)
        ));
    }
    if !settings_default_value(preference).is_empty() {
        controls.push_str(&format!(
            "  Shift+Backspace default -> {}",
            settings_default_value(preference)
        ));
    }
    controls.push_str("  Tab categories  Esc categories");
    controls
}

fn settings_default_value(preference: SettingsPreference) -> &'static str {
    match preference {
        SettingsPreference::ThemePreset => "system",
        SettingsPreference::ColorMode => "color",
        SettingsPreference::GlyphMode => "unicode",
        SettingsPreference::PromptStatusDetail => "workspace",
        SettingsPreference::ReducedMotion => "off",
        SettingsPreference::Density => "comfortable",
        SettingsPreference::Layout => "responsive",
        SettingsPreference::TerminalTimestampStyle => "relative",
        SettingsPreference::ComposerSubmitBehavior => "Ctrl+S",
        _ => "",
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
        TerminalTimestampStyle::Relative => Some(format_relative_age(relative_age(
            updated_at_ms,
            model.wall_ms(),
        ))),
        TerminalTimestampStyle::Absolute => Some(format!("{updated_at_ms} ms")),
        TerminalTimestampStyle::Hidden => None,
    }
}

fn session_age_group(model: &Model, updated_at_ms: i64) -> &'static str {
    match age_bucket(updated_at_ms, model.wall_ms()) {
        AgeBucket::Today => "Today",
        AgeBucket::Yesterday => "Yesterday",
        AgeBucket::ThisWeek => "This week",
        AgeBucket::Older => "Older",
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
            Constraint::Length(TWO_ROWS.min(area.height)),
            Constraint::Min(1),
            Constraint::Length(TWO_ROWS.min(area.height)),
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
    let active_profile = model
        .profiles()
        .profiles
        .iter()
        .find(|profile| profile.active);
    let profile_label = active_profile.map_or_else(
        || "No active provider connection".to_owned(),
        |profile| {
            format!(
                "Active profile  {}   Current default  {}",
                display_safe(&profile.id),
                display_safe(profile.default_model.as_deref().unwrap_or("not set"))
            )
        },
    );
    frame.render_widget(
        Paragraph::new(profile_label).style(model.theme().style(Token::TextSecondary)),
        rows[1],
    );
    render_model_cards(frame, rows[2], model, active_profile);

    if rows[3].height > 0 {
        frame.render_widget(
            Paragraph::new("Thinking").style(
                if model.model_defaults.step == ModelDefaultStep::Thinking {
                    model.theme().style(Token::Accent)
                } else {
                    model.theme().style(Token::TextMuted)
                },
            ),
            Rect::new(rows[3].x, rows[3].y, rows[3].width, ROW),
        );
        SegmentedControl::new(
            model.theme(),
            &MODEL_THINKING_LEVELS,
            model.model_defaults.thinking_selected,
        )
        .render(
            frame.buffer_mut(),
            Rect::new(
                rows[3].x,
                rows[3].y.saturating_add(ROW),
                rows[3].width,
                ROW.min(rows[3].height.saturating_sub(ROW)),
            ),
        );
    }
    if help_height > 0 {
        frame.render_widget(
            Paragraph::new("Up/Down model  Left/Right thinking  Enter save  Esc Settings")
                .style(visual_style(model, VisualRole::Muted)),
            rows[4],
        );
    }
}

fn render_model_cards(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    active_profile: Option<&crate::model::ProviderProfileProjection>,
) {
    let models = model
        .catalog
        .models()
        .iter()
        .filter(|summary| summary.selectable)
        .collect::<Vec<_>>();
    if models.is_empty() {
        let message = if active_profile.is_some() {
            "Waiting for the active provider's compatible model catalog."
        } else {
            "Connect and activate a provider from Providers first."
        };
        frame.render_widget(
            Paragraph::new(message).style(model.theme().style(Token::TextMuted)),
            area,
        );
        return;
    }
    let visible = usize::from(area.height / TWO_ROWS).max(1);
    let selected = model
        .model_defaults
        .model_selected
        .min(models.len().saturating_sub(1));
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(models.len().saturating_sub(visible));
    for (index, summary) in models.iter().enumerate().skip(start).take(visible) {
        let offset = u16::try_from(index.saturating_sub(start))
            .unwrap_or(u16::MAX)
            .saturating_mul(TWO_ROWS);
        let card = Rect::new(
            area.x,
            area.y.saturating_add(offset),
            area.width,
            TWO_ROWS.min(area.bottom().saturating_sub(area.y.saturating_add(offset))),
        );
        render_model_card(
            frame,
            card,
            model,
            summary,
            index == selected,
            active_profile.and_then(|profile| profile.default_model.as_deref())
                == Some(summary.model.model_id().as_str()),
        );
    }
}

fn render_model_card(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    summary: &ModelSummary,
    selected: bool,
    is_default: bool,
) {
    if selected {
        crate::ui::component::paint::fill(
            frame.buffer_mut(),
            area,
            model.theme().style(Token::SurfaceSelected),
            Some(' '),
        );
    }
    let style = if selected {
        model.theme().style(Token::TextOnAccent)
    } else {
        model.theme().style(Token::TextPrimary)
    };
    let caret = if selected {
        model.theme().icons().glyph(Icon::SelectionCaret)
    } else {
        " "
    };
    let mut x = area.x;
    x = x.saturating_add(crate::ui::component::paint::put(
        frame.buffer_mut(),
        x,
        area.y,
        area.width,
        caret,
        style,
    ));
    x = x.saturating_add(crate::ui::component::paint::put(
        frame.buffer_mut(),
        x,
        area.y,
        area.right().saturating_sub(x),
        " ",
        style,
    ));
    x = x.saturating_add(crate::ui::component::paint::put(
        frame.buffer_mut(),
        x,
        area.y,
        area.right().saturating_sub(x),
        &display_safe(&summary.display_name),
        style,
    ));
    if is_default && x < area.right() {
        x = x.saturating_add(1);
        let chip = Chip::new(model.theme(), "Default", ChipVariant::Accent);
        let width = chip.measure().min(area.right().saturating_sub(x));
        chip.render(
            frame.buffer_mut(),
            Rect::new(x, area.y, width, ROW.min(area.height)),
        );
    }
    if let Some(context) = summary.context_window_tokens {
        let label = format_context_window(context);
        let width = u16::try_from(label.len())
            .unwrap_or(u16::MAX)
            .min(area.width);
        crate::ui::component::paint::put(
            frame.buffer_mut(),
            area.right().saturating_sub(width),
            area.y,
            width,
            &label,
            model.theme().style(Token::TextMuted),
        );
    }
    if area.height < TWO_ROWS {
        return;
    }
    let mut chip_x = area.x.saturating_add(TWO_ROWS);
    for capability in model_capabilities(&summary.detail) {
        let chip = Chip::new(model.theme(), capability, ChipVariant::Neutral);
        let width = chip.measure().min(area.right().saturating_sub(chip_x));
        if width == 0 {
            break;
        }
        chip_x = chip_x.saturating_add(chip.render(
            frame.buffer_mut(),
            Rect::new(chip_x, area.y.saturating_add(ROW), width, ROW),
        ));
        chip_x = chip_x.saturating_add(1);
    }
}

fn model_capabilities(detail: &str) -> impl Iterator<Item = &str> {
    detail
        .split(['|', ','])
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
}

fn format_context_window(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M context", tokens / 1_000_000)
    } else if tokens >= 1_000 {
        format!("{}K context", tokens / 1_000)
    } else {
        format!("{tokens} context")
    }
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
    frame.render_widget(Clear, area);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Sessions ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let confirmation_armed = model.overlay() == Some(OverlayKind::Confirmation);
    let help_height = u16::from(inner.height >= PAGE_HELP_MIN && !confirmation_armed);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(ROW),
            Constraint::Min(ROW),
            Constraint::Length(help_height),
        ])
        .split(inner);
    let entries = model.browser_entries();
    let search_value = if model.browser.renaming {
        model.browser.rename_buffer.as_str()
    } else {
        model.browser.query.as_str()
    };
    SearchField::new(
        model.theme(),
        model.theme().icons(),
        search_value,
        search_value.chars().count(),
        (!model.browser.renaming).then_some(u32::try_from(entries.len()).unwrap_or(u32::MAX)),
        true,
    )
    .render(frame.buffer_mut(), rows[0]);

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
            rows[1],
        );
    } else {
        let (list, detail) =
            if inner.width >= SESSION_TWO_PANE_MIN_WIDTH && !presentation(model).single_column {
                let columns = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(SESSION_LIST_PERCENT),
                        Constraint::Percentage(SESSION_DETAIL_PERCENT),
                    ])
                    .split(rows[1]);
                (columns[0], Some(columns[1]))
            } else {
                (rows[1], None)
            };
        render_session_list(frame, list, model, &entries);
        if let Some(detail) = detail {
            render_session_detail(frame, detail, model, &entries);
        }
    }

    if rows[2].height > 0 && !model.browser.renaming {
        let hints = if rows[2].width >= SESSION_HELP_WIDE {
            "[ Open ] Enter  [ Rename ] Ctrl+R  [ Archive ] Ctrl+A  [ Delete ] Ctrl+D  Esc"
        } else {
            "[ Open ]  [ Rename ]  [ Delete ]  Esc"
        };
        frame.render_widget(
            Paragraph::new(hints).style(visual_style(model, VisualRole::Muted)),
            rows[2],
        );
    }
}

fn render_session_list(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    entries: &[&crate::model::SessionBrowserEntry],
) {
    let selected = model
        .browser
        .selected
        .as_ref()
        .and_then(|selected| {
            entries
                .iter()
                .position(|entry| &entry.session_id == selected)
        })
        .unwrap_or(0);
    let metadata = entries
        .iter()
        .map(|entry| session_timestamp_label(model, entry.updated_at_ms).unwrap_or_default())
        .collect::<Vec<_>>();
    let groups = entries
        .iter()
        .map(|entry| session_age_group(model, entry.updated_at_ms))
        .collect::<Vec<_>>();
    let default_model = model.profiles().user.default_model.as_deref();
    let badges = entries
        .iter()
        .map(|entry| {
            let mut badges = Vec::new();
            if entry.active {
                badges.push(ListBadge {
                    label: "active",
                    variant: ChipVariant::Success,
                });
            }
            if entry.archived {
                badges.push(ListBadge {
                    label: "archived",
                    variant: ChipVariant::Muted,
                });
            }
            if entry
                .selected_model
                .as_ref()
                .map(|model| model.model_id().as_str())
                == default_model
            {
                badges.push(ListBadge {
                    label: "default model",
                    variant: ChipVariant::Accent,
                });
            }
            badges
        })
        .collect::<Vec<_>>();
    let items = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| PresentationListItem {
            label: &entry.title,
            metadata: (!metadata[index].is_empty()).then_some(metadata[index].as_str()),
            group: Some(groups[index]),
            badges: badges[index].as_slice(),
            action: (),
        })
        .collect::<Vec<_>>();
    ListView::new(
        model.theme(),
        model.theme().icons(),
        &items,
        selected,
        "No matching sessions",
    )
    .render(frame.buffer_mut(), area);
}

fn render_session_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &Model,
    entries: &[&crate::model::SessionBrowserEntry],
) {
    let inner = Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::RouteSessions),
        Some("Session details"),
        None,
        None,
        false,
    )
    .render(frame.buffer_mut(), area);
    let Some(entry) = model.browser.selected.as_ref().and_then(|selected| {
        entries
            .iter()
            .find(|entry| &entry.session_id == selected)
            .copied()
    }) else {
        return;
    };
    let model_label = entry.selected_model.as_ref().map_or_else(
        || "not selected".to_owned(),
        |model| {
            format!(
                "{}/{}",
                model.provider_id().as_str(),
                model.model_id().as_str()
            )
        },
    );
    let message_count = entry.message_count.to_string();
    let activity =
        session_timestamp_label(model, entry.updated_at_ms).unwrap_or_else(|| "hidden".to_owned());
    let state = if entry.archived { "archived" } else { "active" };
    let rows = [
        KeyValue {
            label: "Title",
            value: &entry.title,
            chip: entry.active.then_some("current"),
        },
        KeyValue {
            label: "Model",
            value: &model_label,
            chip: None,
        },
        KeyValue {
            label: "Messages",
            value: &message_count,
            chip: None,
        },
        KeyValue {
            label: "Last activity",
            value: &activity,
            chip: None,
        },
        KeyValue {
            label: "State",
            value: state,
            chip: None,
        },
    ];
    KeyValueTable::new(model.theme(), &rows).render(frame.buffer_mut(), inner);
    let state_chip = Chip::new(
        model.theme(),
        state,
        if entry.archived {
            ChipVariant::Muted
        } else {
            ChipVariant::Success
        },
    );
    let chip_width = state_chip.measure().min(inner.width);
    if inner.height > 0 {
        state_chip.render(
            frame.buffer_mut(),
            Rect::new(
                inner.right().saturating_sub(chip_width),
                inner.bottom().saturating_sub(1),
                chip_width,
                ROW,
            ),
        );
    }
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
    crate::ui::page::chat_display_lines(model)
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

fn spinner(model: &Model) -> &'static str {
    model.motion().pending_glyph()
}

fn retry_countdown(remaining_ms: u64) -> String {
    let seconds = remaining_ms.saturating_add(999) / 1_000;
    format!("{seconds}s")
}
