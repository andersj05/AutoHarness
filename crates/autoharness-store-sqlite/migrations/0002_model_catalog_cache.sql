CREATE TABLE model_catalog_cache (
    provider_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(CAST(provider_id AS BLOB)) BETWEEN 1 AND 512),
    schema_version INTEGER NOT NULL CHECK (schema_version BETWEEN 1 AND 65535),
    refreshed_at_ms INTEGER NOT NULL,
    catalog_json BLOB NOT NULL CHECK (length(catalog_json) BETWEEN 1 AND 16777216),
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32)
) STRICT;
