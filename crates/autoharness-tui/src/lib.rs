//! Provider-neutral terminal client state, updates, rendering, and runner ports.

#[cfg(feature = "benchmark-instrumentation")]
pub mod benchmark;

mod model;
mod runner;
mod snapshot;
mod text;
mod time;
pub mod ui;
mod update;
mod view;

pub use model::{
    ApiCredential, AttemptKey, AttemptStatus, COMMANDS, CatalogProjection, CommandEntry,
    ComposerState, CredentialSourceLabel, Focus, LocalPreferenceChange, LocalUserProfileProjection,
    MemoryAdmission, MemoryDetail, MemoryLoadState, MemoryPane, MemoryProjection, MemoryScope,
    MemoryScopeFilter, MemoryStatus, MemoryStatusFilter, MemorySummary, MemoryTrust, Message,
    Model, ModelSummary, MouseAction, Notice, OverlayKind, PendingKind, PermissionDetailView,
    PermissionRequestView, ProfileConnectionState, ProfileCredentialStateLabel, ProfilesProjection,
    ProviderKindLabel, ProviderProfileDraft, ProviderProfileProjection, ProviderStatusProjection,
    RequestId, RetryPolicy, Route, SessionBrowserEntry, SessionProjection, SessionsProjection,
    SettingsProjection, ToolCallKey, ToolRowView, TranscriptItem, TranscriptState, UiClock,
    UiEffect, UiFailure, UiIntent, UiNotice, UsageView,
};
pub use runner::{
    APP_NOTICE_CAPACITY, AppPorts, ExitReason, INTENT_CAPACITY, RunnerError, UiPorts,
    bounded_ports, run,
};
pub use snapshot::style_snapshot;
pub use text::display_safe;
pub use time::{AgeBucket, RelativeAge, age_bucket, format_relative_age, relative_age};
pub use ui::{ColorDepth, Gradient, Icon, IconSet, Motion, Theme, Token};
pub use update::update;
pub use view::{hit_test, view};
