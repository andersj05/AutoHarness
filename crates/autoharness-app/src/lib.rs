//! Application-owned composition helpers shared by the terminal binary
//! and integration tests.
//!
//! This library target intentionally exposes only composition-level
//! modules that have no terminal or orchestration state of their own.

pub mod profiles;
pub mod vault;
