//! Choice chips that collapse below the medium breakpoint.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::Icon;
use super::super::metrics::{WidthBand, width_band};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::chip::{Chip, ChipVariant};
use super::paint::put;

/// Horizontal choice control.
pub struct SegmentedControl<'a> {
    theme: &'a Theme,
    options: &'a [&'a str],
    selected: usize,
    focused: bool,
}

impl<'a> SegmentedControl<'a> {
    /// Creates a segmented control.
    #[must_use]
    pub const fn new(theme: &'a Theme, options: &'a [&'a str], selected: usize) -> Self {
        Self {
            theme,
            options,
            selected,
            focused: false,
        }
    }

    /// Marks the control as the active keyboard target.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Cells required at `width`.
    #[must_use]
    pub fn measure(&self, width: u16) -> u16 {
        if matches!(
            width_band(width),
            WidthBand::Md | WidthBand::Lg | WidthBand::Xl
        ) {
            self.options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    u16::try_from((*option).width())
                        .unwrap_or(u16::MAX)
                        .saturating_add(3)
                        .saturating_add(if index == self.selected {
                            self.theme.icons().width(Icon::Success).saturating_add(1)
                        } else {
                            0
                        })
                })
                .fold(0_u16, |acc, item| {
                    if acc == 0 {
                        item
                    } else {
                        acc.saturating_add(1).saturating_add(item)
                    }
                })
        } else {
            12
        }
    }

    /// Renders chips at Md+ or a compact selected-value indicator below Md.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.width == 0 || area.height == 0 || self.options.is_empty() {
            return;
        }
        if matches!(
            width_band(area.width),
            WidthBand::Md | WidthBand::Lg | WidthBand::Xl
        ) {
            let mut x = area.x;
            for (index, option) in self.options.iter().enumerate() {
                if x >= area.right() {
                    break;
                }
                let selected = index == self.selected;
                let variant = if selected {
                    ChipVariant::Accent
                } else {
                    ChipVariant::Neutral
                };
                let label = if selected {
                    let marker = if self.focused {
                        self.theme.icons().glyph(Icon::SelectionCaret)
                    } else {
                        self.theme.icons().glyph(Icon::Success)
                    };
                    format!("{marker} {option}")
                } else {
                    (*option).to_owned()
                };
                let used = Chip::new(self.theme, &label, variant)
                    .render(buf, Rect::new(x, area.y, area.right().saturating_sub(x), 1));
                x = x.saturating_add(used).saturating_add(1);
            }
            return;
        }
        let selected = self.options.get(self.selected).copied().unwrap_or("");
        let marker = if self.focused {
            self.theme.icons().glyph(Icon::SelectionCaret)
        } else {
            self.theme.icons().glyph(Icon::Success)
        };
        let compact = format!(
            "{marker} {selected}  {}/{}",
            self.selected.saturating_add(1),
            self.options.len()
        );
        put(
            buf,
            area.x,
            area.y,
            area.width,
            &compact,
            if self.focused {
                self.theme.filled(Token::Accent)
            } else {
                self.theme.style(Token::AccentSoft)
            },
        );
    }
}
