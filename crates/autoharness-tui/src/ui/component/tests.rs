use autoharness_settings::{ColorMode, ThemePreset};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::color::ColorDepth;
use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::{
    Button, ButtonRow, ButtonVariant, Callout, Chip, ChipVariant, Hero, KeyValue, KeyValueTable,
    ListItem, ListView, MessageBlock, Meter, MeterThreshold, Modal, ModalIntent, Panel, Provenance,
    SearchField, SegmentedControl, SettingKind, SettingRow, StatusBar, StatusSegment, ToolCard,
    modal_size, paint, scrim,
};
use crate::snapshot::style_snapshot;

const WIDTHS: [u16; 4] = [40, 60, 80, 120];

fn theme() -> Theme {
    Theme::from_preset(ThemePreset::System, ColorMode::Color, ColorDepth::TrueColor)
}

fn icons() -> IconSet {
    theme().icons()
}

fn assert_symbols_and_styles(name: &str, buf: &Buffer, needle: &str) {
    let snap = style_snapshot(buf);
    assert!(
        snap.contains(needle),
        "{name} missing {needle:?} in\n{snap}"
    );
    assert!(snap.contains("fg="), "{name} missing style runs in\n{snap}");
}

fn for_widths(height: u16, mut paint: impl FnMut(&mut Buffer, Rect)) {
    for width in WIDTHS {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        paint(&mut buf, area);
        let snap = style_snapshot(&buf);
        assert!(
            snap.contains("fg="),
            "{width}x{height} missing styles:\n{snap}"
        );
    }
}

#[test]
fn every_component_renders_symbols_and_styles_at_reviewed_widths() {
    let theme = theme();
    let icons = icons();
    for_widths(8, |buf, area| {
        Chip::new(&theme, "active", ChipVariant::Accent).render(buf, area);
    });
    for_widths(2, |buf, area| {
        Meter::new(
            &theme,
            icons,
            Icon::Thinking,
            "high",
            "4/6",
            4,
            6,
            MeterThreshold::None,
        )
        .render(buf, area);
    });
    for_widths(2, |buf, area| {
        let _ = SearchField::new(&theme, icons, "sprint", 6, Some(3), true).render(buf, area);
    });
    let buttons = [
        Button::new(
            "Cancel",
            Some("Esc".into()),
            ButtonVariant::Secondary,
            "cancel",
        ),
        Button::new("Delete", Some("Y".into()), ButtonVariant::Danger, "delete"),
        Button::new("Sign in", None, ButtonVariant::Primary, "sign in"),
    ];
    for_widths(2, |buf, area| {
        let _ = ButtonRow::new(&theme, &buttons).render(buf, area);
    });
    let options = ["unicode", "nerd font", "ascii"];
    for_widths(2, |buf, area| {
        SegmentedControl::new(&theme, &options, 0).render(buf, area);
    });
    let rows = [
        KeyValue {
            label: "Git",
            value: "feat/tui-gradient-icons",
            chip: Some("user"),
        },
        KeyValue {
            label: "Workspace",
            value: "~/Desktop/AutoHarness",
            chip: Some("default"),
        },
        KeyValue {
            label: "Model",
            value: "Gemini 2.5 Pro",
            chip: None,
        },
    ];
    for_widths(8, |buf, area| {
        let _ = KeyValueTable::new(&theme, &rows).render(buf, area);
    });
    let segments = [
        StatusSegment {
            priority: 0,
            icon: Some(Icon::Model),
            text: "gemini-2.5-pro-preview-extra-long",
        },
        StatusSegment {
            priority: 1,
            icon: Some(Icon::Thinking),
            text: "high",
        },
        StatusSegment {
            priority: 2,
            icon: Some(Icon::Workspace),
            text: "~/Desktop/AutoHarness/crates/autoharness-tui",
        },
        StatusSegment {
            priority: 3,
            icon: Some(Icon::GitBranch),
            text: "feat/tui-gradient-icons-with-a-very-long-branch",
        },
    ];
    for_widths(2, |buf, area| {
        let _ = StatusBar::new(&theme, icons, &segments, icons.separator()).render(buf, area);
    });
    for_widths(10, |buf, area| {
        let _ = Panel::new(
            &theme,
            icons,
            Some(Icon::RouteSettings),
            Some("Settings"),
            Some("Appearance"),
            Some("Esc back"),
            true,
        )
        .render(buf, area);
    });
    for_widths(12, |buf, area| {
        paint::fill(
            buf,
            area,
            theme.style(super::super::tokens::Token::TextPrimary),
            Some('#'),
        );
        scrim::render(buf, area, &theme);
        let buttons = [Button::new(
            "OK",
            Some("Enter".into()),
            ButtonVariant::Primary,
            "ok",
        )];
        let _ = Modal::new(&theme, icons, "Confirm", Some(Icon::Warning), &buttons)
            .render(buf, area, 40, 8);
    });
    let items = [
        ListItem {
            label: "Cafe launch",
            metadata: Some("2m ago"),
            group: Some("Today"),
            badges: &[],
            action: 0,
        },
        ListItem {
            label: "Earlier notes",
            metadata: Some("3d ago"),
            group: Some("This week"),
            badges: &[],
            action: 1,
        },
    ];
    for_widths(8, |buf, area| {
        let _ = ListView::new(&theme, icons, &items, 0, "No sessions").render(buf, area);
    });
    for_widths(8, |buf, area| {
        let _ = SettingRow::new(
            &theme,
            "Theme",
            SettingKind::Choice {
                options: &["system", "dark", "light"],
                selected: 0,
            },
            Provenance::User,
            Some("Changes the seed palette."),
            true,
            12,
        )
        .render(buf, area);
    });
    for_widths(8, |buf, area| {
        let _ = MessageBlock::new(
            &theme,
            icons,
            Icon::Assistant,
            "AUTOHARNESS",
            "1.2s · 41 tok",
            "Start with a two-week validation sprint.",
        )
        .render(buf, area);
    });
    for_widths(6, |buf, area| {
        let _ = ToolCard::new(
            &theme,
            icons,
            "fs_read",
            "workspace:menu.md",
            "12ms",
            "27 bytes read",
            true,
            Icon::Success,
        )
        .render(buf, area);
    });
    let retry = [Button::new(
        "Retry",
        Some("Ctrl+R".into()),
        ButtonVariant::Primary,
        "retry",
    )];
    for_widths(10, |buf, area| {
        let _ = Callout::new(
            &theme,
            icons,
            Icon::Danger,
            "Failed",
            "Capacity is temporarily exhausted.",
            &retry,
        )
        .render(buf, area);
    });
    for_widths(12, |buf, area| {
        Hero::new(
            &theme,
            icons,
            "AutoHarness",
            "Start a conversation",
            &["Connect", "Choose a model", "Ask"],
            "Open Settings",
        )
        .render(buf, area);
    });
}

#[test]
fn button_row_hit_regions_match_rendered_captions() {
    let theme = theme();
    let buttons = [
        Button::new(
            "Cancel",
            Some("Esc".into()),
            ButtonVariant::Secondary,
            "cancel",
        ),
        Button::new("Delete", Some("Y".into()), ButtonVariant::Danger, "delete"),
    ];
    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    let hits = ButtonRow::new(&theme, &buttons).render(&mut buf, area);
    assert_eq!(hits.len(), 2);
    for (rect, action) in &hits {
        let mut caption = String::new();
        for x in rect.x..rect.right() {
            caption.push_str(buf.cell((x, rect.y)).expect("hit cell").symbol());
        }
        assert!(
            caption.starts_with("[ ") && caption.ends_with(" ]"),
            "{action} caption {caption:?} at {:?}",
            rect
        );
        assert!(
            caption.to_lowercase().contains(action),
            "{caption:?} {action}"
        );
        assert_eq!(rect.y, 0);
        assert_eq!(rect.height, 1);
        assert_eq!(
            u16::try_from(caption.chars().count()).unwrap_or(0),
            rect.width
        );
    }
}

#[test]
fn key_value_table_aligns_mixed_length_labels() {
    let theme = theme();
    let rows = [
        KeyValue {
            label: "Git",
            value: "main",
            chip: Some("user"),
        },
        KeyValue {
            label: "Workspace path",
            value: "~/src",
            chip: Some("default"),
        },
        KeyValue {
            label: "Id",
            value: "session-1",
            chip: None,
        },
    ];
    let table = KeyValueTable::new(&theme, &rows);
    assert_eq!(table.label_width(), 14);
    let area = Rect::new(0, 0, 80, 6);
    let mut buf = Buffer::empty(area);
    let value_x = table.render(&mut buf, area);
    assert_eq!(value_x, 15);
    for y in 0..3 {
        let label = buf.cell((0, y)).expect("label").symbol();
        assert_ne!(label, " ");
        let gap = buf.cell((14, y)).expect("pad").symbol();
        assert_eq!(gap, " ");
    }
}

#[test]
fn status_bar_drops_lowest_priority_at_each_breakpoint() {
    let theme = theme();
    let icons = icons();
    let segments = [
        StatusSegment {
            priority: 0,
            icon: None,
            text: "model-name",
        },
        StatusSegment {
            priority: 1,
            icon: None,
            text: "think",
        },
        StatusSegment {
            priority: 2,
            icon: None,
            text: "workspace-path-is-deliberately-long",
        },
        StatusSegment {
            priority: 3,
            icon: None,
            text: "feature/branch-name-is-also-long",
        },
    ];
    let bar = StatusBar::new(&theme, icons, &segments, " | ");
    let at_40 = bar.visible(40);
    assert!(at_40.contains(&0));
    assert!(
        !at_40.contains(&3),
        "lowest priority should drop at 40: {at_40:?}"
    );
    let at_120 = bar.visible(120);
    assert_eq!(at_120, vec![0, 1, 2, 3]);
}

#[test]
fn modal_sizing_uses_one_clamp_table() {
    let tiny = Rect::new(0, 0, 40, 10);
    assert_eq!(modal_size(tiny, 72, 24), tiny);
    let wide = Rect::new(0, 0, 120, 40);
    let sized = modal_size(wide, 80, 30);
    assert!(sized.width <= 72);
    assert!(sized.height <= 24);
    assert!(sized.x > 0);
}

#[test]
fn modal_scrim_occludes_background_and_intent_selects_the_border_rule() {
    let theme = theme();
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    for y in 0..area.height {
        for x in 0..area.width {
            buf.cell_mut((x, y)).expect("host cell").set_symbol("X");
        }
    }
    let buttons = [Button::new(
        "Confirm",
        Some("Y".to_owned()),
        ButtonVariant::Danger,
        (),
    )];
    let frame = modal_size(area, 40, 8);
    let _ = Modal::new(
        &theme,
        theme.icons(),
        "Danger",
        Some(Icon::Danger),
        &buttons,
    )
    .intent(ModalIntent::Danger)
    .render(&mut buf, area, 40, 8);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let outside = x < frame.x || x >= frame.right() || y < frame.y || y >= frame.bottom();
            if outside {
                assert_eq!(buf.cell((x, y)).expect("scrim cell").symbol(), " ");
            }
        }
    }
    assert_eq!(
        buf.cell((frame.x, frame.y)).expect("danger border").fg,
        theme.style(Token::Danger).fg.expect("danger foreground")
    );
}

#[test]
fn setting_info_rows_are_not_editable() {
    assert!(!SettingKind::Info { value: "read only" }.editable());
    assert!(SettingKind::Toggle { on: true }.editable());
}

#[test]
fn representative_components_pin_style_snapshots() {
    let theme = theme();
    let icons = icons();
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
    let area = buf.area;
    Chip::new(&theme, "active", ChipVariant::Success).render(&mut buf, area);
    assert_symbols_and_styles("chip", &buf, "active");

    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 1));
    let area = buf.area;
    let _ = SearchField::new(&theme, icons, "query", 2, Some(4), true).render(&mut buf, area);
    assert_symbols_and_styles("search", &buf, "query");
}

#[test]
fn whole_word_ellipsis_never_leaks_a_partial_word() {
    assert_eq!(paint::ellipsize_words("alpha beta gamma", 9), "alpha...");
    assert_eq!(paint::ellipsize_words("extraordinary", 8), "...");
    assert_eq!(paint::ellipsize_words("alpha beta", 20), "alpha beta");
}
