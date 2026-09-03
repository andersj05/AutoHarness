//! Renderer-neutral, versioned public client contract for AutoHarness.
//!
//! This crate carries public data and requested intent only.
//! Durable authority, secrets, provider clients, storage, tools, and permissions remain in Rust.

#![forbid(unsafe_code)]

mod bounds;
mod command;
mod content;
mod delta;
mod error;
mod failure;
mod frame;
mod id;
mod memory;
mod profile;
mod projection;
mod settings;

pub use bounds::*;
pub use command::*;
pub use content::*;
pub use delta::*;
pub use error::*;
pub use failure::*;
pub use frame::*;
pub use id::*;
pub use memory::*;
pub use profile::*;
pub use projection::*;
pub use settings::*;

/// Exact schema version implemented by this client contract.
pub const CLIENT_SCHEMA_VERSION: u16 = 4;
