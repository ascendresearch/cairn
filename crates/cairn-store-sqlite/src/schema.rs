use rusqlite::Connection;
use thiserror::Error;

const CURRENT_VERSION: i64 = 2;

#[derive(Debug, Error)]
pub(crate) enum SchemaError {
    #[error("SQLite schema version {found} is newer than supported version {supported}")]
    TooNew { found: i64, supported: i64 },
    #[error("SQLite schema migration failed: {0}")]
    Sql(#[from] rusqlite::Error),
}

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), SchemaError> {
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let version = connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
    if version > CURRENT_VERSION {
        return Err(SchemaError::TooNew {
            found: version,
            supported: CURRENT_VERSION,
        });
    }
    if version < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS streams (
            aggregate_kind TEXT NOT NULL,
            aggregate_id   TEXT NOT NULL,
            revision       INTEGER NOT NULL CHECK (revision > 0),
            PRIMARY KEY (aggregate_kind, aggregate_id)
         ) STRICT;

         CREATE TABLE IF NOT EXISTS commands (
            command_id        TEXT PRIMARY KEY NOT NULL,
            aggregate_kind    TEXT NOT NULL,
            aggregate_id      TEXT NOT NULL,
            expected_kind     INTEGER NOT NULL CHECK (expected_kind IN (0, 1)),
            expected_revision INTEGER,
            first_sequence    INTEGER NOT NULL,
            last_sequence     INTEGER NOT NULL
         ) STRICT;

         CREATE TABLE IF NOT EXISTS events (
            event_id            TEXT PRIMARY KEY NOT NULL,
            aggregate_kind      TEXT NOT NULL,
            aggregate_id        TEXT NOT NULL,
            sequence            INTEGER NOT NULL CHECK (sequence > 0),
            schema_name         TEXT NOT NULL,
            schema_version      INTEGER NOT NULL CHECK (schema_version > 0),
            command_id          TEXT NOT NULL,
            parent_event_id     TEXT,
            observed_at_unix_ms INTEGER NOT NULL,
            payload             BLOB NOT NULL,
            UNIQUE (aggregate_kind, aggregate_id, sequence),
            FOREIGN KEY (command_id) REFERENCES commands(command_id)
         ) STRICT;

         CREATE INDEX IF NOT EXISTS events_by_command ON events(command_id, sequence);

         CREATE TABLE IF NOT EXISTS content_blobs (
            blob_digest TEXT PRIMARY KEY NOT NULL,
            byte_len    INTEGER NOT NULL CHECK (byte_len >= 0)
         ) STRICT;

         CREATE TABLE IF NOT EXISTS content_objects (
            content_id     TEXT PRIMARY KEY NOT NULL,
            content_domain TEXT NOT NULL,
            algorithm      TEXT NOT NULL,
            blob_digest    TEXT NOT NULL,
            byte_len       INTEGER NOT NULL CHECK (byte_len >= 0),
            FOREIGN KEY (blob_digest) REFERENCES content_blobs(blob_digest)
         ) STRICT;

         CREATE INDEX IF NOT EXISTS content_objects_by_blob ON content_objects(blob_digest);

             PRAGMA user_version = 1;",
        )?;
        transaction.commit()?;
    }

    // WAL lets the long-running controller continue reading while a separate administrative
    // command appends an authority fact. Journal mode is persistent and must be changed outside a
    // transaction; FULL synchronization preserves the durable-fact contract.
    if version < 2 {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA user_version = 2;",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{CURRENT_VERSION, SchemaError, migrate};

    #[test]
    fn migration_is_idempotent_and_versioned() {
        let mut connection = Connection::open_in_memory().expect("connection");
        migrate(&mut connection).expect("first migration");
        migrate(&mut connection).expect("second migration");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn v1_file_store_migrates_to_wal_for_concurrent_controller_and_admin_access() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("events.sqlite3");
        let mut connection = Connection::open(&path).expect("open V1 fixture");
        connection
            .execute_batch("PRAGMA user_version = 1;")
            .expect("mark V1 fixture");

        migrate(&mut connection).expect("migrate V1 fixture");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");
        assert_eq!(version, CURRENT_VERSION);
        assert_eq!(journal_mode, "wal");

        let reopened = Connection::open(path).expect("reopen migrated fixture");
        let persisted_mode: String = reopened
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("persisted journal mode");
        assert_eq!(persisted_mode, "wal");
    }

    #[test]
    fn newer_schema_is_rejected() {
        let mut connection = Connection::open_in_memory().expect("connection");
        connection
            .execute_batch("PRAGMA user_version = 99;")
            .expect("set version");
        assert!(matches!(
            migrate(&mut connection),
            Err(SchemaError::TooNew { .. })
        ));
    }
}
