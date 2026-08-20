use autoharness_domain::{
    Causation, CommandEnvelope, CommandId, CommandPayload, CorrelationId, DeliveryMode, ErrorClass,
    EventEnvelope, EventId, EventPayload, InputId, ModelId, ModelRef, PromptText, ProviderId,
    RetryAdvice, SessionId, SessionSequence, TimestampMillis,
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

fn model() -> ModelRef {
    ModelRef::new(
        ProviderId::new("google-ai-studio").expect("valid provider ID"),
        ModelId::new("models/gemini-test").expect("valid model ID"),
    )
}

fn prompt() -> PromptText {
    PromptText::new("  first line\nsecond line: こんにちは  ").expect("non-empty test prompt")
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
