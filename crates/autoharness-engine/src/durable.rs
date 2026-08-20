use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    ClassifiedError, CommandEnvelope, ErrorClass, EventEnvelope, RetryAdvice, SessionId,
};
use autoharness_store::{AppendRequest, DEFAULT_EVENT_PAGE_SIZE, SessionStore, StoreError};

use crate::{EngineError, EventMetadataSource, InMemoryEngine, ReplayError, SessionAggregate};

/// Failure while recovering or durably executing a session command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DurableEngineError {
    /// Command preparation failed before a durable append.
    Engine(EngineError),
    /// The durable store rejected or failed an operation.
    Store(StoreError),
    /// Authoritative events failed closed replay validation.
    Replay(ReplayError),
    /// Store pagination or append receipt contradicted the authoritative stream.
    StoreInvariant,
}

impl Display for DurableEngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(source) => Display::fmt(source, formatter),
            Self::Store(source) => Display::fmt(source, formatter),
            Self::Replay(_) => {
                formatter.write_str("stored session history failed replay validation")
            }
            Self::StoreInvariant => {
                formatter.write_str("durable store returned an inconsistent session version")
            }
        }
    }
}

impl Error for DurableEngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Engine(source) => Some(source),
            Self::Store(source) => Some(source),
            Self::Replay(source) => Some(source),
            Self::StoreInvariant => None,
        }
    }
}

impl ClassifiedError for DurableEngineError {
    fn class(&self) -> ErrorClass {
        match self {
            Self::Engine(source) => source.class(),
            Self::Store(source) => source.class(),
            Self::Replay(source) => source.class(),
            Self::StoreInvariant => ErrorClass::Storage,
        }
    }

    fn retry_advice(&self) -> RetryAdvice {
        match self {
            Self::Engine(source) => source.retry_advice(),
            Self::Store(source) => source.retry_advice(),
            Self::Replay(source) => source.retry_advice(),
            Self::StoreInvariant => RetryAdvice::Never,
        }
    }
}

impl From<EngineError> for DurableEngineError {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}

impl From<StoreError> for DurableEngineError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ReplayError> for DurableEngineError {
    fn from(value: ReplayError) -> Self {
        Self::Replay(value)
    }
}

/// Headless engine that commits authoritative events before publishing state.
#[derive(Debug)]
pub struct DurableEngine<S, M> {
    store: S,
    inner: InMemoryEngine<M>,
}

impl<S, M> DurableEngine<S, M>
where
    S: SessionStore,
    M: EventMetadataSource,
{
    /// Creates an empty durable engine over an already opened store.
    #[must_use]
    pub fn new(store: S, metadata_source: M) -> Self {
        Self {
            store,
            inner: InMemoryEngine::new(metadata_source),
        }
    }

    /// Replays every stored session before accepting commands.
    pub fn recover(store: S, metadata_source: M) -> Result<Self, DurableEngineError> {
        let summaries = store.list_sessions()?;
        let mut events = Vec::new();

        for summary in summaries {
            let mut after = 0_u64;
            let expected_last = summary.last_sequence().get();
            while after < expected_last {
                let page =
                    store.load_events(summary.session_id(), after, DEFAULT_EVENT_PAGE_SIZE)?;
                if page.is_empty() {
                    return Err(DurableEngineError::StoreInvariant);
                }
                after = page
                    .last()
                    .map(EventEnvelope::sequence)
                    .map_or(after, |sequence| sequence.get());
                events.extend(page);
            }
            if after != expected_last {
                return Err(DurableEngineError::StoreInvariant);
            }
        }

        let inner = InMemoryEngine::replay(metadata_source, events)?;
        Ok(Self { store, inner })
    }

    /// Validates, appends, then publishes one command's complete event batch.
    pub fn execute(
        &mut self,
        command: &CommandEnvelope,
    ) -> Result<Vec<EventEnvelope>, DurableEngineError> {
        let prepared = self.inner.prepare(command)?;
        let expected = prepared.expected_last_sequence();
        let event_count = u64::try_from(prepared.events().len())
            .map_err(|_| DurableEngineError::StoreInvariant)?;
        let expected_receipt = expected
            .checked_add(event_count)
            .ok_or(DurableEngineError::StoreInvariant)?;
        let request = AppendRequest::new(
            prepared.session_id().clone(),
            expected,
            prepared.events().to_vec(),
        );
        let receipt = self.store.append(&request)?;
        if receipt.last_sequence() != expected_receipt {
            return Err(DurableEngineError::StoreInvariant);
        }

        let events = prepared.events().to_vec();
        self.inner.commit_prepared(prepared);
        Ok(events)
    }

    /// Returns one replay-derived session projection.
    #[must_use]
    pub fn session(&self, session_id: &SessionId) -> Option<&SessionAggregate> {
        self.inner.session(session_id)
    }

    /// Returns authoritative events in the order loaded or committed locally.
    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        self.inner.events()
    }

    /// Returns the store for read-model queries owned by application composition.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// Returns mutable store access for explicit maintenance operations.
    #[must_use]
    pub const fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Returns the owned store and in-memory replay engine.
    #[must_use]
    pub fn into_parts(self) -> (S, InMemoryEngine<M>) {
        (self.store, self.inner)
    }
}
