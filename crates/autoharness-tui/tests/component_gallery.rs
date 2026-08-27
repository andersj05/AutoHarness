// Component gallery: renders every presentation component in representative
// variants at the reviewed widths. Ignored by default like visual_review.
use autoharness_settings::{ColorMode, ThemePreset};
use autoharness_tui::style_snapshot;
use autoharness_tui::ui::component::{
    Button, ButtonRow, ButtonVariant, Callout, Chip, ChipVariant, Hero, KeyValue, KeyValueTable,
    ListItem, ListView, MessageBlock, Meter, MeterThreshold, Modal, Panel, Provenance, SearchField,
    SegmentedControl, SettingKind, SettingRow, StatusBar, StatusSegment, ToolCard,
};
use autoharness_tui::{ColorDepth, Icon, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn theme() -> Theme {
    Theme::from_preset(ThemePreset::System, ColorMode::Color, ColorDepth::TrueColor)
}

fn dump(label: &str, buf: &Buffer) {
    println!("=== {label} ===");
    print!("{}", style_snapshot(buf));
}

fn area(width: u16, height: u16) -> (Buffer, Rect) {
    let area = Rect::new(0, 0, width, height);
    (Buffer::empty(area), area)
}

#[test]
#[ignore = "component gallery; run with --ignored"]
fn render_component_gallery() {
    let theme = theme();
    let icons = theme.icons();
    for width in [40_u16, 60, 80, 120] {
        let (mut buf, rect) = area(width, 1);
        Chip::new(&theme, "active", ChipVariant::Accent).render(&mut buf, rect);
        Chip::new(&theme, "archived", ChipVariant::Muted).render(&mut buf, Rect::new(10, 0, 12, 1));
        dump(&format!("chip {width}"), &buf);

        let (mut buf, rect) = area(width, 1);
        Meter::new(
            &theme,
            icons,
            Icon::Context,
            "ctx",
            "82%",
            5,
            6,
            MeterThreshold::Warning,
        )
        .render(&mut buf, rect);
        dump(&format!("meter {width}"), &buf);

        let (mut buf, rect) = area(width, 1);
        let _ = SearchField::new(&theme, icons, "sprint", 3, Some(2), true).render(&mut buf, rect);
        dump(&format!("search {width}"), &buf);

        let (mut buf, rect) = area(width, 1);
        let buttons = [
            Button::new("Cancel", Some("Esc".into()), ButtonVariant::Secondary, "c"),
            Button::new("Delete", Some("Y".into()), ButtonVariant::Danger, "d"),
        ];
        let _ = ButtonRow::new(&theme, &buttons).render(&mut buf, rect);
        dump(&format!("buttons {width}"), &buf);

        let (mut buf, rect) = area(width, 2);
        SegmentedControl::new(&theme, &["unicode", "nerd font", "ascii"], 1).render(&mut buf, rect);
        dump(&format!("segmented {width}"), &buf);

        let (mut buf, rect) = area(width, 6);
        let rows = [
            KeyValue {
                label: "Git",
                value: "feat/tui",
                chip: Some("user"),
            },
            KeyValue {
                label: "Workspace",
                value: "~/Desktop/AutoHarness",
                chip: Some("default"),
            },
        ];
        let _ = KeyValueTable::new(&theme, &rows).render(&mut buf, rect);
        dump(&format!("key-value {width}"), &buf);

        let (mut buf, rect) = area(width, 1);
        let segments = [
            StatusSegment {
                priority: 0,
                icon: Some(Icon::Model),
                text: "gemini-2.5-pro",
            },
            StatusSegment {
                priority: 1,
                icon: Some(Icon::Thinking),
                text: "high",
            },
            StatusSegment {
                priority: 3,
                icon: Some(Icon::GitBranch),
                text: "feat/tui-gradient-icons",
            },
        ];
        let _ = StatusBar::new(&theme, icons, &segments, icons.separator()).render(&mut buf, rect);
        dump(&format!("status {width}"), &buf);

        let (mut buf, rect) = area(width, 8);
        let _ = Panel::new(
            &theme,
            icons,
            Some(Icon::RouteSettings),
            Some("Settings"),
            Some("Appearance"),
            Some("Esc back"),
            true,
        )
        .render(&mut buf, rect);
        dump(&format!("panel {width}"), &buf);

        let (mut buf, rect) = area(width, 10);
        let ok = [Button::new(
            "OK",
            Some("Enter".into()),
            ButtonVariant::Primary,
            "ok",
        )];
        let _ = Modal::new(&theme, icons, "Confirm", Some(Icon::Warning), &ok)
            .render(&mut buf, rect, 36, 8);
        dump(&format!("modal {width}"), &buf);

        let (mut buf, rect) = area(width, 6);
        let items = [ListItem {
            label: "Cafe launch",
            metadata: Some("2m ago"),
            group: Some("Today"),
            badges: &[],
            action: 0,
        }];
        let _ = ListView::new(&theme, icons, &items, 0, "No sessions").render(&mut buf, rect);
        dump(&format!("list {width}"), &buf);

        let (mut buf, rect) = area(width, 3);
        let _ = SettingRow::new(
            &theme,
            "Reduced motion",
            SettingKind::Toggle { on: true },
            Provenance::User,
            Some("Freezes animation tables."),
            true,
            16,
        )
        .render(&mut buf, rect);
        dump(&format!("setting {width}"), &buf);

        let (mut buf, rect) = area(width, 6);
        let _ = MessageBlock::new(
            &theme,
            icons,
            Icon::User,
            "YOU",
            "just now",
            "Plan a cafe launch.",
        )
        .render(&mut buf, rect);
        dump(&format!("message {width}"), &buf);

        let (mut buf, rect) = area(width, 4);
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
        .render(&mut buf, rect);
        dump(&format!("tool {width}"), &buf);

        let (mut buf, rect) = area(width, 8);
        let retry = [Button::new(
            "Retry",
            Some("Ctrl+R".into()),
            ButtonVariant::Primary,
            "retry",
        )];
        let _ = Callout::new(
            &theme,
            icons,
            Icon::Danger,
            "Failed",
            "Capacity is temporarily exhausted.",
            &retry,
        )
        .render(&mut buf, rect);
        dump(&format!("callout {width}"), &buf);

        let (mut buf, rect) = area(width, 8);
        Hero::new(
            &theme,
            icons,
            "AutoHarness",
            "Connect a provider to begin",
            &["Credential", "Model", "Prompt"],
            "Open Settings",
        )
        .render(&mut buf, rect);
        dump(&format!("hero {width}"), &buf);
    }
}
