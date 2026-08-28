use autoharness_settings::{Source, TerminalTimestampStyle, ThemePreset};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::{
    COMMANDS, CatalogProjection, MODEL_THINKING_LEVELS, Model, ModelDefaultStep, ModelSummary,
    MouseAction, Notice, OverlayKind, PROVIDER_CHOICES, ProfileCenterFocus,
    ProfileCredentialAction, ProfileEditorMode, ProviderKindLabel, Route, SettingsCategory,
    SettingsPreference,
};
use crate::text::display_safe;
use crate::time::{AgeBucket, age_bucket, format_absolute_time, format_relative_age, relative_age};
use crate::ui::component::{
    Button, ButtonRow, ButtonVariant, Chip, ChipVariant, KeyValue, KeyValueTable, ListBadge,
    ListItem as PresentationListItem, ListView, Modal, ModalIntent, Panel, Provenance, SearchField,
    SegmentedControl, SettingKind, SettingRow,
};
use crate::ui::layout::{self as ui_layout, Layout as UiLayout, Presentation};
use crate::ui::metrics::{
    CREDENTIAL_COMPACT_WIDTH, MODAL_MAX_HEIGHT, MODAL_MAX_WIDTH, PAGE_HEADER_TALL_MIN,
    PAGE_HELP_COMFORTABLE, PAGE_HELP_MIN, PALETTE_COLUMN_GAPS, PALETTE_IDENTIFIER_MAX_WIDTH,
    PALETTE_KEY_MAX_WIDTH, PALETTE_LABEL_MIN_WIDTH, PALETTE_THREE_COLUMN_MIN_WIDTH,
    PROFILE_COMPACT_WIDTH, PROFILE_HELP_MEDIUM, PROFILE_HELP_NARROW, PROFILE_HELP_WIDE, ROW,
    SESSION_DETAIL_PERCENT, SESSION_LIST_PERCENT, SESSION_TWO_PANE_MIN_WIDTH,
    SETTINGS_CATEGORY_RAIL_XS, SETTINGS_THEME_LABEL_WIDTH, SETTINGS_THEME_PREVIEW_CELLS,
    SETTINGS_THEME_PREVIEW_INSET, TWO_ROWS,
};
use crate::ui::{Icon, Theme, Token, normalized_t};

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
    Block::default().border_set(model.theme().icons().border_set())
}

fn selection_marker(model: &Model) -> &'static str {
    model.theme().icons().compact_selection_marker()
}

fn navigation_keys(model: &Model) -> &'static str {
    model.theme().icons().vertical_navigation_hint()
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
        Some(OverlayKind::ProfileCredential) => render_profile_credential(frame, area, model),
        Some(OverlayKind::TranscriptSearch) | None => {}
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
            "Archive session",
            format!("Archive session '{}'?", display_safe(session_id)),
            "The session remains durable and can be unarchived.",
        ))
    } else if let Some(session_id) = &model.browser.confirming_delete {
        let target = model
            .sessions
            .sessions
            .iter()
            .find(|entry| &entry.session_id == session_id);
        let label = target.map_or(session_id.as_str(), |entry| entry.title.as_str());
        let consequence = if target.is_some_and(|entry| entry.active) {
            "Its complete archive is written first, then the next conversation opens."
        } else {
            "Its complete provider-neutral archive is written before deletion."
        };
        Some((
            "Delete session",
            format!("Permanently delete '{}'?", display_safe(label)),
            consequence,
        ))
    } else if let Some(profile_id) = &model.profile_center.confirming_disconnect {
        Some((
            "Disconnect credential",
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
                    "Delete provider profile",
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
    let buttons = [
        Button::new(
            "Cancel",
            Some("N/Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::Cancel,
        ),
        Button::new(
            "Confirm",
            Some("Y".to_owned()),
            ButtonVariant::Danger,
            MouseAction::Confirm,
        ),
    ];
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        title,
        Some(Icon::Danger),
        &buttons,
    )
    .intent(ModalIntent::Danger)
    .render(frame.buffer_mut(), area, popup.width, popup.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let lines = vec![
        Line::styled(question, visual_style(model, VisualRole::User)),
        Line::from(""),
        Line::styled(consequence, visual_style(model, VisualRole::Warning)),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_user_profile(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = user_profile_rect(area);
    let buttons = [
        Button::new(
            "Save",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::UserProfileSave,
        ),
        Button::new(
            "Cancel",
            Some("Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::UserProfileCancel,
        ),
    ];
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "User profile",
        Some(Icon::User),
        &buttons,
    )
    .render(frame.buffer_mut(), area, popup.width, popup.height);
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
    let panel = inline_palette_rect(area, model);
    if panel.width == 0 || panel.height == 0 {
        return;
    }
    frame.render_widget(Clear, Rect::new(area.x, panel.y, area.width, panel.height));
    let body = Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::Search),
        Some("Commands"),
        None,
        None,
        true,
    )
    .render(frame.buffer_mut(), panel);
    render_palette_contents(
        frame.buffer_mut(),
        body,
        ui_layout::inline_palette_list_rect(panel),
        model,
    );
}

/// Renders the searchable command-palette overlay from local state only.
fn render_palette(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let mut buttons = Vec::new();
    if let Some(selected) = model.palette_selection() {
        buttons.push(Button::new(
            "Run",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::PaletteRun(selected.to_owned()),
        ));
    }
    buttons.push(Button::new(
        "Close",
        Some("Esc".to_owned()),
        ButtonVariant::Secondary,
        MouseAction::OverlayCancel,
    ));
    let (body, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "Commands",
        Some(Icon::Search),
        &buttons,
    )
    .render(frame.buffer_mut(), area, MODAL_MAX_WIDTH, MODAL_MAX_HEIGHT);
    let popup = popup_rect(area);
    render_palette_contents(
        frame.buffer_mut(),
        body,
        ui_layout::modal_palette_list_rect(popup),
        model,
    );
}

fn render_palette_contents(
    buffer: &mut ratatui::buffer::Buffer,
    body: Rect,
    list: Rect,
    model: &Model,
) {
    if body.width == 0 || body.height == 0 {
        return;
    }
    let entries = model.palette_entries();
    let search = Rect::new(body.x, body.y, body.width, ROW.min(body.height));
    SearchField::new(
        model.theme(),
        model.theme().icons(),
        &model.palette.query,
        model.palette.query.chars().count(),
        Some(u32::try_from(entries.len()).unwrap_or(u32::MAX)),
        true,
    )
    .render(buffer, search);
    if list.width == 0 || list.height == 0 {
        return;
    }
    if entries.is_empty() {
        crate::ui::component::paint::put(
            buffer,
            list.x,
            list.y,
            list.width,
            "No commands match this search.",
            model.theme().style(Token::TextMuted),
        );
        return;
    }
    let rows = ui_layout::visible_command_palette_rows(model, list.height);
    let (identifier_width, label_width, key_width) =
        palette_column_widths(&rows, list.width, model);
    for (offset, row) in rows.into_iter().enumerate() {
        let y = list
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= list.bottom() {
            break;
        }
        match row {
            ui_layout::CommandPaletteRow::Category(category) => {
                crate::ui::component::paint::fill(
                    buffer,
                    Rect::new(list.x, y, list.width, ROW),
                    model.theme().style(Token::SurfaceRaised),
                    Some(' '),
                );
                crate::ui::component::paint::put(
                    buffer,
                    list.x,
                    y,
                    list.width,
                    &category.to_ascii_uppercase(),
                    model
                        .theme()
                        .style(Token::TextMuted)
                        .add_modifier(Modifier::BOLD),
                );
            }
            ui_layout::CommandPaletteRow::Command(entry) => render_palette_command(
                buffer,
                Rect::new(list.x, y, list.width, ROW),
                entry,
                identifier_width,
                label_width,
                key_width,
                model,
            ),
        }
    }
}

fn palette_column_widths(
    rows: &[ui_layout::CommandPaletteRow],
    width: u16,
    model: &Model,
) -> (u16, u16, u16) {
    let marker_width = model.theme().icons().width(Icon::SelectionCaret);
    let identifier_desired = rows
        .iter()
        .filter_map(|row| match row {
            ui_layout::CommandPaletteRow::Command(entry) => Some(
                marker_width
                    .saturating_add(2)
                    .saturating_add(u16::try_from(entry.id.width()).unwrap_or(u16::MAX)),
            ),
            ui_layout::CommandPaletteRow::Category(_) => None,
        })
        .max()
        .unwrap_or(0)
        .min(PALETTE_IDENTIFIER_MAX_WIDTH);
    let gaps = if width == 0 {
        0
    } else if width >= PALETTE_THREE_COLUMN_MIN_WIDTH {
        PALETTE_COLUMN_GAPS.min(width)
    } else {
        ROW.min(width)
    };
    let available = width.saturating_sub(gaps);
    let reserved_label = PALETTE_LABEL_MIN_WIDTH.min(available / 2);
    let identifier_width = identifier_desired.min(available.saturating_sub(reserved_label));
    let key_width = if width >= PALETTE_THREE_COLUMN_MIN_WIDTH {
        rows.iter()
            .filter_map(|row| match row {
                ui_layout::CommandPaletteRow::Command(entry) => entry.key_hint,
                ui_layout::CommandPaletteRow::Category(_) => None,
            })
            .map(|hint| u16::try_from(hint.width()).unwrap_or(u16::MAX))
            .max()
            .unwrap_or(0)
            .min(PALETTE_KEY_MAX_WIDTH)
            .min(
                available
                    .saturating_sub(identifier_width)
                    .saturating_sub(reserved_label),
            )
    } else {
        0
    };
    let label_width = available
        .saturating_sub(identifier_width)
        .saturating_sub(key_width);
    (identifier_width, label_width, key_width)
}

fn render_palette_command(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    entry: crate::model::CommandEntry,
    identifier_width: u16,
    label_width: u16,
    key_width: u16,
    model: &Model,
) {
    let selected = model.palette_selection() == Some(entry.id);
    let base_style = if selected {
        model.theme().filled(Token::Accent)
    } else {
        model.theme().style(Token::TextPrimary)
    };
    if selected {
        crate::ui::component::paint::fill(buffer, area, base_style, Some(' '));
    }
    let icons = model.theme().icons();
    let marker = if selected {
        icons.glyph(Icon::SelectionCaret).to_owned()
    } else {
        " ".repeat(usize::from(icons.width(Icon::SelectionCaret)))
    };
    let identifier = format!("{marker} /{}", entry.id);
    let identifier = crate::ui::component::paint::ellipsize_words(&identifier, identifier_width);
    let middle = format!(
        "{} - {}",
        display_safe(entry.label),
        display_safe(entry.description)
    );
    let middle = crate::ui::component::paint::ellipsize_words(&middle, label_width);
    let highlights = palette_highlights(entry, &model.palette.query);
    put_highlighted(
        buffer,
        area.x,
        area.y,
        identifier_width,
        &identifier,
        &highlights.identifier,
        base_style,
    );
    let label_x = area.x.saturating_add(identifier_width).saturating_add(ROW);
    put_highlighted(
        buffer,
        label_x,
        area.y,
        label_width,
        &middle,
        &highlights.middle,
        base_style,
    );
    if key_width > 0 {
        let key = entry.key_hint.unwrap_or_default();
        let key = crate::ui::component::paint::ellipsize_words(key, key_width);
        let key = crate::ui::component::paint::right_align(&key, key_width);
        crate::ui::component::paint::put(
            buffer,
            area.right().saturating_sub(key_width),
            area.y,
            key_width,
            &key,
            if selected {
                base_style
            } else {
                model.theme().style(Token::TextMuted)
            },
        );
    }
}

#[derive(Default)]
struct PaletteHighlights {
    identifier: Vec<usize>,
    middle: Vec<usize>,
}

fn palette_highlights(entry: crate::model::CommandEntry, query: &str) -> PaletteHighlights {
    let query = query.trim_start_matches('/').trim().to_ascii_lowercase();
    if query.is_empty() {
        return PaletteHighlights::default();
    }
    let marker_offset = 3;
    let identifier = entry.id.to_ascii_lowercase();
    let label = entry.label.to_ascii_lowercase();
    let description = entry.description.to_ascii_lowercase();
    let mut highlights = PaletteHighlights::default();
    append_occurrences(
        &mut highlights.identifier,
        &identifier,
        &query,
        marker_offset,
    );
    append_occurrences(&mut highlights.middle, &label, &query, 0);
    append_occurrences(
        &mut highlights.middle,
        &description,
        &query,
        entry.label.chars().count().saturating_add(3),
    );
    if !highlights.identifier.is_empty() || !highlights.middle.is_empty() {
        return highlights;
    }
    let id_fuzzy = fuzzy_character_positions(&identifier, &query);
    let label_fuzzy = fuzzy_character_positions(&label, &query);
    if id_fuzzy.len() >= label_fuzzy.len() {
        highlights.identifier.extend(
            id_fuzzy
                .into_iter()
                .map(|index| index.saturating_add(marker_offset)),
        );
    } else {
        highlights.middle.extend(label_fuzzy);
    }
    highlights
}

fn append_occurrences(target: &mut Vec<usize>, haystack: &str, needle: &str, offset: usize) {
    let mut rest = haystack;
    let mut byte_offset = 0_usize;
    while let Some(found) = rest.find(needle) {
        let start = haystack[..byte_offset.saturating_add(found)]
            .chars()
            .count();
        target.extend(
            (start..start.saturating_add(needle.chars().count()))
                .map(|index| index.saturating_add(offset)),
        );
        let advance = found.saturating_add(needle.len());
        byte_offset = byte_offset.saturating_add(advance);
        rest = &rest[advance..];
    }
}

fn fuzzy_character_positions(candidate: &str, query: &str) -> Vec<usize> {
    let query = query.chars().collect::<Vec<_>>();
    let mut query_index = 0;
    let mut positions = Vec::new();
    for (candidate_index, character) in candidate.chars().enumerate() {
        if query.get(query_index) == Some(&character) {
            positions.push(candidate_index);
            query_index = query_index.saturating_add(1);
        }
    }
    positions
}

fn put_highlighted(
    buffer: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    highlighted: &[usize],
    base_style: Style,
) {
    let mut used = 0_u16;
    for (index, character) in text.chars().enumerate() {
        let character_width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
        if used.saturating_add(character_width) > width {
            break;
        }
        let style = if highlighted.contains(&index) {
            base_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            base_style
        };
        crate::ui::component::paint::put(
            buffer,
            x.saturating_add(used),
            y,
            character_width,
            &character.to_string(),
            style,
        );
        used = used.saturating_add(character_width);
    }
}

/// Renders the contextual help overlay from local state only.
///
/// The section matching the surface help was requested from is rendered
/// first and highlighted, and content scrolls without clipping the frame.
fn render_help(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    frame.render_widget(Clear, area);
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Help ")
        .border_style(visual_style(model, VisualRole::Border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let hint_height = u16::from(inner.height >= 2);
    let content_height = inner.height - hint_height;
    let content = Rect::new(inner.x, inner.y, inner.width, content_height);
    let rows = generated_help_rows(model.navigation.previous_route.focus());
    let label_width = rows
        .iter()
        .filter_map(|row| match row {
            HelpRenderRow::Pair { key, .. } => Some(key.chars().count()),
            HelpRenderRow::Header { .. } | HelpRenderRow::Gap => None,
        })
        .max()
        .unwrap_or(0);
    for (visual, row) in rows
        .iter()
        .skip(usize::from(model.help.scroll))
        .take(usize::from(content.height))
        .enumerate()
    {
        let y = content
            .y
            .saturating_add(u16::try_from(visual).unwrap_or(u16::MAX));
        let line = Rect::new(content.x, y, content.width, ROW);
        match row {
            HelpRenderRow::Header { title, icon } => {
                let heading = format!("{} {title}", model.theme().icons().glyph(*icon));
                let used = crate::ui::component::paint::put(
                    frame.buffer_mut(),
                    line.x,
                    line.y,
                    line.width,
                    &heading,
                    model.theme().style(Token::Accent),
                );
                let rule = model.theme().icons().border().horizontal;
                for x in line.x.saturating_add(used).saturating_add(1)..line.right() {
                    crate::ui::component::paint::put(
                        frame.buffer_mut(),
                        x,
                        line.y,
                        ROW,
                        rule,
                        model.theme().style(Token::Divider),
                    );
                }
            }
            HelpRenderRow::Pair { key, description } => {
                let padded = format!("{key:label_width$}");
                let table = [KeyValue {
                    label: &padded,
                    value: description,
                    chip: None,
                }];
                KeyValueTable::new(model.theme(), &table).render(frame.buffer_mut(), line);
            }
            HelpRenderRow::Gap => {}
        }
    }

    if hint_height > 0 {
        let hint = Rect::new(inner.x, inner.y + content_height, inner.width, hint_height);
        frame.render_widget(
            Paragraph::new(format!("{} scroll  Esc close", navigation_keys(model)))
                .style(visual_style(model, VisualRole::Muted)),
            hint,
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HelpRenderRow {
    Header { title: &'static str, icon: Icon },
    Pair { key: String, description: String },
    Gap,
}

fn generated_help_rows(origin: crate::model::Focus) -> Vec<HelpRenderRow> {
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
    let command_hints = COMMANDS
        .iter()
        .filter_map(|command| command.key_hint)
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for (position, section) in ordered.into_iter().enumerate() {
        rows.push(HelpRenderRow::Header {
            title: section.title,
            icon: help_section_icon(section.title),
        });
        rows.extend(
            section
                .rows
                .iter()
                .filter(|(key, _)| !command_hints.contains(key))
                .map(|(key, description)| HelpRenderRow::Pair {
                    key: (*key).to_owned(),
                    description: (*description).to_owned(),
                }),
        );
        rows.push(HelpRenderRow::Gap);
        if position == 0 {
            append_command_help_rows(&mut rows);
            rows.push(HelpRenderRow::Gap);
        }
    }
    rows
}

fn append_command_help_rows(rows: &mut Vec<HelpRenderRow>) {
    rows.push(HelpRenderRow::Header {
        title: "Commands",
        icon: Icon::PromptCaret,
    });
    rows.extend(COMMANDS.iter().filter_map(|command| {
        command.key_hint.map(|key| HelpRenderRow::Pair {
            key: key.to_owned(),
            description: format!("/{}  {}", command.id, command.label),
        })
    }));
}

fn help_section_icon(title: &str) -> Icon {
    match title {
        "Composer" => Icon::RouteChat,
        "Browser" => Icon::RouteSessions,
        "Profiles" => Icon::RouteProviders,
        "Models" => Icon::RouteModels,
        "Settings" => Icon::RouteSettings,
        "Global" => Icon::Brand,
        _ => Icon::RouteHelp,
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
        TerminalTimestampStyle::Absolute => Some(format_absolute_time(updated_at_ms)),
        TerminalTimestampStyle::Hidden => None,
    }
}

fn session_detail_timestamp_label(model: &Model, updated_at_ms: i64) -> Option<String> {
    match *model
        .settings()
        .local_profile
        .preferences()
        .terminal_timestamp_style()
        .value()
    {
        TerminalTimestampStyle::Relative => Some(format!(
            "{} - {}",
            format_relative_age(relative_age(updated_at_ms, model.wall_ms())),
            format_absolute_time(updated_at_ms)
        )),
        TerminalTimestampStyle::Absolute => Some(format_absolute_time(updated_at_ms)),
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
        let vertical = model.theme().icons().vertical_navigation_hint();
        let horizontal = model.theme().icons().horizontal_navigation_hint();
        let help = if inner.width < PROFILE_HELP_NARROW {
            format!("{vertical} choose  Enter open  Esc {return_to}")
        } else if inner.width < PROFILE_HELP_MEDIUM {
            format!("{vertical} choose  {horizontal} section  Enter open  Esc {return_to}")
        } else if inner.width < PROFILE_HELP_WIDE {
            format!(
                "{horizontal} section  {vertical} choose  Enter open  Alt+K sign-in  Esc {return_to}"
            )
        } else {
            format!(
                "{horizontal} section  {vertical} choose  Enter open  Alt+K sign-in  Alt+T test  Del remove  Esc {return_to}"
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
    }
}

fn profile_list_detail_areas(area: Rect, model: &Model) -> (Rect, Option<Rect>) {
    ui_layout::profile_list_detail_areas(area, model)
}

fn render_connected_profiles(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let focused = model.profile_center.focus == ProfileCenterFocus::ProviderChoices;
    let block = app_block(model)
        .borders(Borders::ALL)
        .title(" Provider catalog ")
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
    let scroll = profile_list_scroll(selected, PROVIDER_CHOICES.len(), inner.height);
    for (index, choice) in PROVIDER_CHOICES
        .iter()
        .copied()
        .enumerate()
        .skip(usize::from(scroll))
    {
        let y = inner
            .y
            .saturating_add(u16::try_from(index.saturating_sub(usize::from(scroll))).unwrap_or(0));
        if y >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.x, y, inner.width, ROW);
        let selected_row = index == selected;
        if selected_row && focused {
            crate::ui::component::paint::fill(
                frame.buffer_mut(),
                row,
                model.theme().style(Token::SurfaceSelected),
                Some(' '),
            );
        }
        let style = if selected_row && focused {
            model.theme().style(Token::TextOnAccent)
        } else {
            model.theme().style(Token::TextPrimary)
        };
        let marker = if selected_row {
            model.theme().icons().glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        let label = format!("{marker} {}", choice.label());
        let label_width = crate::ui::component::paint::put(
            frame.buffer_mut(),
            row.x,
            row.y,
            row.width,
            &label,
            style,
        );
        let (status, variant) = provider_choice_chip(model, choice);
        let chip = Chip::new(model.theme(), status, variant);
        let chip_width = chip.measure().min(row.width.saturating_sub(label_width));
        let reason = provider_unavailable_reason(choice);
        let reason_width = reason
            .map(|reason| {
                u16::try_from(reason.len())
                    .unwrap_or(u16::MAX)
                    .saturating_add(1)
            })
            .unwrap_or(0)
            .min(
                row.width
                    .saturating_sub(label_width)
                    .saturating_sub(chip_width),
            );
        let chip_x = row
            .right()
            .saturating_sub(reason_width)
            .saturating_sub(chip_width);
        chip.render(
            frame.buffer_mut(),
            Rect::new(chip_x, row.y, chip_width, ROW),
        );
        if let Some(reason) = reason {
            crate::ui::component::paint::put(
                frame.buffer_mut(),
                chip_x.saturating_add(chip_width).saturating_add(1),
                row.y,
                reason_width.saturating_sub(1),
                reason,
                model.theme().style(Token::TextMuted),
            );
        }
    }
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
        .title(" Saved connections ")
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
    let button_rows = provider_button_rows(inner);
    let list_height = u16::try_from(profiles.len())
        .unwrap_or(u16::MAX)
        .min(button_rows.body.height);
    for (index, candidate) in profiles.iter().enumerate() {
        let y = button_rows
            .body
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if y >= button_rows.body.bottom() {
            break;
        }
        let selected = candidate.id == profile.id;
        let row = Rect::new(button_rows.body.x, y, button_rows.body.width, ROW);
        if selected && focused {
            crate::ui::component::paint::fill(
                frame.buffer_mut(),
                row,
                model.theme().style(Token::SurfaceSelected),
                Some(' '),
            );
        }
        let style = if selected && focused {
            model.theme().style(Token::TextOnAccent)
        } else {
            model.theme().style(Token::TextPrimary)
        };
        let marker = if selected {
            model.theme().icons().glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        crate::ui::component::paint::put(
            frame.buffer_mut(),
            row.x,
            row.y,
            row.width,
            &format!("{marker} {}", display_safe(&candidate.id)),
            style,
        );
        let state = if candidate.active {
            "active"
        } else {
            candidate.connection.label()
        };
        let chip = Chip::new(
            model.theme(),
            state,
            if candidate.active {
                ChipVariant::Success
            } else {
                ChipVariant::Neutral
            },
        );
        let width = chip.measure().min(row.width);
        chip.render(
            frame.buffer_mut(),
            Rect::new(row.right().saturating_sub(width), row.y, width, ROW),
        );
    }
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
    let status = if profile.active && credential_stored {
        "active"
    } else if profile.active {
        "selected - sign-in required"
    } else {
        "saved"
    };
    let model_default = profile
        .default_model
        .as_deref()
        .unwrap_or("provider default");
    let identity = [
        KeyValue {
            label: "Profile",
            value: &profile.id,
            chip: profile.active.then_some("active"),
        },
        KeyValue {
            label: "Provider",
            value: provider_display_name(profile.kind),
            chip: None,
        },
    ];
    let connection = [
        KeyValue {
            label: "Status",
            value: status,
            chip: None,
        },
        KeyValue {
            label: "Connection",
            value: profile.connection.label(),
            chip: None,
        },
    ];
    let credential = [
        KeyValue {
            label: "Sign-in",
            value: sign_in,
            chip: None,
        },
        KeyValue {
            label: "Credential",
            value: managed_by,
            chip: None,
        },
    ];
    let defaults = [
        KeyValue {
            label: "Model",
            value: model_default,
            chip: None,
        },
        KeyValue {
            label: "Thinking",
            value: &profile.default_mode,
            chip: None,
        },
    ];
    let mut y = button_rows.body.y.saturating_add(list_height);
    for (title, rows) in [
        ("Identity", identity.as_slice()),
        ("Connection", connection.as_slice()),
        ("Credential", credential.as_slice()),
        ("Defaults", defaults.as_slice()),
    ] {
        y = render_provider_section(frame, button_rows.body, y, model, title, rows);
    }
    if profile.kind == ProviderKindLabel::Router && y < button_rows.body.bottom() {
        let router = [
            KeyValue {
                label: "Base URL",
                value: &profile.base_url,
                chip: None,
            },
            KeyValue {
                label: "Project",
                value: &profile.project,
                chip: None,
            },
            KeyValue {
                label: "Auth header",
                value: &profile.auth_header,
                chip: None,
            },
        ];
        let _ = render_provider_section(frame, button_rows.body, y, model, "Router", &router);
    }
    let primary = provider_primary_buttons(model);
    ButtonRow::new(model.theme(), &primary).render(frame.buffer_mut(), button_rows.primary);
    let secondary = provider_secondary_buttons();
    ButtonRow::new(model.theme(), &secondary).render(frame.buffer_mut(), button_rows.secondary);
}

#[derive(Clone, Copy)]
struct ProviderButtonRows {
    body: Rect,
    primary: Rect,
    secondary: Rect,
}

fn provider_button_rows(inner: Rect) -> ProviderButtonRows {
    let secondary = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(ROW),
        inner.width,
        ROW.min(inner.height),
    );
    let primary = Rect::new(
        inner.x,
        secondary.y.saturating_sub(ROW),
        inner.width,
        ROW.min(inner.height.saturating_sub(ROW)),
    );
    ProviderButtonRows {
        body: Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(TWO_ROWS),
        ),
        primary,
        secondary,
    }
}

fn render_provider_section(
    frame: &mut Frame<'_>,
    area: Rect,
    y: u16,
    model: &Model,
    title: &str,
    rows: &[KeyValue<'_>],
) -> u16 {
    if y >= area.bottom() {
        return y;
    }
    crate::ui::component::paint::put(
        frame.buffer_mut(),
        area.x,
        y,
        area.width,
        title,
        model.theme().style(Token::Accent),
    );
    let table_y = y.saturating_add(ROW);
    let table = Rect::new(
        area.x,
        table_y,
        area.width,
        area.bottom().saturating_sub(table_y),
    );
    let used = KeyValueTable::new(model.theme(), rows).measure(table.width);
    KeyValueTable::new(model.theme(), rows).render(frame.buffer_mut(), table);
    table_y.saturating_add(used)
}

fn provider_primary_buttons(model: &Model) -> Vec<Button<MouseAction>> {
    vec![
        Button::new(
            if model
                .selected_profile()
                .is_some_and(|profile| profile.kind == ProviderKindLabel::CodexCli)
            {
                "Sign in"
            } else {
                "API key"
            },
            None,
            ButtonVariant::Primary,
            MouseAction::ProfileCredential,
        ),
        Button::new(
            "Test",
            None,
            ButtonVariant::Secondary,
            MouseAction::ProfileTest,
        ),
        Button::new(
            "Model",
            None,
            ButtonVariant::Secondary,
            MouseAction::ProfileDefaultModel,
        ),
    ]
}

fn provider_secondary_buttons() -> Vec<Button<MouseAction>> {
    vec![
        Button::new(
            "Disconnect",
            None,
            ButtonVariant::Secondary,
            MouseAction::ProfileDisconnect,
        ),
        Button::new(
            "Remove",
            None,
            ButtonVariant::Danger,
            MouseAction::ProfileDelete,
        ),
    ]
}

fn render_codex_authentication(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = codex_auth_rect(area);
    let can_start = !matches!(
        model.profile_center.codex_login,
        crate::model::CodexLoginState::Starting | crate::model::CodexLoginState::BrowserOpened
    );
    let mut buttons = Vec::new();
    if can_start {
        buttons.push(Button::new(
            if model.profile_center.codex_login == crate::model::CodexLoginState::Failed {
                "Retry"
            } else {
                "Sign in"
            },
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::CodexLogin,
        ));
    }
    buttons.push(Button::new(
        "Cancel",
        Some("Esc".to_owned()),
        ButtonVariant::Secondary,
        MouseAction::CodexLoginCancel,
    ));
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "Sign in to Codex",
        Some(Icon::Locked),
        &buttons,
    )
    .render(frame.buffer_mut(), area, popup.width, popup.height);
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
    let lines = vec![
        Line::styled(
            "Connect your Codex subscription in your default browser.",
            visual_style(model, VisualRole::User),
        ),
        Line::from(""),
        Line::styled(action, visual_style(model, VisualRole::Selected)),
        Line::styled(status, visual_style(model, VisualRole::Muted)),
        Line::from(""),
        Line::styled(
            "Sign-in tokens are kept in your operating-system credential vault.",
            visual_style(model, VisualRole::Muted),
        ),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn provider_display_name(kind: ProviderKindLabel) -> &'static str {
    match kind {
        ProviderKindLabel::Gemini => "Google AI Studio",
        ProviderKindLabel::Router => "OpenAI-compatible API",
        ProviderKindLabel::CodexCli => "Codex subscription",
    }
}

fn provider_choice_chip(
    model: &Model,
    choice: crate::model::ProviderChoice,
) -> (&'static str, ChipVariant) {
    let label = match choice {
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
    };
    let variant = match choice {
        crate::model::ProviderChoice::Cursor | crate::model::ProviderChoice::ClaudeCode => {
            ChipVariant::Muted
        }
        crate::model::ProviderChoice::Codex if label == "Connected" => ChipVariant::Success,
        _ => ChipVariant::Neutral,
    };
    (label, variant)
}

fn provider_unavailable_reason(choice: crate::model::ProviderChoice) -> Option<&'static str> {
    match choice {
        crate::model::ProviderChoice::Cursor => Some("adapter not available"),
        crate::model::ProviderChoice::ClaudeCode => Some("adapter not available"),
        crate::model::ProviderChoice::Gemini
        | crate::model::ProviderChoice::GoogleAiStudio
        | crate::model::ProviderChoice::Codex
        | crate::model::ProviderChoice::OpenAiCompatible => None,
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
    let title = match (editor.mode, editor.kind) {
        (ProfileEditorMode::Create, ProviderKindLabel::CodexCli) => "Connect Codex subscription",
        (ProfileEditorMode::Create, ProviderKindLabel::Gemini) => "Connect Gemini",
        (ProfileEditorMode::Create, ProviderKindLabel::Router) => "Connect compatible API",
        (ProfileEditorMode::Edit, _) => "Edit provider profile",
        (ProfileEditorMode::Duplicate, _) => "Duplicate provider profile",
    };
    let buttons = [
        Button::new(
            "Save",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::ProfileEditorSubmit,
        ),
        Button::new(
            "Cancel",
            Some("Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::ProfileEditorCancel,
        ),
    ];
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        title,
        Some(Icon::RouteProviders),
        &buttons,
    )
    .render(frame.buffer_mut(), area, popup.width, popup.height);
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
        let marker = if selected {
            selection_marker(model)
        } else {
            " "
        };
        lines.push(Line::styled(
            format!("{marker} {label:<12} {}", display_safe(value)),
            style,
        ));
    }
    if editor.mode == ProfileEditorMode::Create && editor.kind == ProviderKindLabel::CodexCli {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "Use the Codex provider card instead. AutoHarness opens browser sign-in directly.",
            visual_style(model, VisualRole::Muted),
        ));
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
    let action = match editor.action {
        ProfileCredentialAction::Save => "Save",
        ProfileCredentialAction::Replace => "Replace",
    };
    let title = format!("{action} credential - {}", display_safe(&editor.profile_id));
    let buttons = [
        Button::new(
            action,
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::ProfileCredentialSubmit,
        ),
        Button::new(
            "Cancel",
            Some("Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::ProfileCredentialCancel,
        ),
    ];
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        &title,
        Some(Icon::Locked),
        &buttons,
    )
    .intent(ModalIntent::Warning)
    .render(frame.buffer_mut(), area, popup.width, popup.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let masked = if editor.has_value() {
        model.theme().icons().secret_mask(8)
    } else {
        "paste or type API key".to_owned()
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{masked}\n\nStored only in the operating-system vault.",),
            visual_style(model, VisualRole::Warning),
        )))
        .wrap(Wrap { trim: false }),
        inner,
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

    if rows[2].height > 0 {
        if model.browser.renaming {
            frame.render_widget(
                Paragraph::new("Rename session  Enter save  Esc cancel")
                    .style(visual_style(model, VisualRole::Muted)),
                rows[2],
            );
        } else {
            let buttons = ui_layout::session_action_buttons(model, rows[2].width);
            let button_row = ButtonRow::new(model.theme(), &buttons);
            let button_width = button_row.measure().min(rows[2].width);
            let hint_width = rows[2]
                .width
                .saturating_sub(button_width)
                .saturating_sub(ROW);
            if hint_width > 0 {
                crate::ui::component::paint::put(
                    frame.buffer_mut(),
                    rows[2].x,
                    rows[2].y,
                    hint_width,
                    &format!("{} select", navigation_keys(model)),
                    visual_style(model, VisualRole::Muted),
                );
            }
            button_row.render(frame.buffer_mut(), rows[2]);
        }
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
    let activity = session_detail_timestamp_label(model, entry.updated_at_ms)
        .unwrap_or_else(|| "hidden".to_owned());
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
    let Some(request) = model.session.permission_requests.first() else {
        return;
    };
    let pending = model.answering_permissions.contains(&request.tool_call_id);
    let buttons = if pending {
        Vec::new()
    } else {
        vec![
            Button::new(
                "Allow",
                Some("Y".to_owned()),
                ButtonVariant::Primary,
                MouseAction::PermissionAllow,
            ),
            Button::new(
                "Deny",
                Some("N/Esc".to_owned()),
                ButtonVariant::Danger,
                MouseAction::PermissionDeny,
            ),
        ]
    };
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "Tool permission",
        Some(Icon::Warning),
        &buttons,
    )
    .intent(ModalIntent::Warning)
    .render(frame.buffer_mut(), area, popup.width, popup.height);
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
    lines.push(Line::from(""));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((model.permission_scroll, 0)),
        inner,
    );
    if pending && inner.height > 0 {
        frame.render_widget(
            Paragraph::new("Saving answer...").style(visual_style(model, VisualRole::Warning)),
            Rect::new(
                inner.x,
                inner.bottom().saturating_sub(ROW),
                inner.width,
                ROW,
            ),
        );
    }
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
    let mut buttons = Vec::new();
    if let Some(selection) = model.picker.selected.clone() {
        buttons.push(Button::new(
            "Select",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::PickerSelect(selection),
        ));
    }
    buttons.push(Button::new(
        "Close",
        Some("Esc".to_owned()),
        ButtonVariant::Secondary,
        MouseAction::OverlayCancel,
    ));
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "Models",
        Some(Icon::RouteModels),
        &buttons,
    )
    .render(frame.buffer_mut(), area, popup.width, popup.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let search_height = 1.min(inner.height);
    let stale_height = u16::from(
        matches!(
            &*model.catalog,
            CatalogProjection::Ready { stale: true, .. }
        ) && inner.height >= 3,
    );
    let list_height = inner.height.saturating_sub(search_height + stale_height);
    let search = Rect::new(inner.x, inner.y, inner.width, search_height);
    let list = Rect::new(inner.x, inner.y + search_height, inner.width, list_height);
    let stale_area = Rect::new(
        inner.x,
        inner.y + search_height + list_height,
        inner.width,
        stale_height,
    );
    SearchField::new(
        model.theme(),
        model.theme().icons(),
        &model.picker.query,
        model.picker.query.chars().count(),
        Some(u32::try_from(filtered_models(model).len()).unwrap_or(u32::MAX)),
        true,
    )
    .render(frame.buffer_mut(), search);

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
}

fn render_credential(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let popup = credential_rect(area);
    let buttons = [
        Button::new(
            "Connect",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::CredentialSubmit,
        ),
        Button::new(
            "Cancel",
            Some("Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::CredentialCancel,
        ),
    ];
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        "Provider API key",
        Some(Icon::Locked),
        &buttons,
    )
    .render(frame.buffer_mut(), area, popup.width, popup.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mask = if model.credential.has_value() {
        model.theme().icons().secret_mask(12)
    } else {
        "paste or type key".to_owned()
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

#[cfg(test)]
mod tests {
    use super::{HelpRenderRow, generated_help_rows, palette_highlights};
    use crate::model::{COMMANDS, Focus};

    #[test]
    fn generated_help_lists_every_command_hint_exactly_once() {
        let rows = generated_help_rows(Focus::Composer);
        for command in COMMANDS.iter().filter(|command| command.key_hint.is_some()) {
            let hint = command.key_hint.expect("filtered command hint");
            let count = rows
                .iter()
                .filter(|row| matches!(row, HelpRenderRow::Pair { key, .. } if key == hint))
                .count();
            assert_eq!(count, 1, "command hint {hint} must appear exactly once");
        }
    }

    #[test]
    fn palette_highlights_exact_prefix_substring_and_fuzzy_matches() {
        let settings = COMMANDS
            .iter()
            .find(|command| command.id == "settings")
            .copied()
            .expect("settings command");
        let exact = palette_highlights(settings, "settings");
        assert_eq!(exact.identifier, (3..11).collect::<Vec<_>>());

        let prefix = palette_highlights(settings, "set");
        assert_eq!(prefix.identifier, vec![3, 4, 5]);

        let substring = palette_highlights(settings, "ting");
        assert_eq!(substring.identifier, vec![6, 7, 8, 9]);

        let fuzzy = palette_highlights(settings, "setings");
        assert_eq!(fuzzy.identifier, vec![3, 4, 5, 7, 8, 9, 10]);
    }
}
