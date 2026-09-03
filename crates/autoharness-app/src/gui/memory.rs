//! Explicit temporary adapter from runtime memory projections to the public client.
use super::GuiIpcError;
use autoharness_client as client;
use autoharness_tui as tui;

pub(super) fn map_command(
    command: client::MemoryCommand,
    request_id: tui::RequestId,
) -> Result<tui::UiIntent, GuiIpcError> {
    use client::MemoryCommand as C;
    let invalid = |_| GuiIpcError::invalid_command();
    Ok(match command {
        C::Query(query) => tui::UiIntent::QueryMemory {
            request_id,
            view_generation: query.view_generation.get(),
            query: tui::MemoryViewQuery::new(
                query.literal.as_str(),
                map_memory_status_filter(query.status),
                map_memory_scope_filter(query.scope),
                map_memory_page_direction(query.direction),
                query
                    .before
                    .map(|value| tui::MemoryViewCursor::new(value.as_str()))
                    .transpose()
                    .map_err(invalid)?,
                tui::MEMORY_VIEW_PAGE_SIZE,
            )
            .map_err(invalid)?,
        },
        C::Remember { content } => tui::UiIntent::RememberMemory {
            request_id,
            content: tui::MemoryContent::new(content.as_str()).map_err(invalid)?,
        },
        C::Import { path } => tui::UiIntent::ImportMemory {
            request_id,
            path: tui::MemoryImportPath::new(path.as_str()).map_err(invalid)?,
        },
        C::Revise {
            memory_id,
            expected_last_sequence,
            content,
        } => tui::UiIntent::ReviseMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            content: tui::MemoryContent::new(content.as_str()).map_err(invalid)?,
        },
        C::Approve {
            memory_id,
            expected_last_sequence,
            proposal_revision_id,
        } => tui::UiIntent::ApproveMemoryProposal {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            proposal_revision_id: proposal_revision_id.into_inner(),
        },
        C::Reject {
            memory_id,
            expected_last_sequence,
            proposal_revision_id,
        } => tui::UiIntent::RejectMemoryProposal {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            proposal_revision_id: proposal_revision_id.into_inner(),
        },
        C::Retract {
            memory_id,
            expected_last_sequence,
            revision_id,
        } => tui::UiIntent::RetractMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
            revision_id: revision_id.into_inner(),
        },
        C::Delete {
            memory_id,
            expected_last_sequence,
        } => tui::UiIntent::DeleteMemory {
            request_id,
            memory_id: memory_id.into_inner(),
            expected_last_sequence: expected_last_sequence.get(),
        },
        C::Export { memory_id } => tui::UiIntent::ExportMemory {
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
    source: &tui::MemoryProjection,
) -> Result<client::MemoryProjection, GuiIpcError> {
    let page = client::MemoryProjection {
        view_generation: source.view_generation().into(),
        generation: source.generation().into(),
        state: match source.state() {
            tui::MemoryLoadState::Ready => client::MemoryLoadState::Ready,
            tui::MemoryLoadState::Loading => client::MemoryLoadState::Loading,
            tui::MemoryLoadState::Failed(failure) => client::MemoryLoadState::Failed {
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

fn map_detail(detail: &tui::MemoryDetail) -> Result<client::MemoryDetail, GuiIpcError> {
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

fn map_memory_status(value: tui::MemoryStatus) -> client::MemoryStatus {
    match value {
        tui::MemoryStatus::Active => client::MemoryStatus::Active,
        tui::MemoryStatus::Proposed => client::MemoryStatus::Proposed,
        tui::MemoryStatus::Conflicting => client::MemoryStatus::Conflicting,
        tui::MemoryStatus::Superseded => client::MemoryStatus::Superseded,
        tui::MemoryStatus::Rejected => client::MemoryStatus::Rejected,
        tui::MemoryStatus::Retracted => client::MemoryStatus::Retracted,
        tui::MemoryStatus::Expired => client::MemoryStatus::Expired,
        tui::MemoryStatus::Deleted => client::MemoryStatus::Deleted,
    }
}

fn map_memory_scope(value: tui::MemoryScope) -> client::MemoryScope {
    match value {
        tui::MemoryScope::User => client::MemoryScope::User,
        tui::MemoryScope::Workspace => client::MemoryScope::Workspace,
        tui::MemoryScope::Session => client::MemoryScope::Session,
        tui::MemoryScope::Agent => client::MemoryScope::Agent,
    }
}

fn map_memory_trust(value: tui::MemoryTrust) -> client::MemoryTrust {
    match value {
        tui::MemoryTrust::UserApproved => client::MemoryTrust::UserApproved,
        tui::MemoryTrust::VerifiedObservation => client::MemoryTrust::VerifiedObservation,
        tui::MemoryTrust::Imported => client::MemoryTrust::Imported,
        tui::MemoryTrust::UntrustedProposal => client::MemoryTrust::UntrustedProposal,
    }
}

fn map_memory_origin(value: tui::MemoryOrigin) -> client::MemoryOrigin {
    match value {
        tui::MemoryOrigin::ExplicitUser => client::MemoryOrigin::ExplicitUser,
        tui::MemoryOrigin::VerifiedTool => client::MemoryOrigin::VerifiedTool,
        tui::MemoryOrigin::ImportedDocument => client::MemoryOrigin::ImportedDocument,
        tui::MemoryOrigin::ModelProposal => client::MemoryOrigin::ModelProposal,
        tui::MemoryOrigin::Compaction => client::MemoryOrigin::Compaction,
    }
}

fn map_memory_sensitivity(value: tui::MemorySensitivity) -> client::MemorySensitivity {
    match value {
        tui::MemorySensitivity::Public => client::MemorySensitivity::Public,
        tui::MemorySensitivity::Internal => client::MemorySensitivity::Internal,
        tui::MemorySensitivity::Sensitive => client::MemorySensitivity::Sensitive,
        tui::MemorySensitivity::Secret => client::MemorySensitivity::Secret,
    }
}

fn map_memory_evidence_availability(
    value: tui::MemoryEvidenceAvailability,
) -> client::MemoryEvidenceAvailability {
    match value {
        tui::MemoryEvidenceAvailability::Retained => client::MemoryEvidenceAvailability::Retained,
        tui::MemoryEvidenceAvailability::Absent => client::MemoryEvidenceAvailability::Absent,
        tui::MemoryEvidenceAvailability::Erased => client::MemoryEvidenceAvailability::Erased,
    }
}

fn map_memory_relation_kind(value: tui::MemoryRelationKind) -> client::MemoryRelationKind {
    match value {
        tui::MemoryRelationKind::DuplicateOf => client::MemoryRelationKind::DuplicateOf,
        tui::MemoryRelationKind::Contradicts => client::MemoryRelationKind::Contradicts,
        tui::MemoryRelationKind::Refines => client::MemoryRelationKind::Refines,
        tui::MemoryRelationKind::Supersedes => client::MemoryRelationKind::Supersedes,
        tui::MemoryRelationKind::Related => client::MemoryRelationKind::Related,
        tui::MemoryRelationKind::DerivedFrom => client::MemoryRelationKind::DerivedFrom,
    }
}

fn map_memory_finding_kind(value: tui::MemoryFindingKind) -> client::MemoryFindingKind {
    match value {
        tui::MemoryFindingKind::Duplicate => client::MemoryFindingKind::Duplicate,
        tui::MemoryFindingKind::Contradiction => client::MemoryFindingKind::Contradiction,
        tui::MemoryFindingKind::SecretDetected => client::MemoryFindingKind::SecretDetected,
        tui::MemoryFindingKind::UnsupportedScope => client::MemoryFindingKind::UnsupportedScope,
        tui::MemoryFindingKind::MalformedContent => client::MemoryFindingKind::MalformedContent,
        tui::MemoryFindingKind::PolicyConflict => client::MemoryFindingKind::PolicyConflict,
        tui::MemoryFindingKind::InjectionPattern => client::MemoryFindingKind::InjectionPattern,
        tui::MemoryFindingKind::UngroundedEvidence => client::MemoryFindingKind::UngroundedEvidence,
    }
}

fn map_memory_status_filter(value: client::MemoryStatusFilter) -> tui::MemoryStatusFilter {
    match value {
        client::MemoryStatusFilter::Eligible => tui::MemoryStatusFilter::Eligible,
        client::MemoryStatusFilter::All => tui::MemoryStatusFilter::All,
        client::MemoryStatusFilter::Active => tui::MemoryStatusFilter::Active,
        client::MemoryStatusFilter::Proposed => tui::MemoryStatusFilter::Proposed,
        client::MemoryStatusFilter::Inactive => tui::MemoryStatusFilter::Inactive,
    }
}

fn map_memory_scope_filter(value: client::MemoryScopeFilter) -> tui::MemoryScopeFilter {
    match value {
        client::MemoryScopeFilter::All => tui::MemoryScopeFilter::All,
        client::MemoryScopeFilter::User => tui::MemoryScopeFilter::User,
        client::MemoryScopeFilter::Workspace => tui::MemoryScopeFilter::Workspace,
        client::MemoryScopeFilter::Session => tui::MemoryScopeFilter::Session,
        client::MemoryScopeFilter::Agent => tui::MemoryScopeFilter::Agent,
    }
}

fn map_memory_page_direction(value: client::MemoryPageDirection) -> tui::MemoryPageDirection {
    match value {
        client::MemoryPageDirection::First => tui::MemoryPageDirection::First,
        client::MemoryPageDirection::Next => tui::MemoryPageDirection::Next,
        client::MemoryPageDirection::Previous => tui::MemoryPageDirection::Previous,
    }
}
