//! Full-width selection list with optional groups, metadata, and a hit map.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::super::icon::{Icon, IconSet};
use super::super::theme::Theme;
use super::super::tokens::Token;
use super::paint::{ellipsize, fill, put, right_align};

/// One list row.
pub struct ListItem<'a, A> {
    pub label: &'a str,
    pub metadata: Option<&'a str>,
    pub group: Option<&'a str>,
    pub action: A,
}

/// Scrollable list.
pub struct ListView<'a, A> {
    theme: &'a Theme,
    icons: IconSet,
    items: &'a [ListItem<'a, A>],
    selected: usize,
    empty: &'a str,
}

impl<'a, A: Clone> ListView<'a, A> {
    /// Creates a list.
    #[must_use]
    pub const fn new(
        theme: &'a Theme,
        icons: IconSet,
        items: &'a [ListItem<'a, A>],
        selected: usize,
        empty: &'a str,
    ) -> Self {
        Self {
            theme,
            icons,
            items,
            selected,
            empty,
        }
    }

    /// Renders the list and returns row hit regions.
    pub fn render(&self, buf: &mut Buffer, area: Rect) -> Vec<(Rect, A)> {
        let mut hits = Vec::new();
        if area.width == 0 || area.height == 0 {
            return hits;
        }
        if self.items.is_empty() {
            put(
                buf,
                area.x,
                area.y,
                area.width,
                self.empty,
                self.theme.style(Token::TextMuted),
            );
            return hits;
        }
        let mut y = area.y;
        let mut last_group = None;
        for (index, item) in self.items.iter().enumerate() {
            if y >= area.bottom() {
                break;
            }
            if item.group != last_group
                && let Some(group) = item.group
            {
                put(
                    buf,
                    area.x,
                    y,
                    area.width,
                    group,
                    self.theme.style(Token::TextMuted),
                );
                last_group = item.group;
                y = y.saturating_add(1);
                if y >= area.bottom() {
                    break;
                }
            }
            let selected = index == self.selected;
            let row = Rect::new(area.x, y, area.width, 1);
            if selected {
                fill(
                    buf,
                    row,
                    self.theme.style(Token::SurfaceSelected),
                    Some(' '),
                );
            }
            let caret = if selected {
                self.icons.glyph(Icon::SelectionCaret)
            } else {
                " "
            };
            let style = if selected {
                self.theme.style(Token::TextOnAccent)
            } else {
                self.theme.style(Token::TextPrimary)
            };
            let mut x = area.x;
            x = x.saturating_add(put(buf, x, y, area.width, caret, style));
            if x < area.right() {
                x = x.saturating_add(put(buf, x, y, 1, " ", style));
            }
            let meta_w = item
                .metadata
                .map(|meta| u16::try_from(meta.width()).unwrap_or(0))
                .unwrap_or(0);
            let label_w = area
                .right()
                .saturating_sub(x)
                .saturating_sub(meta_w.saturating_add(1));
            put(buf, x, y, label_w, &ellipsize(item.label, label_w), style);
            if let Some(meta) = item.metadata {
                put(
                    buf,
                    area.right().saturating_sub(meta_w),
                    y,
                    meta_w,
                    &right_align(meta, meta_w),
                    if selected {
                        style
                    } else {
                        self.theme.style(Token::TextMuted)
                    },
                );
            }
            hits.push((row, item.action.clone()));
            y = y.saturating_add(1);
        }
        hits
    }
}
