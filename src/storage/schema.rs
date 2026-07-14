use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::error::ArcWrenError;

const INITIAL_VERSION: i64 = 1;
const INITIAL_NAME: &str = "initial schema";
const INITIAL_MIGRATION: &str = include_str!("../../migrations/0001_init.sql");

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), ArcWrenError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;

    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL,
                checksum TEXT NOT NULL
            );",
        )
        .map_err(storage_error)?;

    let highest_version = transaction
        .query_row("SELECT MAX(version) FROM migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(storage_error)?;
    if let Some(version) = highest_version
        && version > INITIAL_VERSION
    {
        return Err(ArcWrenError::Storage {
            detail: format!("unsupported database migration version {version}"),
        });
    }

    let applied = transaction
        .query_row(
            "SELECT name, checksum FROM migrations WHERE version = ?1",
            [INITIAL_VERSION],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let migration_count = transaction
        .query_row("SELECT COUNT(*) FROM migrations", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(storage_error)?;

    if let Some((name, checksum)) = applied {
        if migration_count != INITIAL_VERSION {
            return Err(ArcWrenError::Storage {
                detail: format!(
                    "inconsistent migration ledger: expected {INITIAL_VERSION} row, found {migration_count}"
                ),
            });
        }
        if name != INITIAL_NAME {
            return Err(ArcWrenError::Storage {
                detail: format!("migration 1 name mismatch: found {name:?}"),
            });
        }

        let expected_checksum = initial_checksum();
        match checksum {
            Some(checksum) if checksum == expected_checksum => {}
            Some(checksum) => {
                return Err(ArcWrenError::Storage {
                    detail: format!(
                        "migration 1 checksum mismatch: expected {expected_checksum}, found {checksum}"
                    ),
                });
            }
            None => {
                return Err(ArcWrenError::Storage {
                    detail: "migration 1 checksum is missing".to_owned(),
                });
            }
        }
    } else if migration_count != 0 {
        return Err(ArcWrenError::Storage {
            detail: format!(
                "inconsistent migration ledger: migration 1 is missing but {migration_count} other rows exist"
            ),
        });
    } else {
        let checksum = initial_checksum();
        transaction
            .execute_batch(INITIAL_MIGRATION)
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO migrations (version, name, applied_at, checksum)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    INITIAL_VERSION,
                    INITIAL_NAME,
                    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                    checksum,
                ],
            )
            .map_err(storage_error)?;
    }

    transaction.commit().map_err(storage_error)
}

fn initial_checksum() -> String {
    format!("{:x}", Sha256::digest(INITIAL_MIGRATION.as_bytes()))
}

fn storage_error(error: rusqlite::Error) -> ArcWrenError {
    ArcWrenError::Storage {
        detail: error.to_string(),
    }
}
