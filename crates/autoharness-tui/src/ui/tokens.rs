//! Semantic tokens, background intents, and color-mode treatments.

use autoharness_settings::ColorMode;
use ratatui::style::Modifier;

use super::color::{Rgb, clamp_contrast};
use super::palette::Ramp;

/// Semantic color token. Pages may use only these names.
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
    /// Every token, in index order.
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

    /// Stable index into a resolved style table.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How a token paints its cell background.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundIntent {
    /// Leave the existing background unchanged.
    Inherit,
    /// Paint a named surface.
    Surface(Token),
    /// Force a transparent (`Reset`) background.
    Transparent,
}

/// Unquantized paint for one token.
#[derive(Clone, Copy, Debug)]
pub struct TokenPaint {
    pub foreground: Rgb,
    pub background: Option<Rgb>,
    pub intent: BackgroundIntent,
    pub modifiers: Modifier,
}

/// Applies color-mode treatment and contrast clamps to a derived ramp.
#[must_use]
pub fn paints(ramp: Ramp, mode: ColorMode) -> ([TokenPaint; Token::ALL.len()], Ramp) {
    let ramp = enforce_floors(treat_ramp(ramp, mode));
    let mut paints = [TokenPaint {
        foreground: ramp.text_primary,
        background: None,
        intent: BackgroundIntent::Transparent,
        modifiers: Modifier::empty(),
    }; Token::ALL.len()];
    for token in Token::ALL {
        paints[token.index()] = paint_for(token, &ramp, mode);
    }
    (paints, ramp)
}

fn treat_ramp(mut ramp: Ramp, mode: ColorMode) -> Ramp {
    match mode {
        ColorMode::Color | ColorMode::NoColor => ramp,
        ColorMode::Soft => {
            scale_chroma(&mut ramp, 0.65);
            ramp
        }
        ColorMode::Vivid => {
            scale_chroma(&mut ramp, 1.25);
            ramp.accent = ramp.accent.to_oklab().add_lightness(0.08).to_rgb();
            ramp.accent_alt = ramp.accent_alt.to_oklab().add_lightness(0.08).to_rgb();
            ramp
        }
        ColorMode::HighContrast => high_contrast_ramp(ramp),
    }
}

fn scale_chroma(ramp: &mut Ramp, scale: f32) {
    ramp.accent = ramp.accent.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.accent_alt = ramp.accent_alt.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.accent_soft = ramp
        .accent_soft
        .to_oklab()
        .with_chroma_scale(scale)
        .to_rgb();
    ramp.accent_on_surface = ramp
        .accent_on_surface
        .to_oklab()
        .with_chroma_scale(scale)
        .to_rgb();
    ramp.success = ramp.success.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.warning = ramp.warning.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.danger = ramp.danger.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.info = ramp.info.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.role_user = ramp.role_user.to_oklab().with_chroma_scale(scale).to_rgb();
    ramp.role_assistant = ramp
        .role_assistant
        .to_oklab()
        .with_chroma_scale(scale)
        .to_rgb();
    ramp.role_tool = ramp.role_tool.to_oklab().with_chroma_scale(scale).to_rgb();
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

fn paint_for(token: Token, ramp: &Ramp, mode: ColorMode) -> TokenPaint {
    if mode == ColorMode::NoColor {
        return nocolor_paint(token);
    }
    let mut paint = colored_paint(token, ramp);
    if mode == ColorMode::Vivid && is_accent_or_semantic_text(token) {
        paint.modifiers |= Modifier::BOLD;
    }
    if mode == ColorMode::HighContrast {
        if is_accent_or_semantic_text(token) {
            paint.modifiers |= Modifier::BOLD;
        }
        if matches!(token, Token::SurfaceSelected | Token::FocusRing) {
            paint.modifiers |= Modifier::REVERSED | Modifier::UNDERLINED;
        }
        if matches!(
            token,
            Token::BorderSubtle | Token::Divider | Token::BorderFocus
        ) {
            paint.foreground = ramp.border_strong;
        }
    }
    paint
}

fn colored_paint(token: Token, ramp: &Ramp) -> TokenPaint {
    let inherit = |foreground: Rgb| TokenPaint {
        foreground,
        background: None,
        intent: BackgroundIntent::Inherit,
        modifiers: Modifier::empty(),
    };
    let transparent = |foreground: Rgb| TokenPaint {
        foreground,
        background: None,
        intent: BackgroundIntent::Transparent,
        modifiers: Modifier::empty(),
    };
    let surface = |background: Rgb, foreground: Rgb, token: Token| TokenPaint {
        foreground,
        background: Some(background),
        intent: BackgroundIntent::Surface(token),
        modifiers: Modifier::empty(),
    };
    match token {
        Token::SurfaceBase => surface(ramp.surface_base, ramp.text_primary, token),
        Token::SurfaceSunken => surface(ramp.surface_sunken, ramp.text_primary, token),
        Token::SurfaceRaised => surface(ramp.surface_raised, ramp.text_primary, token),
        Token::SurfaceOverlay => surface(ramp.surface_overlay, ramp.text_primary, token),
        Token::SurfaceScrim => transparent(ramp.text_muted),
        Token::SurfaceSelected => surface(ramp.surface_selected, ramp.text_on_accent, token),
        Token::SurfaceSelectedMuted => {
            surface(ramp.surface_selected_muted, ramp.text_primary, token)
        }
        Token::SurfaceDanger => surface(ramp.surface_danger, ramp.text_on_danger, token),
        Token::SurfaceWarning => surface(ramp.surface_warning, ramp.text_primary, token),
        Token::SurfaceSuccess => surface(ramp.surface_success, ramp.text_primary, token),
        Token::TextPrimary => inherit(ramp.text_primary),
        Token::TextSecondary => inherit(ramp.text_secondary),
        Token::TextMuted => inherit(ramp.text_muted),
        Token::TextDisabled => inherit(ramp.text_disabled),
        Token::TextOnAccent => TokenPaint {
            foreground: ramp.text_on_accent,
            background: Some(ramp.surface_selected),
            intent: BackgroundIntent::Surface(Token::SurfaceSelected),
            modifiers: Modifier::BOLD,
        },
        Token::TextOnDanger => TokenPaint {
            foreground: ramp.text_on_danger,
            background: Some(ramp.danger),
            intent: BackgroundIntent::Surface(Token::SurfaceDanger),
            modifiers: Modifier::BOLD,
        },
        Token::TextLink => TokenPaint {
            foreground: ramp.text_link,
            background: None,
            intent: BackgroundIntent::Transparent,
            modifiers: Modifier::UNDERLINED,
        },
        Token::Accent => transparent(ramp.accent),
        Token::AccentAlt => transparent(ramp.accent_alt),
        Token::AccentSoft => surface(ramp.accent_soft, ramp.accent, token),
        Token::AccentOnSurface => transparent(ramp.accent_on_surface),
        Token::Success => transparent(ramp.success),
        Token::Warning => transparent(ramp.warning),
        Token::Danger => TokenPaint {
            foreground: ramp.danger,
            background: None,
            intent: BackgroundIntent::Transparent,
            modifiers: Modifier::BOLD,
        },
        Token::Info => transparent(ramp.info),
        Token::SuccessSoft => surface(ramp.success_soft, ramp.success, token),
        Token::WarningSoft => surface(ramp.warning_soft, ramp.warning, token),
        Token::DangerSoft => surface(ramp.danger_soft, ramp.danger, token),
        Token::InfoSoft => surface(ramp.info_soft, ramp.info, token),
        Token::BorderSubtle => transparent(ramp.border_subtle),
        Token::BorderStrong => transparent(ramp.border_strong),
        Token::BorderFocus => transparent(ramp.border_focus),
        Token::Divider => transparent(ramp.divider),
        Token::ScrollbarTrack => surface(ramp.scrollbar_track, ramp.scrollbar_thumb, token),
        Token::ScrollbarThumb => transparent(ramp.scrollbar_thumb),
        Token::FocusRing => TokenPaint {
            foreground: ramp.focus_ring,
            background: None,
            intent: BackgroundIntent::Transparent,
            modifiers: Modifier::BOLD,
        },
        Token::RoleUser => TokenPaint {
            foreground: ramp.role_user,
            background: None,
            intent: BackgroundIntent::Transparent,
            modifiers: Modifier::BOLD,
        },
        Token::RoleAssistant => TokenPaint {
            foreground: ramp.role_assistant,
            background: None,
            intent: BackgroundIntent::Transparent,
            modifiers: Modifier::BOLD,
        },
        Token::RoleTool => transparent(ramp.role_tool),
        Token::RoleSystem => transparent(ramp.role_system),
    }
}

fn nocolor_paint(token: Token) -> TokenPaint {
    let dummy = Rgb::from_srgb8(0, 0, 0);
    let modifiers = match token {
        Token::SurfaceSelected
        | Token::SurfaceSelectedMuted
        | Token::TextOnAccent
        | Token::FocusRing
        | Token::SurfaceRaised
        | Token::AccentSoft => Modifier::BOLD | Modifier::REVERSED,
        Token::SurfaceSuccess | Token::Success | Token::SuccessSoft => Modifier::BOLD,
        Token::SurfaceWarning | Token::Warning | Token::WarningSoft => Modifier::UNDERLINED,
        Token::SurfaceDanger | Token::Danger | Token::DangerSoft | Token::TextOnDanger => {
            Modifier::BOLD | Modifier::UNDERLINED
        }
        Token::Info | Token::InfoSoft => Modifier::REVERSED,
        Token::RoleUser | Token::RoleAssistant | Token::RoleTool => Modifier::BOLD,
        Token::TextMuted | Token::TextDisabled | Token::TextSecondary => Modifier::empty(),
        Token::TextLink => Modifier::UNDERLINED,
        _ => Modifier::empty(),
    };
    TokenPaint {
        foreground: dummy,
        background: None,
        intent: BackgroundIntent::Transparent,
        modifiers,
    }
}

const fn is_accent_or_semantic_text(token: Token) -> bool {
    matches!(
        token,
        Token::Accent
            | Token::AccentAlt
            | Token::AccentOnSurface
            | Token::Success
            | Token::Warning
            | Token::Danger
            | Token::Info
            | Token::TextLink
            | Token::RoleUser
            | Token::RoleAssistant
            | Token::RoleTool
            | Token::FocusRing
    )
}

/// Contrast floors from the design system.
pub const TEXT_PRIMARY_FLOOR: f32 = 7.0;
pub const TEXT_SECONDARY_FLOOR: f32 = 4.5;
pub const TEXT_MUTED_FLOOR: f32 = 3.5;
pub const TEXT_ON_ACCENT_FLOOR: f32 = 4.5;
pub const SEMANTIC_SOFT_FLOOR: f32 = 4.5;
pub const BORDER_FOCUS_FLOOR: f32 = 3.0;

/// Re-clamps the ramp after color-mode treatment so floors still hold.
#[must_use]
pub fn enforce_floors(ramp: Ramp) -> Ramp {
    let mut ramp = ramp;
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
