//! Durable record ports and format-neutral event semantics.

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, EventId, EventSequence, SchemaName, SchemaVersion,
    StreamRevision,
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
    /// Stable event identity. Derivation is intentionally outside the store.
    pub event_id: EventId,
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

/// Append-only event-store contract.
pub trait EventStore {
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
