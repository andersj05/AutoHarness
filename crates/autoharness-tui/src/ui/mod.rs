//! Typed terminal presentation layer.

pub mod color;
pub mod component;
pub mod gradient;
pub mod icon;
pub mod layout;
pub mod metrics;
pub mod motion;
pub mod page;
pub mod palette;
pub mod theme;
pub mod tokens;

pub use color::ColorDepth;
pub use gradient::{Gradient, normalized_t};
pub use icon::{Icon, IconSet};
pub use motion::Motion;
pub use theme::Theme;
pub use tokens::Token;
