CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(session_id AS BLOB)) BETWEEN 1 AND 512),
    status TEXT NOT NULL CHECK (status IN ('active', 'archived')),
    selected_provider_id TEXT,
    selected_model_id TEXT,
    last_sequence INTEGER NOT NULL CHECK (last_sequence >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK (
        (selected_provider_id IS NULL AND selected_model_id IS NULL)
        OR
        (selected_provider_id IS NOT NULL AND selected_model_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE session_events (
    event_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(event_id AS BLOB)) BETWEEN 1 AND 512),
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version BETWEEN 1 AND 65535),
    occurred_at_ms INTEGER NOT NULL,
    caused_by_command_id TEXT,
    caused_by_event_id TEXT,
    correlation_id TEXT NOT NULL
        CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 512),
    event_kind TEXT NOT NULL,
    envelope_json BLOB NOT NULL CHECK (length(envelope_json) > 0),
    UNIQUE (session_id, sequence),
    UNIQUE (session_id, event_id),
    UNIQUE (caused_by_command_id),
    CHECK (
        (caused_by_command_id IS NOT NULL AND caused_by_event_id IS NULL)
        OR
        (caused_by_command_id IS NULL AND caused_by_event_id IS NOT NULL)
    ),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, caused_by_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX session_events_replay
    ON session_events(session_id, sequence);

CREATE TABLE admitted_inputs (
    session_id TEXT NOT NULL,
    input_id TEXT NOT NULL
        CHECK (length(CAST(input_id AS BLOB)) BETWEEN 1 AND 512),
    admitted_event_id TEXT NOT NULL,
    admitted_sequence INTEGER NOT NULL CHECK (admitted_sequence > 0),
    delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('next_turn')),
    state TEXT NOT NULL CHECK (state IN ('admitted', 'promoted')),
    prompt_utf8 BLOB NOT NULL,
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    admitted_at_ms INTEGER NOT NULL,
    promoted_at_ms INTEGER,
    PRIMARY KEY (session_id, input_id),
    UNIQUE (admitted_event_id),
    CHECK (
        (state = 'admitted' AND promoted_at_ms IS NULL)
        OR
        (state = 'promoted' AND promoted_at_ms IS NOT NULL)
    ),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, admitted_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX admitted_inputs_order
    ON admitted_inputs(session_id, admitted_sequence);

CREATE TABLE provider_attempts (
    attempt_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(attempt_id AS BLOB)) BETWEEN 1 AND 512),
    session_id TEXT NOT NULL,
    input_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    retry_of_attempt_id TEXT,
    state TEXT NOT NULL
        CHECK (state IN ('prepared', 'in_flight', 'completed', 'failed', 'cancelled', 'unknown')),
    prepared_event_id TEXT NOT NULL,
    prepared_sequence INTEGER NOT NULL CHECK (prepared_sequence > 0),
    started_event_id TEXT,
    settled_event_id TEXT,
    cancellation_requested_event_id TEXT,
    usage_event_id TEXT,
    prepared_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER,
    settled_at_ms INTEGER,
    cancellation_requested_at_ms INTEGER,
    usage_json BLOB,
    failure_json BLOB,
    UNIQUE (session_id, attempt_id),
    UNIQUE (prepared_event_id),
    UNIQUE (started_event_id),
    UNIQUE (settled_event_id),
    UNIQUE (cancellation_requested_event_id),
    UNIQUE (usage_event_id),
    CHECK (
        (state = 'prepared' AND started_at_ms IS NULL AND settled_at_ms IS NULL)
        OR
        (state = 'in_flight' AND started_at_ms IS NOT NULL AND settled_at_ms IS NULL)
        OR
        (state IN ('completed', 'cancelled', 'unknown')
            AND started_at_ms IS NOT NULL AND settled_at_ms IS NOT NULL)
        OR
        (state = 'failed' AND settled_at_ms IS NOT NULL)
    ),
    CHECK (
        (state = 'failed' AND failure_json IS NOT NULL)
        OR
        (state <> 'failed' AND failure_json IS NULL)
    ),
    CHECK (
        (cancellation_requested_event_id IS NULL AND cancellation_requested_at_ms IS NULL)
        OR
        (cancellation_requested_event_id IS NOT NULL AND cancellation_requested_at_ms IS NOT NULL)
    ),
    CHECK (
        (usage_event_id IS NULL AND usage_json IS NULL)
        OR
        (usage_event_id IS NOT NULL AND usage_json IS NOT NULL)
    ),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, input_id)
        REFERENCES admitted_inputs(session_id, input_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, retry_of_attempt_id)
        REFERENCES provider_attempts(session_id, attempt_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, prepared_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, started_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, settled_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, cancellation_requested_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, usage_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX provider_attempts_order
    ON provider_attempts(session_id, prepared_sequence);

CREATE INDEX provider_attempts_recovery
    ON provider_attempts(state, session_id, prepared_sequence);

CREATE TABLE transcript_messages (
    session_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('input', 'attempt')),
    source_id TEXT NOT NULL
        CHECK (length(CAST(source_id AS BLOB)) BETWEEN 1 AND 512),
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    state TEXT NOT NULL
        CHECK (state IN ('complete', 'streaming', 'failed', 'cancelled', 'unknown')),
    first_sequence INTEGER NOT NULL CHECK (first_sequence > 0),
    last_sequence INTEGER NOT NULL CHECK (last_sequence >= first_sequence),
    PRIMARY KEY (session_id, source_kind, source_id),
    UNIQUE (session_id, first_sequence),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE transcript_segments (
    session_id TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
    source_event_id TEXT NOT NULL,
    content_utf8 BLOB NOT NULL,
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    PRIMARY KEY (session_id, source_kind, source_id, event_sequence),
    UNIQUE (source_event_id),
    FOREIGN KEY (session_id, source_kind, source_id)
        REFERENCES transcript_messages(session_id, source_kind, source_id) ON DELETE RESTRICT,
    FOREIGN KEY (session_id, source_event_id)
        REFERENCES session_events(session_id, event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX transcript_messages_order
    ON transcript_messages(session_id, first_sequence);

CREATE INDEX transcript_segments_order
    ON transcript_segments(session_id, source_kind, source_id, event_sequence);
