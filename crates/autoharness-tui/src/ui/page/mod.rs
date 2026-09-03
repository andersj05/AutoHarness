//! Route pages compose presentation components. They do not construct colors.

pub mod chat;
pub mod memory;

pub use chat::{
    collect_hits as chat_hits, content_hits as chat_content_hits,
    display_lines as chat_display_lines, rail_hits, render as render_chat, render_rail,
};
pub use memory::{hits as memory_hits, render as render_memory};
