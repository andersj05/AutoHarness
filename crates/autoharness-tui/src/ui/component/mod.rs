//! Presentation components. Pages compose these; they do not construct colors.

#![allow(dead_code)]

pub mod button_row;
pub mod callout;
pub mod chip;
pub mod hero;
pub mod key_value;
pub mod list_view;
pub mod message_block;
pub mod meter;
pub mod modal;
pub mod paint;
pub mod panel;
pub mod scrim;
pub mod search_field;
pub mod segmented;
pub mod setting_row;
pub mod status_bar;
pub mod tool_card;

pub use button_row::{Button, ButtonRow, ButtonVariant};
pub use callout::Callout;
pub use chip::{Chip, ChipVariant};
pub use hero::Hero;
pub use key_value::{KeyValue, KeyValueTable};
pub use list_view::{ListBadge, ListItem, ListView};
pub use message_block::MessageBlock;
pub use meter::{Meter, MeterThreshold};
pub use modal::{Modal, size as modal_size};
pub use panel::Panel;
pub use search_field::SearchField;
pub use segmented::SegmentedControl;
pub use setting_row::{Provenance, SettingKind, SettingRow};
pub use status_bar::{StatusBar, StatusSegment};
pub use tool_card::ToolCard;

#[cfg(test)]
mod tests;
