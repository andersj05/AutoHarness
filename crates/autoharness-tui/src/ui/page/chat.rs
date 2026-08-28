//! Chat workspace: conversation, status bar, composer, and navigation rail.

use autoharness_settings::{GlyphMode, PromptStatusDetail};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::model::{
    AttemptKey, AttemptStatus, Focus, Model, MouseAction, Notice, PendingKind, Route,
    TranscriptItem,
};
use crate::text::display_safe;
use crate::ui::component::paint::{self, wrap_cells};
use crate::ui::component::{
    Button, ButtonRow, ButtonVariant, Callout, Chip, ChipVariant, MessageBlock, StatusBar,
    StatusSegment, ToolCard,
};
use crate::ui::icon::Icon;
use crate::ui::layout::{NamedRects, prompt_surface_height};
use crate::ui::metrics::{
    PROMPT_INSET_MIN_WIDTH, ROW, SEARCH_LABEL_WIDTH, SEARCH_STATUS_MIN_WIDTH, SIDEBAR_BRAND_ROWS,
    SIDEBAR_FOOTER_ROWS, SIDEBAR_GROUP_GAP, SIDEBAR_LABEL_INSET, SIDEBAR_RECENT_HEADER,
    SIDEBAR_SESSION_CHROME, SIDEBAR_WORKSPACE_ROWS, STREAMING_WAVE_CELLS, TWO_ROWS,
};
use crate::ui::tokens::Token;
use crate::ui::{Theme, normalized_t};

/// Primary rail destinations with their icons.
pub const RAIL_ROUTES: [(Route, Icon); 5] = [
    (Route::Chat, Icon::RouteChat),
    (Route::Sessions, Icon::RouteSessions),
    (Route::Profiles, Icon::RouteProviders),
    (Route::Settings, Icon::RouteSettings),
    (Route::Help, Icon::RouteHelp),
];

/// Renders Chat transcript, notices, search, and the composer.
pub fn render(frame: &mut Frame<'_>, regions: &NamedRects, model: &Model) {
    if let Some(transcript) = regions.transcript {
        render_transcript(frame, transcript, model);
    }
    if let Some(notice) = regions.notice {
        render_notice(frame, notice, model);
    }
    if let Some(search) = regions.search {
        render_search(frame, search, model);
    }
    if let Some(composer) = regions.composer {
        render_composer(frame, composer, model);
    }
}

/// Renders the shared navigation rail used by every primary route.
pub fn render_rail(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let theme = model.theme();
    let icons = theme.icons();
    paint::clear_transparent(buf, area, theme);
    let divider = Rect::new(area.right().saturating_sub(ROW), area.y, ROW, area.height);
    for row in 0..divider.height {
        paint::put(
            buf,
            divider.x,
            divider.y.saturating_add(row),
            ROW,
            icons.vertical_rule(),
            theme.gradient_style(normalized_t(row, divider.height.max(ROW))),
        );
    }
    let inner = Rect::new(area.x, area.y, area.width.saturating_sub(ROW), area.height);
    if inner.width == 0 {
        return;
    }
    let brand = if icons.mode() == GlyphMode::NerdFont {
        format!("{} AutoHarness", icons.glyph(Icon::Brand))
    } else {
        "AutoHarness".to_owned()
    };
    paint_gradient_text(
        buf,
        inner.x.saturating_add(ROW),
        inner.y,
        inner.width.saturating_sub(ROW),
        &brand,
        theme,
    );
    let mut y = inner.y.saturating_add(SIDEBAR_BRAND_ROWS);
    for (route, icon) in RAIL_ROUTES {
        if y >= inner.bottom() {
            break;
        }
        let selected = model.route() == route;
        let style = if selected {
            theme.filled(Token::SurfaceSelected)
        } else {
            theme.style(Token::TextSecondary)
        };
        paint::put(buf, inner.x, y, icons.width(icon), icons.glyph(icon), style);
        paint::put(
            buf,
            inner
                .x
                .saturating_add(icons.width(icon).saturating_add(ROW)),
            y,
            inner.width,
            route.label(),
            style,
        );
        y = y.saturating_add(ROW);
    }
    y = y.saturating_add(SIDEBAR_GROUP_GAP);
    if y < inner.bottom() {
        paint::put(
            buf,
            inner.x.saturating_add(ROW),
            y,
            inner.width,
            "Recent",
            theme.style(Token::TextMuted),
        );
        y = y.saturating_add(SIDEBAR_RECENT_HEADER);
    }
    let session_limit = sidebar_session_limit(inner);
    let label_width = inner.width.saturating_sub(SIDEBAR_LABEL_INSET);
    for entry in model.sessions.sessions.iter().take(session_limit) {
        if y.saturating_add(SIDEBAR_WORKSPACE_ROWS.saturating_add(SIDEBAR_FOOTER_ROWS))
            >= inner.bottom()
        {
            break;
        }
        let active = entry.active || entry.session_id == model.session.session_id;
        let marker = if active {
            icons.glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        let style = if active {
            theme.filled(Token::SurfaceSelected)
        } else {
            theme.style(Token::TextPrimary)
        };
        let label =
            paint::ellipsize_words_with(&display_safe(&entry.title), label_width, icons.ellipsis());
        paint::put(
            buf,
            inner.x,
            y,
            inner.width,
            &format!("{marker} {label}"),
            style,
        );
        y = y.saturating_add(ROW);
    }
    if y.saturating_add(SIDEBAR_WORKSPACE_ROWS.saturating_add(SIDEBAR_FOOTER_ROWS)) < inner.bottom()
    {
        paint::put(
            buf,
            inner.x.saturating_add(ROW),
            y,
            inner.width,
            "Workspace",
            theme.style(Token::TextMuted),
        );
        y = y.saturating_add(ROW);
        paint::put(
            buf,
            inner.x.saturating_add(ROW),
            y,
            inner.width,
            &workspace_label(&model.profiles().user.workspace),
            theme.style(Token::TextPrimary),
        );
    }
    let footer = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(SIDEBAR_FOOTER_ROWS),
        inner.width,
        SIDEBAR_FOOTER_ROWS,
    );
    let settings_style = if model.route() == Route::Settings {
        theme.filled(Token::SurfaceSelected)
    } else {
        theme.style(Token::TextPrimary)
    };
    paint::put(
        buf,
        footer.x.saturating_add(ROW),
        footer.y,
        footer.width,
        "Settings",
        settings_style,
    );
}

/// Paint-order hits for the shared rail.
#[must_use]
pub fn rail_hits(area: Rect, model: &Model) -> Vec<(Rect, MouseAction)> {
    let mut hits = Vec::new();
    let inner = Rect::new(area.x, area.y, area.width.saturating_sub(ROW), area.height);
    let mut y = inner.y.saturating_add(SIDEBAR_BRAND_ROWS);
    for (route, _) in RAIL_ROUTES {
        hits.push((
            Rect::new(inner.x, y, inner.width, ROW),
            MouseAction::Route(route),
        ));
        y = y.saturating_add(ROW);
    }
    y = y
        .saturating_add(SIDEBAR_GROUP_GAP)
        .saturating_add(SIDEBAR_RECENT_HEADER);
    let session_count = model
        .sessions
        .sessions
        .len()
        .min(sidebar_session_limit(inner));
    if session_count > 0 {
        hits.push((
            Rect::new(
                inner.x,
                y,
                inner.width,
                u16::try_from(session_count).unwrap_or(u16::MAX),
            ),
            MouseAction::Route(Route::Sessions),
        ));
    }
    hits.push((
        Rect::new(
            inner.x,
            inner.bottom().saturating_sub(SIDEBAR_FOOTER_ROWS),
            inner.width,
            SIDEBAR_FOOTER_ROWS,
        ),
        MouseAction::Route(Route::Settings),
    ));
    hits
}

/// Paint-order hits for Chat surfaces, including failure callout buttons.
#[must_use]
pub fn content_hits(regions: &NamedRects, model: &Model) -> Vec<(Rect, MouseAction)> {
    let mut hits = Vec::new();
    if model.route() != Route::Chat {
        return hits;
    }
    if let Some(transcript) = regions
        .transcript
        .filter(|rect| rect.width > 0 && rect.height > 0)
    {
        hits.push((transcript, MouseAction::FocusTranscript));
        hits.extend(recovery_hits(transcript, model));
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
    hits
}

/// Rail hits plus Chat content hits.
#[must_use]
pub fn collect_hits(regions: &NamedRects, model: &Model) -> Vec<(Rect, MouseAction)> {
    let mut hits = Vec::new();
    if let Some(sidebar) = regions.sidebar {
        hits.extend(rail_hits(sidebar, model));
    }
    hits.extend(content_hits(regions, model));
    hits
}

/// Visible transcript lines used by search and clipboard copy.
#[must_use]
pub fn display_lines(model: &Model) -> Vec<String> {
    if model.session.transcript.is_empty() {
        return Vec::new();
    }
    model
        .session
        .transcript
        .iter()
        .flat_map(|item| item_lines(model, item))
        .collect()
}

/// Visible composer rectangle within the scrollable conversation viewport.
#[must_use]
pub(crate) fn composer_rect(area: Rect, model: &Model) -> Option<Rect> {
    if area.width == 0
        || area.height == 0
        || model.search_open() && model.search_pinned_row.is_some()
    {
        return None;
    }
    if !model.transcript.follow_tail && model.transcript.rows_from_bottom != 0 {
        return None;
    }
    let composer_height = prompt_surface_height(area, model);
    let viewport = usize::from(area.height);
    let available = viewport.saturating_sub(usize::from(composer_height));
    let y_offset = transcript_tail_height(model, conversation_inner_width(area), available);
    Some(Rect::new(
        area.x,
        area.y
            .saturating_add(u16::try_from(y_offset).unwrap_or(u16::MAX)),
        area.width,
        composer_height,
    ))
}

fn conversation_inner_width(area: Rect) -> u16 {
    let inset = u16::from(area.width >= PROMPT_INSET_MIN_WIDTH);
    area.width.saturating_sub(inset.saturating_mul(TWO_ROWS))
}

fn transcript_tail_height(model: &Model, width: u16, limit: usize) -> usize {
    if limit == 0 {
        return 0;
    }
    let mut total = 0_usize;
    for item in model.session.transcript.iter().rev() {
        total = total
            .saturating_add(usize::from(item_height(model, item, width)))
            .saturating_add(usize::from(ROW));
        if total >= limit {
            return limit;
        }
    }
    total
}

fn sidebar_session_limit(inner: Rect) -> usize {
    usize::from(inner.height.saturating_sub(SIDEBAR_SESSION_CHROME)).max(1)
}

fn item_lines(model: &Model, item: &TranscriptItem) -> Vec<String> {
    match item {
        TranscriptItem::User { text, .. } => vec!["You".to_owned(), display_safe(text)],
        TranscriptItem::Tool(row) => {
            let mut line = format!(
                "{}  {}",
                display_safe(&row.tool_name),
                display_safe(&row.status)
            );
            if let Some(summary) = &row.summary {
                line.push(' ');
                line.push_str(&display_safe(summary));
            }
            let mut lines = vec![line];
            if model.tools_expanded {
                lines.push(display_safe(&row.resource));
            }
            lines
        }
        TranscriptItem::Assistant {
            attempt_id,
            text,
            status,
            usage,
            retry_of,
        } => {
            let mut meta = assistant_meta(model, status, retry_of.is_some(), attempt_id);
            if let Some(usage) = usage {
                meta = format!(
                    "{meta}  {} input tokens / {} output tokens",
                    usage.input_tokens, usage.output_tokens
                );
            }
            let body = if text.is_empty() && matches!(status, AttemptStatus::Streaming) {
                "Waiting for the first token...".to_owned()
            } else {
                display_safe(text)
            };
            let mut lines = vec![format!("AutoHarness  {meta}"), body];
            if let AttemptStatus::Failed(failure) = status {
                lines.push(display_safe(&failure.message));
                lines.push(format!(
                    "{}  Retry  New session",
                    display_safe(&failure.code)
                ));
            }
            lines
        }
    }
}

fn assistant_meta(
    model: &Model,
    status: &AttemptStatus,
    retry: bool,
    attempt_id: &AttemptKey,
) -> String {
    let mut parts = Vec::new();
    if retry {
        parts.push("retry".to_owned());
    }
    match status {
        AttemptStatus::Streaming => parts.push(streaming_label(model)),
        AttemptStatus::Cancelling => parts.push("cancelling".to_owned()),
        AttemptStatus::Completed => parts.push("complete".to_owned()),
        AttemptStatus::Cancelled => parts.push("cancelled".to_owned()),
        AttemptStatus::Failed(_) => parts.push("failed".to_owned()),
    }
    if matches!(status, AttemptStatus::Streaming)
        && (model.cancellation_requested(attempt_id)
            || model.pending.values().any(|pending| {
                matches!(pending, PendingKind::CancelAttempt(candidate) if candidate == attempt_id)
            }))
    {
        parts.push("cancelling".to_owned());
    }
    if model.retry_requested(attempt_id) {
        parts.push("retrying".to_owned());
    }
    parts.join("  ")
}

fn streaming_label(model: &Model) -> String {
    let motion = model.motion();
    if model.theme().icons().mode() == GlyphMode::Ascii || !motion.animating() {
        motion.streaming_wave_ascii().to_owned()
    } else {
        "generating".to_owned()
    }
}

fn render_transcript(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let theme = model.theme();
    paint::clear_transparent(buf, area, theme);
    let inset = u16::from(area.width >= PROMPT_INSET_MIN_WIDTH);
    let inner = Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset.saturating_mul(TWO_ROWS)),
        area.height,
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if model.session.transcript.is_empty() {
        return;
    }
    if model.search_pinned_row.is_none() || !model.search_open() {
        render_tail_window(buf, inner, model);
        return;
    }
    render_pinned_window(buf, inner, model);
}

struct TailSlice {
    index: usize,
    item_height: u16,
    skip_top: u16,
    visible: u16,
    y_offset: u16,
}

fn render_tail_window(buf: &mut Buffer, inner: Rect, model: &Model) {
    let slices = tail_slices(inner, model, prompt_surface_height(inner, model));
    if slices.is_empty() && !model.session.transcript.is_empty() {
        render_transcript_start(buf, inner, model);
        return;
    }
    for slice in slices.into_iter().rev() {
        let y = inner.y.saturating_add(slice.y_offset);
        blit_item(
            buf,
            Rect::new(inner.x, y, inner.width, slice.visible),
            inner.width,
            slice.item_height,
            slice.skip_top,
            |tmp, tmp_area| {
                render_item(tmp, tmp_area, model, &model.session.transcript[slice.index]);
            },
        );
    }
}

fn tail_slices(inner: Rect, model: &Model, trailing_rows: u16) -> Vec<TailSlice> {
    let viewport = usize::from(inner.height);
    let bottom = if model.transcript.follow_tail {
        0
    } else {
        model.transcript.rows_from_bottom
    };
    let top = bottom.saturating_add(viewport);
    let mut cursor = usize::from(trailing_rows);
    let mut slices = Vec::with_capacity(viewport.min(model.session.transcript.len()));
    for (index, item) in model.session.transcript.iter().enumerate().rev() {
        let item_height = item_height(model, item, inner.width);
        let height = usize::from(item_height);
        let block = height.saturating_add(1);
        let block_low = cursor;
        let block_high = cursor.saturating_add(block);
        let intersection_low = block_low.max(bottom);
        let intersection_high = block_high.min(top);
        if intersection_low < intersection_high {
            let from_top = block.saturating_sub(intersection_high.saturating_sub(block_low));
            let to_top = block.saturating_sub(intersection_low.saturating_sub(block_low));
            let content_start = from_top.min(height);
            let content_end = to_top.min(height);
            if content_start < content_end {
                slices.push(TailSlice {
                    index,
                    item_height,
                    skip_top: u16::try_from(content_start).unwrap_or(u16::MAX),
                    visible: u16::try_from(content_end.saturating_sub(content_start))
                        .unwrap_or(u16::MAX),
                    y_offset: u16::try_from(
                        top.saturating_sub(intersection_high)
                            .saturating_add(content_start.saturating_sub(from_top)),
                    )
                    .unwrap_or(u16::MAX),
                });
            }
        }
        cursor = block_high;
        if cursor >= top {
            break;
        }
    }

    let top_shift = u16::try_from(top.saturating_sub(cursor)).unwrap_or(u16::MAX);
    let content_shift = slices
        .iter()
        .map(|slice| slice.y_offset.saturating_sub(top_shift))
        .min()
        .unwrap_or(0);
    for slice in &mut slices {
        slice.y_offset = slice
            .y_offset
            .saturating_sub(top_shift)
            .saturating_sub(content_shift);
    }
    slices
}

fn render_transcript_start(buf: &mut Buffer, inner: Rect, model: &Model) {
    let mut y = inner.y;
    for item in &model.session.transcript {
        if y >= inner.bottom() {
            break;
        }
        let height = item_height(model, item, inner.width);
        let visible = height.min(inner.bottom().saturating_sub(y));
        blit_item(
            buf,
            Rect::new(inner.x, y, inner.width, visible),
            inner.width,
            height,
            0,
            |tmp, tmp_area| {
                render_item(tmp, tmp_area, model, item);
            },
        );
        y = y.saturating_add(visible).saturating_add(ROW);
    }
}

fn render_pinned_window(buf: &mut Buffer, inner: Rect, model: &Model) {
    let items = measured_items(model, inner.width);
    let total = items.iter().fold(0_u16, |acc, item| {
        acc.saturating_add(item.height).saturating_add(ROW)
    });
    let skip = transcript_skip(model, inner.height, total);
    let mut consumed = 0_u16;
    let mut y = inner.y;
    for (index, item) in model.session.transcript.iter().enumerate() {
        if y >= inner.bottom() {
            break;
        }
        let height = items.get(index).map_or(ROW, |entry| entry.height);
        let block = height.saturating_add(ROW);
        if consumed.saturating_add(block) <= skip {
            consumed = consumed.saturating_add(block);
            continue;
        }
        let skip_top = skip.saturating_sub(consumed);
        let visible = height
            .saturating_sub(skip_top)
            .min(inner.bottom().saturating_sub(y));
        if visible > 0 {
            blit_item(
                buf,
                Rect::new(inner.x, y, inner.width, visible),
                inner.width,
                height,
                skip_top,
                |tmp, tmp_area| {
                    render_item(tmp, tmp_area, model, item);
                },
            );
            y = y.saturating_add(visible).saturating_add(ROW);
        }
        consumed = consumed.saturating_add(block);
    }
}

fn transcript_skip(model: &Model, viewport: u16, total: u16) -> u16 {
    let maximum = total.saturating_sub(viewport);
    if let Some(pinned) = model.search_pinned_row.filter(|_| model.search_open()) {
        u16::try_from(pinned)
            .unwrap_or(u16::MAX)
            .saturating_sub(viewport / TWO_ROWS / TWO_ROWS)
            .min(maximum)
    } else if model.transcript.follow_tail {
        maximum
    } else {
        maximum.saturating_sub(u16::try_from(model.transcript.rows_from_bottom).unwrap_or(u16::MAX))
    }
}

struct MeasuredItem {
    height: u16,
    retry: bool,
}

fn measured_items(model: &Model, width: u16) -> Vec<MeasuredItem> {
    model
        .session
        .transcript
        .iter()
        .map(|item| {
            let height = item_height(model, item, width);
            let retry = matches!(
                item,
                TranscriptItem::Assistant {
                    status: AttemptStatus::Failed(_),
                    ..
                }
            );
            MeasuredItem { height, retry }
        })
        .collect()
}

fn item_height(model: &Model, item: &TranscriptItem, width: u16) -> u16 {
    let theme = model.theme();
    let icons = theme.icons();
    match item {
        TranscriptItem::User { text, .. } => {
            let body = display_safe(text);
            MessageBlock::new(theme, icons, Icon::User, "You", "", &body).measure(width)
        }
        TranscriptItem::Tool(row) => {
            let summary = row.summary.as_deref().unwrap_or("");
            ToolCard::new(
                theme,
                icons,
                &row.tool_name,
                summary,
                &row.status,
                &row.resource,
                model.tools_expanded,
                tool_icon(&row.status),
            )
            .measure(width)
        }
        TranscriptItem::Assistant {
            attempt_id,
            text,
            status,
            usage,
            retry_of,
        } => {
            let meta = assistant_heading(model, status, usage, retry_of.is_some(), attempt_id);
            let body = assistant_body(text, status);
            let message =
                MessageBlock::new(theme, icons, Icon::Assistant, "AutoHarness", &meta, &body)
                    .measure(width);
            if let AttemptStatus::Failed(failure) = status {
                let lines = wrap_cells(
                    &display_safe(&failure.message),
                    width.saturating_sub(TWO_ROWS).max(ROW),
                );
                message
                    .saturating_add(TWO_ROWS.saturating_add(TWO_ROWS))
                    .saturating_add(u16::try_from(lines.len()).unwrap_or(ROW))
            } else {
                message
            }
        }
    }
}

fn assistant_heading(
    model: &Model,
    status: &AttemptStatus,
    usage: &Option<crate::model::UsageView>,
    retry: bool,
    attempt_id: &AttemptKey,
) -> String {
    let mut meta = assistant_meta(model, status, retry, attempt_id);
    if let Some(usage) = usage {
        meta = format!(
            "{meta}  {} in / {} out",
            usage.input_tokens, usage.output_tokens
        );
    }
    meta
}

fn assistant_body(text: &str, status: &AttemptStatus) -> String {
    if text.is_empty() && matches!(status, AttemptStatus::Streaming) {
        "Waiting for the first token...".to_owned()
    } else {
        display_safe(text)
    }
}

fn render_item(buf: &mut Buffer, area: Rect, model: &Model, item: &TranscriptItem) -> u16 {
    let theme = model.theme();
    let icons = theme.icons();
    match item {
        TranscriptItem::User { text, .. } => {
            let body = display_safe(text);
            MessageBlock::new(theme, icons, Icon::User, "You", "", &body).render(buf, area)
        }
        TranscriptItem::Tool(row) => {
            let summary = row.summary.as_deref().unwrap_or("");
            ToolCard::new(
                theme,
                icons,
                &row.tool_name,
                summary,
                &row.status,
                &row.resource,
                model.tools_expanded,
                tool_icon(&row.status),
            )
            .render(buf, area)
        }
        TranscriptItem::Assistant {
            attempt_id,
            text,
            status,
            usage,
            retry_of,
        } => {
            let meta = assistant_heading(model, status, usage, retry_of.is_some(), attempt_id);
            let body = assistant_body(text, status);
            let used =
                MessageBlock::new(theme, icons, Icon::Assistant, "AutoHarness", &meta, &body)
                    .render(buf, area);
            if matches!(status, AttemptStatus::Streaming)
                && icons.mode() != GlyphMode::Ascii
                && model.motion().animating()
                && !meta.contains("cancelling")
                && !meta.contains("retrying")
            {
                let meta_w = u16::try_from(meta.chars().count()).unwrap_or(0);
                let wave_x = area.right().saturating_sub(
                    meta_w
                        .saturating_add(STREAMING_WAVE_CELLS)
                        .saturating_add(ROW),
                );
                paint_streaming_wave(buf, wave_x, area.y, model);
            }
            if let AttemptStatus::Failed(failure) = status {
                let y = area.y.saturating_add(used);
                if y < area.bottom() {
                    let buttons = recovery_buttons();
                    let callout_area =
                        Rect::new(area.x, y, area.width, area.bottom().saturating_sub(y));
                    let hits = Callout::new(
                        theme,
                        icons,
                        Icon::Danger,
                        "Request failed",
                        &display_safe(&failure.message),
                        &buttons,
                    )
                    .render(buf, callout_area);
                    if let Some((row, _)) = hits.first() {
                        Chip::new(theme, &display_safe(&failure.code), ChipVariant::Danger).render(
                            buf,
                            Rect::new(
                                area.x.saturating_add(ROW),
                                row.y,
                                area.width / TWO_ROWS,
                                ROW,
                            ),
                        );
                    }
                    return used.saturating_add(
                        callout_area.height.min(
                            TWO_ROWS.saturating_add(TWO_ROWS).saturating_add(
                                u16::try_from(
                                    wrap_cells(
                                        &display_safe(&failure.message),
                                        area.width.saturating_sub(TWO_ROWS).max(ROW),
                                    )
                                    .len(),
                                )
                                .unwrap_or(ROW),
                            ),
                        ),
                    );
                }
            }
            used
        }
    }
}

fn paint_streaming_wave(buf: &mut Buffer, x: u16, y: u16, model: &Model) {
    let theme = model.theme();
    let icons = theme.icons();
    let motion = model.motion();
    let phase = motion.wave_phase();
    for index in 0..STREAMING_WAVE_CELLS {
        let t = (phase + f32::from(index) / f32::from(STREAMING_WAVE_CELLS.max(ROW))) % 1.0;
        paint::put(
            buf,
            x.saturating_add(index),
            y,
            ROW,
            icons.horizontal_rule(),
            theme.gradient_style(t),
        );
    }
}

fn recovery_buttons() -> [Button<MouseAction>; 2] {
    [
        Button::new(
            "Retry",
            Some("Ctrl+R".into()),
            ButtonVariant::Primary,
            MouseAction::ChatRetry,
        ),
        Button::new(
            "New session",
            Some("Ctrl+N".into()),
            ButtonVariant::Secondary,
            MouseAction::ChatFreshSession,
        ),
    ]
}

fn recovery_hits(area: Rect, model: &Model) -> Vec<(Rect, MouseAction)> {
    if model.search_pinned_row.is_none() || !model.search_open() {
        return tail_recovery_hits(area, model);
    }
    let items = measured_items(model, area.width);
    let total = items.iter().fold(0_u16, |acc, item| {
        acc.saturating_add(item.height).saturating_add(ROW)
    });
    let skip = transcript_skip(model, area.height, total);
    let mut consumed = 0_u16;
    let mut y = area.y;
    let mut hits = Vec::new();
    let buttons = recovery_buttons();
    let theme = model.theme();
    for item in &items {
        if y >= area.bottom() {
            break;
        }
        let block = item.height.saturating_add(ROW);
        if consumed.saturating_add(block) <= skip {
            consumed = consumed.saturating_add(block);
            continue;
        }
        let skip_top = skip.saturating_sub(consumed);
        let visible = item
            .height
            .saturating_sub(skip_top)
            .min(area.bottom().saturating_sub(y));
        if item.retry && visible > 0 {
            let button_y = y
                .saturating_add(visible.saturating_sub(ROW))
                .min(area.bottom().saturating_sub(ROW));
            hits.extend(button_row_hits(
                theme,
                &buttons,
                Rect::new(area.x, button_y, area.width, ROW),
            ));
        }
        y = y.saturating_add(visible).saturating_add(ROW);
        consumed = consumed.saturating_add(block);
    }
    hits
}

fn tail_recovery_hits(area: Rect, model: &Model) -> Vec<(Rect, MouseAction)> {
    let buttons = recovery_buttons();
    let theme = model.theme();
    let mut hits = Vec::new();
    for slice in tail_slices(area, model, prompt_surface_height(area, model)) {
        if !matches!(
            model.session.transcript.get(slice.index),
            Some(TranscriptItem::Assistant {
                status: AttemptStatus::Failed(_),
                ..
            })
        ) {
            continue;
        }
        let button_y = area
            .y
            .saturating_add(slice.y_offset)
            .saturating_add(slice.visible.saturating_sub(ROW))
            .min(area.bottom().saturating_sub(ROW));
        hits.extend(button_row_hits(
            theme,
            &buttons,
            Rect::new(area.x, button_y, area.width, ROW),
        ));
    }
    hits
}

fn button_row_hits(
    theme: &Theme,
    buttons: &[Button<MouseAction>],
    area: Rect,
) -> Vec<(Rect, MouseAction)> {
    ButtonRow::new(theme, buttons).render(&mut Buffer::empty(area), area)
}

fn tool_icon(status: &str) -> Icon {
    let lowered = status.to_ascii_lowercase();
    if lowered.contains("fail") || lowered.contains("deny") || lowered.contains("error") {
        Icon::Danger
    } else if lowered.contains("run") || lowered.contains("pend") || lowered.contains("start") {
        Icon::Pending
    } else {
        Icon::Success
    }
}

fn render_composer(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let inset = u16::from(area.width >= PROMPT_INSET_MIN_WIDTH);
    let surface = Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset.saturating_mul(TWO_ROWS)),
        area.height,
    );
    if surface.width == 0 || surface.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let theme = model.theme();
    let icons = theme.icons();
    paint::clear_transparent(buf, surface, theme);
    let rule = Rect::new(surface.x, surface.y, surface.width, ROW);
    for column in 0..rule.width {
        paint::put(
            buf,
            rule.x.saturating_add(column),
            rule.y,
            ROW,
            icons.horizontal_rule(),
            theme.gradient_style(normalized_t(column, rule.width.max(ROW))),
        );
    }
    let inner = Rect::new(
        surface.x,
        surface.y.saturating_add(ROW),
        surface.width,
        surface.height.saturating_sub(ROW),
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    render_status(buf, Rect::new(inner.x, inner.y, inner.width, ROW), model);
    if inner.height < TWO_ROWS {
        return;
    }
    let editor_row = inner.y.saturating_add(ROW);
    let caret = icons.glyph(Icon::PromptCaret);
    paint::put(
        buf,
        inner.x,
        editor_row,
        TWO_ROWS,
        &format!("{caret} "),
        theme.style(Token::RoleAssistant),
    );
    let editor_area = Rect::new(
        inner.x.saturating_add(TWO_ROWS),
        editor_row,
        inner.width.saturating_sub(TWO_ROWS),
        inner.height.saturating_sub(ROW),
    );
    if model.palette_open() {
        paint::put(
            buf,
            editor_area.x,
            editor_area.y,
            editor_area.width,
            &format!("/{}", display_safe(&model.palette.query)),
            theme.style(Token::RoleAssistant),
        );
        return;
    }
    let mut composer = model.composer.editor.clone();
    composer.remove_block();
    composer.set_cursor_line_style(theme.style_transparent(Token::TextPrimary));
    composer.set_cursor_style(theme.filled(Token::SurfaceSelected));
    frame.render_widget(&composer, editor_area);
    set_composer_cursor(frame, editor_area, model);
}

fn render_status(buf: &mut Buffer, area: Rect, model: &Model) {
    let theme = model.theme();
    let icons = theme.icons();
    let model_name = selected_model_name(model);
    let thinking = thinking_label(&model.profiles().user.default_mode);
    let workspace = model.profiles().user.workspace.trim();
    let path = if workspace.is_empty() || workspace == "." {
        String::new()
    } else {
        workspace_display_path(workspace, icons.ellipsis())
    };
    let context = context_label(model);
    let tokens = latest_tokens(model);
    let branch = model.settings().git_branch.clone().unwrap_or_default();
    let detail = *model
        .settings()
        .local_profile
        .preferences()
        .prompt_status_detail()
        .value();
    let mut segments = vec![
        StatusSegment {
            priority: 0,
            icon: Some(Icon::Model),
            text: model_name.as_str(),
        },
        StatusSegment {
            priority: 1,
            icon: Some(Icon::Thinking),
            text: thinking.as_str(),
        },
        StatusSegment {
            priority: 2,
            icon: Some(Icon::Context),
            text: context.as_str(),
        },
    ];
    if detail != PromptStatusDetail::Essential && !path.is_empty() {
        segments.push(StatusSegment {
            priority: 3,
            icon: Some(Icon::Workspace),
            text: path.as_str(),
        });
    }
    if detail != PromptStatusDetail::Essential && !branch.is_empty() {
        segments.push(StatusSegment {
            priority: 4,
            icon: Some(Icon::GitBranch),
            text: branch.as_str(),
        });
    }
    if detail == PromptStatusDetail::Detailed && !tokens.is_empty() {
        segments.push(StatusSegment {
            priority: 5,
            icon: Some(Icon::Tokens),
            text: tokens.as_str(),
        });
    }
    StatusBar::new(theme, icons, &segments, icons.separator()).render(buf, area);
}

fn thinking_label(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" => {
            value.trim().to_ascii_lowercase()
        }
        _ => "auto".to_owned(),
    }
}

fn selected_model_name(model: &Model) -> String {
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

fn context_label(model: &Model) -> String {
    let Some(selected) = model.session.selected_model.as_ref() else {
        return "ctx --".to_owned();
    };
    let window = model
        .catalog
        .models()
        .iter()
        .find(|summary| &summary.model == selected)
        .and_then(|summary| summary.context_window_tokens);
    let Some(window) = window.filter(|value| *value > 0) else {
        return "ctx --".to_owned();
    };
    let used = model
        .session
        .transcript
        .iter()
        .rev()
        .find_map(|item| match item {
            TranscriptItem::Assistant {
                usage: Some(usage), ..
            } => Some(usage.input_tokens.saturating_add(usage.output_tokens)),
            _ => None,
        })
        .unwrap_or(0);
    if used == 0 {
        return "ctx 0%".to_owned();
    }
    let tenths = u128::from(used)
        .saturating_mul(1_000)
        .checked_div(u128::from(window))
        .unwrap_or_default()
        .min(1_000);
    if tenths == 0 {
        "ctx <0.1%".to_owned()
    } else if tenths < 100 {
        format!("ctx {}.{:01}%", tenths / 10, tenths % 10)
    } else {
        format!("ctx {}%", (tenths + 5) / 10)
    }
}

fn latest_tokens(model: &Model) -> String {
    model
        .session
        .transcript
        .iter()
        .rev()
        .find_map(|item| match item {
            TranscriptItem::Assistant {
                usage: Some(usage), ..
            } => Some(format!(
                "in {} / out {}",
                compact_token_count(usage.input_tokens),
                compact_token_count(usage.output_tokens)
            )),
            _ => None,
        })
        .unwrap_or_default()
}

fn workspace_display_path(workspace: &str, ellipsis: &str) -> String {
    let normalized = display_safe(workspace.trim()).replace('\\', "/");
    if normalized.is_empty() || normalized == "." {
        return String::new();
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
        format!("{ellipsis}/{}", parts[parts.len() - 3..].join("/"))
    } else {
        normalized
    }
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000 {
        let tenths = tokens.saturating_add(50) / 100;
        format!("{}.{}k", tenths / 10, tenths % 10)
    } else {
        tokens.to_string()
    }
}

fn workspace_label(workspace: &str) -> String {
    let trimmed = workspace.trim();
    if trimmed.is_empty() || trimmed == "." {
        return "workspace".to_owned();
    }
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map_or_else(|| "workspace".to_owned(), display_safe)
}

fn paint_gradient_text(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_width: u16,
    text: &str,
    theme: &Theme,
) {
    let count = u16::try_from(text.chars().count()).unwrap_or(ROW).max(ROW);
    let mut cursor = x;
    for (index, ch) in text.chars().enumerate() {
        if cursor >= x.saturating_add(max_width) {
            break;
        }
        let mut tmp = [0; 4];
        let glyph = ch.encode_utf8(&mut tmp);
        cursor = cursor.saturating_add(paint::put(
            buf,
            cursor,
            y,
            x.saturating_add(max_width).saturating_sub(cursor),
            glyph,
            theme.gradient_style(normalized_t(u16::try_from(index).unwrap_or(0), count)),
        ));
    }
}

fn render_notice(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let Some(notice) = &model.notice else {
        return;
    };
    let (label, token) = match notice {
        Notice::Info(message) => (display_safe(message), Token::Info),
        Notice::Failure(failure) => (
            format!(
                "{}: {}",
                display_safe(&failure.code),
                display_safe(&failure.message)
            ),
            Token::Danger,
        ),
    };
    let buf = frame.buffer_mut();
    paint::put(
        buf,
        area.x,
        area.y,
        area.width,
        &label,
        model.theme().style(token),
    );
}

fn render_search(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    let buf = frame.buffer_mut();
    let theme = model.theme();
    let icons = theme.icons();
    let query = display_safe(&model.search.query);
    let status = model.search_status_label();
    paint::put(
        buf,
        area.x,
        area.y,
        SEARCH_LABEL_WIDTH,
        "Search",
        theme.style(Token::TextMuted),
    );
    let field = format!("/{query}");
    let matches = u32::try_from(model.search.matches.len()).ok();
    let field_area = Rect::new(
        area.x.saturating_add(SEARCH_LABEL_WIDTH),
        area.y,
        area.width.saturating_sub(SEARCH_LABEL_WIDTH),
        area.height,
    );
    crate::ui::component::SearchField::new(
        theme,
        icons,
        &field,
        field.chars().count(),
        matches,
        true,
    )
    .render(buf, field_area);
    if field_area.width > SEARCH_STATUS_MIN_WIDTH {
        paint::put(
            buf,
            field_area.x.saturating_add(field_area.width / TWO_ROWS),
            field_area.y,
            field_area.width / TWO_ROWS,
            &status,
            theme.style(Token::TextMuted),
        );
    }
}

fn set_composer_cursor(frame: &mut Frame<'_>, area: Rect, model: &Model) {
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
    let x = area
        .x
        .saturating_add(u16::try_from(cursor.col).unwrap_or(u16::MAX));
    let y = area
        .y
        .saturating_add(u16::try_from(cursor.row).unwrap_or(u16::MAX));
    if x < area.right() && y < area.bottom() {
        frame.set_cursor_position((x, y));
    }
}

fn blit_item(
    dest: &mut Buffer,
    dest_area: Rect,
    src_width: u16,
    src_height: u16,
    skip_top: u16,
    paint: impl FnOnce(&mut Buffer, Rect),
) {
    if dest_area.width == 0 || dest_area.height == 0 || src_height == 0 {
        return;
    }
    let tmp_area = Rect::new(0, 0, src_width.max(ROW), src_height.max(ROW));
    let mut tmp = Buffer::empty(tmp_area);
    paint(&mut tmp, tmp_area);
    let copy_rows = src_height.saturating_sub(skip_top).min(dest_area.height);
    for row in 0..copy_rows {
        let src_y = skip_top.saturating_add(row);
        let dst_y = dest_area.y.saturating_add(row);
        for col in 0..dest_area.width.min(src_width) {
            if let Some(from) = tmp.cell((col, src_y)).cloned()
                && let Some(to) = dest.cell_mut((dest_area.x.saturating_add(col), dst_y))
            {
                *to = from;
            }
        }
    }
}
