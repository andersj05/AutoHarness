use std::collections::{BTreeMap, BTreeSet};

use autoharness_domain::{
    AttemptId, Causation, CommandId, DeliveryMode, EVENT_SCHEMA_V1, EventEnvelope, EventId,
    EventPayload, InputId, MemoryRevisionStatus, PermissionAnswer, PermissionDecisionId,
    PermissionOutcome, PromptText, SessionId, SessionSequence, Sha256Digest, ToolCallId,
    ToolCallSpec,
};

use crate::{
    CanonicalEncoder, MemoryCandidate, MemoryError, RetrievalScope, normalized_content_hash,
};

/// Stable canonical contract for effective durable facts across compaction epochs.
pub const COMPACTION_FACTS_VERSION: u16 = 1;

/// Unsettled session state that compaction must never summarize away.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PendingSessionFactKind {
    /// An admitted input has not reached a final disposition.
    Input,
    /// A tool permission decision remains relevant to execution.
    Permission,
    /// A tool call has not reached an authoritative settled state.
    Tool,
}

/// Contentless hashes of one authoritative unsettled session fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSessionFact {
    kind: PendingSessionFactKind,
    identity_hash: Sha256Digest,
    state_hash: Sha256Digest,
}

/// Independently reproducible fingerprint and exact cardinalities for one compaction boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveDurableFactsFingerprint {
    hash: Sha256Digest,
    memory_fact_count: u32,
    pending_session_fact_count: u32,
}

impl EffectiveDurableFactsFingerprint {
    /// Returns the canonical effective-facts digest.
    #[must_use]
    pub const fn hash(&self) -> &Sha256Digest {
        &self.hash
    }

    /// Returns the number of eligible active retained memory facts in the digest.
    #[must_use]
    pub const fn memory_fact_count(&self) -> u32 {
        self.memory_fact_count
    }

    /// Returns the number of unsettled session facts in the digest.
    #[must_use]
    pub const fn pending_session_fact_count(&self) -> u32 {
        self.pending_session_fact_count
    }
}

#[derive(Clone, Debug)]
struct InputState {
    prompt: PromptText,
    delivery_mode: DeliveryMode,
    promoted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingToolStage {
    Proposed,
    PermissionPending,
    Authorized,
    DeniedPending,
    Running,
    Settled,
}

#[derive(Clone, Debug)]
struct PendingToolState {
    attempt_id: AttemptId,
    call: ToolCallSpec,
    stage: PendingToolStage,
    policy_decision: Option<(PermissionDecisionId, PermissionOutcome)>,
    human_answer: Option<(PermissionDecisionId, PermissionAnswer)>,
}

impl PendingSessionFact {
    /// Constructs one contentless fact from independently computed authoritative hashes.
    #[must_use]
    pub const fn new(
        kind: PendingSessionFactKind,
        identity_hash: Sha256Digest,
        state_hash: Sha256Digest,
    ) -> Self {
        Self {
            kind,
            identity_hash,
            state_hash,
        }
    }

    /// Returns the stable session-fact class.
    #[must_use]
    pub const fn kind(&self) -> PendingSessionFactKind {
        self.kind
    }

    /// Returns the hash of the fact's authoritative stable identity.
    #[must_use]
    pub const fn identity_hash(&self) -> &Sha256Digest {
        &self.identity_hash
    }

    /// Returns the hash of the complete authoritative state that must survive compaction.
    #[must_use]
    pub const fn state_hash(&self) -> &Sha256Digest {
        &self.state_hash
    }
}

/// Hashes every effective memory fact and unsettled session fact in stable logical order.
///
/// Retrieval-only signals such as lexical score and exact-query match are intentionally excluded.
/// The supplied retrieval scope determines eligibility at the exact compaction boundary.
pub fn effective_durable_facts_hash(
    scope: &RetrievalScope,
    candidates: &[MemoryCandidate],
    pending_session_facts: &[PendingSessionFact],
) -> Result<Sha256Digest, MemoryError> {
    Ok(effective_durable_facts(scope, candidates, pending_session_facts)?.hash)
}

/// Hashes and counts every effective fact under the stable compaction contract.
pub fn effective_durable_facts(
    scope: &RetrievalScope,
    candidates: &[MemoryCandidate],
    pending_session_facts: &[PendingSessionFact],
) -> Result<EffectiveDurableFactsFingerprint, MemoryError> {
    let mut memories = candidates
        .iter()
        .filter(|candidate| crate::rank::eligible(scope, candidate))
        .collect::<Vec<_>>();
    memories.sort_by(|left, right| {
        (&left.memory_id, &left.revision_id).cmp(&(&right.memory_id, &right.revision_id))
    });
    if let Some(pair) = memories
        .windows(2)
        .find(|pair| pair[0].revision_id == pair[1].revision_id)
    {
        return Err(MemoryError::DuplicateMemoryRevision(
            pair[0].revision_id.clone(),
        ));
    }

    let mut pending = pending_session_facts.iter().collect::<Vec<_>>();
    pending.sort_by(|left, right| {
        (left.kind, &left.identity_hash, &left.state_hash).cmp(&(
            right.kind,
            &right.identity_hash,
            &right.state_hash,
        ))
    });
    if pending
        .windows(2)
        .any(|pair| pair[0].kind == pair[1].kind && pair[0].identity_hash == pair[1].identity_hash)
    {
        return Err(MemoryError::DuplicateCompactionFact);
    }
    let memory_fact_count =
        u32::try_from(memories.len()).map_err(|_| MemoryError::NumericOverflow)?;
    let pending_session_fact_count =
        u32::try_from(pending.len()).map_err(|_| MemoryError::NumericOverflow)?;

    let mut encoder = CanonicalEncoder::new();
    encoder.integer(
        "compaction_facts_version",
        u64::from(COMPACTION_FACTS_VERSION),
    )?;
    encoder.integer(
        "memory_fact_count",
        u64::try_from(memories.len()).map_err(|_| MemoryError::NumericOverflow)?,
    )?;
    for memory in memories {
        if memory.status != MemoryRevisionStatus::Active
            || normalized_content_hash(memory.content.as_str())? != memory.content_hash
        {
            return Err(MemoryError::InvalidCompactionFact);
        }
        encoder.field("memory_id", memory.memory_id.as_str().as_bytes())?;
        encoder.field("revision_id", memory.revision_id.as_str().as_bytes())?;
        encoder.field(
            "scope",
            &serde_json::to_vec(&memory.scope).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "kind",
            &serde_json::to_vec(&memory.kind).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "trust",
            &serde_json::to_vec(&memory.trust).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.integer("confidence", u64::from(memory.confidence.get()))?;
        encoder.field(
            "sensitivity",
            &serde_json::to_vec(&memory.sensitivity)
                .map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field(
            "validity",
            &serde_json::to_vec(&memory.validity).map_err(|_| MemoryError::InvalidDomainValue)?,
        )?;
        encoder.field("content_hash", memory.content_hash.as_str().as_bytes())?;
        encoder.field("created_at", &memory.created_at.get().to_be_bytes())?;
    }

    encoder.integer(
        "pending_session_fact_count",
        u64::try_from(pending.len()).map_err(|_| MemoryError::NumericOverflow)?,
    )?;
    for fact in pending {
        encoder.integer("pending_kind", pending_kind_code(fact.kind))?;
        encoder.field(
            "pending_identity_hash",
            fact.identity_hash.as_str().as_bytes(),
        )?;
        encoder.field("pending_state_hash", fact.state_hash.as_str().as_bytes())?;
    }
    Ok(EffectiveDurableFactsFingerprint {
        hash: encoder.finish()?,
        memory_fact_count,
        pending_session_fact_count,
    })
}

/// Derives every pending session fact from one complete authoritative event prefix.
///
/// Raw prompts and model-authored tool arguments are admitted only to the state hash and are never
/// returned. An unresolved `ask` decision is represented both as a permission fact and inside its
/// unsettled tool fact so compaction preserves the human decision boundary and the complete call.
pub fn pending_session_facts_from_events(
    session_id: &SessionId,
    expected_last_sequence: SessionSequence,
    events: &[EventEnvelope],
) -> Result<Vec<PendingSessionFact>, MemoryError> {
    validate_event_prefix(session_id, expected_last_sequence, events)?;

    let mut inputs = BTreeMap::<InputId, InputState>::new();
    let mut attempts = BTreeSet::<AttemptId>::new();
    let mut tools = BTreeMap::<ToolCallId, PendingToolState>::new();
    for event in events {
        match event.payload() {
            EventPayload::InputAdmitted {
                input_id,
                prompt,
                delivery_mode,
            } => {
                if inputs
                    .insert(
                        input_id.clone(),
                        InputState {
                            prompt: prompt.clone(),
                            delivery_mode: *delivery_mode,
                            promoted: false,
                        },
                    )
                    .is_some()
                {
                    return Err(MemoryError::InvalidCompactionEventStream);
                }
            }
            EventPayload::AttemptPrepared {
                attempt_id,
                input_id,
                retry_of,
                ..
            } => {
                if !attempts.insert(attempt_id.clone()) {
                    return Err(MemoryError::InvalidCompactionEventStream);
                }
                let input = inputs
                    .get_mut(input_id)
                    .ok_or(MemoryError::InvalidCompactionEventStream)?;
                if retry_of.is_none() {
                    if input.promoted {
                        return Err(MemoryError::InvalidCompactionEventStream);
                    }
                    input.promoted = true;
                }
            }
            EventPayload::ToolCallProposed { attempt_id, call } => {
                if !attempts.contains(attempt_id)
                    || tools
                        .insert(
                            call.tool_call_id.clone(),
                            PendingToolState {
                                attempt_id: attempt_id.clone(),
                                call: call.clone(),
                                stage: PendingToolStage::Proposed,
                                policy_decision: None,
                                human_answer: None,
                            },
                        )
                        .is_some()
                {
                    return Err(MemoryError::InvalidCompactionEventStream);
                }
            }
            EventPayload::ToolPermissionRecorded {
                tool_call_id,
                decision_id,
                outcome,
            } => {
                let tool =
                    require_tool_stage(&mut tools, tool_call_id, PendingToolStage::Proposed)?;
                tool.policy_decision = Some((decision_id.clone(), *outcome));
                tool.stage = match outcome {
                    PermissionOutcome::Deny => PendingToolStage::DeniedPending,
                    PermissionOutcome::Ask => PendingToolStage::PermissionPending,
                    PermissionOutcome::Allow => PendingToolStage::Authorized,
                };
            }
            EventPayload::ToolPermissionAnswered {
                tool_call_id,
                decision_id,
                answer,
            } => {
                let tool = require_tool_stage(
                    &mut tools,
                    tool_call_id,
                    PendingToolStage::PermissionPending,
                )?;
                if tool
                    .policy_decision
                    .as_ref()
                    .is_none_or(|(recorded_id, outcome)| {
                        recorded_id != decision_id || *outcome != PermissionOutcome::Ask
                    })
                {
                    return Err(MemoryError::InvalidCompactionEventStream);
                }
                tool.human_answer = Some((decision_id.clone(), *answer));
                tool.stage = match answer {
                    PermissionAnswer::AllowOnce => PendingToolStage::Authorized,
                    PermissionAnswer::Deny => PendingToolStage::DeniedPending,
                };
            }
            EventPayload::ToolCallStarted { tool_call_id } => {
                require_tool_stage(&mut tools, tool_call_id, PendingToolStage::Authorized)?.stage =
                    PendingToolStage::Running;
            }
            EventPayload::ToolCallCompleted { tool_call_id, .. }
            | EventPayload::ToolCallFailed { tool_call_id, .. }
            | EventPayload::ToolCallMarkedUnknown { tool_call_id } => {
                require_tool_stage(&mut tools, tool_call_id, PendingToolStage::Running)?.stage =
                    PendingToolStage::Settled;
            }
            EventPayload::ToolCallDenied { tool_call_id } => {
                require_tool_stage(&mut tools, tool_call_id, PendingToolStage::DeniedPending)?
                    .stage = PendingToolStage::Settled;
            }
            EventPayload::ToolCallCancelled { tool_call_id } => {
                let tool = tools
                    .get_mut(tool_call_id)
                    .ok_or(MemoryError::InvalidCompactionEventStream)?;
                if !matches!(
                    tool.stage,
                    PendingToolStage::Proposed
                        | PendingToolStage::PermissionPending
                        | PendingToolStage::Authorized
                        | PendingToolStage::Running
                ) {
                    return Err(MemoryError::InvalidCompactionEventStream);
                }
                tool.stage = PendingToolStage::Settled;
            }
            _ => {}
        }
    }

    let mut facts = Vec::new();
    for (input_id, input) in inputs.iter().filter(|(_, input)| !input.promoted) {
        facts.push(pending_input_fact(input_id, input)?);
    }
    for (tool_call_id, tool) in tools
        .iter()
        .filter(|(_, tool)| tool.stage != PendingToolStage::Settled)
    {
        if tool.stage == PendingToolStage::PermissionPending {
            facts.push(pending_permission_fact(tool_call_id, tool)?);
        }
        facts.push(pending_tool_fact(tool_call_id, tool)?);
    }
    Ok(facts)
}

fn validate_event_prefix(
    session_id: &SessionId,
    expected_last_sequence: SessionSequence,
    events: &[EventEnvelope],
) -> Result<(), MemoryError> {
    if u64::try_from(events.len()).ok() != Some(expected_last_sequence.get()) {
        return Err(MemoryError::InvalidCompactionEventStream);
    }
    if events
        .first()
        .is_none_or(|event| !matches!(event.payload(), EventPayload::SessionCreated))
    {
        return Err(MemoryError::InvalidCompactionEventStream);
    }
    let mut event_ids = BTreeSet::<EventId>::new();
    let mut command_ids = BTreeSet::<CommandId>::new();
    for (index, event) in events.iter().enumerate() {
        let expected = u64::try_from(index)
            .map_err(|_| MemoryError::NumericOverflow)?
            .checked_add(1)
            .ok_or(MemoryError::NumericOverflow)?;
        if event.schema_version() != EVENT_SCHEMA_V1
            || event.session_id() != session_id
            || event.sequence().get() != expected
            || !event_ids.insert(event.event_id().clone())
        {
            return Err(MemoryError::InvalidCompactionEventStream);
        }
        if let Causation::Event(cause) = event.causation()
            && !event_ids.contains(cause)
        {
            return Err(MemoryError::InvalidCompactionEventStream);
        }
        if let Causation::Command(command_id) = event.causation()
            && !command_ids.insert(command_id.clone())
        {
            return Err(MemoryError::InvalidCompactionEventStream);
        }
    }
    Ok(())
}

fn require_tool_stage<'a>(
    tools: &'a mut BTreeMap<ToolCallId, PendingToolState>,
    tool_call_id: &ToolCallId,
    expected: PendingToolStage,
) -> Result<&'a mut PendingToolState, MemoryError> {
    let tool = tools
        .get_mut(tool_call_id)
        .ok_or(MemoryError::InvalidCompactionEventStream)?;
    if tool.stage != expected {
        return Err(MemoryError::InvalidCompactionEventStream);
    }
    Ok(tool)
}

fn pending_input_fact(
    input_id: &InputId,
    input: &InputState,
) -> Result<PendingSessionFact, MemoryError> {
    let identity_hash = fact_identity_hash("input", input_id.as_str())?;
    let mut state = CanonicalEncoder::new();
    state.field("kind", b"input")?;
    state.field("input_id", input_id.as_str().as_bytes())?;
    state.field("prompt", input.prompt.as_str().as_bytes())?;
    state.field(
        "delivery_mode",
        match input.delivery_mode {
            DeliveryMode::NextTurn => b"next_turn",
        },
    )?;
    Ok(PendingSessionFact::new(
        PendingSessionFactKind::Input,
        identity_hash,
        state.finish()?,
    ))
}

fn pending_permission_fact(
    tool_call_id: &ToolCallId,
    tool: &PendingToolState,
) -> Result<PendingSessionFact, MemoryError> {
    let (decision_id, outcome) = tool
        .policy_decision
        .as_ref()
        .ok_or(MemoryError::InvalidCompactionEventStream)?;
    if *outcome != PermissionOutcome::Ask || tool.human_answer.is_some() {
        return Err(MemoryError::InvalidCompactionEventStream);
    }
    let identity_hash = fact_identity_hash("permission", tool_call_id.as_str())?;
    let mut state = CanonicalEncoder::new();
    state.field("kind", b"permission")?;
    state.field("tool_call_id", tool_call_id.as_str().as_bytes())?;
    state.field("decision_id", decision_id.as_str().as_bytes())?;
    state.field("outcome", permission_outcome_code(*outcome))?;
    Ok(PendingSessionFact::new(
        PendingSessionFactKind::Permission,
        identity_hash,
        state.finish()?,
    ))
}

fn pending_tool_fact(
    tool_call_id: &ToolCallId,
    tool: &PendingToolState,
) -> Result<PendingSessionFact, MemoryError> {
    let identity_hash = fact_identity_hash("tool", tool_call_id.as_str())?;
    let mut state = CanonicalEncoder::new();
    state.field("kind", b"tool")?;
    state.field("tool_call_id", tool_call_id.as_str().as_bytes())?;
    state.field("attempt_id", tool.attempt_id.as_str().as_bytes())?;
    state.field(
        "call",
        &serde_json::to_vec(&tool.call).map_err(|_| MemoryError::InvalidDomainValue)?,
    )?;
    state.field("stage", pending_tool_stage_code(tool.stage))?;
    match &tool.policy_decision {
        Some((decision_id, outcome)) => {
            state.field("policy_decision_id", decision_id.as_str().as_bytes())?;
            state.field("policy_outcome", permission_outcome_code(*outcome))?;
        }
        None => {
            state.field("policy_decision_id", b"")?;
            state.field("policy_outcome", b"")?;
        }
    }
    match &tool.human_answer {
        Some((decision_id, answer)) => {
            state.field("human_decision_id", decision_id.as_str().as_bytes())?;
            state.field("human_answer", permission_answer_code(*answer))?;
        }
        None => {
            state.field("human_decision_id", b"")?;
            state.field("human_answer", b"")?;
        }
    }
    Ok(PendingSessionFact::new(
        PendingSessionFactKind::Tool,
        identity_hash,
        state.finish()?,
    ))
}

fn fact_identity_hash(kind: &str, identity: &str) -> Result<Sha256Digest, MemoryError> {
    let mut encoder = CanonicalEncoder::new();
    encoder.field("kind", kind.as_bytes())?;
    encoder.field("identity", identity.as_bytes())?;
    encoder.finish()
}

const fn pending_tool_stage_code(stage: PendingToolStage) -> &'static [u8] {
    match stage {
        PendingToolStage::Proposed => b"proposed",
        PendingToolStage::PermissionPending => b"permission_pending",
        PendingToolStage::Authorized => b"authorized",
        PendingToolStage::DeniedPending => b"denied_pending",
        PendingToolStage::Running => b"running",
        PendingToolStage::Settled => b"settled",
    }
}

const fn permission_outcome_code(outcome: PermissionOutcome) -> &'static [u8] {
    match outcome {
        PermissionOutcome::Deny => b"deny",
        PermissionOutcome::Ask => b"ask",
        PermissionOutcome::Allow => b"allow",
    }
}

const fn permission_answer_code(answer: PermissionAnswer) -> &'static [u8] {
    match answer {
        PermissionAnswer::AllowOnce => b"allow_once",
        PermissionAnswer::Deny => b"deny",
    }
}

/// Verifies a persisted compaction fingerprint against authoritative current facts.
pub fn verify_effective_durable_facts_hash(
    scope: &RetrievalScope,
    candidates: &[MemoryCandidate],
    pending_session_facts: &[PendingSessionFact],
    expected: &Sha256Digest,
) -> Result<bool, MemoryError> {
    Ok(effective_durable_facts_hash(scope, candidates, pending_session_facts)? == *expected)
}

const fn pending_kind_code(kind: PendingSessionFactKind) -> u64 {
    match kind {
        PendingSessionFactKind::Input => 1,
        PendingSessionFactKind::Permission => 2,
        PendingSessionFactKind::Tool => 3,
    }
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        CapabilityKind, CapabilityRequest, CommandId, ConfidenceBasisPoints, CorrelationId,
        EventId, MemoryContent, MemoryId, MemoryKind, MemoryRevisionId, MemoryScope,
        MemoryValidity, ModelId, ModelRef, PermissionDecisionId, ProviderCallId, ProviderId,
        ResourceRef, Sensitivity, SessionId, TimestampMillis, ToolArguments, ToolCallId, ToolName,
        TrustClass, UserId, WorkspaceId,
    };
    use serde_json::json;

    use super::*;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::new(character.to_string().repeat(64)).expect("digest")
    }

    fn scope() -> RetrievalScope {
        RetrievalScope {
            user_id: UserId::new("user-1").expect("user ID"),
            workspace_id: WorkspaceId::new("workspace-1").expect("workspace ID"),
            session_id: SessionId::new("session-1").expect("session ID"),
            agent_id: None,
            as_of: TimestampMillis::new(20),
            sensitivity_ceiling: Sensitivity::Internal,
        }
    }

    fn candidate(id: &str, content: &str) -> MemoryCandidate {
        MemoryCandidate {
            memory_id: MemoryId::new(format!("memory-{id}")).expect("memory ID"),
            revision_id: MemoryRevisionId::new(format!("revision-{id}")).expect("revision ID"),
            status: MemoryRevisionStatus::Active,
            scope: MemoryScope::Workspace(WorkspaceId::new("workspace-1").expect("workspace ID")),
            kind: MemoryKind::Fact,
            trust: TrustClass::UserApproved,
            confidence: ConfidenceBasisPoints::new(9_000).expect("confidence"),
            sensitivity: Sensitivity::Internal,
            validity: MemoryValidity::Indefinite,
            content: MemoryContent::new(content).expect("content"),
            content_hash: normalized_content_hash(content).expect("content hash"),
            created_at: TimestampMillis::new(10),
            exact_match: false,
            lexical_basis_points: 0,
            conflicted: false,
        }
    }

    fn event(sequence: u64, payload: EventPayload) -> EventEnvelope {
        EventEnvelope::new_v1(
            EventId::new(format!("event-{sequence}")).expect("event ID"),
            SessionId::new("session-1").expect("session ID"),
            SessionSequence::new(sequence).expect("sequence"),
            TimestampMillis::new(i64::try_from(sequence).expect("timestamp")),
            Causation::Command(CommandId::new(format!("command-{sequence}")).expect("command ID")),
            CorrelationId::new(format!("correlation-{sequence}")).expect("correlation ID"),
            payload,
        )
    }

    fn tool_call() -> ToolCallSpec {
        ToolCallSpec {
            tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
            provider_call_id: ProviderCallId::new("provider-call-1").expect("provider call ID"),
            tool_name: ToolName::new("read_file").expect("tool name"),
            schema_version: 1,
            arguments: ToolArguments::new(json!({"path": "notes/雪.txt"})).expect("arguments"),
            capability: CapabilityRequest {
                kind: CapabilityKind::FilesystemRead,
                resource: ResourceRef::new("notes/雪.txt").expect("resource"),
            },
        }
    }

    fn pending_history() -> Vec<EventEnvelope> {
        let promoted_input = InputId::new("input-promoted").expect("promoted input ID");
        vec![
            event(1, EventPayload::SessionCreated),
            event(
                2,
                EventPayload::InputAdmitted {
                    input_id: InputId::new("input-pending").expect("pending input ID"),
                    prompt: PromptText::new("retain this secret-shaped-but-redacted fact")
                        .expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            ),
            event(
                3,
                EventPayload::InputAdmitted {
                    input_id: promoted_input.clone(),
                    prompt: PromptText::new("already promoted").expect("prompt"),
                    delivery_mode: DeliveryMode::NextTurn,
                },
            ),
            event(
                4,
                EventPayload::AttemptPrepared {
                    attempt_id: AttemptId::new("attempt-1").expect("attempt ID"),
                    input_id: promoted_input,
                    model: ModelRef::new(
                        ProviderId::new("provider").expect("provider ID"),
                        ModelId::new("model").expect("model ID"),
                    ),
                    retry_of: None,
                },
            ),
            event(
                5,
                EventPayload::ToolCallProposed {
                    attempt_id: AttemptId::new("attempt-1").expect("attempt ID"),
                    call: tool_call(),
                },
            ),
            event(
                6,
                EventPayload::ToolPermissionRecorded {
                    tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
                    decision_id: PermissionDecisionId::new("decision-1").expect("decision ID"),
                    outcome: PermissionOutcome::Ask,
                },
            ),
        ]
    }

    #[test]
    fn shuffled_physical_order_has_one_effective_facts_hash() {
        let left = candidate("left", "left durable fact");
        let right = candidate("right", "right durable fact");
        let pending_input =
            PendingSessionFact::new(PendingSessionFactKind::Input, digest('a'), digest('b'));
        let pending_tool =
            PendingSessionFact::new(PendingSessionFactKind::Tool, digest('c'), digest('d'));

        let first = effective_durable_facts_hash(
            &scope(),
            &[right.clone(), left.clone()],
            &[pending_tool.clone(), pending_input.clone()],
        )
        .expect("first hash");
        let second =
            effective_durable_facts_hash(&scope(), &[left, right], &[pending_input, pending_tool])
                .expect("second hash");

        assert_eq!(first, second);
    }

    #[test]
    fn retrieval_scores_do_not_change_the_durable_fact_set() {
        let original = candidate("stable", "stable durable fact");
        let mut reranked = original.clone();
        reranked.exact_match = true;
        reranked.lexical_basis_points = 10_000;

        assert_eq!(
            effective_durable_facts_hash(&scope(), &[original], &[]).expect("original"),
            effective_durable_facts_hash(&scope(), &[reranked], &[]).expect("reranked")
        );
    }

    #[test]
    fn changed_memory_or_pending_state_changes_the_hash() {
        let original = candidate("stable", "first durable fact");
        let changed = candidate("stable", "changed durable fact");
        let pending =
            PendingSessionFact::new(PendingSessionFactKind::Permission, digest('e'), digest('f'));
        let changed_pending =
            PendingSessionFact::new(PendingSessionFactKind::Permission, digest('e'), digest('0'));

        let baseline = effective_durable_facts_hash(
            &scope(),
            std::slice::from_ref(&original),
            std::slice::from_ref(&pending),
        )
        .expect("baseline");
        assert_ne!(
            baseline,
            effective_durable_facts_hash(&scope(), &[changed], &[pending]).expect("changed memory")
        );
        assert_ne!(
            baseline,
            effective_durable_facts_hash(&scope(), &[original], &[changed_pending])
                .expect("changed pending state")
        );
    }

    #[test]
    fn ineligible_memory_is_not_an_effective_durable_fact() {
        let mut proposed = candidate("proposal", "untrusted candidate");
        proposed.status = MemoryRevisionStatus::Proposed;
        proposed.trust = TrustClass::UntrustedProposal;

        assert_eq!(
            effective_durable_facts_hash(&scope(), &[proposed], &[]).expect("filtered"),
            effective_durable_facts_hash(&scope(), &[], &[]).expect("empty")
        );
    }

    #[test]
    fn malformed_or_duplicate_facts_fail_closed() {
        let mut malformed = candidate("bad", "actual bytes");
        malformed.content_hash = digest('1');
        assert_eq!(
            effective_durable_facts_hash(&scope(), &[malformed], &[]),
            Err(MemoryError::InvalidCompactionFact)
        );

        let fact = PendingSessionFact::new(PendingSessionFactKind::Tool, digest('2'), digest('3'));
        let conflicting =
            PendingSessionFact::new(PendingSessionFactKind::Tool, digest('2'), digest('4'));
        assert_eq!(
            effective_durable_facts_hash(&scope(), &[], &[fact, conflicting]),
            Err(MemoryError::DuplicateCompactionFact)
        );
    }

    #[test]
    fn event_prefix_derives_unpromoted_input_pending_permission_and_unsettled_tool() {
        let events = pending_history();
        let facts = pending_session_facts_from_events(
            &SessionId::new("session-1").expect("session ID"),
            SessionSequence::new(6).expect("sequence"),
            &events,
        )
        .expect("pending facts");

        assert_eq!(
            facts
                .iter()
                .map(PendingSessionFact::kind)
                .collect::<Vec<_>>(),
            vec![
                PendingSessionFactKind::Input,
                PendingSessionFactKind::Permission,
                PendingSessionFactKind::Tool,
            ]
        );
        let fingerprint = effective_durable_facts(&scope(), &[], &facts).expect("fingerprint");
        assert_eq!(fingerprint.memory_fact_count(), 0);
        assert_eq!(fingerprint.pending_session_fact_count(), 3);
        assert!(!format!("{fingerprint:?}").contains("secret-shaped"));
    }

    #[test]
    fn permission_answer_and_tool_settlement_change_the_exact_pending_snapshot() {
        let mut events = pending_history();
        let pending = pending_session_facts_from_events(
            &SessionId::new("session-1").expect("session ID"),
            SessionSequence::new(6).expect("sequence"),
            &events,
        )
        .expect("pending facts");
        events.push(event(
            7,
            EventPayload::ToolPermissionAnswered {
                tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
                decision_id: PermissionDecisionId::new("decision-1").expect("decision ID"),
                answer: PermissionAnswer::AllowOnce,
            },
        ));
        let authorized = pending_session_facts_from_events(
            &SessionId::new("session-1").expect("session ID"),
            SessionSequence::new(7).expect("sequence"),
            &events,
        )
        .expect("authorized facts");
        assert_eq!(authorized.len(), 2);
        assert!(
            !authorized
                .iter()
                .any(|fact| fact.kind() == PendingSessionFactKind::Permission)
        );
        assert_ne!(
            effective_durable_facts_hash(&scope(), &[], &pending).expect("pending hash"),
            effective_durable_facts_hash(&scope(), &[], &authorized).expect("authorized hash")
        );

        events.push(event(
            8,
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
            },
        ));
        events.push(event(
            9,
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
                output: autoharness_domain::ToolOutput::new("done", None, 4, false)
                    .expect("output"),
            },
        ));
        let settled = pending_session_facts_from_events(
            &SessionId::new("session-1").expect("session ID"),
            SessionSequence::new(9).expect("sequence"),
            &events,
        )
        .expect("settled facts");
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].kind(), PendingSessionFactKind::Input);
    }

    #[test]
    fn incomplete_or_semantically_invalid_event_prefix_fails_closed() {
        let mut events = pending_history();
        assert_eq!(
            pending_session_facts_from_events(
                &SessionId::new("session-1").expect("session ID"),
                SessionSequence::new(7).expect("sequence"),
                &events,
            ),
            Err(MemoryError::InvalidCompactionEventStream)
        );
        events.push(event(
            7,
            EventPayload::ToolPermissionAnswered {
                tool_call_id: ToolCallId::new("tool-1").expect("tool call ID"),
                decision_id: PermissionDecisionId::new("forged-decision").expect("decision ID"),
                answer: PermissionAnswer::Deny,
            },
        ));
        assert_eq!(
            pending_session_facts_from_events(
                &SessionId::new("session-1").expect("session ID"),
                SessionSequence::new(7).expect("sequence"),
                &events,
            ),
            Err(MemoryError::InvalidCompactionEventStream)
        );
    }
}
