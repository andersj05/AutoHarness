//! Measured button row that exports exact hit regions.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::put;

/// Visual weight for a button.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Danger,
}

/// One labeled button with a key annotation and typed action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button<A> {
    /// Visible label, without brackets.
    pub label: String,
    /// Optional key hint rendered after the label.
    pub key: Option<String>,
    /// Color treatment.
    pub variant: ButtonVariant,
    /// Action returned in the hit region.
    pub action: A,
}

impl<A> Button<A> {
    /// Creates a button.
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        key: Option<String>,
        variant: ButtonVariant,
        action: A,
    ) -> Self {
        Self {
            label: label.into(),
            key,
            variant,
            action,
        }
    }

    fn caption(&self) -> String {
        match &self.key {
            Some(key) => format!("[ {} ({key}) ]", self.label),
            None => format!("[ {} ]", self.label),
        }
    }

    fn width(&self) -> u16 {
        u16::try_from(self.caption().width()).unwrap_or(u16::MAX)
    }
}

/// Right-aligned row of buttons.
pub struct ButtonRow<'a, A> {
    theme: &'a Theme,
    buttons: &'a [Button<A>],
}

impl<'a, A: Clone> ButtonRow<'a, A> {
    /// Creates a button row.
    #[must_use]
    pub const fn new(theme: &'a Theme, buttons: &'a [Button<A>]) -> Self {
        Self { theme, buttons }
    }

    /// Total cells including one-cell gaps.
    #[must_use]
    pub fn measure(&self) -> u16 {
        self.buttons
            .iter()
            .map(Button::width)
            .fold(0_u16, |acc, width| {
                if acc == 0 {
                    width
                } else {
                    acc.saturating_add(1).saturating_add(width)
                }
            })
    }

    /// Renders right-aligned buttons and returns hit regions matching captions.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Vec<(Rect, A)> {
        if area.width == 0 || area.height == 0 || self.buttons.is_empty() {
            return Vec::new();
        }
        let hits = self.regions(area);
        for (index, (button, (rect, _))) in self.buttons.iter().zip(&hits).enumerate() {
            if index > 0 && rect.x > area.x {
                put(
                    buf,
                    rect.x.saturating_sub(1),
                    rect.y,
                    1,
                    " ",
                    self.theme.style(Token::TextMuted),
                );
            }
            let caption = button.caption();
            put(
                buf,
                rect.x,
                rect.y,
                rect.width,
                &caption,
                button_style(self.theme, button.variant),
            );
        }
        hits
    }

    /// Returns the exact right-aligned caption rectangles without painting.
    #[must_use]
    pub fn regions(&self, area: Rect) -> Vec<(Rect, A)> {
        let mut hits = Vec::new();
        if area.width == 0 || area.height == 0 || self.buttons.is_empty() {
            return hits;
        }
        let total = self.measure().min(area.width);
        let mut x = area.x.saturating_add(area.width.saturating_sub(total));
        for (index, button) in self.buttons.iter().enumerate() {
            if index > 0 {
                x = x.saturating_add(1);
            }
            let width = button.width().min(area.right().saturating_sub(x));
            if width == 0 {
                break;
            }
            hits.push((Rect::new(x, area.y, width, 1), button.action.clone()));
            x = x.saturating_add(width);
        }
        hits
    }
}

fn button_style(theme: &Theme, variant: ButtonVariant) -> Style {
    match variant {
        ButtonVariant::Primary => theme.filled(Token::Accent),
        ButtonVariant::Secondary => theme.style(Token::BorderStrong),
        ButtonVariant::Danger => theme.filled(Token::Danger),
    }
}
