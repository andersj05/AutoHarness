//! Three-stop theme gradients sampled in Oklab at a normalized position.

use autoharness_settings::ColorMode;
use ratatui::style::{Modifier, Style};

use super::color::{ColorDepth, Rgb, quantize, reset_color};
use super::theme::Theme;
use super::tokens::Token;

/// Three-stop gradient: accent start, lifted midpoint, accent end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    stops: [(f32, Rgb); 3],
}

impl Gradient {
    /// Builds the sanctioned three-stop accent gradient for a theme ramp.
    #[must_use]
    pub fn from_accents(accent_a: Rgb, accent_b: Rgb) -> Self {
        let midpoint = accent_a
            .mix(accent_b, 0.5)
            .to_oklab()
            .add_lightness(0.06)
            .to_rgb();
        Self {
            stops: [(0.0, accent_a), (0.5, midpoint), (1.0, accent_b)],
        }
    }

    /// Interpolates in Oklab between the bracketing stops.
    #[must_use]
    pub fn sample(&self, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        if t <= self.stops[0].0 {
            return self.stops[0].1;
        }
        if t >= self.stops[2].0 {
            return self.stops[2].1;
        }
        let (left, right) = if t <= self.stops[1].0 {
            (self.stops[0], self.stops[1])
        } else {
            (self.stops[1], self.stops[2])
        };
        let span = right.0 - left.0;
        let local = if span <= f32::EPSILON {
            0.0
        } else {
            (t - left.0) / span
        };
        left.1.mix(right.1, local)
    }

    /// Samples with Basic16 snapped to the three stops to avoid banding.
    #[must_use]
    pub fn sample_for_depth(&self, t: f32, depth: ColorDepth) -> Rgb {
        match depth {
            ColorDepth::Basic16 => self.sample(snap_basic16(t)),
            ColorDepth::TrueColor | ColorDepth::Indexed256 => self.sample(t),
        }
    }

    /// Returns the three stop colors in order.
    #[must_use]
    pub const fn stops(&self) -> [(f32, Rgb); 3] {
        self.stops
    }
}

/// Converts a discrete cell index into a normalized gradient position.
#[must_use]
pub fn normalized_t(index: u16, count: u16) -> f32 {
    let last = count.saturating_sub(1);
    if last == 0 {
        0.0
    } else {
        f32::from(index.min(last)) / f32::from(last)
    }
}

/// Theme-aware gradient cell style for a normalized position.
#[must_use]
pub fn gradient_style(theme: &Theme, t: f32) -> Style {
    match theme.mode() {
        ColorMode::NoColor => theme.style(Token::BorderSubtle),
        ColorMode::HighContrast => theme.style(Token::Accent),
        ColorMode::Color | ColorMode::Soft | ColorMode::Vivid => {
            let sample = theme.gradient().sample_for_depth(t, theme.depth());
            let fg = quantize(sample, theme.depth());
            let mut style = Style::new().fg(fg.color).bg(reset_color());
            if fg.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if theme.mode() == ColorMode::Vivid {
                style = style.add_modifier(Modifier::BOLD);
            }
            style
        }
    }
}

fn snap_basic16(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.25 {
        0.0
    } else if t < 0.75 {
        0.5
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use autoharness_settings::{ColorMode, ThemePreset};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::{Gradient, gradient_style, normalized_t};
    use crate::snapshot::style_snapshot;
    use crate::ui::color::ColorDepth;
    use crate::ui::palette::Seed;
    use crate::ui::theme::Theme;

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

    #[test]
    fn endpoints_match_the_seed_and_midpoint_is_lifted() {
        for preset in PRESETS {
            let seed = Seed::for_preset(preset);
            let gradient = Gradient::from_accents(seed.accent_a, seed.accent_b);
            assert_eq!(gradient.sample(0.0).to_srgb8(), seed.accent_a.to_srgb8());
            assert_eq!(gradient.sample(1.0).to_srgb8(), seed.accent_b.to_srgb8());
            let unlifted = seed.accent_a.mix(seed.accent_b, 0.5);
            let mid = gradient.sample(0.5);
            assert_ne!(mid.to_srgb8(), unlifted.to_srgb8(), "{preset:?}");
            assert!(
                mid.relative_luminance() >= unlifted.relative_luminance(),
                "{preset:?} midpoint should be lifted"
            );
        }
    }

    #[test]
    fn system_midpoint_is_pinned() {
        let seed = Seed::for_preset(ThemePreset::System);
        let mid = Gradient::from_accents(seed.accent_a, seed.accent_b)
            .sample(0.5)
            .to_srgb8();
        assert_eq!(mid, (147, 196, 255));
    }

    #[test]
    fn normalized_position_is_reproducible_across_widths() {
        assert!((normalized_t(0, 10) - 0.0).abs() < f32::EPSILON);
        assert!((normalized_t(9, 10) - 1.0).abs() < f32::EPSILON);
        assert!((normalized_t(4, 9) - normalized_t(8, 17)).abs() < 0.02);
        let seed = Seed::for_preset(ThemePreset::System);
        let gradient = Gradient::from_accents(seed.accent_a, seed.accent_b);
        assert_eq!(
            gradient.sample(normalized_t(0, 12)).to_srgb8(),
            gradient.sample(normalized_t(0, 40)).to_srgb8()
        );
        assert_eq!(
            gradient.sample(normalized_t(11, 12)).to_srgb8(),
            gradient.sample(normalized_t(39, 40)).to_srgb8()
        );
    }

    #[test]
    fn basic16_samples_only_the_three_stops() {
        let seed = Seed::for_preset(ThemePreset::System);
        let gradient = Gradient::from_accents(seed.accent_a, seed.accent_b);
        assert_eq!(
            gradient
                .sample_for_depth(0.1, ColorDepth::Basic16)
                .to_srgb8(),
            gradient.sample(0.0).to_srgb8()
        );
        assert_eq!(
            gradient
                .sample_for_depth(0.5, ColorDepth::Basic16)
                .to_srgb8(),
            gradient.sample(0.5).to_srgb8()
        );
        assert_eq!(
            gradient
                .sample_for_depth(0.9, ColorDepth::Basic16)
                .to_srgb8(),
            gradient.sample(1.0).to_srgb8()
        );
    }

    fn paint_rule(theme: &Theme, width: u16, glyph: char) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, 1));
        for x in 0..width {
            let cell = buffer
                .cell_mut((x, 0))
                .expect("gradient fixture stays inside the buffer");
            cell.set_char(glyph);
            cell.set_style(gradient_style(theme, normalized_t(x, width)));
        }
        buffer
    }

    #[test]
    fn nocolor_and_high_contrast_degrade_without_a_ramp() {
        let color =
            Theme::from_preset(ThemePreset::System, ColorMode::Color, ColorDepth::TrueColor);
        let no_color = Theme::from_preset(
            ThemePreset::System,
            ColorMode::NoColor,
            ColorDepth::TrueColor,
        );
        let high = Theme::from_preset(
            ThemePreset::System,
            ColorMode::HighContrast,
            ColorDepth::TrueColor,
        );
        let color_snap = style_snapshot(&paint_rule(&color, 12, '-'));
        let no_color_snap = style_snapshot(&paint_rule(&no_color, 12, '-'));
        let high_snap = style_snapshot(&paint_rule(&high, 12, '-'));
        assert_eq!(
            no_color_snap,
            concat!(
                "# autoharness-tui style snapshot v1\n",
                "# 12x1\n",
                "@0\n",
                "------------\n",
                " | 0-11 fg=reset bg=reset\n",
            )
        );
        assert_eq!(
            high_snap,
            concat!(
                "# autoharness-tui style snapshot v1\n",
                "# 12x1\n",
                "@0\n",
                "------------\n",
                " | 0-11 fg=#ffffff bg=reset bold\n",
            )
        );
        assert_ne!(color_snap, no_color_snap);
        assert_ne!(color_snap, high_snap);
        assert!(
            color_snap.contains("fg=#22d3ee"),
            "color mode should keep the start stop: {color_snap}"
        );
        assert!(
            color_snap.contains("fg=#a78bfa"),
            "color mode should keep the end stop: {color_snap}"
        );
    }
}
