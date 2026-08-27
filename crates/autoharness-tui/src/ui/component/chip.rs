//! One-cell-padded semantic label.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{ellipsize, put};

/// Chip color pairing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChipVariant {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    Muted,
}

/// Compact labeled pill.
pub struct Chip<'a> {
    theme: &'a Theme,
    label: &'a str,
    variant: ChipVariant,
}

impl<'a> Chip<'a> {
    /// Creates a chip.
    #[must_use]
    pub const fn new(theme: &'a Theme, label: &'a str, variant: ChipVariant) -> Self {
        Self {
            theme,
            label,
            variant,
        }
    }

    /// Cells required including padding.
    #[must_use]
    pub fn measure(&self) -> u16 {
        let inner = u16::try_from(self.label.chars().count()).unwrap_or(u16::MAX);
        inner.saturating_add(2).max(3)
    }

    /// Renders into `area` and returns the used width.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return 0;
        }
        let style = match self.variant {
            ChipVariant::Neutral => self.theme.style(Token::SurfaceRaised),
            ChipVariant::Accent => self.theme.filled(Token::Accent),
            ChipVariant::Success => self.theme.style(Token::SuccessSoft),
            ChipVariant::Warning => self.theme.style(Token::WarningSoft),
            ChipVariant::Danger => self.theme.style(Token::DangerSoft),
            ChipVariant::Muted => self.theme.style(Token::TextMuted),
        };
        let body = ellipsize(self.label, area.width.saturating_sub(2));
        let text = format!(" {body} ");
        put(buf, area.x, area.y, area.width, &text, style)
    }
}
