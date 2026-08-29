//! Setting row whose variant is the single editability authority.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::Icon;
use super::super::metrics::SETTINGS_ROW_SELECTION_INSET;
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::chip::{Chip, ChipVariant};
use super::paint::{fill, put};
use super::segmented::SegmentedControl;

/// Provenance layer chip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance {
    Default,
    User,
    Workspace,
    Env,
    Runtime,
    Profile,
    Policy,
    System,
}

impl Provenance {
    /// Short chip label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Env => "env",
            Self::Runtime => "runtime",
            Self::Profile => "profile",
            Self::Policy => "policy",
            Self::System => "system",
        }
    }
}

/// Row control kind. Info is never selectable.
pub enum SettingKind<'a> {
    Toggle {
        on: bool,
    },
    Choice {
        options: &'a [&'a str],
        selected: usize,
    },
    Text {
        value: &'a str,
        max_len: usize,
    },
    Action {
        label: &'a str,
    },
    Info {
        value: &'a str,
    },
}

impl SettingKind<'_> {
    /// Whether the row can receive selection.
    #[must_use]
    pub const fn editable(&self) -> bool {
        !matches!(self, Self::Info { .. })
    }
}

/// One settings row: label, control, provenance chip, optional description.
pub struct SettingRow<'a> {
    theme: &'a Theme,
    label: &'a str,
    kind: SettingKind<'a>,
    provenance: Provenance,
    description: Option<&'a str>,
    focused: bool,
    label_width: u16,
}

impl<'a> SettingRow<'a> {
    /// Creates a setting row.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        label: &'a str,
        kind: SettingKind<'a>,
        provenance: Provenance,
        description: Option<&'a str>,
        focused: bool,
        label_width: u16,
    ) -> Self {
        Self {
            theme,
            label,
            kind,
            provenance,
            description,
            focused,
            label_width,
        }
    }

    /// Rows consumed: one, plus a description row when focused.
    #[must_use]
    pub fn measure(&self) -> u16 {
        if self.focused && self.description.is_some() {
            2
        } else {
            1
        }
    }

    /// Renders the row. Returns the control rectangle.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Rect {
        if area.width == 0 || area.height == 0 {
            return area;
        }
        if self.focused {
            fill(
                buf,
                Rect::new(area.x, area.y, area.width, self.measure().min(area.height)),
                self.theme.style(Token::SurfaceSelectedMuted),
                Some(' '),
            );
        }
        let marker_width = SETTINGS_ROW_SELECTION_INSET.min(area.width);
        if self.focused {
            put(
                buf,
                area.x,
                area.y,
                marker_width,
                self.theme.icons().glyph(Icon::SelectionCaret),
                self.theme.gradient_emphasis_style(0.0),
            );
        }
        let content_x = area.x.saturating_add(marker_width);
        let content_width = area.right().saturating_sub(content_x);
        let label_style = if self.kind.editable() {
            if self.focused {
                self.theme.style(Token::TextPrimary)
            } else {
                self.theme.style(Token::TextSecondary)
            }
        } else {
            self.theme.style(Token::TextMuted)
        };
        let label_width = self.label_width.min(content_width);
        let padded = format!("{:width$}", self.label, width = usize::from(label_width));
        put(buf, content_x, area.y, label_width, &padded, label_style);
        let control_x = content_x.saturating_add(label_width.saturating_add(1));
        let chip_label = self.provenance.label();
        let chip_w = u16::try_from(chip_label.width())
            .unwrap_or(0)
            .saturating_add(2);
        let control_width = area
            .right()
            .saturating_sub(control_x)
            .saturating_sub(chip_w.saturating_add(1));
        let control = Rect::new(control_x, area.y, control_width, 1);
        match self.kind {
            SettingKind::Toggle { on } => {
                SegmentedControl::new(self.theme, &["on", "off"], usize::from(!on))
                    .focused(self.focused)
                    .render(buf, control);
            }
            SettingKind::Choice { options, selected } => {
                SegmentedControl::new(self.theme, options, selected)
                    .focused(self.focused)
                    .render(buf, control);
            }
            SettingKind::Text { value, max_len } => {
                let indicator = format!("{}/{}", value.chars().count(), max_len);
                let value_width = control.width.saturating_sub(
                    u16::try_from(indicator.len())
                        .unwrap_or(0)
                        .saturating_add(1),
                );
                let used = put(
                    buf,
                    control.x,
                    area.y,
                    value_width,
                    value,
                    self.theme.style(Token::TextPrimary),
                );
                if self.focused && used < value_width {
                    put(
                        buf,
                        control.x.saturating_add(used),
                        area.y,
                        1,
                        "|",
                        self.theme.style(Token::FocusRing),
                    );
                }
                put(
                    buf,
                    control
                        .right()
                        .saturating_sub(u16::try_from(indicator.len()).unwrap_or(0)),
                    area.y,
                    u16::try_from(indicator.len()).unwrap_or(0),
                    &indicator,
                    self.theme.style(Token::TextMuted),
                );
            }
            SettingKind::Action { label } => {
                put(
                    buf,
                    control.x,
                    area.y,
                    control.width,
                    &format!("[ {label} ]"),
                    self.theme.filled(Token::Accent),
                );
            }
            SettingKind::Info { value } => {
                put(
                    buf,
                    control.x,
                    area.y,
                    control.width,
                    value,
                    self.theme.style(Token::TextMuted),
                );
            }
        }
        let chip_variant = match self.provenance {
            Provenance::Default | Provenance::Runtime | Provenance::System => ChipVariant::Muted,
            Provenance::User | Provenance::Profile => ChipVariant::Accent,
            Provenance::Workspace => ChipVariant::Success,
            Provenance::Env | Provenance::Policy => ChipVariant::Warning,
        };
        Chip::new(self.theme, chip_label, chip_variant).render(
            buf,
            Rect::new(area.right().saturating_sub(chip_w), area.y, chip_w, 1),
        );
        if self.focused
            && let Some(description) = self.description
            && area.height > 1
        {
            put(
                buf,
                control_x,
                area.y.saturating_add(1),
                area.right().saturating_sub(control_x),
                description,
                self.theme.style(Token::TextMuted),
            );
        }
        control
    }
}
