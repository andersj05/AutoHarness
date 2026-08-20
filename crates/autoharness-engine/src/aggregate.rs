use std::collections::{BTreeMap, BTreeSet};

use autoharness_domain::{
    AttemptFailure, AttemptId, Causation, CommandEnvelope, CommandId, CommandPayload, DeliveryMode,
    EVENT_SCHEMA_V1, EventEnvelope, EventId, EventPayload, InputId, ModelRef, PromptText,
    ResponseText, RetryAdvice, SessionId, SessionSequence, UsageSnapshot,
};

use crate::{CommandRejection, ReplayError};

/// One durable user input in the visible session projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedInput {
    input_id: InputId,
    prompt: PromptText,
    delivery_mode: DeliveryMode,
    promoted_by: Option<AttemptId>,
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

    /// Returns the first attempt that promoted this input.
    #[must_use]
    pub const fn promoted_by(&self) -> Option<&AttemptId> {
        self.promoted_by.as_ref()
    }
}

/// Provider-attempt lifecycle derived from durable events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptStatus {
    /// Input and model are durable, but provider dispatch has not begun.
    Prepared,
    /// The provider request was dispatched and may be producing output.
    InFlight,
    /// Cancellation is durable and the provider task is being stopped.
    CancellationRequested,
    /// The provider interaction completed successfully.
    Completed,
    /// The provider interaction settled with a safe failure.
    Failed,
    /// The provider task observed cooperative cancellation.
    Cancelled,
    /// Recovery cannot determine the provider-side outcome.
    Unknown,
}

impl AttemptStatus {
    /// Returns whether no further provider lifecycle events may be applied.
    #[must_use]
    pub const fn is_settled(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}

/// One provider attempt reconstructed exclusively from ordered session events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptProjection {
    attempt_id: AttemptId,
    input_id: InputId,
    model: ModelRef,
    retry_of: Option<AttemptId>,
    status: AttemptStatus,
    text: Vec<ResponseText>,
    usage: Option<UsageSnapshot>,
    failure: Option<AttemptFailure>,
}

impl AttemptProjection {
    /// Returns the stable attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }

    /// Returns the exact input bound to this attempt.
    #[must_use]
    pub const fn input_id(&self) -> &InputId {
        &self.input_id
    }

    /// Returns the provider-neutral model snapshot.
    #[must_use]
    pub const fn model(&self) -> &ModelRef {
        &self.model
    }

    /// Returns the prior attempt when this is an explicit retry.
    #[must_use]
    pub const fn retry_of(&self) -> Option<&AttemptId> {
        self.retry_of.as_ref()
    }

    /// Returns current durable lifecycle state.
    #[must_use]
    pub const fn status(&self) -> AttemptStatus {
        self.status
    }

    /// Returns response deltas in durable event order.
    #[must_use]
    pub fn text_deltas(&self) -> &[ResponseText] {
        &self.text
    }

    /// Concatenates exact durable response deltas for display or provider context.
    #[must_use]
    pub fn response_text(&self) -> String {
        let capacity = self.text.iter().map(|delta| delta.as_str().len()).sum();
        let mut output = String::with_capacity(capacity);
        for delta in &self.text {
            output.push_str(delta.as_str());
        }
        output
    }

    /// Returns the newest cumulative usage snapshot.
    #[must_use]
    pub const fn usage(&self) -> Option<UsageSnapshot> {
        self.usage
    }

    /// Returns the safe terminal failure, when present.
    #[must_use]
    pub const fn failure(&self) -> Option<&AttemptFailure> {
        self.failure.as_ref()
    }

    fn can_retry(&self) -> bool {
        match self.status {
            AttemptStatus::Failed => self
                .failure
                .as_ref()
                .is_some_and(|failure| failure.retry_advice() != RetryAdvice::Never),
            AttemptStatus::Cancelled | AttemptStatus::Unknown => true,
            AttemptStatus::Prepared
            | AttemptStatus::InFlight
            | AttemptStatus::CancellationRequested
            | AttemptStatus::Completed => false,
        }
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
    attempts: Vec<AttemptProjection>,
    attempt_indexes: BTreeMap<AttemptId, usize>,
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
            attempts: Vec::new(),
            attempt_indexes: BTreeMap::new(),
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
            CommandPayload::AdmitPromptAndPrepareAttempt {
                input_id,
                prompt,
                delivery_mode,
                attempt_id,
                ..
            } => {
                self.require_created()?;
                if self.admitted_input_ids.contains(input_id) {
                    return Err(CommandRejection::DuplicateInput {
                        session_id: self.session_id.clone(),
                        input_id: input_id.clone(),
                    });
                }
                if self.attempt_indexes.contains_key(attempt_id) {
                    return Err(CommandRejection::DuplicateAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id: attempt_id.clone(),
                    });
                }
                let model = self.selected_model.clone().ok_or_else(|| {
                    CommandRejection::ModelNotSelected {
                        session_id: self.session_id.clone(),
                    }
                })?;
                return Ok(vec![
                    EventPayload::InputAdmitted {
                        input_id: input_id.clone(),
                        prompt: prompt.clone(),
                        delivery_mode: *delivery_mode,
                    },
                    EventPayload::AttemptPrepared {
                        attempt_id: attempt_id.clone(),
                        input_id: input_id.clone(),
                        model,
                        retry_of: None,
                    },
                ]);
            }
            CommandPayload::PrepareAttempt {
                attempt_id,
                input_id,
                retry_of,
                ..
            } => {
                self.require_created()?;
                if self.attempt_indexes.contains_key(attempt_id) {
                    return Err(CommandRejection::DuplicateAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id: attempt_id.clone(),
                    });
                }
                let input =
                    self.input(input_id)
                        .ok_or_else(|| CommandRejection::InputNotFound {
                            session_id: self.session_id.clone(),
                            input_id: input_id.clone(),
                        })?;
                let model = if let Some(prior_id) = retry_of {
                    let prior = self.attempt(prior_id).ok_or_else(|| {
                        CommandRejection::AttemptNotFound {
                            session_id: self.session_id.clone(),
                            attempt_id: prior_id.clone(),
                        }
                    })?;
                    if !prior.can_retry() {
                        return Err(CommandRejection::RetryNotAllowed {
                            session_id: self.session_id.clone(),
                            attempt_id: prior_id.clone(),
                        });
                    }
                    if prior.input_id() != input_id {
                        return Err(CommandRejection::RetryInputMismatch {
                            session_id: self.session_id.clone(),
                            attempt_id: prior_id.clone(),
                            input_id: input_id.clone(),
                        });
                    }
                    prior.model.clone()
                } else {
                    if input.promoted_by.is_some() {
                        return Err(CommandRejection::InputAlreadyPromoted {
                            session_id: self.session_id.clone(),
                            input_id: input_id.clone(),
                        });
                    }
                    self.selected_model.clone().ok_or_else(|| {
                        CommandRejection::ModelNotSelected {
                            session_id: self.session_id.clone(),
                        }
                    })?
                };

                EventPayload::AttemptPrepared {
                    attempt_id: attempt_id.clone(),
                    input_id: input_id.clone(),
                    model,
                    retry_of: retry_of.clone(),
                }
            }
            CommandPayload::StartAttempt { attempt_id, .. } => {
                self.require_attempt_status(attempt_id, &[AttemptStatus::Prepared])?;
                EventPayload::AttemptStarted {
                    attempt_id: attempt_id.clone(),
                }
            }
            CommandPayload::AppendAttemptText {
                attempt_id, text, ..
            } => {
                self.require_attempt_status(
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                )?;
                EventPayload::AttemptTextAppended {
                    attempt_id: attempt_id.clone(),
                    text: text.clone(),
                }
            }
            CommandPayload::RecordAttemptUsage {
                attempt_id, usage, ..
            } => {
                self.require_attempt_status(
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                )?;
                EventPayload::AttemptUsageRecorded {
                    attempt_id: attempt_id.clone(),
                    usage: *usage,
                }
            }
            CommandPayload::RequestAttemptCancellation { attempt_id, .. } => {
                self.require_attempt_status(attempt_id, &[AttemptStatus::InFlight])?;
                EventPayload::AttemptCancellationRequested {
                    attempt_id: attempt_id.clone(),
                }
            }
            CommandPayload::CompleteAttempt { attempt_id, .. } => {
                self.require_attempt_status(
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                )?;
                EventPayload::AttemptCompleted {
                    attempt_id: attempt_id.clone(),
                }
            }
            CommandPayload::FailAttempt {
                attempt_id,
                failure,
                ..
            } => {
                self.require_attempt_status(
                    attempt_id,
                    &[
                        AttemptStatus::Prepared,
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                )?;
                EventPayload::AttemptFailed {
                    attempt_id: attempt_id.clone(),
                    failure: failure.clone(),
                }
            }
            CommandPayload::CancelAttempt { attempt_id, .. } => {
                self.require_attempt_status(attempt_id, &[AttemptStatus::CancellationRequested])?;
                EventPayload::AttemptCancelled {
                    attempt_id: attempt_id.clone(),
                }
            }
            CommandPayload::MarkAttemptUnknown { attempt_id, .. } => {
                self.require_attempt_status(
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                )?;
                EventPayload::AttemptMarkedUnknown {
                    attempt_id: attempt_id.clone(),
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

    /// Returns attempts in preparation-event order.
    #[must_use]
    pub fn attempts(&self) -> &[AttemptProjection] {
        &self.attempts
    }

    /// Returns one attempt projection by stable identity.
    #[must_use]
    pub fn attempt(&self, attempt_id: &AttemptId) -> Option<&AttemptProjection> {
        self.attempt_indexes
            .get(attempt_id)
            .and_then(|index| self.attempts.get(*index))
    }

    /// Returns the last applied event sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<SessionSequence> {
        self.last_sequence
    }

    fn input(&self, input_id: &InputId) -> Option<&AdmittedInput> {
        self.admitted_inputs
            .iter()
            .find(|input| input.input_id() == input_id)
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

    fn require_attempt_status(
        &self,
        attempt_id: &AttemptId,
        allowed: &[AttemptStatus],
    ) -> Result<(), CommandRejection> {
        let attempt =
            self.attempt(attempt_id)
                .ok_or_else(|| CommandRejection::AttemptNotFound {
                    session_id: self.session_id.clone(),
                    attempt_id: attempt_id.clone(),
                })?;
        if allowed.contains(&attempt.status) {
            Ok(())
        } else {
            Err(CommandRejection::InvalidAttemptState {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
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
                    promoted_by: None,
                });
            }
            EventPayload::AttemptPrepared {
                attempt_id,
                input_id,
                model,
                retry_of,
            } => {
                self.require_created_for_replay(event)?;
                if self.attempt_indexes.contains_key(attempt_id) {
                    return Err(ReplayError::DuplicateAttempt {
                        session_id: self.session_id.clone(),
                        attempt_id: attempt_id.clone(),
                        event_id: event.event_id().clone(),
                    });
                }
                let input_index = self
                    .admitted_inputs
                    .iter()
                    .position(|input| input.input_id() == input_id)
                    .ok_or_else(|| ReplayError::UnknownInput {
                        session_id: self.session_id.clone(),
                        input_id: input_id.clone(),
                        event_id: event.event_id().clone(),
                    })?;

                if let Some(prior_id) = retry_of {
                    let prior =
                        self.attempt(prior_id)
                            .ok_or_else(|| ReplayError::InvalidRetry {
                                session_id: self.session_id.clone(),
                                attempt_id: attempt_id.clone(),
                                event_id: event.event_id().clone(),
                            })?;
                    if !prior.can_retry() || prior.input_id() != input_id || prior.model() != model
                    {
                        return Err(ReplayError::InvalidRetry {
                            session_id: self.session_id.clone(),
                            attempt_id: attempt_id.clone(),
                            event_id: event.event_id().clone(),
                        });
                    }
                } else {
                    if self.admitted_inputs[input_index].promoted_by.is_some() {
                        return Err(ReplayError::InputAlreadyPromoted {
                            session_id: self.session_id.clone(),
                            input_id: input_id.clone(),
                            event_id: event.event_id().clone(),
                        });
                    }
                    if self.selected_model.as_ref() != Some(model) {
                        return Err(ReplayError::ModelSnapshotMismatch {
                            session_id: self.session_id.clone(),
                            attempt_id: attempt_id.clone(),
                            event_id: event.event_id().clone(),
                        });
                    }
                    self.admitted_inputs[input_index].promoted_by = Some(attempt_id.clone());
                }

                let index = self.attempts.len();
                self.attempts.push(AttemptProjection {
                    attempt_id: attempt_id.clone(),
                    input_id: input_id.clone(),
                    model: model.clone(),
                    retry_of: retry_of.clone(),
                    status: AttemptStatus::Prepared,
                    text: Vec::new(),
                    usage: None,
                    failure: None,
                });
                self.attempt_indexes.insert(attempt_id.clone(), index);
            }
            EventPayload::AttemptStarted { attempt_id } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[AttemptStatus::Prepared],
                    |attempt| {
                        attempt.status = AttemptStatus::InFlight;
                    },
                )?;
            }
            EventPayload::AttemptTextAppended { attempt_id, text } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                    |attempt| attempt.text.push(text.clone()),
                )?;
            }
            EventPayload::AttemptUsageRecorded { attempt_id, usage } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                    |attempt| attempt.usage = Some(*usage),
                )?;
            }
            EventPayload::AttemptCancellationRequested { attempt_id } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[AttemptStatus::InFlight],
                    |attempt| {
                        attempt.status = AttemptStatus::CancellationRequested;
                    },
                )?;
            }
            EventPayload::AttemptCompleted { attempt_id } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                    |attempt| attempt.status = AttemptStatus::Completed,
                )?;
            }
            EventPayload::AttemptFailed {
                attempt_id,
                failure,
            } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[
                        AttemptStatus::Prepared,
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                    |attempt| {
                        attempt.status = AttemptStatus::Failed;
                        attempt.failure = Some(failure.clone());
                    },
                )?;
            }
            EventPayload::AttemptCancelled { attempt_id } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[AttemptStatus::CancellationRequested],
                    |attempt| attempt.status = AttemptStatus::Cancelled,
                )?;
            }
            EventPayload::AttemptMarkedUnknown { attempt_id } => {
                self.transition_attempt(
                    event,
                    attempt_id,
                    &[
                        AttemptStatus::InFlight,
                        AttemptStatus::CancellationRequested,
                    ],
                    |attempt| attempt.status = AttemptStatus::Unknown,
                )?;
            }
        }

        self.applied_event_ids.insert(event.event_id().clone());
        if let Causation::Command(command_id) = event.causation() {
            self.applied_command_ids.insert(command_id.clone());
        }
        self.last_sequence = Some(event.sequence());
        Ok(())
    }

    fn transition_attempt(
        &mut self,
        event: &EventEnvelope,
        attempt_id: &AttemptId,
        allowed: &[AttemptStatus],
        transition: impl FnOnce(&mut AttemptProjection),
    ) -> Result<(), ReplayError> {
        let index = self
            .attempt_indexes
            .get(attempt_id)
            .copied()
            .ok_or_else(|| ReplayError::UnknownAttempt {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                event_id: event.event_id().clone(),
            })?;
        let attempt = &mut self.attempts[index];
        if !allowed.contains(&attempt.status) {
            return Err(ReplayError::IllegalAttemptTransition {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                event_id: event.event_id().clone(),
            });
        }
        transition(attempt);
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
