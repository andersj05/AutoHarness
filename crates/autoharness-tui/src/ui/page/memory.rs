//! Memory workspace: bounded inspection plus deliberate lifecycle workflows.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as Split, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::model::{
    MemoryLifecycleMode, MemoryLoadState, MemoryPane, MemoryStatus, MemorySummary,
    MemoryWorkspaceFocus, Model, MouseAction,
};
use crate::text::display_safe;
use crate::time::{format_absolute_time, format_relative_age, relative_age};
use crate::ui::component::paint::{self, wrap_cells};
use crate::ui::component::{
    Button, ButtonRow, ButtonVariant, Chip, ChipVariant, KeyValue, KeyValueTable, Modal,
    ModalIntent, Panel, SearchField, StatusBar, StatusSegment,
};
use crate::ui::layout::presentation;
use crate::ui::metrics::{
    MEMORY_ACTIONS_FULL_WIDTH, MEMORY_ADMISSION_CONTEXT_MIN_HEIGHT, MEMORY_ADMISSION_CONTEXT_ROWS,
    MEMORY_ADMISSIONS_PERCENT_WIDE, MEMORY_CONTENT_PREVIEW_ROWS, MEMORY_DETAIL_PERCENT,
    MEMORY_DETAIL_PERCENT_WIDE, MEMORY_FOOTER_MIN_HEIGHT, MEMORY_LIST_PERCENT,
    MEMORY_LIST_PERCENT_WIDE, MEMORY_REMEMBER_EDITOR_CHROME_ROWS, MEMORY_ROW_BADGE_MIN_WIDTH,
    MEMORY_TALL_HEADER_MIN_HEIGHT, MEMORY_TALL_LIST_MIN_HEIGHT, MEMORY_THREE_PANE_MIN_WIDTH,
    MEMORY_TWO_PANE_MIN_WIDTH, ROW, TWO_ROWS,
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
    let buttons = footer_buttons(model, regions.footer.width);
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
    let total_label = format!("{} on page", model.memory().summaries().len());
    let state_label = match model.memory().state() {
        _ if model.memory_view_loading() => "refreshing view",
        MemoryLoadState::Loading => "loading",
        MemoryLoadState::Ready if model.memory_has_next_page() => "more available",
        MemoryLoadState::Ready if model.memory().stale() => "partial result",
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
                _ if model.memory_view_loading() => Icon::Pending,
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
            "Search all memory",
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
            _ if model.memory_view_loading() => memory_loading_label(model),
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
            MemoryLoadState::Ready if model.memory_has_next_page() || model.memory().stale() => {
                "No matches on this page; use Next for older results."
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
        MemoryStatus::Conflicting => ChipVariant::Danger,
        MemoryStatus::Superseded => ChipVariant::Muted,
        MemoryStatus::Rejected | MemoryStatus::Retracted => ChipVariant::Danger,
        MemoryStatus::Expired => ChipVariant::Warning,
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
    let scope_identity = detail.revision_context().map_or_else(
        || summary.scope().label().to_owned(),
        |context| {
            format!(
                "{} - {}",
                summary.scope().label(),
                display_safe(context.scope_identity())
            )
        },
    );
    let origin = detail
        .revision_context()
        .map_or("not loaded", |context| context.origin().label());
    let sensitivity = detail
        .revision_context()
        .map_or("not loaded", |context| context.sensitivity().label());
    let evidence_count = detail
        .revision_context()
        .map_or(0, |context| context.evidence().len())
        .to_string();
    let finding_count = detail
        .revision_context()
        .map_or(0, |context| context.findings().len())
        .to_string();
    let rows = vec![
        KeyValue {
            label: "State",
            value: summary.status().label(),
            chip: None,
        },
        KeyValue {
            label: "Scope",
            value: &scope_identity,
            chip: None,
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
            label: "Origin",
            value: origin,
            chip: Some(sensitivity),
        },
        KeyValue {
            label: "Review data",
            value: &evidence_count,
            chip: Some(&finding_count),
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
            let metadata = admission.context().map_or_else(
                || {
                    format!(
                        "{}{}{}{}rank {}",
                        display_safe(admission.session()),
                        icons.separator(),
                        display_safe(admission.model()),
                        icons.separator(),
                        admission.rank()
                    )
                },
                |context| {
                    format!(
                        "{}{}{}{}turn {}",
                        display_safe(admission.session()),
                        icons.separator(),
                        display_safe(context.provider_attempt()),
                        icons.separator(),
                        context.run_turn()
                    )
                },
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
    let (_, context_area) = admission_areas(area, model);
    if let Some(context_area) = context_area {
        render_admission_context(buf, context_area, model);
    }
}

fn admission_rows(area: Rect, model: &Model) -> Vec<(Rect, usize)> {
    let (inner, _) = admission_areas(area, model);
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

fn admission_areas(area: Rect, model: &Model) -> (Rect, Option<Rect>) {
    let inner = admissions_panel(model).content_rect(area);
    let context_loaded = model
        .selected_memory()
        .and_then(|(_, detail)| detail)
        .and_then(|detail| {
            detail
                .admissions()
                .get(model.memory_workspace.admission_selected)
        })
        .and_then(|admission| admission.context())
        .is_some();
    if !context_loaded || inner.height < MEMORY_ADMISSION_CONTEXT_MIN_HEIGHT {
        return (inner, None);
    }
    let rows = Split::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(ROW),
            Constraint::Length(MEMORY_ADMISSION_CONTEXT_ROWS),
        ])
        .split(inner);
    (rows[0], Some(rows[1]))
}

fn render_admission_context(buf: &mut Buffer, area: Rect, model: &Model) {
    let Some((_, Some(detail))) = model.selected_memory() else {
        return;
    };
    let Some(admission) = detail
        .admissions()
        .get(model.memory_workspace.admission_selected)
    else {
        return;
    };
    let Some(context) = admission.context() else {
        return;
    };
    let turn = context.run_turn().to_string();
    let tokens = context.token_count().to_string();
    let rank = admission.rank().to_string();
    let factors = if context.reason_factors().is_empty() {
        "none loaded".to_owned()
    } else {
        context
            .reason_factors()
            .iter()
            .map(|factor| display_safe(factor))
            .collect::<Vec<_>>()
            .join(model.theme().icons().separator())
    };
    let rows = [
        KeyValue {
            label: "Attempt",
            value: context.provider_attempt(),
            chip: Some(admission.model()),
        },
        KeyValue {
            label: "Turn",
            value: &turn,
            chip: Some(context.epoch()),
        },
        KeyValue {
            label: "Tokens",
            value: &tokens,
            chip: Some(&rank),
        },
        KeyValue {
            label: "Revision",
            value: context.source_revision(),
            chip: None,
        },
        KeyValue {
            label: "Renderer",
            value: context.renderer_version(),
            chip: None,
        },
        KeyValue {
            label: "Reasons",
            value: &factors,
            chip: None,
        },
    ];
    KeyValueTable::new(model.theme(), &rows).render(buf, area);
}

fn footer_buttons(model: &Model, width: u16) -> Vec<Button<MouseAction>> {
    if model.memory_view_loading() {
        return Vec::new();
    }
    let mut buttons = vec![Button::new(
        "Remember",
        None,
        ButtonVariant::Primary,
        MouseAction::MemoryRemember,
    )];
    if width >= MEMORY_ACTIONS_FULL_WIDTH {
        let primary = model.memory_actions().into_iter().find(|mode| {
            matches!(
                mode,
                MemoryLifecycleMode::Review | MemoryLifecycleMode::Revise
            )
        });
        if let Some(mode) = primary {
            buttons.push(Button::new(
                if mode == MemoryLifecycleMode::Review {
                    "Review"
                } else {
                    "Correct"
                },
                None,
                ButtonVariant::Secondary,
                if mode == MemoryLifecycleMode::Review {
                    MouseAction::MemoryReview
                } else {
                    MouseAction::MemoryRevise
                },
            ));
        }
    }
    buttons.push(Button::new(
        "Actions",
        None,
        ButtonVariant::Secondary,
        MouseAction::MemoryActions,
    ));
    buttons.push(match model.memory_workspace.pane {
        MemoryPane::List => Button::new(
            "Open",
            None,
            ButtonVariant::Secondary,
            MouseAction::MemoryOpen,
        ),
        MemoryPane::Detail => Button::new(
            "Usage",
            None,
            ButtonVariant::Secondary,
            MouseAction::MemoryAdmissions,
        ),
        MemoryPane::Admissions => Button::new(
            "Detail",
            None,
            ButtonVariant::Secondary,
            MouseAction::MemoryBack,
        ),
    });
    let compact_page_keys = width < MEMORY_ACTIONS_FULL_WIDTH;
    if model.memory_has_previous_page() {
        buttons.push(Button::new(
            if compact_page_keys {
                "< PgUp"
            } else {
                "Previous"
            },
            if compact_page_keys {
                None
            } else {
                Some("PgUp".to_owned())
            },
            ButtonVariant::Secondary,
            MouseAction::MemoryPreviousPage,
        ));
    }
    if model.memory_has_next_page() {
        buttons.push(Button::new(
            if compact_page_keys { "PgDn >" } else { "Next" },
            if compact_page_keys {
                None
            } else {
                Some("PgDn".to_owned())
            },
            ButtonVariant::Secondary,
            MouseAction::MemoryNextPage,
        ));
    }
    while ButtonRow::new(model.theme(), &buttons).measure() > width {
        let removable = buttons
            .iter()
            .position(|button| {
                matches!(
                    button.action,
                    MouseAction::MemoryReview | MouseAction::MemoryRevise
                )
            })
            .or_else(|| {
                buttons
                    .iter()
                    .position(|button| button.action == MouseAction::MemoryRemember)
            })
            .or_else(|| {
                buttons.iter().position(|button| {
                    matches!(
                        button.action,
                        MouseAction::MemoryOpen
                            | MouseAction::MemoryAdmissions
                            | MouseAction::MemoryBack
                    )
                })
            });
        let Some(index) = removable else {
            break;
        };
        buttons.remove(index);
    }
    buttons
}

fn render_footer(buf: &mut Buffer, area: Rect, model: &Model) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buttons = footer_buttons(model, area.width);
    let button_row = ButtonRow::new(model.theme(), &buttons);
    let button_width = button_row.measure().min(area.width);
    let hint_width = area.width.saturating_sub(button_width).saturating_sub(ROW);
    let loading_hint = memory_loading_label(model);
    let full_hint = "Alt+N remember  Alt+A actions";
    let compact_hint = "Alt+N remember";
    let hint = if model.memory_view_loading()
        && u16::try_from(loading_hint.width()).unwrap_or(u16::MAX) <= hint_width
    {
        Some(loading_hint)
    } else if u16::try_from(full_hint.width()).unwrap_or(u16::MAX) <= hint_width {
        Some(full_hint)
    } else if u16::try_from(compact_hint.width()).unwrap_or(u16::MAX) <= hint_width {
        Some(compact_hint)
    } else {
        None
    };
    if let Some(hint) = hint {
        paint::put(
            buf,
            area.x,
            area.y,
            hint_width,
            hint,
            model.theme().style(Token::TextMuted),
        );
    }
    button_row.render(buf, area);
}

fn memory_loading_label(model: &Model) -> &'static str {
    match model.memory_workspace.page_direction {
        crate::model::MemoryPageDirection::First => "Searching all memory...",
        crate::model::MemoryPageDirection::Next => "Loading next page...",
        crate::model::MemoryPageDirection::Previous => "Loading previous page...",
    }
}

/// Buttons shared by Memory lifecycle rendering and paint-order hit testing.
#[must_use]
pub(crate) fn lifecycle_buttons(model: &Model) -> Vec<Button<MouseAction>> {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return Vec::new();
    };
    if state.pending_request.is_some() {
        return Vec::new();
    }
    match state.mode {
        MemoryLifecycleMode::Remember | MemoryLifecycleMode::Revise => vec![
            Button::new(
                "Cancel",
                None,
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Save",
                Some("Ctrl+S".to_owned()),
                ButtonVariant::Primary,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
        MemoryLifecycleMode::Review => vec![
            Button::new(
                "Close",
                None,
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Reject",
                None,
                ButtonVariant::Danger,
                MouseAction::MemoryProposalReject,
            ),
            Button::new(
                "Approve",
                None,
                ButtonVariant::Primary,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
        MemoryLifecycleMode::Actions => vec![
            Button::new(
                "Close",
                Some("Esc".to_owned()),
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Choose",
                Some("Enter".to_owned()),
                ButtonVariant::Primary,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
        MemoryLifecycleMode::Retract => vec![
            Button::new(
                "Cancel",
                None,
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Retract",
                None,
                ButtonVariant::Danger,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
        MemoryLifecycleMode::Delete => vec![
            Button::new(
                "Cancel",
                None,
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Delete",
                None,
                ButtonVariant::Danger,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
        MemoryLifecycleMode::Export => vec![
            Button::new(
                "Cancel",
                None,
                ButtonVariant::Secondary,
                MouseAction::MemoryLifecycleCancel,
            ),
            Button::new(
                "Export",
                None,
                ButtonVariant::Primary,
                MouseAction::MemoryLifecycleSubmit,
            ),
        ],
    }
}

/// Exact action-row regions inside the Memory action chooser.
#[must_use]
pub(crate) fn lifecycle_action_rows(popup: Rect, model: &Model) -> Vec<(Rect, usize)> {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return Vec::new();
    };
    if state.mode != MemoryLifecycleMode::Actions {
        return Vec::new();
    }
    let panel = Panel::new(
        model.theme(),
        model.theme().icons(),
        Some(Icon::RouteMemory),
        Some(state.mode.label()),
        None,
        None,
        true,
    );
    let inner = panel.content_rect(popup);
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(ROW),
    );
    let start_y = body.y.saturating_add(TWO_ROWS);
    model
        .memory_actions()
        .into_iter()
        .enumerate()
        .take(usize::from(body.bottom().saturating_sub(start_y)))
        .map(|(index, _)| {
            (
                Rect::new(
                    body.x,
                    start_y.saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
                    body.width,
                    ROW,
                ),
                index,
            )
        })
        .collect()
}

/// Renders the single Memory lifecycle overlay over the complete terminal host.
pub(crate) fn render_lifecycle(frame: &mut Frame<'_>, host: Rect, model: &Model) {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return;
    };
    let popup = crate::ui::layout::popup_rect(host);
    let buttons = lifecycle_buttons(model);
    let icon = match state.mode {
        MemoryLifecycleMode::Delete | MemoryLifecycleMode::Retract => Icon::Danger,
        MemoryLifecycleMode::Review => Icon::Warning,
        MemoryLifecycleMode::Remember
        | MemoryLifecycleMode::Revise
        | MemoryLifecycleMode::Actions
        | MemoryLifecycleMode::Export => Icon::RouteMemory,
    };
    let intent = match state.mode {
        MemoryLifecycleMode::Delete => ModalIntent::Danger,
        MemoryLifecycleMode::Retract | MemoryLifecycleMode::Review => ModalIntent::Warning,
        MemoryLifecycleMode::Remember
        | MemoryLifecycleMode::Revise
        | MemoryLifecycleMode::Actions
        | MemoryLifecycleMode::Export => ModalIntent::Neutral,
    };
    let title = if state.mode == MemoryLifecycleMode::Review {
        "Review proposal - Up/Down"
    } else {
        state.mode.label()
    };
    let (inner, _) = Modal::new(
        model.theme(),
        model.theme().icons(),
        title,
        Some(icon),
        &buttons,
    )
    .intent(intent)
    .render(frame.buffer_mut(), host, popup.width, popup.height);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match state.mode {
        MemoryLifecycleMode::Remember | MemoryLifecycleMode::Revise => {
            render_lifecycle_editor(frame.buffer_mut(), inner, model)
        }
        MemoryLifecycleMode::Actions => render_lifecycle_actions(frame.buffer_mut(), inner, model),
        MemoryLifecycleMode::Review
        | MemoryLifecycleMode::Retract
        | MemoryLifecycleMode::Delete
        | MemoryLifecycleMode::Export => {
            let lines = lifecycle_review_lines(model);
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll, 0))
                .render(inner, frame.buffer_mut());
        }
    }
    if state.pending_request.is_some() && inner.height > 0 {
        paint::put(
            frame.buffer_mut(),
            inner.x,
            inner.bottom().saturating_sub(ROW),
            inner.width,
            "Saving durable change...",
            model.theme().style(Token::Warning),
        );
    }
}

fn render_lifecycle_editor(buf: &mut Buffer, area: Rect, model: &Model) {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return;
    };
    let Some(editor) = state.editor.as_ref() else {
        return;
    };
    let instruction = if state.mode == MemoryLifecycleMode::Remember {
        format!(
            "Scope: Workspace{}Kind: Fact",
            model.theme().icons().separator()
        )
    } else {
        "Correct the exact revision below. Prior revisions remain auditable.".to_owned()
    };
    paint::put(
        buf,
        area.x,
        area.y,
        area.width,
        &instruction,
        model.theme().style(Token::TextMuted),
    );
    if area.height <= ROW {
        return;
    }
    let editor_offset = if state.mode == MemoryLifecycleMode::Remember {
        paint::put(
            buf,
            area.x,
            area.y.saturating_add(ROW),
            area.width,
            "Sensitivity: Internal",
            model.theme().style(Token::Warning),
        );
        paint::put(
            buf,
            area.x,
            area.y.saturating_add(TWO_ROWS),
            area.width,
            "Ordinary Remember rejects secrets.",
            model.theme().style(Token::Warning),
        );
        paint::put(
            buf,
            area.x,
            area.y.saturating_add(TWO_ROWS).saturating_add(ROW),
            area.width,
            "Enter adds a line; Ctrl+S saves.",
            model.theme().style(Token::TextMuted),
        );
        MEMORY_REMEMBER_EDITOR_CHROME_ROWS
    } else {
        ROW
    };
    let count = format!("{} characters", editor.char_count());
    paint::put(
        buf,
        area.x,
        area.y.saturating_add(editor_offset),
        area.width,
        &count,
        model.theme().style(Token::TextSecondary),
    );
    let editor_area = Rect::new(
        area.x,
        area.y.saturating_add(editor_offset).saturating_add(ROW),
        area.width,
        area.height
            .saturating_sub(editor_offset.saturating_add(ROW)),
    );
    let content = if editor.text().is_empty() {
        "Type memory content...".to_owned()
    } else {
        display_safe(editor.text())
    };
    Paragraph::new(content)
        .style(if editor.text().is_empty() {
            model.theme().style(Token::TextMuted)
        } else {
            model.theme().style(Token::TextPrimary)
        })
        .wrap(Wrap { trim: false })
        .render(editor_area, buf);
}

fn render_lifecycle_actions(buf: &mut Buffer, area: Rect, model: &Model) {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return;
    };
    paint::put(
        buf,
        area.x,
        area.y,
        area.width,
        "Choose what happens to this exact loaded revision.",
        model.theme().style(Token::TextMuted),
    );
    for (offset, mode) in model.memory_actions().into_iter().enumerate() {
        let y = area
            .y
            .saturating_add(TWO_ROWS)
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= area.bottom() {
            break;
        }
        let selected = offset == state.action_selected;
        if selected {
            paint::fill(
                buf,
                Rect::new(area.x, y, area.width, ROW),
                model.theme().style(Token::SurfaceSelectedMuted),
                Some(' '),
            );
        }
        let marker = if selected {
            model.theme().icons().glyph(Icon::SelectionCaret)
        } else {
            " "
        };
        let description = match mode {
            MemoryLifecycleMode::Revise => "write a corrected revision",
            MemoryLifecycleMode::Review => "inspect and decide this proposal",
            MemoryLifecycleMode::Retract => "stop future admission, keep audit history",
            MemoryLifecycleMode::Delete => "record a logical tombstone",
            MemoryLifecycleMode::Export => "save a user-owned artifact",
            MemoryLifecycleMode::Remember | MemoryLifecycleMode::Actions => "",
        };
        let label = format!("{marker} {} - {description}", mode.label());
        paint::put(
            buf,
            area.x,
            y,
            area.width,
            &label,
            if selected {
                model.theme().style(Token::FocusRing)
            } else {
                model.theme().style(Token::TextPrimary)
            },
        );
    }
}

fn lifecycle_review_lines(model: &Model) -> Vec<Line<'static>> {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return Vec::new();
    };
    let Some(target) = state.target.as_ref() else {
        return Vec::new();
    };
    let primary = model.theme().style(Token::TextPrimary);
    let muted = model.theme().style(Token::TextMuted);
    let warning = model.theme().style(Token::Warning);
    let danger = model.theme().style(Token::Danger);
    let mut lines = Vec::new();
    match state.mode {
        MemoryLifecycleMode::Review => {
            lines.push(Line::styled("Exact proposed content", warning));
            lines.push(Line::styled(target_content_line(target), primary));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("State: ", muted),
                Span::styled(target.status.label().to_owned(), primary),
                Span::styled("  Scope: ", muted),
                Span::styled(target.scope.label().to_owned(), primary),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Source: ", muted),
                Span::styled(display_safe(&target.source), primary),
                Span::styled("  Trust: ", muted),
                Span::styled(target.trust.label().to_owned(), primary),
            ]));
            if let Some(context) = target.revision_context.as_ref() {
                lines.push(Line::from(vec![
                    Span::styled("Scope identity: ", muted),
                    Span::styled(display_safe(context.scope_identity()), primary),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Origin: ", muted),
                    Span::styled(context.origin().label().to_owned(), primary),
                    Span::styled("  Sensitivity: ", muted),
                    Span::styled(context.sensitivity().label().to_owned(), warning),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::styled("Evidence", muted));
                if context.evidence().is_empty() {
                    lines.push(Line::styled("No evidence was loaded.", warning));
                }
                for evidence in context.evidence() {
                    lines.push(Line::styled(
                        format!(
                            "{} - {}",
                            display_safe(evidence.label()),
                            display_safe(evidence.source())
                        ),
                        primary,
                    ));
                    lines.push(Line::styled(display_safe(evidence.excerpt()), muted));
                }
                lines.push(Line::from(""));
                lines.push(Line::styled("Duplicate and contradiction checks", muted));
                if context.findings().is_empty() {
                    lines.push(Line::styled(
                        "No findings in the loaded validation.",
                        primary,
                    ));
                }
                for finding in context.findings() {
                    lines.push(Line::styled(
                        format!(
                            "{} - {} - {}",
                            finding.kind().label(),
                            display_safe(finding.related_memory_id()),
                            display_safe(finding.summary())
                        ),
                        warning,
                    ));
                }
                if !context.relations().is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::styled("Relations", muted));
                    for relation in context.relations() {
                        lines.push(Line::styled(
                            format!(
                                "{} {}",
                                relation.kind().label(),
                                display_safe(relation.memory_id())
                            ),
                            primary,
                        ));
                    }
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Approval is deliberate and only affects future eligible turns. Up/Down scrolls the complete review.",
                warning,
            ));
        }
        MemoryLifecycleMode::Retract => {
            lines.push(Line::styled(target_content_line(target), primary));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Retraction stops this revision from future admission. It keeps the memory and its audit history.",
                warning,
            ));
            lines.push(Line::styled(
                "Already dispatched provider turns cannot be recalled.",
                danger,
            ));
        }
        MemoryLifecycleMode::Delete => {
            lines.push(Line::styled(target_content_line(target), primary));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Logical delete records a tombstone; retraction only stops future admission.",
                danger,
            ));
            lines.push(Line::styled("Audit history remains.", warning));
            lines.push(Line::styled(
                "Dispatched turns cannot be recalled.",
                warning,
            ));
            lines.push(Line::styled(
                "Press Y or choose Delete to confirm this exact identity.",
                muted,
            ));
        }
        MemoryLifecycleMode::Export => {
            lines.push(Line::styled(target_content_line(target), primary));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Export writes this exact loaded revision and its safe provenance as a user-owned artifact.",
                muted,
            ));
        }
        MemoryLifecycleMode::Remember
        | MemoryLifecycleMode::Revise
        | MemoryLifecycleMode::Actions => {}
    }
    lines
}

fn target_content_line(target: &crate::model::MemoryTargetSnapshot) -> String {
    target.content.as_ref().map_or_else(
        || "Exact content sidecar unavailable; lifecycle metadata remains actionable.".to_owned(),
        |content| display_safe(content.as_str()),
    )
}
