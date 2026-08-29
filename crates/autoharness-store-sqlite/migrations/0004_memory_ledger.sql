ALTER TABLE sessions ADD COLUMN workspace_id TEXT;

CREATE TABLE memory_store_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    generation INTEGER NOT NULL CHECK (generation >= 0),
    mutation_generation INTEGER NOT NULL CHECK (mutation_generation >= 0),
    updated_at_ms INTEGER NOT NULL
) STRICT;

INSERT INTO memory_store_state (singleton, generation, mutation_generation, updated_at_ms)
VALUES (1, 0, 0, 0);

CREATE TABLE memory_items (
    memory_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(memory_id AS BLOB)) BETWEEN 1 AND 512),
    scope_type TEXT NOT NULL
        CHECK (scope_type IN ('user', 'workspace', 'session', 'agent')),
    scope_id TEXT NOT NULL
        CHECK (length(CAST(scope_id AS BLOB)) BETWEEN 1 AND 512),
    kind TEXT NOT NULL
        CHECK (kind IN ('fact', 'preference', 'constraint', 'lesson', 'procedure')),
    lifecycle TEXT NOT NULL
        CHECK (lifecycle IN (
            'proposed', 'active', 'superseded', 'rejected', 'retracted', 'deleted'
        )),
    last_sequence INTEGER NOT NULL CHECK (last_sequence >= 0),
    latest_revision INTEGER NOT NULL CHECK (latest_revision >= 0),
    latest_revision_id TEXT,
    active_revision_id TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK (
        (lifecycle = 'active' AND active_revision_id IS NOT NULL)
        OR
        (lifecycle <> 'active' AND active_revision_id IS NULL)
    )
) STRICT;

CREATE TABLE memory_content_blobs (
    content_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(content_id AS BLOB)) BETWEEN 1 AND 512),
    media_type TEXT NOT NULL CHECK (media_type = 'text/plain; charset=utf-8'),
    content_utf8 BLOB NOT NULL CHECK (length(content_utf8) BETWEEN 1 AND 16384),
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE memory_operations (
    operation_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(operation_id AS BLOB)) BETWEEN 1 AND 512),
    memory_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    schema_version INTEGER NOT NULL CHECK (schema_version BETWEEN 1 AND 65535),
    operation_kind TEXT NOT NULL
        CHECK (operation_kind IN (
            'created', 'proposed', 'revised', 'validated', 'approved',
            'activated', 'superseded', 'rejected', 'retracted', 'deleted'
        )),
    revision_id TEXT,
    caused_by_command_id TEXT,
    caused_by_operation_id TEXT,
    correlation_id TEXT NOT NULL
        CHECK (length(CAST(correlation_id AS BLOB)) BETWEEN 1 AND 512),
    occurred_at_ms INTEGER NOT NULL,
    envelope_json BLOB NOT NULL CHECK (length(envelope_json) BETWEEN 1 AND 1048576),
    operation_sha256 BLOB NOT NULL CHECK (length(operation_sha256) = 32),
    UNIQUE (memory_id, sequence),
    UNIQUE (caused_by_command_id),
    CHECK (
        (caused_by_command_id IS NOT NULL AND caused_by_operation_id IS NULL)
        OR
        (caused_by_command_id IS NULL AND caused_by_operation_id IS NOT NULL)
    ),
    FOREIGN KEY (memory_id) REFERENCES memory_items(memory_id) ON DELETE RESTRICT,
    FOREIGN KEY (caused_by_operation_id)
        REFERENCES memory_operations(operation_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX memory_operations_replay
    ON memory_operations(memory_id, sequence);

CREATE TABLE memory_revisions (
    search_rowid INTEGER PRIMARY KEY NOT NULL,
    revision_id TEXT NOT NULL UNIQUE
        CHECK (length(CAST(revision_id AS BLOB)) BETWEEN 1 AND 512),
    memory_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    subject_key TEXT
        CHECK (subject_key IS NULL OR length(CAST(subject_key AS BLOB)) BETWEEN 1 AND 512),
    introduced_operation_id TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL
        CHECK (state IN (
            'proposed', 'active', 'superseded', 'rejected', 'retracted', 'deleted'
        )),
    content_id TEXT,
    content_hash_sha256 BLOB NOT NULL CHECK (length(content_hash_sha256) = 32),
    metadata_json BLOB NOT NULL CHECK (length(metadata_json) BETWEEN 1 AND 1048576),
    metadata_sha256 BLOB NOT NULL CHECK (length(metadata_sha256) = 32),
    origin TEXT NOT NULL
        CHECK (origin IN (
            'explicit_user', 'verified_tool', 'imported_document',
            'model_proposal', 'compaction'
        )),
    trust_class TEXT NOT NULL
        CHECK (trust_class IN (
            'user_approved', 'verified_observation', 'imported', 'untrusted_proposal'
        )),
    confidence_basis_points INTEGER NOT NULL
        CHECK (confidence_basis_points BETWEEN 0 AND 10000),
    sensitivity TEXT NOT NULL
        CHECK (sensitivity IN ('public', 'internal', 'sensitive', 'secret')),
    valid_from_ms INTEGER,
    valid_until_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    supersedes_revision_id TEXT,
    CHECK (valid_until_ms IS NULL OR valid_from_ms IS NULL OR valid_until_ms > valid_from_ms),
    CHECK ((state = 'deleted' AND content_id IS NULL) OR state <> 'deleted'),
    UNIQUE (memory_id, revision),
    FOREIGN KEY (memory_id) REFERENCES memory_items(memory_id) ON DELETE RESTRICT,
    FOREIGN KEY (introduced_operation_id)
        REFERENCES memory_operations(operation_id) ON DELETE RESTRICT,
    FOREIGN KEY (content_id) REFERENCES memory_content_blobs(content_id) ON DELETE SET NULL,
    FOREIGN KEY (supersedes_revision_id) REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX memory_items_scope
    ON memory_items(scope_type, scope_id, kind, lifecycle, memory_id);

CREATE INDEX memory_revisions_item
    ON memory_revisions(memory_id, revision);

CREATE INDEX memory_revisions_validity
    ON memory_revisions(state, valid_from_ms, valid_until_ms, memory_id);

CREATE INDEX memory_revisions_subject
    ON memory_revisions(subject_key, content_hash_sha256, state, memory_id);

CREATE TABLE memory_evidence (
    revision_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id TEXT NOT NULL UNIQUE
        CHECK (length(CAST(evidence_id AS BLOB)) BETWEEN 1 AND 512),
    source_json BLOB NOT NULL CHECK (length(source_json) BETWEEN 1 AND 16384),
    relation TEXT NOT NULL
        CHECK (relation IN ('supports', 'contradicts', 'derived_from')),
    excerpt_content_id TEXT,
    excerpt_sha256 BLOB,
    erased_by_session_id TEXT,
    erased_at_ms INTEGER,
    PRIMARY KEY (revision_id, ordinal),
    CHECK (
        (excerpt_content_id IS NULL AND excerpt_sha256 IS NULL)
        OR
        (excerpt_content_id IS NOT NULL AND length(excerpt_sha256) = 32)
    ),
    CHECK (
        (erased_by_session_id IS NULL AND erased_at_ms IS NULL)
        OR
        (erased_by_session_id IS NOT NULL AND erased_at_ms IS NOT NULL
            AND excerpt_content_id IS NULL AND excerpt_sha256 IS NULL)
    ),
    FOREIGN KEY (revision_id) REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT,
    FOREIGN KEY (excerpt_content_id) REFERENCES memory_content_blobs(content_id) ON DELETE SET NULL
) STRICT;

CREATE INDEX memory_evidence_erased_session
    ON memory_evidence(erased_by_session_id, revision_id)
    WHERE erased_by_session_id IS NOT NULL;

CREATE TABLE memory_session_erasure_tombstones (
    memory_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL
        CHECK (length(CAST(session_id AS BLOB)) BETWEEN 1 AND 512),
    session_last_sequence INTEGER NOT NULL CHECK (session_last_sequence > 0),
    erased_at_ms INTEGER NOT NULL,
    FOREIGN KEY (memory_id) REFERENCES memory_items(memory_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX memory_session_erasure_tombstones_session
    ON memory_session_erasure_tombstones(session_id, memory_id);

CREATE TABLE memory_relations (
    revision_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    to_memory_id TEXT NOT NULL
        CHECK (length(CAST(to_memory_id AS BLOB)) BETWEEN 1 AND 512),
    relation TEXT NOT NULL
        CHECK (relation IN ('duplicate_of', 'contradicts', 'refines', 'supersedes', 'related')),
    PRIMARY KEY (revision_id, ordinal),
    FOREIGN KEY (revision_id) REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX memory_relations_target
    ON memory_relations(to_memory_id, relation, revision_id);

CREATE TABLE memory_validations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    revision_id TEXT NOT NULL,
    validator_version INTEGER NOT NULL CHECK (validator_version BETWEEN 1 AND 65535),
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'needs_review', 'rejected')),
    validation_json BLOB NOT NULL CHECK (length(validation_json) BETWEEN 1 AND 65536),
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES memory_operations(operation_id) ON DELETE RESTRICT,
    FOREIGN KEY (revision_id) REFERENCES memory_revisions(revision_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX memory_validations_revision
    ON memory_validations(revision_id, created_at_ms, operation_id);
