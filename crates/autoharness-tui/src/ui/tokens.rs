//! Ratatui paint translation for shared semantic appearance tokens.

use autoharness_presentation::{Ramp, Rgb, resolve_ramp};
use autoharness_settings::ColorMode;
use ratatui::style::Modifier;

pub use autoharness_presentation::{
    BORDER_FOCUS_FLOOR, SEMANTIC_SOFT_FLOOR, TEXT_MUTED_FLOOR, TEXT_ON_ACCENT_FLOOR,
    TEXT_PRIMARY_FLOOR, TEXT_SECONDARY_FLOOR, Token,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackgroundIntent {
    Inherit,
    Surface(Token),
    Transparent,
}

#[derive(Clone, Copy, Debug)]
pub struct TokenPaint {
    pub foreground: Rgb,
    pub background: Option<Rgb>,
    pub intent: BackgroundIntent,
    pub modifiers: Modifier,
}

#[must_use]
pub fn paints(ramp: Ramp, mode: ColorMode) -> ([TokenPaint; Token::ALL.len()], Ramp) {
    let ramp = resolve_ramp(ramp, mode);
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

fn paint_for(token: Token, ramp: &Ramp, mode: ColorMode) -> TokenPaint {
    if mode == ColorMode::NoColor {
        return no_color_paint(token);
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
    let inherit = |foreground| TokenPaint {
        foreground,
        background: None,
        intent: BackgroundIntent::Inherit,
        modifiers: Modifier::empty(),
    };
    let transparent = |foreground| TokenPaint {
        foreground,
        background: None,
        intent: BackgroundIntent::Transparent,
        modifiers: Modifier::empty(),
    };
    let surface = |background, foreground, surface_token| TokenPaint {
        foreground,
        background: Some(background),
        intent: BackgroundIntent::Surface(surface_token),
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

fn no_color_paint(token: Token) -> TokenPaint {
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
        Token::TextLink => Modifier::UNDERLINED,
        _ => Modifier::empty(),
    };
    TokenPaint {
        foreground: Rgb::from_srgb8(0, 0, 0),
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
