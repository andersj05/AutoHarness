//! Linear RGB and Oklab arithmetic shared by every renderer.

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
            srgb_to_linear(f32::from(red) / 255.0),
            srgb_to_linear(f32::from(green) / 255.0),
            srgb_to_linear(f32::from(blue) / 255.0),
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

    /// Returns a stable lowercase CSS hexadecimal color.
    #[must_use]
    pub fn to_hex(self) -> String {
        let (red, green, blue) = self.to_srgb8();
        format!("#{red:02x}{green:02x}{blue:02x}")
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

    /// Squared Oklab distance, useful for renderer-specific quantization.
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
    /// Returns chroma independently of lightness.
    #[must_use]
    pub fn chroma(self) -> f32 {
        (self.a * self.a + self.b * self.b).sqrt()
    }

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

fn unit_to_u8(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    let as_u16 = if scaled < 0.0 { 0 } else { scaled as u16 };
    u8::try_from(as_u16.min(255)).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Rgb, contrast_ratio};

    #[test]
    fn oklab_round_trip_preserves_srgb_anchors() {
        for channels in [(8, 12, 24), (34, 211, 238), (250, 250, 251), (0, 0, 0)] {
            let rgb = Rgb::from_srgb8(channels.0, channels.1, channels.2);
            assert_eq!(rgb.to_oklab().to_rgb().to_srgb8(), channels);
        }
    }

    #[test]
    fn wcag_extremes_have_maximum_contrast() {
        let ratio = contrast_ratio(Rgb::from_srgb8(255, 255, 255), Rgb::from_srgb8(0, 0, 0));
        assert!((ratio - 21.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn cyan_to_violet_midpoint_stays_chromatic() {
        let midpoint = Rgb::from_srgb8(0x22, 0xd3, 0xee)
            .mix(Rgb::from_srgb8(0xa7, 0x8b, 0xfa), 0.5)
            .to_oklab();
        assert!(midpoint.chroma() > 0.08, "{}", midpoint.chroma());
    }
}
