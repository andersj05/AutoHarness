//! Renderer-neutral appearance seeds, semantic tokens, and color treatments.
//!
//! Renderers translate this resolved appearance into their own paint types.

#![forbid(unsafe_code)]

mod color;
mod theme;

pub use color::{Oklab, Rgb, clamp_contrast, contrast_ratio};
pub use theme::{
    BORDER_FOCUS_FLOOR, Ramp, SEMANTIC_SOFT_FLOOR, Seed, TEXT_MUTED_FLOOR, TEXT_ON_ACCENT_FLOOR,
    TEXT_PRIMARY_FLOOR, TEXT_SECONDARY_FLOOR, THEME_PRESETS, Token, generate_css, resolve_ramp,
    theme_preset_name,
};
