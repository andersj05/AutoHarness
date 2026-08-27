//! Named breakpoints, spacing, and shell geometry.

#![allow(dead_code)]

/// Width band for a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum HeightBand {
    /// Under 20 rows.
    Short,
    /// 20 to 35 rows.
    Medium,
    /// 36 rows and above.
    Tall,
}

/// One cell of chrome: a rule, a border, or a single-row bar.
pub const ROW: u16 = 1;
/// Two-row page headers and compact notices.
pub const TWO_ROWS: u16 = 2;
/// Minimum height that can show a two-line page header.
pub const PAGE_HEADER_TALL_MIN: u16 = 8;
/// Minimum height that can show a page help row.
pub const PAGE_HELP_MIN: u16 = 3;
/// Comfortable height for profile-center help.
pub const PAGE_HELP_COMFORTABLE: u16 = 4;

/// Minimum width of the wide navigation rail shell.
pub const WIDE_SHELL_MIN_WIDTH: u16 = 100;
/// Minimum height of the wide navigation rail shell.
pub const WIDE_SHELL_MIN_HEIGHT: u16 = 16;
/// Current sidebar column count until Chat rebuilds the rail.
pub const SIDEBAR_WIDTH: u16 = 28;
/// Compact Chat falls back below this width.
pub const COMPACT_CHAT_MIN_WIDTH: u16 = 24;
/// Compact Chat falls back below this height.
pub const COMPACT_CHAT_MIN_HEIGHT: u16 = 7;
/// Sidebar width at the large breakpoint.
pub const SIDEBAR_WIDTH_LG: u16 = 26;
/// Sidebar width at the extra-large breakpoint.
pub const SIDEBAR_WIDTH_XL: u16 = 32;
/// Rows reserved above and below the sidebar session list.
pub const SIDEBAR_SESSION_CHROME: u16 = 5;
/// Horizontal inset around a sidebar session label.
pub const SIDEBAR_LABEL_INSET: u16 = 4;
/// Page gutter at medium width and above.
pub const GUTTER_MD: u16 = 2;
/// Page gutter below medium width.
pub const GUTTER_SM: u16 = 1;
/// Horizontal panel padding.
pub const PANEL_PAD_X: u16 = 1;
/// Vertical panel padding when the panel has no title.
pub const PANEL_PAD_Y: u16 = 0;
/// Vertical panel padding when the panel has a title.
pub const PANEL_PAD_Y_TITLED: u16 = 1;
/// Host width below which a modal fills the frame.
pub const MODAL_FULL_WIDTH: u16 = 48;
/// Host height below which a modal fills the frame.
pub const MODAL_FULL_HEIGHT: u16 = 12;
/// Outer horizontal inset when a modal is centered.
pub const MODAL_MARGIN_X: u16 = 2;
/// Outer vertical inset when a modal is centered.
pub const MODAL_MARGIN_Y: u16 = 1;
/// Maximum modal width at medium frames and above.
pub const MODAL_MAX_WIDTH: u16 = 72;
/// Maximum modal height at medium frames and above.
pub const MODAL_MAX_HEIGHT: u16 = 24;
/// Allowed spacing scale.
pub const SPACING: [u16; 4] = [0, 1, 2, 4];

/// Startup indicator minimum width.
pub const STARTUP_MIN_WIDTH: u16 = 28;
/// Startup indicator maximum width.
pub const STARTUP_MAX_WIDTH: u16 = 48;
/// Startup indicator minimum height.
pub const STARTUP_MIN_HEIGHT: u16 = 4;
/// Startup indicator maximum height.
pub const STARTUP_MAX_HEIGHT: u16 = 5;

/// Confirmation dialog fills the host at or below this width.
pub const CONFIRMATION_FULL_WIDTH: u16 = 40;
/// Confirmation dialog fills the host at or below this height.
pub const CONFIRMATION_FULL_HEIGHT: u16 = 12;
/// Confirmation dialog maximum height.
pub const CONFIRMATION_MAX_HEIGHT: u16 = 9;
/// Confirmation horizontal margin when centered.
pub const CONFIRMATION_MARGIN_X: u16 = 4;
/// Confirmation vertical margin when centered.
pub const CONFIRMATION_MARGIN_Y: u16 = 2;

/// Centered picker and palette fill the host below this width.
pub const POPUP_MIN_WIDTH: u16 = 30;
/// Centered picker and palette fill the host below this height.
pub const POPUP_MIN_HEIGHT: u16 = 10;
/// Popup width numerator against host width.
pub const POPUP_WIDTH_NUMERATOR: u16 = 4;
/// Popup width denominator against host width.
pub const POPUP_WIDTH_DENOMINATOR: u16 = 5;
/// Popup height numerator against host height.
pub const POPUP_HEIGHT_NUMERATOR: u16 = 3;
/// Popup height denominator against host height.
pub const POPUP_HEIGHT_DENOMINATOR: u16 = 4;

/// Codex sign-in dialog maximum height.
pub const CODEX_AUTH_MAX_HEIGHT: u16 = 14;
/// Row offset of the Codex sign-in action inside its dialog.
pub const CODEX_ACTION_ROW_OFFSET: u16 = 3;

/// Credential and permission dialog maximum width.
pub const CREDENTIAL_MAX_WIDTH: u16 = 68;
/// Credential and permission dialog maximum height.
pub const CREDENTIAL_MAX_HEIGHT: u16 = 11;

/// User-profile dialog fills the host at or below this width.
pub const USER_PROFILE_FULL_WIDTH: u16 = 44;
/// User-profile dialog fills the host at or below this height.
pub const USER_PROFILE_FULL_HEIGHT: u16 = 14;
/// User-profile dialog maximum height.
pub const USER_PROFILE_MAX_HEIGHT: u16 = 16;
/// User-profile extra vertical margin when centered.
pub const USER_PROFILE_MARGIN_Y: u16 = 4;
/// Inner line index of the user-profile Save/Cancel row.
pub const USER_PROFILE_BUTTON_LINE: u16 = 9;

/// Two-pane provider list requires at least this width.
pub const PROFILE_TWO_PANE_MIN_WIDTH: u16 = 60;
/// Profile-center copy and layout compact below this width.
pub const PROFILE_COMPACT_WIDTH: u16 = 72;
/// Profile-center help uses the shortest string below this width.
pub const PROFILE_HELP_NARROW: u16 = 48;
/// Profile-center help uses the medium string below this width.
pub const PROFILE_HELP_MEDIUM: u16 = 72;
/// Profile-center help uses the wide string below this width.
pub const PROFILE_HELP_WIDE: u16 = 96;
/// Horizontal split for the provider catalog pane.
pub const PROFILE_LIST_PERCENT: u16 = 52;
/// Horizontal split for the connected-profile pane.
pub const PROFILE_DETAIL_PERCENT: u16 = 48;
/// Vertical split for the provider catalog pane in one column.
pub const PROFILE_LIST_PERCENT_STACKED: u16 = 62;
/// Vertical split for the connected-profile pane in one column.
pub const PROFILE_DETAIL_PERCENT_STACKED: u16 = 38;
/// Detail-pane chrome rows above the profile action buttons.
pub const PROFILE_DETAIL_CHROME_ROWS: u16 = 9;

/// Session action bar uses the long hint at or above this width.
pub const SESSION_HELP_WIDE: u16 = 50;
/// Session action hits land this many rows above the frame bottom.
pub const SESSION_ACTION_FROM_BOTTOM: u16 = 2;

/// Inline command palette shows at most this many rows.
pub const INLINE_PALETTE_MAX_ROWS: u16 = 8;
/// Inline palette left inset.
pub const INLINE_PALETTE_INSET_X: u16 = 2;
/// Inline palette combined left and right inset.
pub const INLINE_PALETTE_INSET_X_TOTAL: u16 = 4;
/// Centered overlay list leaves this many chrome rows above the items.
pub const OVERLAY_LIST_TOP_CHROME: u16 = 2;

/// Composer metadata and editor inset below this content width.
pub const PROMPT_INSET_MIN_WIDTH: u16 = 4;
/// Minimum composer surface height.
pub const COMPOSER_MIN_HEIGHT: u16 = 3;
/// Maximum composer surface height.
pub const COMPOSER_MAX_HEIGHT: u16 = 5;
/// Maximum composer surface height in compact density.
pub const COMPOSER_MAX_HEIGHT_COMPACT: u16 = 4;
/// Prompt caret width.
pub const COMPOSER_CARET_WIDTH: u16 = 2;

/// Status line treats values as compact below this width.
pub const STATUS_COMPACT_WIDTH: u16 = 34;
/// Status line uses the wide model column at or above this width.
pub const STATUS_MODEL_WIDE_MIN: u16 = 64;
/// Narrow model column character budget.
pub const STATUS_MODEL_CHARS_NARROW: u16 = 8;
/// Mid model column character budget.
pub const STATUS_MODEL_CHARS_MID: u16 = 14;
/// Wide model column character budget.
pub const STATUS_MODEL_CHARS_WIDE: u16 = 22;
/// Status line keeps thinking at or above this width.
pub const STATUS_THINKING_MIN: u16 = 20;
/// Status line keeps context at or above this width.
pub const STATUS_CONTEXT_MIN: u16 = 32;
/// Context metric uses the compact form below this width.
pub const STATUS_CONTEXT_COMPACT: u16 = 42;
/// Status line keeps workspace at or above this width.
pub const STATUS_WORKSPACE_MIN: u16 = 56;
/// Workspace path uses the wide budget at or above this width.
pub const STATUS_WORKSPACE_WIDE: u16 = 84;
/// Wide workspace path character budget.
pub const STATUS_PATH_CHARS_WIDE: u16 = 28;
/// Narrow workspace path character budget.
pub const STATUS_PATH_CHARS_NARROW: u16 = 18;
/// Status line keeps the git branch at or above this width.
pub const STATUS_BRANCH_MIN: u16 = 76;
/// Git branch character budget.
pub const STATUS_BRANCH_CHARS: u16 = 18;
/// Status line keeps token totals at or above this width.
pub const STATUS_TOKENS_MIN: u16 = 98;
/// Credential overlay copy compact below this width.
pub const CREDENTIAL_COMPACT_WIDTH: u16 = 36;

/// Settings navigation uses unpadded labels below this width.
pub const SETTINGS_NAV_COMPACT_WIDTH: u16 = 48;
/// Settings body inset from the page frame.
pub const SETTINGS_BODY_INSET_X: u16 = 1;
/// Settings body drops the nav row and page border.
pub const SETTINGS_BODY_INSET_Y: u16 = 2;
/// Settings body drops left, right, and bottom chrome.
pub const SETTINGS_BODY_INSET_Y_TOTAL: u16 = 3;
/// Settings body drops left and right chrome.
pub const SETTINGS_BODY_INSET_X_TOTAL: u16 = 2;

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

/// Returns the page gutter for a width.
#[must_use]
pub const fn gutter(width: u16) -> u16 {
    match width_band(width) {
        WidthBand::Xs | WidthBand::Sm => GUTTER_SM,
        WidthBand::Md | WidthBand::Lg | WidthBand::Xl => GUTTER_MD,
    }
}

/// Returns the sidebar width for a later Chat rail rebuild.
#[must_use]
pub const fn sidebar_width_for(width: u16) -> u16 {
    match width_band(width) {
        WidthBand::Xl => SIDEBAR_WIDTH_XL,
        _ => SIDEBAR_WIDTH_LG,
    }
}

/// Centers a child of `width` by `height` inside `host`.
#[must_use]
pub fn centered(host: RectLike, width: u16, height: u16) -> RectLike {
    RectLike {
        x: host.x + host.width.saturating_sub(width) / 2,
        y: host.y + host.height.saturating_sub(height) / 2,
        width: width.max(1),
        height: height.max(1),
    }
}

/// Integer rectangle used by layout helpers that must not depend on Ratatui.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectLike {
    /// Left column.
    pub x: u16,
    /// Top row.
    pub y: u16,
    /// Width in cells.
    pub width: u16,
    /// Height in cells.
    pub height: u16,
}

#[cfg(test)]
mod tests {
    use super::{
        GUTTER_MD, GUTTER_SM, HeightBand, PANEL_PAD_X, PANEL_PAD_Y, SIDEBAR_WIDTH_LG,
        SIDEBAR_WIDTH_XL, SPACING, WIDE_SHELL_MIN_HEIGHT, WIDE_SHELL_MIN_WIDTH, WidthBand,
        height_band, sidebar_width_for, wide_shell, width_band,
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
        assert_eq!(sidebar_width_for(120), SIDEBAR_WIDTH_LG);
        assert_eq!(sidebar_width_for(140), SIDEBAR_WIDTH_XL);
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

    #[test]
    fn threshold_literals_only_live_in_metrics() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations = Vec::new();
        visit_rust_files(&root, &root, &mut violations);
        assert!(
            violations.is_empty(),
            "width and height thresholds must live in ui/metrics.rs:\n{}",
            violations.join("\n")
        );
    }

    fn visit_rust_files(
        root: &std::path::Path,
        dir: &std::path::Path,
        violations: &mut Vec<String>,
    ) {
        for entry in std::fs::read_dir(dir).expect("read tui sources") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                visit_rust_files(root, &path, violations);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("src-relative path");
            if relative == std::path::Path::new("ui/metrics.rs")
                || relative.file_name().is_some_and(|name| name == "tests.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read rust source");
            let mut in_test = false;
            for (index, line) in source.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("#[cfg(test)]") {
                    in_test = true;
                }
                if in_test && trimmed == "}" && !line.starts_with(' ') && !line.starts_with('\t') {
                    in_test = false;
                }
                if in_test || trimmed.starts_with("//") {
                    continue;
                }
                if threshold_literal(line) {
                    violations.push(format!("{}:{}:{line}", relative.display(), index + 1));
                }
            }
        }
    }

    fn threshold_literal(line: &str) -> bool {
        let trimmed = line.trim_start();
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("pub const fn ")
        {
            return false;
        }
        if line.contains("u16::MAX") || line.contains(".width()") {
            return false;
        }
        comparison_threshold(line) || constraint_length_threshold(line) || clamp_threshold(line)
    }

    fn comparison_threshold(line: &str) -> bool {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if matches!(bytes[i], b'>' | b'<') {
                let mut j = i + 1;
                if bytes[j] == b'=' {
                    j += 1;
                }
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j].is_ascii_digit() {
                    let start = j;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if let Ok(value) = line[start..j].parse::<u32>()
                        && value > 4
                        && mentions_extent(line)
                    {
                        return true;
                    }
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        false
    }

    fn constraint_length_threshold(line: &str) -> bool {
        let Some(start) = line.find("Constraint::Length(") else {
            return false;
        };
        let rest = &line[start + "Constraint::Length(".len()..];
        numeric_over_spacing(rest)
    }

    fn clamp_threshold(line: &str) -> bool {
        for key in [".clamp(", ".min(", ".max("] {
            if let Some(start) = line.find(key) {
                let rest = &line[start + key.len()..];
                if numeric_over_spacing(rest) && mentions_extent(line) {
                    return true;
                }
            }
        }
        false
    }

    fn numeric_over_spacing(rest: &str) -> bool {
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        digits.parse::<u32>().is_ok_and(|value| value > 4)
    }

    fn mentions_extent(line: &str) -> bool {
        has_word(line, "width")
            || has_word(line, "height")
            || has_word(line, "column")
            || has_word(line, "row")
            || line.contains("Constraint::Length")
            || line.contains("clamp(")
    }

    fn has_word(line: &str, word: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        let mut rest = lower.as_str();
        while let Some(at) = rest.find(word) {
            let before = if at == 0 {
                true
            } else {
                let ch = rest[..at].chars().last().unwrap_or(' ');
                !ch.is_ascii_alphanumeric() && ch != '_'
            };
            let after_index = at + word.len();
            let after = rest[after_index..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
            if before && after {
                return true;
            }
            rest = &rest[at + 1..];
        }
        false
    }
}
