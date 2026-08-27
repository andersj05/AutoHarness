//! One layout pass produces named rectangles and ordered hit regions.

use autoharness_settings::{Density, Layout as PreferenceLayout};
use ratatui::layout::{Constraint, Direction, Layout as Split, Position, Rect};
use unicode_width::UnicodeWidthStr;

use super::component::{Button, ButtonRow, ButtonVariant, modal_size};
use super::metrics::{
    CODEX_ACTION_ROW_OFFSET, CODEX_AUTH_MAX_HEIGHT, COMPACT_CHAT_MIN_HEIGHT,
    COMPACT_CHAT_MIN_WIDTH, COMPOSER_MAX_HEIGHT, COMPOSER_MAX_HEIGHT_COMPACT, COMPOSER_MIN_HEIGHT,
    CONFIRMATION_MAX_HEIGHT, CREDENTIAL_MAX_HEIGHT, CREDENTIAL_MAX_WIDTH,
    INLINE_PALETTE_CHROME_ROWS, INLINE_PALETTE_INSET_X, INLINE_PALETTE_INSET_X_TOTAL,
    INLINE_PALETTE_MAX_ROWS, MODAL_MAX_HEIGHT, MODAL_MAX_WIDTH, PAGE_HEADER_TALL_MIN,
    PAGE_HELP_COMFORTABLE, PAGE_HELP_MIN, PALETTE_MODAL_CHROME_ROWS, PALETTE_MODAL_LIST_TOP_CHROME,
    PROFILE_COMPACT_WIDTH, PROFILE_DETAIL_PERCENT, PROFILE_DETAIL_PERCENT_STACKED,
    PROFILE_LIST_PERCENT, PROFILE_LIST_PERCENT_STACKED, PROFILE_TWO_PANE_MIN_WIDTH,
    PROMPT_INSET_MIN_WIDTH, ROW, SESSION_ACTION_FROM_BOTTOM, SESSION_HELP_WIDE,
    SETTINGS_BODY_INSET_X, SETTINGS_BODY_INSET_X_TOTAL, SETTINGS_BODY_INSET_Y,
    SETTINGS_BODY_INSET_Y_TOTAL, SETTINGS_CATEGORY_RAIL_COMPACT, SETTINGS_CATEGORY_RAIL_WIDE,
    SETTINGS_CATEGORY_RAIL_XS, SETTINGS_FOOTER_ROWS, STARTUP_MAX_HEIGHT, STARTUP_MAX_WIDTH,
    STARTUP_MIN_HEIGHT, STARTUP_MIN_WIDTH, TWO_ROWS, USER_PROFILE_MAX_HEIGHT, WidthBand,
    sidebar_width_for, wide_shell, width_band,
};
use crate::model::{
    CatalogProjection, CommandEntry, Model, MouseAction, OverlayKind, PROVIDER_CHOICES,
    ProfileCredentialAction, ProviderChoice, ProviderKindLabel, Route, SettingsCategory,
    SettingsPreference,
};

/// Settings tab labels, in the same order as `SettingsTab` indices.
pub const SETTINGS_NAV: [&str; 9] = [
    "Appearance",
    "Chat & Composer",
    "Accessibility",
    "Providers",
    "Models & Thinking",
    "Profile",
    "Sessions & Data",
    "Shortcuts",
    "About",
];

/// One planned row in either command-palette presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandPaletteRow {
    /// A category separator that introduces the following commands.
    Category(&'static str),
    /// An executable command.
    Command(CommandEntry),
}

/// Compact footer labels used by both painting and hit testing.
const FOOTER_PROFILE: &str = " Profile ";
const FOOTER_GAP: &str = " | ";
const FOOTER_SETTINGS: &str = " Settings ";

/// Named rectangles for one frame.
#[derive(Clone, Copy, Debug)]
pub struct NamedRects {
    /// Whole terminal area.
    pub area: Rect,
    /// Wide navigation rail, when present.
    pub sidebar: Option<Rect>,
    /// Primary page rectangle.
    pub content: Rect,
    /// Compact-shell footer, empty when the rail is visible.
    pub footer: Rect,
    /// Chat transcript, when Chat is showing a usable page.
    pub transcript: Option<Rect>,
    /// Chat composer surface, including metadata and editor.
    pub composer: Option<Rect>,
    /// Chat composer metadata row.
    pub composer_metadata: Option<Rect>,
    /// Chat notice row.
    pub notice: Option<Rect>,
    /// Chat search row.
    pub search: Option<Rect>,
    /// Settings tab strip.
    pub settings_nav: Option<Rect>,
    /// Settings body below the tab strip.
    pub settings_body: Option<Rect>,
    /// Persistent two-row Settings footer.
    pub settings_footer: Option<Rect>,
    /// Active overlay frame.
    pub overlay: Option<Rect>,
    /// Centered startup indicator.
    pub startup: Option<Rect>,
}

/// Layout result: named rects plus paint-order hit regions.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Regions used by rendering.
    pub regions: NamedRects,
    /// Hit regions in paint order; the last match wins.
    pub hits: Vec<(Rect, MouseAction)>,
}

impl Frame {
    /// Reverse-scans paint-order hits for the cell at `(column, row)`.
    #[must_use]
    pub fn hit_at(&self, column: u16, row: u16) -> Option<MouseAction> {
        let position = Position::new(column, row);
        self.hits
            .iter()
            .rev()
            .find_map(|(rect, action)| rect.contains(position).then(|| action.clone()))
    }
}

/// Computes named rectangles and hit regions for one frame.
pub struct Layout;

impl Layout {
    /// Runs the single layout pass used by both `view` and `hit_test`.
    #[must_use]
    pub fn compute(area: Rect, model: &Model) -> Frame {
        let mut regions = shell_regions(area, model);
        regions.startup = Some(startup_rect(area));
        fill_page_regions(&mut regions, model);
        let overlay = overlay_rect(area, model);
        regions.overlay = overlay;

        let mut hits = Vec::new();
        if model.startup_active() || area.width == 0 || area.height == 0 {
            return Frame { regions, hits };
        }
        if let Some(overlay_kind) = model.overlay() {
            push_overlay_hits(
                &mut hits,
                area,
                overlay.unwrap_or(area),
                overlay_kind,
                model,
            );
            return Frame { regions, hits };
        }
        if model.profile_center.auth_page == Some(ProviderChoice::Codex) {
            push_codex_login_hit(&mut hits, area);
            return Frame { regions, hits };
        }
        push_shell_hits(&mut hits, &regions, model);
        push_route_hits(&mut hits, &regions, model);
        Frame { regions, hits }
    }
}

/// Presentation flags derived from local preferences.
#[derive(Clone, Copy)]
pub struct Presentation {
    /// ASCII glyph mode.
    pub ascii: bool,
    /// Nerd Font glyph mode.
    pub nerd_font: bool,
    /// Compact density.
    pub compact: bool,
    /// Forced single-column layout.
    pub single_column: bool,
}

/// Returns presentation flags for `model`.
#[must_use]
pub fn presentation(model: &Model) -> Presentation {
    let preferences = model.settings().local_profile.preferences();
    Presentation {
        ascii: *preferences.glyph_mode().value() == autoharness_settings::GlyphMode::Ascii,
        nerd_font: *preferences.glyph_mode().value() == autoharness_settings::GlyphMode::NerdFont,
        compact: *preferences.density().value() == Density::Compact,
        single_column: *preferences.layout().value() == PreferenceLayout::SingleColumn,
    }
}

/// Shell split used by rendering.
#[must_use]
pub fn shell_regions(area: Rect, model: &Model) -> NamedRects {
    let wide = wide_shell(area.width, area.height, presentation(model).single_column);
    if wide {
        let columns = Split::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width_for(area.width)),
                Constraint::Min(ROW),
            ])
            .split(area);
        NamedRects {
            area,
            sidebar: Some(columns[0]),
            content: columns[1],
            footer: Rect::default(),
            transcript: None,
            composer: None,
            composer_metadata: None,
            notice: None,
            search: None,
            settings_nav: None,
            settings_body: None,
            settings_footer: None,
            overlay: None,
            startup: None,
        }
    } else if shows_compact_footer(model) {
        let rows = Split::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(ROW)])
            .split(area);
        NamedRects {
            area,
            sidebar: None,
            content: rows[0],
            footer: rows[1],
            transcript: None,
            composer: None,
            composer_metadata: None,
            notice: None,
            search: None,
            settings_nav: None,
            settings_body: None,
            settings_footer: None,
            overlay: None,
            startup: None,
        }
    } else {
        NamedRects {
            area,
            sidebar: None,
            content: area,
            footer: Rect::default(),
            transcript: None,
            composer: None,
            composer_metadata: None,
            notice: None,
            search: None,
            settings_nav: None,
            settings_body: None,
            settings_footer: None,
            overlay: None,
            startup: None,
        }
    }
}

/// Compact Profile/Settings footer stays on Settings and Profiles only.
#[must_use]
pub fn shows_compact_footer(model: &Model) -> bool {
    matches!(model.route(), Route::Settings | Route::Profiles)
}

fn fill_page_regions(regions: &mut NamedRects, model: &Model) {
    match model.route() {
        Route::Chat => fill_chat_regions(regions, model),
        Route::Settings => fill_settings_regions(regions),
        Route::Sessions | Route::Profiles | Route::Help => {}
    }
}

fn fill_chat_regions(regions: &mut NamedRects, model: &Model) {
    let content = regions.content;
    if content.width < COMPACT_CHAT_MIN_WIDTH || content.height < COMPACT_CHAT_MIN_HEIGHT {
        let composer_height = prompt_surface_height(content, model);
        let transcript_height = content.height.saturating_sub(composer_height);
        regions.transcript = Some(Rect::new(
            content.x,
            content.y,
            content.width,
            transcript_height,
        ));
        regions.composer = Some(Rect::new(
            content.x,
            content.y.saturating_add(transcript_height),
            content.width,
            composer_height,
        ));
    } else {
        let composer_height = prompt_surface_height(content, model);
        let notice_height = if model.notice.is_some() {
            if presentation(model).compact {
                ROW
            } else {
                TWO_ROWS
            }
        } else {
            0
        };
        let search_height = u16::from(model.search_open());
        let chunks = Split::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(ROW),
                Constraint::Length(notice_height),
                Constraint::Length(search_height),
                Constraint::Length(composer_height),
            ])
            .split(content);
        regions.transcript = Some(chunks[0]);
        regions.notice = (notice_height > 0).then_some(chunks[1]);
        regions.search = (search_height > 0).then_some(chunks[2]);
        regions.composer = Some(chunks[3]);
    }
    if let Some(composer) = regions.composer {
        let inset = u16::from(composer.width >= PROMPT_INSET_MIN_WIDTH);
        let surface = Rect::new(
            composer.x.saturating_add(inset),
            composer.y,
            composer
                .width
                .saturating_sub(inset.saturating_mul(TWO_ROWS)),
            composer.height,
        );
        let metadata_y = if surface.height > ROW {
            surface.y.saturating_add(ROW)
        } else {
            surface.y
        };
        regions.composer_metadata = Some(Rect::new(surface.x, metadata_y, surface.width, ROW));
    }
}

fn fill_settings_regions(regions: &mut NamedRects) {
    let content = regions.content;
    let inner = Rect::new(
        content.x.saturating_add(ROW),
        content.y.saturating_add(ROW),
        content.width.saturating_sub(TWO_ROWS),
        content.height.saturating_sub(TWO_ROWS),
    );
    let footer_height = SETTINGS_FOOTER_ROWS.min(inner.height);
    let workspace_height = inner.height.saturating_sub(footer_height);
    let rail_width = match width_band(inner.width) {
        WidthBand::Xs => SETTINGS_CATEGORY_RAIL_XS,
        WidthBand::Sm => SETTINGS_CATEGORY_RAIL_COMPACT,
        WidthBand::Md | WidthBand::Lg | WidthBand::Xl => SETTINGS_CATEGORY_RAIL_WIDE,
    }
    .min(inner.width);
    regions.settings_nav = Some(Rect::new(inner.x, inner.y, rail_width, workspace_height));
    regions.settings_body = Some(Rect::new(
        inner.x.saturating_add(rail_width).saturating_add(ROW),
        inner.y,
        inner.width.saturating_sub(rail_width.saturating_add(ROW)),
        workspace_height,
    ));
    regions.settings_footer = Some(Rect::new(
        inner.x,
        inner.y.saturating_add(workspace_height),
        inner.width,
        footer_height,
    ));
}

/// Settings body inside the bordered Settings page.
#[must_use]
pub fn settings_body_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(SETTINGS_BODY_INSET_X),
        area.y.saturating_add(SETTINGS_BODY_INSET_Y),
        area.width.saturating_sub(SETTINGS_BODY_INSET_X_TOTAL),
        area.height.saturating_sub(SETTINGS_BODY_INSET_Y_TOTAL),
    )
}

/// Composer surface height including metadata and the bottom rule.
#[must_use]
pub fn prompt_surface_height(area: Rect, model: &Model) -> u16 {
    u16::try_from(model.composer.lines().len())
        .unwrap_or(u16::MAX)
        .saturating_add(TWO_ROWS)
        .clamp(
            COMPOSER_MIN_HEIGHT,
            if presentation(model).compact {
                COMPOSER_MAX_HEIGHT_COMPACT
            } else {
                COMPOSER_MAX_HEIGHT
            },
        )
        .min(area.height)
}

/// Startup indicator rectangle.
#[must_use]
pub fn startup_rect(area: Rect) -> Rect {
    let width = area.width.clamp(STARTUP_MIN_WIDTH, STARTUP_MAX_WIDTH);
    let height = area.height.clamp(STARTUP_MIN_HEIGHT, STARTUP_MAX_HEIGHT);
    center(area, width, height)
}

/// Confirmation dialog rectangle.
#[must_use]
pub fn confirmation_rect(area: Rect) -> Rect {
    modal_size(area, MODAL_MAX_WIDTH, CONFIRMATION_MAX_HEIGHT)
}

/// Centered picker and command-palette rectangle.
#[must_use]
pub fn popup_rect(area: Rect) -> Rect {
    modal_size(area, MODAL_MAX_WIDTH, MODAL_MAX_HEIGHT)
}

/// Codex sign-in dialog rectangle.
#[must_use]
pub fn codex_auth_rect(area: Rect) -> Rect {
    modal_size(area, MODAL_MAX_WIDTH, CODEX_AUTH_MAX_HEIGHT)
}

/// Credential and permission dialog rectangle.
#[must_use]
pub fn credential_rect(area: Rect) -> Rect {
    modal_size(area, CREDENTIAL_MAX_WIDTH, CREDENTIAL_MAX_HEIGHT)
}

/// Local user-profile dialog rectangle.
#[must_use]
pub fn user_profile_rect(area: Rect) -> Rect {
    modal_size(area, MODAL_MAX_WIDTH, USER_PROFILE_MAX_HEIGHT)
}

/// Inline Chat command palette rectangle.
#[must_use]
pub fn inline_palette_rect(area: Rect, model: &Model) -> Rect {
    let prompt_height = prompt_surface_height(area, model);
    let height = u16::try_from(command_palette_row_count(model).max(1))
        .unwrap_or(u16::MAX)
        .min(INLINE_PALETTE_MAX_ROWS)
        .saturating_add(INLINE_PALETTE_CHROME_ROWS)
        .min(area.height.saturating_sub(prompt_height));
    Rect::new(
        area.x.saturating_add(INLINE_PALETTE_INSET_X),
        area.bottom()
            .saturating_sub(prompt_height.saturating_add(height)),
        area.width.saturating_sub(INLINE_PALETTE_INSET_X_TOTAL),
        height,
    )
}

/// Result-list rectangle within the anchored command palette panel.
#[must_use]
pub(crate) fn inline_palette_list_rect(panel: Rect) -> Rect {
    Rect::new(
        panel.x.saturating_add(ROW),
        panel.y.saturating_add(PALETTE_MODAL_LIST_TOP_CHROME),
        panel.width.saturating_sub(TWO_ROWS),
        panel.height.saturating_sub(INLINE_PALETTE_CHROME_ROWS),
    )
}

/// Result-list rectangle within the centered command palette modal.
#[must_use]
pub(crate) fn modal_palette_list_rect(popup: Rect) -> Rect {
    Rect::new(
        popup.x.saturating_add(ROW),
        popup.y.saturating_add(PALETTE_MODAL_LIST_TOP_CHROME),
        popup.width.saturating_sub(TWO_ROWS),
        popup.height.saturating_sub(PALETTE_MODAL_CHROME_ROWS),
    )
}

/// Returns a selected-command-centered slice shared by painting and hit tests.
#[must_use]
pub(crate) fn visible_command_palette_rows(model: &Model, visible: u16) -> Vec<CommandPaletteRow> {
    let rows = command_palette_rows(model);
    let visible = usize::from(visible);
    if visible == 0 || rows.is_empty() {
        return Vec::new();
    }
    let selected = model.palette_selection();
    let selected_index = selected
        .and_then(|selected| {
            rows.iter().position(
                |row| matches!(row, CommandPaletteRow::Command(entry) if entry.id == selected),
            )
        })
        .unwrap_or(0);
    let mut start = selected_index
        .saturating_add(1)
        .saturating_sub(visible)
        .min(rows.len().saturating_sub(visible));
    let category_start = rows[..=selected_index]
        .iter()
        .rposition(|row| matches!(row, CommandPaletteRow::Category(_)))
        .unwrap_or(start);
    if selected_index.saturating_sub(category_start) < visible {
        start = start.min(category_start);
    }
    rows.into_iter().skip(start).take(visible).collect()
}

fn command_palette_row_count(model: &Model) -> usize {
    command_palette_rows(model).len()
}

fn command_palette_rows(model: &Model) -> Vec<CommandPaletteRow> {
    let mut rows = Vec::new();
    let mut previous_category = None;
    for entry in model.palette_entries() {
        let category = command_category(entry.id);
        if previous_category != Some(category) {
            rows.push(CommandPaletteRow::Category(category));
            previous_category = Some(category);
        }
        rows.push(CommandPaletteRow::Command(entry));
    }
    rows
}

fn command_category(id: &str) -> &'static str {
    match id {
        "chat" | "sessions" | "profile" | "provider" | "models" | "user" => "Workspace",
        "new" | "session-model" => "Session setup",
        "refresh" | "connect" => "Connections",
        "retry" | "cancel" | "search" | "tools" => "Conversation",
        "settings" | "help" => "Navigation",
        "copy" | "export" => "Artifacts",
        _ => "Commands",
    }
}

/// Provider catalog and connected-profile split.
#[must_use]
pub fn profile_list_detail_areas(area: Rect, model: &Model) -> (Rect, Option<Rect>) {
    if !presentation(model).single_column && area.width >= PROFILE_TWO_PANE_MIN_WIDTH {
        let columns = Split::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(PROFILE_LIST_PERCENT),
                Constraint::Percentage(PROFILE_DETAIL_PERCENT),
            ])
            .split(area);
        (columns[0], Some(columns[1]))
    } else {
        let rows = Split::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(PROFILE_LIST_PERCENT_STACKED),
                Constraint::Percentage(PROFILE_DETAIL_PERCENT_STACKED),
            ])
            .split(area);
        (rows[0], Some(rows[1]))
    }
}

fn overlay_rect(area: Rect, model: &Model) -> Option<Rect> {
    match model.overlay() {
        Some(OverlayKind::UserProfile) => Some(user_profile_rect(area)),
        Some(OverlayKind::Confirmation) => Some(confirmation_rect(area)),
        Some(OverlayKind::ModelPicker) => Some(popup_rect(area)),
        Some(OverlayKind::CommandPalette) if model.route() == Route::Chat => {
            let content = shell_regions(area, model).content;
            Some(inline_palette_rect(content, model))
        }
        Some(OverlayKind::CommandPalette) => Some(popup_rect(area)),
        Some(OverlayKind::SessionCredential | OverlayKind::Permission) => {
            Some(credential_rect(area))
        }
        Some(OverlayKind::ProfileCredential) => Some(popup_rect(area)),
        Some(OverlayKind::TranscriptSearch) | None => None,
    }
}

fn center(host: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        host.x + host.width.saturating_sub(width) / 2,
        host.y + host.height.saturating_sub(height) / 2,
        width.max(ROW),
        height.max(ROW),
    )
}

fn push_shell_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    if let Some(sidebar) = regions.sidebar {
        hits.extend(crate::ui::page::rail_hits(sidebar, model));
    } else if regions.footer.height > 0 {
        push_footer_hits(hits, regions.footer);
    }
}

fn push_footer_hits(hits: &mut Vec<(Rect, MouseAction)>, area: Rect) {
    let profile_width = u16::try_from(FOOTER_PROFILE.width()).unwrap_or(0);
    let gap_width = u16::try_from(FOOTER_GAP.width()).unwrap_or(0);
    let settings_width = u16::try_from(FOOTER_SETTINGS.width()).unwrap_or(0);
    hits.push((
        Rect::new(area.x, area.y, profile_width.min(area.width), area.height),
        MouseAction::Route(Route::Profiles),
    ));
    let settings_x = area
        .x
        .saturating_add(profile_width)
        .saturating_add(gap_width);
    if settings_x < area.right() {
        hits.push((
            Rect::new(
                settings_x,
                area.y,
                settings_width.min(area.right().saturating_sub(settings_x)),
                area.height,
            ),
            MouseAction::Route(Route::Settings),
        ));
    }
}

fn push_route_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    match model.route() {
        Route::Chat => hits.extend(crate::ui::page::chat_content_hits(regions, model)),
        Route::Settings => push_settings_hits(hits, regions, model),
        Route::Sessions => push_session_hits(hits, regions, model),
        Route::Profiles => push_profile_hits(hits, regions.content, model),
        Route::Help => {}
    }
}

fn push_settings_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    if let Some(nav) = regions.settings_nav {
        push_settings_nav_hits(hits, nav, model.settings_workspace.nav_selected);
    }
    let Some(body) = regions.settings_body else {
        return;
    };
    push_settings_row_hits(hits, body, model);
}

fn push_settings_nav_hits(hits: &mut Vec<(Rect, MouseAction)>, area: Rect, selected: usize) {
    let visible = usize::from(area.height);
    let scroll = selected.saturating_sub(visible.saturating_sub(1));
    for index in scroll..SETTINGS_NAV.len().min(scroll.saturating_add(visible)) {
        hits.push((
            Rect::new(
                area.x,
                area.y
                    .saturating_add(u16::try_from(index.saturating_sub(scroll)).unwrap_or(0)),
                area.width,
                ROW,
            ),
            MouseAction::SettingsTab(index),
        ));
    }
}

fn push_settings_row_hits(hits: &mut Vec<(Rect, MouseAction)>, body: Rect, model: &Model) {
    if model.settings_workspace.search_active
        || model.settings_workspace.choice_picker_open
        || model.settings_workspace.detail_open
    {
        return;
    }
    let header_height = if body.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(body.height >= PAGE_HELP_MIN);
    let content_height = body
        .height
        .saturating_sub(header_height)
        .saturating_sub(help_height);
    if content_height == 0 {
        return;
    }
    let content = Rect::new(
        body.x,
        body.y.saturating_add(header_height),
        body.width,
        content_height,
    );
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let rows = SettingsPreference::rows(category);
    let visible = usize::from(content.height.max(ROW));
    let scroll = model
        .settings_workspace
        .selected
        .saturating_sub(visible.saturating_div(3));
    let mut y = content.y;
    for (index, preference) in rows.iter().copied().enumerate().skip(scroll) {
        if y >= content.y && y < content.bottom() && preference.editable() {
            hits.push((
                Rect::new(content.x, y, content.width, ROW),
                MouseAction::SettingsRow(index),
            ));
        }
        let focused = !model.settings_workspace.nav_focus
            && preference.editable()
            && index == model.settings_workspace.selected;
        y = y.saturating_add(if focused { TWO_ROWS } else { ROW });
    }
}

fn push_session_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    let row_y = regions
        .area
        .height
        .saturating_sub(SESSION_ACTION_FROM_BOTTOM);
    let row = Rect::new(regions.content.x, row_y, regions.content.width, ROW);
    let inner_width = regions.content.width.saturating_sub(TWO_ROWS);
    let text = if model.overlay() == Some(OverlayKind::Confirmation) {
        "[ Y Confirm ]  [ N Cancel ]"
    } else if inner_width >= SESSION_HELP_WIDE {
        "[ Open ] Enter  [ Rename ] Ctrl+R  [ Archive ] Ctrl+A  [ Delete ] Ctrl+D  Esc"
    } else {
        "[ Open ]  [ Rename ]  [ Delete ]  Esc"
    };
    for (x, width, label) in bracket_spans(text) {
        let action = match label.as_str() {
            "[ Open ]" => MouseAction::SessionOpen,
            "[ Rename ]" => MouseAction::SessionRename,
            "[ Archive ]" => MouseAction::SessionArchive,
            "[ Delete ]" => MouseAction::SessionDelete,
            "[ Y Confirm ]" => MouseAction::Confirm,
            "[ N Cancel ]" => MouseAction::Cancel,
            _ => continue,
        };
        hits.push((
            Rect::new(row.x.saturating_add(x), row.y, width, ROW),
            action,
        ));
    }
}

fn push_profile_hits(hits: &mut Vec<(Rect, MouseAction)>, area: Rect, model: &Model) {
    let content = profile_center_content_area(model, area);
    let (list_area, detail_area) = profile_list_detail_areas(content, model);
    if let Some(list_inner) = bordered_inner(list_area) {
        let selected = model
            .profile_center
            .choice_selected
            .min(PROVIDER_CHOICES.len().saturating_sub(1));
        let scroll = profile_list_scroll(selected, PROVIDER_CHOICES.len(), list_inner.height);
        for index in 0..PROVIDER_CHOICES.len() {
            let visual =
                u16::try_from(index.saturating_sub(usize::from(scroll))).unwrap_or(u16::MAX);
            let y = list_inner.y.saturating_add(visual);
            if y < list_inner.bottom() {
                hits.push((
                    Rect::new(list_inner.x, y, list_inner.width, ROW),
                    MouseAction::SelectProviderChoice(index),
                ));
            }
        }
    }
    let Some(detail) = detail_area else {
        return;
    };
    let Some(detail_inner) = bordered_inner(detail) else {
        return;
    };
    let profiles = model.filtered_profiles().collect::<Vec<_>>();
    for (index, profile) in profiles.iter().enumerate() {
        let y = detail_inner
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if y < detail_inner.bottom() {
            hits.push((
                Rect::new(detail_inner.x, y, detail_inner.width, ROW),
                MouseAction::SelectProfile(profile.id.clone()),
            ));
        }
    }
    if let Some((first, second)) = profile_detail_button_rows(model, area) {
        let relative = Rect::new(detail_inner.x, first, detail_inner.width, ROW);
        push_profile_primary_buttons(hits, relative, model);
        push_profile_secondary_buttons(
            hits,
            Rect::new(detail_inner.x, second, detail_inner.width, ROW),
            model,
        );
    }
}

fn profile_center_content_area(model: &Model, area: Rect) -> Rect {
    let compact = presentation(model).compact || area.width < PROFILE_COMPACT_WIDTH;
    let notice_height = if model.notice.is_some() && area.height >= PAGE_HEADER_TALL_MIN {
        if compact { ROW } else { TWO_ROWS }
    } else {
        0
    };
    let header_height = if area.height >= PAGE_HEADER_TALL_MIN {
        TWO_ROWS
    } else {
        ROW
    };
    let help_height = u16::from(area.height >= PAGE_HELP_COMFORTABLE);
    let rows = Split::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(ROW),
            Constraint::Length(notice_height),
            Constraint::Length(help_height),
        ])
        .split(area);
    rows[1]
}

fn bordered_inner(area: Rect) -> Option<Rect> {
    if area.width <= TWO_ROWS || area.height <= TWO_ROWS {
        return None;
    }
    Some(Rect::new(
        area.x.saturating_add(ROW),
        area.y.saturating_add(ROW),
        area.width.saturating_sub(TWO_ROWS),
        area.height.saturating_sub(TWO_ROWS),
    ))
}

fn profile_list_scroll(selected: usize, count: usize, visible: u16) -> u16 {
    let visible = usize::from(visible.max(ROW));
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(count.saturating_sub(visible));
    u16::try_from(start).unwrap_or(u16::MAX)
}

fn profile_detail_button_rows(model: &Model, area: Rect) -> Option<(u16, u16)> {
    model.selected_profile()?;
    let content = profile_center_content_area(model, area);
    let (_, Some(detail)) = profile_list_detail_areas(content, model) else {
        return None;
    };
    let inner = bordered_inner(detail)?;
    let second = inner.bottom().saturating_sub(ROW);
    Some((second.saturating_sub(ROW), second))
}

fn push_profile_primary_buttons(hits: &mut Vec<(Rect, MouseAction)>, row: Rect, model: &Model) {
    let buttons = [
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
    ];
    hits.extend(ButtonRow::new(model.theme(), &buttons).regions(row));
}

fn push_profile_secondary_buttons(hits: &mut Vec<(Rect, MouseAction)>, row: Rect, model: &Model) {
    let buttons = [
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
    ];
    hits.extend(ButtonRow::new(model.theme(), &buttons).regions(row));
}

fn push_overlay_hits(
    hits: &mut Vec<(Rect, MouseAction)>,
    area: Rect,
    popup: Rect,
    overlay: OverlayKind,
    model: &Model,
) {
    match overlay {
        OverlayKind::UserProfile => {
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::Confirmation => {
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::ModelPicker => {
            push_picker_hits(hits, popup, model);
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::CommandPalette if model.route() == Route::Chat => {
            let content = shell_regions(area, model).content;
            push_inline_palette_hits(hits, content, model);
        }
        OverlayKind::CommandPalette => {
            push_palette_hits(hits, popup, model);
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::SessionCredential => {
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::ProfileCredential => {
            let action = model
                .profile_center
                .credential
                .as_ref()
                .map(|editor| match editor.action {
                    ProfileCredentialAction::Save => "Save",
                    ProfileCredentialAction::Replace => "Replace",
                })
                .unwrap_or("Save");
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
            push_modal_buttons(hits, popup, model, &buttons);
        }
        OverlayKind::Permission => {
            if model.answering_permissions.is_empty() {
                let buttons = [
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
                ];
                push_modal_buttons(hits, popup, model, &buttons);
            }
        }
        OverlayKind::TranscriptSearch => {}
    }
}

fn push_modal_buttons(
    hits: &mut Vec<(Rect, MouseAction)>,
    popup: Rect,
    model: &Model,
    buttons: &[Button<MouseAction>],
) {
    let footer = Rect::new(
        popup.x.saturating_add(ROW),
        popup.bottom().saturating_sub(TWO_ROWS),
        popup.width.saturating_sub(TWO_ROWS),
        ROW,
    );
    hits.extend(ButtonRow::new(model.theme(), buttons).regions(footer));
}

fn push_codex_login_hit(hits: &mut Vec<(Rect, MouseAction)>, area: Rect) {
    let popup = codex_auth_rect(area);
    let action_row = popup.y.saturating_add(CODEX_ACTION_ROW_OFFSET);
    let width = popup.width.saturating_sub(TWO_ROWS);
    if width > 0 {
        hits.push((
            Rect::new(popup.x.saturating_add(ROW), action_row, width, ROW),
            MouseAction::CodexLogin,
        ));
    }
}

fn push_picker_hits(hits: &mut Vec<(Rect, MouseAction)>, popup: Rect, model: &Model) {
    let inner_height = popup.height.saturating_sub(TWO_ROWS);
    let stale_height = u16::from(
        matches!(
            &*model.catalog,
            CatalogProjection::Ready { stale: true, .. }
        ) && inner_height >= PAGE_HELP_MIN,
    );
    let help_height = u16::from(inner_height >= PAGE_HELP_COMFORTABLE);
    let list_height = inner_height.saturating_sub(ROW + stale_height + help_height);
    let list_start = popup.y.saturating_add(PALETTE_MODAL_LIST_TOP_CHROME);
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
    for (offset, summary) in models.iter().skip(start).take(visible).enumerate() {
        let y = list_start.saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        hits.push((
            Rect::new(popup.x, y, popup.width, ROW),
            MouseAction::PickerSelect(summary.model.clone()),
        ));
    }
}

fn push_palette_hits(hits: &mut Vec<(Rect, MouseAction)>, popup: Rect, model: &Model) {
    let list = modal_palette_list_rect(popup);
    push_palette_entries(hits, list, model);
}

fn push_inline_palette_hits(hits: &mut Vec<(Rect, MouseAction)>, content: Rect, model: &Model) {
    let list = inline_palette_rect(content, model);
    if list.width == 0 || list.height == 0 {
        return;
    }
    let list = inline_palette_list_rect(list);
    push_palette_entries(hits, list, model);
}

fn push_palette_entries(hits: &mut Vec<(Rect, MouseAction)>, list: Rect, model: &Model) {
    for (offset, row) in visible_command_palette_rows(model, list.height)
        .into_iter()
        .enumerate()
    {
        if let CommandPaletteRow::Command(entry) = row {
            let y = list
                .y
                .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
            hits.push((
                Rect::new(list.x, y, list.width, ROW),
                MouseAction::PaletteRun(entry.id.to_owned()),
            ));
        }
    }
}

fn filtered_models(model: &Model) -> Vec<&crate::model::ModelSummary> {
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

fn bracket_spans(text: &str) -> Vec<(u16, u16, String)> {
    let mut spans = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find('[') {
        let start = search_from + rel;
        let Some(end_rel) = text[start..].find(']') else {
            break;
        };
        let end = start + end_rel;
        let label = text[start..=end].to_owned();
        let x = u16::try_from(text[..start].width()).unwrap_or(u16::MAX);
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        spans.push((x, width, label));
        search_from = end.saturating_add(1);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::{Frame, SETTINGS_NAV};
    use crate::model::MouseAction;
    use ratatui::layout::Rect;

    #[test]
    fn reverse_scan_prefers_the_later_hit() {
        let frame = Frame {
            regions: super::shell_regions(Rect::new(0, 0, 40, 12), &empty_shell_model()),
            hits: vec![
                (Rect::new(0, 0, 40, 12), MouseAction::FocusTranscript),
                (Rect::new(0, 10, 40, 2), MouseAction::FocusComposer),
            ],
        };
        assert_eq!(frame.hit_at(2, 10), Some(MouseAction::FocusComposer));
        assert_eq!(frame.hit_at(2, 0), Some(MouseAction::FocusTranscript));
    }

    #[test]
    fn settings_nav_labels_are_stable() {
        assert_eq!(
            SETTINGS_NAV,
            [
                "Appearance",
                "Chat & Composer",
                "Accessibility",
                "Providers",
                "Models & Thinking",
                "Profile",
                "Sessions & Data",
                "Shortcuts",
                "About",
            ]
        );
    }

    fn empty_shell_model() -> crate::model::Model {
        use std::sync::Arc;

        use crate::model::{CatalogProjection, Model, SessionProjection, SessionsProjection};
        use autoharness_domain::{ModelId, ModelRef, ProviderId};

        let model = ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider"),
            ModelId::new("models/test").expect("model"),
        );
        Model::new(
            Arc::new(SessionProjection {
                session_id: "s".to_owned(),
                revision: 1,
                selected_model: Some(model),
                transcript: Vec::new(),
                permission_requests: Vec::new(),
            }),
            Arc::new(SessionsProjection::default()),
            Arc::new(CatalogProjection::Loading),
        )
    }
}
