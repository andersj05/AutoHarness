//! Priority-ordered status segments that drop the lowest priority until the line fits.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::put;

/// One status segment. Lower `priority` is kept longer.
pub struct StatusSegment<'a> {
    pub priority: u8,
    pub icon: Option<Icon>,
    pub text: &'a str,
}

/// Priority-dropping status bar.
pub struct StatusBar<'a> {
    theme: &'a Theme,
    icons: IconSet,
    segments: &'a [StatusSegment<'a>],
    separator: &'a str,
}

impl<'a> StatusBar<'a> {
    /// Creates a status bar.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        segments: &'a [StatusSegment<'a>],
        separator: &'a str,
    ) -> Self {
        Self {
            theme,
            icons,
            segments,
            separator,
        }
    }

    fn segment_width(&self, segment: &StatusSegment<'a>) -> u16 {
        let icon_w = segment.icon.map(|icon| self.icons.width(icon)).unwrap_or(0);
        let gap = u16::from(icon_w > 0);
        icon_w
            .saturating_add(gap)
            .saturating_add(u16::try_from(segment.text.width()).unwrap_or(0))
    }

    /// Segments that fit `width`, dropping highest priority numbers first.
    #[must_use]
    pub fn visible(&self, width: u16) -> Vec<usize> {
        let mut keep: Vec<usize> = (0..self.segments.len()).collect();
        loop {
            let used = self.width_of(&keep);
            if used <= width || keep.len() <= 1 {
                return keep;
            }
            let drop_at = keep
                .iter()
                .copied()
                .max_by_key(|index| self.segments[*index].priority)
                .unwrap_or(0);
            keep.retain(|index| *index != drop_at);
        }
    }

    fn width_of(&self, indices: &[usize]) -> u16 {
        let sep = u16::try_from(self.separator.width()).unwrap_or(0);
        indices
            .iter()
            .enumerate()
            .fold(0_u16, |acc, (slot, index)| {
                let seg = self.segment_width(&self.segments[*index]);
                if slot == 0 {
                    seg
                } else {
                    acc.saturating_add(sep).saturating_add(seg)
                }
            })
    }

    /// Renders the bar and returns the indices still visible.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Vec<usize> {
        let visible = self.visible(area.width);
        if area.width == 0 || area.height == 0 {
            return visible;
        }
        let mut x = area.x;
        for (slot, index) in visible.iter().copied().enumerate() {
            if slot > 0 {
                x = x.saturating_add(put(
                    buf,
                    x,
                    area.y,
                    area.right().saturating_sub(x),
                    self.separator,
                    self.theme.style(Token::Divider),
                ));
            }
            let segment = &self.segments[index];
            if let Some(icon) = segment.icon {
                x = x.saturating_add(put(
                    buf,
                    x,
                    area.y,
                    area.right().saturating_sub(x),
                    self.icons.glyph(icon),
                    self.theme.style(Token::Accent),
                ));
                x = x.saturating_add(put(
                    buf,
                    x,
                    area.y,
                    area.right().saturating_sub(x),
                    " ",
                    self.theme.style(Token::TextSecondary),
                ));
            }
            x = x.saturating_add(put(
                buf,
                x,
                area.y,
                area.right().saturating_sub(x),
                segment.text,
                self.theme.style(Token::TextPrimary),
            ));
        }
        visible
    }
}
