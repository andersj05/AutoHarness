use autoharness_domain::{
    ArtifactId, ArtifactRef, AttemptFailure, AttemptId, CapabilityKind, CapabilityRequest,
    Causation, CommandEnvelope, CommandId, CommandPayload, CorrelationId, DeliveryMode, ErrorClass,
    ErrorCode, EventEnvelope, EventId, EventPayload, InputId, ModelId, ModelRef, PermissionAnswer,
    PermissionDecisionId, PermissionOutcome, PromptText, ProviderCallId, ProviderId, PublicMessage,
    ResourceRef, ResponseText, RetryAdvice, RunLimits, SessionId, SessionSequence, SessionTitle,
    TimestampMillis, ToolArguments, ToolCallId, ToolCallSpec, ToolName, ToolOutput, UsageSnapshot,
};

fn session_id() -> SessionId {
    SessionId::new("session-1").expect("valid test ID")
}

fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("valid test ID")
}

fn event_id(value: &str) -> EventId {
    EventId::new(value).expect("valid test ID")
}

fn correlation_id() -> CorrelationId {
    CorrelationId::new("correlation-1").expect("valid test ID")
}

fn input_id() -> InputId {
    InputId::new("input-1").expect("valid test ID")
}

fn attempt_id(value: &str) -> AttemptId {
    AttemptId::new(value).expect("valid test ID")
}

fn model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("valid provider ID"),
        ModelId::new("models/gemini-test").expect("valid model ID"),
    )
}

fn prompt() -> PromptText {
    PromptText::new("  first line\nsecond line: こんにちは  ").expect("non-empty test prompt")
}

fn failure() -> AttemptFailure {
    AttemptFailure::new(
        ErrorClass::Unavailable,
        ErrorCode::new("provider_unavailable").expect("valid error code"),
        PublicMessage::new("The provider is temporarily unavailable")
            .expect("valid public message"),
        RetryAdvice::After { delay_ms: 250 },
    )
}

fn tool_call() -> ToolCallSpec {
    ToolCallSpec {
        tool_call_id: ToolCallId::new("tool-call-1").expect("valid tool call ID"),
        provider_call_id: ProviderCallId::new("provider-call-1").expect("valid provider call ID"),
        tool_name: ToolName::new("fs_write").expect("valid tool name"),
        schema_version: 1,
        arguments: ToolArguments::new(serde_json::json!({
            "path": "notes.txt",
            "content": "hello"
        }))
        .expect("valid tool arguments"),
        capability: CapabilityRequest {
            kind: CapabilityKind::FilesystemWrite,
            resource: ResourceRef::new("workspace:notes.txt").expect("valid resource"),
        },
    }
}

#[test]
fn every_v1_event_payload_has_a_stable_serialized_shape() {
    let events = [
        EventEnvelope::new_v1(
            event_id("event-1"),
            session_id(),
            SessionSequence::FIRST,
            TimestampMillis::new(100),
            Causation::Command(command_id("command-create")),
            correlation_id(),
            EventPayload::SessionCreated,
        ),
        EventEnvelope::new_v1(
            event_id("event-2"),
            session_id(),
            SessionSequence::new(2).expect("nonzero sequence"),
            TimestampMillis::new(90),
            Causation::Event(event_id("event-1")),
            correlation_id(),
            EventPayload::ModelSelected { model: model() },
        ),
        EventEnvelope::new_v1(
            event_id("event-3"),
            session_id(),
            SessionSequence::new(3).expect("nonzero sequence"),
            TimestampMillis::new(80),
            Causation::Command(command_id("command-admit")),
            correlation_id(),
            EventPayload::InputAdmitted {
                input_id: input_id(),
                prompt: prompt(),
                delivery_mode: DeliveryMode::NextTurn,
            },
        ),
    ];

    assert_eq!(
        serde_json::to_value(events).expect("serialize v1 events"),
        serde_json::json!([
            {
                "schema_version": 1,
                "event_id": "event-1",
                "session_id": "session-1",
                "sequence": 1,
                "occurred_at": 100,
                "causation": {
                    "kind": "command",
                    "id": "command-create"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "session_created"
                }
            },
            {
                "schema_version": 1,
                "event_id": "event-2",
                "session_id": "session-1",
                "sequence": 2,
                "occurred_at": 90,
                "causation": {
                    "kind": "event",
                    "id": "event-1"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "model_selected",
                    "payload": {
                        "model": {
                            "provider_id": "google-ai-studio",
                            "model_id": "models/gemini-test"
                        }
                    }
                }
            },
            {
                "schema_version": 1,
                "event_id": "event-3",
                "session_id": "session-1",
                "sequence": 3,
                "occurred_at": 80,
                "causation": {
                    "kind": "command",
                    "id": "command-admit"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "input_admitted",
                    "payload": {
                        "input_id": "input-1",
                        "prompt": "  first line\nsecond line: こんにちは  ",
                        "delivery_mode": "next_turn"
                    }
                }
            }
        ])
    );
}

#[test]
fn every_session_lifecycle_payload_has_a_stable_serialized_shape() {
    let commands = [
        CommandEnvelope::new(
            command_id("command-rename"),
            correlation_id(),
            CommandPayload::RenameSession {
                session_id: session_id(),
                title: SessionTitle::new("Deep dive: streaming").expect("valid test title"),
            },
        ),
        CommandEnvelope::new(
            command_id("command-archive"),
            correlation_id(),
            CommandPayload::ArchiveSession {
                session_id: session_id(),
            },
        ),
        CommandEnvelope::new(
            command_id("command-unarchive"),
            correlation_id(),
            CommandPayload::UnarchiveSession {
                session_id: session_id(),
            },
        ),
    ];
    assert_eq!(
        serde_json::to_value(commands).expect("serialize lifecycle commands"),
        serde_json::json!([
            {
                "command_id": "command-rename",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "rename_session",
                    "payload": {
                        "session_id": "session-1",
                        "title": "Deep dive: streaming"
                    }
                }
            },
            {
                "command_id": "command-archive",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "archive_session",
                    "payload": {
                        "session_id": "session-1"
                    }
                }
            },
            {
                "command_id": "command-unarchive",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "unarchive_session",
                    "payload": {
                        "session_id": "session-1"
                    }
                }
            }
        ])
    );

    let events = [
        EventEnvelope::new_v1(
            event_id("event-rename"),
            session_id(),
            SessionSequence::FIRST,
            TimestampMillis::new(100),
            Causation::Command(command_id("command-rename")),
            correlation_id(),
            EventPayload::SessionRenamed {
                title: SessionTitle::new("Deep dive: streaming").expect("valid test title"),
            },
        ),
        EventEnvelope::new_v1(
            event_id("event-archive"),
            session_id(),
            SessionSequence::new(2).expect("nonzero sequence"),
            TimestampMillis::new(90),
            Causation::Event(event_id("event-rename")),
            correlation_id(),
            EventPayload::SessionArchived,
        ),
        EventEnvelope::new_v1(
            event_id("event-unarchive"),
            session_id(),
            SessionSequence::new(3).expect("nonzero sequence"),
            TimestampMillis::new(80),
            Causation::Command(command_id("command-unarchive")),
            correlation_id(),
            EventPayload::SessionUnarchived,
        ),
    ];
    assert_eq!(
        serde_json::to_value(&events).expect("serialize lifecycle events"),
        serde_json::json!([
            {
                "schema_version": 1,
                "event_id": "event-rename",
                "session_id": "session-1",
                "sequence": 1,
                "occurred_at": 100,
                "causation": {
                    "kind": "command",
                    "id": "command-rename"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "session_renamed",
                    "payload": {
                        "title": "Deep dive: streaming"
                    }
                }
            },
            {
                "schema_version": 1,
                "event_id": "event-archive",
                "session_id": "session-1",
                "sequence": 2,
                "occurred_at": 90,
                "causation": {
                    "kind": "event",
                    "id": "event-rename"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "session_archived"
                }
            },
            {
                "schema_version": 1,
                "event_id": "event-unarchive",
                "session_id": "session-1",
                "sequence": 3,
                "occurred_at": 80,
                "causation": {
                    "kind": "command",
                    "id": "command-unarchive"
                },
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "session_unarchived"
                }
            }
        ])
    );

    let renamed: EventEnvelope = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "event_id": "event-rename",
        "session_id": "session-1",
        "sequence": 1,
        "occurred_at": 100,
        "causation": {"kind": "command", "id": "command-rename"},
        "correlation_id": "correlation-1",
        "payload": {"kind": "session_renamed", "payload": {"title": "Deep dive: streaming"}}
    }))
    .expect("deserialize renamed event");
    assert_eq!(renamed, events[0]);
    let payload_kinds: Vec<_> = events
        .iter()
        .map(|event| serde_json::to_value(event.payload()).expect("serialize payload"))
        .collect();
    assert_eq!(
        serde_json::to_value(payload_kinds).expect("serialize payloads"),
        serde_json::json!([
            {"kind": "session_renamed", "payload": {"title": "Deep dive: streaming"}},
            {"kind": "session_archived"},
            {"kind": "session_unarchived"}
        ])
    );
}

#[test]
fn every_initial_command_payload_has_a_stable_serialized_shape() {
    let commands = [
        CommandEnvelope::new(
            command_id("command-create"),
            correlation_id(),
            CommandPayload::CreateSession {
                session_id: session_id(),
            },
        ),
        CommandEnvelope::new(
            command_id("command-select"),
            correlation_id(),
            CommandPayload::SelectModel {
                session_id: session_id(),
                model: model(),
            },
        ),
        CommandEnvelope::new(
            command_id("command-admit"),
            correlation_id(),
            CommandPayload::AdmitPrompt {
                session_id: session_id(),
                input_id: input_id(),
                prompt: prompt(),
                delivery_mode: DeliveryMode::NextTurn,
            },
        ),
    ];

    assert_eq!(
        serde_json::to_value(commands).expect("serialize commands"),
        serde_json::json!([
            {
                "command_id": "command-create",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "create_session",
                    "payload": {
                        "session_id": "session-1"
                    }
                }
            },
            {
                "command_id": "command-select",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "select_model",
                    "payload": {
                        "session_id": "session-1",
                        "model": {
                            "provider_id": "google-ai-studio",
                            "model_id": "models/gemini-test"
                        }
                    }
                }
            },
            {
                "command_id": "command-admit",
                "correlation_id": "correlation-1",
                "payload": {
                    "kind": "admit_prompt",
                    "payload": {
                        "session_id": "session-1",
                        "input_id": "input-1",
                        "prompt": "  first line\nsecond line: こんにちは  ",
                        "delivery_mode": "next_turn"
                    }
                }
            }
        ])
    );
}

#[test]
fn every_attempt_command_payload_has_a_stable_serialized_shape() {
    let commands = [
        CommandPayload::AdmitPromptAndPrepareAttempt {
            session_id: session_id(),
            input_id: input_id(),
            prompt: prompt(),
            delivery_mode: DeliveryMode::NextTurn,
            attempt_id: attempt_id("attempt-1"),
        },
        CommandPayload::PrepareAttempt {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            input_id: input_id(),
            retry_of: None,
        },
        CommandPayload::StartAttempt {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
        },
        CommandPayload::AppendAttemptText {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            text: ResponseText::new("hello\n世界").expect("non-empty response"),
        },
        CommandPayload::RecordAttemptUsage {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            usage: UsageSnapshot::new(Some(3), Some(5), Some(8)).with_breakdown(
                Some(2),
                Some(1),
                Some(0),
            ),
        },
        CommandPayload::RequestAttemptCancellation {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
        },
        CommandPayload::CompleteAttempt {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
        },
        CommandPayload::FailAttempt {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            failure: failure(),
        },
        CommandPayload::CancelAttempt {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
        },
        CommandPayload::MarkAttemptUnknown {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
        },
    ];

    assert_eq!(
        serde_json::to_value(commands).expect("serialize attempt commands"),
        serde_json::json!([
            {
                "kind": "admit_prompt_and_prepare_attempt",
                "payload": {
                    "session_id": "session-1",
                    "input_id": "input-1",
                    "prompt": "  first line\nsecond line: こんにちは  ",
                    "delivery_mode": "next_turn",
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "prepare_attempt",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "input_id": "input-1",
                    "retry_of": null
                }
            },
            {
                "kind": "start_attempt",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "append_attempt_text",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "text": "hello\n世界"
                }
            },
            {
                "kind": "record_attempt_usage",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "usage": {
                        "input_tokens": 3,
                        "output_tokens": 5,
                        "total_tokens": 8,
                        "cached_input_tokens": 2,
                        "reasoning_tokens": 1,
                        "tool_tokens": 0
                    }
                }
            },
            {
                "kind": "request_attempt_cancellation",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "complete_attempt",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "fail_attempt",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "failure": {
                        "class": "unavailable",
                        "code": "provider_unavailable",
                        "message": "The provider is temporarily unavailable",
                        "retry_advice": {
                            "kind": "after",
                            "delay_ms": 250
                        }
                    }
                }
            },
            {
                "kind": "cancel_attempt",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "mark_attempt_unknown",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1"
                }
            }
        ])
    );
}

#[test]
fn every_attempt_event_payload_has_a_stable_serialized_shape() {
    let events = [
        EventPayload::AttemptPrepared {
            attempt_id: attempt_id("attempt-1"),
            input_id: input_id(),
            model: model(),
            retry_of: Some(attempt_id("attempt-0")),
        },
        EventPayload::AttemptStarted {
            attempt_id: attempt_id("attempt-1"),
        },
        EventPayload::AttemptTextAppended {
            attempt_id: attempt_id("attempt-1"),
            text: ResponseText::new("hello\n世界").expect("non-empty response"),
        },
        EventPayload::AttemptUsageRecorded {
            attempt_id: attempt_id("attempt-1"),
            usage: UsageSnapshot::new(Some(3), Some(5), Some(8)).with_breakdown(
                Some(2),
                Some(1),
                Some(0),
            ),
        },
        EventPayload::AttemptCancellationRequested {
            attempt_id: attempt_id("attempt-1"),
        },
        EventPayload::AttemptCompleted {
            attempt_id: attempt_id("attempt-1"),
        },
        EventPayload::AttemptFailed {
            attempt_id: attempt_id("attempt-1"),
            failure: failure(),
        },
        EventPayload::AttemptCancelled {
            attempt_id: attempt_id("attempt-1"),
        },
        EventPayload::AttemptMarkedUnknown {
            attempt_id: attempt_id("attempt-1"),
        },
    ];

    assert_eq!(
        serde_json::to_value(events).expect("serialize attempt events"),
        serde_json::json!([
            {
                "kind": "attempt_prepared",
                "payload": {
                    "attempt_id": "attempt-1",
                    "input_id": "input-1",
                    "model": {
                        "provider_id": "google-ai-studio",
                        "model_id": "models/gemini-test"
                    },
                    "retry_of": "attempt-0"
                }
            },
            {
                "kind": "attempt_started",
                "payload": {
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "attempt_text_appended",
                "payload": {
                    "attempt_id": "attempt-1",
                    "text": "hello\n世界"
                }
            },
            {
                "kind": "attempt_usage_recorded",
                "payload": {
                    "attempt_id": "attempt-1",
                    "usage": {
                        "input_tokens": 3,
                        "output_tokens": 5,
                        "total_tokens": 8,
                        "cached_input_tokens": 2,
                        "reasoning_tokens": 1,
                        "tool_tokens": 0
                    }
                }
            },
            {
                "kind": "attempt_cancellation_requested",
                "payload": {
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "attempt_completed",
                "payload": {
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "attempt_failed",
                "payload": {
                    "attempt_id": "attempt-1",
                    "failure": {
                        "class": "unavailable",
                        "code": "provider_unavailable",
                        "message": "The provider is temporarily unavailable",
                        "retry_advice": {
                            "kind": "after",
                            "delay_ms": 250
                        }
                    }
                }
            },
            {
                "kind": "attempt_cancelled",
                "payload": {
                    "attempt_id": "attempt-1"
                }
            },
            {
                "kind": "attempt_marked_unknown",
                "payload": {
                    "attempt_id": "attempt-1"
                }
            }
        ])
    );
}

#[test]
fn error_classification_shapes_are_explicit() {
    assert_eq!(
        serde_json::to_value([
            RetryAdvice::Never,
            RetryAdvice::Immediate,
            RetryAdvice::Backoff,
            RetryAdvice::After { delay_ms: 250 },
        ])
        .expect("serialize retry advice"),
        serde_json::json!([
            { "kind": "never" },
            { "kind": "immediate" },
            { "kind": "backoff" },
            { "kind": "after", "delay_ms": 250 }
        ])
    );
    assert_eq!(
        serde_json::to_value([
            ErrorClass::Validation,
            ErrorClass::NotFound,
            ErrorClass::Conflict,
            ErrorClass::Authentication,
            ErrorClass::PermissionDenied,
            ErrorClass::RateLimited,
            ErrorClass::Timeout,
            ErrorClass::Unavailable,
            ErrorClass::Cancelled,
            ErrorClass::Protocol,
            ErrorClass::Storage,
            ErrorClass::Internal,
        ])
        .expect("serialize error classes"),
        serde_json::json!([
            "validation",
            "not_found",
            "conflict",
            "authentication",
            "permission_denied",
            "rate_limited",
            "timeout",
            "unavailable",
            "cancelled",
            "protocol",
            "storage",
            "internal"
        ])
    );
}

#[test]
fn enclosing_command_and_event_debug_output_redacts_prompt_content() {
    let secret = "wrapper-level secret prompt";
    let prompt = PromptText::new(secret).expect("non-empty test prompt");
    let command = CommandEnvelope::new(
        command_id("command-admit"),
        correlation_id(),
        CommandPayload::AdmitPrompt {
            session_id: session_id(),
            input_id: input_id(),
            prompt: prompt.clone(),
            delivery_mode: DeliveryMode::NextTurn,
        },
    );
    let event = EventEnvelope::new_v1(
        event_id("event-1"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-admit")),
        correlation_id(),
        EventPayload::InputAdmitted {
            input_id: input_id(),
            prompt,
            delivery_mode: DeliveryMode::NextTurn,
        },
    );

    assert!(!format!("{command:?}").contains(secret));
    assert!(!format!("{event:?}").contains(secret));
}

#[test]
fn enclosing_attempt_debug_output_redacts_response_and_public_message_content() {
    let response_secret = "wrapper-level secret response";
    let message_secret = "wrapper-level secret public message";
    let response = ResponseText::new(response_secret).expect("non-empty response");
    let failure = AttemptFailure::new(
        ErrorClass::Unavailable,
        ErrorCode::new("provider_unavailable").expect("valid code"),
        PublicMessage::new(message_secret).expect("valid public message"),
        RetryAdvice::Backoff,
    );
    let command = CommandEnvelope::new(
        command_id("command-text"),
        correlation_id(),
        CommandPayload::AppendAttemptText {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            text: response.clone(),
        },
    );
    let event = EventEnvelope::new_v1(
        event_id("event-text"),
        session_id(),
        SessionSequence::FIRST,
        TimestampMillis::new(1),
        Causation::Command(command_id("command-text")),
        correlation_id(),
        EventPayload::AttemptTextAppended {
            attempt_id: attempt_id("attempt-1"),
            text: response,
        },
    );
    let failed = EventPayload::AttemptFailed {
        attempt_id: attempt_id("attempt-1"),
        failure,
    };

    assert!(!format!("{command:?}").contains(response_secret));
    assert!(!format!("{event:?}").contains(response_secret));
    assert!(!format!("{failed:?}").contains(message_secret));
}

#[test]
fn phase_three_authority_contracts_have_stable_serialized_shapes() {
    let limits = RunLimits::new(4, 30_000, 20_000, 65_536, 2).expect("valid run limits");
    let decision_id =
        PermissionDecisionId::new("permission-1").expect("valid permission decision ID");
    let output = ToolOutput::new(
        "hello",
        Some(
            ArtifactRef::new(
                ArtifactId::new("sha256:aaaaaaaa").expect("valid artifact ID"),
                10,
                "text/plain",
            )
            .expect("valid artifact"),
        ),
        10,
        true,
    )
    .expect("valid bounded tool output");
    let commands = [
        CommandPayload::ConfigureRunBudget {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            limits,
        },
        CommandPayload::ProposeToolCall {
            session_id: session_id(),
            attempt_id: attempt_id("attempt-1"),
            call: tool_call(),
        },
        CommandPayload::RecordToolPermission {
            session_id: session_id(),
            tool_call_id: ToolCallId::new("tool-call-1").expect("valid tool call ID"),
            decision_id: decision_id.clone(),
            outcome: PermissionOutcome::Ask,
        },
        CommandPayload::AnswerToolPermission {
            session_id: session_id(),
            tool_call_id: ToolCallId::new("tool-call-1").expect("valid tool call ID"),
            decision_id,
            answer: PermissionAnswer::AllowOnce,
        },
        CommandPayload::CompleteToolCall {
            session_id: session_id(),
            tool_call_id: ToolCallId::new("tool-call-1").expect("valid tool call ID"),
            output,
        },
    ];

    assert_eq!(
        serde_json::to_value(commands).expect("serialize authority commands"),
        serde_json::json!([
            {
                "kind": "configure_run_budget",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "limits": {
                        "max_turns": 4,
                        "max_time_ms": 30000,
                        "max_tokens": 20000,
                        "max_output_bytes": 65536,
                        "max_concurrency": 2
                    }
                }
            },
            {
                "kind": "propose_tool_call",
                "payload": {
                    "session_id": "session-1",
                    "attempt_id": "attempt-1",
                    "call": {
                        "tool_call_id": "tool-call-1",
                        "provider_call_id": "provider-call-1",
                        "tool_name": "fs_write",
                        "schema_version": 1,
                        "arguments": {
                            "path": "notes.txt",
                            "content": "hello"
                        },
                        "capability": {
                            "kind": "filesystem_write",
                            "resource": "workspace:notes.txt"
                        }
                    }
                }
            },
            {
                "kind": "record_tool_permission",
                "payload": {
                    "session_id": "session-1",
                    "tool_call_id": "tool-call-1",
                    "decision_id": "permission-1",
                    "outcome": "ask"
                }
            },
            {
                "kind": "answer_tool_permission",
                "payload": {
                    "session_id": "session-1",
                    "tool_call_id": "tool-call-1",
                    "decision_id": "permission-1",
                    "answer": "allow_once"
                }
            },
            {
                "kind": "complete_tool_call",
                "payload": {
                    "session_id": "session-1",
                    "tool_call_id": "tool-call-1",
                    "output": {
                        "content": "hello",
                        "artifact": {
                            "artifact_id": "sha256:aaaaaaaa",
                            "byte_len": 10,
                            "media_type": "text/plain"
                        },
                        "original_bytes": 10,
                        "truncated": true
                    }
                }
            }
        ])
    );

    let proposed = EventPayload::ToolCallProposed {
        attempt_id: attempt_id("attempt-1"),
        call: tool_call(),
    };
    assert_eq!(
        serde_json::to_value(proposed).expect("serialize tool event"),
        serde_json::json!({
            "kind": "tool_call_proposed",
            "payload": {
                "attempt_id": "attempt-1",
                "call": {
                    "tool_call_id": "tool-call-1",
                    "provider_call_id": "provider-call-1",
                    "tool_name": "fs_write",
                    "schema_version": 1,
                    "arguments": {
                        "path": "notes.txt",
                        "content": "hello"
                    },
                    "capability": {
                        "kind": "filesystem_write",
                        "resource": "workspace:notes.txt"
                    }
                }
            }
        })
    );
}
