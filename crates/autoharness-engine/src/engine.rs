use std::collections::{BTreeMap, BTreeSet};

use autoharness_domain::{
    Causation, CommandEnvelope, CommandId, EventEnvelope, EventId, SessionId, SessionSequence,
    TimestampMillis,
};

use crate::{CommandRejection, EngineError, ReplayError, SessionAggregate};

/// Event identity and observation time supplied by application composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedEventMetadata {
    event_id: EventId,
    occurred_at: TimestampMillis,
}

impl GeneratedEventMetadata {
    /// Constructs generated metadata.
    #[must_use]
    pub const fn new(event_id: EventId, occurred_at: TimestampMillis) -> Self {
        Self {
            event_id,
            occurred_at,
        }
    }
}

/// Supplies event identity and time without coupling replay to a clock or ID library.
pub trait EventMetadataSource {
    /// Returns metadata for the next accepted event.
    fn next_event_metadata(&mut self) -> GeneratedEventMetadata;
}

/// Synchronous headless harness used to prove command and replay semantics.
#[derive(Debug)]
pub struct InMemoryEngine<M> {
    metadata_source: M,
    sessions: BTreeMap<SessionId, SessionAggregate>,
    events: Vec<EventEnvelope>,
    event_ids: BTreeSet<EventId>,
    command_ids: BTreeSet<CommandId>,
}

impl<M> InMemoryEngine<M>
where
    M: EventMetadataSource,
{
    /// Creates an empty engine with an injected metadata source.
    #[must_use]
    pub fn new(metadata_source: M) -> Self {
        Self {
            metadata_source,
            sessions: BTreeMap::new(),
            events: Vec::new(),
            event_ids: BTreeSet::new(),
            command_ids: BTreeSet::new(),
        }
    }

    /// Replays events in supplied order and prepares the engine for later commands.
    pub fn replay(
        metadata_source: M,
        events: impl IntoIterator<Item = EventEnvelope>,
    ) -> Result<Self, ReplayError> {
        let mut engine = Self::new(metadata_source);
        for event in events {
            if engine.event_ids.contains(event.event_id()) {
                return Err(ReplayError::DuplicateEventId {
                    event_id: event.event_id().clone(),
                });
            }

            let session_id = event.session_id().clone();
            let aggregate = engine
                .sessions
                .entry(session_id.clone())
                .or_insert_with(|| SessionAggregate::empty(session_id));
            aggregate.apply_uncommitted_batch(std::slice::from_ref(&event))?;
            if let Causation::Command(command_id) = event.causation()
                && !engine.command_ids.insert(command_id.clone())
            {
                return Err(ReplayError::DuplicateCommandCausation {
                    event_id: event.event_id().clone(),
                    command_id: command_id.clone(),
                });
            }
            engine.event_ids.insert(event.event_id().clone());
            engine.events.push(event);
        }
        Ok(engine)
    }

    /// Validates a single-use command ID, commits its event batch, and returns that batch.
    pub fn execute(
        &mut self,
        command: &CommandEnvelope,
    ) -> Result<Vec<EventEnvelope>, EngineError> {
        if self.command_ids.contains(command.command_id()) {
            return Err(CommandRejection::DuplicateCommand {
                command_id: command.command_id().clone(),
            }
            .into());
        }
        let session_id = command.session_id().clone();
        let current = self
            .sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| SessionAggregate::empty(session_id.clone()));
        let payloads = current.decide(command)?;

        let mut next_sequence = match current.last_sequence() {
            Some(sequence) => {
                sequence
                    .checked_next()
                    .ok_or_else(|| EngineError::SequenceExhausted {
                        session_id: session_id.clone(),
                    })?
            }
            None => SessionSequence::FIRST,
        };
        let mut batch_event_ids = BTreeSet::new();
        let mut events = Vec::with_capacity(payloads.len());

        for (index, payload) in payloads.into_iter().enumerate() {
            if index > 0 {
                next_sequence =
                    next_sequence
                        .checked_next()
                        .ok_or_else(|| EngineError::SequenceExhausted {
                            session_id: session_id.clone(),
                        })?;
            }
            let generated = self.metadata_source.next_event_metadata();
            if self.event_ids.contains(&generated.event_id)
                || !batch_event_ids.insert(generated.event_id.clone())
            {
                return Err(EngineError::EventIdCollision {
                    event_id: generated.event_id,
                });
            }
            events.push(EventEnvelope::new_v1(
                generated.event_id,
                session_id.clone(),
                next_sequence,
                generated.occurred_at,
                Causation::Command(command.command_id().clone()),
                command.correlation_id().clone(),
                payload,
            ));
        }

        let mut candidate = current;
        candidate
            .apply_uncommitted_batch(&events)
            .map_err(|source| EngineError::InvariantViolation { source })?;

        self.sessions.insert(session_id, candidate);
        self.command_ids.insert(command.command_id().clone());
        self.event_ids.extend(batch_event_ids);
        self.events.extend(events.iter().cloned());
        Ok(events)
    }

    /// Returns one session projection reconstructed from emitted events.
    #[must_use]
    pub fn session(&self, session_id: &SessionId) -> Option<&SessionAggregate> {
        self.sessions.get(session_id)
    }

    /// Returns the append order across all in-memory sessions.
    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        &self.events
    }
}
