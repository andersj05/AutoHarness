//! Occludes a region with the scrim surface before a modal is painted.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::theme::Theme;
use super::super::tokens::Token;

/// Applies the scrim token to every cell in `area` and clears background symbols.
pub fn render(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let area = area.intersection(buf.area);
    let style = theme.style(Token::SurfaceScrim);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.set_style(style);
            }
        }
    }
}
