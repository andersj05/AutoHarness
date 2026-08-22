use autoharness_domain::{ErrorClass, ModelRef};
use autoharness_engine::{AttemptStatus as EngineAttemptStatus, SessionAggregate};
use autoharness_provider::{CapabilitySupport, ModelDescriptor};
use autoharness_tui::{
    AttemptKey, AttemptStatus, CatalogProjection, ModelSummary, PermissionDetailView,
    PermissionRequestView, RetryPolicy, SessionProjection, ToolCallKey, TranscriptItem, UiFailure,
    UsageView,
};

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
                        }),
                ),
                EngineAttemptStatus::Unknown => AttemptStatus::Failed(interrupted_failure()),
            };
            let usage = attempt.usage().map(|usage| UsageView {
                input_tokens: usage.input_tokens().unwrap_or_default(),
                output_tokens: usage.output_tokens().unwrap_or_default(),
            });
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
        revision: aggregate
            .last_sequence()
            .map_or(0, |sequence| sequence.get()),
        selected_model: aggregate.selected_model().cloned(),
        transcript,
        permission_requests,
    }
}

const fn capability_name(kind: autoharness_domain::CapabilityKind) -> &'static str {
    match kind {
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
                selectable,
            }
        })
        .collect();
    CatalogProjection::Ready { models, stale }
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
