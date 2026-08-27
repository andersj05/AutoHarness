//! Style-aware terminal snapshot encoding for regression tests.

use std::fmt::Write as _;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// Serializes every cell's symbol, colors, and modifiers into a stable text form.
#[must_use]
pub fn style_snapshot(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    let _ = writeln!(out, "# autoharness-tui style snapshot v1");
    let _ = writeln!(out, "# {}x{}", area.width, area.height);
    for y in area.y..area.bottom() {
        let mut text = String::new();
        let mut runs = Vec::new();
        let mut current: Option<StyleRun> = None;
        for x in area.x..area.right() {
            let cell = buffer
                .cell((x, y))
                .expect("snapshot cell stays inside the buffer");
            text.push_str(cell.symbol());
            let sample = StyleRun {
                start: x,
                end: x,
                fg: cell.fg,
                bg: cell.bg,
                modifier: cell.modifier,
            };
            match current.as_mut() {
                Some(run) if run.same_style(sample) => run.end = x,
                Some(run) => {
                    runs.push(*run);
                    current = Some(sample);
                }
                None => current = Some(sample),
            }
        }
        if let Some(run) = current {
            runs.push(run);
        }
        let _ = writeln!(out, "@{y}");
        out.push_str(&text);
        out.push('\n');
        for run in runs {
            let _ = write!(
                out,
                " | {}-{} fg={} bg={}",
                run.start,
                run.end,
                format_color(run.fg),
                format_color(run.bg)
            );
            let modifiers = format_modifier(run.modifier);
            if !modifiers.is_empty() {
                let _ = write!(out, " {modifiers}");
            }
            out.push('\n');
        }
    }
    out
}

#[derive(Clone, Copy)]
struct StyleRun {
    start: u16,
    end: u16,
    fg: Color,
    bg: Color,
    modifier: Modifier,
}

impl StyleRun {
    fn same_style(self, other: Self) -> bool {
        self.fg == other.fg && self.bg == other.bg && self.modifier == other.modifier
    }
}

fn format_color(color: Color) -> String {
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

fn format_modifier(modifier: Modifier) -> String {
    const FLAGS: [(Modifier, &str); 9] = [
        (Modifier::BOLD, "bold"),
        (Modifier::DIM, "dim"),
        (Modifier::ITALIC, "italic"),
        (Modifier::UNDERLINED, "underlined"),
        (Modifier::SLOW_BLINK, "slowblink"),
        (Modifier::RAPID_BLINK, "rapidblink"),
        (Modifier::REVERSED, "reversed"),
        (Modifier::HIDDEN, "hidden"),
        (Modifier::CROSSED_OUT, "crossedout"),
    ];
    let mut parts = Vec::new();
    for (flag, name) in FLAGS {
        if modifier.contains(flag) {
            parts.push(name);
        }
    }
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};

    use super::style_snapshot;

    #[test]
    fn style_snapshot_records_symbol_colors_and_modifiers() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
        for (x, glyph) in ['H', 'e', 'l', 'l', 'o'].into_iter().enumerate() {
            let x = u16::try_from(x).expect("fixture column");
            let style = if x < 2 {
                Style::new()
                    .fg(Color::Rgb(34, 211, 238))
                    .bg(Color::Rgb(8, 12, 24))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Gray).bg(Color::Reset)
            };
            let cell = buffer
                .cell_mut((x, 0))
                .expect("fixture cell stays inside the buffer");
            cell.set_char(glyph);
            cell.set_style(style);
        }

        assert_eq!(
            style_snapshot(&buffer),
            concat!(
                "# autoharness-tui style snapshot v1\n",
                "# 5x1\n",
                "@0\n",
                "Hello\n",
                " | 0-1 fg=#22d3ee bg=#080c18 bold\n",
                " | 2-4 fg=gray bg=reset\n",
            )
        );
    }
}
