//! Durable record ports and format-neutral event semantics.

use std::io::{Read, Write};

use cairn_protocol::{
    AggregateId, AggregateKind, BlobDigest, CommandId, ContentId, ContentType, EventId,
    EventSequence, SchemaName, SchemaVersion, StreamRevision,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Identifies an aggregate event stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StreamId {
    /// Aggregate category.
    pub kind: AggregateKind,
    /// Aggregate lifecycle identifier.
    pub id: AggregateId,
}

/// Optimistic concurrency precondition for an append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedRevision {
    /// The aggregate must not exist.
    NoStream,
    /// The aggregate must currently have this revision.
    Exact(StreamRevision),
}

/// Caller-supplied immutable event fields. Sequence and command causality are assigned by append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEvent {
    /// Payload schema name.
    pub schema_name: SchemaName,
    /// Payload schema version.
    pub schema_version: SchemaVersion,
    /// Optional causal parent event.
    pub parent_event_id: Option<EventId>,
    /// Observation timestamp in Unix milliseconds; not an ordering authority.
    pub observed_at_unix_ms: i64,
    /// Canonical payload bytes.
    pub payload: Vec<u8>,
}

/// One committed event envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventEnvelope {
    /// Event identity.
    pub event_id: EventId,
    /// Aggregate stream.
    pub stream: StreamId,
    /// One-based aggregate sequence.
    pub sequence: EventSequence,
    /// Payload schema name.
    pub schema_name: SchemaName,
    /// Payload schema version.
    pub schema_version: SchemaVersion,
    /// Command that authorized the event.
    pub command_id: CommandId,
    /// Optional causal parent event.
    pub parent_event_id: Option<EventId>,
    /// Observation timestamp in Unix milliseconds.
    pub observed_at_unix_ms: i64,
    /// Canonical payload bytes.
    pub payload: Vec<u8>,
}

/// Successful append result, including whether this was an idempotent replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendOutcome {
    /// First sequence in the command batch.
    pub first_sequence: EventSequence,
    /// Last sequence in the command batch.
    pub last_sequence: EventSequence,
    /// Event identities in sequence order.
    pub event_ids: Vec<EventId>,
    /// True when the command had already committed with exactly the same input.
    pub was_replay: bool,
}

/// Storage-port error with no adapter-specific error type leakage.
#[derive(Debug, Error)]
pub enum EventStoreError {
    /// An append batch must contain at least one event.
    #[error("event append batch cannot be empty")]
    EmptyBatch,
    /// Event payload bytes are not valid canonical JSON V1.
    #[error("event payload is not canonical JSON V1: {message}")]
    InvalidEventPayload {
        /// Codec diagnostic.
        message: String,
    },
    /// Optimistic concurrency failed.
    #[error("expected revision {expected:?}, but current revision is {current:?}")]
    RevisionConflict {
        /// Requested revision.
        expected: ExpectedRevision,
        /// Observed revision; `None` means the stream does not exist.
        current: Option<StreamRevision>,
    },
    /// A command identifier was reused for different input.
    #[error("command {command_id} was already used with different input")]
    CommandConflict {
        /// Conflicting command identity.
        command_id: CommandId,
    },
    /// A persisted integer cannot be represented safely.
    #[error("stored integer is outside the supported range: {field}")]
    IntegerRange {
        /// Field that failed conversion.
        field: &'static str,
    },
    /// Adapter failure without exposing its concrete dependency.
    #[error("event storage failure: {message}")]
    Storage {
        /// Diagnostic suitable for logs and tests.
        message: String,
    },
}

#[derive(Serialize)]
struct EventIdentityMaterial<'a> {
    stream: &'a StreamId,
    sequence: EventSequence,
    schema_name: &'a SchemaName,
    schema_version: SchemaVersion,
    encoding: &'static str,
    command_id: &'a CommandId,
    parent_event_id: Option<EventId>,
    observed_at_unix_ms: i64,
    payload: serde_json::Value,
}

/// Derives an event identity from the complete canonical envelope material available after sequence
/// allocation. The identity field itself is intentionally absent from the preimage.
///
/// # Errors
///
/// Returns [`EventStoreError::InvalidEventPayload`] when payload bytes are not canonical JSON V1 or
/// when identity material cannot be encoded/hashed.
pub fn derive_event_id(
    stream: &StreamId,
    sequence: EventSequence,
    command_id: &CommandId,
    event: &NewEvent,
) -> Result<EventId, EventStoreError> {
    let payload =
        cairn_codec::from_slice::<serde_json::Value>(&event.payload).map_err(|error| {
            EventStoreError::InvalidEventPayload {
                message: error.to_string(),
            }
        })?;
    let material = EventIdentityMaterial {
        stream,
        sequence,
        schema_name: &event.schema_name,
        schema_version: event.schema_version,
        encoding: cairn_codec::ENCODING_ID,
        command_id,
        parent_event_id: event.parent_event_id,
        observed_at_unix_ms: event.observed_at_unix_ms,
        payload,
    };
    let bytes =
        cairn_codec::to_vec(&material).map_err(|error| EventStoreError::InvalidEventPayload {
            message: error.to_string(),
        })?;
    EventId::derive(&bytes).map_err(|error| EventStoreError::InvalidEventPayload {
        message: error.to_string(),
    })
}

/// Append-only event-store contract.
pub trait EventStore {
    /// Lists aggregate streams of one exact kind in aggregate-identity order.
    ///
    /// This is a discovery read model over the authoritative stream catalog. It does not create a
    /// second task registry and it grants no authority to mutate the returned aggregates.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when persisted identities fail validation or the adapter cannot
    /// read the stream catalog.
    fn list_streams(&self, kind: &AggregateKind) -> Result<Vec<StreamId>, EventStoreError>;

    /// Atomically appends a non-empty event batch under an expected stream revision.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the precondition fails, a command identity conflicts, the
    /// batch is invalid, or the adapter cannot commit the complete batch.
    fn append(
        &mut self,
        stream: &StreamId,
        expected: ExpectedRevision,
        command_id: &CommandId,
        events: &[NewEvent],
    ) -> Result<AppendOutcome, EventStoreError>;

    /// Reads committed events after `after_sequence`, in ascending sequence order. `None` starts at
    /// the beginning without assigning sentinel meaning to the invalid sequence zero.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when persisted values fail validation or the adapter cannot read
    /// the stream.
    fn read_stream(
        &self,
        stream: &StreamId,
        after_sequence: Option<EventSequence>,
    ) -> Result<Vec<EventEnvelope>, EventStoreError>;
}

/// Verified relationship between one semantic identity and its physical bytes.
pub struct ContentDescriptor<T: ContentType> {
    /// Domain-separated public identity.
    pub content_id: ContentId<T>,
    /// Internal exact-byte storage digest.
    pub blob_digest: BlobDigest,
    /// Verified physical byte length.
    pub byte_len: u64,
}

impl<T: ContentType> Copy for ContentDescriptor<T> {}

impl<T: ContentType> Clone for ContentDescriptor<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ContentType> std::fmt::Debug for ContentDescriptor<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContentDescriptor")
            .field("content_id", &self.content_id)
            .field("blob_digest", &self.blob_digest)
            .field("byte_len", &self.byte_len)
            .finish()
    }
}

/// Content-addressed storage failure.
#[derive(Debug, Error)]
pub enum ContentStoreError {
    /// The semantic identity has no metadata binding.
    #[error("content object was not found: {content_id}")]
    NotFound {
        /// Requested tagged identity.
        content_id: String,
    },
    /// Physical bytes or metadata do not match their recorded identities.
    #[error("content integrity failure: {message}")]
    Integrity {
        /// Mismatch diagnostic.
        message: String,
    },
    /// Filesystem or stream operation failed.
    #[error("content I/O failure: {message}")]
    Io {
        /// Adapter-neutral I/O diagnostic.
        message: String,
    },
    /// Metadata adapter failed.
    #[error("content metadata failure: {message}")]
    Metadata {
        /// Adapter-neutral metadata diagnostic.
        message: String,
    },
}

/// Strongly typed, streaming content-addressed storage contract.
///
/// Generic methods intentionally keep `ContentId<T>` intact. An erased metadata representation may
/// exist inside an adapter, but product logic cannot exchange content domains through this port.
pub trait ContentStore {
    /// Streams exact bytes into immutable storage and binds their typed semantic identity.
    ///
    /// # Errors
    ///
    /// Returns [`ContentStoreError`] when input cannot be read, bytes cannot be published, metadata
    /// cannot commit, or an existing binding fails verification.
    fn put<T: ContentType>(
        &mut self,
        reader: &mut dyn Read,
    ) -> Result<ContentDescriptor<T>, ContentStoreError>;

    /// Verifies the complete stored object, then streams it to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`ContentStoreError`] when metadata/object bytes are missing or corrupt, or output
    /// cannot be written.
    fn write_to<T: ContentType>(
        &self,
        content_id: &ContentId<T>,
        writer: &mut dyn Write,
    ) -> Result<ContentDescriptor<T>, ContentStoreError>;
}

/// Efficient immutable range-read port for resumable replication.
///
/// A range read validates the typed metadata binding, physical length, and requested bounds but
/// does not claim that one range proves the complete object's digest. Callers must first validate
/// the authoritative source object and must verify the fully assembled destination identity.
pub trait ContentRangeStore: ContentStore {
    /// Streams one exact contiguous range to `writer`.
    ///
    /// # Errors
    ///
    /// Returns [`ContentStoreError`] for an unknown typed object, invalid range, changed physical
    /// length, short write, or adapter failure.
    fn write_range_to<T: ContentType>(
        &self,
        content_id: &ContentId<T>,
        offset: u64,
        byte_len: u64,
        writer: &mut dyn Write,
    ) -> Result<ContentDescriptor<T>, ContentStoreError>;
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{
        AggregateId, AggregateKind, CommandId, EventId, EventSequence, SchemaName, SchemaVersion,
    };

    use super::{NewEvent, StreamId, derive_event_id};

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
    fn every_authoritative_envelope_change_mutates_event_identity() {
        let stream = StreamId {
            kind: AggregateKind::new("task").expect("kind"),
            id: AggregateId::new("task:fixture").expect("id"),
        };
        let command = CommandId::new();
        let sequence = EventSequence::new(1).expect("sequence");
        let base = event(b"{\"value\":1}");
        let base_id = derive_event_id(&stream, sequence, &command, &base).expect("base id");

        let mut changed_payload = event(b"{\"value\":2}");
        assert_ne!(
            base_id,
            derive_event_id(&stream, sequence, &command, &changed_payload).expect("payload id")
        );
        changed_payload.payload = base.payload.clone();
        changed_payload.observed_at_unix_ms += 1;
        assert_ne!(
            base_id,
            derive_event_id(&stream, sequence, &command, &changed_payload).expect("timestamp id")
        );
        assert_ne!(
            base_id,
            derive_event_id(
                &stream,
                EventSequence::new(2).expect("sequence"),
                &command,
                &base
            )
            .expect("sequence id")
        );
        assert_ne!(
            base_id,
            derive_event_id(&stream, sequence, &CommandId::new(), &base).expect("command id")
        );

        let other_stream = StreamId {
            kind: stream.kind.clone(),
            id: AggregateId::new("task:other").expect("other id"),
        };
        assert_ne!(
            base_id,
            derive_event_id(&other_stream, sequence, &command, &base).expect("stream id")
        );

        let mut changed_schema = event(&base.payload);
        changed_schema.schema_version = SchemaVersion::new(99).expect("unsupported version");
        assert_ne!(
            base_id,
            derive_event_id(&stream, sequence, &command, &changed_schema).expect("schema id")
        );

        let mut changed_parent = event(&base.payload);
        changed_parent.parent_event_id =
            Some(EventId::derive(b"parent fixture").expect("parent event id"));
        assert_ne!(
            base_id,
            derive_event_id(&stream, sequence, &command, &changed_parent).expect("parent id")
        );
    }

    #[test]
    fn event_identity_rejects_noncanonical_payload() {
        let stream = StreamId {
            kind: AggregateKind::new("task").expect("kind"),
            id: AggregateId::new("task:fixture").expect("id"),
        };
        let error = derive_event_id(
            &stream,
            EventSequence::new(1).expect("sequence"),
            &CommandId::new(),
            &event(b"{ \"not\":\"canonical\" }"),
        )
        .expect_err("noncanonical payload must fail");
        assert!(error.to_string().contains("not canonical"));
    }
}
