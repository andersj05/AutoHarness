use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    ClassifiedError, CommandEnvelope, ErrorClass, EventEnvelope, EventPayload, RetryAdvice,
    SessionId,
};
use autoharness_store::{
    AppendRequest, BoundContextTurnCommitRequest, ContextCompactionBoundary, ContextStore,
    ContextTurnCommitRequest, DEFAULT_EVENT_PAGE_SIZE, SessionStore, StoreError,
};

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

    /// Atomically commits a provider-turn context and its authoritative binding event.
    ///
    /// The command must prepare exactly one `ContextTurnBound` event matching the context
    /// manifest. The durable transaction completes before the binding becomes visible to the
    /// in-memory aggregate, so a following `StartRunTurn` command observes the exact committed
    /// sequence without a replay window.
    pub fn commit_context_turn_and_bind(
        &mut self,
        context: ContextTurnCommitRequest,
        command: &CommandEnvelope,
    ) -> Result<Vec<EventEnvelope>, DurableEngineError>
    where
        S: ContextStore,
    {
        self.commit_context_turn_and_bind_inner(context, None, command)
    }

    /// Atomically commits a verified compaction context and its authoritative binding event.
    ///
    /// The storage adapter validates that the explicit durable-facts boundary belongs to the
    /// compaction epoch in the context request. The engine retains sole ownership of constructing
    /// and publishing the adjacent session binding event.
    pub fn commit_compaction_context_turn_and_bind(
        &mut self,
        context: ContextTurnCommitRequest,
        boundary: ContextCompactionBoundary,
        command: &CommandEnvelope,
    ) -> Result<Vec<EventEnvelope>, DurableEngineError>
    where
        S: ContextStore,
    {
        self.commit_context_turn_and_bind_inner(context, Some(boundary), command)
    }

    fn commit_context_turn_and_bind_inner(
        &mut self,
        context: ContextTurnCommitRequest,
        compaction_boundary: Option<ContextCompactionBoundary>,
        command: &CommandEnvelope,
    ) -> Result<Vec<EventEnvelope>, DurableEngineError>
    where
        S: ContextStore,
    {
        let prepared = self.inner.prepare(command)?;
        let [binding_event] = prepared.events() else {
            return Err(DurableEngineError::StoreInvariant);
        };
        let EventPayload::ContextTurnBound {
            attempt_id,
            run_turn,
            context_turn_id,
            manifest_hash,
        } = binding_event.payload()
        else {
            return Err(DurableEngineError::StoreInvariant);
        };
        let turn = context.turn();
        if binding_event.session_id() != turn.session_id()
            || attempt_id != turn.attempt_id()
            || *run_turn != turn.run_turn()
            || context_turn_id != turn.context_turn_id()
            || manifest_hash != turn.manifest_hash()
        {
            return Err(DurableEngineError::StoreInvariant);
        }
        let expected_receipt = prepared
            .expected_last_sequence()
            .checked_add(1)
            .ok_or(DurableEngineError::StoreInvariant)?;
        if binding_event.sequence().get() != expected_receipt {
            return Err(DurableEngineError::StoreInvariant);
        }

        let mut request = BoundContextTurnCommitRequest::new(context, binding_event.clone());
        if let Some(boundary) = compaction_boundary {
            request = request.with_compaction_boundary(boundary);
        }
        let receipt = self.store.commit_context_turn_and_bind(&request)?;
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
