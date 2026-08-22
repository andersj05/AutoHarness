use std::fmt::Write as _;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::model::{
    AttemptStatus, CatalogProjection, Focus, Model, ModelSummary, Notice, PendingKind, RetryPolicy,
    TranscriptItem,
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

/// Renders the complete terminal client from local state only.
pub fn view(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    if area.width < 24 || area.height < 7 {
        render_compact(frame, area, model);
    } else {
        render_standard(frame, area, model);
    }

    if model.focus == Focus::Permission {
        render_permission(frame, area, model);
    } else if model.browser.open {
        render_browser(frame, area, model);
    } else if model.credential.open {
        render_credential(frame, area, model);
    } else if model.picker.open {
        render_picker(frame, area, model);
    }
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(notice_height),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], model);
    render_transcript(frame, chunks[1], model, true);
    if notice_height > 0 {
        render_notice(frame, chunks[2], model);
    }
    frame.render_widget(&model.composer.editor, chunks[3]);
    render_footer(frame, chunks[4], model);
    set_composer_cursor(frame, chunks[3], model, true);
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
    let title = if area.width < 50 {
        format!(" AutoHarness | {state} ")
    } else {
        format!(" AutoHarness  |  {selected}  |  {state} ")
    };
    frame.render_widget(
        Paragraph::new(display_safe(&title)).style(HEADER_STYLE),
        area,
    );
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
    let rows_from_bottom = if model.transcript.follow_tail {
        0
    } else {
        model.transcript.rows_from_bottom.min(maximum_scroll)
    };
    let top = maximum_scroll.saturating_sub(rows_from_bottom);
    let top = u16::try_from(top).unwrap_or(u16::MAX);
    frame.render_widget(paragraph.scroll((top, 0)), inner);
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
        || model.picker.open
        || model.credential.open
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
