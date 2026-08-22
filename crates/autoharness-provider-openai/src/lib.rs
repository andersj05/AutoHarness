//! Configurable OpenAI-compatible model-router adapter.
//!
//! The adapter keeps router-native model and chat-completions payloads at the
//! provider edge and emits only provider-neutral catalog and lifecycle values.

mod auth;
mod client;
mod models;
mod native_stream;
mod settings;
#[cfg(test)]
mod test_http;

pub use auth::{ROUTER_API_KEY_ENV, RouterCredential};
pub use client::OpenAiRouterProvider;
pub use reqwest::Url as RouterUrl;
pub use settings::{
    ROUTER_AUTH_HEADER_ENV, ROUTER_AUTH_SCHEME_ENV, ROUTER_BASE_URL_ENV, ROUTER_CHAT_PATH_ENV,
    ROUTER_MODELS_PATH_ENV, ROUTER_PROJECT_ENV, RouterSettings,
};
