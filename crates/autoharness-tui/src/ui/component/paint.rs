//! Shared cell painting helpers for presentation components.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::super::Token;
use super::super::theme::Theme;

/// Writes `text` at `(x, y)` clipped to `max_width` cells. Returns cells used.
pub fn put(buf: &mut Buffer, x: u16, y: u16, max_width: u16, text: &str, style: Style) -> u16 {
    if max_width == 0 || !buf.area.contains(ratatui::layout::Position { x, y }) {
        return 0;
    }
    let mut used = 0_u16;
    for ch in text.chars() {
        let width = u16::try_from(ch.width().unwrap_or(0)).unwrap_or(0);
        if width == 0 {
            continue;
        }
        if used.saturating_add(width) > max_width {
            break;
        }
        let col = x.saturating_add(used);
        if col >= buf.area.right() {
            break;
        }
        if let Some(cell) = buf.cell_mut((col, y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
        used = used.saturating_add(width);
    }
    used
}

/// Fills `area` with `fill` using `style`, leaving symbols unchanged when `fill` is None.
pub fn fill(buf: &mut Buffer, area: Rect, style: Style, fill: Option<char>) {
    let area = area.intersection(buf.area);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if let Some(ch) = fill {
                    cell.set_char(ch);
                }
                cell.set_style(style);
            }
        }
    }
}

/// Fills `area` with the surface-base token.
pub fn clear_surface(buf: &mut Buffer, area: Rect, theme: &Theme) {
    fill(buf, area, theme.style(Token::SurfaceBase), Some(' '));
}

/// Clears `area` while preserving the terminal's own background.
pub fn clear_transparent(buf: &mut Buffer, area: Rect, theme: &Theme) {
    fill(
        buf,
        area,
        theme.style_transparent(Token::TextPrimary),
        Some(' '),
    );
}

/// Truncates `text` to `width` cells, adding an ASCII ellipsis when clipped.
#[must_use]
pub fn ellipsize(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    if u16::try_from(text.width()).unwrap_or(u16::MAX) <= width {
        return text.to_owned();
    }
    if width <= 3 {
        return ".".repeat(usize::from(width));
    }
    let budget = usize::from(width.saturating_sub(3));
    let mut out = String::new();
    let mut used = 0_usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push_str("...");
    out
}

/// Wraps `text` to `width` cells without splitting mid-word when possible.
#[must_use]
pub fn wrap_cells(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let width = usize::from(width);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut used = 0_usize;
        for word in paragraph.split(' ') {
            let word_width = word.width();
            if current.is_empty() {
                if word_width <= width {
                    current.push_str(word);
                    used = word_width;
                } else {
                    for ch in word.chars() {
                        let w = ch.width().unwrap_or(0);
                        if used + w > width && !current.is_empty() {
                            lines.push(core::mem::take(&mut current));
                            used = 0;
                        }
                        current.push(ch);
                        used += w;
                    }
                }
                continue;
            }
            if used + 1 + word_width <= width {
                current.push(' ');
                current.push_str(word);
                used += 1 + word_width;
            } else {
                lines.push(core::mem::take(&mut current));
                current.push_str(word);
                used = word_width.min(width);
            }
        }
        if !current.is_empty() || paragraph.is_empty() {
            lines.push(current);
        }
    }
    lines
}

/// Right-aligns `text` in `width` cells.
#[must_use]
pub fn right_align(text: &str, width: u16) -> String {
    let used = u16::try_from(text.width()).unwrap_or(u16::MAX);
    if used >= width {
        ellipsize(text, width)
    } else {
        format!("{}{text}", " ".repeat(usize::from(width - used)))
    }
}

#[cfg(test)]
pub(crate) fn render_area(
    width: u16,
    height: u16,
    paint: impl FnOnce(&mut Buffer, Rect),
) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    paint(&mut buf, area);
    buf
}
