use arcwren::error::ArcWrenError;
use arcwren::events::{ApprovalId, Event, EventId, SessionId, ToolCallId};
use arcwren::storage::{ApprovalStatus, MemoryState, Store};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("arcwren-storage-{}.sqlite", Uuid::new_v4()));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        for path in [
            self.path.clone(),
            PathBuf::from(format!("{}-wal", self.path.display())),
            PathBuf::from(format!("{}-shm", self.path.display())),
        ] {
            let _ = fs::remove_file(path);
        }
    }
}

#[test]
fn fresh_database_is_migrated_and_configured_for_durable_use() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let store = Store::open(database.path())?;

    assert_eq!(store.journal_mode()?, "wal");
    assert!(store.foreign_keys_enabled()?);
    assert!(store.busy_timeout_millis()? >= 5_000);

    let connection = Connection::open(database.path())?;
    let tables = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let required = BTreeSet::from([
        "approvals".to_owned(),
        "events".to_owned(),
        "memories".to_owned(),
        "messages".to_owned(),
        "migrations".to_owned(),
        "processed_telegram_updates".to_owned(),
        "sessions".to_owned(),
        "telegram_state".to_owned(),
        "usage_observations".to_owned(),
    ]);
    assert!(
        required.is_subset(&tables),
        "missing tables: {required:?} vs {tables:?}"
    );

    let migrations = connection.query_row("SELECT COUNT(*) FROM migrations", [], |row| {
        row.get::<_, u64>(0)
    })?;
    assert_eq!(migrations, 1);

    Ok(())
}

#[test]
fn sessions_can_be_created_and_listed_newest_first() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let store = Store::open(database.path())?;

    let first = store.create_session()?;
    let second = store.create_session()?;
    let sessions = store.list_sessions()?;

    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, second.id);
    assert_eq!(sessions[1].id, first.id);

    Ok(())
}

#[test]
fn appends_allocate_monotonic_per_session_sequences_and_read_in_order() -> Result<(), Box<dyn Error>>
{
    let database = TemporaryDatabase::new();
    let mut store = Store::open(database.path())?;
    let first_session = store.create_session()?;
    let second_session = store.create_session()?;

    let first = store.append(
        first_session.id,
        None,
        Event::UserInput { text: "one".into() },
    )?;
    let other = store.append(
        second_session.id,
        None,
        Event::UserInput {
            text: "other".into(),
        },
    )?;
    let second = store.append(
        first_session.id,
        None,
        Event::UserInput { text: "two".into() },
    )?;

    assert_eq!((first.sequence, second.sequence), (1, 2));
    assert_eq!(other.sequence, 1);
    assert_eq!(store.read_events(first_session.id)?, vec![first, second]);

    Ok(())
}

#[test]
fn approvals_persist_pending_and_resolved_state() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let store = Store::open(database.path())?;
    let session = store.create_session()?;
    let approval_id = ApprovalId::new();
    let tool_call_id = ToolCallId::new();

    let pending =
        store.create_approval(session.id, approval_id, tool_call_id, "Read project notes")?;
    assert_eq!(pending.status, ApprovalStatus::Pending);
    assert_eq!(store.get_approval(approval_id)?, Some(pending));

    let allowed = store.resolve_approval(approval_id, ApprovalStatus::Allowed)?;
    assert_eq!(allowed.status, ApprovalStatus::Allowed);
    assert!(allowed.resolved_at.is_some());
    assert_eq!(store.get_approval(approval_id)?, Some(allowed));

    Ok(())
}

#[test]
fn explicit_memories_are_retained_when_forgotten() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let store = Store::open(database.path())?;

    let active = store.remember_explicit("The owner prefers terse output", "user request")?;
    assert_eq!(active.state, MemoryState::Active);
    assert_eq!(store.list_active_memories()?, vec![active.clone()]);

    let forgotten = store.forget_memory(active.id)?;
    assert_eq!(forgotten.state, MemoryState::Forgotten);
    assert!(forgotten.forgotten_at.is_some());
    assert!(store.list_active_memories()?.is_empty());
    assert_eq!(store.get_memory(active.id)?, Some(forgotten));

    Ok(())
}

#[test]
fn failed_event_insert_rolls_back_the_allocated_sequence() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let mut store = Store::open(database.path())?;
    let session = store.create_session()?;
    let connection = Connection::open(database.path())?;
    connection.execute_batch(
        "CREATE TRIGGER reject_event_insert
         BEFORE INSERT ON events
         BEGIN
             SELECT RAISE(ABORT, 'injected append failure');
         END;",
    )?;

    let error = store
        .append(
            session.id,
            None,
            Event::UserInput {
                text: "rejected".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(error, ArcWrenError::Storage { .. }));
    connection.execute_batch("DROP TRIGGER reject_event_insert;")?;

    let accepted = store.append(
        session.id,
        None,
        Event::UserInput {
            text: "accepted".into(),
        },
    )?;
    assert_eq!(accepted.sequence, 1);
    assert_eq!(store.read_events(session.id)?, vec![accepted]);

    Ok(())
}

#[test]
fn reading_rejects_a_future_event_schema_as_a_typed_storage_error() -> Result<(), Box<dyn Error>> {
    let database = TemporaryDatabase::new();
    let store = Store::open(database.path())?;
    let session = store.create_session()?;
    inject_future_event(database.path(), session.id)?;

    let error = store.read_events(session.id).unwrap_err();
    assert!(matches!(
        error,
        ArcWrenError::Storage { ref detail }
            if detail.contains("unsupported event schema version 2")
    ));

    Ok(())
}

fn inject_future_event(path: &Path, session_id: SessionId) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(path)?;
    connection.execute(
        "INSERT INTO events (
            id, session_id, turn_id, sequence, timestamp, schema_version, event_json
         ) VALUES (?1, ?2, NULL, 1, ?3, 2, ?4)",
        params![
            EventId::new().to_string(),
            session_id.to_string(),
            "2026-07-13T12:00:00Z",
            r#"{"schema_version":2,"type":"user_input","text":"future"}"#,
        ],
    )?;
    Ok(())
}
