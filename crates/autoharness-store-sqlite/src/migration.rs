use autoharness_store::{CorruptionArea, StoreError};
use rusqlite::{Connection, ErrorCode, TransactionBehavior, params};
use sha2::{Digest, Sha256};

const LATEST_SCHEMA_VERSION: u32 = 2;
const MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "session_store",
        include_str!("../migrations/0001_session_store.sql"),
    ),
    (
        2,
        "model_catalog_cache",
        include_str!("../migrations/0002_model_catalog_cache.sql"),
    ),
];

const CREATE_MIGRATION_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL CHECK (version > 0),
    name TEXT NOT NULL,
    checksum BLOB NOT NULL CHECK (length(checksum) = 32),
    applied_at_ms INTEGER NOT NULL
) STRICT;
"#;

pub(crate) fn apply(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_migration_error)?;
    transaction
        .execute_batch(CREATE_MIGRATION_TABLE_SQL)
        .map_err(map_migration_error)?;

    let user_version = transaction
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(map_migration_error)?;
    if user_version > i64::from(LATEST_SCHEMA_VERSION) {
        return Err(StoreError::NewerSchema {
            found: u32::try_from(user_version).unwrap_or(u32::MAX),
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    if user_version < 0 {
        return Err(StoreError::CorruptData {
            area: CorruptionArea::MigrationHistory,
        });
    }

    let applied = {
        let mut statement = transaction
            .prepare("SELECT version, name, checksum FROM schema_migrations ORDER BY version")
            .map_err(map_migration_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(map_migration_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_migration_error)?
    };

    if applied.len() > MIGRATIONS.len() {
        let found = applied
            .last()
            .and_then(|(version, _, _)| u32::try_from(*version).ok())
            .unwrap_or(u32::MAX);
        return Err(StoreError::NewerSchema {
            found,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    if user_version != i64::try_from(applied.len()).unwrap_or(i64::MAX) {
        return Err(StoreError::CorruptData {
            area: CorruptionArea::MigrationHistory,
        });
    }
    for ((version, name, checksum), (expected_version, expected_name, expected_sql)) in
        applied.iter().zip(MIGRATIONS)
    {
        let expected_checksum = Sha256::digest(expected_sql.as_bytes());
        if *version != i64::from(*expected_version)
            || name != expected_name
            || checksum.as_slice() != expected_checksum.as_slice()
        {
            return Err(StoreError::CorruptData {
                area: CorruptionArea::MigrationHistory,
            });
        }
    }

    for (version, name, sql) in MIGRATIONS.iter().skip(applied.len()) {
        transaction
            .execute_batch(sql)
            .map_err(map_migration_error)?;
        let checksum = Sha256::digest(sql.as_bytes());
        transaction
            .execute(
                "INSERT INTO schema_migrations \
                 (version, name, checksum, applied_at_ms) \
                 VALUES (?1, ?2, ?3, unixepoch('now') * 1000)",
                params![version, name, checksum.as_slice()],
            )
            .map_err(map_migration_error)?;
        transaction
            .pragma_update(None, "user_version", version)
            .map_err(map_migration_error)?;
    }

    transaction.commit().map_err(map_migration_error)
}

fn map_migration_error(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(details, _)
            if matches!(
                details.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            StoreError::Busy
        }
        _ => StoreError::Migration,
    }
}
