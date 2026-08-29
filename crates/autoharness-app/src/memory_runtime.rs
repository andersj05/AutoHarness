//! Trusted application planning for revisioned memory commands.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use autoharness_domain::{
    MAX_MEMORY_VALIDATION_CANDIDATES, MemoryCausation, MemoryCommandEnvelope, MemoryCommandPayload,
    MemoryOperationEnvelope, MemoryOperationId, MemoryOperationPayload, MemoryRevision,
    MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionStatus, MemorySequence,
    MemoryValidationResult, MemoryValidationStatus, Sensitivity, TimestampMillis,
};
use autoharness_memory::{ExistingMemory, MemoryError, MemoryValidationPolicy, MemoryValidatorV1};
use autoharness_store::{
    ActiveMemoryHead, ActiveMemoryHeadQuery, DEFAULT_MEMORY_PAGE_SIZE, MemoryAppendBatchRequest,
    MemoryAppendOperation, MemoryAppendReceipt, MemoryEvidenceContent, MemoryRevisionContent,
    MemoryStore,
};
use sha2::{Digest, Sha256};

/// Maximum operations emitted for one atomic trusted memory command.
pub const MAX_MEMORY_COMMAND_OPERATIONS: usize = 4;

/// One trusted command translated into a contiguous atomic store batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCommandPlan {
    batch: MemoryAppendBatchRequest,
    validation: Option<MemoryValidationResult>,
}

impl MemoryCommandPlan {
    /// Returns the contiguous operation batch that must commit atomically.
    #[must_use]
    pub const fn batch(&self) -> &MemoryAppendBatchRequest {
        &self.batch
    }

    /// Returns deterministic validation captured by this command, when any.
    #[must_use]
    pub const fn validation(&self) -> Option<&MemoryValidationResult> {
        self.validation.as_ref()
    }
}

/// Durable result of one trusted atomic memory command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryCommandCommit {
    receipt: MemoryAppendReceipt,
    validation: Option<MemoryValidationResult>,
}

impl MemoryCommandCommit {
    /// Returns the atomic store receipt.
    #[must_use]
    pub const fn receipt(&self) -> MemoryAppendReceipt {
        self.receipt
    }

    /// Returns deterministic validation persisted in the same batch, when any.
    #[must_use]
    pub const fn validation(&self) -> Option<&MemoryValidationResult> {
        self.validation.as_ref()
    }

    /// Returns the exact existing duplicate identity, when detected.
    #[must_use]
    pub fn duplicate_memory_id(&self) -> Option<&autoharness_domain::MemoryId> {
        self.validation
            .as_ref()
            .and_then(|validation| validation.duplicate_candidates().first())
    }

    /// Returns exact existing contradiction candidates.
    #[must_use]
    pub fn contradiction_candidates(&self) -> &[autoharness_domain::MemoryId] {
        self.validation
            .as_ref()
            .map_or(&[], |validation| validation.contradiction_candidates())
    }
}

/// Safe trusted-command planning failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryCommandError {
    /// The command's optimistic item sequence no longer matches durable state.
    VersionConflict,
    /// The requested lifecycle transition is not valid for the current item.
    InvalidTransition,
    /// Deterministic validation rejected the exact proposed content.
    ValidationRejected,
    /// The command requires a trusted internal path that this planner does not expose.
    UnsupportedCommand,
    /// Pure validation or canonical construction failed.
    Policy(MemoryError),
}

impl Display for MemoryCommandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::VersionConflict => "memory changed before the command could be committed",
            Self::InvalidTransition => "the requested memory lifecycle transition is invalid",
            Self::ValidationRejected => "memory content failed deterministic validation",
            Self::UnsupportedCommand => {
                "the memory command is reserved for a trusted internal workflow"
            }
            Self::Policy(error) => return Display::fmt(error, formatter),
        };
        formatter.write_str(message)
    }
}

impl Error for MemoryCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::VersionConflict
            | Self::InvalidTransition
            | Self::ValidationRejected
            | Self::UnsupportedCommand => None,
        }
    }
}

impl From<MemoryError> for MemoryCommandError {
    fn from(value: MemoryError) -> Self {
        Self::Policy(value)
    }
}

/// Executes one trusted command entirely inside the application's storage owner.
///
/// Snapshot reads, deterministic planning, and the final batch append cannot
/// interleave with another application storage request.
pub fn execute_memory_command(
    store: &mut impl MemoryStore,
    command: &MemoryCommandEnvelope,
    occurred_at: TimestampMillis,
) -> Result<MemoryCommandCommit, crate::error::AppError> {
    let operations = load_all_operations(store, command.memory_id())?;
    let revisions = store.load_memory_revisions(command.memory_id())?;
    let existing = duplicate_candidates(store, command, &operations, occurred_at)?;
    let plan = plan_memory_command(command, occurred_at, &operations, &revisions, &existing)?;
    let receipt = store.append_memory_batch(plan.batch())?;
    Ok(MemoryCommandCommit {
        receipt,
        validation: plan.validation().cloned(),
    })
}

fn load_all_operations(
    store: &impl MemoryStore,
    memory_id: &autoharness_domain::MemoryId,
) -> Result<Vec<MemoryOperationEnvelope>, crate::error::AppError> {
    let mut operations = Vec::new();
    let mut after_sequence = 0;
    loop {
        let page =
            store.load_memory_operations(memory_id, after_sequence, DEFAULT_MEMORY_PAGE_SIZE)?;
        let loaded = page.len();
        if let Some(last) = page.last() {
            after_sequence = last.sequence().get();
        }
        operations.extend(page);
        if loaded < usize::try_from(DEFAULT_MEMORY_PAGE_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(operations)
}

fn duplicate_candidates(
    store: &impl MemoryStore,
    command: &MemoryCommandEnvelope,
    operations: &[MemoryOperationEnvelope],
    _as_of: TimestampMillis,
) -> Result<Vec<ExistingMemory>, crate::error::AppError> {
    let Some(draft) = command_draft(command.payload()) else {
        return Ok(Vec::new());
    };
    let (scope, memory_kind) = match command.payload() {
        MemoryCommandPayload::CreateMemory {
            scope, memory_kind, ..
        } => (scope.clone(), *memory_kind),
        _ => item_scope_kind(operations)?,
    };
    let candidate_limit = u32::try_from(MAX_MEMORY_VALIDATION_CANDIDATES)
        .expect("the domain validation-candidate bound fits a store query");
    let identity_query = ActiveMemoryHeadQuery::new(
        vec![scope.clone()],
        memory_kind,
        draft.subject_key().cloned(),
        candidate_limit,
    )?;
    let duplicate_query = ActiveMemoryHeadQuery::new(
        vec![scope],
        memory_kind,
        draft.subject_key().cloned(),
        candidate_limit,
    )?
    .with_content_hash(draft.content_hash().clone());
    let exact_heads = store.load_active_memory_heads(&duplicate_query)?;
    let identity_heads = store.load_active_memory_heads(&identity_query)?;
    let mut heads = Vec::<ActiveMemoryHead>::with_capacity(MAX_MEMORY_VALIDATION_CANDIDATES);
    for head in exact_heads.into_iter().chain(identity_heads) {
        if head.memory_id() != command.memory_id()
            && heads.len() < MAX_MEMORY_VALIDATION_CANDIDATES
            && !heads
                .iter()
                .any(|existing| existing.memory_id() == head.memory_id())
        {
            heads.push(head);
        }
    }
    Ok(heads
        .into_iter()
        .map(|head| ExistingMemory {
            memory_id: head.memory_id().clone(),
            scope: head.scope().clone(),
            kind: head.memory_kind(),
            subject_key: head.revision().subject_key().cloned(),
            content_hash: head.revision().content_hash().clone(),
        })
        .collect())
}

const fn command_draft(payload: &MemoryCommandPayload) -> Option<&MemoryRevisionDraft> {
    match payload {
        MemoryCommandPayload::CreateMemory { revision, .. }
        | MemoryCommandPayload::ProposeRevision { revision, .. }
        | MemoryCommandPayload::ReviseMemory { revision, .. } => Some(revision),
        MemoryCommandPayload::ApproveProposal {
            approved_revision, ..
        } => Some(approved_revision),
        MemoryCommandPayload::RecordValidation { .. }
        | MemoryCommandPayload::ActivateRevision { .. }
        | MemoryCommandPayload::RejectRevision { .. }
        | MemoryCommandPayload::RetractMemory { .. }
        | MemoryCommandPayload::DeleteMemory { .. } => None,
    }
}

/// Plans one trusted memory command against an immutable store snapshot.
///
/// The returned appends are intentionally not exposed as independent writes.
/// The storage owner must submit the complete slice through one atomic batch.
pub fn plan_memory_command(
    command: &MemoryCommandEnvelope,
    occurred_at: TimestampMillis,
    current_operations: &[MemoryOperationEnvelope],
    current_revisions: &[MemoryRevision],
    duplicate_candidates: &[ExistingMemory],
) -> Result<MemoryCommandPlan, MemoryCommandError> {
    let actual_sequence = current_operations
        .last()
        .map_or(0, |operation| operation.sequence().get());
    validate_snapshot(
        command,
        current_operations,
        current_revisions,
        actual_sequence,
    )?;
    let mut batch = BatchBuilder::new(command, occurred_at, actual_sequence);

    match command.payload() {
        MemoryCommandPayload::CreateMemory {
            scope,
            memory_kind,
            revision,
        } => {
            let outcome =
                validate_revision(scope, *memory_kind, revision, duplicate_candidates, None)?;
            let validation = outcome.result().clone();
            if validation.status() == MemoryValidationStatus::Rejected {
                return Err(MemoryCommandError::ValidationRejected);
            }
            batch.introduce(
                "create",
                MemoryOperationPayload::MemoryCreated {
                    scope: scope.clone(),
                    memory_kind: *memory_kind,
                    revision: MemoryRevision::from_draft(
                        MemoryRevisionStatus::Proposed,
                        revision,
                        occurred_at,
                        None,
                    ),
                },
                revision,
            )?;
            batch.validation(revision.revision_id().clone(), validation.clone())?;
            if is_explicit_accepted(revision, &validation) {
                batch.activate(revision.revision_id().clone())?;
            }
            Ok(batch.finish(Some(outcome)))
        }
        MemoryCommandPayload::ProposeRevision {
            revision,
            supersedes_revision_id,
        } => {
            let (scope, memory_kind) = item_scope_kind(current_operations)?;
            require_revision(
                current_revisions,
                supersedes_revision_id,
                &[MemoryRevisionStatus::Active],
            )?;
            require_subject(current_revisions, supersedes_revision_id, revision)?;
            require_next_revision(current_revisions, revision)?;
            let outcome = validate_revision(
                &scope,
                memory_kind,
                revision,
                duplicate_candidates,
                Some(command.memory_id()),
            )?;
            let validation = outcome.result().clone();
            if validation.status() == MemoryValidationStatus::Rejected {
                return Err(MemoryCommandError::ValidationRejected);
            }
            batch.introduce(
                "propose",
                MemoryOperationPayload::RevisionProposed {
                    revision: MemoryRevision::from_draft(
                        MemoryRevisionStatus::Proposed,
                        revision,
                        occurred_at,
                        Some(supersedes_revision_id.clone()),
                    ),
                },
                revision,
            )?;
            batch.validation(revision.revision_id().clone(), validation.clone())?;
            Ok(batch.finish(Some(outcome)))
        }
        MemoryCommandPayload::ReviseMemory {
            revision,
            supersedes_revision_id,
        } => {
            let (scope, memory_kind) = item_scope_kind(current_operations)?;
            require_revision(
                current_revisions,
                supersedes_revision_id,
                &[MemoryRevisionStatus::Active],
            )?;
            require_subject(current_revisions, supersedes_revision_id, revision)?;
            require_next_revision(current_revisions, revision)?;
            let outcome = validate_revision(
                &scope,
                memory_kind,
                revision,
                duplicate_candidates,
                Some(command.memory_id()),
            )?;
            let validation = outcome.result().clone();
            if !is_explicit_accepted(revision, &validation) {
                return Err(MemoryCommandError::ValidationRejected);
            }
            batch.introduce(
                "revise",
                MemoryOperationPayload::MemoryRevised {
                    revision: MemoryRevision::from_draft(
                        MemoryRevisionStatus::Proposed,
                        revision,
                        occurred_at,
                        Some(supersedes_revision_id.clone()),
                    ),
                },
                revision,
            )?;
            batch.validation(revision.revision_id().clone(), validation.clone())?;
            batch.activate(revision.revision_id().clone())?;
            batch.supersede(
                supersedes_revision_id.clone(),
                revision.revision_id().clone(),
            )?;
            Ok(batch.finish(Some(outcome)))
        }
        MemoryCommandPayload::ApproveProposal {
            proposal_revision_id,
            approved_revision,
        } => {
            let (scope, memory_kind) = item_scope_kind(current_operations)?;
            let proposal = require_revision(
                current_revisions,
                proposal_revision_id,
                &[MemoryRevisionStatus::Proposed],
            )?;
            let superseded_revision_id = proposal
                .supersedes_revision_id()
                .cloned()
                .unwrap_or_else(|| proposal_revision_id.clone());
            if &superseded_revision_id != proposal_revision_id {
                require_revision(
                    current_revisions,
                    &superseded_revision_id,
                    &[MemoryRevisionStatus::Active],
                )?;
            }
            require_subject(current_revisions, proposal_revision_id, approved_revision)?;
            require_next_revision(current_revisions, approved_revision)?;
            let outcome = validate_revision(
                &scope,
                memory_kind,
                approved_revision,
                duplicate_candidates,
                Some(command.memory_id()),
            )?;
            let validation = outcome.result().clone();
            if !is_explicit_accepted(approved_revision, &validation)
                || approved_revision.revision_id() == proposal_revision_id
            {
                return Err(MemoryCommandError::ValidationRejected);
            }
            batch.introduce(
                "approve",
                MemoryOperationPayload::ProposalApproved {
                    proposal_revision_id: proposal_revision_id.clone(),
                    approved_revision: MemoryRevision::from_draft(
                        MemoryRevisionStatus::Proposed,
                        approved_revision,
                        occurred_at,
                        Some(superseded_revision_id.clone()),
                    ),
                },
                approved_revision,
            )?;
            batch.validation(approved_revision.revision_id().clone(), validation.clone())?;
            batch.activate(approved_revision.revision_id().clone())?;
            batch.supersede(
                superseded_revision_id,
                approved_revision.revision_id().clone(),
            )?;
            Ok(batch.finish(Some(outcome)))
        }
        MemoryCommandPayload::RejectRevision {
            revision_id,
            reason,
        } => {
            require_revision(
                current_revisions,
                revision_id,
                &[MemoryRevisionStatus::Proposed],
            )?;
            batch.push(
                "reject",
                MemoryOperationPayload::RevisionRejected {
                    revision_id: revision_id.clone(),
                    reason: *reason,
                },
                None,
            )?;
            Ok(batch.finish(None))
        }
        MemoryCommandPayload::RetractMemory { revision_id } => {
            require_revision(
                current_revisions,
                revision_id,
                &[MemoryRevisionStatus::Active],
            )?;
            batch.push(
                "retract",
                MemoryOperationPayload::MemoryRetracted {
                    revision_id: revision_id.clone(),
                },
                None,
            )?;
            Ok(batch.finish(None))
        }
        MemoryCommandPayload::DeleteMemory { revision_id } => {
            require_revision(
                current_revisions,
                revision_id,
                &[
                    MemoryRevisionStatus::Proposed,
                    MemoryRevisionStatus::Active,
                    MemoryRevisionStatus::Rejected,
                    MemoryRevisionStatus::Retracted,
                ],
            )?;
            batch.push(
                "delete",
                MemoryOperationPayload::MemoryDeleted {
                    revision_id: revision_id.clone(),
                },
                None,
            )?;
            Ok(batch.finish(None))
        }
        MemoryCommandPayload::RecordValidation { .. }
        | MemoryCommandPayload::ActivateRevision { .. } => {
            Err(MemoryCommandError::UnsupportedCommand)
        }
    }
}

fn validate_snapshot(
    command: &MemoryCommandEnvelope,
    operations: &[MemoryOperationEnvelope],
    revisions: &[MemoryRevision],
    actual_sequence: u64,
) -> Result<(), MemoryCommandError> {
    if operations
        .iter()
        .any(|operation| operation.memory_id() != command.memory_id())
        || !operations
            .windows(2)
            .all(|pair| pair[1].sequence().get() == pair[0].sequence().get().saturating_add(1))
        || revisions
            .windows(2)
            .any(|pair| pair[1].revision().get() != pair[0].revision().get().saturating_add(1))
    {
        return Err(MemoryCommandError::InvalidTransition);
    }

    let expected = command.expected_sequence().map(MemorySequence::get);
    match (command.payload(), expected, actual_sequence) {
        (MemoryCommandPayload::CreateMemory { .. }, None, 0) => Ok(()),
        (MemoryCommandPayload::CreateMemory { .. }, _, _) => {
            Err(MemoryCommandError::VersionConflict)
        }
        (_, Some(expected), actual) if expected == actual && actual > 0 => Ok(()),
        _ => Err(MemoryCommandError::VersionConflict),
    }
}

fn item_scope_kind(
    operations: &[MemoryOperationEnvelope],
) -> Result<
    (
        autoharness_domain::MemoryScope,
        autoharness_domain::MemoryKind,
    ),
    MemoryCommandError,
> {
    let Some(first) = operations.first() else {
        return Err(MemoryCommandError::InvalidTransition);
    };
    let MemoryOperationPayload::MemoryCreated {
        scope, memory_kind, ..
    } = first.payload()
    else {
        return Err(MemoryCommandError::InvalidTransition);
    };
    Ok((scope.clone(), *memory_kind))
}

fn validate_revision(
    scope: &autoharness_domain::MemoryScope,
    kind: autoharness_domain::MemoryKind,
    revision: &MemoryRevisionDraft,
    existing: &[ExistingMemory],
    exclude_memory_id: Option<&autoharness_domain::MemoryId>,
) -> Result<autoharness_memory::MemoryValidationOutcome, MemoryCommandError> {
    let filtered = existing
        .iter()
        .filter(|candidate| exclude_memory_id != Some(&candidate.memory_id))
        .cloned()
        .collect::<Vec<_>>();
    MemoryValidatorV1
        .analyze(
            MemoryValidationPolicy {
                authorized_scopes: std::slice::from_ref(scope),
                sensitivity_ceiling: Sensitivity::Sensitive,
            },
            scope,
            kind,
            revision,
            &filtered,
        )
        .map_err(MemoryCommandError::from)
}

fn require_subject(
    revisions: &[MemoryRevision],
    prior_revision_id: &MemoryRevisionId,
    candidate: &MemoryRevisionDraft,
) -> Result<(), MemoryCommandError> {
    revisions
        .iter()
        .find(|revision| revision.revision_id() == prior_revision_id)
        .filter(|revision| revision.subject_key() == candidate.subject_key())
        .map(|_| ())
        .ok_or(MemoryCommandError::InvalidTransition)
}

fn is_explicit_accepted(
    revision: &MemoryRevisionDraft,
    validation: &MemoryValidationResult,
) -> bool {
    revision.origin() == autoharness_domain::MemoryOrigin::ExplicitUser
        && revision.trust_class() == autoharness_domain::TrustClass::UserApproved
        && validation.status() == MemoryValidationStatus::Accepted
}

fn require_revision<'a>(
    revisions: &'a [MemoryRevision],
    revision_id: &MemoryRevisionId,
    states: &[MemoryRevisionStatus],
) -> Result<&'a MemoryRevision, MemoryCommandError> {
    revisions
        .iter()
        .find(|revision| revision.revision_id() == revision_id)
        .filter(|revision| states.contains(&revision.status()))
        .ok_or(MemoryCommandError::InvalidTransition)
}

fn require_next_revision(
    current: &[MemoryRevision],
    candidate: &MemoryRevisionDraft,
) -> Result<(), MemoryCommandError> {
    let expected = current
        .last()
        .map_or(1, |revision| revision.revision().get().saturating_add(1));
    if candidate.revision().get() == expected
        && current
            .iter()
            .all(|revision| revision.revision_id() != candidate.revision_id())
    {
        Ok(())
    } else {
        Err(MemoryCommandError::InvalidTransition)
    }
}

struct BatchBuilder<'a> {
    command: &'a MemoryCommandEnvelope,
    occurred_at: TimestampMillis,
    base_sequence: u64,
    operations: Vec<MemoryAppendOperation>,
    previous_operation_id: Option<MemoryOperationId>,
}

impl<'a> BatchBuilder<'a> {
    fn new(
        command: &'a MemoryCommandEnvelope,
        occurred_at: TimestampMillis,
        base_sequence: u64,
    ) -> Self {
        Self {
            command,
            occurred_at,
            base_sequence,
            operations: Vec::with_capacity(MAX_MEMORY_COMMAND_OPERATIONS),
            previous_operation_id: None,
        }
    }

    fn introduce(
        &mut self,
        stage: &'static str,
        payload: MemoryOperationPayload,
        revision: &MemoryRevisionDraft,
    ) -> Result<(), MemoryCommandError> {
        self.push(stage, payload, Some(revision_sidecar(revision)))
    }

    fn validation(
        &mut self,
        revision_id: MemoryRevisionId,
        validation: MemoryValidationResult,
    ) -> Result<(), MemoryCommandError> {
        self.push(
            "validate",
            MemoryOperationPayload::RevisionValidated {
                revision_id,
                validation,
            },
            None,
        )
    }

    fn activate(&mut self, revision_id: MemoryRevisionId) -> Result<(), MemoryCommandError> {
        self.push(
            "activate",
            MemoryOperationPayload::RevisionActivated { revision_id },
            None,
        )
    }

    fn supersede(
        &mut self,
        revision_id: MemoryRevisionId,
        superseded_by_revision_id: MemoryRevisionId,
    ) -> Result<(), MemoryCommandError> {
        self.push(
            "supersede",
            MemoryOperationPayload::RevisionSuperseded {
                revision_id,
                superseded_by_revision_id,
            },
            None,
        )
    }

    fn push(
        &mut self,
        stage: &'static str,
        payload: MemoryOperationPayload,
        content: Option<MemoryRevisionContent>,
    ) -> Result<(), MemoryCommandError> {
        if self.operations.len() >= MAX_MEMORY_COMMAND_OPERATIONS {
            return Err(MemoryCommandError::InvalidTransition);
        }
        let offset = u64::try_from(self.operations.len())
            .map_err(|_| MemoryCommandError::InvalidTransition)?;
        let expected_last_sequence = self
            .base_sequence
            .checked_add(offset)
            .ok_or(MemoryCommandError::InvalidTransition)?;
        let sequence = expected_last_sequence
            .checked_add(1)
            .and_then(|value| MemorySequence::new(value).ok())
            .ok_or(MemoryCommandError::InvalidTransition)?;
        let operation_id = operation_id(self.command, stage);
        let causation = self.previous_operation_id.as_ref().map_or_else(
            || MemoryCausation::Command(self.command.command_id().clone()),
            |prior| MemoryCausation::Operation(prior.clone()),
        );
        let operation = MemoryOperationEnvelope::new_v1(
            operation_id.clone(),
            self.command.memory_id().clone(),
            sequence,
            self.occurred_at,
            causation,
            self.command.correlation_id().clone(),
            payload,
        );
        self.operations
            .push(MemoryAppendOperation::new(operation, content));
        self.previous_operation_id = Some(operation_id);
        Ok(())
    }

    fn finish(
        self,
        validation: Option<autoharness_memory::MemoryValidationOutcome>,
    ) -> MemoryCommandPlan {
        MemoryCommandPlan {
            batch: MemoryAppendBatchRequest::new(self.base_sequence, self.operations),
            validation: validation.map(autoharness_memory::MemoryValidationOutcome::into_result),
        }
    }
}

fn revision_sidecar(revision: &MemoryRevisionDraft) -> MemoryRevisionContent {
    let evidence = revision
        .evidence()
        .iter()
        .filter_map(|evidence| {
            evidence.excerpt().map(|excerpt| {
                MemoryEvidenceContent::new(evidence.evidence_id().clone(), excerpt.clone())
            })
        })
        .collect();
    MemoryRevisionContent::new(
        revision.revision_id().clone(),
        revision.content().clone(),
        evidence,
    )
}

fn operation_id(command: &MemoryCommandEnvelope, stage: &'static str) -> MemoryOperationId {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, command.command_id().as_str().as_bytes());
    hash_field(&mut hasher, command.memory_id().as_str().as_bytes());
    hash_field(&mut hasher, stage.as_bytes());
    let encoded = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    MemoryOperationId::new(format!("memory-operation:{encoded}"))
        .expect("SHA-256 memory operation IDs are valid")
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        CommandId, ConfidenceBasisPoints, CorrelationId, MemoryContent, MemoryId, MemoryKind,
        MemoryOrigin, MemoryRelation, MemoryRelationKind, MemoryRevisionId, MemoryRevisionNumber,
        MemoryScope, MemorySubjectKey, MemoryValidity, Sha256Digest, TrustClass, UserId,
    };
    use autoharness_memory::normalized_content_hash;
    use autoharness_store_sqlite::SqliteStore;

    use super::*;

    fn scope() -> MemoryScope {
        MemoryScope::User(UserId::new("user-local").expect("user ID"))
    }

    fn draft(
        id: &str,
        revision: u64,
        content: &str,
        origin: MemoryOrigin,
        trust: TrustClass,
    ) -> MemoryRevisionDraft {
        MemoryRevisionDraft::new(
            MemoryRevisionId::new(id).expect("revision ID"),
            MemoryRevisionNumber::new(revision).expect("revision number"),
            None,
            MemoryContent::new(content).expect("content"),
            normalized_content_hash(content).expect("hash"),
            origin,
            trust,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("revision draft")
    }

    fn command(
        id: &str,
        expected: Option<MemorySequence>,
        payload: MemoryCommandPayload,
    ) -> MemoryCommandEnvelope {
        MemoryCommandEnvelope::new_v1(
            CommandId::new(id).expect("command ID"),
            MemoryId::new("memory-1").expect("memory ID"),
            expected,
            CorrelationId::new("correlation-1").expect("correlation ID"),
            payload,
        )
        .expect("memory command")
    }

    fn projected_created(
        revision: &MemoryRevisionDraft,
        status: MemoryRevisionStatus,
    ) -> (Vec<MemoryOperationEnvelope>, Vec<MemoryRevision>) {
        let metadata = MemoryRevision::from_draft(status, revision, TimestampMillis::new(1), None);
        let operation = MemoryOperationEnvelope::new_v1(
            MemoryOperationId::new("operation-created").expect("operation ID"),
            MemoryId::new("memory-1").expect("memory ID"),
            MemorySequence::FIRST,
            TimestampMillis::new(1),
            MemoryCausation::Command(CommandId::new("seed-command").expect("command ID")),
            CorrelationId::new("seed-correlation").expect("correlation ID"),
            MemoryOperationPayload::MemoryCreated {
                scope: scope(),
                memory_kind: MemoryKind::Preference,
                revision: metadata.clone(),
            },
        );
        (vec![operation], vec![metadata])
    }

    #[test]
    fn explicit_create_is_one_atomic_propose_validate_activate_batch() {
        let revision = draft(
            "revision-1",
            1,
            "Prefer concise status updates.",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let command = command(
            "command-create",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Preference,
                revision,
            },
        );

        let plan = plan_memory_command(&command, TimestampMillis::new(10), &[], &[], &[])
            .expect("plan explicit memory");

        assert_eq!(plan.batch().operations().len(), 3);
        assert!(matches!(
            plan.batch().operations()[0].operation().payload(),
            MemoryOperationPayload::MemoryCreated { revision, .. }
                if revision.status() == MemoryRevisionStatus::Proposed
        ));
        assert!(matches!(
            plan.batch().operations()[1].operation().payload(),
            MemoryOperationPayload::RevisionValidated { validation, .. }
                if validation.status() == MemoryValidationStatus::Accepted
        ));
        assert!(matches!(
            plan.batch().operations()[2].operation().payload(),
            MemoryOperationPayload::RevisionActivated { .. }
        ));
        assert_eq!(
            plan.batch()
                .operations()
                .iter()
                .map(|operation| operation.operation().sequence().get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(plan.batch().expected_last_sequence(), 0);
    }

    #[test]
    fn model_proposal_never_receives_activation_authority() {
        let revision = draft(
            "revision-model",
            1,
            "The workspace probably prefers compact output.",
            MemoryOrigin::ModelProposal,
            TrustClass::UntrustedProposal,
        );
        let command = command(
            "command-proposal",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Preference,
                revision,
            },
        );

        let plan = plan_memory_command(&command, TimestampMillis::new(10), &[], &[], &[])
            .expect("plan proposal");

        assert_eq!(plan.batch().operations().len(), 2);
        assert_eq!(
            plan.validation().expect("validation").status(),
            MemoryValidationStatus::NeedsReview
        );
        assert!(plan.batch().operations().iter().all(|append| !matches!(
            append.operation().payload(),
            MemoryOperationPayload::RevisionActivated { .. }
        )));
    }

    #[test]
    fn imported_revision_stays_proposed_until_a_distinct_user_revision_is_approved() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("imported-memory.sqlite3");
        let content = MemoryContent::new("The workspace uses Rust 2024.").expect("content");
        let subject = MemorySubjectKey::new("workspace:rust-edition").expect("subject key");
        let contradiction_id = MemoryId::new("memory-rust-2021").expect("memory ID");
        let imported = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-imported").expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            Some(subject.clone()),
            content.clone(),
            normalized_content_hash(content.as_str()).expect("hash"),
            MemoryOrigin::ImportedDocument,
            TrustClass::Imported,
            ConfidenceBasisPoints::new(8_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            vec![MemoryRelation::new(
                contradiction_id.clone(),
                MemoryRelationKind::Contradicts,
            )],
        )
        .expect("imported revision");
        let create = command(
            "command-import",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Fact,
                revision: imported.clone(),
            },
        );

        let mut store = SqliteStore::open(&database).expect("open store");
        let import_commit = execute_memory_command(&mut store, &create, TimestampMillis::new(10))
            .expect("commit imported proposal");
        assert_eq!(
            import_commit.validation().expect("validation").status(),
            MemoryValidationStatus::NeedsReview
        );
        assert_eq!(
            import_commit.contradiction_candidates(),
            std::slice::from_ref(&contradiction_id)
        );
        let imported_operations = store
            .load_memory_operations(create.memory_id(), 0, DEFAULT_MEMORY_PAGE_SIZE)
            .expect("load imported operations");
        assert!(matches!(
            imported_operations[1].payload(),
            MemoryOperationPayload::RevisionValidated { validation, .. }
                if validation.contradiction_candidates() == [contradiction_id.clone()]
        ));
        assert!(imported_operations.iter().all(|operation| !matches!(
            operation.payload(),
            MemoryOperationPayload::RevisionActivated { .. }
        )));
        assert_eq!(
            store
                .load_memory_revisions(create.memory_id())
                .expect("load imported revision")[0]
                .status(),
            MemoryRevisionStatus::Proposed
        );

        drop(store);
        let mut store = SqliteStore::open(&database).expect("reopen store");
        let reopened_operations = store
            .load_memory_operations(create.memory_id(), 0, DEFAULT_MEMORY_PAGE_SIZE)
            .expect("load reopened operations");
        assert!(matches!(
            reopened_operations[1].payload(),
            MemoryOperationPayload::RevisionValidated { validation, .. }
                if validation.contradiction_candidates() == [contradiction_id]
        ));

        let approved = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-approved-import").expect("revision ID"),
            MemoryRevisionNumber::new(2).expect("revision number"),
            Some(subject),
            content.clone(),
            normalized_content_hash(content.as_str()).expect("hash"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(10_000).expect("confidence"),
            Sensitivity::Internal,
            MemoryValidity::Indefinite,
            Vec::new(),
            Vec::new(),
        )
        .expect("approved revision");
        let approve = command(
            "command-approve-import",
            MemorySequence::new(import_commit.receipt().last_sequence()).ok(),
            MemoryCommandPayload::ApproveProposal {
                proposal_revision_id: imported.revision_id().clone(),
                approved_revision: approved.clone(),
            },
        );
        execute_memory_command(&mut store, &approve, TimestampMillis::new(20))
            .expect("approve imported proposal");

        let revisions = store
            .load_memory_revisions(create.memory_id())
            .expect("load approved revisions");
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0].status(), MemoryRevisionStatus::Superseded);
        assert_eq!(revisions[0].origin(), MemoryOrigin::ImportedDocument);
        assert_eq!(revisions[1].status(), MemoryRevisionStatus::Active);
        assert_eq!(revisions[1].origin(), MemoryOrigin::ExplicitUser);
        assert_ne!(revisions[0].revision_id(), revisions[1].revision_id());
        assert_eq!(revisions[1].revision_id(), approved.revision_id());
    }

    #[test]
    fn proposal_approval_creates_a_distinct_user_revision_before_activation() {
        let proposed = draft(
            "revision-proposed",
            1,
            "Prefer compact output.",
            MemoryOrigin::ModelProposal,
            TrustClass::UntrustedProposal,
        );
        let (operations, revisions) = projected_created(&proposed, MemoryRevisionStatus::Proposed);
        let approved = draft(
            "revision-approved",
            2,
            "Prefer compact output.",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let command = command(
            "command-approve",
            Some(MemorySequence::FIRST),
            MemoryCommandPayload::ApproveProposal {
                proposal_revision_id: proposed.revision_id().clone(),
                approved_revision: approved,
            },
        );

        let plan = plan_memory_command(
            &command,
            TimestampMillis::new(20),
            &operations,
            &revisions,
            &[],
        )
        .expect("plan approval");

        assert_eq!(plan.batch().operations().len(), 4);
        assert!(matches!(
            plan.batch().operations()[0].operation().payload(),
            MemoryOperationPayload::ProposalApproved {
                proposal_revision_id,
                approved_revision,
            } if proposal_revision_id.as_str() == "revision-proposed"
                && approved_revision.revision_id().as_str() == "revision-approved"
                && approved_revision.status() == MemoryRevisionStatus::Proposed
                && approved_revision.supersedes_revision_id() == Some(proposal_revision_id)
        ));
        assert!(matches!(
            plan.batch().operations()[3].operation().payload(),
            MemoryOperationPayload::RevisionSuperseded {
                revision_id,
                superseded_by_revision_id,
            } if revision_id.as_str() == "revision-proposed"
                && superseded_by_revision_id.as_str() == "revision-approved"
        ));
    }

    #[test]
    fn revision_batch_records_the_old_active_transition_explicitly() {
        let current = draft(
            "revision-current",
            1,
            "Prefer detailed output.",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let (operations, revisions) = projected_created(&current, MemoryRevisionStatus::Active);
        let replacement = draft(
            "revision-replacement",
            2,
            "Prefer compact output.",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let command = command(
            "command-revise",
            Some(MemorySequence::FIRST),
            MemoryCommandPayload::ReviseMemory {
                revision: replacement,
                supersedes_revision_id: current.revision_id().clone(),
            },
        );

        let plan = plan_memory_command(
            &command,
            TimestampMillis::new(20),
            &operations,
            &revisions,
            &[],
        )
        .expect("plan revision");

        assert_eq!(plan.batch().operations().len(), 4);
        assert!(matches!(
            plan.batch().operations()[3].operation().payload(),
            MemoryOperationPayload::RevisionSuperseded {
                revision_id,
                superseded_by_revision_id,
            } if revision_id == current.revision_id()
                && superseded_by_revision_id.as_str() == "revision-replacement"
        ));
        assert_eq!(
            plan.batch()
                .operations()
                .iter()
                .map(|operation| operation.operation().sequence().get())
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
    }

    #[test]
    fn secrets_and_stale_sequences_fail_before_any_append_is_planned() {
        let secret = draft(
            "revision-secret",
            1,
            "api_key=do-not-store",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let secret_command = command(
            "command-secret",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Fact,
                revision: secret,
            },
        );
        assert_eq!(
            plan_memory_command(&secret_command, TimestampMillis::new(1), &[], &[], &[]),
            Err(MemoryCommandError::ValidationRejected)
        );

        let current = draft(
            "revision-current",
            1,
            "Current memory",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let (operations, revisions) = projected_created(&current, MemoryRevisionStatus::Active);
        let stale = command(
            "command-stale",
            Some(MemorySequence::new(2).expect("sequence")),
            MemoryCommandPayload::RetractMemory {
                revision_id: current.revision_id().clone(),
            },
        );
        assert_eq!(
            plan_memory_command(
                &stale,
                TimestampMillis::new(2),
                &operations,
                &revisions,
                &[]
            ),
            Err(MemoryCommandError::VersionConflict)
        );
    }

    #[test]
    fn deterministic_stage_ids_make_exact_batch_retries_identical() {
        let revision = draft(
            "revision-stable",
            1,
            "Stable retry bytes",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let command = command(
            "command-stable",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Fact,
                revision,
            },
        );

        let first = plan_memory_command(&command, TimestampMillis::new(5), &[], &[], &[])
            .expect("first plan");
        let second = plan_memory_command(&command, TimestampMillis::new(5), &[], &[], &[])
            .expect("second plan");

        assert_eq!(first, second);
        assert_ne!(
            first.batch().operations()[0].operation().operation_id(),
            first.batch().operations()[1].operation().operation_id()
        );
    }

    #[test]
    fn duplicate_validation_inputs_are_rejected() {
        let revision = draft(
            "revision-duplicate",
            1,
            "Duplicate me",
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
        );
        let existing = ExistingMemory {
            memory_id: MemoryId::new("memory-existing").expect("memory ID"),
            scope: scope(),
            kind: MemoryKind::Fact,
            subject_key: None,
            content_hash: Sha256Digest::new(revision.content_hash().as_str().to_owned())
                .expect("digest"),
        };
        let command = command(
            "command-duplicate",
            None,
            MemoryCommandPayload::CreateMemory {
                scope: scope(),
                memory_kind: MemoryKind::Fact,
                revision,
            },
        );

        assert_eq!(
            plan_memory_command(&command, TimestampMillis::new(5), &[], &[], &[existing]),
            Err(MemoryCommandError::ValidationRejected)
        );
    }
}
