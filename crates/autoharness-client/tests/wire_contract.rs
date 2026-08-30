use autoharness_client::*;
use serde_json::{Value, json};

fn request_id(value: u64) -> RequestId {
    RequestId::new(value).expect("positive request identity")
}

fn session_id(value: &str) -> SessionId {
    SessionId::new(value).expect("valid session identity")
}

fn attempt_id(value: &str) -> AttemptId {
    AttemptId::new(value).expect("valid attempt identity")
}

fn tool_call_id(value: &str) -> ToolCallId {
    ToolCallId::new(value).expect("valid tool-call identity")
}

fn model_ref() -> ModelRef {
    ModelRef::new(
        ProviderId::new("gemini").expect("valid provider identity"),
        ModelId::new("models/gemini-test").expect("valid model identity"),
    )
}

fn model_summary() -> ModelSummary {
    ModelSummary::new(
        model_ref(),
        "Gemini Test",
        "Streaming chat, reasoning, and tools",
        Some(1_048_576),
        true,
        CapabilitySupport::Supported,
        CapabilitySupport::Supported,
        CapabilitySupport::Supported,
        CapabilitySupport::Supported,
    )
    .expect("valid model summary")
}

fn sample_snapshot() -> ClientSnapshot {
    let identity = session_id("session-1");
    let permission = PermissionRequest::new(
        tool_call_id("tool-1"),
        "workspace_read_v1",
        "filesystem_read",
        "workspace/private.txt",
        vec![
            PermissionDetail::new("Path", "workspace/private.txt")
                .expect("valid permission detail"),
        ],
    )
    .expect("valid permission");
    let active = SessionProjection::new(
        identity.clone(),
        SessionRevision::new(7),
        Some(model_ref()),
        vec![
            TranscriptItem::User {
                input_id: InputId::new("input-1").expect("valid input identity"),
                content: TranscriptContent::new("hello").expect("valid transcript content"),
            },
            TranscriptItem::Assistant {
                attempt_id: attempt_id("attempt-1"),
                content: TranscriptContent::new("hello back").expect("valid transcript content"),
                state: AttemptState::Streaming,
                usage: Some(UsageProjection {
                    input_tokens: Some(DecimalU64::new(8)),
                    output_tokens: Some(DecimalU64::new(2)),
                    ..UsageProjection::default()
                }),
                retry_of: None,
            },
        ],
        vec![permission],
    )
    .expect("valid session projection");
    ClientSnapshot::new(
        ClientLifecycle::Ready,
        Some(identity.clone()),
        vec![SessionSummary::new(
            identity,
            SessionTitle::new("New conversation").expect("valid title"),
            Some(7),
            Some(model_ref()),
            Some(1_788_100_000_000),
            Some(2),
            false,
        )],
        Some(active),
        CatalogProjection::ready(3, vec![model_summary()], false).expect("valid catalog"),
        vec![
            ProviderProjection::new(
                ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
                ProviderId::new("gemini").expect("valid provider identity"),
                "Google AI Studio",
                true,
                ProviderStatus::Ready,
                CredentialSource::Environment,
                Some(model_ref()),
            )
            .expect("valid provider projection"),
        ],
    )
    .expect("valid client snapshot")
}

#[test]
fn request_ids_and_revisions_are_json_strings() {
    assert_eq!(
        serde_json::to_value(request_id(u64::MAX)).expect("serialize request identity"),
        json!(u64::MAX.to_string())
    );
    assert_eq!(
        serde_json::to_value(TransportRevision::new(u64::MAX).expect("positive revision"))
            .expect("serialize revision"),
        json!(u64::MAX.to_string())
    );
    assert_eq!(
        serde_json::to_value(SessionRevision::new(0)).expect("serialize session revision"),
        json!("0")
    );
    assert!(serde_json::from_value::<RequestId>(json!(1)).is_err());
    assert!(serde_json::from_value::<RequestId>(json!("01")).is_err());
    assert!(serde_json::from_value::<RequestId>(json!("0")).is_err());
    assert!(RequestId::new(0).is_err());
    assert!(
        TransportRevision::new(u64::MAX)
            .expect("valid")
            .next()
            .is_err()
    );
}

#[test]
fn every_unbounded_numeric_wire_value_is_an_exact_decimal_string() {
    let value = serde_json::to_value(sample_snapshot()).expect("serialize snapshot");
    assert_eq!(
        value["sessions"][0]["updated_at_ms"],
        json!("1788100000000")
    );
    assert_eq!(value["sessions"][0]["message_count"], json!("2"));
    assert_eq!(value["catalog"]["payload"]["generation"], json!("3"));
    assert_eq!(
        value["catalog"]["payload"]["models"][0]["context_window_tokens"],
        json!("1048576")
    );
    assert_eq!(
        value["active_session"]["transcript"][1]["payload"]["usage"]["input_tokens"],
        json!("8")
    );

    let retry = RetryDirective::after(MAX_RETRY_DELAY_MS).expect("bounded retry delay");
    assert_eq!(
        serde_json::to_value(retry).expect("serialize retry"),
        json!({"kind":"after", "payload":{"delay_ms": MAX_RETRY_DELAY_MS.to_string()}})
    );
    assert!(
        serde_json::from_value::<RetryDirective>(
            json!({"kind":"after", "payload":{"delay_ms": 1000}})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<UnixMillis>(json!(1_788_100_000_000_i64)).is_err());
    assert!(serde_json::from_value::<DecimalU64>(json!(u64::MAX)).is_err());
    assert_eq!(
        serde_json::from_value::<DecimalU64>(json!(u64::MAX.to_string()))
            .expect("exact maximum unsigned value")
            .get(),
        u64::MAX
    );
}

#[test]
fn synthetic_session_summary_preserves_unknown_durable_metadata() {
    let summary = SessionSummary::new(
        session_id("session-synthetic"),
        SessionTitle::new("New session").expect("valid title"),
        None,
        None,
        None,
        None,
        false,
    );

    let value = serde_json::to_value(&summary).expect("serialize synthetic summary");
    assert_eq!(value["revision"], serde_json::Value::Null);
    assert_eq!(value["updated_at_ms"], serde_json::Value::Null);
    assert_eq!(value["message_count"], serde_json::Value::Null);
    assert_eq!(
        serde_json::from_value::<SessionSummary>(value).expect("deserialize synthetic summary"),
        summary
    );
}

#[test]
fn commands_are_versioned_and_have_no_secret_variant_or_client_request_id() {
    let envelope = CommandEnvelope::new(ClientCommand::SubmitPrompt {
        session_id: session_id("session-1"),
        prompt: PromptContent::new("public prompt").expect("valid prompt"),
    });
    let value = serde_json::to_value(&envelope).expect("serialize command");

    assert_eq!(value["schema_version"], json!(CLIENT_SCHEMA_VERSION));
    assert_eq!(value["command"]["kind"], json!("submit_prompt"));
    assert!(value.get("request_id").is_none());
    let encoded =
        serde_json::to_string(&ClientCommand::RequestShutdown).expect("serialize shutdown command");
    assert!(!encoded.contains("credential"));
    assert!(!format!("{:?}", envelope.command).contains("public prompt"));

    let unsupported = json!({
        "schema_version": 2,
        "command": { "kind": "create_session" }
    });
    assert!(serde_json::from_value::<CommandEnvelope>(unsupported).is_err());

    let smuggled_secret = json!({
        "schema_version": 1,
        "command": {
            "kind": "submit_prompt",
            "payload": {
                "session_id": "session-1",
                "prompt": "public prompt",
                "credential": "must-not-enter-the-command-contract"
            }
        }
    });
    assert!(serde_json::from_value::<CommandEnvelope>(smuggled_secret).is_err());

    let unknown_envelope_field = json!({
        "schema_version": 1,
        "command": { "kind": "create_session" },
        "future_field": true
    });
    assert!(serde_json::from_value::<CommandEnvelope>(unknown_envelope_field).is_err());
}

#[test]
fn dedicated_secret_ingress_is_bounded_and_debug_redacted() {
    let sentinel = "credential-sentinel-value";
    let ingress = SecretIngress::new(
        ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
        sentinel,
    )
    .expect("valid ingress");

    assert_eq!(ingress.credential(), sentinel);
    assert!(!format!("{ingress:?}").contains(sentinel));
    assert!(
        SecretIngress::new(
            ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
            "x".repeat(MAX_CREDENTIAL_BYTES + 1),
        )
        .is_err()
    );
}

#[test]
fn complete_snapshot_round_trips_and_rejects_inconsistent_active_state() {
    let snapshot = sample_snapshot();
    let encoded = serde_json::to_string(&snapshot).expect("serialize snapshot");
    let decoded: ClientSnapshot = serde_json::from_str(&encoded).expect("deserialize snapshot");
    assert_eq!(decoded, snapshot);

    let mut value: Value = serde_json::from_str(&encoded).expect("snapshot json");
    value["active_session_id"] = json!("missing-session");
    assert!(serde_json::from_value::<ClientSnapshot>(value).is_err());

    let mut wrong_version = serde_json::to_value(&snapshot).expect("snapshot json");
    wrong_version["schema_version"] = json!(99);
    assert!(serde_json::from_value::<ClientSnapshot>(wrong_version).is_err());
}

#[test]
fn connection_identity_is_distinct_from_adapter_identity() {
    let base = sample_snapshot();
    let second = ProviderProjection::new(
        ConnectionId::new("connection-gemini-secondary").expect("valid connection identity"),
        ProviderId::new("gemini").expect("valid provider identity"),
        "Gemini secondary",
        false,
        ProviderStatus::Offline,
        CredentialSource::Vault,
        Some(model_ref()),
    )
    .expect("same adapter may own another connection");
    assert!(
        ClientSnapshot::new(
            base.lifecycle.clone(),
            base.active_session_id.clone(),
            base.sessions.clone(),
            base.active_session.clone(),
            base.catalog.clone(),
            vec![base.providers[0].clone(), second],
        )
        .is_ok()
    );

    let duplicate_connection = ProviderProjection::new(
        base.providers[0].connection_id.clone(),
        ProviderId::new("gemini").expect("valid provider identity"),
        "Duplicate connection identity",
        false,
        ProviderStatus::Offline,
        CredentialSource::Vault,
        Some(model_ref()),
    )
    .expect("projection is locally valid before snapshot uniqueness validation");
    assert!(
        ClientSnapshot::new(
            base.lifecycle,
            base.active_session_id,
            base.sessions,
            base.active_session,
            base.catalog,
            vec![base.providers[0].clone(), duplicate_connection],
        )
        .is_err()
    );
}

#[test]
fn content_tool_and_permission_debug_forms_are_redacted() {
    let sentinel = "private-content-sentinel";
    let content = TranscriptContent::new(sentinel).expect("valid transcript content");
    let detail = PermissionDetail::new("Command", sentinel).expect("valid permission detail");
    let permission = PermissionRequest::new(
        tool_call_id("tool-private"),
        "process_execute_v1",
        "process_execute",
        sentinel,
        vec![detail.clone()],
    )
    .expect("valid permission request");
    let tool = ToolCallProjection::new(
        tool_call_id("tool-private"),
        "process_execute_v1",
        "process_execute",
        sentinel,
        ToolCallState::Running,
        Some(sentinel.to_owned()),
    )
    .expect("valid tool call");
    let failure = SafeFailure::new(
        FailureClass::Validation,
        "invalid_request",
        sentinel,
        RetryDirective::Never,
    )
    .expect("valid safe failure");

    assert!(!format!("{content:?}").contains(sentinel));
    assert!(!format!("{detail:?}").contains(sentinel));
    assert!(!format!("{permission:?}").contains(sentinel));
    assert!(!format!("{tool:?}").contains(sentinel));
    assert!(!format!("{failure:?}").contains(sentinel));
}

#[test]
fn constructors_and_deserializers_enforce_content_and_collection_bounds() {
    assert!(PromptContent::new("x".repeat(MAX_PROMPT_BYTES)).is_ok());
    assert!(PromptContent::new("x".repeat(MAX_PROMPT_BYTES + 1)).is_err());
    assert!(PromptContent::new(" \t").is_err());
    assert!(SessionId::new(format!(" {}", "x".repeat(4))).is_err());

    let permission = PermissionRequest::new(
        tool_call_id("tool-many-details"),
        "workspace_read_v1",
        "filesystem_read",
        "workspace/file.txt",
        (0..=MAX_PERMISSION_DETAILS)
            .map(|index| {
                PermissionDetail::new(format!("Field {index}"), "value")
                    .expect("valid permission detail")
            })
            .collect(),
    );
    assert!(permission.is_err());

    let oversized = json!({
        "tool_call_id": "tool-many-details",
        "tool_name": "workspace_read_v1",
        "capability": "filesystem_read",
        "resource": "workspace/file.txt",
        "details": (0..=MAX_PERMISSION_DETAILS)
            .map(|index| json!({"label": format!("Field {index}"), "value": "value"}))
            .collect::<Vec<_>>()
    });
    assert!(serde_json::from_value::<PermissionRequest>(oversized).is_err());
}

#[test]
fn frames_detect_gaps_stale_data_and_authoritative_resynchronization() {
    let snapshot = sample_snapshot();
    let initial = ServerFrame::snapshot(
        TransportRevision::INITIAL,
        SnapshotReason::Initial,
        snapshot.clone(),
    );
    assert_eq!(initial.classify_after(None), FrameDisposition::Baseline);

    let second = ServerFrame::notice(
        TransportRevision::new(2).expect("valid revision"),
        ClientNotice::CommandCommitted {
            request_id: request_id(9),
        },
    );
    assert_eq!(
        second.classify_after(Some(TransportRevision::INITIAL)),
        FrameDisposition::Next
    );
    assert_eq!(
        second.classify_after(Some(TransportRevision::new(2).expect("valid revision"))),
        FrameDisposition::Stale {
            last_applied: TransportRevision::new(2).expect("valid revision"),
            received: TransportRevision::new(2).expect("valid revision"),
        }
    );

    let gap = ServerFrame::notice(
        TransportRevision::new(5).expect("valid revision"),
        ClientNotice::Shutdown {
            state: ShutdownState::Requested,
        },
    );
    assert_eq!(
        gap.classify_after(Some(TransportRevision::new(2).expect("valid revision"))),
        FrameDisposition::Gap {
            expected: TransportRevision::new(3).expect("valid revision"),
            received: TransportRevision::new(5).expect("valid revision"),
        }
    );
    assert!(matches!(
        gap.classify_after(None),
        FrameDisposition::InvalidBaseline { .. }
    ));

    let resync = ServerFrame::snapshot(
        TransportRevision::new(6).expect("valid revision"),
        SnapshotReason::Resynchronization,
        snapshot,
    );
    assert_eq!(
        resync.classify_after(Some(TransportRevision::new(2).expect("valid revision"))),
        FrameDisposition::Baseline
    );

    let encoded = serde_json::to_value(resync).expect("serialize resync frame");
    assert_eq!(encoded["revision"], json!("6"));
    assert_eq!(encoded["payload"]["kind"], json!("snapshot"));

    let mut unknown_frame = encoded.clone();
    unknown_frame["future_field"] = json!(true);
    assert!(serde_json::from_value::<ServerFrame>(unknown_frame).is_err());

    let mut unknown_snapshot = encoded;
    unknown_snapshot["payload"]["payload"]["snapshot"]["future_field"] = json!(true);
    assert!(serde_json::from_value::<ServerFrame>(unknown_snapshot).is_err());
}

#[test]
fn notices_preserve_request_correlation() {
    let committed = ClientNotice::CommandCommitted {
        request_id: request_id(41),
    };
    let rejected = ClientNotice::CommandRejected {
        request_id: request_id(42),
        failure: SafeFailure::new(
            FailureClass::Conflict,
            "stale_revision",
            "The durable state changed before this request committed.",
            RetryDirective::Immediate,
        )
        .expect("valid safe failure"),
    };
    assert_eq!(committed.request_id(), Some(request_id(41)));
    assert_eq!(rejected.request_id(), Some(request_id(42)));
    assert_eq!(
        ClientNotice::Shutdown {
            state: ShutdownState::Ready
        }
        .request_id(),
        None
    );
}
