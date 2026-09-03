//! Renderer-neutral theme seeds and ramps shared with the desktop client.

pub use autoharness_presentation::{Ramp, Seed};

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
