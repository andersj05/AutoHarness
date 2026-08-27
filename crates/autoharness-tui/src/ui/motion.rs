//! Animation frame tables, tick math, and the reduced-motion gate.

use autoharness_settings::GlyphMode;

/// Monotonic tick source plus motion policy for one frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Motion {
    now: u64,
    last_activity: u64,
    reduced: bool,
    glyph_mode: GlyphMode,
}

impl Motion {
    /// Fastest allowed animation step.
    pub const REPAINT_FLOOR_MS: u64 = 100;
    /// Animations freeze after this much idle time.
    pub const IDLE_SUSPEND_MS: u64 = 30_000;

    /// Creates a motion sample from the clock and accessibility flags.
    #[must_use]
    pub const fn new(now: u64, last_activity: u64, reduced: bool, glyph_mode: GlyphMode) -> Self {
        Self {
            now,
            last_activity,
            reduced,
            glyph_mode,
        }
    }

    /// Returns whether a new animation frame may be shown.
    #[must_use]
    pub const fn animating(self) -> bool {
        !self.reduced && self.now.saturating_sub(self.last_activity) <= Self::IDLE_SUSPEND_MS
    }

    /// Pending spinner glyph selected by glyph mode and the motion gate.
    #[must_use]
    pub fn pending_glyph(self) -> &'static str {
        if matches!(self.glyph_mode, GlyphMode::Ascii) {
            PENDING_ASCII[self.frame_index(PENDING_ASCII.len())]
        } else {
            PENDING_BRAILLE[self.frame_index(PENDING_BRAILLE.len())]
        }
    }

    /// ASCII generation scanner used by the current Chat surface.
    #[must_use]
    pub fn generation_scanner(self) -> &'static str {
        if self.reduced {
            GENERATION_STATIC
        } else {
            GENERATION_SCANNER[self.frame_index(GENERATION_SCANNER.len())]
        }
    }

    /// Design-system ASCII streaming bar for later Chat conversion.
    #[must_use]
    pub fn streaming_wave_ascii(self) -> &'static str {
        if !self.animating() {
            STREAMING_STATIC
        } else {
            STREAMING_WAVE[self.frame_index(STREAMING_WAVE.len())]
        }
    }

    /// Normalized phase for a six-cell gradient sweep.
    #[must_use]
    pub fn wave_phase(self) -> f32 {
        if !self.animating() {
            0.0
        } else {
            f32::from(u16::try_from(self.step() % 6).unwrap_or(0)) / 6.0
        }
    }

    fn step(self) -> u64 {
        self.now / Self::REPAINT_FLOOR_MS
    }

    fn frame_index(self, len: usize) -> usize {
        if !self.animating() || len == 0 {
            0
        } else {
            usize::try_from(self.step()).unwrap_or(0) % len
        }
    }
}

const PENDING_BRAILLE: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const PENDING_ASCII: [&str; 4] = ["|", "/", "-", "\\"];
const GENERATION_STATIC: &str = "[--------]";
const GENERATION_SCANNER: [&str; 16] = [
    "[>-------]",
    "[=>------]",
    "[==>-----]",
    "[===>----]",
    "[-===>---]",
    "[--===>--]",
    "[---===>-]",
    "[----===>]",
    "[----<===]",
    "[---<===-]",
    "[--<===--]",
    "[-<===---]",
    "[<===----]",
    "[<==-----]",
    "[<=------]",
    "[<-------]",
];
const STREAMING_STATIC: &str = "[========]";
const STREAMING_WAVE: [&str; 8] = [
    "[==>-----]",
    "[-==>----]",
    "[--==>---]",
    "[---==>--]",
    "[----==>-]",
    "[-----==>]",
    "[====>---]",
    "[===>----]",
];

#[cfg(test)]
mod tests {
    use autoharness_settings::GlyphMode;

    use super::Motion;

    fn live(now: u64, glyph_mode: GlyphMode) -> Motion {
        Motion::new(now, 0, false, glyph_mode)
    }

    #[test]
    fn reduced_motion_uses_the_static_first_frame() {
        let reduced = Motion::new(700, 0, true, GlyphMode::Ascii);
        let later = Motion::new(1_500, 0, true, GlyphMode::Ascii);
        assert_eq!(reduced.pending_glyph(), "|");
        assert_eq!(reduced.pending_glyph(), later.pending_glyph());
        assert_eq!(reduced.generation_scanner(), "[--------]");
        assert_eq!(reduced.generation_scanner(), later.generation_scanner());
        assert!(!reduced.animating());
    }

    #[test]
    fn idle_suspension_freezes_after_thirty_seconds() {
        let active = Motion::new(1_000, 1_000, false, GlyphMode::Unicode);
        let idle = Motion::new(
            1_000 + Motion::IDLE_SUSPEND_MS + 1,
            1_000,
            false,
            GlyphMode::Unicode,
        );
        assert!(active.animating());
        assert!(!idle.animating());
        assert_eq!(idle.pending_glyph(), "⠋");
        assert_eq!(idle.wave_phase(), 0.0);
    }

    #[test]
    fn repaint_floor_holds_a_frame_for_one_hundred_milliseconds() {
        let first = live(0, GlyphMode::Ascii);
        let same = live(99, GlyphMode::Ascii);
        let next = live(100, GlyphMode::Ascii);
        assert_eq!(first.pending_glyph(), same.pending_glyph());
        assert_ne!(first.pending_glyph(), next.pending_glyph());
        assert_eq!(first.pending_glyph(), "|");
        assert_eq!(next.pending_glyph(), "/");
    }

    #[test]
    fn animation_frame_tables_live_only_in_motion() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations = Vec::new();
        visit(&root, &root, &mut violations);
        assert!(
            violations.is_empty(),
            "animation frame tables must live in ui/motion.rs:\n{}",
            violations.join("\n")
        );
    }

    fn visit(root: &std::path::Path, dir: &std::path::Path, violations: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read sources") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, violations);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("src-relative path");
            if relative == std::path::Path::new("ui/motion.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read rust source");
            let mut in_test = false;
            for (index, line) in source.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("#[cfg(test)]") {
                    in_test = true;
                }
                if in_test && trimmed == "}" && !line.starts_with(' ') && !line.starts_with('\t') {
                    in_test = false;
                }
                if in_test {
                    continue;
                }
                if line.contains("\"⠙\"")
                    || line.contains("[>-------]")
                    || line.contains("[==>-----]")
                {
                    violations.push(format!("{}:{}:{line}", relative.display(), index + 1));
                }
            }
        }
    }
}
