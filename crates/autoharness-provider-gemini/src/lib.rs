//! Google AI Studio adapter.
//!
//! Model discovery uses the paginated `v1beta/models` resource because that is
//! the stable discovery contract. Chat defaults to the stable `v1/interactions`
//! SSE transport in stateless `store: false` mode. A Generate Content fallback
//! is permitted only when Interactions rejects the request before streaming as
//! unsupported or model-not-found.

mod auth;
mod client;
mod models;
mod native_stream;
mod sse;
#[cfg(test)]
mod test_http;

pub use auth::{GEMINI_API_KEY_ENV, GeminiApiKey};
pub use client::GeminiProvider;
