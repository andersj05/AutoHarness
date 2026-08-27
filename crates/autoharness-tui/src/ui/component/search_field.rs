//! Icon, inline query, visible cursor, and right-aligned match count.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{ellipsize, put, right_align};

/// Search field used by palettes, session filters, and Settings search.
pub struct SearchField<'a> {
    theme: &'a Theme,
    icons: IconSet,
    query: &'a str,
    cursor: usize,
    matches: Option<u32>,
    focused: bool,
}

impl<'a> SearchField<'a> {
    /// Creates a search field.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        query: &'a str,
        cursor: usize,
        matches: Option<u32>,
        focused: bool,
    ) -> Self {
        Self {
            theme,
            icons,
            query,
            cursor,
            matches,
            focused,
        }
    }

    /// Always one row.
    #[must_use]
    pub const fn measure(&self) -> u16 {
        1
    }

    /// Renders the field and returns the cursor column relative to `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return area.x;
        }
        let icon_style = if self.focused {
            self.theme.style(Token::Accent)
        } else {
            self.theme.style(Token::TextMuted)
        };
        let mut x = area.x;
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.width,
            self.icons.glyph(Icon::Search),
            icon_style,
        ));
        if x < area.right() {
            x = x.saturating_add(put(
                buf,
                x,
                area.y,
                area.right().saturating_sub(x),
                " ",
                self.theme.style(Token::TextPrimary),
            ));
        }
        let count = self.matches.map(|n| format!("{n}"));
        let count_w = count
            .as_ref()
            .map(|value| u16::try_from(value.len()).unwrap_or(u16::MAX))
            .unwrap_or(0);
        let query_width = area.right().saturating_sub(x).saturating_sub(count_w);
        let query = ellipsize(self.query, query_width);
        put(
            buf,
            x,
            area.y,
            query_width,
            &query,
            self.theme.style(Token::TextPrimary),
        );
        if let Some(count) = count {
            let aligned = right_align(&count, count_w.max(1));
            put(
                buf,
                area.right().saturating_sub(count_w.max(1)),
                area.y,
                count_w.max(1),
                &aligned,
                self.theme.style(Token::TextMuted),
            );
        }
        let cursor_offset = u16::try_from(self.cursor.min(query.chars().count())).unwrap_or(0);
        x.saturating_add(cursor_offset)
    }
}
