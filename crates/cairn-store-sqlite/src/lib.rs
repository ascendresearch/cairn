//! `SQLite` reference adapters for Cairn storage ports.

mod content;
mod schema;

pub use content::SqliteContentStore;

use std::{path::Path, str::FromStr};

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, EventId, EventSequence, SchemaName, SchemaVersion,
    StreamRevision,
};
use cairn_record::{
    AppendOutcome, EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent,
    StreamId, derive_event_id,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

/// `SQLite` implementation of the append-only [`EventStore`] contract.
pub struct SqliteEventStore {
    connection: Connection,
}

impl SqliteEventStore {
    /// Opens or creates a store at `path` and applies the V1 schema.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the database cannot be opened, configured, or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventStoreError> {
        Self::from_connection(Connection::open(path).map_err(storage_error)?)
    }

    /// Creates an in-memory store, primarily for contract tests and embedding.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the in-memory database cannot be configured or migrated.
    pub fn in_memory() -> Result<Self, EventStoreError> {
        Self::from_connection(Connection::open_in_memory().map_err(storage_error)?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, EventStoreError> {
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_error)?;
        schema::migrate(&mut connection).map_err(protocol_error)?;
        Ok(Self { connection })
    }
}

impl EventStore for SqliteEventStore {
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedRevision,
        command_id: &CommandId,
        events: &[NewEvent],
    ) -> Result<AppendOutcome, EventStoreError> {
        if events.is_empty() {
            return Err(EventStoreError::EmptyBatch);
        }

        // Acquire the single SQLite writer slot before reading the expected revision. A deferred
        // transaction can deadlock during read-to-write upgrade when an administrative process
        // appends authority facts while the controller is active; IMMEDIATE makes busy_timeout
        // serialize those writers instead.
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Some(outcome) = replay_outcome(&transaction, stream, expected, command_id, events)? {
            transaction.commit().map_err(storage_error)?;
            return Ok(outcome);
        }
        let outcome = append_new(&transaction, stream, expected, command_id, events)?;
        transaction.commit().map_err(storage_error)?;
        Ok(outcome)
    }

    fn read_stream(
        &self,
        stream: &StreamId,
        after_sequence: Option<EventSequence>,
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        read_events(
            &self.connection,
            "WHERE aggregate_kind = ?1 AND aggregate_id = ?2 AND sequence > ?3
             ORDER BY sequence ASC",
            params![
                stream.kind.as_str(),
                stream.id.as_str(),
                to_i64(
                    after_sequence.map_or(0, EventSequence::get),
                    "after_sequence"
                )?
            ],
        )
    }
}

fn append_new(
    transaction: &Transaction<'_>,
    stream: &StreamId,
    expected: ExpectedRevision,
    command_id: &CommandId,
    events: &[NewEvent],
) -> Result<AppendOutcome, EventStoreError> {
    let current = current_revision(transaction, stream)?;
    let matches = match expected {
        ExpectedRevision::NoStream => current.is_none(),
        ExpectedRevision::Exact(value) => current == Some(value),
    };
    if !matches {
        return Err(EventStoreError::RevisionConflict { expected, current });
    }

    let first_value = current
        .map_or(0, StreamRevision::get)
        .checked_add(1)
        .ok_or(EventStoreError::IntegerRange { field: "sequence" })?;
    let event_count =
        u64::try_from(events.len() - 1).map_err(|_| EventStoreError::IntegerRange {
            field: "event batch length",
        })?;
    let last_value = first_value
        .checked_add(event_count)
        .ok_or(EventStoreError::IntegerRange { field: "sequence" })?;
    let first_sequence = EventSequence::new(first_value).map_err(protocol_error)?;
    let last_sequence = EventSequence::new(last_value).map_err(protocol_error)?;

    insert_command(
        transaction,
        stream,
        expected,
        command_id,
        first_sequence,
        last_sequence,
    )?;
    let event_ids = insert_events(transaction, stream, command_id, first_sequence, events)?;
    update_stream(transaction, stream, current, last_sequence)?;

    Ok(AppendOutcome {
        first_sequence,
        last_sequence,
        event_ids,
        was_replay: false,
    })
}

fn insert_command(
    transaction: &Transaction<'_>,
    stream: &StreamId,
    expected: ExpectedRevision,
    command_id: &CommandId,
    first_sequence: EventSequence,
    last_sequence: EventSequence,
) -> Result<(), EventStoreError> {
    let (expected_kind, expected_revision) = encode_expected(expected)?;
    let command_wire = command_id.to_string();
    transaction
        .execute(
            "INSERT INTO commands (
                command_id, aggregate_kind, aggregate_id, expected_kind, expected_revision,
                first_sequence, last_sequence
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                command_wire,
                stream.kind.as_str(),
                stream.id.as_str(),
                expected_kind,
                expected_revision,
                to_i64(first_sequence.get(), "first_sequence")?,
                to_i64(last_sequence.get(), "last_sequence")?,
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn insert_events(
    transaction: &Transaction<'_>,
    stream: &StreamId,
    command_id: &CommandId,
    first_sequence: EventSequence,
    events: &[NewEvent],
) -> Result<Vec<EventId>, EventStoreError> {
    let command_wire = command_id.to_string();
    let mut event_ids = Vec::with_capacity(events.len());
    for (offset, event) in events.iter().enumerate() {
        let offset = u64::try_from(offset).map_err(|_| EventStoreError::IntegerRange {
            field: "event offset",
        })?;
        let sequence = first_sequence
            .get()
            .checked_add(offset)
            .ok_or(EventStoreError::IntegerRange { field: "sequence" })?;
        let sequence = EventSequence::new(sequence).map_err(protocol_error)?;
        let event_id = derive_event_id(stream, sequence, command_id, event)?;
        let event_wire = event_id.to_wire();
        let parent_wire = event.parent_event_id.map(EventId::to_wire);
        transaction
            .execute(
                "INSERT INTO events (
                    event_id, aggregate_kind, aggregate_id, sequence, schema_name,
                    schema_version, command_id, parent_event_id, observed_at_unix_ms, payload
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    event_wire,
                    stream.kind.as_str(),
                    stream.id.as_str(),
                    to_i64(sequence.get(), "sequence")?,
                    event.schema_name.as_str(),
                    i64::from(event.schema_version.get()),
                    command_wire,
                    parent_wire,
                    event.observed_at_unix_ms,
                    event.payload,
                ],
            )
            .map_err(storage_error)?;
        event_ids.push(event_id);
    }
    Ok(event_ids)
}

fn update_stream(
    transaction: &Transaction<'_>,
    stream: &StreamId,
    current: Option<StreamRevision>,
    last_sequence: EventSequence,
) -> Result<(), EventStoreError> {
    let Some(revision) = current else {
        transaction
            .execute(
                "INSERT INTO streams (aggregate_kind, aggregate_id, revision) VALUES (?1, ?2, ?3)",
                params![
                    stream.kind.as_str(),
                    stream.id.as_str(),
                    to_i64(last_sequence.get(), "revision")?
                ],
            )
            .map_err(storage_error)?;
        return Ok(());
    };

    let changed = transaction
        .execute(
            "UPDATE streams SET revision = ?1
             WHERE aggregate_kind = ?2 AND aggregate_id = ?3 AND revision = ?4",
            params![
                to_i64(last_sequence.get(), "revision")?,
                stream.kind.as_str(),
                stream.id.as_str(),
                to_i64(revision.get(), "revision")?
            ],
        )
        .map_err(storage_error)?;
    if changed != 1 {
        return Err(EventStoreError::Storage {
            message: "stream revision changed inside append transaction".to_owned(),
        });
    }
    Ok(())
}

fn current_revision(
    transaction: &Transaction<'_>,
    stream: &StreamId,
) -> Result<Option<StreamRevision>, EventStoreError> {
    let value = transaction
        .query_row(
            "SELECT revision FROM streams WHERE aggregate_kind = ?1 AND aggregate_id = ?2",
            params![stream.kind.as_str(), stream.id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?;
    value
        .map(|revision| StreamRevision::new(to_u64(revision, "revision")?).map_err(protocol_error))
        .transpose()
}

fn replay_outcome(
    transaction: &Transaction<'_>,
    stream: &StreamId,
    expected: ExpectedRevision,
    command_id: &CommandId,
    proposed: &[NewEvent],
) -> Result<Option<AppendOutcome>, EventStoreError> {
    let command = transaction
        .query_row(
            "SELECT aggregate_kind, aggregate_id, expected_kind, expected_revision,
                    first_sequence, last_sequence
             FROM commands WHERE command_id = ?1",
            [command_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;

    let Some((kind, id, expected_kind, expected_revision, first, last)) = command else {
        return Ok(None);
    };

    let stored_expected = decode_expected(expected_kind, expected_revision)?;
    let stored_events = read_events(
        transaction,
        "WHERE command_id = ?1 ORDER BY sequence ASC",
        [command_id.to_string()],
    )?;
    let same_events = stored_events.len() == proposed.len()
        && stored_events.iter().zip(proposed).all(|(stored, new)| {
            stored.schema_name == new.schema_name
                && stored.schema_version == new.schema_version
                && stored.parent_event_id == new.parent_event_id
                && stored.observed_at_unix_ms == new.observed_at_unix_ms
                && stored.payload == new.payload
        });
    if kind != stream.kind.as_str()
        || id != stream.id.as_str()
        || stored_expected != expected
        || !same_events
    {
        return Err(EventStoreError::CommandConflict {
            command_id: *command_id,
        });
    }

    Ok(Some(AppendOutcome {
        first_sequence: EventSequence::new(to_u64(first, "first_sequence")?)
            .map_err(protocol_error)?,
        last_sequence: EventSequence::new(to_u64(last, "last_sequence")?)
            .map_err(protocol_error)?,
        event_ids: stored_events
            .into_iter()
            .map(|event| event.event_id)
            .collect(),
        was_replay: true,
    }))
}

fn read_events<P: rusqlite::Params>(
    connection: &Connection,
    suffix: &str,
    parameters: P,
) -> Result<Vec<EventEnvelope>, EventStoreError> {
    let sql = format!(
        "SELECT event_id, aggregate_kind, aggregate_id, sequence, schema_name, schema_version,
                command_id, parent_event_id, observed_at_unix_ms, payload
         FROM events {suffix}"
    );
    let mut statement = connection.prepare(&sql).map_err(storage_error)?;
    let rows = statement
        .query_map(parameters, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Vec<u8>>(9)?,
            ))
        })
        .map_err(storage_error)?;

    let mut events = Vec::new();
    for row in rows {
        let (event_id, kind, id, sequence, schema, version, command, parent, observed, payload) =
            row.map_err(storage_error)?;
        events.push(EventEnvelope {
            event_id: EventId::from_str(&event_id).map_err(protocol_error)?,
            stream: StreamId {
                kind: AggregateKind::from_str(&kind).map_err(protocol_error)?,
                id: AggregateId::from_str(&id).map_err(protocol_error)?,
            },
            sequence: EventSequence::new(to_u64(sequence, "sequence")?).map_err(protocol_error)?,
            schema_name: SchemaName::from_str(&schema).map_err(protocol_error)?,
            schema_version: SchemaVersion::new(u32::try_from(version).map_err(|_| {
                EventStoreError::IntegerRange {
                    field: "schema_version",
                }
            })?)
            .map_err(protocol_error)?,
            command_id: CommandId::from_str(&command).map_err(protocol_error)?,
            parent_event_id: parent
                .map(|value| EventId::from_str(&value).map_err(protocol_error))
                .transpose()?,
            observed_at_unix_ms: observed,
            payload,
        });
    }
    Ok(events)
}

fn encode_expected(expected: ExpectedRevision) -> Result<(i64, Option<i64>), EventStoreError> {
    match expected {
        ExpectedRevision::NoStream => Ok((0, None)),
        ExpectedRevision::Exact(value) => Ok((1, Some(to_i64(value.get(), "expected_revision")?))),
    }
}

fn decode_expected(kind: i64, revision: Option<i64>) -> Result<ExpectedRevision, EventStoreError> {
    match (kind, revision) {
        (0, None) => Ok(ExpectedRevision::NoStream),
        (1, Some(value)) => Ok(ExpectedRevision::Exact(
            StreamRevision::new(to_u64(value, "expected_revision")?).map_err(protocol_error)?,
        )),
        _ => Err(EventStoreError::Storage {
            message: "invalid persisted expected revision".to_owned(),
        }),
    }
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, EventStoreError> {
    i64::try_from(value).map_err(|_| EventStoreError::IntegerRange { field })
}

fn to_u64(value: i64, field: &'static str) -> Result<u64, EventStoreError> {
    u64::try_from(value).map_err(|_| EventStoreError::IntegerRange { field })
}

fn storage_error(error: impl std::fmt::Display) -> EventStoreError {
    EventStoreError::Storage {
        message: error.to_string(),
    }
}

fn protocol_error(error: impl std::fmt::Display) -> EventStoreError {
    EventStoreError::Storage {
        message: format!("invalid persisted protocol value: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{
        AggregateId, AggregateKind, CommandId, SchemaName, SchemaVersion, StreamRevision,
    };
    use cairn_record::{EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId};

    use super::SqliteEventStore;

    fn stream() -> StreamId {
        StreamId {
            kind: AggregateKind::new("task").expect("kind"),
            id: AggregateId::new("task:one").expect("id"),
        }
    }

    fn event(payload: &[u8]) -> NewEvent {
        NewEvent {
            schema_name: SchemaName::new("task.created").expect("schema"),
            schema_version: SchemaVersion::new(1).expect("version"),
            parent_event_id: None,
            observed_at_unix_ms: 1_777_000_000_000,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn append_is_atomic_ordered_and_revision_checked() {
        let mut store = SqliteEventStore::in_memory().expect("store");
        let events = [event(b"{\"name\":\"one\"}"), event(b"{}")];
        let outcome = store
            .append(
                &stream(),
                ExpectedRevision::NoStream,
                &CommandId::new(),
                &events,
            )
            .expect("append");
        assert_eq!(outcome.first_sequence.get(), 1);
        assert_eq!(outcome.last_sequence.get(), 2);
        assert!(!outcome.was_replay);

        let stored = store.read_stream(&stream(), None).expect("read");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[1].sequence.get(), 2);

        let error = store
            .append(
                &stream(),
                ExpectedRevision::Exact(StreamRevision::new(1).expect("revision")),
                &CommandId::new(),
                &[event(b"{}")],
            )
            .expect_err("stale revision must fail");
        assert!(matches!(error, EventStoreError::RevisionConflict { .. }));
        assert_eq!(store.read_stream(&stream(), None).expect("read").len(), 2);
    }

    #[test]
    fn identical_command_retry_returns_prior_result() {
        let mut store = SqliteEventStore::in_memory().expect("store");
        let command = CommandId::new();
        let proposed = [event(b"{}")];
        let first = store
            .append(&stream(), ExpectedRevision::NoStream, &command, &proposed)
            .expect("first append");
        let replay = store
            .append(&stream(), ExpectedRevision::NoStream, &command, &proposed)
            .expect("idempotent replay");
        assert!(!first.was_replay);
        assert!(replay.was_replay);
        assert_eq!(first.event_ids, replay.event_ids);
        assert_eq!(store.read_stream(&stream(), None).expect("read").len(), 1);
    }

    #[test]
    fn command_id_cannot_authorize_different_input() {
        let mut store = SqliteEventStore::in_memory().expect("store");
        let command = CommandId::new();
        store
            .append(
                &stream(),
                ExpectedRevision::NoStream,
                &command,
                &[event(b"{}")],
            )
            .expect("first append");
        let error = store
            .append(
                &stream(),
                ExpectedRevision::NoStream,
                &command,
                &[event(b"{\"different\":true}")],
            )
            .expect_err("command reuse must fail");
        assert!(matches!(error, EventStoreError::CommandConflict { .. }));
    }

    #[test]
    fn file_store_survives_reopen() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("record.db");
        {
            let mut store = SqliteEventStore::open(&path).expect("open");
            store
                .append(
                    &stream(),
                    ExpectedRevision::NoStream,
                    &CommandId::new(),
                    &[event(b"{}")],
                )
                .expect("append");
        }
        let store = SqliteEventStore::open(path).expect("reopen");
        assert_eq!(store.read_stream(&stream(), None).expect("read").len(), 1);
    }
}
