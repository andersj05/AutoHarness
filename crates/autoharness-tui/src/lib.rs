//! Provider-neutral terminal client state, updates, rendering, and runner ports.

mod model;
mod runner;
mod text;
mod update;
mod view;

pub use model::{
    ApiCredential, AttemptKey, AttemptStatus, COMMANDS, CatalogProjection, CommandEntry,
    ComposerState, CredentialSourceLabel, Focus, Message, Model, ModelSummary, Notice, PendingKind,
    PermissionDetailView, PermissionRequestView, ProviderKindLabel, ProviderStatusProjection,
    RequestId, RetryPolicy, SessionBrowserEntry, SessionProjection, SessionsProjection,
    SettingsProjection, ToolCallKey, ToolRowView, TranscriptItem, TranscriptState, UiEffect,
    UiFailure, UiIntent, UiNotice, UsageView,
};
pub use runner::{
    APP_NOTICE_CAPACITY, AppPorts, ExitReason, INTENT_CAPACITY, RunnerError, UiPorts,
    bounded_ports, run,
};
pub use text::display_safe;
pub use update::update;
pub use view::view;
