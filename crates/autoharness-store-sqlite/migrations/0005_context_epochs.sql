CREATE TABLE context_epochs (
    epoch_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(epoch_id AS BLOB)) BETWEEN 1 AND 512),
    session_id TEXT NOT NULL,
    memory_generation INTEGER NOT NULL CHECK (memory_generation >= 0),
    reason TEXT NOT NULL
        CHECK (reason IN (
            'new_attempt', 'explicit_retry', 'compaction', 'source_incompatibility',
            'policy_change', 'recovery'
        )),
    predecessor_epoch_id TEXT,
    baseline_sha256 BLOB NOT NULL CHECK (length(baseline_sha256) = 32),
    builder_version INTEGER NOT NULL CHECK (builder_version BETWEEN 1 AND 65535),
    registry_version INTEGER NOT NULL CHECK (registry_version BETWEEN 1 AND 65535),
    ranker_version INTEGER NOT NULL CHECK (ranker_version BETWEEN 1 AND 65535),
    renderer_version INTEGER NOT NULL CHECK (renderer_version BETWEEN 1 AND 65535),
    sizer_version INTEGER NOT NULL CHECK (sizer_version BETWEEN 1 AND 65535),
    config_sha256 BLOB NOT NULL CHECK (length(config_sha256) = 32),
    catalog_sha256 BLOB NOT NULL CHECK (length(catalog_sha256) = 32),
    model_capability_sha256 BLOB NOT NULL CHECK (length(model_capability_sha256) = 32),
    tool_registry_sha256 BLOB NOT NULL CHECK (length(tool_registry_sha256) = 32),
    token_budget INTEGER NOT NULL CHECK (token_budget > 0),
    started_at_ms INTEGER NOT NULL,
    manifest_json BLOB NOT NULL CHECK (length(manifest_json) BETWEEN 1 AND 1048576),
    manifest_json_sha256 BLOB NOT NULL CHECK (length(manifest_json_sha256) = 32),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (predecessor_epoch_id) REFERENCES context_epochs(epoch_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX context_epochs_session
    ON context_epochs(session_id, started_at_ms, epoch_id);

CREATE TABLE context_compaction_boundaries (
    epoch_id TEXT PRIMARY KEY NOT NULL,
    predecessor_epoch_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    expected_session_sequence INTEGER NOT NULL CHECK (expected_session_sequence > 0),
    memory_generation INTEGER NOT NULL CHECK (memory_generation >= 0),
    facts_version INTEGER NOT NULL CHECK (facts_version BETWEEN 1 AND 65535),
    facts_sha256 BLOB NOT NULL CHECK (length(facts_sha256) = 32),
    memory_fact_count INTEGER NOT NULL CHECK (memory_fact_count >= 0),
    pending_session_fact_count INTEGER NOT NULL CHECK (pending_session_fact_count >= 0),
    summary_revision_id TEXT,
    verified_at_ms INTEGER NOT NULL,
    FOREIGN KEY (epoch_id) REFERENCES context_epochs(epoch_id) ON DELETE RESTRICT,
    FOREIGN KEY (predecessor_epoch_id) REFERENCES context_epochs(epoch_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (summary_revision_id)
        REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX context_compaction_boundaries_session
    ON context_compaction_boundaries(session_id, verified_at_ms, epoch_id);

CREATE TABLE context_turns (
    context_turn_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(context_turn_id AS BLOB)) BETWEEN 1 AND 512),
    session_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    run_turn INTEGER NOT NULL CHECK (run_turn > 0),
    epoch_id TEXT NOT NULL,
    expected_session_sequence INTEGER NOT NULL CHECK (expected_session_sequence > 0),
    memory_generation INTEGER NOT NULL CHECK (memory_generation >= 0),
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    request_sha256 BLOB NOT NULL CHECK (length(request_sha256) = 32),
    rendered_sha256 BLOB NOT NULL CHECK (length(rendered_sha256) = 32),
    manifest_sha256 BLOB NOT NULL CHECK (length(manifest_sha256) = 32),
    eligibility_user_id TEXT NOT NULL
        CHECK (length(CAST(eligibility_user_id AS BLOB)) BETWEEN 1 AND 512),
    eligibility_workspace_id TEXT NOT NULL
        CHECK (length(CAST(eligibility_workspace_id AS BLOB)) BETWEEN 1 AND 512),
    eligibility_agent_id TEXT,
    sensitivity_ceiling TEXT NOT NULL
        CHECK (sensitivity_ceiling IN ('public', 'internal', 'sensitive', 'secret')),
    token_budget INTEGER NOT NULL CHECK (token_budget > 0),
    reserved_tokens INTEGER NOT NULL CHECK (reserved_tokens >= 0),
    durable_memory_limit INTEGER NOT NULL CHECK (durable_memory_limit >= 0),
    rendered_token_count INTEGER NOT NULL CHECK (rendered_token_count >= 0),
    committed_at_ms INTEGER NOT NULL,
    rendered_state TEXT NOT NULL CHECK (rendered_state IN ('retained', 'absent', 'erased')),
    rendered_utf8 BLOB,
    rendered_content_sha256 BLOB,
    manifest_json BLOB NOT NULL CHECK (length(manifest_json) BETWEEN 1 AND 4194304),
    manifest_json_sha256 BLOB NOT NULL CHECK (length(manifest_json_sha256) = 32),
    CHECK (reserved_tokens <= token_budget),
    CHECK (durable_memory_limit <= token_budget - reserved_tokens),
    CHECK (rendered_token_count <= token_budget - reserved_tokens),
    CHECK (
        (rendered_state IN ('absent', 'erased')
            AND rendered_utf8 IS NULL AND rendered_content_sha256 IS NULL)
        OR
        (rendered_state = 'retained'
            AND length(rendered_utf8) BETWEEN 1 AND 262144
            AND length(rendered_content_sha256) = 32)
    ),
    UNIQUE (attempt_id, run_turn),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, attempt_id)
        REFERENCES provider_attempts(session_id, attempt_id) ON DELETE RESTRICT,
    FOREIGN KEY (epoch_id) REFERENCES context_epochs(epoch_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX context_turns_session
    ON context_turns(session_id, epoch_id, run_turn, context_turn_id);

CREATE TABLE context_turn_bindings (
    context_turn_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    run_turn INTEGER NOT NULL CHECK (run_turn > 0),
    bound_event_id TEXT NOT NULL,
    manifest_sha256 BLOB NOT NULL CHECK (length(manifest_sha256) = 32),
    bound_at_ms INTEGER NOT NULL,
    UNIQUE (attempt_id, run_turn),
    UNIQUE (session_id, bound_event_id),
    FOREIGN KEY (context_turn_id) REFERENCES context_turns(context_turn_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, attempt_id)
        REFERENCES provider_attempts(session_id, attempt_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, bound_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX context_turn_bindings_attempt
    ON context_turn_bindings(attempt_id, run_turn, context_turn_id);

CREATE TABLE context_turn_sources (
    context_turn_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    source_key TEXT NOT NULL
        CHECK (length(CAST(source_key AS BLOB)) BETWEEN 1 AND 512),
    observation_state TEXT NOT NULL
        CHECK (observation_state IN (
            'available', 'retained_stale', 'observed_absent', 'unavailable'
        )),
    source_revision_sha256 BLOB,
    value_sha256 BLOB,
    observed_at_ms INTEGER NOT NULL,
    snapshot_json BLOB NOT NULL CHECK (length(snapshot_json) BETWEEN 1 AND 65536),
    snapshot_json_sha256 BLOB NOT NULL CHECK (length(snapshot_json_sha256) = 32),
    PRIMARY KEY (context_turn_id, ordinal),
    UNIQUE (context_turn_id, source_key),
    CHECK (
        (observation_state IN ('available', 'retained_stale')
            AND length(source_revision_sha256) = 32
            AND length(value_sha256) = 32)
        OR
        (observation_state IN ('observed_absent', 'unavailable')
            AND source_revision_sha256 IS NULL
            AND value_sha256 IS NULL)
    ),
    FOREIGN KEY (context_turn_id) REFERENCES context_turns(context_turn_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE context_admissions (
    admission_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(admission_id AS BLOB)) BETWEEN 1 AND 512),
    context_turn_id TEXT NOT NULL,
    rank INTEGER NOT NULL CHECK (rank > 0),
    section TEXT NOT NULL
        CHECK (section IN (
            'safety_policy', 'current_instruction', 'authorized_instruction',
            'tool_contract', 'conversation_history', 'durable_memory'
        )),
    source_key TEXT NOT NULL
        CHECK (length(CAST(source_key AS BLOB)) BETWEEN 1 AND 512),
    source_revision_sha256 BLOB NOT NULL CHECK (length(source_revision_sha256) = 32),
    memory_revision_id TEXT,
    renderer_version INTEGER NOT NULL CHECK (renderer_version BETWEEN 1 AND 65535),
    rendered_sha256 BLOB NOT NULL CHECK (length(rendered_sha256) = 32),
    rank_score INTEGER NOT NULL,
    token_count INTEGER NOT NULL CHECK (token_count >= 0),
    admitted_at_ms INTEGER NOT NULL,
    rendered_state TEXT NOT NULL CHECK (rendered_state IN ('retained', 'erased')),
    rendered_utf8 BLOB,
    rendered_content_sha256 BLOB,
    admission_json BLOB NOT NULL CHECK (length(admission_json) BETWEEN 1 AND 1048576),
    admission_json_sha256 BLOB NOT NULL CHECK (length(admission_json_sha256) = 32),
    CHECK (
        (rendered_state = 'erased'
            AND rendered_utf8 IS NULL AND rendered_content_sha256 IS NULL)
        OR
        (rendered_state = 'retained'
            AND length(rendered_utf8) BETWEEN 1 AND 262144
            AND length(rendered_content_sha256) = 32)
    ),
    UNIQUE (context_turn_id, rank),
    FOREIGN KEY (context_turn_id) REFERENCES context_turns(context_turn_id) ON DELETE RESTRICT,
    FOREIGN KEY (memory_revision_id) REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX context_admissions_memory
    ON context_admissions(memory_revision_id, admitted_at_ms, admission_id);

CREATE INDEX context_admissions_source
    ON context_admissions(source_key, source_revision_sha256, admitted_at_ms, admission_id);

CREATE TABLE context_admission_reasons (
    admission_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    factor TEXT NOT NULL
        CHECK (factor IN (
            'pin', 'authority', 'exact_match', 'scope_specificity',
            'lexical_overlap', 'freshness', 'confidence', 'prior_utility', 'diversity',
            'budget_fit'
        )),
    contribution INTEGER NOT NULL,
    PRIMARY KEY (admission_id, ordinal),
    FOREIGN KEY (admission_id) REFERENCES context_admissions(admission_id) ON DELETE RESTRICT
) STRICT;
