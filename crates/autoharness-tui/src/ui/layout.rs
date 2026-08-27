//! One layout pass produces named rectangles and ordered hit regions.

use autoharness_settings::{Density, Layout as PreferenceLayout};
use ratatui::layout::{Constraint, Direction, Layout as Split, Position, Rect};
use unicode_width::UnicodeWidthStr;

use super::metrics::{
    CODEX_ACTION_ROW_OFFSET, CODEX_AUTH_MAX_HEIGHT, COMPACT_CHAT_MIN_HEIGHT,
    COMPACT_CHAT_MIN_WIDTH, COMPOSER_CARET_WIDTH, COMPOSER_MAX_HEIGHT, COMPOSER_MAX_HEIGHT_COMPACT,
    COMPOSER_MIN_HEIGHT, CONFIRMATION_FULL_HEIGHT, CONFIRMATION_FULL_WIDTH, CONFIRMATION_MARGIN_X,
    CONFIRMATION_MARGIN_Y, CONFIRMATION_MAX_HEIGHT, CREDENTIAL_MAX_HEIGHT, CREDENTIAL_MAX_WIDTH,
    INLINE_PALETTE_INSET_X, INLINE_PALETTE_INSET_X_TOTAL, INLINE_PALETTE_MAX_ROWS, MODAL_MAX_WIDTH,
    OVERLAY_LIST_TOP_CHROME, PAGE_HEADER_TALL_MIN, PAGE_HELP_COMFORTABLE, PAGE_HELP_MIN,
    POPUP_HEIGHT_DENOMINATOR, POPUP_HEIGHT_NUMERATOR, POPUP_MIN_HEIGHT, POPUP_MIN_WIDTH,
    POPUP_WIDTH_DENOMINATOR, POPUP_WIDTH_NUMERATOR, PROFILE_COMPACT_WIDTH,
    PROFILE_DETAIL_CHROME_ROWS, PROFILE_DETAIL_PERCENT, PROFILE_DETAIL_PERCENT_STACKED,
    PROFILE_LIST_PERCENT, PROFILE_LIST_PERCENT_STACKED, PROFILE_TWO_PANE_MIN_WIDTH,
    PROMPT_INSET_MIN_WIDTH, ROW, SESSION_ACTION_FROM_BOTTOM, SESSION_HELP_WIDE,
    SETTINGS_BODY_INSET_X, SETTINGS_BODY_INSET_X_TOTAL, SETTINGS_BODY_INSET_Y,
    SETTINGS_BODY_INSET_Y_TOTAL, SETTINGS_NAV_COMPACT_WIDTH, SIDEBAR_SESSION_CHROME, SIDEBAR_WIDTH,
    STARTUP_MAX_HEIGHT, STARTUP_MAX_WIDTH, STARTUP_MIN_HEIGHT, STARTUP_MIN_WIDTH, TWO_ROWS,
    USER_PROFILE_BUTTON_LINE, USER_PROFILE_FULL_HEIGHT, USER_PROFILE_FULL_WIDTH,
    USER_PROFILE_MARGIN_Y, USER_PROFILE_MAX_HEIGHT, wide_shell,
};
use crate::model::{
    CatalogProjection, Model, MouseAction, OverlayKind, PROVIDER_CHOICES, ProfileConnectionState,
    ProviderChoice, ProviderKindLabel, Route, SettingsPreference,
};

/// Settings tab labels, in the same order as `SettingsTab` indices.
pub const SETTINGS_NAV: [&str; 4] = ["Settings", "Providers", "Profile", "Models"];

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
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(ROW)])
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
            overlay: None,
            startup: None,
        }
    } else {
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
            overlay: None,
            startup: None,
        }
    }
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
        let inner_height = surface.height.saturating_sub(ROW);
        regions.composer_metadata = Some(Rect::new(surface.x, surface.y, surface.width, ROW));
        let _ = (inner_height, COMPOSER_CARET_WIDTH);
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
    if inner.height >= TWO_ROWS {
        regions.settings_nav = Some(Rect::new(inner.x, inner.y, inner.width, ROW));
    }
    regions.settings_body = Some(settings_body_area(content));
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
    if area.width <= CONFIRMATION_FULL_WIDTH || area.height <= CONFIRMATION_FULL_HEIGHT {
        return area;
    }
    let width = area
        .width
        .saturating_sub(CONFIRMATION_MARGIN_X)
        .clamp(ROW, MODAL_MAX_WIDTH);
    let height = area
        .height
        .saturating_sub(CONFIRMATION_MARGIN_Y)
        .clamp(ROW, CONFIRMATION_MAX_HEIGHT);
    center(area, width, height)
}

/// Centered picker and command-palette rectangle.
#[must_use]
pub fn popup_rect(area: Rect) -> Rect {
    if area.width < POPUP_MIN_WIDTH || area.height < POPUP_MIN_HEIGHT {
        return area;
    }
    let width = area.width.saturating_mul(POPUP_WIDTH_NUMERATOR) / POPUP_WIDTH_DENOMINATOR;
    let height = area.height.saturating_mul(POPUP_HEIGHT_NUMERATOR) / POPUP_HEIGHT_DENOMINATOR;
    center(area, width.max(ROW), height.max(ROW))
}

/// Codex sign-in dialog rectangle.
#[must_use]
pub fn codex_auth_rect(area: Rect) -> Rect {
    if area.width <= CONFIRMATION_FULL_WIDTH || area.height <= CONFIRMATION_FULL_HEIGHT {
        return area;
    }
    let width = area
        .width
        .saturating_sub(CONFIRMATION_MARGIN_X)
        .min(MODAL_MAX_WIDTH);
    let height = area
        .height
        .saturating_sub(CONFIRMATION_MARGIN_Y)
        .min(CODEX_AUTH_MAX_HEIGHT);
    center(area, width.max(ROW), height.max(ROW))
}

/// Credential and permission dialog rectangle.
#[must_use]
pub fn credential_rect(area: Rect) -> Rect {
    if area.width <= CONFIRMATION_FULL_WIDTH || area.height <= CONFIRMATION_FULL_HEIGHT {
        return area;
    }
    let width = area
        .width
        .saturating_sub(CONFIRMATION_MARGIN_X)
        .min(CREDENTIAL_MAX_WIDTH);
    let height = area
        .height
        .saturating_sub(CONFIRMATION_MARGIN_Y)
        .min(CREDENTIAL_MAX_HEIGHT);
    center(area, width.max(ROW), height.max(ROW))
}

/// Local user-profile dialog rectangle.
#[must_use]
pub fn user_profile_rect(area: Rect) -> Rect {
    if area.width <= USER_PROFILE_FULL_WIDTH || area.height <= USER_PROFILE_FULL_HEIGHT {
        return area;
    }
    let width = area
        .width
        .saturating_sub(CONFIRMATION_MARGIN_X)
        .min(MODAL_MAX_WIDTH);
    let height = area
        .height
        .saturating_sub(USER_PROFILE_MARGIN_Y)
        .min(USER_PROFILE_MAX_HEIGHT);
    center(area, width.max(ROW), height.max(ROW))
}

/// Inline Chat command palette rectangle.
#[must_use]
pub fn inline_palette_rect(area: Rect, model: &Model) -> Rect {
    let prompt_height = prompt_surface_height(area, model);
    let height = u16::try_from(model.palette_entries().len())
        .unwrap_or(u16::MAX)
        .min(INLINE_PALETTE_MAX_ROWS)
        .min(area.height.saturating_sub(prompt_height));
    Rect::new(
        area.x.saturating_add(INLINE_PALETTE_INSET_X),
        area.bottom()
            .saturating_sub(prompt_height.saturating_add(height)),
        area.width.saturating_sub(INLINE_PALETTE_INSET_X_TOTAL),
        height,
    )
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
        let footer_row = sidebar.bottom().saturating_sub(ROW);
        push_footer_hits(hits, Rect::new(sidebar.x, footer_row, sidebar.width, ROW));
        let sessions_start = sidebar.y.saturating_add(ROW);
        let session_count = model
            .sessions
            .sessions
            .len()
            .min(sidebar_session_limit(sidebar));
        let sessions_end =
            sessions_start.saturating_add(u16::try_from(session_count).unwrap_or(u16::MAX));
        if sessions_end > sessions_start {
            hits.push((
                Rect::new(
                    sidebar.x,
                    sessions_start,
                    sidebar.width,
                    sessions_end.saturating_sub(sessions_start),
                ),
                MouseAction::Route(Route::Sessions),
            ));
        }
    } else if regions.footer.height > 0 {
        push_footer_hits(hits, regions.footer);
    }
}

fn sidebar_session_limit(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(SIDEBAR_SESSION_CHROME)).max(1)
}

fn push_footer_hits(hits: &mut Vec<(Rect, MouseAction)>, area: Rect) {
    let profile_width = u16::try_from(FOOTER_PROFILE.width()).unwrap_or(0);
    let gap_width = u16::try_from(FOOTER_GAP.width()).unwrap_or(0);
    let settings_width = u16::try_from(FOOTER_SETTINGS.width()).unwrap_or(0);
    hits.push((
        Rect::new(area.x, area.y, profile_width.min(area.width), area.height),
        MouseAction::SettingsTab(2),
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
            MouseAction::SettingsTab(0),
        ));
    }
}

fn push_route_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    match model.route() {
        Route::Chat => push_chat_hits(hits, regions),
        Route::Settings => push_settings_hits(hits, regions, model),
        Route::Sessions => push_session_hits(hits, regions, model),
        Route::Profiles => push_profile_hits(hits, regions.content, model),
        Route::Help => {}
    }
}

fn push_chat_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects) {
    if let Some(transcript) = regions
        .transcript
        .filter(|rect| rect.width > 0 && rect.height > 0)
    {
        hits.push((transcript, MouseAction::FocusTranscript));
    }
    if let Some(composer) = regions
        .composer
        .filter(|rect| rect.width > 0 && rect.height > 0)
    {
        hits.push((composer, MouseAction::FocusComposer));
    }
    if let Some(metadata) = regions
        .composer_metadata
        .filter(|rect| rect.width > 0 && rect.height > 0)
    {
        hits.push((metadata, MouseAction::ChatModels));
    }
}

fn push_settings_hits(hits: &mut Vec<(Rect, MouseAction)>, regions: &NamedRects, model: &Model) {
    if let Some(nav) = regions.settings_nav {
        push_settings_nav_hits(hits, nav);
    }
    let Some(body) = regions.settings_body else {
        return;
    };
    match model.settings_workspace.nav_selected {
        1 => push_profile_hits(hits, body, model),
        2 => hits.push((body, MouseAction::OpenUserProfile)),
        0 => push_settings_row_hits(hits, body, model),
        _ => {}
    }
}

fn push_settings_nav_hits(hits: &mut Vec<(Rect, MouseAction)>, area: Rect) {
    let compact = area.width < SETTINGS_NAV_COMPACT_WIDTH;
    let padding = if compact { 0 } else { TWO_ROWS };
    let gap = if compact { ROW } else { TWO_ROWS };
    let mut offset = area.x;
    for (index, label) in SETTINGS_NAV.iter().enumerate() {
        let width =
            u16::try_from(label.len().saturating_add(usize::from(padding))).unwrap_or(u16::MAX);
        hits.push((
            Rect::new(offset, area.y, width, area.height),
            MouseAction::SettingsTab(index),
        ));
        offset = offset.saturating_add(width).saturating_add(gap);
    }
}

fn push_settings_row_hits(hits: &mut Vec<(Rect, MouseAction)>, body: Rect, model: &Model) {
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
    let selected_line = settings_preference_line(model.settings_workspace.selected);
    let visible = usize::from(content.height.max(ROW));
    let scroll = u16::try_from(usize::from(selected_line).saturating_sub(visible / 3)).unwrap_or(0);
    for index in 0..SettingsPreference::ALL.len() {
        let line = settings_preference_line(index);
        let y = content.y.saturating_add(line.saturating_sub(scroll));
        if y >= content.y && y < content.bottom() {
            hits.push((
                Rect::new(content.x, y, content.width, ROW),
                MouseAction::SettingsRow(index),
            ));
        }
    }
}

fn settings_preference_line(index: usize) -> u16 {
    match SettingsPreference::at(index) {
        SettingsPreference::DisplayLabel => 1,
        SettingsPreference::Provider => 3,
        SettingsPreference::Profile => 4,
        SettingsPreference::Credential => 5,
        SettingsPreference::Source => 6,
        SettingsPreference::Model => 11,
        SettingsPreference::Mode => 12,
        SettingsPreference::Approvals => 16,
        SettingsPreference::Retention => 18,
        SettingsPreference::ThemePreset => 21,
        SettingsPreference::ColorMode => 22,
        SettingsPreference::GlyphMode => 23,
        SettingsPreference::PromptStatusDetail => 25,
        SettingsPreference::ReducedMotion => 27,
        SettingsPreference::Density => 28,
        SettingsPreference::Logging => 30,
        SettingsPreference::Layout => 32,
        SettingsPreference::TerminalTimestampStyle => 33,
        SettingsPreference::ComposerSubmitBehavior => 34,
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
    let selected = model.selected_profile()?;
    let content = profile_center_content_area(model, area);
    let (_, Some(detail)) = profile_list_detail_areas(content, model) else {
        return None;
    };
    let mut lines = u16::try_from(model.filtered_profiles().count())
        .unwrap_or(u16::MAX)
        .saturating_add(PROFILE_DETAIL_CHROME_ROWS);
    if selected.kind == ProviderKindLabel::Router {
        lines = lines.saturating_add(ROW);
        if !selected.project.is_empty() {
            lines = lines.saturating_add(ROW);
        }
        if !selected.auth_header.is_empty() {
            lines = lines.saturating_add(ROW);
        }
    }
    if matches!(selected.connection, ProfileConnectionState::Failed(_)) {
        lines = lines.saturating_add(ROW);
    }
    if model.profiles().pending_recovery > 0 {
        lines = lines.saturating_add(ROW);
    }
    let first = detail
        .y
        .saturating_add(ROW)
        .saturating_add(lines)
        .saturating_add(ROW);
    Some((first, first.saturating_add(ROW)))
}

fn push_profile_primary_buttons(hits: &mut Vec<(Rect, MouseAction)>, row: Rect, model: &Model) {
    let text = if model
        .selected_profile()
        .is_some_and(|profile| profile.kind == ProviderKindLabel::CodexCli)
    {
        "[ Sign in ] [ Test ] [ Model ]"
    } else {
        "[ API key ] [ Test ] [ Model ]"
    };
    for (x, width, label) in bracket_spans(text) {
        let action = match label.as_str() {
            "[ Sign in ]" | "[ API key ]" => MouseAction::ProfileCredential,
            "[ Test ]" => MouseAction::ProfileTest,
            "[ Model ]" => MouseAction::ProfileDefaultModel,
            _ => continue,
        };
        hits.push((
            Rect::new(row.x.saturating_add(x), row.y, width, ROW),
            action,
        ));
    }
}

fn push_profile_secondary_buttons(hits: &mut Vec<(Rect, MouseAction)>, row: Rect) {
    for (x, width, label) in bracket_spans("[ Disconnect ] [ Remove ]") {
        let action = match label.as_str() {
            "[ Disconnect ]" => MouseAction::ProfileDisconnect,
            "[ Remove ]" => MouseAction::ProfileDelete,
            _ => continue,
        };
        hits.push((
            Rect::new(row.x.saturating_add(x), row.y, width, ROW),
            action,
        ));
    }
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
            let button_row = popup
                .y
                .saturating_add(USER_PROFILE_BUTTON_LINE)
                .saturating_add(ROW);
            if button_row < popup.bottom() {
                let half = popup.width / TWO_ROWS;
                hits.push((
                    Rect::new(popup.x, button_row, half, ROW),
                    MouseAction::UserProfileSave,
                ));
                hits.push((
                    Rect::new(
                        popup.x.saturating_add(half),
                        button_row,
                        popup.width.saturating_sub(half),
                        ROW,
                    ),
                    MouseAction::UserProfileCancel,
                ));
            }
        }
        OverlayKind::Confirmation => {
            push_split_buttons(hits, popup, MouseAction::Confirm, MouseAction::Cancel);
        }
        OverlayKind::ModelPicker => push_picker_hits(hits, popup, model),
        OverlayKind::CommandPalette if model.route() == Route::Chat => {
            let content = shell_regions(area, model).content;
            push_inline_palette_hits(hits, content, model);
        }
        OverlayKind::CommandPalette => push_palette_hits(hits, popup, model),
        OverlayKind::SessionCredential => push_split_buttons(
            hits,
            popup,
            MouseAction::CredentialSubmit,
            MouseAction::CredentialCancel,
        ),
        OverlayKind::ProfileCredential => push_split_buttons(
            hits,
            popup,
            MouseAction::ProfileCredentialSubmit,
            MouseAction::ProfileCredentialCancel,
        ),
        OverlayKind::Permission => push_split_buttons(
            hits,
            popup,
            MouseAction::PermissionAllow,
            MouseAction::PermissionDeny,
        ),
        OverlayKind::TranscriptSearch => {}
    }
}

fn push_split_buttons(
    hits: &mut Vec<(Rect, MouseAction)>,
    popup: Rect,
    primary: MouseAction,
    secondary: MouseAction,
) {
    let action_row = popup.bottom().saturating_sub(TWO_ROWS);
    let half = popup.width / TWO_ROWS;
    hits.push((Rect::new(popup.x, action_row, half, ROW), primary));
    hits.push((
        Rect::new(
            popup.x.saturating_add(half),
            action_row,
            popup.width.saturating_sub(half),
            ROW,
        ),
        secondary,
    ));
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
    let list_start = popup.y.saturating_add(OVERLAY_LIST_TOP_CHROME);
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
    let inner_height = popup.height.saturating_sub(TWO_ROWS);
    let help_height = u16::from(inner_height >= PAGE_HELP_MIN);
    let list_height = inner_height.saturating_sub(ROW + help_height);
    let list_start = popup.y.saturating_add(OVERLAY_LIST_TOP_CHROME);
    push_palette_entries(hits, popup.x, popup.width, list_start, list_height, model);
}

fn push_inline_palette_hits(hits: &mut Vec<(Rect, MouseAction)>, content: Rect, model: &Model) {
    let list = inline_palette_rect(content, model);
    if list.width == 0 || list.height == 0 {
        return;
    }
    push_palette_entries(hits, list.x, list.width, list.y, list.height, model);
}

fn push_palette_entries(
    hits: &mut Vec<(Rect, MouseAction)>,
    x: u16,
    width: u16,
    list_start: u16,
    list_height: u16,
    model: &Model,
) {
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
    for (offset, entry) in entries.iter().skip(start).take(visible).enumerate() {
        let y = list_start.saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        hits.push((
            Rect::new(x, y, width, ROW),
            MouseAction::PaletteRun(entry.id.to_owned()),
        ));
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
        assert_eq!(SETTINGS_NAV, ["Settings", "Providers", "Profile", "Models"]);
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
