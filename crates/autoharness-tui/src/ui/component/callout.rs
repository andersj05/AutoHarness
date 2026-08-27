//! Bordered semantic block with icon, title, message, and optional buttons.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::button_row::{Button, ButtonRow};
use super::paint::{put, wrap_cells};
use super::panel::Panel;

/// Semantic callout.
pub struct Callout<'a, A> {
    theme: &'a Theme,
    icons: IconSet,
    icon: Icon,
    title: &'a str,
    message: &'a str,
    buttons: &'a [Button<A>],
}

impl<'a, A: Clone> Callout<'a, A> {
    /// Creates a callout.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        icon: Icon,
        title: &'a str,
        message: &'a str,
        buttons: &'a [Button<A>],
    ) -> Self {
        Self {
            theme,
            icons,
            icon,
            title,
            message,
            buttons,
        }
    }

    /// Renders the callout and returns button hits.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Vec<(Rect, A)> {
        let inner = Panel::new(
            self.theme,
            self.icons,
            Some(self.icon),
            Some(self.title),
            None,
            None,
            false,
        )
        .render(buf, area);
        if inner.width == 0 || inner.height == 0 {
            return Vec::new();
        }
        let lines = wrap_cells(self.message, inner.width.max(1));
        let mut y = inner.y;
        for line in &lines {
            if y >= inner.bottom() {
                break;
            }
            put(
                buf,
                inner.x,
                y,
                inner.width,
                line,
                self.theme.style(Token::TextPrimary),
            );
            y = y.saturating_add(1);
        }
        if self.buttons.is_empty() || y >= inner.bottom() {
            return Vec::new();
        }
        ButtonRow::new(self.theme, self.buttons).render(buf, Rect::new(inner.x, y, inner.width, 1))
    }
}
