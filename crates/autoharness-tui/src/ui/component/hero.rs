//! Centered brand, headline, numbered steps, and a highlighted next action.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::gradient::normalized_t;
use super::super::icon::IconSet;
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::chip::{Chip, ChipVariant};
use super::paint::put;

/// Onboarding or empty-state hero.
pub struct Hero<'a> {
    theme: &'a Theme,
    icons: IconSet,
    brand: &'a str,
    headline: &'a str,
    steps: &'a [&'a str],
    next: &'a str,
}

impl<'a> Hero<'a> {
    /// Creates a hero.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        brand: &'a str,
        headline: &'a str,
        steps: &'a [&'a str],
        next: &'a str,
    ) -> Self {
        Self {
            theme,
            icons,
            brand,
            headline,
            steps,
            next,
        }
    }

    /// Renders the hero centered in `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut y = area.y.saturating_add(area.height.saturating_sub(5) / 2);
        paint_gradient_text(buf, area, y, self.brand, self.theme);
        y = y.saturating_add(1);
        let headline_w = u16::try_from(self.headline.width()).unwrap_or(0);
        let hx = area
            .x
            .saturating_add(area.width.saturating_sub(headline_w) / 2);
        put(
            buf,
            hx,
            y,
            area.width,
            self.headline,
            self.theme.style(Token::TextSecondary),
        );
        y = y.saturating_add(1);
        let mut x = area.x.saturating_add(2);
        for (index, step) in self.steps.iter().enumerate() {
            let label = format!("{}. {step}", index + 1);
            let used = Chip::new(self.theme, &label, ChipVariant::Neutral)
                .render(buf, Rect::new(x, y, area.right().saturating_sub(x), 1));
            x = x.saturating_add(used).saturating_add(1);
        }
        y = y.saturating_add(1);
        let next_w = u16::try_from(self.next.width())
            .unwrap_or(0)
            .saturating_add(2);
        Chip::new(self.theme, self.next, ChipVariant::Accent).render(
            buf,
            Rect::new(
                area.x.saturating_add(area.width.saturating_sub(next_w) / 2),
                y,
                next_w.min(area.width),
                1,
            ),
        );
        let _ = self.icons;
    }
}

fn paint_gradient_text(buf: &mut Buffer, area: Rect, y: u16, text: &str, theme: &Theme) {
    let count = u16::try_from(text.chars().count()).unwrap_or(1).max(1);
    let text_w = u16::try_from(text.width()).unwrap_or(count);
    let mut x = area.x.saturating_add(area.width.saturating_sub(text_w) / 2);
    for (index, ch) in text.chars().enumerate() {
        if x >= area.right() {
            break;
        }
        let mut tmp = [0; 4];
        let glyph = ch.encode_utf8(&mut tmp);
        x = x.saturating_add(put(
            buf,
            x,
            y,
            area.right().saturating_sub(x),
            glyph,
            theme.gradient_style(normalized_t(u16::try_from(index).unwrap_or(0), count)),
        ));
    }
}
