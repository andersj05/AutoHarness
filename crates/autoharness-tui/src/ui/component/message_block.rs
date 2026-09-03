//! Conversation turn with a compact role gutter, metadata, and hanging body.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{put, wrap_cells};

/// One conversation block.
pub struct MessageBlock<'a> {
    theme: &'a Theme,
    icons: IconSet,
    role: Icon,
    role_name: &'a str,
    metadata: &'a str,
    body: &'a str,
}

impl<'a> MessageBlock<'a> {
    /// Creates a message block.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        role: Icon,
        role_name: &'a str,
        metadata: &'a str,
        body: &'a str,
    ) -> Self {
        Self {
            theme,
            icons,
            role,
            role_name,
            metadata,
            body,
        }
    }

    /// Rows required at `width`.
    #[must_use]
    pub fn measure(&self, width: u16) -> u16 {
        let inner = width.saturating_sub(2).max(1);
        1 + u16::try_from(wrap_cells(self.body, inner).len()).unwrap_or(1)
    }

    /// Renders the block and returns the used height.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return 0;
        }
        let role_token = match self.role {
            Icon::User => Token::RoleUser,
            Icon::Tool => Token::RoleTool,
            _ => Token::RoleAssistant,
        };
        put(
            buf,
            area.x,
            area.y,
            self.icons.width(self.role),
            self.icons.glyph(self.role),
            self.theme.style(role_token),
        );
        let body_x = area.x.saturating_add(2);
        let height = self.measure(area.width).min(area.height);
        put(
            buf,
            body_x,
            area.y,
            area.right().saturating_sub(body_x),
            self.role_name,
            self.theme.style(role_token),
        );
        let meta_w = u16::try_from(self.metadata.chars().count()).unwrap_or(0);
        if meta_w > 0 && area.width > meta_w + 6 {
            put(
                buf,
                area.right().saturating_sub(meta_w),
                area.y,
                meta_w,
                self.metadata,
                self.theme.style(Token::TextMuted),
            );
        }
        let lines = wrap_cells(self.body, area.width.saturating_sub(2).max(1));
        for (index, line) in lines.iter().enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(0).saturating_add(1));
            if y >= area.bottom() {
                break;
            }
            put(
                buf,
                body_x,
                y,
                area.right().saturating_sub(body_x),
                line,
                self.theme.style(Token::TextPrimary),
            );
        }
        height.min(area.height)
    }
}
