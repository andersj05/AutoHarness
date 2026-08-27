//! Dims a region by replacing every cell style with `surface_scrim`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::super::theme::Theme;
use super::super::tokens::Token;

/// Applies the scrim token to every cell in `area`, preserving symbols.
pub fn render(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let area = area.intersection(buf.area);
    let style = theme.style(Token::SurfaceScrim);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_style(style);
            }
        }
    }
}
