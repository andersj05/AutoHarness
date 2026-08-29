use autoharness_domain::{
    ContextAdmissionFactor, ErrorClass, MemoryOrigin, MemoryRevisionStatus,
    MemoryScope as DomainMemoryScope, MemoryValidity, ModelRef, TrustClass,
};
use autoharness_engine::{AttemptStatus as EngineAttemptStatus, SessionAggregate};
use autoharness_provider::{CapabilitySupport, ModelDescriptor};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, ModelSummary, PermissionDetailView,
    PermissionRequestView, RetryPolicy, SessionProjection, ToolCallKey, ToolRowView,
    TranscriptItem, UiFailure, UsageView,
};
use autoharness_tui::{
    MemoryAdmission, MemoryAdmissionContext, MemoryDetail, MemoryFindingKind,
    MemoryOrigin as UiMemoryOrigin, MemoryProjection, MemoryRelation as UiMemoryRelation,
    MemoryRelationKind as UiMemoryRelationKind, MemoryRevisionContext, MemoryScope,
    MemorySensitivity, MemoryStatus, MemorySummary, MemoryTrust, MemoryValidationFinding,
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
        summaries.push(MemorySummary::new(
            record.memory_id().as_str(),
            preview,
            memory_status(record.lifecycle()),
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
    let relations = revision
        .relations()
        .iter()
        .filter_map(|relation| {
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
                autoharness_domain::MemoryRelationKind::Refines
                | autoharness_domain::MemoryRelationKind::Related => return None,
            };
            UiMemoryRelation::new(kind, relation.memory_id().as_str()).ok()
        })
        .collect::<Vec<_>>();
    let findings = revision
        .relations()
        .iter()
        .filter_map(|relation| {
            let (kind, summary) = match relation.kind() {
                autoharness_domain::MemoryRelationKind::DuplicateOf => {
                    (MemoryFindingKind::Duplicate, "Exact duplicate relation")
                }
                autoharness_domain::MemoryRelationKind::Contradicts => (
                    MemoryFindingKind::Contradiction,
                    "Contradictory memory relation",
                ),
                autoharness_domain::MemoryRelationKind::Refines
                | autoharness_domain::MemoryRelationKind::Supersedes
                | autoharness_domain::MemoryRelationKind::Related => return None,
            };
            MemoryValidationFinding::new(kind, relation.memory_id().as_str(), summary).ok()
        })
        .collect();
    MemoryRevisionContext::new(
        record.last_sequence(),
        revision.revision_id().as_str(),
        (record.lifecycle() == MemoryRevisionStatus::Proposed)
            .then(|| revision.revision_id().as_str().to_owned()),
        memory_scope_identity(record.scope()),
        memory_origin(revision.origin()),
        memory_sensitivity(revision.sensitivity()),
        Vec::new(),
        relations,
        findings,
    )
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
        Causation, CommandId, CorrelationId, EventEnvelope, EventId, EventPayload, SessionId,
        SessionSequence, TimestampMillis,
    };

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
}
