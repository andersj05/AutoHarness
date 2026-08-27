//! Segmented gradient fill with a label, value, and threshold override.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::super::gradient::normalized_t;
use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::put;

/// Threshold that replaces the gradient with a semantic fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeterThreshold {
    None,
    Warning,
    Danger,
}

/// Labeled segmented meter.
pub struct Meter<'a> {
    theme: &'a Theme,
    icons: IconSet,
    icon: Icon,
    label: &'a str,
    value: &'a str,
    filled: u16,
    total: u16,
    threshold: MeterThreshold,
}

impl<'a> Meter<'a> {
    /// Creates a meter with `filled` of `total` segments.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        icon: Icon,
        label: &'a str,
        value: &'a str,
        filled: u16,
        total: u16,
        threshold: MeterThreshold,
    ) -> Self {
        Self {
            theme,
            icons,
            icon,
            label,
            value,
            filled,
            total,
            threshold,
        }
    }

    /// Minimum cells required for icon, label, segments, and value.
    #[must_use]
    pub fn measure(&self) -> u16 {
        let icon_w = self.icons.width(self.icon);
        let label_w = u16::try_from(self.label.chars().count()).unwrap_or(u16::MAX);
        let value_w = u16::try_from(self.value.chars().count()).unwrap_or(u16::MAX);
        icon_w
            .saturating_add(1)
            .saturating_add(label_w)
            .saturating_add(1)
            .saturating_add(self.total.max(1))
            .saturating_add(1)
            .saturating_add(value_w)
    }

    /// Renders the meter into `area`.
    pub fn render(&self, buf: &mut Buffer, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut x = area.x;
        x = x.saturating_add(put(
            buf,
            x,
            area.y,
            area.width,
            self.icons.glyph(self.icon),
            self.theme.style(Token::Accent),
        ));
        if x + 1 < area.right() {
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
            self.label,
            self.theme.style(Token::TextSecondary),
        ));
        if x + 1 < area.right() {
            x = x.saturating_add(put(
                buf,
                x,
                area.y,
                area.right().saturating_sub(x),
                " ",
                self.theme.style(Token::TextSecondary),
            ));
        }
        let total = self.total.max(1);
        for index in 0..total {
            if x >= area.right() {
                break;
            }
            let filled = index < self.filled;
            let glyph = if filled { "▰" } else { "▱" };
            let style = segment_style(self.theme, filled, index, total, self.threshold);
            x = x.saturating_add(put(buf, x, area.y, 1, glyph, style));
        }
        if x + 1 < area.right() {
            x = x.saturating_add(put(
                buf,
                x,
                area.y,
                area.right().saturating_sub(x),
                " ",
                self.theme.style(Token::TextMuted),
            ));
        }
        put(
            buf,
            x,
            area.y,
            area.right().saturating_sub(x),
            self.value,
            self.theme.style(Token::TextMuted),
        );
    }
}

fn segment_style(
    theme: &Theme,
    filled: bool,
    index: u16,
    total: u16,
    threshold: MeterThreshold,
) -> Style {
    if !filled {
        return theme.style(Token::TextMuted);
    }
    match threshold {
        MeterThreshold::Danger => theme.style(Token::Danger),
        MeterThreshold::Warning => theme.style(Token::Warning),
        MeterThreshold::None => theme.gradient_emphasis_style(normalized_t(index, total)),
    }
}
