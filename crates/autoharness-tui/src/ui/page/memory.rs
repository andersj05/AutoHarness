//! Read-only Memory workspace: index, revision detail, and admission provenance.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as Split, Rect};

use crate::model::{
    MemoryLoadState, MemoryPane, MemoryStatus, MemorySummary, MemoryWorkspaceFocus, Model,
    MouseAction,
};
use crate::text::display_safe;
use crate::time::{format_absolute_time, format_relative_age, relative_age};
use crate::ui::component::paint::{self, wrap_cells};
use crate::ui::component::{
    Button, ButtonRow, ButtonVariant, Chip, ChipVariant, KeyValue, KeyValueTable, Panel,
    SearchField, StatusBar, StatusSegment,
};
use crate::ui::layout::presentation;
use crate::ui::metrics::{
    MEMORY_ADMISSIONS_PERCENT_WIDE, MEMORY_CONTENT_PREVIEW_ROWS, MEMORY_DETAIL_PERCENT,
    MEMORY_DETAIL_PERCENT_WIDE, MEMORY_FOOTER_MIN_HEIGHT, MEMORY_LIST_PERCENT,
    MEMORY_LIST_PERCENT_WIDE, MEMORY_ROW_BADGE_MIN_WIDTH, MEMORY_TALL_HEADER_MIN_HEIGHT,
    MEMORY_TALL_LIST_MIN_HEIGHT, MEMORY_THREE_PANE_MIN_WIDTH, MEMORY_TWO_PANE_MIN_WIDTH, ROW,
    TWO_ROWS,
};
use crate::ui::{Icon, Token, normalized_t};

#[derive(Clone, Copy, Debug, Default)]
struct MemoryRegions {
    header: Rect,
    search: Rect,
    status_filter: Rect,
    scope_filter: Rect,
    list: Option<Rect>,
    detail: Option<Rect>,
    admissions: Option<Rect>,
    footer: Rect,
}

/// Renders the complete Memory route from local state and a bounded projection.
pub fn render(frame: &mut Frame<'_>, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let regions = regions(area, model);
    paint::clear_transparent(frame.buffer_mut(), area, model.theme());
    render_header(frame.buffer_mut(), regions.header, model);
    render_search(frame.buffer_mut(), regions.search, model);
    render_filters(
        frame.buffer_mut(),
        regions.status_filter,
        regions.scope_filter,
        model,
    );
    if let Some(list) = regions.list {
        render_list(frame.buffer_mut(), list, model);
    }
    if let Some(detail) = regions.detail {
        render_detail(frame.buffer_mut(), detail, model);
    }
    if let Some(admissions) = regions.admissions {
        render_admissions(frame.buffer_mut(), admissions, model);
    }
    render_footer(frame.buffer_mut(), regions.footer, model);
}

/// Paint-order hit regions for the Memory page.
#[must_use]
pub fn hits(area: Rect, model: &Model) -> Vec<(Rect, MouseAction)> {
    let regions = regions(area, model);
    let mut hits = Vec::new();
    if regions.search.width > 0 && regions.search.height > 0 {
        hits.push((regions.search, MouseAction::MemoryFocusSearch));
    }
    if regions.status_filter.width > 0 && regions.status_filter.height > 0 {
        hits.push((regions.status_filter, MouseAction::MemoryCycleStatus));
    }
    if regions.scope_filter.width > 0 && regions.scope_filter.height > 0 {
        hits.push((regions.scope_filter, MouseAction::MemoryCycleScope));
    }
    if let Some(list) = regions.list {
        hits.extend(
            list_rows(list, model)
                .into_iter()
                .map(|(row, summary)| (row, MouseAction::MemorySelect(summary.id().to_owned()))),
        );
    }
    if let Some(admissions) = regions.admissions {
        hits.extend(
            admission_rows(admissions, model)
                .into_iter()
                .map(|(row, index)| (row, MouseAction::MemorySelectAdmission(index))),
        );
    }
    let buttons = footer_buttons(model);
    hits.extend(ButtonRow::new(model.theme(), &buttons).regions(regions.footer));
    hits
}

fn regions(area: Rect, model: &Model) -> MemoryRegions {
    let area = Rect::new(
        area.x.saturating_add(ROW),
        area.y,
        area.width.saturating_sub(TWO_ROWS),
        area.height,
    );
    let header_height = if area.height >= MEMORY_TALL_HEADER_MIN_HEIGHT {
        TWO_ROWS
    } else {
        ROW
    };
    let footer_height = u16::from(area.height >= MEMORY_FOOTER_MIN_HEIGHT);
    let rows = Split::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(ROW),
            Constraint::Length(ROW),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(area);
    let filters = Split::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[2]);
    let mut regions = MemoryRegions {
        header: rows[0],
        search: rows[1],
        status_filter: filters[0],
        scope_filter: filters[1],
        footer: rows[4],
        ..MemoryRegions::default()
    };
    let body = rows[3];
    let single_column = presentation(model).single_column;
    if !single_column && body.width >= MEMORY_THREE_PANE_MIN_WIDTH {
        let columns = Split::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(MEMORY_LIST_PERCENT_WIDE),
                Constraint::Percentage(MEMORY_DETAIL_PERCENT_WIDE),
                Constraint::Percentage(MEMORY_ADMISSIONS_PERCENT_WIDE),
            ])
            .split(body);
        regions.list = Some(columns[0]);
        regions.detail = Some(columns[1]);
        regions.admissions = Some(columns[2]);
    } else if !single_column && body.width >= MEMORY_TWO_PANE_MIN_WIDTH {
        let columns = Split::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(MEMORY_LIST_PERCENT),
                Constraint::Percentage(MEMORY_DETAIL_PERCENT),
            ])
            .split(body);
        regions.list = Some(columns[0]);
        if model.memory_workspace.pane == MemoryPane::Admissions {
            regions.admissions = Some(columns[1]);
        } else {
            regions.detail = Some(columns[1]);
        }
    } else {
        match model.memory_workspace.pane {
            MemoryPane::List => regions.list = Some(body),
            MemoryPane::Detail => regions.detail = Some(body),
            MemoryPane::Admissions => regions.admissions = Some(body),
        }
    }
    regions
}

fn render_header(buf: &mut Buffer, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    paint_gradient_text(buf, area, "Memory", model);
    if area.height < TWO_ROWS {
        return;
    }
    let visible = model.memory_entries().len().to_string();
    let visible_label = format!("{visible} visible");
    let total_label = format!("{} total", model.memory().total());
    let state_label = match model.memory().state() {
        MemoryLoadState::Loading => "loading",
        MemoryLoadState::Ready if model.memory().stale() => "stale snapshot",
        MemoryLoadState::Ready => "ready",
        MemoryLoadState::Failed(_) => "unavailable",
    };
    let segments = [
        StatusSegment {
            priority: 0,
            icon: Some(Icon::RouteMemory),
            text: &visible_label,
        },
        StatusSegment {
            priority: 2,
            icon: None,
            text: &total_label,
        },
        StatusSegment {
            priority: 1,
            icon: Some(match model.memory().state() {
                MemoryLoadState::Failed(_) => Icon::Warning,
                MemoryLoadState::Loading => Icon::Pending,
                MemoryLoadState::Ready => Icon::Success,
            }),
            text: state_label,
        },
    ];
    StatusBar::new(
        model.theme(),
        model.theme().icons(),
        &segments,
        model.theme().icons().separator(),
    )
    .render(
        buf,
        Rect::new(area.x, area.y.saturating_add(ROW), area.width, ROW),
    );
}

fn paint_gradient_text(buf: &mut Buffer, area: Rect, text: &str, model: &Model) {
    let mut x = area.x;
    let total = u16::try_from(text.chars().count()).unwrap_or(ROW).max(ROW);
    for (index, character) in text.chars().enumerate() {
        if x >= area.right() {
            break;
        }
        let rendered = character.to_string();
        x = x.saturating_add(paint::put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            &rendered,
            model
                .theme()
                .gradient_style(normalized_t(u16::try_from(index).unwrap_or(0), total)),
        ));
    }
}

fn render_search(buf: &mut Buffer, area: Rect, model: &Model) {
    SearchField::new(
        model.theme(),
        model.theme().icons(),
        &model.memory_workspace.query,
        model.memory_workspace.query.chars().count(),
        Some(u32::try_from(model.memory_entries().len()).unwrap_or(u32::MAX)),
        model.memory_workspace.focus == MemoryWorkspaceFocus::Search,
    )
    .render(buf, area);
    if model.memory_workspace.query.is_empty() {
        let x = area
            .x
            .saturating_add(model.theme().icons().width(Icon::Search))
            .saturating_add(ROW);
        paint::put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x).saturating_sub(TWO_ROWS),
            "Search memories",
            model.theme().style(Token::TextMuted),
        );
    }
}

fn render_filters(buf: &mut Buffer, status: Rect, scope: Rect, model: &Model) {
    let state_label = format!("State: {}", model.memory_workspace.status.label());
    let scope_label = format!("Scope: {}", model.memory_workspace.scope.label());
    Chip::new(
        model.theme(),
        &state_label,
        if model.memory_workspace.focus == MemoryWorkspaceFocus::Status {
            ChipVariant::Accent
        } else {
            ChipVariant::Neutral
        },
    )
    .render(buf, status);
    Chip::new(
        model.theme(),
        &scope_label,
        if model.memory_workspace.focus == MemoryWorkspaceFocus::Scope {
            ChipVariant::Accent
        } else {
            ChipVariant::Neutral
        },
    )
    .render(buf, scope);
}

fn list_panel<'a>(model: &'a Model) -> Panel<'a> {
    Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::RouteMemory),
        Some("Memory index"),
        None,
        None,
        model.memory_workspace.focus == MemoryWorkspaceFocus::List,
    )
}

fn render_list(buf: &mut Buffer, area: Rect, model: &Model) {
    let inner = list_panel(model).render(buf, area);
    let rows = list_rows(area, model);
    if rows.is_empty() {
        let message = match model.memory().state() {
            MemoryLoadState::Loading => "Loading memory index...",
            MemoryLoadState::Failed(_) => model
                .memory()
                .failure()
                .map_or("Memory index unavailable", |failure| {
                    failure.message.as_str()
                }),
            MemoryLoadState::Ready if model.memory().summaries().is_empty() => {
                "No admitted memories yet."
            }
            MemoryLoadState::Ready => "No memories match these filters.",
        };
        paint::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            &display_safe(message),
            model.theme().style(Token::TextMuted),
        );
        return;
    }
    let icons = model.theme().icons();
    for (row, summary) in rows {
        let selected = model.memory_workspace.selected.as_deref() == Some(summary.id());
        let style = if selected {
            model.theme().style(Token::SurfaceSelectedMuted)
        } else {
            model.theme().style(Token::TextPrimary)
        };
        if selected {
            paint::fill(buf, row, style, Some(' '));
        }
        let marker_width = icons.width(Icon::SelectionCaret);
        if selected {
            paint::put(
                buf,
                row.x,
                row.y,
                marker_width,
                icons.glyph(Icon::SelectionCaret),
                model.theme().style(Token::Accent),
            );
        }
        let text_x = row.x.saturating_add(marker_width).saturating_add(ROW);
        let mut text_width = row.right().saturating_sub(text_x);
        if row.width >= MEMORY_ROW_BADGE_MIN_WIDTH {
            let chip = Chip::new(
                model.theme(),
                summary.status().label(),
                status_variant(summary.status()),
            );
            let chip_width = chip.measure().min(text_width);
            text_width = text_width.saturating_sub(chip_width.saturating_add(ROW));
            chip.render(
                buf,
                Rect::new(
                    row.right().saturating_sub(chip_width),
                    row.y,
                    chip_width,
                    ROW,
                ),
            );
        }
        let preview = paint::ellipsize_words_with(
            &display_safe(summary.preview()),
            text_width,
            icons.ellipsis(),
        );
        paint::put(buf, text_x, row.y, text_width, &preview, style);
        if row.height >= TWO_ROWS {
            let confidence = summary.confidence_bps().map_or_else(
                || "unscored".to_owned(),
                |value| format!("{}%", value / 100),
            );
            let metadata = format!(
                "{}{}{}{}{}",
                summary.scope().label(),
                icons.separator(),
                confidence,
                icons.separator(),
                format_relative_age(relative_age(summary.updated_at_ms(), model.wall_ms))
            );
            let metadata = paint::ellipsize_words_with(
                &metadata,
                row.right().saturating_sub(text_x),
                icons.ellipsis(),
            );
            paint::put(
                buf,
                text_x,
                row.y.saturating_add(ROW),
                row.right().saturating_sub(text_x),
                &metadata,
                model.theme().style(Token::TextMuted),
            );
        }
    }
}

fn list_rows(area: Rect, model: &Model) -> Vec<(Rect, &MemorySummary)> {
    let inner = list_panel(model).content_rect(area);
    let item_height = if inner.height >= MEMORY_TALL_LIST_MIN_HEIGHT {
        TWO_ROWS
    } else {
        ROW
    };
    let entries = model.memory_entries();
    let visible = usize::from(inner.height / item_height).max(1);
    let selected = model
        .memory_workspace
        .selected
        .as_deref()
        .and_then(|selected| entries.iter().position(|entry| entry.id() == selected))
        .unwrap_or_default();
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(entries.len().saturating_sub(visible));
    entries
        .into_iter()
        .skip(start)
        .take(visible)
        .enumerate()
        .map(|(offset, summary)| {
            (
                Rect::new(
                    inner.x,
                    inner
                        .y
                        .saturating_add(u16::try_from(offset).unwrap_or(0) * item_height),
                    inner.width,
                    item_height.min(inner.height),
                ),
                summary,
            )
        })
        .collect()
}

fn status_variant(status: MemoryStatus) -> ChipVariant {
    match status {
        MemoryStatus::Active => ChipVariant::Success,
        MemoryStatus::Proposed => ChipVariant::Warning,
        MemoryStatus::Superseded => ChipVariant::Muted,
        MemoryStatus::Rejected | MemoryStatus::Retracted => ChipVariant::Danger,
        MemoryStatus::Deleted => ChipVariant::Muted,
    }
}

fn render_detail(buf: &mut Buffer, area: Rect, model: &Model) {
    let inner = Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::Info),
        Some("Revision detail"),
        None,
        Some("Enter opens admission history"),
        model.memory_workspace.focus == MemoryWorkspaceFocus::Detail,
    )
    .render(buf, area);
    let Some((summary, detail)) = model.selected_memory() else {
        paint::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            "Choose a memory to inspect its revision.",
            model.theme().style(Token::TextMuted),
        );
        return;
    };
    let Some(detail) = detail else {
        paint::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            "Revision detail is not loaded for this memory.",
            model.theme().style(Token::TextMuted),
        );
        return;
    };
    let content = display_safe(detail.content());
    let lines = wrap_cells(&content, inner.width.max(ROW));
    let content_rows = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(MEMORY_CONTENT_PREVIEW_ROWS)
        .min(inner.height);
    for (offset, line) in lines.iter().take(usize::from(content_rows)).enumerate() {
        paint::put(
            buf,
            inner.x,
            inner.y.saturating_add(u16::try_from(offset).unwrap_or(0)),
            inner.width,
            line,
            model.theme().style(Token::TextPrimary),
        );
    }
    let metadata_y = inner.y.saturating_add(content_rows);
    if metadata_y >= inner.bottom() {
        return;
    }
    let revision = detail.revision().to_string();
    let confidence = summary.confidence_bps().map_or_else(
        || "unscored".to_owned(),
        |value| format!("{}%", value / 100),
    );
    let created = format_absolute_time(detail.created_at_ms());
    let validity = detail
        .valid_until_ms()
        .map_or_else(|| "open ended".to_owned(), format_absolute_time);
    let rows = [
        KeyValue {
            label: "State",
            value: summary.status().label(),
            chip: Some(summary.scope().label()),
        },
        KeyValue {
            label: "Trust",
            value: detail.trust().label(),
            chip: None,
        },
        KeyValue {
            label: "Source",
            value: detail.source(),
            chip: None,
        },
        KeyValue {
            label: "Revision",
            value: &revision,
            chip: Some(&confidence),
        },
        KeyValue {
            label: "Created",
            value: &created,
            chip: None,
        },
        KeyValue {
            label: "Valid",
            value: &validity,
            chip: None,
        },
    ];
    KeyValueTable::new(model.theme(), &rows).render(
        buf,
        Rect::new(
            inner.x,
            metadata_y,
            inner.width,
            inner.bottom().saturating_sub(metadata_y),
        ),
    );
}

fn admissions_panel<'a>(model: &'a Model) -> Panel<'a> {
    Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::Context),
        Some("Admission history"),
        None,
        Some("Why and where this memory entered context"),
        model.memory_workspace.focus == MemoryWorkspaceFocus::Admissions,
    )
}

fn render_admissions(buf: &mut Buffer, area: Rect, model: &Model) {
    let inner = admissions_panel(model).render(buf, area);
    let Some((_, Some(detail))) = model.selected_memory() else {
        paint::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            "No admission history is loaded.",
            model.theme().style(Token::TextMuted),
        );
        return;
    };
    if detail.admissions().is_empty() {
        paint::put(
            buf,
            inner.x,
            inner.y,
            inner.width,
            "This revision has not been admitted into a run.",
            model.theme().style(Token::TextMuted),
        );
        return;
    }
    let icons = model.theme().icons();
    for (row, index) in admission_rows(area, model) {
        let admission = &detail.admissions()[index];
        let selected = model.memory_workspace.admission_selected == index;
        let style = if selected {
            model.theme().style(Token::SurfaceSelectedMuted)
        } else {
            model.theme().style(Token::TextPrimary)
        };
        if selected {
            paint::fill(buf, row, style, Some(' '));
        }
        let marker_width = icons.width(Icon::SelectionCaret);
        if selected {
            paint::put(
                buf,
                row.x,
                row.y,
                marker_width,
                icons.glyph(Icon::SelectionCaret),
                model.theme().style(Token::Accent),
            );
        }
        let x = row.x.saturating_add(marker_width).saturating_add(ROW);
        paint::put(
            buf,
            x,
            row.y,
            row.right().saturating_sub(x),
            &paint::ellipsize_words_with(
                &display_safe(admission.reason()),
                row.right().saturating_sub(x),
                icons.ellipsis(),
            ),
            style,
        );
        if row.height >= TWO_ROWS {
            let metadata = format!(
                "{}{}{}{}rank {}",
                display_safe(admission.session()),
                icons.separator(),
                display_safe(admission.model()),
                icons.separator(),
                admission.rank()
            );
            let metadata = paint::ellipsize_words_with(
                &metadata,
                row.right().saturating_sub(x),
                icons.ellipsis(),
            );
            paint::put(
                buf,
                x,
                row.y.saturating_add(ROW),
                row.right().saturating_sub(x),
                &metadata,
                model.theme().style(Token::TextMuted),
            );
        }
    }
}

fn admission_rows(area: Rect, model: &Model) -> Vec<(Rect, usize)> {
    let inner = admissions_panel(model).content_rect(area);
    let Some((_, Some(detail))) = model.selected_memory() else {
        return Vec::new();
    };
    let item_height = if inner.height >= MEMORY_TALL_LIST_MIN_HEIGHT {
        TWO_ROWS
    } else {
        ROW
    };
    let visible = usize::from(inner.height / item_height).max(1);
    let selected = model
        .memory_workspace
        .admission_selected
        .min(detail.admissions().len().saturating_sub(1));
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(detail.admissions().len().saturating_sub(visible));
    (start..detail.admissions().len().min(start.saturating_add(visible)))
        .enumerate()
        .map(|(offset, index)| {
            (
                Rect::new(
                    inner.x,
                    inner
                        .y
                        .saturating_add(u16::try_from(offset).unwrap_or(0) * item_height),
                    inner.width,
                    item_height.min(inner.height),
                ),
                index,
            )
        })
        .collect()
}

fn footer_buttons(model: &Model) -> Vec<Button<MouseAction>> {
    match model.memory_workspace.pane {
        MemoryPane::List => vec![Button::new(
            "Open",
            Some("Enter".to_owned()),
            ButtonVariant::Primary,
            MouseAction::MemoryOpen,
        )],
        MemoryPane::Detail => vec![
            Button::new(
                "Admissions",
                Some("Enter".to_owned()),
                ButtonVariant::Primary,
                MouseAction::MemoryAdmissions,
            ),
            Button::new(
                "Index",
                Some("Esc".to_owned()),
                ButtonVariant::Secondary,
                MouseAction::MemoryBack,
            ),
        ],
        MemoryPane::Admissions => vec![Button::new(
            "Detail",
            Some("Esc".to_owned()),
            ButtonVariant::Secondary,
            MouseAction::MemoryBack,
        )],
    }
}

fn render_footer(buf: &mut Buffer, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buttons = footer_buttons(model);
    let button_row = ButtonRow::new(model.theme(), &buttons);
    let button_width = button_row.measure().min(area.width);
    let hint_width = area.width.saturating_sub(button_width).saturating_sub(ROW);
    if hint_width > 0 {
        paint::put(
            buf,
            area.x,
            area.y,
            hint_width,
            "Tab focus  / search",
            model.theme().style(Token::TextMuted),
        );
    }
    button_row.render(buf, area);
}
