//! Immutable per-frame theme resolved from preferences and terminal capability.

use autoharness_settings::{ColorMode, EffectiveLocalPreferences, ThemePreset};
use ratatui::style::{Modifier, Style};

use super::color::{ColorDepth, Rgb, clamp_contrast, quantize, reset_color};
use super::gradient::{Gradient, gradient_style, normalized_t};
use super::palette::{Ramp, Seed};
use super::tokens::{BackgroundIntent, TEXT_ON_ACCENT_FLOOR, Token, TokenPaint, paints};

/// Resolved appearance for one frame.
#[derive(Clone, Debug)]
pub struct Theme {
    styles: [Style; Token::ALL.len()],
    paints: [TokenPaint; Token::ALL.len()],
    #[allow(dead_code)]
    ramp: Ramp,
    depth: ColorDepth,
    preset: ThemePreset,
    mode: ColorMode,
    gradient: Gradient,
}

impl Theme {
    /// Resolves tokens from preferences and detected color depth.
    #[must_use]
    pub fn resolve(preferences: &EffectiveLocalPreferences, depth: ColorDepth) -> Self {
        Self::from_preset(
            *preferences.theme_preset().value(),
            *preferences.color_mode().value(),
            depth,
        )
    }

    /// Resolves a theme from explicit preset, mode, and depth.
    #[must_use]
    pub fn from_preset(preset: ThemePreset, mode: ColorMode, depth: ColorDepth) -> Self {
        let seed = Seed::for_preset(preset);
        let (paints, ramp) = paints(Ramp::derive(seed), mode);
        let styles = core::array::from_fn(|index| emit(paints[index], depth, mode));
        Self {
            styles,
            paints,
            ramp,
            depth,
            preset,
            mode,
            gradient: Gradient::from_accents(ramp.accent, ramp.accent_alt),
        }
    }

    /// Returns the style for a semantic token.
    #[must_use]
    pub fn style(&self, token: Token) -> Style {
        self.styles[token.index()]
    }

    /// Returns the token style with a transparent background.
    #[must_use]
    pub fn style_transparent(&self, token: Token) -> Style {
        self.style(token).bg(reset_color())
    }

    /// Inverts a color token as a filled chip: token color as background, on-accent ink.
    #[must_use]
    pub fn filled(&self, token: Token) -> Style {
        if self.mode == ColorMode::NoColor {
            return Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        let fill = self.paints[token.index()].foreground;
        let ink = contrast_ink(fill);
        let fg = quantize(ink, self.depth);
        let bg = quantize(fill, self.depth);
        let mut style = Style::new()
            .fg(fg.color)
            .bg(bg.color)
            .add_modifier(Modifier::BOLD);
        if fg.bold || bg.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        style
    }

    /// Returns the three-stop accent gradient for this theme.
    #[must_use]
    pub const fn gradient(&self) -> Gradient {
        self.gradient
    }

    /// Samples the theme gradient at a normalized position in `0.0..=1.0`.
    #[must_use]
    pub fn gradient_style(&self, t: f32) -> Style {
        gradient_style(self, t)
    }

    /// Samples a discrete cell using a normalized position derived from its index.
    #[must_use]
    pub fn gradient_cell(&self, index: u16, count: u16) -> Style {
        self.gradient_style(normalized_t(index, count))
    }

    /// Samples the theme gradient and emphasizes the cell for meter fills.
    #[must_use]
    pub fn gradient_emphasis_style(&self, t: f32) -> Style {
        self.gradient_style(t).add_modifier(Modifier::BOLD)
    }

    /// Detected color depth used during resolution.
    #[must_use]
    pub const fn depth(&self) -> ColorDepth {
        self.depth
    }

    /// Preset used during resolution.
    #[must_use]
    pub const fn preset(&self) -> ThemePreset {
        self.preset
    }

    /// Color mode used during resolution.
    #[must_use]
    pub const fn mode(&self) -> ColorMode {
        self.mode
    }

    /// Surface-base background color after quantization.
    #[must_use]
    pub fn surface_base_color(&self) -> ratatui::style::Color {
        self.style(Token::SurfaceBase)
            .bg
            .unwrap_or_else(reset_color)
    }
}

fn emit(paint: TokenPaint, depth: ColorDepth, mode: ColorMode) -> Style {
    if mode == ColorMode::NoColor {
        return Style::default().add_modifier(paint.modifiers);
    }
    let foreground = quantize(paint.foreground, depth);
    let mut style = Style::new().fg(foreground.color);
    if foreground.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    match paint.intent {
        BackgroundIntent::Transparent => {
            style = style.bg(reset_color());
        }
        BackgroundIntent::Inherit => {}
        BackgroundIntent::Surface(_) => {
            if let Some(background) = paint.background {
                let background = quantize(background, depth);
                style = style.bg(background.color);
                if background.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
            }
        }
    }
    style.add_modifier(paint.modifiers)
}

fn contrast_ink(background: Rgb) -> Rgb {
    clamp_contrast(
        if background.relative_luminance() > 0.32 {
            Rgb::from_srgb8(0x0B, 0x0F, 0x19)
        } else {
            Rgb::from_srgb8(0xF8, 0xFA, 0xFC)
        },
        background,
        TEXT_ON_ACCENT_FLOOR,
    )
}

#[cfg(test)]
mod tests {
    use autoharness_settings::{ColorMode, ThemePreset};

    use super::{Theme, Token};
    use crate::ui::color::{ColorDepth, contrast_ratio};
    use crate::ui::tokens::{
        BORDER_FOCUS_FLOOR, SEMANTIC_SOFT_FLOOR, TEXT_MUTED_FLOOR, TEXT_ON_ACCENT_FLOOR,
        TEXT_PRIMARY_FLOOR, TEXT_SECONDARY_FLOOR,
    };

    const PRESETS: [ThemePreset; 9] = [
        ThemePreset::System,
        ThemePreset::Dark,
        ThemePreset::Light,
        ThemePreset::Aurora,
        ThemePreset::Ember,
        ThemePreset::Midnight,
        ThemePreset::Ocean,
        ThemePreset::Forest,
        ThemePreset::Rose,
    ];
    const MODES: [ColorMode; 5] = [
        ColorMode::Color,
        ColorMode::Soft,
        ColorMode::Vivid,
        ColorMode::NoColor,
        ColorMode::HighContrast,
    ];

    #[test]
    fn contrast_matrix_meets_documented_floors() {
        for preset in PRESETS {
            for mode in MODES {
                if mode == ColorMode::NoColor {
                    continue;
                }
                let theme = Theme::from_preset(preset, mode, ColorDepth::TrueColor);
                let ramp = theme.ramp;
                assert!(
                    contrast_ratio(ramp.text_primary, ramp.surface_base) + 0.001
                        >= TEXT_PRIMARY_FLOOR,
                    "{preset:?} {mode:?} text_primary"
                );
                assert!(
                    contrast_ratio(ramp.text_secondary, ramp.surface_base) + 0.001
                        >= TEXT_SECONDARY_FLOOR,
                    "{preset:?} {mode:?} text_secondary"
                );
                assert!(
                    contrast_ratio(ramp.text_muted, ramp.surface_base) + 0.001 >= TEXT_MUTED_FLOOR,
                    "{preset:?} {mode:?} text_muted"
                );
                assert!(
                    contrast_ratio(ramp.text_on_accent, ramp.surface_selected) + 0.001
                        >= TEXT_ON_ACCENT_FLOOR,
                    "{preset:?} {mode:?} text_on_accent"
                );
                for (name, fg, bg) in [
                    ("success", ramp.success, ramp.success_soft),
                    ("warning", ramp.warning, ramp.warning_soft),
                    ("danger", ramp.danger, ramp.danger_soft),
                    ("info", ramp.info, ramp.info_soft),
                ] {
                    assert!(
                        contrast_ratio(fg, bg) + 0.001 >= SEMANTIC_SOFT_FLOOR,
                        "{preset:?} {mode:?} {name}"
                    );
                }
                assert!(
                    contrast_ratio(ramp.border_focus, ramp.surface_base) + 0.001
                        >= BORDER_FOCUS_FLOOR,
                    "{preset:?} {mode:?} border_focus"
                );
            }
        }
    }

    #[test]
    fn system_and_dark_emit_different_surface_backgrounds() {
        let system =
            Theme::from_preset(ThemePreset::System, ColorMode::Color, ColorDepth::TrueColor);
        let dark = Theme::from_preset(ThemePreset::Dark, ColorMode::Color, ColorDepth::TrueColor);
        assert_ne!(system.surface_base_color(), dark.surface_base_color());
        assert_ne!(
            system.style(Token::SurfaceBase).bg,
            dark.style(Token::SurfaceBase).bg
        );
    }

    #[test]
    fn color_literals_only_live_in_palette_and_color_modules() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let allowed = [
            std::path::Path::new("ui/color.rs"),
            std::path::Path::new("ui/palette.rs"),
        ];
        let mut violations = Vec::new();
        visit_rust_files(&root, &root, &allowed, &mut violations);
        assert!(
            violations.is_empty(),
            "color literals must live in ui/color.rs and ui/palette.rs:\n{}",
            violations.join("\n")
        );
    }

    fn visit_rust_files(
        root: &std::path::Path,
        dir: &std::path::Path,
        allowed: &[&std::path::Path],
        violations: &mut Vec<String>,
    ) {
        for entry in std::fs::read_dir(dir).expect("read ui sources") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                visit_rust_files(root, &path, allowed, violations);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("src-relative path");
            if allowed.contains(&relative) {
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
                if in_test {
                    continue;
                }
                if line.contains("Color::Rgb(")
                    || line.contains("Color::Indexed(")
                    || named_color_literal(line)
                {
                    violations.push(format!("{}:{}:{line}", relative.display(), index + 1));
                }
            }
        }
    }

    fn named_color_literal(line: &str) -> bool {
        const NAMES: [&str; 17] = [
            "Color::Reset",
            "Color::Black",
            "Color::Red",
            "Color::Green",
            "Color::Yellow",
            "Color::Blue",
            "Color::Magenta",
            "Color::Cyan",
            "Color::Gray",
            "Color::DarkGray",
            "Color::LightRed",
            "Color::LightGreen",
            "Color::LightYellow",
            "Color::LightBlue",
            "Color::LightMagenta",
            "Color::LightCyan",
            "Color::White",
        ];
        NAMES.iter().any(|name| line.contains(name))
    }
}
