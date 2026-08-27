//! Named breakpoints, spacing, and shell geometry.

#![allow(dead_code)]

/// Width band for a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidthBand {
    /// Under 48 columns.
    Xs,
    /// 48 to 71 columns.
    Sm,
    /// 72 to 99 columns.
    Md,
    /// 100 to 139 columns.
    Lg,
    /// 140 columns and above.
    Xl,
}

/// Height band for a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeightBand {
    /// Under 20 rows.
    Short,
    /// 20 to 35 rows.
    Medium,
    /// 36 rows and above.
    Tall,
}

/// Minimum width of the wide navigation rail shell.
pub const WIDE_SHELL_MIN_WIDTH: u16 = 100;
/// Minimum height of the wide navigation rail shell.
pub const WIDE_SHELL_MIN_HEIGHT: u16 = 16;
/// Current sidebar column count until the layout contract step.
pub const SIDEBAR_WIDTH: u16 = 28;
/// Compact Chat falls back below this width.
pub const COMPACT_CHAT_MIN_WIDTH: u16 = 24;
/// Compact Chat falls back below this height.
pub const COMPACT_CHAT_MIN_HEIGHT: u16 = 7;
/// Sidebar width at the large breakpoint, for later layout work.
pub const SIDEBAR_WIDTH_LG: u16 = 26;
/// Sidebar width at the extra-large breakpoint, for later layout work.
pub const SIDEBAR_WIDTH_XL: u16 = 32;
/// Page gutter at medium width and above.
pub const GUTTER_MD: u16 = 2;
/// Page gutter below medium width.
pub const GUTTER_SM: u16 = 1;
/// Horizontal panel padding.
pub const PANEL_PAD_X: u16 = 1;
/// Vertical panel padding when the panel has no title.
pub const PANEL_PAD_Y: u16 = 0;
/// Allowed spacing scale.
pub const SPACING: [u16; 4] = [0, 1, 2, 4];

/// Returns the width band for a column count.
#[must_use]
pub const fn width_band(width: u16) -> WidthBand {
    match width {
        0..=47 => WidthBand::Xs,
        48..=71 => WidthBand::Sm,
        72..=99 => WidthBand::Md,
        100..=139 => WidthBand::Lg,
        _ => WidthBand::Xl,
    }
}

/// Returns the height band for a row count.
#[must_use]
pub const fn height_band(height: u16) -> HeightBand {
    match height {
        0..=19 => HeightBand::Short,
        20..=35 => HeightBand::Medium,
        _ => HeightBand::Tall,
    }
}

/// Returns whether the shell should render the wide navigation rail.
#[must_use]
pub const fn wide_shell(width: u16, height: u16, single_column: bool) -> bool {
    !single_column && width >= WIDE_SHELL_MIN_WIDTH && height >= WIDE_SHELL_MIN_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::{
        GUTTER_MD, GUTTER_SM, HeightBand, PANEL_PAD_X, PANEL_PAD_Y, SIDEBAR_WIDTH_LG,
        SIDEBAR_WIDTH_XL, SPACING, WIDE_SHELL_MIN_HEIGHT, WIDE_SHELL_MIN_WIDTH, WidthBand,
        height_band, wide_shell, width_band,
    };

    #[test]
    fn width_bands_match_the_design_system() {
        assert_eq!(width_band(47), WidthBand::Xs);
        assert_eq!(width_band(48), WidthBand::Sm);
        assert_eq!(width_band(72), WidthBand::Md);
        assert_eq!(width_band(100), WidthBand::Lg);
        assert_eq!(width_band(140), WidthBand::Xl);
    }

    #[test]
    fn height_bands_match_the_design_system() {
        assert_eq!(height_band(19), HeightBand::Short);
        assert_eq!(height_band(20), HeightBand::Medium);
        assert_eq!(height_band(36), HeightBand::Tall);
    }

    #[test]
    fn spacing_and_sidebar_constants_are_named() {
        assert_eq!(SPACING, [0, 1, 2, 4]);
        assert_eq!(SIDEBAR_WIDTH_LG, 26);
        assert_eq!(SIDEBAR_WIDTH_XL, 32);
        assert_eq!(GUTTER_MD, 2);
        assert_eq!(GUTTER_SM, 1);
        assert_eq!(PANEL_PAD_X, 1);
        assert_eq!(PANEL_PAD_Y, 0);
    }

    #[test]
    fn wide_shell_uses_the_named_thresholds() {
        assert!(wide_shell(
            WIDE_SHELL_MIN_WIDTH,
            WIDE_SHELL_MIN_HEIGHT,
            false
        ));
        assert!(!wide_shell(99, 50, false));
        assert!(!wide_shell(120, 15, false));
        assert!(!wide_shell(120, 40, true));
    }
}
