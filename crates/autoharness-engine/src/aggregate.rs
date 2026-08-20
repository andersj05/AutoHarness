use std::collections::BTreeSet;

use autoharness_domain::{
    Causation, CommandEnvelope, CommandId, CommandPayload, DeliveryMode, EVENT_SCHEMA_V1,
    EventEnvelope, EventId, EventPayload, InputId, ModelRef, PromptText, SessionId,
    SessionSequence,
};

use crate::{CommandRejection, ReplayError};

/// One durable user input in the visible session projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedInput {
    input_id: InputId,
    prompt: PromptText,
    delivery_mode: DeliveryMode,
}

impl AdmittedInput {
    /// Returns the stable input identity.
    #[must_use]
    pub const fn input_id(&self) -> &InputId {
        &self.input_id
    }

    /// Returns the exact admitted content.
    #[must_use]
    pub const fn prompt(&self) -> &PromptText {
        &self.prompt
    }

    /// Returns the provider-turn eligibility rule.
    #[must_use]
    pub const fn delivery_mode(&self) -> DeliveryMode {
        self.delivery_mode
    }
}

/// Session state derived exclusively from its ordered event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionAggregate {
    session_id: SessionId,
    is_created: bool,
    selected_model: Option<ModelRef>,
    admitted_inputs: Vec<AdmittedInput>,
    admitted_input_ids: BTreeSet<InputId>,
    applied_event_ids: BTreeSet<EventId>,
    applied_command_ids: BTreeSet<CommandId>,
    last_sequence: Option<SessionSequence>,
}

impl SessionAggregate {
    /// Creates an uninitialized aggregate that accepts only `SessionCreated`.
    #[must_use]
    pub fn empty(session_id: SessionId) -> Self {
        Self {
            session_id,
            is_created: false,
            selected_model: None,
            admitted_inputs: Vec::new(),
            admitted_input_ids: BTreeSet::new(),
            applied_event_ids: BTreeSet::new(),
            applied_command_ids: BTreeSet::new(),
            last_sequence: None,
        }
    }

    /// Produces event payloads for a valid command without mutating state.
    pub fn decide(&self, command: &CommandEnvelope) -> Result<Vec<EventPayload>, CommandRejection> {
        if command.session_id() != &self.session_id {
            return Err(CommandRejection::WrongSession {
                expected: self.session_id.clone(),
                found: command.session_id().clone(),
            });
        }

        let payload = match command.payload() {
            CommandPayload::CreateSession { .. } => {
                if self.is_created {
                    return Err(CommandRejection::SessionAlreadyExists {
                        session_id: self.session_id.clone(),
                    });
                }
                EventPayload::SessionCreated
            }
            CommandPayload::SelectModel { model, .. } => {
                self.require_created()?;
                EventPayload::ModelSelected {
                    model: model.clone(),
                }
            }
            CommandPayload::AdmitPrompt {
                input_id,
                prompt,
                delivery_mode,
                ..
            } => {
                self.require_created()?;
                if self.admitted_input_ids.contains(input_id) {
                    return Err(CommandRejection::DuplicateInput {
                        session_id: self.session_id.clone(),
                        input_id: input_id.clone(),
                    });
                }
                EventPayload::InputAdmitted {
                    input_id: input_id.clone(),
                    prompt: prompt.clone(),
                    delivery_mode: *delivery_mode,
                }
            }
        };

        Ok(vec![payload])
    }

    /// Applies a complete event batch atomically after replay validation.
    pub fn apply_batch(&mut self, events: &[EventEnvelope]) -> Result<(), ReplayError> {
        let mut candidate = self.clone();
        candidate.apply_uncommitted_batch(events)?;
        *self = candidate;
        Ok(())
    }

    /// Reconstructs a session from events in their supplied durable order.
    pub fn rehydrate<'a>(
        session_id: SessionId,
        events: impl IntoIterator<Item = &'a EventEnvelope>,
    ) -> Result<Self, ReplayError> {
        let mut aggregate = Self::empty(session_id);
        for event in events {
            aggregate.apply_one(event)?;
        }
        Ok(aggregate)
    }

    /// Returns the stable session identity.
    #[must_use]
    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns whether the creation event has been applied.
    #[must_use]
    pub const fn is_created(&self) -> bool {
        self.is_created
    }

    /// Returns the latest selected model, if any.
    #[must_use]
    pub const fn selected_model(&self) -> Option<&ModelRef> {
        self.selected_model.as_ref()
    }

    /// Returns admitted inputs in event order.
    #[must_use]
    pub fn admitted_inputs(&self) -> &[AdmittedInput] {
        &self.admitted_inputs
    }

    /// Returns the last applied event sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<SessionSequence> {
        self.last_sequence
    }

    fn require_created(&self) -> Result<(), CommandRejection> {
        if self.is_created {
            Ok(())
        } else {
            Err(CommandRejection::SessionNotFound {
                session_id: self.session_id.clone(),
            })
        }
    }

    pub(crate) fn apply_uncommitted_batch(
        &mut self,
        events: &[EventEnvelope],
    ) -> Result<(), ReplayError> {
        for event in events {
            self.apply_one(event)?;
        }
        Ok(())
    }

    fn apply_one(&mut self, event: &EventEnvelope) -> Result<(), ReplayError> {
        self.validate_envelope(event)?;

        match event.payload() {
            EventPayload::SessionCreated => {
                if self.is_created {
                    return Err(ReplayError::SessionAlreadyCreated {
                        session_id: self.session_id.clone(),
                        event_id: event.event_id().clone(),
                    });
                }
                self.is_created = true;
            }
            EventPayload::ModelSelected { model } => {
                self.require_created_for_replay(event)?;
                self.selected_model = Some(model.clone());
            }
            EventPayload::InputAdmitted {
                input_id,
                prompt,
                delivery_mode,
            } => {
                self.require_created_for_replay(event)?;
                if !self.admitted_input_ids.insert(input_id.clone()) {
                    return Err(ReplayError::DuplicateInput {
                        session_id: self.session_id.clone(),
                        input_id: input_id.clone(),
                        event_id: event.event_id().clone(),
                    });
                }
                self.admitted_inputs.push(AdmittedInput {
                    input_id: input_id.clone(),
                    prompt: prompt.clone(),
                    delivery_mode: *delivery_mode,
                });
            }
        }

        self.applied_event_ids.insert(event.event_id().clone());
        if let Causation::Command(command_id) = event.causation() {
            self.applied_command_ids.insert(command_id.clone());
        }
        self.last_sequence = Some(event.sequence());
        Ok(())
    }

    fn validate_envelope(&self, event: &EventEnvelope) -> Result<(), ReplayError> {
        if event.schema_version() != EVENT_SCHEMA_V1 {
            return Err(ReplayError::UnsupportedSchema {
                event_id: event.event_id().clone(),
                found: event.schema_version(),
            });
        }
        if event.session_id() != &self.session_id {
            return Err(ReplayError::WrongSession {
                expected: self.session_id.clone(),
                found: event.session_id().clone(),
                event_id: event.event_id().clone(),
            });
        }
        if self.applied_event_ids.contains(event.event_id()) {
            return Err(ReplayError::DuplicateEventId {
                event_id: event.event_id().clone(),
            });
        }
        if let Causation::Command(command_id) = event.causation()
            && self.applied_command_ids.contains(command_id)
        {
            return Err(ReplayError::DuplicateCommandCausation {
                event_id: event.event_id().clone(),
                command_id: command_id.clone(),
            });
        }
        if let Causation::Event(cause_event_id) = event.causation()
            && !self.applied_event_ids.contains(cause_event_id)
        {
            return Err(ReplayError::UnknownCausation {
                event_id: event.event_id().clone(),
                cause_event_id: cause_event_id.clone(),
            });
        }

        let expected = match self.last_sequence {
            Some(sequence) => {
                sequence
                    .checked_next()
                    .ok_or_else(|| ReplayError::SequenceExhausted {
                        session_id: self.session_id.clone(),
                    })?
            }
            None => SessionSequence::FIRST,
        };
        if event.sequence() != expected {
            return Err(ReplayError::NonContiguousSequence {
                session_id: self.session_id.clone(),
                expected: expected.get(),
                found: event.sequence().get(),
                event_id: event.event_id().clone(),
            });
        }

        Ok(())
    }

    fn require_created_for_replay(&self, event: &EventEnvelope) -> Result<(), ReplayError> {
        if self.is_created {
            Ok(())
        } else {
            Err(ReplayError::SessionNotCreated {
                session_id: self.session_id.clone(),
                event_id: event.event_id().clone(),
            })
        }
    }
}
