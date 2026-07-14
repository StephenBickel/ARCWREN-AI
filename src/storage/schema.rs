use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

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
                applied_at TEXT NOT NULL
            );",
        )
        .map_err(storage_error)?;

    let applied = transaction
        .query_row(
            "SELECT version FROM migrations WHERE version = ?1",
            [INITIAL_VERSION],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?;

    if applied.is_none() {
        transaction
            .execute_batch(INITIAL_MIGRATION)
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![
                    INITIAL_VERSION,
                    INITIAL_NAME,
                    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .map_err(storage_error)?;
    }

    transaction.commit().map_err(storage_error)
}

fn storage_error(error: rusqlite::Error) -> ArcWrenError {
    ArcWrenError::Storage {
        detail: error.to_string(),
    }
}
