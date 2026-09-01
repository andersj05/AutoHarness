//! Theme seeds, semantic color tokens, and renderer-neutral treatments.

use std::fmt::Write;

use autoharness_settings::{ColorMode, ThemePreset};

use crate::{Rgb, clamp_contrast};

/// Every stable theme preset in display order.
pub const THEME_PRESETS: [ThemePreset; 9] = [
    ThemePreset::System,
    ThemePreset::Light,
    ThemePreset::Dark,
    ThemePreset::Aurora,
    ThemePreset::Ember,
    ThemePreset::Midnight,
    ThemePreset::Ocean,
    ThemePreset::Forest,
    ThemePreset::Rose,
];

const COLOR_MODES: [ColorMode; 5] = [
    ColorMode::Color,
    ColorMode::Soft,
    ColorMode::Vivid,
    ColorMode::NoColor,
    ColorMode::HighContrast,
];

/// Four-value theme seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seed {
    pub base: Rgb,
    pub accent_a: Rgb,
    pub accent_b: Rgb,
    pub light: bool,
}

/// Fully derived semantic color ramp shared by every renderer.
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

/// Semantic color token shared by renderers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum Token {
    SurfaceBase,
    SurfaceSunken,
    SurfaceRaised,
    SurfaceOverlay,
    SurfaceScrim,
    SurfaceSelected,
    SurfaceSelectedMuted,
    SurfaceDanger,
    SurfaceWarning,
    SurfaceSuccess,
    TextPrimary,
    TextSecondary,
    TextMuted,
    TextDisabled,
    TextOnAccent,
    TextOnDanger,
    TextLink,
    Accent,
    AccentAlt,
    AccentSoft,
    AccentOnSurface,
    Success,
    Warning,
    Danger,
    Info,
    SuccessSoft,
    WarningSoft,
    DangerSoft,
    InfoSoft,
    BorderSubtle,
    BorderStrong,
    BorderFocus,
    Divider,
    ScrollbarTrack,
    ScrollbarThumb,
    FocusRing,
    RoleUser,
    RoleAssistant,
    RoleTool,
    RoleSystem,
}

impl Token {
    pub const ALL: [Self; 40] = [
        Self::SurfaceBase,
        Self::SurfaceSunken,
        Self::SurfaceRaised,
        Self::SurfaceOverlay,
        Self::SurfaceScrim,
        Self::SurfaceSelected,
        Self::SurfaceSelectedMuted,
        Self::SurfaceDanger,
        Self::SurfaceWarning,
        Self::SurfaceSuccess,
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextMuted,
        Self::TextDisabled,
        Self::TextOnAccent,
        Self::TextOnDanger,
        Self::TextLink,
        Self::Accent,
        Self::AccentAlt,
        Self::AccentSoft,
        Self::AccentOnSurface,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Info,
        Self::SuccessSoft,
        Self::WarningSoft,
        Self::DangerSoft,
        Self::InfoSoft,
        Self::BorderSubtle,
        Self::BorderStrong,
        Self::BorderFocus,
        Self::Divider,
        Self::ScrollbarTrack,
        Self::ScrollbarThumb,
        Self::FocusRing,
        Self::RoleUser,
        Self::RoleAssistant,
        Self::RoleTool,
        Self::RoleSystem,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn css_name(self) -> &'static str {
        match self {
            Self::SurfaceBase => "surface-base",
            Self::SurfaceSunken => "surface-sunken",
            Self::SurfaceRaised => "surface-raised",
            Self::SurfaceOverlay => "surface-overlay",
            Self::SurfaceScrim => "surface-scrim",
            Self::SurfaceSelected => "surface-selected",
            Self::SurfaceSelectedMuted => "surface-selected-muted",
            Self::SurfaceDanger => "surface-danger",
            Self::SurfaceWarning => "surface-warning",
            Self::SurfaceSuccess => "surface-success",
            Self::TextPrimary => "text-primary",
            Self::TextSecondary => "text-secondary",
            Self::TextMuted => "text-muted",
            Self::TextDisabled => "text-disabled",
            Self::TextOnAccent => "text-on-accent",
            Self::TextOnDanger => "text-on-danger",
            Self::TextLink => "text-link",
            Self::Accent => "accent",
            Self::AccentAlt => "accent-alt",
            Self::AccentSoft => "accent-soft",
            Self::AccentOnSurface => "accent-on-surface",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Info => "info",
            Self::SuccessSoft => "success-soft",
            Self::WarningSoft => "warning-soft",
            Self::DangerSoft => "danger-soft",
            Self::InfoSoft => "info-soft",
            Self::BorderSubtle => "border-subtle",
            Self::BorderStrong => "border-strong",
            Self::BorderFocus => "border-focus",
            Self::Divider => "divider",
            Self::ScrollbarTrack => "scrollbar-track",
            Self::ScrollbarThumb => "scrollbar-thumb",
            Self::FocusRing => "focus-ring",
            Self::RoleUser => "role-user",
            Self::RoleAssistant => "role-assistant",
            Self::RoleTool => "role-tool",
            Self::RoleSystem => "role-system",
        }
    }
}

impl Seed {
    #[must_use]
    pub fn for_preset(preset: ThemePreset) -> Self {
        let (base, accent_a, accent_b, light) = match preset {
            ThemePreset::System => (
                (0x08, 0x0c, 0x18),
                (0x22, 0xd3, 0xee),
                (0xa7, 0x8b, 0xfa),
                false,
            ),
            ThemePreset::Dark => (
                (0x05, 0x07, 0x0e),
                (0x22, 0xd3, 0xee),
                (0xa7, 0x8b, 0xfa),
                false,
            ),
            ThemePreset::Light => (
                (0xfa, 0xfa, 0xfb),
                (0x25, 0x63, 0xeb),
                (0xdb, 0x27, 0x77),
                true,
            ),
            ThemePreset::Aurora => (
                (0x04, 0x0f, 0x1e),
                (0x2d, 0xd4, 0xbf),
                (0x81, 0x8c, 0xf8),
                false,
            ),
            ThemePreset::Ember => (
                (0x1a, 0x0a, 0x0a),
                (0xfb, 0x92, 0x3c),
                (0xf4, 0x3f, 0x5e),
                false,
            ),
            ThemePreset::Midnight => (
                (0x03, 0x07, 0x12),
                (0x60, 0xa5, 0xfa),
                (0x63, 0x66, 0xf1),
                false,
            ),
            ThemePreset::Ocean => (
                (0x02, 0x14, 0x20),
                (0x22, 0xd3, 0xee),
                (0x0e, 0xa5, 0xe9),
                false,
            ),
            ThemePreset::Forest => (
                (0x07, 0x14, 0x0d),
                (0x4a, 0xde, 0x80),
                (0xfa, 0xcc, 0x15),
                false,
            ),
            ThemePreset::Rose => (
                (0x1d, 0x08, 0x14),
                (0xf4, 0x72, 0xb6),
                (0xc0, 0x84, 0xfc),
                false,
            ),
        };
        Self {
            base: Rgb::from_srgb8(base.0, base.1, base.2),
            accent_a: Rgb::from_srgb8(accent_a.0, accent_a.1, accent_a.2),
            accent_b: Rgb::from_srgb8(accent_b.0, accent_b.1, accent_b.2),
            light,
        }
    }
}

impl Ramp {
    #[must_use]
    pub fn derive(seed: Seed) -> Self {
        let surface_base = seed.base;
        let surface_sunken = shift_toward_text(surface_base, seed.light, -0.04);
        let surface_raised = shift_toward_text(surface_base, seed.light, 0.06);
        let surface_overlay = shift_toward_text(surface_base, seed.light, 0.10);
        let surface_scrim = surface_base.mix(
            Rgb::from_srgb8(0, 0, 0),
            if seed.light { 0.12 } else { 0.45 },
        );
        let text_anchor = if seed.light {
            Rgb::from_srgb8(0x0b, 0x0f, 0x19)
        } else {
            Rgb::from_srgb8(0xf8, 0xfa, 0xfc)
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
                Rgb::from_srgb8(0x15, 0x80, 0x3d),
                Rgb::from_srgb8(0xa1, 0x62, 0x07),
                Rgb::from_srgb8(0xbe, 0x12, 0x3c),
                Rgb::from_srgb8(0x1d, 0x4e, 0xd8),
            )
        } else {
            (
                Rgb::from_srgb8(0x4a, 0xde, 0x80),
                Rgb::from_srgb8(0xfb, 0xbf, 0x24),
                Rgb::from_srgb8(0xfb, 0x71, 0x85),
                Rgb::from_srgb8(0x60, 0xa5, 0xfa),
            )
        };
        let success_soft = success.mix(surface_base, 0.70);
        let warning_soft = warning.mix(surface_base, 0.70);
        let danger_soft = danger.mix(surface_base, 0.70);
        let info_soft = info.mix(surface_base, 0.70);
        let surface_selected = accent_alt.mix(surface_base, 0.15);
        let surface_selected_muted = accent.mix(surface_base, 0.55);
        let text_on_accent = clamp_contrast(contrast_ink(accent), accent, 4.5);
        let text_on_danger = clamp_contrast(contrast_ink(danger), danger, 4.5);
        let border_subtle = text_muted.mix(surface_base, 0.55);
        let border_strong = text_secondary.mix(surface_base, 0.25);
        Self {
            surface_base,
            surface_sunken,
            surface_raised,
            surface_overlay,
            surface_scrim,
            surface_selected,
            surface_selected_muted,
            surface_danger: danger_soft,
            surface_warning: warning_soft,
            surface_success: success_soft,
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
            border_focus: accent,
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

    #[must_use]
    pub const fn color(self, token: Token) -> Rgb {
        match token {
            Token::SurfaceBase => self.surface_base,
            Token::SurfaceSunken => self.surface_sunken,
            Token::SurfaceRaised => self.surface_raised,
            Token::SurfaceOverlay => self.surface_overlay,
            Token::SurfaceScrim => self.surface_scrim,
            Token::SurfaceSelected => self.surface_selected,
            Token::SurfaceSelectedMuted => self.surface_selected_muted,
            Token::SurfaceDanger => self.surface_danger,
            Token::SurfaceWarning => self.surface_warning,
            Token::SurfaceSuccess => self.surface_success,
            Token::TextPrimary => self.text_primary,
            Token::TextSecondary => self.text_secondary,
            Token::TextMuted => self.text_muted,
            Token::TextDisabled => self.text_disabled,
            Token::TextOnAccent => self.text_on_accent,
            Token::TextOnDanger => self.text_on_danger,
            Token::TextLink => self.text_link,
            Token::Accent => self.accent,
            Token::AccentAlt => self.accent_alt,
            Token::AccentSoft => self.accent_soft,
            Token::AccentOnSurface => self.accent_on_surface,
            Token::Success => self.success,
            Token::Warning => self.warning,
            Token::Danger => self.danger,
            Token::Info => self.info,
            Token::SuccessSoft => self.success_soft,
            Token::WarningSoft => self.warning_soft,
            Token::DangerSoft => self.danger_soft,
            Token::InfoSoft => self.info_soft,
            Token::BorderSubtle => self.border_subtle,
            Token::BorderStrong => self.border_strong,
            Token::BorderFocus => self.border_focus,
            Token::Divider => self.divider,
            Token::ScrollbarTrack => self.scrollbar_track,
            Token::ScrollbarThumb => self.scrollbar_thumb,
            Token::FocusRing => self.focus_ring,
            Token::RoleUser => self.role_user,
            Token::RoleAssistant => self.role_assistant,
            Token::RoleTool => self.role_tool,
            Token::RoleSystem => self.role_system,
        }
    }
}

pub const TEXT_PRIMARY_FLOOR: f32 = 7.0;
pub const TEXT_SECONDARY_FLOOR: f32 = 4.5;
pub const TEXT_MUTED_FLOOR: f32 = 3.5;
pub const TEXT_ON_ACCENT_FLOOR: f32 = 4.5;
pub const SEMANTIC_SOFT_FLOOR: f32 = 4.5;
pub const BORDER_FOCUS_FLOOR: f32 = 3.0;

/// Applies one shared color treatment and re-enforces contrast floors.
#[must_use]
pub fn resolve_ramp(ramp: Ramp, mode: ColorMode) -> Ramp {
    let mut ramp = match mode {
        ColorMode::Color => ramp,
        ColorMode::Soft => chroma_scaled(ramp, 0.65),
        ColorMode::Vivid => {
            let mut vivid = chroma_scaled(ramp, 1.25);
            vivid.accent = vivid.accent.to_oklab().add_lightness(0.08).to_rgb();
            vivid.accent_alt = vivid.accent_alt.to_oklab().add_lightness(0.08).to_rgb();
            vivid
        }
        ColorMode::NoColor => no_color_ramp(ramp),
        ColorMode::HighContrast => high_contrast_ramp(ramp),
    };
    ramp.text_primary = clamp_contrast(ramp.text_primary, ramp.surface_base, TEXT_PRIMARY_FLOOR);
    ramp.text_secondary =
        clamp_contrast(ramp.text_secondary, ramp.surface_base, TEXT_SECONDARY_FLOOR);
    ramp.text_muted = clamp_contrast(ramp.text_muted, ramp.surface_base, TEXT_MUTED_FLOOR);
    ramp.text_on_accent = clamp_contrast(
        ramp.text_on_accent,
        ramp.surface_selected,
        TEXT_ON_ACCENT_FLOOR,
    );
    ramp.success = clamp_contrast(ramp.success, ramp.success_soft, SEMANTIC_SOFT_FLOOR);
    ramp.warning = clamp_contrast(ramp.warning, ramp.warning_soft, SEMANTIC_SOFT_FLOOR);
    ramp.danger = clamp_contrast(ramp.danger, ramp.danger_soft, SEMANTIC_SOFT_FLOOR);
    ramp.info = clamp_contrast(ramp.info, ramp.info_soft, SEMANTIC_SOFT_FLOOR);
    ramp.border_focus = clamp_contrast(ramp.border_focus, ramp.surface_base, BORDER_FOCUS_FLOOR);
    ramp
}

/// Generates the complete color custom-property matrix consumed by the GUI.
#[must_use]
pub fn generate_css() -> String {
    let mut css =
        String::from("/* @generated by autoharness-presentation. Do not edit manually. */\n\n");
    for preset in THEME_PRESETS {
        for mode in COLOR_MODES {
            let preset_name = theme_preset_name(preset);
            let mode_name = color_mode_name(mode);
            let selector = if preset == ThemePreset::System && mode == ColorMode::Color {
                format!(":root, [data-theme=\"{preset_name}\"][data-color-mode=\"{mode_name}\"]")
            } else {
                format!("[data-theme=\"{preset_name}\"][data-color-mode=\"{mode_name}\"]")
            };
            let seed = Seed::for_preset(preset);
            let ramp = resolve_ramp(Ramp::derive(seed), mode);
            writeln!(css, "{selector} {{").expect("write to string");
            writeln!(
                css,
                "  color-scheme: {};",
                if seed.light { "light" } else { "dark" }
            )
            .expect("write to string");
            for token in Token::ALL {
                writeln!(
                    css,
                    "  --color-{}: {};",
                    token.css_name(),
                    ramp.color(token).to_hex()
                )
                .expect("write to string");
            }
            css.push_str("}\n\n");
        }
    }
    css
}

#[must_use]
pub const fn theme_preset_name(preset: ThemePreset) -> &'static str {
    match preset {
        ThemePreset::System => "system",
        ThemePreset::Light => "light",
        ThemePreset::Dark => "dark",
        ThemePreset::Aurora => "aurora",
        ThemePreset::Ember => "ember",
        ThemePreset::Midnight => "midnight",
        ThemePreset::Ocean => "ocean",
        ThemePreset::Forest => "forest",
        ThemePreset::Rose => "rose",
    }
}

const fn color_mode_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Color => "color",
        ColorMode::Soft => "soft",
        ColorMode::Vivid => "vivid",
        ColorMode::NoColor => "no-color",
        ColorMode::HighContrast => "high-contrast",
    }
}

fn shift_toward_text(base: Rgb, light: bool, amount: f32) -> Rgb {
    base.to_oklab()
        .add_lightness(if light { -amount } else { amount })
        .to_rgb()
}

fn contrast_ink(background: Rgb) -> Rgb {
    if background.relative_luminance() > 0.32 {
        Rgb::from_srgb8(0x0b, 0x0f, 0x19)
    } else {
        Rgb::from_srgb8(0xf8, 0xfa, 0xfc)
    }
}

fn chroma_scaled(mut ramp: Ramp, scale: f32) -> Ramp {
    for color in [
        &mut ramp.accent,
        &mut ramp.accent_alt,
        &mut ramp.accent_soft,
        &mut ramp.accent_on_surface,
        &mut ramp.success,
        &mut ramp.warning,
        &mut ramp.danger,
        &mut ramp.info,
        &mut ramp.role_user,
        &mut ramp.role_assistant,
        &mut ramp.role_tool,
    ] {
        *color = color.to_oklab().with_chroma_scale(scale).to_rgb();
    }
    ramp
}

fn no_color_ramp(ramp: Ramp) -> Ramp {
    let mut monochrome = ramp;
    let inverse = contrast_ink(ramp.surface_base);
    monochrome.accent = inverse;
    monochrome.accent_alt = inverse;
    monochrome.accent_on_surface = inverse;
    monochrome.success = inverse;
    monochrome.warning = inverse;
    monochrome.danger = inverse;
    monochrome.info = inverse;
    monochrome.role_user = inverse;
    monochrome.role_assistant = inverse;
    monochrome.role_tool = inverse;
    monochrome
}

fn high_contrast_ramp(ramp: Ramp) -> Ramp {
    let light = ramp.surface_base.relative_luminance() > 0.5;
    let base = if light {
        Rgb::from_srgb8(255, 255, 255)
    } else {
        Rgb::from_srgb8(0, 0, 0)
    };
    let inverse = if light {
        Rgb::from_srgb8(0, 0, 0)
    } else {
        Rgb::from_srgb8(255, 255, 255)
    };
    Ramp {
        surface_base: base,
        surface_sunken: base,
        surface_raised: base,
        surface_overlay: inverse,
        surface_scrim: base,
        surface_selected: inverse,
        surface_selected_muted: inverse,
        surface_danger: inverse,
        surface_warning: inverse,
        surface_success: inverse,
        text_primary: inverse,
        text_secondary: inverse,
        text_muted: inverse,
        text_disabled: inverse,
        text_on_accent: base,
        text_on_danger: base,
        text_link: inverse,
        accent: inverse,
        accent_alt: inverse,
        accent_soft: base,
        accent_on_surface: inverse,
        success: inverse,
        warning: inverse,
        danger: inverse,
        info: inverse,
        success_soft: base,
        warning_soft: base,
        danger_soft: base,
        info_soft: base,
        border_subtle: inverse,
        border_strong: inverse,
        border_focus: inverse,
        divider: inverse,
        scrollbar_track: base,
        scrollbar_thumb: inverse,
        focus_ring: inverse,
        role_user: inverse,
        role_assistant: inverse,
        role_tool: inverse,
        role_system: inverse,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_settings::{ColorMode, ThemePreset};

    use super::{
        BORDER_FOCUS_FLOOR, Ramp, SEMANTIC_SOFT_FLOOR, Seed, TEXT_MUTED_FLOOR,
        TEXT_ON_ACCENT_FLOOR, TEXT_PRIMARY_FLOOR, TEXT_SECONDARY_FLOOR, THEME_PRESETS,
        generate_css, resolve_ramp,
    };
    use crate::contrast_ratio;

    #[test]
    fn every_theme_and_treatment_meets_contrast_floors() {
        let modes = [
            ColorMode::Color,
            ColorMode::Soft,
            ColorMode::Vivid,
            ColorMode::NoColor,
            ColorMode::HighContrast,
        ];
        for preset in THEME_PRESETS {
            for mode in modes {
                let ramp = resolve_ramp(Ramp::derive(Seed::for_preset(preset)), mode);
                for (name, ratio, floor) in [
                    (
                        "primary",
                        contrast_ratio(ramp.text_primary, ramp.surface_base),
                        TEXT_PRIMARY_FLOOR,
                    ),
                    (
                        "secondary",
                        contrast_ratio(ramp.text_secondary, ramp.surface_base),
                        TEXT_SECONDARY_FLOOR,
                    ),
                    (
                        "muted",
                        contrast_ratio(ramp.text_muted, ramp.surface_base),
                        TEXT_MUTED_FLOOR,
                    ),
                    (
                        "on-accent",
                        contrast_ratio(ramp.text_on_accent, ramp.surface_selected),
                        TEXT_ON_ACCENT_FLOOR,
                    ),
                    (
                        "success",
                        contrast_ratio(ramp.success, ramp.success_soft),
                        SEMANTIC_SOFT_FLOOR,
                    ),
                    (
                        "warning",
                        contrast_ratio(ramp.warning, ramp.warning_soft),
                        SEMANTIC_SOFT_FLOOR,
                    ),
                    (
                        "danger",
                        contrast_ratio(ramp.danger, ramp.danger_soft),
                        SEMANTIC_SOFT_FLOOR,
                    ),
                    (
                        "info",
                        contrast_ratio(ramp.info, ramp.info_soft),
                        SEMANTIC_SOFT_FLOOR,
                    ),
                    (
                        "focus",
                        contrast_ratio(ramp.border_focus, ramp.surface_base),
                        BORDER_FOCUS_FLOOR,
                    ),
                ] {
                    assert!(
                        ratio + 0.001 >= floor,
                        "{preset:?} {mode:?} {name}: {ratio}"
                    );
                }
            }
        }
    }

    #[test]
    fn generated_css_covers_every_preset_and_mode() {
        let css = generate_css();
        assert_eq!(css.matches("--color-surface-base:").count(), 45);
        assert!(css.contains("[data-theme=\"rose\"][data-color-mode=\"high-contrast\"]"));
        assert!(css.contains("[data-theme=\"light\"][data-color-mode=\"soft\"]"));
    }

    #[test]
    fn system_and_dark_remain_distinct() {
        assert_ne!(
            Seed::for_preset(ThemePreset::System).base,
            Seed::for_preset(ThemePreset::Dark).base
        );
    }
}
