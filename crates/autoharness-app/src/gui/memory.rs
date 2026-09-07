//! Explicit temporary adapter from runtime memory projections to the public client.
use super::GuiIpcError;
use autoharness_client as client;
use autoharness_client::runtime as host;

pub(super) fn map_command(
    command: client::MemoryCommand,
    request_id: host::RequestId,
) -> Result<host::UiIntent, GuiIpcError> {
    use client::MemoryCommand as C;
    let invalid = |_| GuiIpcError::invalid_command();
    Ok(match command {
        C::Query(query) => host::UiIntent::QueryMemory {
            request_id,
            view_generation: query.view_generation.get(),
            query: host::MemoryViewQuery::new(
                query.literal.as_str(),
                map_memory_status_filter(query.status),
                map_memory_scope_filter(query.scope),
                map_memory_page_direction(query.direction),
                query
                    .before
                    .map(|value| host::MemoryViewCursor::new(value.as_str()))
                    .transpose()
                    .map_err(invalid)?,
                host::MEMORY_VIEW_PAGE_SIZE,
            )
            .map_err(invalid)?,
        },
        C::Remember { content } => host::UiIntent::RememberMemory {
            request_id,
            content: host::MemoryContent::new(content.as_str()).map_err(invalid)?,
        },
        C::Import { path } => host::UiIntent::ImportMemory {
            request_id,
            path: host::MemoryImportPath::new(path.as_str()).map_err(invalid)?,
        },
        C::Revise {
            memory_id,
            expected_last_sequence,
            content,
        } => host::UiIntent::ReviseMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            content: host::MemoryContent::new(content.as_str()).map_err(invalid)?,
        },
        C::Approve {
            memory_id,
            expected_last_sequence,
            proposal_revision_id,
        } => host::UiIntent::ApproveMemoryProposal {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            proposal_revision_id: proposal_revision_id.into_inner(),
        },
        C::Reject {
            memory_id,
            expected_last_sequence,
            proposal_revision_id,
        } => host::UiIntent::RejectMemoryProposal {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            proposal_revision_id: proposal_revision_id.into_inner(),
        },
        C::Retract {
            memory_id,
            expected_last_sequence,
            revision_id,
        } => host::UiIntent::RetractMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            revision_id: revision_id.into_inner(),
        },
        C::Delete {
            memory_id,
            expected_last_sequence,
        } => host::UiIntent::DeleteMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
        },
        C::Export { memory_id } => host::UiIntent::ExportMemory {
            request_id,
            memory_id: memory_id.into_inner(),
        },
    })
}

fn text(value: &str) -> Result<client::MemoryText, GuiIpcError> {
    client::MemoryText::new(value).map_err(|_| GuiIpcError::invalid_projection())
}
fn id(value: &str) -> Result<client::MemoryId, GuiIpcError> {
    client::MemoryId::new(value).map_err(|_| GuiIpcError::invalid_projection())
}

pub(super) fn map_projection(
    source: &host::MemoryProjection,
) -> Result<client::MemoryProjection, GuiIpcError> {
    match try_map_projection(source) {
        Ok(page) => Ok(page),
        Err(_) => Ok(client::MemoryProjection {
            view_generation: source.view_generation().into(), generation: source.generation().into(),
            state: client::MemoryLoadState::Failed { failure: client::SafeFailure::new(
                client::FailureClass::Unavailable, "memory_page_unrepresentable",
                "This memory page exceeds display limits. Narrow the search or scope and try again.",
                client::RetryDirective::Immediate).map_err(|_| GuiIpcError::invalid_projection())? },
            ..Default::default()
        }),
    }
}

fn try_map_projection(
    source: &host::MemoryProjection,
) -> Result<client::MemoryProjection, GuiIpcError> {
    let page = client::MemoryProjection {
        view_generation: source.view_generation().into(),
        generation: source.generation().into(),
        state: match source.state() {
            host::MemoryLoadState::Ready => client::MemoryLoadState::Ready,
            host::MemoryLoadState::Loading => client::MemoryLoadState::Loading,
            host::MemoryLoadState::Failed(failure) => client::MemoryLoadState::Failed {
                failure: super::map_failure(failure)?,
            },
        },
        rows: source
            .summaries()
            .iter()
            .map(|row| {
                Ok(client::MemoryRow {
                    memory_id: id(row.id())?,
                    preview: text(row.preview())?,
                    status: map_memory_status(row.status()),
                    scope: map_memory_scope(row.scope()),
                    updated_at_ms: client::UnixMillis::new(row.updated_at_ms()),
                    confidence_bps: row.confidence_bps(),
                    admission_count: row.admission_count(),
                    detail: source.detail(row.id()).map(map_detail).transpose()?,
                })
            })
            .collect::<Result<_, GuiIpcError>>()?,
        total: source.total(),
        stale: source.stale(),
        next_cursor: source
            .next_cursor()
            .map(|cursor| text(cursor.as_str()))
            .transpose()?,
    };
    page.validate()
        .map_err(|_| GuiIpcError::invalid_projection())?;
    Ok(page)
}

fn map_detail(detail: &host::MemoryDetail) -> Result<client::MemoryDetail, GuiIpcError> {
    Ok(client::MemoryDetail {
        revision: detail.revision(),
        content: detail
            .has_content()
            .then(|| text(detail.content()))
            .transpose()?,
        source: text(detail.source())?,
        trust: map_memory_trust(detail.trust()),
        created_at_ms: client::UnixMillis::new(detail.created_at_ms()),
        valid_until_ms: detail.valid_until_ms().map(client::UnixMillis::new),
        admissions: detail
            .admissions()
            .iter()
            .map(|a| {
                Ok(client::MemoryAdmission {
                    session: text(a.session())?,
                    model: text(a.model())?,
                    reason: text(a.reason())?,
                    admitted_at_ms: client::UnixMillis::new(a.admitted_at_ms()),
                    rank: a.rank(),
                    context: a
                        .context()
                        .map(|c| {
                            Ok(client::MemoryAdmissionContext {
                                provider_attempt: text(c.provider_attempt())?,
                                run_turn: c.run_turn(),
                                epoch: text(c.epoch())?,
                                token_count: c.token_count(),
                                source_revision: text(c.source_revision())?,
                                renderer_version: text(c.renderer_version())?,
                                reason_factors: c
                                    .reason_factors()
                                    .iter()
                                    .map(|value| text(value))
                                    .collect::<Result<_, GuiIpcError>>()?,
                            })
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<_, GuiIpcError>>()?,
        revision_context: detail
            .revision_context()
            .map(|c| {
                Ok(client::MemoryRevisionContext {
                    expected_last_sequence: c.expected_last_sequence().into(),
                    revision_id: id(c.revision_id())?,
                    proposal_revision_id: c.proposal_revision_id().map(id).transpose()?,
                    scope_identity: text(c.scope_identity())?,
                    origin: map_memory_origin(c.origin()),
                    sensitivity: map_memory_sensitivity(c.sensitivity()),
                    evidence: c
                        .evidence()
                        .iter()
                        .map(|e| {
                            Ok(client::MemoryEvidence {
                                label: text(e.label())?,
                                source: text(e.source())?,
                                excerpt: e.retained_excerpt().map(text).transpose()?,
                                availability: map_memory_evidence_availability(e.availability()),
                            })
                        })
                        .collect::<Result<_, GuiIpcError>>()?,
                    relations: c
                        .relations()
                        .iter()
                        .map(|r| {
                            Ok(client::MemoryRelation {
                                kind: map_memory_relation_kind(r.kind()),
                                memory_id: id(r.memory_id())?,
                            })
                        })
                        .collect::<Result<_, GuiIpcError>>()?,
                    findings: c
                        .findings()
                        .iter()
                        .map(|f| {
                            Ok(client::MemoryFinding {
                                kind: map_memory_finding_kind(f.kind()),
                                related_memory_id: text(f.related_memory_id())?,
                                summary: text(f.summary())?,
                            })
                        })
                        .collect::<Result<_, GuiIpcError>>()?,
                })
            })
            .transpose()?,
    })
}

fn map_memory_status(value: host::MemoryStatus) -> client::MemoryStatus {
    match value {
        host::MemoryStatus::Active => client::MemoryStatus::Active,
        host::MemoryStatus::Proposed => client::MemoryStatus::Proposed,
        host::MemoryStatus::Conflicting => client::MemoryStatus::Conflicting,
        host::MemoryStatus::Superseded => client::MemoryStatus::Superseded,
        host::MemoryStatus::Rejected => client::MemoryStatus::Rejected,
        host::MemoryStatus::Retracted => client::MemoryStatus::Retracted,
        host::MemoryStatus::Expired => client::MemoryStatus::Expired,
        host::MemoryStatus::Deleted => client::MemoryStatus::Deleted,
    }
}

fn map_memory_scope(value: host::MemoryScope) -> client::MemoryScope {
    match value {
        host::MemoryScope::User => client::MemoryScope::User,
        host::MemoryScope::Workspace => client::MemoryScope::Workspace,
        host::MemoryScope::Session => client::MemoryScope::Session,
        host::MemoryScope::Agent => client::MemoryScope::Agent,
    }
}

fn map_memory_trust(value: host::MemoryTrust) -> client::MemoryTrust {
    match value {
        host::MemoryTrust::UserApproved => client::MemoryTrust::UserApproved,
        host::MemoryTrust::VerifiedObservation => client::MemoryTrust::VerifiedObservation,
        host::MemoryTrust::Imported => client::MemoryTrust::Imported,
        host::MemoryTrust::UntrustedProposal => client::MemoryTrust::UntrustedProposal,
    }
}

fn map_memory_origin(value: host::MemoryOrigin) -> client::MemoryOrigin {
    match value {
        host::MemoryOrigin::ExplicitUser => client::MemoryOrigin::ExplicitUser,
        host::MemoryOrigin::VerifiedTool => client::MemoryOrigin::VerifiedTool,
        host::MemoryOrigin::ImportedDocument => client::MemoryOrigin::ImportedDocument,
        host::MemoryOrigin::ModelProposal => client::MemoryOrigin::ModelProposal,
        host::MemoryOrigin::Compaction => client::MemoryOrigin::Compaction,
    }
}

fn map_memory_sensitivity(value: host::MemorySensitivity) -> client::MemorySensitivity {
    match value {
        host::MemorySensitivity::Public => client::MemorySensitivity::Public,
        host::MemorySensitivity::Internal => client::MemorySensitivity::Internal,
        host::MemorySensitivity::Sensitive => client::MemorySensitivity::Sensitive,
        host::MemorySensitivity::Secret => client::MemorySensitivity::Secret,
    }
}

fn map_memory_evidence_availability(
    value: host::MemoryEvidenceAvailability,
) -> client::MemoryEvidenceAvailability {
    match value {
        host::MemoryEvidenceAvailability::Retained => client::MemoryEvidenceAvailability::Retained,
        host::MemoryEvidenceAvailability::Absent => client::MemoryEvidenceAvailability::Absent,
        host::MemoryEvidenceAvailability::Erased => client::MemoryEvidenceAvailability::Erased,
    }
}

fn map_memory_relation_kind(value: host::MemoryRelationKind) -> client::MemoryRelationKind {
    match value {
        host::MemoryRelationKind::DuplicateOf => client::MemoryRelationKind::DuplicateOf,
        host::MemoryRelationKind::Contradicts => client::MemoryRelationKind::Contradicts,
        host::MemoryRelationKind::Refines => client::MemoryRelationKind::Refines,
        host::MemoryRelationKind::Supersedes => client::MemoryRelationKind::Supersedes,
        host::MemoryRelationKind::Related => client::MemoryRelationKind::Related,
        host::MemoryRelationKind::DerivedFrom => client::MemoryRelationKind::DerivedFrom,
    }
}

fn map_memory_finding_kind(value: host::MemoryFindingKind) -> client::MemoryFindingKind {
    match value {
        host::MemoryFindingKind::Duplicate => client::MemoryFindingKind::Duplicate,
        host::MemoryFindingKind::Contradiction => client::MemoryFindingKind::Contradiction,
        host::MemoryFindingKind::SecretDetected => client::MemoryFindingKind::SecretDetected,
        host::MemoryFindingKind::UnsupportedScope => client::MemoryFindingKind::UnsupportedScope,
        host::MemoryFindingKind::MalformedContent => client::MemoryFindingKind::MalformedContent,
        host::MemoryFindingKind::PolicyConflict => client::MemoryFindingKind::PolicyConflict,
        host::MemoryFindingKind::InjectionPattern => client::MemoryFindingKind::InjectionPattern,
        host::MemoryFindingKind::UngroundedEvidence => {
            client::MemoryFindingKind::UngroundedEvidence
        }
    }
}

fn map_memory_status_filter(value: client::MemoryStatusFilter) -> host::MemoryStatusFilter {
    match value {
        client::MemoryStatusFilter::Eligible => host::MemoryStatusFilter::Eligible,
        client::MemoryStatusFilter::All => host::MemoryStatusFilter::All,
        client::MemoryStatusFilter::Active => host::MemoryStatusFilter::Active,
        client::MemoryStatusFilter::Proposed => host::MemoryStatusFilter::Proposed,
        client::MemoryStatusFilter::Inactive => host::MemoryStatusFilter::Inactive,
    }
}

fn map_memory_scope_filter(value: client::MemoryScopeFilter) -> host::MemoryScopeFilter {
    match value {
        client::MemoryScopeFilter::All => host::MemoryScopeFilter::All,
        client::MemoryScopeFilter::User => host::MemoryScopeFilter::User,
        client::MemoryScopeFilter::Workspace => host::MemoryScopeFilter::Workspace,
        client::MemoryScopeFilter::Session => host::MemoryScopeFilter::Session,
        client::MemoryScopeFilter::Agent => host::MemoryScopeFilter::Agent,
    }
}

fn map_memory_page_direction(value: client::MemoryPageDirection) -> host::MemoryPageDirection {
    match value {
        client::MemoryPageDirection::First => host::MemoryPageDirection::First,
        client::MemoryPageDirection::Next => host::MemoryPageDirection::Next,
        client::MemoryPageDirection::Previous => host::MemoryPageDirection::Previous,
    }
}
