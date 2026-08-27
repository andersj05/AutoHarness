//! Label column sized to the widest label, hanging value wrap, trailing chip.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;
use super::super::tokens::Token;
use super::chip::{Chip, ChipVariant};
use super::paint::{put, wrap_cells};

/// One table row.
pub struct KeyValue<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub chip: Option<&'a str>,
}

/// Aligned key/value table.
pub struct KeyValueTable<'a> {
    theme: &'a Theme,
    rows: &'a [KeyValue<'a>],
}

impl<'a> KeyValueTable<'a> {
    /// Creates a table.
    #[must_use]
    pub const fn new(theme: &'a Theme, rows: &'a [KeyValue<'a>]) -> Self {
        Self { theme, rows }
    }

    /// Label column width from the widest label.
    #[must_use]
    pub fn label_width(&self) -> u16 {
        self.rows
            .iter()
            .map(|row| u16::try_from(row.label.width()).unwrap_or(0))
            .max()
            .unwrap_or(0)
    }

    /// Rows consumed including wrapped values.
    #[must_use]
    pub fn measure(&self, width: u16) -> u16 {
        let label_w = self.label_width();
        let value_w = width.saturating_sub(label_w.saturating_add(2));
        self.rows
            .iter()
            .map(|row| u16::try_from(wrap_cells(row.value, value_w.max(1)).len()).unwrap_or(1))
            .fold(0_u16, u16::saturating_add)
            .max(u16::try_from(self.rows.len()).unwrap_or(0))
    }

    /// Renders the table. Every value column starts at the same x.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return 0;
        }
        let label_w = self.label_width().min(area.width);
        let value_x = area.x.saturating_add(label_w.saturating_add(1));
        let mut y = area.y;
        for row in self.rows {
            if y >= area.bottom() {
                break;
            }
            let padded = format!("{:width$}", row.label, width = usize::from(label_w));
            put(
                buf,
                area.x,
                y,
                label_w,
                &padded,
                self.theme.style(Token::TextMuted),
            );
            let chip_w = row
                .chip
                .map(|chip| u16::try_from(chip.width()).unwrap_or(0).saturating_add(2))
                .unwrap_or(0);
            let value_width = area
                .right()
                .saturating_sub(value_x)
                .saturating_sub(if chip_w == 0 {
                    0
                } else {
                    chip_w.saturating_add(1)
                });
            let lines = wrap_cells(row.value, value_width.max(1));
            for (index, line) in lines.iter().enumerate() {
                if y >= area.bottom() {
                    break;
                }
                put(
                    buf,
                    value_x,
                    y,
                    value_width,
                    line,
                    self.theme.style(Token::TextPrimary),
                );
                if index == 0
                    && let Some(chip) = row.chip
                {
                    Chip::new(self.theme, chip, ChipVariant::Muted).render(
                        buf,
                        Rect::new(area.right().saturating_sub(chip_w), y, chip_w, 1),
                    );
                }
                y = y.saturating_add(1);
            }
            if lines.is_empty() {
                y = y.saturating_add(1);
            }
        }
        value_x
    }
}
