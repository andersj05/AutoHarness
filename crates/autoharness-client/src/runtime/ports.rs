use super::{
    CatalogProjection, MemoryProjection, ProfilesProjection, SessionProjection, SessionsProjection,
    SettingsProjection, UiIntent, UiNotice,
};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

/// Maximum queued user intents before explicit backpressure is presented.
pub const INTENT_CAPACITY: usize = 32;
/// Maximum queued application notices before their producer is backpressured.
pub const APP_NOTICE_CAPACITY: usize = 128;
/// Channel endpoints owned by the client carrier.
pub struct UiPorts {
    /// Bounded user-intent sender.
    pub intents: mpsc::Sender<UiIntent>,
    /// Latest session projection, coalesced by `watch`.
    pub sessions: watch::Receiver<Arc<SessionProjection>>,
    /// Latest all-sessions read model, coalesced by `watch`.
    pub session_lists: watch::Receiver<Arc<SessionsProjection>>,
    /// Latest catalog projection, coalesced by `watch`.
    pub catalogs: watch::Receiver<Arc<CatalogProjection>>,
    /// Latest local profile and provider-connection projection.
    pub profiles: watch::Receiver<Arc<ProfilesProjection>>,
    /// Latest resolved settings and provenance projection.
    pub settings: watch::Receiver<Arc<SettingsProjection>>,
    /// Latest bounded memory read model, coalesced by `watch`.
    pub memories: watch::Receiver<Arc<MemoryProjection>>,
    /// Bounded commit and rejection notices.
    pub notices: mpsc::Receiver<UiNotice>,
}

/// Channel endpoints owned by application composition.
pub struct AppPorts {
    /// Bounded user-intent receiver.
    pub intents: mpsc::Receiver<UiIntent>,
    /// Session projection publisher.
    pub sessions: watch::Sender<Arc<SessionProjection>>,
    /// All-sessions read-model publisher.
    pub session_lists: watch::Sender<Arc<SessionsProjection>>,
    /// Catalog projection publisher.
    pub catalogs: watch::Sender<Arc<CatalogProjection>>,
    /// Local profile and provider-connection publisher.
    pub profiles: watch::Sender<Arc<ProfilesProjection>>,
    /// Resolved settings and provenance publisher.
    pub settings: watch::Sender<Arc<SettingsProjection>>,
    /// Bounded memory read-model publisher.
    pub memories: watch::Sender<Arc<MemoryProjection>>,
    /// Bounded commit and rejection publisher.
    pub notices: mpsc::Sender<UiNotice>,
}

/// Creates bounded/coalescing client channels and their application endpoints.
#[must_use]
pub fn bounded_ports(
    session: Arc<SessionProjection>,
    session_list: Arc<SessionsProjection>,
    catalog: Arc<CatalogProjection>,
) -> (UiPorts, AppPorts) {
    let (intent_tx, intent_rx) = mpsc::channel(INTENT_CAPACITY);
    let (session_tx, session_rx) = watch::channel(session);
    let (session_list_tx, session_list_rx) = watch::channel(session_list);
    let (catalog_tx, catalog_rx) = watch::channel(catalog);
    let (profile_tx, profile_rx) = watch::channel(Arc::new(ProfilesProjection::default()));
    let (settings_tx, settings_rx) = watch::channel(Arc::new(SettingsProjection::default()));
    let (memory_tx, memory_rx) = watch::channel(Arc::new(MemoryProjection::default()));
    let (notice_tx, notice_rx) = mpsc::channel(APP_NOTICE_CAPACITY);
    (
        UiPorts {
            intents: intent_tx,
            sessions: session_rx,
            session_lists: session_list_rx,
            catalogs: catalog_rx,
            profiles: profile_rx,
            settings: settings_rx,
            memories: memory_rx,
            notices: notice_rx,
        },
        AppPorts {
            intents: intent_rx,
            sessions: session_tx,
            session_lists: session_list_tx,
            catalogs: catalog_tx,
            profiles: profile_tx,
            settings: settings_tx,
            memories: memory_tx,
            notices: notice_tx,
        },
    )
}
