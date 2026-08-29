use autoharness_domain::{
    AgentId, AttemptId, CommandId, ConfidenceBasisPoints, ContextAdmission, ContextAdmissionFactor,
    ContextAdmissionId, ContextAdmissionReason, ContextBudgetAllocation, ContextEligibility,
    ContextEpochHashes, ContextEpochId, ContextEpochManifest, ContextEpochReason,
    ContextEpochVersions, ContextObservationState, ContextSection, ContextSourceKey,
    ContextSourceSnapshot, ContextTokenBudget, ContextTurnId, ContextTurnManifest, CorrelationId,
    EstimatedTokens, InputId, MemoryCausation, MemoryCommandEnvelope, MemoryCommandPayload,
    MemoryContent, MemoryEvidence, MemoryEvidenceExcerpt, MemoryEvidenceId, MemoryEvidenceRelation,
    MemoryEvidenceSource, MemoryGeneration, MemoryId, MemoryKind, MemoryOperationEnvelope,
    MemoryOperationId, MemoryOperationPayload, MemoryOrigin, MemoryRelation, MemoryRelationKind,
    MemoryRevisionDraft, MemoryRevisionId, MemoryRevisionMetadata, MemoryRevisionNumber,
    MemoryRevisionStatus, MemoryScope, MemorySequence, MemoryValidity, ModelId, ModelRef,
    ProviderId, Sensitivity, SessionId, SessionSequence, Sha256Digest, TimestampMillis, TrustClass,
    UserId, WorkspaceId,
};

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::new(character.to_string().repeat(64)).expect("valid digest")
}

fn session_id() -> SessionId {
    SessionId::new("session-1").expect("valid session ID")
}

fn revision_draft() -> MemoryRevisionDraft {
    let evidence = MemoryEvidence::new(
        MemoryEvidenceId::new("evidence-1").expect("valid evidence ID"),
        MemoryEvidenceSource::UserInput {
            session_id: session_id(),
            input_id: InputId::new("input-1").expect("valid input ID"),
        },
        MemoryEvidenceRelation::Supports,
        Some(MemoryEvidenceExcerpt::new("compact explanations").expect("valid excerpt")),
        Some(digest('b')),
    )
    .expect("valid evidence");

    MemoryRevisionDraft::new(
        MemoryRevisionId::new("revision-1").expect("valid revision ID"),
        MemoryRevisionNumber::FIRST,
        MemoryContent::new("Use compact terminal explanations.").expect("valid content"),
        digest('a'),
        MemoryOrigin::ExplicitUser,
        TrustClass::UserApproved,
        ConfidenceBasisPoints::new(10_000).expect("valid confidence"),
        Sensitivity::Internal,
        MemoryValidity::Indefinite,
        vec![evidence],
        vec![MemoryRelation::new(
            MemoryId::new("memory-related").expect("valid memory ID"),
            MemoryRelationKind::Related,
        )],
    )
    .expect("valid revision draft")
}

#[test]
fn memory_command_and_contentless_operation_shapes_are_stable() {
    let draft = revision_draft();
    let command = MemoryCommandEnvelope::new_v1(
        CommandId::new("command-memory-1").expect("valid command ID"),
        MemoryId::new("memory-1").expect("valid memory ID"),
        None,
        CorrelationId::new("correlation-1").expect("valid correlation ID"),
        MemoryCommandPayload::CreateMemory {
            scope: MemoryScope::User(UserId::new("user-1").expect("valid user ID")),
            memory_kind: MemoryKind::Preference,
            revision: draft.clone(),
        },
    )
    .expect("valid create command");
    let metadata = MemoryRevisionMetadata::from_draft(
        MemoryRevisionStatus::Active,
        &draft,
        TimestampMillis::new(1_000),
        None,
    );
    let operation = MemoryOperationEnvelope::new_v1(
        MemoryOperationId::new("operation-1").expect("valid operation ID"),
        MemoryId::new("memory-1").expect("valid memory ID"),
        MemorySequence::FIRST,
        TimestampMillis::new(1_000),
        MemoryCausation::Command(CommandId::new("command-memory-1").expect("valid command ID")),
        CorrelationId::new("correlation-1").expect("valid correlation ID"),
        MemoryOperationPayload::MemoryCreated {
            scope: MemoryScope::User(UserId::new("user-1").expect("valid user ID")),
            memory_kind: MemoryKind::Preference,
            revision: metadata,
        },
    );

    assert_eq!(
        serde_json::to_value(&command).expect("serialize memory command"),
        serde_json::json!({
            "schema_version": 1,
            "command_id": "command-memory-1",
            "memory_id": "memory-1",
            "expected_sequence": null,
            "correlation_id": "correlation-1",
            "payload": {
                "kind": "create_memory",
                "payload": {
                    "scope": { "kind": "user", "id": "user-1" },
                    "memory_kind": "preference",
                    "revision": {
                        "revision_id": "revision-1",
                        "revision": 1,
                        "content": "Use compact terminal explanations.",
                        "content_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "origin": "explicit_user",
                        "trust_class": "user_approved",
                        "confidence": 10000,
                        "sensitivity": "internal",
                        "validity": { "kind": "indefinite" },
                        "evidence": [{
                            "evidence_id": "evidence-1",
                            "source": {
                                "kind": "user_input",
                                "payload": {
                                    "session_id": "session-1",
                                    "input_id": "input-1"
                                }
                            },
                            "relation": "supports",
                            "excerpt": "compact explanations",
                            "excerpt_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        }],
                        "relations": [{
                            "memory_id": "memory-related",
                            "kind": "related"
                        }]
                    }
                }
            }
        })
    );
    assert_eq!(
        serde_json::to_value(&operation).expect("serialize memory operation"),
        serde_json::json!({
            "schema_version": 1,
            "operation_id": "operation-1",
            "memory_id": "memory-1",
            "sequence": 1,
            "occurred_at": 1000,
            "causation": { "kind": "command", "id": "command-memory-1" },
            "correlation_id": "correlation-1",
            "payload": {
                "kind": "memory_created",
                "payload": {
                    "scope": { "kind": "user", "id": "user-1" },
                    "memory_kind": "preference",
                    "revision": {
                        "status": "active",
                        "revision_id": "revision-1",
                        "revision": 1,
                        "content_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "origin": "explicit_user",
                        "trust_class": "user_approved",
                        "confidence": 10000,
                        "sensitivity": "internal",
                        "validity": { "kind": "indefinite" },
                        "evidence": [{
                            "evidence_id": "evidence-1",
                            "source": {
                                "kind": "user_input",
                                "payload": {
                                    "session_id": "session-1",
                                    "input_id": "input-1"
                                }
                            },
                            "relation": "supports",
                            "excerpt_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        }],
                        "relations": [{
                            "memory_id": "memory-related",
                            "kind": "related"
                        }],
                        "created_at": 1000
                    }
                }
            }
        })
    );

    let operation_json = serde_json::to_string(&operation).expect("serialize operation text");
    assert!(!operation_json.contains("Use compact terminal explanations."));
    assert!(!operation_json.contains("compact explanations"));
    assert!(!format!("{command:?}").contains("Use compact terminal explanations."));
}

#[test]
fn memory_scope_and_lifecycle_enum_shapes_are_stable() {
    assert_eq!(
        serde_json::to_value([
            MemoryScope::User(UserId::new("user-1").expect("valid user ID")),
            MemoryScope::Workspace(WorkspaceId::new("workspace-1").expect("valid workspace ID"),),
            MemoryScope::Session(session_id()),
            MemoryScope::Agent(AgentId::new("agent-1").expect("valid agent ID")),
        ])
        .expect("serialize scopes"),
        serde_json::json!([
            { "kind": "user", "id": "user-1" },
            { "kind": "workspace", "id": "workspace-1" },
            { "kind": "session", "id": "session-1" },
            { "kind": "agent", "id": "agent-1" }
        ])
    );
    assert_eq!(
        serde_json::to_value([
            MemoryRevisionStatus::Proposed,
            MemoryRevisionStatus::Active,
            MemoryRevisionStatus::Superseded,
            MemoryRevisionStatus::Rejected,
            MemoryRevisionStatus::Retracted,
            MemoryRevisionStatus::Deleted,
        ])
        .expect("serialize statuses"),
        serde_json::json!([
            "proposed",
            "active",
            "superseded",
            "rejected",
            "retracted",
            "deleted"
        ])
    );
}

#[test]
fn context_epoch_and_turn_manifest_shapes_are_stable() {
    let epoch = ContextEpochManifest::new(
        ContextEpochId::new("epoch-1").expect("valid epoch ID"),
        session_id(),
        MemoryGeneration::new(7).expect("valid memory generation"),
        ContextEpochReason::NewAttempt,
        None,
        digest('a'),
        ContextEpochVersions::new(1, 1, 1, 1, 1).expect("valid versions"),
        ContextEpochHashes::new(digest('b'), digest('c'), digest('d'), digest('e')),
        ContextTokenBudget::new(4_096).expect("valid budget"),
        TimestampMillis::new(2_000),
    )
    .expect("valid epoch");
    let context_turn_id = ContextTurnId::new("turn-1").expect("valid context turn ID");
    let source = ContextSourceSnapshot::new(
        ContextSourceKey::new("memory:memory-1").expect("valid source key"),
        ContextObservationState::Available,
        Some(digest('f')),
        Some(digest('a')),
        TimestampMillis::new(2_001),
    )
    .expect("valid source snapshot");
    let admission = ContextAdmission::new(
        ContextAdmissionId::new("admission-1").expect("valid admission ID"),
        context_turn_id.clone(),
        ContextSection::DurableMemory,
        ContextSourceKey::new("memory:memory-1").expect("valid source key"),
        digest('f'),
        Some(MemoryRevisionId::new("revision-1").expect("valid revision ID")),
        1,
        digest('9'),
        1,
        325,
        EstimatedTokens::new(12).expect("valid token estimate"),
        TimestampMillis::new(2_002),
        vec![
            ContextAdmissionReason::new(1, ContextAdmissionFactor::Authority, 300)
                .expect("valid admission reason"),
            ContextAdmissionReason::new(2, ContextAdmissionFactor::Confidence, 25)
                .expect("valid admission reason"),
        ],
    )
    .expect("valid admission");
    let turn = ContextTurnManifest::new(
        context_turn_id,
        ContextEpochId::new("epoch-1").expect("valid epoch ID"),
        session_id(),
        AttemptId::new("attempt-1").expect("valid attempt ID"),
        1,
        SessionSequence::new(11).expect("valid session sequence"),
        MemoryGeneration::new(7).expect("valid memory generation"),
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("valid provider ID"),
            ModelId::new("models/gemini-test").expect("valid model ID"),
        ),
        digest('1'),
        digest('2'),
        digest('3'),
        ContextEligibility::new(
            UserId::new("user-1").expect("valid user ID"),
            WorkspaceId::new("workspace-1").expect("valid workspace ID"),
            session_id(),
            Some(AgentId::new("agent-1").expect("valid agent ID")),
            Sensitivity::Internal,
        ),
        ContextBudgetAllocation::new(
            ContextTokenBudget::new(4_096).expect("valid budget"),
            EstimatedTokens::new(512).expect("valid reservation"),
            EstimatedTokens::new(2_048).expect("valid memory limit"),
        )
        .expect("valid allocation"),
        EstimatedTokens::new(12).expect("valid token estimate"),
        TimestampMillis::new(2_003),
        vec![source],
        vec![admission],
    )
    .expect("valid turn manifest");

    assert_eq!(
        serde_json::to_value(epoch).expect("serialize epoch"),
        serde_json::json!({
            "epoch_id": "epoch-1",
            "session_id": "session-1",
            "memory_generation": 7,
            "reason": "new_attempt",
            "baseline_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "versions": {
                "builder_version": 1,
                "registry_version": 1,
                "ranker_version": 1,
                "renderer_version": 1,
                "sizer_version": 1
            },
            "hashes": {
                "config_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "catalog_hash": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "model_capability_hash": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                "tool_registry_hash": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
            },
            "token_budget": 4096,
            "started_at": 2000
        })
    );
    assert_eq!(
        serde_json::to_value(turn).expect("serialize turn manifest"),
        serde_json::json!({
            "context_turn_id": "turn-1",
            "epoch_id": "epoch-1",
            "session_id": "session-1",
            "attempt_id": "attempt-1",
            "run_turn": 1,
            "expected_session_sequence": 11,
            "memory_generation": 7,
            "model": {
                "provider_id": "google-ai-studio",
                "model_id": "models/gemini-test"
            },
            "request_hash": "1111111111111111111111111111111111111111111111111111111111111111",
            "rendered_hash": "2222222222222222222222222222222222222222222222222222222222222222",
            "manifest_hash": "3333333333333333333333333333333333333333333333333333333333333333",
            "eligibility": {
                "user_id": "user-1",
                "workspace_id": "workspace-1",
                "session_id": "session-1",
                "agent_id": "agent-1",
                "sensitivity_ceiling": "internal"
            },
            "budget": {
                "token_budget": 4096,
                "reserved_tokens": 512,
                "durable_memory_limit": 2048
            },
            "rendered_token_count": 12,
            "committed_at": 2003,
            "sources": [{
                "source_key": "memory:memory-1",
                "observation_state": "available",
                "source_revision": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "value_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "observed_at": 2001
            }],
            "admissions": [{
                "admission_id": "admission-1",
                "context_turn_id": "turn-1",
                "section": "durable_memory",
                "source_key": "memory:memory-1",
                "source_revision": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "memory_revision_id": "revision-1",
                "renderer_version": 1,
                "rendered_hash": "9999999999999999999999999999999999999999999999999999999999999999",
                "rank": 1,
                "rank_score": 325,
                "token_count": 12,
                "admitted_at": 2002,
                "reasons": [
                    {
                        "ordinal": 1,
                        "factor": "authority",
                        "contribution": 300
                    },
                    {
                        "ordinal": 2,
                        "factor": "confidence",
                        "contribution": 25
                    }
                ]
            }]
        })
    );
}
