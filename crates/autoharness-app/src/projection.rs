use autoharness_domain::{
    ContextAdmissionFactor, ErrorClass, MemoryEvidenceRelation, MemoryEvidenceSource, MemoryOrigin,
    MemoryRevisionStatus, MemoryScope as DomainMemoryScope, MemoryValidationIssue, MemoryValidity,
    ModelRef, TimestampMillis, TrustClass,
};
use autoharness_engine::{AttemptStatus as EngineAttemptStatus, SessionAggregate};
use autoharness_provider::{CapabilitySupport, ModelDescriptor};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, ModelSummary, PermissionDetailView,
    PermissionRequestView, RetryPolicy, SessionProjection, ToolCallKey, ToolRowView,
    TranscriptItem, UiFailure, UsageView,
};
use autoharness_tui::{
    MemoryAdmission, MemoryAdmissionContext, MemoryDetail, MemoryEvidence as UiMemoryEvidence,
    MemoryFindingKind, MemoryOrigin as UiMemoryOrigin, MemoryProjection,
    MemoryRelation as UiMemoryRelation, MemoryRelationKind as UiMemoryRelationKind,
    MemoryRevisionContext, MemoryScope, MemorySensitivity, MemoryStatus, MemorySummary,
    MemoryTrust, MemoryValidationFinding,
};

const MEMORY_PROJECTION_PAGE_SIZE: u32 = 100;
const MEMORY_ADMISSION_PAGE_SIZE: u32 = 64;

/// Converts the authoritative aggregate into the complete visible TUI state.
#[must_use]
pub fn session(aggregate: &SessionAggregate) -> SessionProjection {
    let mut transcript = Vec::new();
    for input in aggregate.admitted_inputs() {
        transcript.push(TranscriptItem::User {
            input_id: input.input_id().as_str().to_owned(),
            text: input.prompt().as_str().to_owned(),
        });
        for attempt in aggregate
            .attempts()
            .iter()
            .filter(|attempt| attempt.input_id() == input.input_id())
        {
            let attempt_id = AttemptKey::new(attempt.attempt_id().as_str())
                .expect("domain attempt IDs are non-empty");
            let retry_of = attempt
                .retry_of()
                .map(|prior| AttemptKey::new(prior.as_str()).expect("domain IDs are non-empty"));
            let status = match attempt.status() {
                EngineAttemptStatus::Prepared
                | EngineAttemptStatus::InFlight
                | EngineAttemptStatus::AwaitingTools => AttemptStatus::Streaming,
                EngineAttemptStatus::CancellationRequested => AttemptStatus::Cancelling,
                EngineAttemptStatus::Completed => AttemptStatus::Completed,
                EngineAttemptStatus::Cancelled => AttemptStatus::Cancelled,
                EngineAttemptStatus::Failed => AttemptStatus::Failed(
                    attempt
                        .failure()
                        .map_or_else(interrupted_failure, |failure| {
                            UiFailure::new(
                                failure.class(),
                                failure.message().as_str(),
                                RetryPolicy::from_advice(failure.retry_advice(), 0),
                            )
                            .with_code(failure.code().as_str())
                        }),
                ),
                EngineAttemptStatus::Unknown => AttemptStatus::Failed(interrupted_failure()),
            };
            let usage = attempt.usage().map(|usage| UsageView {
                input_tokens: usage.input_tokens().unwrap_or_default(),
                output_tokens: usage.output_tokens().unwrap_or_default(),
            });
            for call in aggregate
                .tool_calls()
                .iter()
                .filter(|call| call.attempt_id() == attempt.attempt_id())
            {
                transcript.push(TranscriptItem::Tool(ToolRowView {
                    tool_call_id: ToolCallKey::new(call.call().tool_call_id.as_str())
                        .expect("domain tool-call IDs are non-empty"),
                    tool_name: call.call().tool_name.as_str().to_owned(),
                    resource: call.call().capability.resource.as_str().to_owned(),
                    status: tool_status(call.status()).to_owned(),
                    summary: tool_summary(call),
                }));
            }
            transcript.push(TranscriptItem::Assistant {
                attempt_id,
                text: attempt.response_text(),
                status,
                usage,
                retry_of,
            });
        }
    }

    let permission_requests = aggregate
        .tool_calls()
        .iter()
        .filter(|call| call.status() == autoharness_engine::ToolCallStatus::PermissionPending)
        .map(|call| PermissionRequestView {
            tool_call_id: ToolCallKey::new(call.call().tool_call_id.as_str())
                .expect("domain tool-call IDs are non-empty"),
            tool_name: call.call().tool_name.as_str().to_owned(),
            capability: capability_name(call.call().capability.kind).to_owned(),
            resource: call.call().capability.resource.as_str().to_owned(),
            details: autoharness_tool::permission_details(call.call())
                .expect("durable tool calls must replan")
                .into_iter()
                .map(|detail| PermissionDetailView {
                    label: detail.label.to_owned(),
                    value: detail.value,
                })
                .collect(),
        })
        .collect();

    SessionProjection {
        session_id: aggregate.session_id().as_str().to_owned(),
        revision: aggregate
            .last_sequence()
            .map_or(0, |sequence| sequence.get()),
        selected_model: aggregate.selected_model().cloned(),
        transcript,
        permission_requests,
    }
}

fn tool_status(status: autoharness_engine::ToolCallStatus) -> &'static str {
    match status {
        autoharness_engine::ToolCallStatus::Proposed => "proposed",
        autoharness_engine::ToolCallStatus::PermissionPending => "permission pending",
        autoharness_engine::ToolCallStatus::Authorized => "authorized",
        autoharness_engine::ToolCallStatus::DeniedPending => "denying",
        autoharness_engine::ToolCallStatus::Running => "running",
        autoharness_engine::ToolCallStatus::Completed => "completed",
        autoharness_engine::ToolCallStatus::Failed => "failed",
        autoharness_engine::ToolCallStatus::Denied => "denied",
        autoharness_engine::ToolCallStatus::Cancelled => "cancelled",
        autoharness_engine::ToolCallStatus::Unknown => "unknown",
    }
}

fn tool_summary(call: &autoharness_engine::ToolCallProjection) -> Option<String> {
    if let Some(output) = call.output() {
        let suffix = if output.truncated() {
            " retained as artifact"
        } else {
            ""
        };
        return Some(format!("{} bytes{suffix}", output.original_bytes()));
    }
    call.failure()
        .map(|failure| failure.message().as_str().to_owned())
}

const fn capability_name(kind: autoharness_domain::CapabilityKind) -> &'static str {
    match kind {
        autoharness_domain::CapabilityKind::InvalidToolCall => "invalid tool call",
        autoharness_domain::CapabilityKind::MemoryProposal => "memory proposal",
        autoharness_domain::CapabilityKind::FilesystemRead => "filesystem read",
        autoharness_domain::CapabilityKind::FilesystemWrite => "filesystem write",
        autoharness_domain::CapabilityKind::ProcessExecute => "process execute",
        autoharness_domain::CapabilityKind::HttpRequest => "HTTP request",
    }
}

/// Converts provider discovery into a selectable provider-neutral catalog.
#[must_use]
pub fn catalog(models: Vec<ModelDescriptor>, stale: bool) -> CatalogProjection {
    let models = models
        .into_iter()
        .map(|descriptor| {
            let selectable = descriptor.capabilities.supports_streamed_chat();
            let detail = catalog_detail(&descriptor);
            ModelSummary {
                model: ModelRef::new(descriptor.provider_id, descriptor.model_id),
                display_name: descriptor.display_name,
                detail,
                context_window_tokens: descriptor.input_token_limit,
                selectable,
            }
        })
        .collect();
    CatalogProjection::Ready { models, stale }
}

/// Converts bounded authorized store rows into the complete Memory workspace projection.
pub fn memory(
    generation: u64,
    records: Vec<(
        autoharness_store::MemoryInspectionRecord,
        Vec<autoharness_store::MemoryAdmissionRecord>,
    )>,
    stale: bool,
) -> Result<MemoryProjection, &'static str> {
    memory_projection(generation, records, stale, None)
}

/// Converts authorized store rows at an explicit wall-clock boundary.
pub fn memory_at(
    generation: u64,
    records: Vec<(
        autoharness_store::MemoryInspectionRecord,
        Vec<autoharness_store::MemoryAdmissionRecord>,
    )>,
    stale: bool,
    as_of: TimestampMillis,
) -> Result<MemoryProjection, &'static str> {
    memory_projection(generation, records, stale, Some(as_of))
}

fn memory_projection(
    generation: u64,
    records: Vec<(
        autoharness_store::MemoryInspectionRecord,
        Vec<autoharness_store::MemoryAdmissionRecord>,
    )>,
    stale: bool,
    as_of: Option<TimestampMillis>,
) -> Result<MemoryProjection, &'static str> {
    let total = u32::try_from(records.len()).unwrap_or(u32::MAX);
    let mut summaries = Vec::with_capacity(records.len());
    let mut details = Vec::with_capacity(records.len());
    for (record, admission_records) in records {
        let preview = record.content().map_or_else(
            || "Content erased".to_owned(),
            |value| memory_preview(value.as_str()),
        );
        let admissions = admission_records
            .iter()
            .map(memory_admission)
            .collect::<Result<Vec<_>, _>>()?;
        let admission_count = u32::try_from(admissions.len()).unwrap_or(u32::MAX);
        let revision = record.latest_revision();
        let status = as_of.map_or_else(
            || memory_status(record.lifecycle()),
            |as_of| effective_memory_status(&record, as_of),
        );
        summaries.push(MemorySummary::new(
            record.memory_id().as_str(),
            preview,
            status,
            memory_scope(record.scope()),
            record.updated_at().get(),
            Some(revision.confidence().get()),
            admission_count,
        )?);
        let detail = match record.content() {
            Some(content) => MemoryDetail::new(
                record.memory_id().as_str(),
                u32::try_from(revision.revision().get()).unwrap_or(u32::MAX),
                content.as_str(),
                memory_source(revision.origin()),
                memory_trust(revision.trust_class()),
                revision.created_at().get(),
                memory_valid_until(revision.validity()),
                admissions,
            )?,
            None => MemoryDetail::metadata_only(
                record.memory_id().as_str(),
                u32::try_from(revision.revision().get()).unwrap_or(u32::MAX),
                memory_source(revision.origin()),
                memory_trust(revision.trust_class()),
                revision.created_at().get(),
                memory_valid_until(revision.validity()),
                admissions,
            )?,
        };
        details.push(detail.with_revision_context(memory_revision_context(&record)?));
    }
    MemoryProjection::ready(generation, summaries, details, total, stale)
}

#[must_use]
pub const fn memory_projection_page_size() -> u32 {
    MEMORY_PROJECTION_PAGE_SIZE
}

#[must_use]
pub const fn memory_admission_page_size() -> u32 {
    MEMORY_ADMISSION_PAGE_SIZE
}

fn memory_preview(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&normalized, 240)
}

fn memory_admission(
    record: &autoharness_store::MemoryAdmissionRecord,
) -> Result<MemoryAdmission, &'static str> {
    let model = truncate_chars(
        &format!(
            "{}/{}",
            record.model().provider_id().as_str(),
            record.model().model_id().as_str()
        ),
        256,
    );
    let factors = record
        .reasons()
        .iter()
        .map(|reason| admission_factor(reason.factor()))
        .collect::<Vec<_>>()
        .join(", ");
    let reason = if factors.is_empty() {
        format!("turn {}", record.run_turn())
    } else {
        format!("turn {}: {factors}", record.run_turn())
    };
    let context = MemoryAdmissionContext::new(
        record.attempt_id().as_str(),
        record.run_turn(),
        record.epoch_id().as_str(),
        u32::try_from(record.token_count().get()).unwrap_or(u32::MAX),
        record.memory_revision_id().as_str(),
        format!("v{}", record.renderer_version()),
        record
            .reasons()
            .iter()
            .map(|reason| {
                format!(
                    "{} {:+}",
                    admission_factor(reason.factor()),
                    reason.contribution()
                )
            })
            .collect(),
    )?;
    Ok(MemoryAdmission::new(
        truncate_chars(record.session_id().as_str(), 256),
        model,
        truncate_chars(&reason, 256),
        record.admitted_at().get(),
        record.rank(),
    )?
    .with_context(context))
}

fn memory_revision_context(
    record: &autoharness_store::MemoryInspectionRecord,
) -> Result<MemoryRevisionContext, &'static str> {
    let revision = record.latest_revision();
    if revision.evidence().len() != record.evidence_content().len() {
        return Err("memory evidence metadata and content do not align");
    }
    let evidence = revision
        .evidence()
        .iter()
        .zip(record.evidence_content())
        .map(|(metadata, content)| {
            if metadata.evidence_id() != content.evidence_id() {
                return Err("memory evidence identities do not align");
            }
            let label = format!(
                "{} - {}",
                memory_evidence_relation(metadata.relation()),
                metadata.evidence_id().as_str()
            );
            let source = memory_evidence_source(metadata.source());
            match content.excerpt() {
                autoharness_store::MemoryEvidenceExcerptState::Retained(excerpt) => {
                    UiMemoryEvidence::new(label, source, excerpt.as_str())
                }
                autoharness_store::MemoryEvidenceExcerptState::Absent => {
                    UiMemoryEvidence::absent(label, source)
                }
                autoharness_store::MemoryEvidenceExcerptState::Erased => {
                    UiMemoryEvidence::erased(label, source)
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let relations = revision
        .relations()
        .iter()
        .map(|relation| {
            let kind = match relation.kind() {
                autoharness_domain::MemoryRelationKind::DuplicateOf => {
                    UiMemoryRelationKind::DuplicateOf
                }
                autoharness_domain::MemoryRelationKind::Contradicts => {
                    UiMemoryRelationKind::Contradicts
                }
                autoharness_domain::MemoryRelationKind::Supersedes => {
                    UiMemoryRelationKind::Supersedes
                }
                autoharness_domain::MemoryRelationKind::Refines => UiMemoryRelationKind::Refines,
                autoharness_domain::MemoryRelationKind::Related => UiMemoryRelationKind::Related,
            };
            UiMemoryRelation::new(kind, relation.memory_id().as_str())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut findings = revision
        .relations()
        .iter()
        .filter_map(|relation| {
            let (kind, summary) = match relation.kind() {
                autoharness_domain::MemoryRelationKind::DuplicateOf => (
                    MemoryFindingKind::Duplicate,
                    "Durable exact-duplicate relation",
                ),
                autoharness_domain::MemoryRelationKind::Contradicts => (
                    MemoryFindingKind::Contradiction,
                    "Durable contradictory-memory relation",
                ),
                autoharness_domain::MemoryRelationKind::Refines
                | autoharness_domain::MemoryRelationKind::Supersedes
                | autoharness_domain::MemoryRelationKind::Related => return None,
            };
            Some(MemoryValidationFinding::new(
                kind,
                relation.memory_id().as_str(),
                summary,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(validation) = record.latest_validation() {
        if validation.content_hash() != revision.content_hash() {
            return Err("memory validation does not match the latest revision");
        }
        let has_duplicate_relation = revision
            .relations()
            .iter()
            .any(|relation| relation.kind() == autoharness_domain::MemoryRelationKind::DuplicateOf);
        let has_contradiction_relation = revision
            .relations()
            .iter()
            .any(|relation| relation.kind() == autoharness_domain::MemoryRelationKind::Contradicts);
        let validation_anchor = format!("validation result v{}", validation.validator_version());
        for issue in validation.issues() {
            if (*issue == MemoryValidationIssue::Duplicate && has_duplicate_relation)
                || (*issue == MemoryValidationIssue::Contradiction && has_contradiction_relation)
            {
                continue;
            }
            let kind = memory_finding_kind(*issue);
            findings.push(MemoryValidationFinding::new(
                kind,
                &validation_anchor,
                format!(
                    "Validator v{} reported {} without a retained related-memory identity",
                    validation.validator_version(),
                    kind.label()
                ),
            )?);
        }
    }
    MemoryRevisionContext::new(
        record.last_sequence(),
        revision.revision_id().as_str(),
        (record.lifecycle() == MemoryRevisionStatus::Proposed)
            .then(|| revision.revision_id().as_str().to_owned()),
        memory_scope_identity(record.scope()),
        memory_origin(revision.origin()),
        memory_sensitivity(revision.sensitivity()),
        evidence,
        relations,
        findings,
    )
}

const fn memory_evidence_relation(relation: MemoryEvidenceRelation) -> &'static str {
    match relation {
        MemoryEvidenceRelation::Supports => "supports",
        MemoryEvidenceRelation::Contradicts => "contradicts",
        MemoryEvidenceRelation::DerivedFrom => "derived from",
    }
}

fn memory_evidence_source(source: &MemoryEvidenceSource) -> String {
    match source {
        MemoryEvidenceSource::UserInput {
            session_id,
            input_id,
        } => format!(
            "session {} / user input {}",
            session_id.as_str(),
            input_id.as_str()
        ),
        MemoryEvidenceSource::ToolObservation {
            session_id,
            tool_call_id,
            output_hash,
        } => format!(
            "session {} / tool {} / output sha256:{}",
            session_id.as_str(),
            tool_call_id.as_str(),
            output_hash.as_str()
        ),
        MemoryEvidenceSource::ImportedDocument {
            source_key,
            source_revision,
        } => format!(
            "source {} / revision sha256:{}",
            source_key.as_str(),
            source_revision.as_str()
        ),
        MemoryEvidenceSource::SessionEvent {
            session_id,
            event_id,
        } => format!(
            "session {} / event {}",
            session_id.as_str(),
            event_id.as_str()
        ),
        MemoryEvidenceSource::MemoryRevision {
            memory_id,
            revision_id,
        } => format!(
            "memory {} / revision {}",
            memory_id.as_str(),
            revision_id.as_str()
        ),
    }
}

const fn memory_finding_kind(issue: MemoryValidationIssue) -> MemoryFindingKind {
    match issue {
        MemoryValidationIssue::SecretDetected => MemoryFindingKind::SecretDetected,
        MemoryValidationIssue::UnsupportedScope => MemoryFindingKind::UnsupportedScope,
        MemoryValidationIssue::MalformedContent => MemoryFindingKind::MalformedContent,
        MemoryValidationIssue::PolicyConflict => MemoryFindingKind::PolicyConflict,
        MemoryValidationIssue::Duplicate => MemoryFindingKind::Duplicate,
        MemoryValidationIssue::Contradiction => MemoryFindingKind::Contradiction,
        MemoryValidationIssue::InjectionPattern => MemoryFindingKind::InjectionPattern,
        MemoryValidationIssue::UngroundedEvidence => MemoryFindingKind::UngroundedEvidence,
    }
}

const fn memory_status(status: MemoryRevisionStatus) -> MemoryStatus {
    match status {
        MemoryRevisionStatus::Active => MemoryStatus::Active,
        MemoryRevisionStatus::Proposed => MemoryStatus::Proposed,
        MemoryRevisionStatus::Superseded => MemoryStatus::Superseded,
        MemoryRevisionStatus::Rejected => MemoryStatus::Rejected,
        MemoryRevisionStatus::Retracted => MemoryStatus::Retracted,
        MemoryRevisionStatus::Deleted => MemoryStatus::Deleted,
    }
}

fn effective_memory_status(
    record: &autoharness_store::MemoryInspectionRecord,
    as_of: TimestampMillis,
) -> MemoryStatus {
    let lifecycle = record.lifecycle();
    if matches!(
        lifecycle,
        MemoryRevisionStatus::Active | MemoryRevisionStatus::Proposed
    ) {
        let revision = record.latest_revision();
        let conflicting =
            revision.relations().iter().any(|relation| {
                relation.kind() == autoharness_domain::MemoryRelationKind::Contradicts
            }) || record.latest_validation().is_some_and(|validation| {
                validation
                    .issues()
                    .contains(&MemoryValidationIssue::Contradiction)
            });
        if conflicting {
            return MemoryStatus::Conflicting;
        }
        if !memory_validity_contains(revision.validity(), as_of) {
            return MemoryStatus::Expired;
        }
    }
    memory_status(lifecycle)
}

const fn memory_validity_contains(validity: MemoryValidity, as_of: TimestampMillis) -> bool {
    match validity {
        MemoryValidity::Indefinite => true,
        MemoryValidity::From { valid_from } => as_of.get() >= valid_from.get(),
        MemoryValidity::Until { valid_until } => as_of.get() < valid_until.get(),
        MemoryValidity::Window(window) => {
            as_of.get() >= window.valid_from().get() && as_of.get() < window.valid_until().get()
        }
    }
}

const fn memory_scope(scope: &DomainMemoryScope) -> MemoryScope {
    match scope {
        DomainMemoryScope::User(_) => MemoryScope::User,
        DomainMemoryScope::Workspace(_) => MemoryScope::Workspace,
        DomainMemoryScope::Session(_) => MemoryScope::Session,
        DomainMemoryScope::Agent(_) => MemoryScope::Agent,
    }
}

fn memory_scope_identity(scope: &DomainMemoryScope) -> &str {
    match scope {
        DomainMemoryScope::User(id) => id.as_str(),
        DomainMemoryScope::Workspace(id) => id.as_str(),
        DomainMemoryScope::Session(id) => id.as_str(),
        DomainMemoryScope::Agent(id) => id.as_str(),
    }
}

const fn memory_origin(origin: MemoryOrigin) -> UiMemoryOrigin {
    match origin {
        MemoryOrigin::ExplicitUser => UiMemoryOrigin::ExplicitUser,
        MemoryOrigin::VerifiedTool => UiMemoryOrigin::VerifiedTool,
        MemoryOrigin::ImportedDocument => UiMemoryOrigin::ImportedDocument,
        MemoryOrigin::ModelProposal => UiMemoryOrigin::ModelProposal,
        MemoryOrigin::Compaction => UiMemoryOrigin::Compaction,
    }
}

const fn memory_sensitivity(sensitivity: autoharness_domain::Sensitivity) -> MemorySensitivity {
    match sensitivity {
        autoharness_domain::Sensitivity::Public => MemorySensitivity::Public,
        autoharness_domain::Sensitivity::Internal => MemorySensitivity::Internal,
        autoharness_domain::Sensitivity::Sensitive => MemorySensitivity::Sensitive,
        autoharness_domain::Sensitivity::Secret => MemorySensitivity::Secret,
    }
}

const fn memory_trust(trust: TrustClass) -> MemoryTrust {
    match trust {
        TrustClass::UserApproved => MemoryTrust::UserApproved,
        TrustClass::VerifiedObservation => MemoryTrust::VerifiedObservation,
        TrustClass::Imported => MemoryTrust::Imported,
        TrustClass::UntrustedProposal => MemoryTrust::UntrustedProposal,
    }
}

const fn memory_source(origin: MemoryOrigin) -> &'static str {
    match origin {
        MemoryOrigin::ExplicitUser => "Explicit user request",
        MemoryOrigin::VerifiedTool => "Verified tool observation",
        MemoryOrigin::ImportedDocument => "Imported document",
        MemoryOrigin::ModelProposal => "Model proposal",
        MemoryOrigin::Compaction => "Context compaction proposal",
    }
}

const fn memory_valid_until(validity: MemoryValidity) -> Option<i64> {
    match validity {
        MemoryValidity::Indefinite | MemoryValidity::From { .. } => None,
        MemoryValidity::Until { valid_until } => Some(valid_until.get()),
        MemoryValidity::Window(window) => Some(window.valid_until().get()),
    }
}

const fn admission_factor(factor: ContextAdmissionFactor) -> &'static str {
    match factor {
        ContextAdmissionFactor::Pin => "pinned",
        ContextAdmissionFactor::Authority => "authority",
        ContextAdmissionFactor::ExactMatch => "exact match",
        ContextAdmissionFactor::ScopeSpecificity => "scope",
        ContextAdmissionFactor::LexicalOverlap => "lexical match",
        ContextAdmissionFactor::Freshness => "freshness",
        ContextAdmissionFactor::Confidence => "confidence",
        ContextAdmissionFactor::PriorUtility => "prior utility",
        ContextAdmissionFactor::Diversity => "diversity",
        ContextAdmissionFactor::BudgetFit => "budget fit",
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn catalog_detail(descriptor: &ModelDescriptor) -> String {
    let mut detail = Vec::new();
    if let Some(limit) = descriptor.input_token_limit {
        detail.push(format!("{limit} input tokens"));
    }
    if descriptor.capabilities.thinking == CapabilitySupport::Supported {
        detail.push("thinking".to_owned());
    }
    if descriptor.capabilities.managed_interactions == CapabilitySupport::Unknown {
        detail.push("Interactions support unknown".to_owned());
    }
    detail.join(" | ")
}

fn interrupted_failure() -> UiFailure {
    UiFailure::new(
        ErrorClass::Unavailable,
        "The provider attempt was interrupted before its outcome was known",
        RetryPolicy::Now,
    )
    .with_code("interrupted_attempt")
}

#[cfg(test)]
mod tests {
    use autoharness_domain::{
        Causation, CommandId, ConfidenceBasisPoints, ContextSourceKey, CorrelationId,
        EventEnvelope, EventId, EventPayload, InputId, MemoryContent as DomainMemoryContent,
        MemoryEvidence, MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryEvidenceSource, MemoryId,
        MemoryKind, MemoryOrigin, MemoryRelation, MemoryRelationKind, MemoryRevision,
        MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionNumber, MemoryRevisionStatus,
        MemoryScope, MemoryValidationIssue, MemoryValidationResult, MemoryValidationStatus,
        MemoryValidity, SessionId, SessionSequence, Sha256Digest, TimestampMillis, ToolCallId,
        TrustClass, UserId,
    };
    use autoharness_memory::normalized_content_hash;
    use autoharness_store::{
        MemoryEvidenceExcerptState, MemoryInspectionRecord, StoredMemoryEvidenceContent,
    };
    use autoharness_tui::MemoryEvidenceAvailability;
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn projection_revision_comes_from_durable_sequence_not_time() {
        let session_id = SessionId::new("session-1").expect("valid session ID");
        let event = EventEnvelope::new_v1(
            EventId::new("event-1").expect("valid event ID"),
            session_id.clone(),
            SessionSequence::FIRST,
            TimestampMillis::new(-100),
            Causation::Command(CommandId::new("command-1").expect("valid command ID")),
            CorrelationId::new("correlation-1").expect("valid correlation ID"),
            EventPayload::SessionCreated,
        );
        let aggregate = SessionAggregate::rehydrate(session_id, [&event]).expect("valid history");

        assert_eq!(session(&aggregate).revision, 1);
    }

    #[test]
    fn memory_projection_preserves_provenance_and_derives_risk_states() {
        const CONTENT_SENTINEL: &str = "private projection content sentinel";
        const EVIDENCE_SENTINEL: &str = "authorization bearer evidence sentinel";

        let retained_excerpt =
            MemoryEvidenceExcerpt::new(EVIDENCE_SENTINEL).expect("evidence excerpt");
        let erased_excerpt =
            MemoryEvidenceExcerpt::new("erased evidence bytes").expect("evidence excerpt");
        let retained_hash = raw_digest(retained_excerpt.as_str());
        let erased_hash = raw_digest(erased_excerpt.as_str());
        let evidence = vec![
            MemoryEvidence::new(
                MemoryEvidenceId::new("evidence-user").expect("evidence ID"),
                MemoryEvidenceSource::UserInput {
                    session_id: SessionId::new("session-evidence-user").expect("session ID"),
                    input_id: InputId::new("input-evidence-user").expect("input ID"),
                },
                MemoryEvidenceRelation::Supports,
                Some(retained_excerpt.clone()),
                Some(retained_hash),
            )
            .expect("user evidence"),
            MemoryEvidence::new(
                MemoryEvidenceId::new("evidence-tool").expect("evidence ID"),
                MemoryEvidenceSource::ToolObservation {
                    session_id: SessionId::new("session-evidence-tool").expect("session ID"),
                    tool_call_id: ToolCallId::new("tool-evidence").expect("tool ID"),
                    output_hash: raw_digest("tool output"),
                },
                MemoryEvidenceRelation::Contradicts,
                None,
                None,
            )
            .expect("tool evidence"),
            MemoryEvidence::new(
                MemoryEvidenceId::new("evidence-import").expect("evidence ID"),
                MemoryEvidenceSource::ImportedDocument {
                    source_key: ContextSourceKey::new("source-evidence-import")
                        .expect("source key"),
                    source_revision: raw_digest("import revision"),
                },
                MemoryEvidenceRelation::DerivedFrom,
                Some(erased_excerpt.clone()),
                Some(erased_hash),
            )
            .expect("import evidence"),
            MemoryEvidence::new(
                MemoryEvidenceId::new("evidence-event").expect("evidence ID"),
                MemoryEvidenceSource::SessionEvent {
                    session_id: SessionId::new("session-evidence-event").expect("session ID"),
                    event_id: EventId::new("event-evidence").expect("event ID"),
                },
                MemoryEvidenceRelation::Supports,
                None,
                None,
            )
            .expect("event evidence"),
            MemoryEvidence::new(
                MemoryEvidenceId::new("evidence-memory").expect("evidence ID"),
                MemoryEvidenceSource::MemoryRevision {
                    memory_id: MemoryId::new("memory-evidence-source").expect("memory ID"),
                    revision_id: MemoryRevisionId::new("revision-evidence-source")
                        .expect("revision ID"),
                },
                MemoryEvidenceRelation::DerivedFrom,
                None,
                None,
            )
            .expect("memory evidence"),
        ];
        let relations = [
            MemoryRelationKind::DuplicateOf,
            MemoryRelationKind::Contradicts,
            MemoryRelationKind::Refines,
            MemoryRelationKind::Supersedes,
            MemoryRelationKind::Related,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            MemoryRelation::new(
                MemoryId::new(format!("memory-related-{index}")).expect("memory ID"),
                kind,
            )
        })
        .collect::<Vec<_>>();
        let content = DomainMemoryContent::new(CONTENT_SENTINEL).expect("memory content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new("revision-projection-risk").expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ModelProposal,
            TrustClass::UntrustedProposal,
            ConfidenceBasisPoints::new(7_500).expect("confidence"),
            autoharness_domain::Sensitivity::Internal,
            MemoryValidity::Indefinite,
            evidence,
            relations,
        )
        .expect("revision draft");
        let revision = MemoryRevision::from_draft(
            MemoryRevisionStatus::Proposed,
            &draft,
            TimestampMillis::new(10),
            None,
        );
        let validation_issues = vec![
            MemoryValidationIssue::SecretDetected,
            MemoryValidationIssue::UnsupportedScope,
            MemoryValidationIssue::MalformedContent,
            MemoryValidationIssue::PolicyConflict,
            MemoryValidationIssue::Duplicate,
            MemoryValidationIssue::Contradiction,
            MemoryValidationIssue::InjectionPattern,
            MemoryValidationIssue::UngroundedEvidence,
        ];
        let validation = MemoryValidationResult::new(
            3,
            revision.content_hash().clone(),
            MemoryValidationStatus::NeedsReview,
            validation_issues,
        )
        .expect("validation");
        let evidence_content = vec![
            StoredMemoryEvidenceContent::new(
                MemoryEvidenceId::new("evidence-user").expect("evidence ID"),
                MemoryEvidenceExcerptState::Retained(retained_excerpt),
            ),
            StoredMemoryEvidenceContent::new(
                MemoryEvidenceId::new("evidence-tool").expect("evidence ID"),
                MemoryEvidenceExcerptState::Absent,
            ),
            StoredMemoryEvidenceContent::new(
                MemoryEvidenceId::new("evidence-import").expect("evidence ID"),
                MemoryEvidenceExcerptState::Erased,
            ),
            StoredMemoryEvidenceContent::new(
                MemoryEvidenceId::new("evidence-event").expect("evidence ID"),
                MemoryEvidenceExcerptState::Absent,
            ),
            StoredMemoryEvidenceContent::new(
                MemoryEvidenceId::new("evidence-memory").expect("evidence ID"),
                MemoryEvidenceExcerptState::Absent,
            ),
        ];
        let record = MemoryInspectionRecord::new(
            MemoryId::new("memory-projection-risk").expect("memory ID"),
            MemoryScope::User(UserId::new("user-projection").expect("user ID")),
            MemoryKind::Fact,
            MemoryRevisionStatus::Proposed,
            revision,
            Some(content),
            evidence_content,
            Some(validation),
            None,
            2,
            TimestampMillis::new(10),
            TimestampMillis::new(11),
        );

        let projected = memory_at(
            4,
            vec![(record, Vec::new())],
            false,
            TimestampMillis::new(20),
        )
        .expect("memory projection");
        assert_eq!(projected.summaries()[0].status(), MemoryStatus::Conflicting);
        let context = projected
            .detail("memory-projection-risk")
            .and_then(MemoryDetail::revision_context)
            .expect("revision context");
        assert_eq!(context.evidence().len(), 5);
        assert_eq!(
            context.evidence()[0].availability(),
            MemoryEvidenceAvailability::Retained
        );
        assert_eq!(
            context.evidence()[1].availability(),
            MemoryEvidenceAvailability::Absent
        );
        assert_eq!(
            context.evidence()[2].availability(),
            MemoryEvidenceAvailability::Erased
        );
        assert!(
            context.evidence()[0]
                .source()
                .contains("input-evidence-user")
        );
        assert!(context.evidence()[1].source().contains("tool-evidence"));
        assert!(
            context.evidence()[2]
                .source()
                .contains("source-evidence-import")
        );
        assert!(context.evidence()[3].source().contains("event-evidence"));
        assert!(
            context.evidence()[4]
                .source()
                .contains("revision-evidence-source")
        );
        let maximum_id = "a".repeat(512);
        let maximum_source = memory_evidence_source(&MemoryEvidenceSource::ToolObservation {
            session_id: SessionId::new(maximum_id.clone()).expect("maximum session ID"),
            tool_call_id: ToolCallId::new(maximum_id).expect("maximum tool ID"),
            output_hash: raw_digest("maximum source output"),
        });
        assert!(UiMemoryEvidence::absent("maximum source", maximum_source).is_ok());
        assert_eq!(
            context
                .relations()
                .iter()
                .map(autoharness_tui::MemoryRelation::kind)
                .collect::<Vec<_>>(),
            vec![
                UiMemoryRelationKind::DuplicateOf,
                UiMemoryRelationKind::Contradicts,
                UiMemoryRelationKind::Refines,
                UiMemoryRelationKind::Supersedes,
                UiMemoryRelationKind::Related,
            ]
        );
        assert_eq!(context.findings().len(), 8);
        assert_eq!(
            context
                .findings()
                .iter()
                .map(MemoryValidationFinding::kind)
                .collect::<Vec<_>>(),
            vec![
                MemoryFindingKind::Duplicate,
                MemoryFindingKind::Contradiction,
                MemoryFindingKind::SecretDetected,
                MemoryFindingKind::UnsupportedScope,
                MemoryFindingKind::MalformedContent,
                MemoryFindingKind::PolicyConflict,
                MemoryFindingKind::InjectionPattern,
                MemoryFindingKind::UngroundedEvidence,
            ]
        );
        assert_eq!(
            context.findings()[0].related_memory_id(),
            "memory-related-0"
        );
        assert_eq!(
            context.findings()[1].related_memory_id(),
            "memory-related-1"
        );
        assert!(
            context.findings()[2]
                .related_memory_id()
                .starts_with("validation result v")
        );
        let debug = format!("{projected:?}");
        assert!(!debug.contains(CONTENT_SENTINEL));
        assert!(!debug.contains(EVIDENCE_SENTINEL));

        let expired = inspection_record_with_validity(
            "memory-projection-expired",
            "revision-projection-expired",
            MemoryValidity::Until {
                valid_until: TimestampMillis::new(100),
            },
        );
        let before = memory_at(
            5,
            vec![(expired.clone(), Vec::new())],
            false,
            TimestampMillis::new(99),
        )
        .expect("pre-expiry projection");
        assert_eq!(before.summaries()[0].status(), MemoryStatus::Active);
        let at_boundary = memory_at(
            5,
            vec![(expired, Vec::new())],
            false,
            TimestampMillis::new(100),
        )
        .expect("expiry-boundary projection");
        assert_eq!(at_boundary.summaries()[0].status(), MemoryStatus::Expired);
    }

    fn inspection_record_with_validity(
        memory_id: &str,
        revision_id: &str,
        validity: MemoryValidity,
    ) -> MemoryInspectionRecord {
        let content = DomainMemoryContent::new("time-bound memory").expect("memory content");
        let draft = MemoryRevisionDraft::new(
            MemoryRevisionId::new(revision_id).expect("revision ID"),
            MemoryRevisionNumber::FIRST,
            None,
            content.clone(),
            normalized_content_hash(content.as_str()).expect("content hash"),
            MemoryOrigin::ExplicitUser,
            TrustClass::UserApproved,
            ConfidenceBasisPoints::new(9_000).expect("confidence"),
            autoharness_domain::Sensitivity::Internal,
            validity,
            Vec::new(),
            Vec::new(),
        )
        .expect("revision draft");
        MemoryInspectionRecord::new(
            MemoryId::new(memory_id).expect("memory ID"),
            MemoryScope::User(UserId::new("user-projection").expect("user ID")),
            MemoryKind::Fact,
            MemoryRevisionStatus::Active,
            MemoryRevision::from_draft(
                MemoryRevisionStatus::Active,
                &draft,
                TimestampMillis::new(10),
                None,
            ),
            Some(content),
            Vec::new(),
            None,
            Some(draft.revision_id().clone()),
            1,
            TimestampMillis::new(10),
            TimestampMillis::new(10),
        )
    }

    fn raw_digest(value: &str) -> Sha256Digest {
        Sha256Digest::new(
            Sha256::digest(value.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .expect("SHA-256 digest")
    }
}
