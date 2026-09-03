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

fn gemini_configuration() -> ProviderConfiguration {
    ProviderConfiguration::new(ProviderKind::Gemini, None, None, None)
        .expect("valid Gemini configuration")
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
                gemini_configuration(),
                ProviderProfileScope::Named,
                true,
                ProviderStatus::Ready,
                CredentialSource::Environment,
                ProviderCredentialState::Disconnected,
                Some(model_ref()),
                Some(ReasoningEffort::High),
            )
            .expect("valid provider projection"),
        ],
        0,
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
    assert_eq!(value["provider_recovery_pending"], json!("0"));
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
        "schema_version": CLIENT_SCHEMA_VERSION + 1,
        "command": { "kind": "create_session" }
    });
    assert!(serde_json::from_value::<CommandEnvelope>(unsupported).is_err());

    let smuggled_secret = json!({
        "schema_version": CLIENT_SCHEMA_VERSION,
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
        "schema_version": CLIENT_SCHEMA_VERSION,
        "command": { "kind": "create_session" },
        "future_field": true
    });
    assert!(serde_json::from_value::<CommandEnvelope>(unknown_envelope_field).is_err());
}

#[test]
fn session_lifecycle_commands_are_bounded_and_exactly_scoped() {
    let rename = CommandEnvelope::new(ClientCommand::RenameSession {
        session_id: session_id("session-1"),
        title: SessionTitle::new("Exact title").expect("valid title"),
    });
    assert_eq!(
        serde_json::to_value(rename).expect("serialize rename"),
        json!({
            "schema_version": CLIENT_SCHEMA_VERSION,
            "command": {
                "kind": "rename_session",
                "payload": { "session_id": "session-1", "title": "Exact title" }
            }
        })
    );

    let empty_title = json!({
        "schema_version": CLIENT_SCHEMA_VERSION,
        "command": {
            "kind": "rename_session",
            "payload": { "session_id": "session-1", "title": " " }
        }
    });
    assert!(serde_json::from_value::<CommandEnvelope>(empty_title).is_err());

    for command in [
        ClientCommand::ArchiveSession {
            session_id: session_id("session-1"),
        },
        ClientCommand::UnarchiveSession {
            session_id: session_id("session-1"),
        },
        ClientCommand::ExportTranscript {
            session_id: session_id("session-1"),
        },
        ClientCommand::DeleteSession {
            session_id: session_id("session-1"),
        },
    ] {
        let encoded = serde_json::to_value(CommandEnvelope::new(command))
            .expect("serialize lifecycle command");
        assert_eq!(
            encoded["command"]["payload"]["session_id"],
            json!("session-1")
        );
    }
}

#[test]
fn provider_management_commands_are_bounded_typed_and_secret_free() {
    let profile = ProviderProfileInput::new(
        ConnectionId::new("work-router").expect("profile identity"),
        ProviderConfiguration::new(
            ProviderKind::Router,
            Some("https://router.example.test/v1".to_owned()),
            Some("workspace-a".to_owned()),
            Some("x-api-key".to_owned()),
        )
        .expect("router configuration"),
    )
    .expect("complete profile input");
    let upsert = CommandEnvelope::new(ClientCommand::UpsertProviderProfile { profile });
    let encoded = serde_json::to_value(upsert).expect("serialize profile command");
    assert_eq!(
        encoded["command"]["payload"]["profile"]["configuration"]["kind"],
        json!("router")
    );
    assert_eq!(
        encoded["command"]["payload"]["profile"]["connection_id"],
        json!("work-router")
    );
    assert!(!encoded.to_string().contains("credential"));

    let defaults = CommandEnvelope::new(ClientCommand::SetProviderDefaults {
        connection_id: ConnectionId::new("work-router").expect("profile identity"),
        model: ModelRef::new(
            ProviderId::new("router:workspace-a").expect("provider identity"),
            ModelId::new("agent-model").expect("model identity"),
        ),
        reasoning_effort: Some(ReasoningEffort::Xhigh),
    });
    let encoded = serde_json::to_value(defaults).expect("serialize defaults command");
    assert_eq!(
        encoded["command"]["payload"]["reasoning_effort"],
        json!("xhigh")
    );

    let cancel = CommandEnvelope::new(ClientCommand::CancelCodexAuthentication {
        authentication_request_id: request_id(41),
    });
    let encoded = serde_json::to_value(cancel).expect("serialize cancellation");
    assert_eq!(
        encoded["command"]["payload"]["authentication_request_id"],
        json!("41")
    );

    let incomplete_router = json!({
        "schema_version": CLIENT_SCHEMA_VERSION,
        "command": {
            "kind": "upsert_provider_profile",
            "payload": {
                "profile": {
                    "connection_id": "work-router",
                    "configuration": {
                        "kind": "router",
                        "base_url": null,
                        "project": null,
                        "auth_header": null
                    }
                }
            }
        }
    });
    assert!(serde_json::from_value::<CommandEnvelope>(incomplete_router).is_err());
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
    assert_eq!(ingress.operation(), CredentialOperation::SessionOnly);
    assert!(!format!("{ingress:?}").contains(sentinel));
}

#[test]
fn saved_secret_ingress_carries_only_a_non_secret_operation_label() {
    let sentinel = "saved-credential-sentinel";
    let ingress = SecretIngress::with_operation(
        ConnectionId::new("profile-work").expect("valid connection identity"),
        CredentialOperation::Replace,
        sentinel,
    )
    .expect("valid ingress");

    assert_eq!(ingress.operation(), CredentialOperation::Replace);
    let debug = format!("{ingress:?}");
    assert!(debug.contains("Replace"));
    assert!(!debug.contains(sentinel));
}

#[test]
fn provider_configuration_is_kind_scoped_and_debug_redacted() {
    let router = ProviderConfiguration::new(
        ProviderKind::Router,
        Some("https://private-router.example.test/v1".to_owned()),
        Some("workspace-a".to_owned()),
        Some("x-api-key".to_owned()),
    )
    .expect("valid router configuration");
    let debug = format!("{router:?}");
    assert!(debug.contains("has_base_url: true"));
    assert!(!debug.contains("private-router"));

    assert!(
        ProviderConfiguration::new(
            ProviderKind::Gemini,
            Some("https://must-not-apply.example".to_owned()),
            None,
            None,
        )
        .is_err()
    );
    let incomplete = ProviderConfiguration::new(ProviderKind::Router, None, None, None)
        .expect("projection may omit externally configured router details");
    assert!(
        ProviderProfileInput::new(
            ConnectionId::new("router-profile").expect("profile identity"),
            incomplete,
        )
        .is_err()
    );
}

#[test]
fn secret_ingress_accepts_exact_visible_ascii_boundary() {
    let credential = "x".repeat(MAX_CREDENTIAL_BYTES);
    let ingress = SecretIngress::new(
        ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
        credential.clone(),
    )
    .expect("4096 visible ASCII bytes must be accepted");

    assert_eq!(ingress.credential(), credential);
}

#[test]
fn secret_ingress_rejects_values_above_legacy_boundary() {
    let error = SecretIngress::new(
        ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
        "x".repeat(MAX_CREDENTIAL_BYTES + 1),
    )
    .expect_err("4097 bytes must be rejected");

    assert_eq!(
        error,
        ValidationError::TooLong {
            field: "credential",
            max_bytes: MAX_CREDENTIAL_BYTES,
            actual_bytes: MAX_CREDENTIAL_BYTES + 1,
        }
    );
}

#[test]
fn secret_ingress_rejects_empty_whitespace_and_control_characters() {
    for credential in [
        "", " ", "abc def", "abc\tdef", "abc\ndef", "abc\rdef", "abc\0def",
    ] {
        assert!(
            SecretIngress::new(
                ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
                credential,
            )
            .is_err(),
            "credential containing non-graphic ASCII was accepted"
        );
    }
}

#[test]
fn secret_ingress_rejects_non_ascii_characters() {
    assert!(
        SecretIngress::new(
            ConnectionId::new("connection-gemini-primary").expect("valid connection identity"),
            "credential-é",
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
fn active_session_delta_scales_with_changed_rows_and_rebuilds_snapshot() {
    let mut previous = sample_snapshot();
    let active = previous.active_session.as_mut().expect("active session");
    active.transcript = (0..1_000)
        .map(|index| TranscriptItem::User {
            input_id: InputId::new(format!("history-{index}")).expect("valid input identity"),
            content: TranscriptContent::new(format!("unchanged history row {index}"))
                .expect("valid transcript content"),
        })
        .chain([TranscriptItem::Assistant {
            attempt_id: attempt_id("attempt-streaming"),
            content: TranscriptContent::new("partial").expect("valid transcript content"),
            state: AttemptState::Streaming,
            usage: None,
            retry_of: None,
        }])
        .collect();

    let mut next = previous.clone();
    let next_active = next.active_session.as_mut().expect("active session");
    next_active.revision = SessionRevision::new(8);
    next_active.transcript[1_000] = TranscriptItem::Assistant {
        attempt_id: attempt_id("attempt-streaming"),
        content: TranscriptContent::new("partial response extended")
            .expect("valid transcript content"),
        state: AttemptState::Streaming,
        usage: None,
        retry_of: None,
    };
    next.sessions[0] = SessionSummary::new(
        session_id("session-1"),
        SessionTitle::new("New conversation").expect("valid title"),
        Some(8),
        Some(model_ref()),
        Some(1_788_100_000_001),
        Some(1_001),
        false,
    );

    let delta = ActiveSessionDelta::between(&previous, &next).expect("localized delta");
    assert_eq!(delta.transcript.start, 1_000);
    assert_eq!(delta.transcript.delete_count, 1);
    assert_eq!(delta.transcript.items.len(), 1);
    assert_eq!(delta.apply_to(&previous).expect("apply delta"), next);

    let encoded = serde_json::to_string(&delta).expect("serialize delta");
    assert!(!encoded.contains("unchanged history row 999"));
    let decoded = serde_json::from_str::<ActiveSessionDelta>(&encoded).expect("deserialize delta");
    assert_eq!(decoded, delta);
}

#[test]
fn connection_identity_is_distinct_from_adapter_identity() {
    let base = sample_snapshot();
    let second = ProviderProjection::new(
        ConnectionId::new("connection-gemini-secondary").expect("valid connection identity"),
        ProviderId::new("gemini").expect("valid provider identity"),
        "Gemini secondary",
        gemini_configuration(),
        ProviderProfileScope::Named,
        false,
        ProviderStatus::Untested,
        CredentialSource::Vault,
        ProviderCredentialState::Stored,
        Some(model_ref()),
        None,
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
            0,
        )
        .is_ok()
    );

    let duplicate_connection = ProviderProjection::new(
        base.providers[0].connection_id.clone(),
        ProviderId::new("gemini").expect("valid provider identity"),
        "Duplicate connection identity",
        gemini_configuration(),
        ProviderProfileScope::Named,
        false,
        ProviderStatus::Untested,
        CredentialSource::Vault,
        ProviderCredentialState::Stored,
        Some(model_ref()),
        None,
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
            0,
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
fn permission_wire_rejects_deceptive_directional_controls() {
    for unsafe_text in ["safe\u{202e}txt.exe", "report.p\u{200b}df"] {
        assert!(PermissionDetail::new("Path", unsafe_text).is_err());
        assert!(
            PermissionRequest::new(
                tool_call_id("tool-bidi"),
                "workspace_read_v1",
                "filesystem_read",
                format!("workspace:{unsafe_text}"),
                Vec::new(),
            )
            .is_err()
        );

        let wire = json!({
            "tool_call_id": "tool-bidi",
            "tool_name": "workspace_read_v1",
            "capability": "filesystem_read",
            "resource": format!("workspace:{unsafe_text}"),
            "details": []
        });
        assert!(serde_json::from_value::<PermissionRequest>(wire).is_err());
    }
}

#[test]
fn permission_wire_covers_exact_built_in_tool_boundaries() {
    let mut details = (0..256)
        .map(|index| {
            PermissionDetail::new("Argument", format!("{}: value", index + 1))
                .expect("bounded argument detail")
        })
        .collect::<Vec<_>>();
    details.push(PermissionDetail::new("Program", "cargo").expect("program detail"));
    details.push(PermissionDetail::new("Working directory", ".").expect("directory detail"));
    details[0] = PermissionDetail::new("Argument", "1: ".to_owned() + &"\\u{7f}".repeat(60 * 1024))
        .expect("escaped exact argument can exceed the ordinary display-detail bound");

    assert!(
        PermissionRequest::new(
            tool_call_id("tool-boundary"),
            "process_run",
            "process_execute",
            "program:cargo@workspace:.",
            details,
        )
        .is_ok()
    );
    assert!(
        PermissionDetail::new("Argument", "x".repeat(MAX_PERMISSION_DETAIL_BYTES + 1)).is_err()
    );

    let aggregate_too_large = vec![
        PermissionDetail::new("First", "x".repeat(MAX_PERMISSION_TOTAL_BYTES / 2))
            .expect("individually bounded field"),
        PermissionDetail::new("Second", "x".repeat(MAX_PERMISSION_TOTAL_BYTES / 2))
            .expect("individually bounded field"),
    ];
    assert!(
        PermissionRequest::new(
            tool_call_id("tool-aggregate"),
            "http_request",
            "http_request",
            "https://example.com",
            aggregate_too_large,
        )
        .is_err()
    );
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
