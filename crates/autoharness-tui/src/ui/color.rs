//! Linear RGB, Oklab arithmetic, contrast, and terminal quantization.

use ratatui::style::Color;

/// Detected terminal color capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorDepth {
    /// Twenty-four-bit `Color::Rgb` emission.
    #[default]
    TrueColor,
    /// Nearest xterm-256 index.
    Indexed256,
    /// Nearest of the sixteen ANSI colors.
    Basic16,
}

impl ColorDepth {
    /// Detects color depth from `COLORTERM` and `TERM`.
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

    /// Detects color depth from the process environment.
    #[must_use]
    pub fn detect() -> Self {
        Self::detect_from_env(
            std::env::var("COLORTERM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        )
    }
}

/// Linear-light RGB in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    red: f32,
    green: f32,
    blue: f32,
}

/// Oklab coordinates used for mixing, chroma, and lightness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    lightness: f32,
    a: f32,
    b: f32,
}

impl Rgb {
    /// Creates a linear-light RGB triple, clamping each channel.
    #[must_use]
    pub fn from_linear(red: f32, green: f32, blue: f32) -> Self {
        Self {
            red: red.clamp(0.0, 1.0),
            green: green.clamp(0.0, 1.0),
            blue: blue.clamp(0.0, 1.0),
        }
    }

    /// Creates linear-light RGB from an 8-bit sRGB triple.
    #[must_use]
    pub fn from_srgb8(red: u8, green: u8, blue: u8) -> Self {
        Self::from_linear(
            srgb_to_linear(u8_to_unit(red)),
            srgb_to_linear(u8_to_unit(green)),
            srgb_to_linear(u8_to_unit(blue)),
        )
    }

    /// Returns 8-bit sRGB channels.
    #[must_use]
    pub fn to_srgb8(self) -> (u8, u8, u8) {
        (
            unit_to_u8(linear_to_srgb(self.red)),
            unit_to_u8(linear_to_srgb(self.green)),
            unit_to_u8(linear_to_srgb(self.blue)),
        )
    }

    /// WCAG relative luminance from linear channels.
    #[must_use]
    pub fn relative_luminance(self) -> f32 {
        0.2126 * self.red + 0.7152 * self.green + 0.0722 * self.blue
    }

    /// Converts to Oklab.
    #[must_use]
    #[allow(clippy::excessive_precision)]
    pub fn to_oklab(self) -> Oklab {
        let long =
            0.412_221_470_8 * self.red + 0.536_332_536_3 * self.green + 0.051_445_992_9 * self.blue;
        let medium =
            0.211_903_498_2 * self.red + 0.680_699_545_1 * self.green + 0.107_396_956_6 * self.blue;
        let short =
            0.088_302_461_9 * self.red + 0.281_718_837_6 * self.green + 0.629_978_700_5 * self.blue;
        let long_c = long.cbrt();
        let medium_c = medium.cbrt();
        let short_c = short.cbrt();
        Oklab {
            lightness: 0.210_454_255_3 * long_c + 0.793_617_785_0 * medium_c
                - 0.004_072_046_8 * short_c,
            a: 1.977_998_495_1 * long_c - 2.428_592_205_0 * medium_c + 0.450_593_709_9 * short_c,
            b: 0.025_904_037_1 * long_c + 0.782_771_766_2 * medium_c - 0.808_675_766_0 * short_c,
        }
    }

    /// Interpolates toward `other` in Oklab.
    #[must_use]
    pub fn mix(self, other: Self, amount: f32) -> Self {
        self.to_oklab()
            .mix(other.to_oklab(), amount.clamp(0.0, 1.0))
            .to_rgb()
    }

    /// Squared Oklab distance, used for quantization.
    #[must_use]
    pub fn oklab_distance_squared(self, other: Self) -> f32 {
        let left = self.to_oklab();
        let right = other.to_oklab();
        let dl = left.lightness - right.lightness;
        let da = left.a - right.a;
        let db = left.b - right.b;
        dl * dl + da * da + db * db
    }
}

impl Oklab {
    /// Interpolates toward `other`.
    #[must_use]
    pub fn mix(self, other: Self, amount: f32) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        Self {
            lightness: self.lightness + (other.lightness - self.lightness) * amount,
            a: self.a + (other.a - self.a) * amount,
            b: self.b + (other.b - self.b) * amount,
        }
    }

    /// Scales chroma without changing lightness.
    #[must_use]
    pub fn with_chroma_scale(self, scale: f32) -> Self {
        Self {
            lightness: self.lightness,
            a: self.a * scale,
            b: self.b * scale,
        }
    }

    /// Adds a lightness delta, clamped to `0.0..=1.0`.
    #[must_use]
    pub fn add_lightness(self, delta: f32) -> Self {
        Self {
            lightness: (self.lightness + delta).clamp(0.0, 1.0),
            a: self.a,
            b: self.b,
        }
    }

    /// Converts back to linear RGB.
    #[must_use]
    #[allow(clippy::excessive_precision)]
    pub fn to_rgb(self) -> Rgb {
        let long_c = self.lightness + 0.396_337_777_4 * self.a + 0.215_803_757_3 * self.b;
        let medium_c = self.lightness - 0.105_561_345_8 * self.a - 0.063_854_172_8 * self.b;
        let short_c = self.lightness - 0.089_484_177_5 * self.a - 1.291_485_548_0 * self.b;
        let long = long_c * long_c * long_c;
        let medium = medium_c * medium_c * medium_c;
        let short = short_c * short_c * short_c;
        Rgb::from_linear(
            4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
            -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
            -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short,
        )
    }
}

/// WCAG contrast ratio of two colors.
#[must_use]
pub fn contrast_ratio(left: Rgb, right: Rgb) -> f32 {
    let first = left.relative_luminance();
    let second = right.relative_luminance();
    let (higher, lower) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (higher + 0.05) / (lower + 0.05)
}

/// Mixes `foreground` toward black or white until `floor` is met.
#[must_use]
pub fn clamp_contrast(foreground: Rgb, background: Rgb, floor: f32) -> Rgb {
    if contrast_ratio(foreground, background) >= floor {
        return foreground;
    }
    let white = Rgb::from_srgb8(255, 255, 255);
    let black = Rgb::from_srgb8(0, 0, 0);
    let target = if contrast_ratio(white, background) >= contrast_ratio(black, background) {
        white
    } else {
        black
    };
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if contrast_ratio(foreground.mix(target, mid), background) >= floor {
            high = mid;
        } else {
            low = mid;
        }
    }
    foreground.mix(target, high)
}

/// Terminal color plus an optional bold bit for the ANSI bright half.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuantizedColor {
    /// Ratatui color to emit.
    pub color: Color,
    /// When set, the 16-color mapping uses bold to reach the bright half.
    pub bold: bool,
}

/// Converts linear RGB into the terminal's color depth.
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

/// Returns the reset color used for transparent backgrounds.
#[must_use]
pub const fn reset_color() -> Color {
    Color::Reset
}

/// Formats a Ratatui color for style-aware snapshots.
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
        let candidate = xterm256_rgb(index);
        let distance = rgb.oklab_distance_squared(candidate);
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
            let red = cube / 36;
            let green = (cube % 36) / 6;
            let blue = cube % 6;
            Rgb::from_srgb8(cube_channel(red), cube_channel(green), cube_channel(blue))
        }
        _ => {
            let level = 8 + 10 * u16::from(index - 232);
            let gray = u8::try_from(level.min(255)).unwrap_or(u8::MAX);
            Rgb::from_srgb8(gray, gray, gray)
        }
    }
}

fn cube_channel(level: u8) -> u8 {
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

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn u8_to_unit(value: u8) -> f32 {
    f32::from(value) / 255.0
}

fn unit_to_u8(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    let as_u16 = if scaled < 0.0 { 0 } else { scaled as u16 };
    u8::try_from(as_u16.min(255)).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        ColorDepth, Rgb, contrast_ratio, format_color, nearest_basic16, nearest_xterm256, quantize,
    };
    use ratatui::style::Color;

    #[test]
    fn white_on_black_meets_the_maximum_contrast_ratio() {
        let white = Rgb::from_srgb8(255, 255, 255);
        let black = Rgb::from_srgb8(0, 0, 0);
        let ratio = contrast_ratio(white, black);
        assert!((ratio - 21.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn oklab_round_trip_preserves_srgb_anchors() {
        for (red, green, blue) in [(8, 12, 24), (34, 211, 238), (250, 250, 251), (0, 0, 0)] {
            let rgb = Rgb::from_srgb8(red, green, blue);
            let back = rgb.to_oklab().to_rgb().to_srgb8();
            assert_eq!(back, (red, green, blue));
        }
    }

    #[test]
    fn cyan_to_violet_midpoint_stays_chromatic() {
        let cyan = Rgb::from_srgb8(0x22, 0xD3, 0xEE);
        let violet = Rgb::from_srgb8(0xA7, 0x8B, 0xFA);
        let mid = cyan.mix(violet, 0.5).to_oklab();
        let chroma = (mid.a * mid.a + mid.b * mid.b).sqrt();
        assert!(chroma > 0.08, "{chroma}");
    }

    #[test]
    fn indexed256_quantization_pins_system_accent() {
        let accent = Rgb::from_srgb8(0x22, 0xD3, 0xEE);
        assert_eq!(nearest_xterm256(accent), 45);
        assert_eq!(
            quantize(accent, ColorDepth::Indexed256).color,
            Color::Indexed(45)
        );
    }

    #[test]
    fn basic16_quantization_pins_system_accent_to_cyan() {
        let accent = Rgb::from_srgb8(0x22, 0xD3, 0xEE);
        let quantized = nearest_basic16(accent);
        assert_eq!(quantized.color, Color::Cyan);
        assert!(!quantized.bold);
    }

    #[test]
    fn color_depth_detects_truecolor_and_256_from_env() {
        assert_eq!(
            ColorDepth::detect_from_env(Some("truecolor"), Some("xterm")),
            ColorDepth::TrueColor
        );
        assert_eq!(
            ColorDepth::detect_from_env(Some(""), Some("xterm-256color")),
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
