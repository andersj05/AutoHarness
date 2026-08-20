use autoharness_store::{CorruptionArea, StoreError};
use rusqlite::{Connection, ErrorCode, TransactionBehavior, params};
use sha2::{Digest, Sha256};

const LATEST_SCHEMA_VERSION: u32 = 1;
const INITIAL_MIGRATION_NAME: &str = "session_store";
const INITIAL_MIGRATION_SQL: &str = include_str!("../migrations/0001_session_store.sql");

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

    if applied.len() > usize::try_from(LATEST_SCHEMA_VERSION).unwrap_or(usize::MAX) {
        let found = applied
            .last()
            .and_then(|(version, _, _)| u32::try_from(*version).ok())
            .unwrap_or(u32::MAX);
        return Err(StoreError::NewerSchema {
            found,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    let expected_checksum = Sha256::digest(INITIAL_MIGRATION_SQL.as_bytes());
    match applied.as_slice() {
        [] => {
            if user_version != 0 {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::MigrationHistory,
                });
            }
            transaction
                .execute_batch(INITIAL_MIGRATION_SQL)
                .map_err(map_migration_error)?;
            transaction
                .execute(
                    "INSERT INTO schema_migrations \
                     (version, name, checksum, applied_at_ms) \
                     VALUES (1, ?1, ?2, unixepoch('now') * 1000)",
                    params![INITIAL_MIGRATION_NAME, expected_checksum.as_slice()],
                )
                .map_err(map_migration_error)?;
            transaction
                .pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)
                .map_err(map_migration_error)?;
        }
        [(version, name, checksum)] => {
            if *version != 1
                || name != INITIAL_MIGRATION_NAME
                || checksum.as_slice() != expected_checksum.as_slice()
                || user_version != 1
            {
                return Err(StoreError::CorruptData {
                    area: CorruptionArea::MigrationHistory,
                });
            }
        }
        _ => {
            return Err(StoreError::NewerSchema {
                found: applied
                    .last()
                    .and_then(|(version, _, _)| u32::try_from(*version).ok())
                    .unwrap_or(u32::MAX),
                supported: LATEST_SCHEMA_VERSION,
            });
        }
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
