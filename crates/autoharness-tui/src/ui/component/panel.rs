//! Optional icon, title, subtitle, footer, and focused gradient perimeter.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::gradient::normalized_t;
use super::super::icon::{Icon, IconSet};
use super::super::metrics::{PANEL_PAD_X, PANEL_PAD_Y, PANEL_PAD_Y_TITLED};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{clear_surface, ellipsize_words_with, put};

/// Framed panel.
pub struct Panel<'a> {
    theme: &'a Theme,
    icons: IconSet,
    icon: Option<Icon>,
    title: Option<&'a str>,
    subtitle: Option<&'a str>,
    footer: Option<&'a str>,
    focused: bool,
}

impl<'a> Panel<'a> {
    /// Creates a panel.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        icon: Option<Icon>,
        title: Option<&'a str>,
        subtitle: Option<&'a str>,
        footer: Option<&'a str>,
        focused: bool,
    ) -> Self {
        Self {
            theme,
            icons,
            icon,
            title,
            subtitle,
            footer,
            focused,
        }
    }

    /// Chrome height besides the inner body.
    #[must_use]
    pub fn chrome_height(&self) -> u16 {
        let mut height = 2;
        if self.title.is_some() {
            height += PANEL_PAD_Y_TITLED;
        } else {
            height += PANEL_PAD_Y;
        }
        if self.subtitle.is_some() {
            height += 1;
        }
        if self.footer.is_some() {
            height += 1;
        }
        height
    }

    /// Inner content rectangle after painting the frame.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Rect {
        if area.width == 0 || area.height == 0 {
            return area;
        }
        clear_surface(buf, area, self.theme);
        paint_border(buf, area, self.theme, self.icons, self.focused);
        let mut y = area.y.saturating_add(1);
        if let Some(title) = self.title {
            let mut x = area.x.saturating_add(1);
            if let Some(icon) = self.icon {
                x = x.saturating_add(put(
                    buf,
                    x,
                    y,
                    area.width.saturating_sub(2),
                    self.icons.glyph(icon),
                    self.theme.style(Token::Accent),
                ));
                x = x.saturating_add(put(buf, x, y, 1, " ", self.theme.style(Token::TextPrimary)));
            }
            let width = area.right().saturating_sub(x.saturating_add(1));
            let title = ellipsize_words_with(title, width, self.icons.ellipsis());
            put(
                buf,
                x,
                y,
                width,
                &title,
                self.theme.style(Token::TextPrimary),
            );
            y = y.saturating_add(1);
        }
        if let Some(subtitle) = self.subtitle
            && y < area.bottom().saturating_sub(1)
        {
            let width = area.width.saturating_sub(2);
            let subtitle = ellipsize_words_with(subtitle, width, self.icons.ellipsis());
            put(
                buf,
                area.x.saturating_add(1),
                y,
                width,
                &subtitle,
                self.theme.style(Token::TextMuted),
            );
            y = y.saturating_add(1);
        }
        if let Some(footer) = self.footer
            && area.height > 2
        {
            let width = area.width.saturating_sub(2);
            let footer = ellipsize_words_with(footer, width, self.icons.ellipsis());
            put(
                buf,
                area.x.saturating_add(1),
                area.bottom().saturating_sub(2),
                width,
                &footer,
                self.theme.style(Token::TextMuted),
            );
        }
        let bottom_inset = 1 + u16::from(self.footer.is_some());
        Rect {
            x: area.x.saturating_add(PANEL_PAD_X),
            y,
            width: area.width.saturating_sub(PANEL_PAD_X.saturating_mul(2)),
            height: area.bottom().saturating_sub(bottom_inset).saturating_sub(y),
        }
    }
}

fn paint_border(buf: &mut Buffer, area: Rect, theme: &Theme, icons: IconSet, focused: bool) {
    let border = icons.border();
    let width = area.width;
    let height = area.height;
    if width == 0 || height == 0 {
        return;
    }
    let perimeter = width
        .saturating_mul(2)
        .saturating_add(height.saturating_mul(2))
        .saturating_sub(4)
        .max(1);
    let mut index = 0_u16;
    let mut paint = |x: u16, y: u16, glyph: &'static str| {
        let style = if focused {
            theme.gradient_style(normalized_t(index, perimeter))
        } else {
            theme.style(Token::BorderSubtle)
        };
        put(buf, x, y, 1, glyph, style);
        index = index.saturating_add(1);
    };
    paint(area.x, area.y, border.top_left);
    for x in 1..width.saturating_sub(1) {
        paint(area.x.saturating_add(x), area.y, border.horizontal);
    }
    if width > 1 {
        paint(area.right().saturating_sub(1), area.y, border.top_right);
    }
    for y in 1..height.saturating_sub(1) {
        paint(
            area.right().saturating_sub(1),
            area.y.saturating_add(y),
            border.vertical,
        );
    }
    if height > 1 {
        paint(
            area.right().saturating_sub(1),
            area.bottom().saturating_sub(1),
            border.bottom_right,
        );
    }
    for x in (1..width.saturating_sub(1)).rev() {
        paint(
            area.x.saturating_add(x),
            area.bottom().saturating_sub(1),
            border.horizontal,
        );
    }
    if height > 1 && width > 1 {
        paint(area.x, area.bottom().saturating_sub(1), border.bottom_left);
    }
    for y in (1..height.saturating_sub(1)).rev() {
        paint(area.x, area.y.saturating_add(y), border.vertical);
    }
}
