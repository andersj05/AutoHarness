//! Theme seeds and the shared derivation that expands a seed into a color ramp.

use autoharness_settings::ThemePreset;

use super::color::{Rgb, clamp_contrast};

/// Four-value theme seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seed {
    /// Surface background anchor.
    pub base: Rgb,
    /// Primary accent and gradient start.
    pub accent_a: Rgb,
    /// Secondary accent and gradient end.
    pub accent_b: Rgb,
    /// When true, derived steps invert toward darker text.
    pub light: bool,
}

/// Fully derived linear colors shared by every preset.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pub surface_base: Rgb,
    pub surface_sunken: Rgb,
    pub surface_raised: Rgb,
    pub surface_overlay: Rgb,
    pub surface_scrim: Rgb,
    pub surface_selected: Rgb,
    pub surface_selected_muted: Rgb,
    pub surface_danger: Rgb,
    pub surface_warning: Rgb,
    pub surface_success: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_muted: Rgb,
    pub text_disabled: Rgb,
    pub text_on_accent: Rgb,
    pub text_on_danger: Rgb,
    pub text_link: Rgb,
    pub accent: Rgb,
    pub accent_alt: Rgb,
    pub accent_soft: Rgb,
    pub accent_on_surface: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub danger: Rgb,
    pub info: Rgb,
    pub success_soft: Rgb,
    pub warning_soft: Rgb,
    pub danger_soft: Rgb,
    pub info_soft: Rgb,
    pub border_subtle: Rgb,
    pub border_strong: Rgb,
    pub border_focus: Rgb,
    pub divider: Rgb,
    pub scrollbar_track: Rgb,
    pub scrollbar_thumb: Rgb,
    pub focus_ring: Rgb,
    pub role_user: Rgb,
    pub role_assistant: Rgb,
    pub role_tool: Rgb,
    pub role_system: Rgb,
}

impl Seed {
    /// Returns the seed for a named preset.
    #[must_use]
    pub fn for_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::System => Self {
                base: Rgb::from_srgb8(0x08, 0x0C, 0x18),
                accent_a: Rgb::from_srgb8(0x22, 0xD3, 0xEE),
                accent_b: Rgb::from_srgb8(0xA7, 0x8B, 0xFA),
                light: false,
            },
            ThemePreset::Dark => Self {
                base: Rgb::from_srgb8(0x05, 0x07, 0x0E),
                accent_a: Rgb::from_srgb8(0x22, 0xD3, 0xEE),
                accent_b: Rgb::from_srgb8(0xA7, 0x8B, 0xFA),
                light: false,
            },
            ThemePreset::Light => Self {
                base: Rgb::from_srgb8(0xFA, 0xFA, 0xFB),
                accent_a: Rgb::from_srgb8(0x25, 0x63, 0xEB),
                accent_b: Rgb::from_srgb8(0xDB, 0x27, 0x77),
                light: true,
            },
            ThemePreset::Aurora => Self {
                base: Rgb::from_srgb8(0x04, 0x0F, 0x1E),
                accent_a: Rgb::from_srgb8(0x2D, 0xD4, 0xBF),
                accent_b: Rgb::from_srgb8(0x81, 0x8C, 0xF8),
                light: false,
            },
            ThemePreset::Ember => Self {
                base: Rgb::from_srgb8(0x1A, 0x0A, 0x0A),
                accent_a: Rgb::from_srgb8(0xFB, 0x92, 0x3C),
                accent_b: Rgb::from_srgb8(0xF4, 0x3F, 0x5E),
                light: false,
            },
            ThemePreset::Midnight => Self {
                base: Rgb::from_srgb8(0x03, 0x07, 0x12),
                accent_a: Rgb::from_srgb8(0x60, 0xA5, 0xFA),
                accent_b: Rgb::from_srgb8(0x63, 0x66, 0xF1),
                light: false,
            },
            ThemePreset::Ocean => Self {
                base: Rgb::from_srgb8(0x02, 0x14, 0x20),
                accent_a: Rgb::from_srgb8(0x22, 0xD3, 0xEE),
                accent_b: Rgb::from_srgb8(0x0E, 0xA5, 0xE9),
                light: false,
            },
            ThemePreset::Forest => Self {
                base: Rgb::from_srgb8(0x07, 0x14, 0x0D),
                accent_a: Rgb::from_srgb8(0x4A, 0xDE, 0x80),
                accent_b: Rgb::from_srgb8(0xFA, 0xCC, 0x15),
                light: false,
            },
            ThemePreset::Rose => Self {
                base: Rgb::from_srgb8(0x1D, 0x08, 0x14),
                accent_a: Rgb::from_srgb8(0xF4, 0x72, 0xB6),
                accent_b: Rgb::from_srgb8(0xC0, 0x84, 0xFC),
                light: false,
            },
        }
    }
}

impl Ramp {
    /// Derives every token color from a seed.
    #[must_use]
    pub fn derive(seed: Seed) -> Self {
        let surface_base = seed.base;
        let surface_sunken = shift_from_text(surface_base, seed.light, 0.04);
        let surface_raised = shift_toward_text(surface_base, seed.light, 0.06);
        let surface_overlay = shift_toward_text(surface_base, seed.light, 0.10);
        let surface_scrim = surface_base.mix(
            Rgb::from_srgb8(0, 0, 0),
            if seed.light { 0.12 } else { 0.45 },
        );
        let text_anchor = if seed.light {
            Rgb::from_srgb8(0x0B, 0x0F, 0x19)
        } else {
            Rgb::from_srgb8(0xF8, 0xFA, 0xFC)
        };
        let text_primary = clamp_contrast(text_anchor.mix(seed.accent_a, 0.04), surface_base, 7.0);
        let text_secondary =
            clamp_contrast(text_primary.mix(surface_base, 0.30), surface_base, 4.5);
        let text_muted = clamp_contrast(text_primary.mix(surface_base, 0.55), surface_base, 3.5);
        let text_disabled = text_muted.mix(surface_base, 0.25);
        let accent = seed.accent_a;
        let accent_alt = seed.accent_b;
        let accent_soft = accent.mix(surface_base, 0.70);
        let accent_on_surface = clamp_contrast(accent, surface_base, 4.5);
        let (success, warning, danger, info) = if seed.light {
            (
                Rgb::from_srgb8(0x15, 0x80, 0x3D),
                Rgb::from_srgb8(0xA1, 0x62, 0x07),
                Rgb::from_srgb8(0xBE, 0x12, 0x3C),
                Rgb::from_srgb8(0x1D, 0x4E, 0xD8),
            )
        } else {
            (
                Rgb::from_srgb8(0x4A, 0xDE, 0x80),
                Rgb::from_srgb8(0xFB, 0xBF, 0x24),
                Rgb::from_srgb8(0xFB, 0x71, 0x85),
                Rgb::from_srgb8(0x60, 0xA5, 0xFA),
            )
        };
        let success_soft = success.mix(surface_base, 0.70);
        let warning_soft = warning.mix(surface_base, 0.70);
        let danger_soft = danger.mix(surface_base, 0.70);
        let info_soft = info.mix(surface_base, 0.70);
        let surface_selected = accent_alt.mix(surface_base, 0.15);
        let surface_selected_muted = accent.mix(surface_base, 0.55);
        let surface_danger = danger_soft;
        let surface_warning = warning_soft;
        let surface_success = success_soft;
        let text_on_accent = clamp_contrast(contrast_ink(accent), accent, 4.5);
        let text_on_danger = clamp_contrast(contrast_ink(danger), danger, 4.5);
        let border_subtle = text_muted.mix(surface_base, 0.55);
        let border_strong = text_secondary.mix(surface_base, 0.25);
        let border_focus = accent;
        Self {
            surface_base,
            surface_sunken,
            surface_raised,
            surface_overlay,
            surface_scrim,
            surface_selected,
            surface_selected_muted,
            surface_danger,
            surface_warning,
            surface_success,
            text_primary,
            text_secondary,
            text_muted,
            text_disabled,
            text_on_accent,
            text_on_danger,
            text_link: accent_on_surface,
            accent,
            accent_alt,
            accent_soft,
            accent_on_surface,
            success,
            warning,
            danger,
            info,
            success_soft,
            warning_soft,
            danger_soft,
            info_soft,
            border_subtle,
            border_strong,
            border_focus,
            divider: border_subtle,
            scrollbar_track: surface_sunken,
            scrollbar_thumb: border_strong,
            focus_ring: accent,
            role_user: clamp_contrast(info.mix(text_primary, 0.25), surface_base, 4.5),
            role_assistant: clamp_contrast(accent.mix(text_primary, 0.20), surface_base, 4.5),
            role_tool: clamp_contrast(accent_alt.mix(text_primary, 0.20), surface_base, 4.5),
            role_system: text_secondary,
        }
    }
}

fn shift_toward_text(base: Rgb, light: bool, amount: f32) -> Rgb {
    let lab = base.to_oklab();
    let delta = if light { -amount } else { amount };
    lab.add_lightness(delta).to_rgb()
}

fn shift_from_text(base: Rgb, light: bool, amount: f32) -> Rgb {
    shift_toward_text(base, light, -amount)
}

fn contrast_ink(background: Rgb) -> Rgb {
    if background.relative_luminance() > 0.32 {
        Rgb::from_srgb8(0x0B, 0x0F, 0x19)
    } else {
        Rgb::from_srgb8(0xF8, 0xFA, 0xFC)
    }
}

#[cfg(test)]
mod tests {
    use autoharness_settings::ThemePreset;

    use super::{Ramp, Seed};

    #[test]
    fn system_and_dark_use_distinct_bases() {
        let system = Seed::for_preset(ThemePreset::System);
        let dark = Seed::for_preset(ThemePreset::Dark);
        assert_ne!(system.base.to_srgb8(), dark.base.to_srgb8());
        assert_ne!(
            Ramp::derive(system).surface_base.to_srgb8(),
            Ramp::derive(dark).surface_base.to_srgb8()
        );
    }
}
