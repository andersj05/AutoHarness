//! Glyph triples, icon-set resolution, and chrome line marks.

use autoharness_settings::GlyphMode;

/// Semantic icon. Pages may not write glyph literals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Icon {
    Brand,
    RouteChat,
    RouteSessions,
    RouteProviders,
    RouteSettings,
    RouteModels,
    RouteHelp,
    User,
    Assistant,
    Tool,
    Workspace,
    GitBranch,
    Model,
    Thinking,
    Context,
    Tokens,
    Success,
    Warning,
    Danger,
    Info,
    Pending,
    Connected,
    Disconnected,
    Locked,
    Search,
    Collapsed,
    Expanded,
    SelectionCaret,
    PromptCaret,
    Archived,
    Default,
    Reset,
    Inherited,
}

impl Icon {
    /// Every icon, in table order.
    pub const ALL: [Self; 33] = [
        Self::Brand,
        Self::RouteChat,
        Self::RouteSessions,
        Self::RouteProviders,
        Self::RouteSettings,
        Self::RouteModels,
        Self::RouteHelp,
        Self::User,
        Self::Assistant,
        Self::Tool,
        Self::Workspace,
        Self::GitBranch,
        Self::Model,
        Self::Thinking,
        Self::Context,
        Self::Tokens,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Info,
        Self::Pending,
        Self::Connected,
        Self::Disconnected,
        Self::Locked,
        Self::Search,
        Self::Collapsed,
        Self::Expanded,
        Self::SelectionCaret,
        Self::PromptCaret,
        Self::Archived,
        Self::Default,
        Self::Reset,
        Self::Inherited,
    ];
}

/// Resolved glyph mode plus chrome marks for one frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IconSet {
    mode: GlyphMode,
}

impl IconSet {
    /// Resolves the glyph set from the effective glyph mode.
    #[must_use]
    pub const fn resolve(mode: GlyphMode) -> Self {
        Self { mode }
    }

    /// Effective glyph mode.
    #[must_use]
    pub const fn mode(self) -> GlyphMode {
        self.mode
    }

    /// Returns the glyph occupying the reserved cell budget for `icon`.
    #[must_use]
    pub fn glyph(self, icon: Icon) -> &'static str {
        let triple = triple(icon);
        match self.mode {
            GlyphMode::NerdFont => triple.nerd,
            GlyphMode::Unicode => triple.unicode,
            GlyphMode::Ascii => triple.ascii,
        }
    }

    /// Measured cell budget: two cells for Nerd Font, one otherwise.
    #[must_use]
    pub fn width(self, _icon: Icon) -> u16 {
        if self.mode == GlyphMode::NerdFont {
            2
        } else {
            1
        }
    }

    /// One line containing every icon in the selected mode, for Settings glyph check.
    #[must_use]
    pub fn glyph_check_line(self) -> String {
        let mut line = String::new();
        for icon in Icon::ALL {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(self.glyph(icon));
        }
        line
    }

    /// Box-drawing set for panels and modals.
    #[must_use]
    pub const fn border(self) -> BorderGlyphs {
        if matches!(self.mode, GlyphMode::Ascii) {
            BorderGlyphs::ASCII
        } else {
            BorderGlyphs::ROUNDED
        }
    }

    /// Vertical structural rule.
    #[must_use]
    pub const fn vertical_rule(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            "|"
        } else {
            "│"
        }
    }

    /// Horizontal structural rule.
    #[must_use]
    pub const fn horizontal_rule(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            "-"
        } else {
            "─"
        }
    }

    /// Compact metadata separator.
    #[must_use]
    pub const fn separator(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            " | "
        } else {
            " · "
        }
    }

    /// Truncation marker appropriate for the active glyph mode.
    #[must_use]
    pub const fn ellipsis(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            "..."
        } else {
            "…"
        }
    }

    /// Vertical navigation hint appropriate for the active glyph mode.
    #[must_use]
    pub const fn vertical_navigation_hint(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            "Up/Down"
        } else {
            "↑/↓"
        }
    }

    /// Horizontal navigation hint appropriate for the active glyph mode.
    #[must_use]
    pub const fn horizontal_navigation_hint(self) -> &'static str {
        if matches!(self.mode, GlyphMode::Ascii) {
            "Left/Right"
        } else {
            "←/→"
        }
    }
}

/// Box-drawing characters for a panel frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorderGlyphs {
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub horizontal: &'static str,
    pub vertical: &'static str,
}

impl BorderGlyphs {
    pub const ASCII: Self = Self {
        top_left: "+",
        top_right: "+",
        bottom_left: "+",
        bottom_right: "+",
        horizontal: "-",
        vertical: "|",
    };

    pub const ROUNDED: Self = Self {
        top_left: "╭",
        top_right: "╮",
        bottom_left: "╰",
        bottom_right: "╯",
        horizontal: "─",
        vertical: "│",
    };
}

struct Triple {
    nerd: &'static str,
    unicode: &'static str,
    ascii: &'static str,
}

fn triple(icon: Icon) -> Triple {
    match icon {
        Icon::Brand => Triple {
            nerd: " ",
            unicode: "◆",
            ascii: "#",
        },
        Icon::RouteChat => Triple {
            nerd: " ",
            unicode: "▣",
            ascii: "c",
        },
        Icon::RouteSessions => Triple {
            nerd: " ",
            unicode: "≡",
            ascii: "s",
        },
        Icon::RouteProviders => Triple {
            nerd: " ",
            unicode: "⌘",
            ascii: "p",
        },
        Icon::RouteSettings => Triple {
            nerd: " ",
            unicode: "⚙",
            ascii: "*",
        },
        Icon::RouteModels => Triple {
            nerd: " ",
            unicode: "◈",
            ascii: "m",
        },
        Icon::RouteHelp => Triple {
            nerd: " ",
            unicode: "?",
            ascii: "?",
        },
        Icon::User => Triple {
            nerd: " ",
            unicode: "☺",
            ascii: "u",
        },
        Icon::Assistant => Triple {
            nerd: " ",
            unicode: "◆",
            ascii: "a",
        },
        Icon::Tool => Triple {
            nerd: " ",
            unicode: "⚒",
            ascii: "&",
        },
        Icon::Workspace => Triple {
            nerd: " ",
            unicode: "▸",
            ascii: "/",
        },
        Icon::GitBranch => Triple {
            nerd: " ",
            unicode: "⑂",
            ascii: "*",
        },
        Icon::Model => Triple {
            nerd: " ",
            unicode: "●",
            ascii: "o",
        },
        Icon::Thinking => Triple {
            nerd: " ",
            unicode: "◐",
            ascii: "@",
        },
        Icon::Context => Triple {
            nerd: " ",
            unicode: "▰",
            ascii: "=",
        },
        Icon::Tokens => Triple {
            nerd: " ",
            unicode: "Σ",
            ascii: "T",
        },
        Icon::Success => Triple {
            nerd: " ",
            unicode: "✔",
            ascii: "+",
        },
        Icon::Warning => Triple {
            nerd: " ",
            unicode: "⚠",
            ascii: "!",
        },
        Icon::Danger => Triple {
            nerd: " ",
            unicode: "✖",
            ascii: "x",
        },
        Icon::Info => Triple {
            nerd: " ",
            unicode: "ⓘ",
            ascii: "i",
        },
        Icon::Pending => Triple {
            nerd: " ",
            unicode: "⠋",
            ascii: "|",
        },
        Icon::Connected => Triple {
            nerd: " ",
            unicode: "●",
            ascii: "*",
        },
        Icon::Disconnected => Triple {
            nerd: " ",
            unicode: "○",
            ascii: "-",
        },
        Icon::Locked => Triple {
            nerd: " ",
            unicode: "⚿",
            ascii: "K",
        },
        Icon::Search => Triple {
            nerd: " ",
            unicode: "⌕",
            ascii: "/",
        },
        Icon::Collapsed => Triple {
            nerd: " ",
            unicode: "▸",
            ascii: ">",
        },
        Icon::Expanded => Triple {
            nerd: " ",
            unicode: "▾",
            ascii: "v",
        },
        Icon::SelectionCaret => Triple {
            nerd: " ",
            unicode: "❯",
            ascii: ">",
        },
        Icon::PromptCaret => Triple {
            nerd: " ",
            unicode: "❯",
            ascii: ">",
        },
        Icon::Archived => Triple {
            nerd: " ",
            unicode: "▪",
            ascii: "~",
        },
        Icon::Default => Triple {
            nerd: " ",
            unicode: "★",
            ascii: "!",
        },
        Icon::Reset => Triple {
            nerd: " ",
            unicode: "↺",
            ascii: "^",
        },
        Icon::Inherited => Triple {
            nerd: " ",
            unicode: "↓",
            ascii: "v",
        },
    }
}

#[cfg(test)]
mod tests {
    use autoharness_settings::GlyphMode;
    use unicode_width::UnicodeWidthStr;

    use super::{Icon, IconSet};

    #[test]
    fn unicode_and_ascii_icons_measure_one_cell() {
        let mut failures = Vec::new();
        for mode in [GlyphMode::Unicode, GlyphMode::Ascii] {
            let icons = IconSet::resolve(mode);
            for icon in Icon::ALL {
                let glyph = icons.glyph(icon);
                let width = UnicodeWidthStr::width(glyph);
                if width != 1 {
                    failures.push(format!(
                        "{mode:?} {icon:?} {glyph:?} U+{:04X} measured {width}",
                        glyph.chars().next().unwrap() as u32
                    ));
                }
                if icons.width(icon) != 1 {
                    failures.push(format!("{mode:?} {icon:?} width()={}", icons.width(icon)));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn nerd_font_slot_measures_two_cells() {
        let icons = IconSet::resolve(GlyphMode::NerdFont);
        for icon in Icon::ALL {
            let glyph = icons.glyph(icon);
            assert!(
                glyph.ends_with(' '),
                "{icon:?} nerd slot must reserve a trailing space: {glyph:?}"
            );
            let width = UnicodeWidthStr::width(glyph);
            assert_eq!(width, 2, "{icon:?} {glyph:?} measured {width}");
            assert_eq!(icons.width(icon), 2);
        }
    }

    #[test]
    fn nerd_font_icons_stay_in_the_bmp_private_use_area() {
        let icons = IconSet::resolve(GlyphMode::NerdFont);
        for icon in Icon::ALL {
            let glyph = icons.glyph(icon);
            let codepoint = glyph.chars().next().expect("Nerd Font glyph") as u32;
            assert!(
                (0xE000..=0xF8FF).contains(&codepoint),
                "{icon:?} uses supplementary or non-private codepoint U+{codepoint:04X}"
            );
        }
    }

    #[test]
    fn glyph_check_line_contains_every_icon() {
        let icons = IconSet::resolve(GlyphMode::Unicode);
        let line = icons.glyph_check_line();
        for icon in Icon::ALL {
            assert!(
                line.contains(icons.glyph(icon)),
                "{icon:?} missing from {line}"
            );
        }
    }

    #[test]
    fn ascii_ellipsis_stays_ascii() {
        let ascii = IconSet::resolve(GlyphMode::Ascii);
        assert_eq!(ascii.ellipsis(), "...");
        assert_eq!(ascii.vertical_navigation_hint(), "Up/Down");
        assert_eq!(ascii.horizontal_navigation_hint(), "Left/Right");
        assert_eq!(IconSet::resolve(GlyphMode::Unicode).ellipsis(), "…");
        assert_eq!(IconSet::resolve(GlyphMode::NerdFont).ellipsis(), "…");
    }

    #[test]
    fn symbol_codepoints_live_only_in_icon_and_component_modules() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("ui");
        let allowed = [
            std::path::Path::new("icon.rs"),
            std::path::Path::new("motion.rs"),
            std::path::Path::new("component"),
        ];
        let mut violations = Vec::new();
        visit_rust_files(&root, &root, &allowed, &mut violations);
        assert!(
            violations.is_empty(),
            "box-drawing and symbol codepoints must live in ui/icon.rs, ui/motion.rs, and ui/component/:\n{}",
            violations.join("\n")
        );
    }

    fn visit_rust_files(
        root: &std::path::Path,
        dir: &std::path::Path,
        allowed: &[&std::path::Path],
        violations: &mut Vec<String>,
    ) {
        if !dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(dir).expect("read ui sources") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                let relative = path.strip_prefix(root).expect("ui-relative path");
                if allowed.iter().any(|allowed| relative.starts_with(allowed)) {
                    continue;
                }
                visit_rust_files(root, &path, allowed, violations);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("ui-relative path");
            if allowed.contains(&relative) {
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
                if let Some(ch) = first_symbol(line) {
                    violations.push(format!(
                        "{}:{} U+{:04X} {line}",
                        relative.display(),
                        index + 1,
                        u32::from(ch)
                    ));
                }
            }
        }
    }

    fn first_symbol(line: &str) -> Option<char> {
        line.chars().find(|ch| is_symbol(*ch))
    }

    fn is_symbol(ch: char) -> bool {
        matches!(
            ch as u32,
            0x03A3
                | 0x2193
                | 0x21BA
                | 0x2315
                | 0x2318
                | 0x2442
                | 0x24D8
                | 0x2500..=0x257F
                | 0x2580..=0x259F
                | 0x25A0..=0x25FF
                | 0x2600..=0x27BF
                | 0x2800..=0x28FF
                | 0xE000..=0xF8FF
                | 0xF0000..=0xFFFFD
        )
    }
}
