//! Renderer-neutral color arithmetic plus terminal-specific quantization.

use ratatui::style::Color;

pub use autoharness_presentation::{Rgb, clamp_contrast, contrast_ratio};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorDepth {
    #[default]
    TrueColor,
    Indexed256,
    Basic16,
}

impl ColorDepth {
    #[must_use]
    pub fn detect_from_env(colorterm: Option<&str>, term: Option<&str>) -> Self {
        if colorterm.is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("truecolor") || value.contains("24bit")
        }) {
            return Self::TrueColor;
        }
        if term.is_some_and(|value| value.to_ascii_lowercase().contains("256color")) {
            return Self::Indexed256;
        }
        Self::Basic16
    }

    #[must_use]
    pub fn detect() -> Self {
        Self::detect_from_env(
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuantizedColor {
    pub color: Color,
    pub bold: bool,
}

#[must_use]
pub fn quantize(rgb: Rgb, depth: ColorDepth) -> QuantizedColor {
    match depth {
        ColorDepth::TrueColor => {
            let (red, green, blue) = rgb.to_srgb8();
            QuantizedColor {
                color: Color::Rgb(red, green, blue),
                bold: false,
            }
        }
        ColorDepth::Indexed256 => QuantizedColor {
            color: Color::Indexed(nearest_xterm256(rgb)),
            bold: false,
        },
        ColorDepth::Basic16 => nearest_basic16(rgb),
    }
}

#[must_use]
pub const fn reset_color() -> Color {
    Color::Reset
}

#[must_use]
pub fn format_color(color: Color) -> String {
    match color {
        Color::Reset => "reset".to_owned(),
        Color::Black => "black".to_owned(),
        Color::Red => "red".to_owned(),
        Color::Green => "green".to_owned(),
        Color::Yellow => "yellow".to_owned(),
        Color::Blue => "blue".to_owned(),
        Color::Magenta => "magenta".to_owned(),
        Color::Cyan => "cyan".to_owned(),
        Color::Gray => "gray".to_owned(),
        Color::DarkGray => "darkgray".to_owned(),
        Color::LightRed => "lightred".to_owned(),
        Color::LightGreen => "lightgreen".to_owned(),
        Color::LightYellow => "lightyellow".to_owned(),
        Color::LightBlue => "lightblue".to_owned(),
        Color::LightMagenta => "lightmagenta".to_owned(),
        Color::LightCyan => "lightcyan".to_owned(),
        Color::White => "white".to_owned(),
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Color::Indexed(index) => format!("i{index}"),
    }
}

fn nearest_xterm256(rgb: Rgb) -> u8 {
    let mut best_index = 0_u8;
    let mut best_distance = f32::MAX;
    for index in 0..=255_u8 {
        let distance = rgb.oklab_distance_squared(xterm256_rgb(index));
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

fn nearest_basic16(rgb: Rgb) -> QuantizedColor {
    let mut best = 0_usize;
    let mut best_distance = f32::MAX;
    for index in 0..16_usize {
        let distance = rgb.oklab_distance_squared(ansi16_rgb(index));
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    QuantizedColor {
        color: ANSI16_COLORS[best],
        bold: best >= 8,
    }
}

fn xterm256_rgb(index: u8) -> Rgb {
    match index {
        0..=15 => ansi16_rgb(usize::from(index)),
        16..=231 => {
            let cube = index - 16;
            Rgb::from_srgb8(
                cube_channel(cube / 36),
                cube_channel((cube % 36) / 6),
                cube_channel(cube % 6),
            )
        }
        _ => {
            let level = 8 + 10 * u16::from(index - 232);
            let gray = u8::try_from(level.min(255)).unwrap_or(u8::MAX);
            Rgb::from_srgb8(gray, gray, gray)
        }
    }
}

const fn cube_channel(level: u8) -> u8 {
    if level == 0 { 0 } else { 55 + 40 * level }
}

const ANSI16_SRGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

fn ansi16_rgb(index: usize) -> Rgb {
    let (red, green, blue) = ANSI16_SRGB[index];
    Rgb::from_srgb8(red, green, blue)
}

const ANSI16_COLORS: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{
        ColorDepth, Rgb, contrast_ratio, format_color, nearest_basic16, nearest_xterm256, quantize,
    };

    #[test]
    fn white_on_black_meets_the_maximum_contrast_ratio() {
        let ratio = contrast_ratio(Rgb::from_srgb8(255, 255, 255), Rgb::from_srgb8(0, 0, 0));
        assert!((ratio - 21.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn oklab_round_trip_preserves_srgb_anchors() {
        for channels in [(8, 12, 24), (34, 211, 238), (250, 250, 251), (0, 0, 0)] {
            let rgb = Rgb::from_srgb8(channels.0, channels.1, channels.2);
            assert_eq!(rgb.to_oklab().to_rgb().to_srgb8(), channels);
        }
    }

    #[test]
    fn indexed256_quantization_pins_system_accent() {
        let accent = Rgb::from_srgb8(0x22, 0xd3, 0xee);
        assert_eq!(nearest_xterm256(accent), 45);
        assert_eq!(
            quantize(accent, ColorDepth::Indexed256).color,
            Color::Indexed(45)
        );
    }

    #[test]
    fn basic16_quantization_pins_system_accent_to_cyan() {
        let quantized = nearest_basic16(Rgb::from_srgb8(0x22, 0xd3, 0xee));
        assert_eq!(quantized.color, Color::Cyan);
        assert!(!quantized.bold);
    }

    #[test]
    fn color_depth_detects_environment_capabilities() {
        assert_eq!(
            ColorDepth::detect_from_env(Some("truecolor"), Some("xterm")),
            ColorDepth::TrueColor
        );
        assert_eq!(
            ColorDepth::detect_from_env(None, Some("xterm-256color")),
            ColorDepth::Indexed256
        );
        assert_eq!(
            ColorDepth::detect_from_env(None, Some("xterm")),
            ColorDepth::Basic16
        );
    }

    #[test]
    fn format_color_uses_stable_tokens() {
        assert_eq!(format_color(Color::Reset), "reset");
        assert_eq!(format_color(Color::Rgb(34, 211, 238)), "#22d3ee");
        assert_eq!(format_color(Color::Indexed(45)), "i45");
    }
}
