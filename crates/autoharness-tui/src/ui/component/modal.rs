//! Scrim plus panel plus body plus a right-aligned button row.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::icon::{Icon, IconSet};
use super::super::metrics::{
    MODAL_FULL_HEIGHT, MODAL_FULL_WIDTH, MODAL_MARGIN_X, MODAL_MARGIN_Y, MODAL_MAX_HEIGHT,
    MODAL_MAX_WIDTH,
};
use super::super::theme::Theme;
use super::button_row::{Button, ButtonRow};
use super::panel::Panel;
use super::scrim;

/// Single clamp table for every overlay.
#[must_use]
pub fn size(host: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    if host.width < MODAL_FULL_WIDTH || host.height < MODAL_FULL_HEIGHT {
        return host;
    }
    let width = preferred_width
        .min(MODAL_MAX_WIDTH)
        .min(host.width.saturating_sub(MODAL_MARGIN_X.saturating_mul(2)))
        .max(1);
    let height = preferred_height
        .min(MODAL_MAX_HEIGHT)
        .min(host.height.saturating_sub(MODAL_MARGIN_Y.saturating_mul(2)))
        .max(1);
    Rect::new(
        host.x + host.width.saturating_sub(width) / 2,
        host.y + host.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// Modal frame with a footer of buttons.
pub struct Modal<'a, A> {
    theme: &'a Theme,
    icons: IconSet,
    title: &'a str,
    icon: Option<Icon>,
    buttons: &'a [Button<A>],
}

impl<'a, A: Clone> Modal<'a, A> {
    /// Creates a modal.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        title: &'a str,
        icon: Option<Icon>,
        buttons: &'a [Button<A>],
    ) -> Self {
        Self {
            theme,
            icons,
            title,
            icon,
            buttons,
        }
    }

    /// Renders scrim, frame, and footer. Returns inner body rect and button hits.
    pub fn render(
        &self,
        buf: &mut Buffer,
        host: Rect,
        preferred_width: u16,
        preferred_height: u16,
    ) -> (Rect, Vec<(Rect, A)>) {
        scrim::render(buf, host, self.theme);
        let frame = size(host, preferred_width, preferred_height);
        let inner = Panel::new(
            self.theme,
            self.icons,
            self.icon,
            Some(self.title),
            None,
            None,
            true,
        )
        .render(buf, frame);
        let footer = Rect::new(
            inner.x,
            inner.bottom().saturating_sub(1),
            inner.width,
            1.min(inner.height),
        );
        let hits = ButtonRow::new(self.theme, self.buttons).render(buf, footer);
        let body = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(u16::from(footer.height > 0)),
        };
        (body, hits)
    }
}
