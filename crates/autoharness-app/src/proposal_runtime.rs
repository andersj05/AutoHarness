//! Deterministic application-owned memory proposal construction and verification.

use std::path::Path;

use autoharness_domain::{
    ConfidenceBasisPoints, MemoryCausation, MemoryCommandEnvelope, MemoryCommandPayload,
    MemoryContent, MemoryEvidence, MemoryEvidenceRelation, MemoryEvidenceSource,
    MemoryOperationEnvelope, MemoryOperationPayload, MemoryOrigin, MemoryRevision,
    MemoryRevisionDraft, MemoryRevisionStatus, MemoryScope, MemorySequence, MemoryValidationStatus,
    MemoryValidity, SessionId, Sha256Digest, ToolOutput, TrustClass, WorkspaceId,
};
use autoharness_engine::{SessionAggregate, ToolCallProjection, ToolCallStatus};
use autoharness_memory::normalized_content_hash;
use autoharness_tool::{MemoryProposal, MemoryProposalScope, ToolError};
use sha2::{Digest as _, Sha256};

use crate::ids;

/// Safe classification for a proposal command that could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProposalBuildError {
    /// Model-authored arguments or claimed provenance could not be verified exactly.
    Invalid,
    /// Required durable session state was unexpectedly unavailable.
    Internal,
}

impl ProposalBuildError {
    /// Converts construction failures into the content-free durable tool failure class.
    pub(crate) fn into_tool_error(self) -> ToolError {
        let kind = match self {
            Self::Invalid => autoharness_tool::ToolErrorKind::InvalidCall,
            Self::Internal => autoharness_tool::ToolErrorKind::Internal,
        };
        ToolError::new(kind, autoharness_domain::RetryAdvice::Never)
    }
}

/// Builds the one deterministic, no-authority memory command owned by a proposal tool call.
pub(crate) fn build_memory_proposal_command(
    session: &SessionAggregate,
    session_id: &SessionId,
    workspace_id: &WorkspaceId,
    artifact_root: Option<&Path>,
    call: &ToolCallProjection,
    proposal: &MemoryProposal,
) -> Result<MemoryCommandEnvelope, ProposalBuildError> {
    let scope = match proposal.scope() {
        MemoryProposalScope::Session => MemoryScope::Session(session_id.clone()),
        MemoryProposalScope::Workspace => MemoryScope::Workspace(workspace_id.clone()),
    };
    let (origin, trust, evidence) = match proposal.source_provider_call_id() {
        Some(provider_call_id) => (
            MemoryOrigin::VerifiedTool,
            TrustClass::VerifiedObservation,
            verified_tool_evidence(session, session_id, artifact_root, call, provider_call_id)?,
        ),
        None => (
            MemoryOrigin::ModelProposal,
            TrustClass::UntrustedProposal,
            current_input_evidence(session, session_id, call)?,
        ),
    };
    let content_hash = normalized_content_hash(proposal.content().as_str())
        .map_err(|_| ProposalBuildError::Invalid)?;
    let revision = MemoryRevisionDraft::new(
        ids::memory_proposal_revision_id(&call.call().tool_call_id),
        autoharness_domain::MemoryRevisionNumber::FIRST,
        None,
        proposal.content().clone(),
        content_hash,
        origin,
        trust,
        ConfidenceBasisPoints::new(9_000).expect("static proposal confidence is valid"),
        proposal.sensitivity(),
        MemoryValidity::Indefinite,
        vec![evidence],
        Vec::new(),
    )
    .map_err(|_| ProposalBuildError::Invalid)?;
    Ok(ids::memory_proposal_command(
        &call.call().tool_call_id,
        MemoryCommandPayload::CreateMemory {
            scope,
            memory_kind: proposal.memory_kind(),
            revision,
        },
    ))
}

fn current_input_evidence(
    session: &SessionAggregate,
    session_id: &SessionId,
    call: &ToolCallProjection,
) -> Result<MemoryEvidence, ProposalBuildError> {
    let attempt = session
        .attempt(call.attempt_id())
        .ok_or(ProposalBuildError::Internal)?;
    let input = session
        .admitted_inputs()
        .iter()
        .find(|input| {
            input.input_id() == attempt.input_id() && input.promoted_by() == Some(call.attempt_id())
        })
        .ok_or(ProposalBuildError::Internal)?;
    MemoryEvidence::new(
        ids::memory_proposal_evidence_id(&call.call().tool_call_id),
        MemoryEvidenceSource::UserInput {
            session_id: session_id.clone(),
            input_id: input.input_id().clone(),
        },
        MemoryEvidenceRelation::DerivedFrom,
        None,
        None,
    )
    .map_err(|_| ProposalBuildError::Internal)
}

fn verified_tool_evidence(
    session: &SessionAggregate,
    session_id: &SessionId,
    artifact_root: Option<&Path>,
    proposal_call: &ToolCallProjection,
    provider_call_id: &autoharness_domain::ProviderCallId,
) -> Result<MemoryEvidence, ProposalBuildError> {
    let mut matching = session.tool_calls().iter().filter(|candidate| {
        candidate.call().provider_call_id == *provider_call_id
            && candidate.call().tool_call_id != proposal_call.call().tool_call_id
    });
    let source = matching.next().ok_or(ProposalBuildError::Invalid)?;
    if matching.next().is_some()
        || source.status() != ToolCallStatus::Completed
        || matches!(
            source.call().capability.kind,
            autoharness_domain::CapabilityKind::InvalidToolCall
                | autoharness_domain::CapabilityKind::MemoryProposal
        )
    {
        return Err(ProposalBuildError::Invalid);
    }
    let output = source.output().ok_or(ProposalBuildError::Invalid)?;
    let output_hash = exact_tool_output_hash(output, artifact_root)?;
    MemoryEvidence::new(
        ids::memory_proposal_evidence_id(&proposal_call.call().tool_call_id),
        MemoryEvidenceSource::ToolObservation {
            session_id: session_id.clone(),
            tool_call_id: source.call().tool_call_id.clone(),
            output_hash,
        },
        MemoryEvidenceRelation::DerivedFrom,
        None,
        None,
    )
    .map_err(|_| ProposalBuildError::Internal)
}

/// Hashes exact inline output or verifies and hashes the complete artifact bytes.
pub(crate) fn exact_tool_output_hash(
    output: &ToolOutput,
    artifact_root: Option<&Path>,
) -> Result<Sha256Digest, ProposalBuildError> {
    match output.artifact() {
        None if !output.truncated()
            && output.original_bytes()
                == u64::try_from(output.content().len()).unwrap_or(u64::MAX) =>
        {
            raw_sha256(output.content().as_bytes())
        }
        Some(artifact) if output.truncated() => {
            let digest = artifact
                .artifact_id
                .as_str()
                .strip_prefix("sha256:")
                .filter(|digest| {
                    digest.len() == 64
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
                .ok_or(ProposalBuildError::Invalid)?;
            let root = artifact_root.ok_or(ProposalBuildError::Invalid)?;
            let bytes =
                std::fs::read(root.join(digest)).map_err(|_| ProposalBuildError::Invalid)?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != artifact.byte_len
                || artifact.byte_len != output.original_bytes()
                || !bytes.starts_with(output.content().as_bytes())
            {
                return Err(ProposalBuildError::Invalid);
            }
            let verified = raw_sha256(&bytes)?;
            if verified.as_str() != digest {
                return Err(ProposalBuildError::Invalid);
            }
            Ok(verified)
        }
        None | Some(_) => Err(ProposalBuildError::Invalid),
    }
}

fn raw_sha256(bytes: &[u8]) -> Result<Sha256Digest, ProposalBuildError> {
    let digest = Sha256::digest(bytes);
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Sha256Digest::new(encoded).map_err(|_| ProposalBuildError::Internal)
}

/// Proves that one deterministic proposal command committed exactly and never activated.
pub(crate) fn exact_memory_proposal_committed(
    command: &MemoryCommandEnvelope,
    operations: &[MemoryOperationEnvelope],
    content: Option<&MemoryContent>,
) -> bool {
    let MemoryCommandPayload::CreateMemory {
        scope,
        memory_kind,
        revision,
    } = command.payload()
    else {
        return false;
    };
    let [created, validated, ..] = operations else {
        return false;
    };
    let expected_revision = MemoryRevision::from_draft(
        MemoryRevisionStatus::Proposed,
        revision,
        created.occurred_at(),
        None,
    );
    let created_matches = created.memory_id() == command.memory_id()
        && created.sequence() == MemorySequence::FIRST
        && created.correlation_id() == command.correlation_id()
        && created.causation() == &MemoryCausation::Command(command.command_id().clone())
        && matches!(
            created.payload(),
            MemoryOperationPayload::MemoryCreated {
                scope: actual_scope,
                memory_kind: actual_kind,
                revision: actual_revision,
            } if actual_scope == scope
                && actual_kind == memory_kind
                && actual_revision == &expected_revision
        );
    let validation_matches = validated.memory_id() == command.memory_id()
        && validated.sequence().get() == 2
        && validated.correlation_id() == command.correlation_id()
        && validated.causation() == &MemoryCausation::Operation(created.operation_id().clone())
        && matches!(
            validated.payload(),
            MemoryOperationPayload::RevisionValidated {
                revision_id,
                validation,
            } if revision_id == revision.revision_id()
                && validation.content_hash() == revision.content_hash()
                && validation.status() == MemoryValidationStatus::NeedsReview
        );
    let never_activated = operations.iter().all(|operation| {
        !matches!(
            operation.payload(),
            MemoryOperationPayload::RevisionActivated { revision_id }
                if revision_id == revision.revision_id()
        )
    });
    created_matches && validation_matches && never_activated && content == Some(revision.content())
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{ArtifactId, ArtifactRef};

    use super::*;

    #[test]
    fn artifact_hash_requires_exact_full_bytes_length_digest_and_inline_prefix() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let original = b"verified complete artifact bytes";
        let digest = raw_sha256(original).expect("artifact digest");
        std::fs::write(directory.path().join(digest.as_str()), original).expect("artifact fixture");
        let artifact = ArtifactRef::new(
            ArtifactId::new(format!("sha256:{}", digest.as_str())).expect("artifact ID"),
            u64::try_from(original.len()).expect("artifact length"),
            "text/plain",
        )
        .expect("artifact reference");
        let output = ToolOutput::new(
            "verified complete",
            Some(artifact.clone()),
            artifact.byte_len,
            true,
        )
        .expect("truncated output");

        assert_eq!(
            exact_tool_output_hash(&output, Some(directory.path())),
            Ok(digest.clone())
        );

        std::fs::write(
            directory.path().join(digest.as_str()),
            b"tampered complete artifact bytes",
        )
        .expect("tamper artifact");
        assert_eq!(
            exact_tool_output_hash(&output, Some(directory.path())),
            Err(ProposalBuildError::Invalid)
        );

        std::fs::write(directory.path().join(digest.as_str()), original).expect("restore artifact");
        let forged_prefix = ToolOutput::new(
            "forged inline data",
            Some(artifact),
            u64::try_from(original.len()).expect("artifact length"),
            true,
        )
        .expect("forged output shape");
        assert_eq!(
            exact_tool_output_hash(&forged_prefix, Some(directory.path())),
            Err(ProposalBuildError::Invalid)
        );
    }
}
