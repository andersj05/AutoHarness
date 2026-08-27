//! Collapsible tool call card.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{put, wrap_cells};

/// Tool invocation card.
pub struct ToolCard<'a> {
    theme: &'a Theme,
    icons: IconSet,
    name: &'a str,
    target: &'a str,
    duration: &'a str,
    detail: &'a str,
    expanded: bool,
    status: Icon,
}

impl<'a> ToolCard<'a> {
    /// Creates a tool card.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        name: &'a str,
        target: &'a str,
        duration: &'a str,
        detail: &'a str,
        expanded: bool,
        status: Icon,
    ) -> Self {
        Self {
            theme,
            icons,
            name,
            target,
            duration,
            detail,
            expanded,
            status,
        }
    }

    /// Rows required at `width`.
    #[must_use]
    pub fn measure(&self, width: u16) -> u16 {
        if self.expanded {
            1 + u16::try_from(wrap_cells(self.detail, width.saturating_sub(4).max(1)).len())
                .unwrap_or(1)
        } else {
            1
        }
    }

    /// Renders the card.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return 0;
        }
        let caret = if self.expanded {
            self.icons.glyph(Icon::Expanded)
        } else {
            self.icons.glyph(Icon::Collapsed)
        };
        let mut x = area.x;
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.width,
            caret,
            self.theme.style(Token::TextMuted),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            1,
            " ",
            self.theme.style(Token::TextMuted),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            self.icons.glyph(self.status),
            self.theme.style(Token::Success),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            1,
            " ",
            self.theme.style(Token::TextMuted),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            self.name,
            self.theme.style(Token::RoleTool),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            " ",
            self.theme.style(Token::TextMuted),
        ));
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            self.target,
            self.theme.style(Token::TextSecondary),
        ));
        put(
            buf,
            area.right()
                .saturating_sub(u16::try_from(self.duration.chars().count()).unwrap_or(0)),
            area.y,
            u16::try_from(self.duration.chars().count()).unwrap_or(0),
            self.duration,
            self.theme.style(Token::TextMuted),
        );
        let mut height = 1;
        if self.expanded {
            let lines = wrap_cells(self.detail, area.width.saturating_sub(4).max(1));
            for (index, line) in lines.iter().enumerate() {
                let y = area
                    .y
                    .saturating_add(u16::try_from(index).unwrap_or(0).saturating_add(1));
                if y >= area.bottom() {
                    break;
                }
                put(
                    buf,
                    area.x.saturating_add(4),
                    y,
                    area.width.saturating_sub(4),
                    line,
                    self.theme.style(Token::TextSecondary),
                );
                height += 1;
            }
        }
        let _ = x;
        height.min(area.height)
    }
}
