use std::io::Cursor;

use cairn_protocol::{
    CommandId, ContentId, EventId, ModelAttemptId, ObservedAtUnixMillis, SchemaName, SchemaVersion,
    StreamRevision,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AdapterVersion, MaterializedRequestArtifact, ModelResponseArtifact, ModelTransport,
    PreparedModelRequest, TransportFailureClass, TurnInputDecisionArtifact,
};

const PREPARED: &str = "agent.model-request-prepared";
const STARTED: &str = "agent.model-dispatch-started";
const RESPONSE: &str = "agent.model-response-received";
const NOT_SENT: &str = "agent.model-dispatch-not-sent";
const REJECTED: &str = "agent.model-dispatch-rejected";
const AMBIGUOUS: &str = "agent.model-dispatch-ambiguous";

/// Failure of durable dispatch orchestration.
#[derive(Debug, Error)]
pub enum DispatchCoordinatorError {
    /// Durable event transition failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Provider response could not be archived.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// A transport failure occurred but its terminal fact could not be committed.
    #[error(
        "model attempt {attempt_id} transport failed ({transport}); recording the outcome also failed ({record})"
    )]
    UnrecordedTransportFailure {
        /// Attempt requiring reconciliation.
        attempt_id: ModelAttemptId,
        /// Original transport diagnostic.
        transport: String,
        /// Record failure diagnostic.
        record: String,
    },
    /// Response bytes were archived but their terminal fact could not be committed.
    #[error(
        "model attempt {attempt_id} archived response {response_id}, but recording completion failed ({record})"
    )]
    UnrecordedResponse {
        /// Attempt requiring reconciliation.
        attempt_id: ModelAttemptId,
        /// Recoverable response artifact identity.
        response_id: ContentId<ModelResponseArtifact>,
        /// Record failure diagnostic.
        record: String,
    },
    /// Persisted attempt events are malformed or contradict the state machine.
    #[error("invalid persisted model-attempt history: {0}")]
    InvalidHistory(String),
}

/// Proof that a prepared request has durable dispatch authority.
pub struct DispatchAuthority {
    attempt_id: ModelAttemptId,
    stream: StreamId,
    revision: StreamRevision,
    prepared_event_id: EventId,
    request: PreparedModelRequest,
}

/// Proof that the at-most-once external attempt was durably marked started.
pub struct StartedDispatch {
    attempt_id: ModelAttemptId,
    stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
    request: PreparedModelRequest,
}

/// One-shot proof that exact response bytes and their receipt event are durable.
#[derive(Debug)]
pub struct ReceivedModelResponse {
    pub(crate) attempt_id: ModelAttemptId,
    pub(crate) stream: StreamId,
    pub(crate) revision: StreamRevision,
    pub(crate) response_event_id: EventId,
    pub(crate) response_id: ContentId<ModelResponseArtifact>,
    pub(crate) adapter_version: AdapterVersion,
}

impl ReceivedModelResponse {
    /// Returns the provider-attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> ModelAttemptId {
        self.attempt_id
    }

    /// Returns the archived raw-response identity.
    #[must_use]
    pub const fn response_id(&self) -> ContentId<ModelResponseArtifact> {
        self.response_id
    }

    /// Returns the semantic adapter version pinned before dispatch.
    #[must_use]
    pub fn adapter_version(&self) -> &AdapterVersion {
        &self.adapter_version
    }
}

struct TerminalContext {
    stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
}

impl StartedDispatch {
    /// Returns the external attempt identity for reconciliation and diagnostics.
    #[must_use]
    pub const fn attempt_id(&self) -> ModelAttemptId {
        self.attempt_id
    }
}

/// Terminal outcome after both external effect and durable recording complete.
#[derive(Debug)]
pub enum DispatchCompletion {
    /// Exact response bytes were archived and cited by a durable event.
    Response(ReceivedModelResponse),
    /// Transport proved the request was not sent.
    NotSent,
    /// Provider definitively rejected the request.
    Rejected,
    /// External outcome cannot be determined.
    Ambiguous,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedPayload {
    #[serde(rename = "attempt_id")]
    attempt: ModelAttemptId,
    #[serde(rename = "decision_id")]
    decision: ContentId<TurnInputDecisionArtifact>,
    #[serde(rename = "request_id")]
    request: ContentId<MaterializedRequestArtifact>,
    adapter_version: AdapterVersion,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StartedPayload {
    #[serde(rename = "attempt_id")]
    attempt: ModelAttemptId,
    #[serde(rename = "request_id")]
    request: ContentId<MaterializedRequestArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OutcomePayload {
    attempt_id: ModelAttemptId,
    response_id: Option<ContentId<ModelResponseArtifact>>,
    diagnostic: Option<String>,
}

struct Fact<'a, P> {
    stream: &'a StreamId,
    expected: ExpectedRevision,
    command_id: &'a CommandId,
    schema: &'a str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &'a P,
}

/// Commits `ModelRequestPrepared` and returns unforgeable dispatch authority.
///
/// # Errors
///
/// Returns [`DispatchCoordinatorError`] when the prepared fact cannot be encoded or committed.
pub fn authorize_model_request<E: EventStore>(
    events: &mut E,
    stream: &StreamId,
    expected: ExpectedRevision,
    command_id: &CommandId,
    attempt_id: ModelAttemptId,
    observed_at: ObservedAtUnixMillis,
    request: PreparedModelRequest,
) -> Result<DispatchAuthority, DispatchCoordinatorError> {
    let payload = PreparedPayload {
        attempt: attempt_id,
        decision: request.decision_id,
        request: request.request_id,
        adapter_version: request.adapter_version.clone(),
    };
    let outcome = append_fact(
        events,
        &Fact {
            stream,
            expected,
            command_id,
            schema: PREPARED,
            parent_event_id: None,
            observed_at,
            payload: &payload,
        },
    )?;
    Ok(DispatchAuthority {
        attempt_id,
        stream: stream.clone(),
        revision: revision(outcome.last_sequence)?,
        prepared_event_id: outcome.event_ids[0],
        request,
    })
}

/// Commits `ModelDispatchStarted` before any external call may occur.
///
/// # Errors
///
/// Returns [`DispatchCoordinatorError`] when the started fact cannot be encoded or committed.
pub fn begin_model_dispatch<E: EventStore>(
    events: &mut E,
    authority: DispatchAuthority,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StartedDispatch, DispatchCoordinatorError> {
    let DispatchAuthority {
        attempt_id,
        stream,
        revision: current_revision,
        prepared_event_id,
        request,
    } = authority;
    let payload = StartedPayload {
        attempt: attempt_id,
        request: request.request_id,
    };
    let outcome = append_fact(
        events,
        &Fact {
            stream: &stream,
            expected: ExpectedRevision::Exact(current_revision),
            command_id,
            schema: STARTED,
            parent_event_id: Some(prepared_event_id),
            observed_at,
            payload: &payload,
        },
    )?;
    Ok(StartedDispatch {
        attempt_id,
        stream,
        revision: revision(outcome.last_sequence)?,
        started_event_id: outcome.event_ids[0],
        request,
    })
}

/// Executes exactly one started transport attempt, archives a response, and records a terminal fact.
/// The consumed [`StartedDispatch`] makes accidental in-process reuse a compile-time error.
///
/// ```compile_fail
/// use cairn_agent::{ModelTransport, StartedDispatch, execute_model_dispatch};
/// use cairn_protocol::{CommandId, ObservedAtUnixMillis};
/// use cairn_record::{ContentStore, EventStore};
///
/// fn dispatch_twice<E, C, T>(
///     events: &mut E,
///     content: &mut C,
///     transport: &mut T,
///     started: StartedDispatch,
///     command: &CommandId,
/// ) where
///     E: EventStore,
///     C: ContentStore,
///     T: ModelTransport,
/// {
///     let _ = execute_model_dispatch(
///         events, content, transport, started, command, ObservedAtUnixMillis::new(1),
///     );
///     let _ = execute_model_dispatch(
///         events, content, transport, started, command, ObservedAtUnixMillis::new(2),
///     );
/// }
/// ```
///
/// # Errors
///
/// Returns [`DispatchCoordinatorError`] when response archival or terminal-fact recording fails.
pub fn execute_model_dispatch<E: EventStore, C: ContentStore, T: ModelTransport>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    started: StartedDispatch,
    outcome_command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<DispatchCompletion, DispatchCoordinatorError> {
    let StartedDispatch {
        attempt_id,
        stream,
        revision: started_revision,
        started_event_id,
        request,
    } = started;
    let terminal = TerminalContext {
        stream,
        revision: started_revision,
        started_event_id,
    };
    match transport.dispatch(&request) {
        Ok(response_bytes) => {
            let descriptor =
                content.put::<ModelResponseArtifact>(&mut Cursor::new(response_bytes))?;
            let payload = OutcomePayload {
                attempt_id,
                response_id: Some(descriptor.content_id),
                diagnostic: None,
            };
            let outcome = append_terminal(
                events,
                &terminal,
                outcome_command_id,
                observed_at,
                RESPONSE,
                &payload,
            )
            .map_err(|record| DispatchCoordinatorError::UnrecordedResponse {
                attempt_id,
                response_id: descriptor.content_id,
                record: record.to_string(),
            })?;
            Ok(DispatchCompletion::Response(ReceivedModelResponse {
                attempt_id,
                stream: terminal.stream,
                revision: revision(outcome.last_sequence)?,
                response_event_id: outcome.event_ids[0],
                response_id: descriptor.content_id,
                adapter_version: request.adapter_version,
            }))
        }
        Err(error) => {
            let class = error.failure_class();
            let schema = match class {
                TransportFailureClass::NotSent => NOT_SENT,
                TransportFailureClass::Rejected => REJECTED,
                TransportFailureClass::Ambiguous => AMBIGUOUS,
            };
            let payload = OutcomePayload {
                attempt_id,
                response_id: None,
                diagnostic: Some(error.to_string()),
            };
            append_terminal(
                events,
                &terminal,
                outcome_command_id,
                observed_at,
                schema,
                &payload,
            )
            .map_err(|record| {
                DispatchCoordinatorError::UnrecordedTransportFailure {
                    attempt_id,
                    transport: error.to_string(),
                    record: record.to_string(),
                }
            })?;
            Ok(match class {
                TransportFailureClass::NotSent => DispatchCompletion::NotSent,
                TransportFailureClass::Rejected => DispatchCompletion::Rejected,
                TransportFailureClass::Ambiguous => DispatchCompletion::Ambiguous,
            })
        }
    }
}

fn append_terminal<E: EventStore>(
    events: &mut E,
    terminal: &TerminalContext,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &OutcomePayload,
) -> Result<cairn_record::AppendOutcome, DispatchCoordinatorError> {
    append_fact(
        events,
        &Fact {
            stream: &terminal.stream,
            expected: ExpectedRevision::Exact(terminal.revision),
            command_id,
            schema,
            parent_event_id: Some(terminal.started_event_id),
            observed_at,
            payload,
        },
    )
}

fn append_fact<E: EventStore, P: Serialize>(
    events: &mut E,
    fact: &Fact<'_, P>,
) -> Result<cairn_record::AppendOutcome, DispatchCoordinatorError> {
    let bytes = cairn_codec::to_vec(fact.payload)
        .map_err(|error| DispatchCoordinatorError::InvalidHistory(error.to_string()))?;
    let event = NewEvent {
        schema_name: SchemaName::new(fact.schema)
            .map_err(|error| DispatchCoordinatorError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| DispatchCoordinatorError::InvalidHistory(error.to_string()))?,
        parent_event_id: fact.parent_event_id,
        observed_at_unix_ms: fact.observed_at.get(),
        payload: bytes,
    };
    events
        .append(fact.stream, fact.expected, fact.command_id, &[event])
        .map_err(Into::into)
}

fn revision(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, DispatchCoordinatorError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| DispatchCoordinatorError::InvalidHistory(error.to_string()))
}

/// Recovered durable state for one model attempt. A started attempt without terminal fact is in doubt.
#[derive(Debug, Eq, PartialEq)]
pub enum ModelAttemptState {
    /// No event for the identity exists.
    NotFound,
    /// Prepared authority exists but dispatch was never marked started.
    Authorized,
    /// Dispatch was marked started and has no terminal fact; never blind-retry it.
    InDoubt,
    /// Response bytes were archived and cited.
    Completed {
        response_id: ContentId<ModelResponseArtifact>,
    },
    /// Transport proved no request was sent.
    NotSent,
    /// Provider rejected the request.
    Rejected,
    /// Transport outcome was explicitly ambiguous.
    Ambiguous,
}

/// Rebuilds model-attempt state only from committed event facts.
///
/// # Errors
///
/// Returns [`DispatchCoordinatorError::InvalidHistory`] when relevant facts are malformed or do
/// not follow the prepared, started, terminal state machine.
pub fn recover_model_attempt(
    events: &[EventEnvelope],
    attempt_id: ModelAttemptId,
) -> Result<ModelAttemptState, DispatchCoordinatorError> {
    let mut state = ModelAttemptState::NotFound;
    let mut request_id = None;
    let mut prepared_event_id = None;
    let mut started_event_id = None;
    for event in events {
        let schema = event.schema_name.as_str();
        if schema == PREPARED {
            let payload: PreparedPayload = decode_payload(event)?;
            if payload.attempt != attempt_id {
                continue;
            }
            transition(&state, &ModelAttemptState::NotFound, PREPARED)?;
            if event.parent_event_id.is_some() {
                return invalid_history("prepared event must not have a causal parent");
            }
            request_id = Some(payload.request);
            prepared_event_id = Some(event.event_id);
            state = ModelAttemptState::Authorized;
        } else if schema == STARTED {
            let payload: StartedPayload = decode_payload(event)?;
            if payload.attempt != attempt_id {
                continue;
            }
            transition(&state, &ModelAttemptState::Authorized, STARTED)?;
            if Some(payload.request) != request_id {
                return invalid_history("started event cites a different prepared request");
            }
            if event.parent_event_id != prepared_event_id {
                return invalid_history("started event does not cite its prepared event");
            }
            started_event_id = Some(event.event_id);
            state = ModelAttemptState::InDoubt;
        } else if [RESPONSE, NOT_SENT, REJECTED, AMBIGUOUS].contains(&schema) {
            let payload: OutcomePayload = decode_payload(event)?;
            if payload.attempt_id != attempt_id {
                continue;
            }
            transition(&state, &ModelAttemptState::InDoubt, schema)?;
            if event.parent_event_id != started_event_id {
                return invalid_history("terminal event does not cite its started event");
            }
            state = match schema {
                RESPONSE if payload.diagnostic.is_some() => {
                    return invalid_history("response event unexpectedly has a failure diagnostic");
                }
                RESPONSE => ModelAttemptState::Completed {
                    response_id: payload.response_id.ok_or_else(|| {
                        DispatchCoordinatorError::InvalidHistory(
                            "response event lacks response identity".to_owned(),
                        )
                    })?,
                },
                NOT_SENT | REJECTED | AMBIGUOUS if payload.response_id.is_some() => {
                    return invalid_history("failure event unexpectedly cites a response");
                }
                NOT_SENT | REJECTED | AMBIGUOUS if payload.diagnostic.is_none() => {
                    return invalid_history("failure event lacks a diagnostic");
                }
                NOT_SENT => ModelAttemptState::NotSent,
                REJECTED => ModelAttemptState::Rejected,
                AMBIGUOUS => ModelAttemptState::Ambiguous,
                _ => unreachable!("schema was filtered above"),
            };
        }
    }
    Ok(state)
}

/// Reconstructs one-shot semantic-decoding authority from a valid response-received history.
///
/// # Errors
///
/// Returns [`DispatchCoordinatorError`] when the attempt history is malformed.
pub fn recover_received_model_response(
    events: &[EventEnvelope],
    attempt_id: ModelAttemptId,
) -> Result<Option<ReceivedModelResponse>, DispatchCoordinatorError> {
    let ModelAttemptState::Completed { response_id } = recover_model_attempt(events, attempt_id)?
    else {
        return Ok(None);
    };
    let adapter_version = prepared_adapter_version(events, attempt_id)?;
    for event in events.iter().rev() {
        if event.schema_name.as_str() != RESPONSE {
            continue;
        }
        let payload: OutcomePayload = decode_payload(event)?;
        if payload.attempt_id == attempt_id {
            if payload.response_id != Some(response_id) {
                return invalid_history("recovered response identity changed during projection");
            }
            return Ok(Some(ReceivedModelResponse {
                attempt_id,
                stream: event.stream.clone(),
                revision: revision(event.sequence)?,
                response_event_id: event.event_id,
                response_id,
                adapter_version,
            }));
        }
    }
    invalid_history("completed attempt has no response event")
}

fn prepared_adapter_version(
    events: &[EventEnvelope],
    attempt_id: ModelAttemptId,
) -> Result<AdapterVersion, DispatchCoordinatorError> {
    for event in events {
        if event.schema_name.as_str() != PREPARED {
            continue;
        }
        let payload: PreparedPayload = decode_payload(event)?;
        if payload.attempt == attempt_id {
            return Ok(payload.adapter_version);
        }
    }
    invalid_history("completed attempt has no prepared adapter version")
}

fn decode_payload<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, DispatchCoordinatorError> {
    if event.schema_version.get() != 1 {
        return invalid_history("unsupported model-dispatch event schema version");
    }
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| DispatchCoordinatorError::InvalidHistory(error.to_string()))
}

fn invalid_history<T>(message: &str) -> Result<T, DispatchCoordinatorError> {
    Err(DispatchCoordinatorError::InvalidHistory(message.to_owned()))
}

fn transition(
    actual: &ModelAttemptState,
    expected: &ModelAttemptState,
    schema: &str,
) -> Result<(), DispatchCoordinatorError> {
    if actual == expected {
        Ok(())
    } else {
        Err(DispatchCoordinatorError::InvalidHistory(format!(
            "event {schema} follows state {actual:?}, expected {expected:?}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{AggregateId, AggregateKind, CommandId, ContentId, ModelAttemptId};
    use cairn_record::{ContentStore, EventStore, ExpectedRevision, StreamId};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        DispatchCompletion, ModelAttemptState, authorize_model_request, begin_model_dispatch,
        execute_model_dispatch, recover_model_attempt,
    };
    use crate::{
        MaterializedRequestArtifact, PreparedModelRequest, ScriptedModelTransport, TransportError,
        TurnInputDecisionArtifact,
    };

    fn stream() -> StreamId {
        StreamId {
            kind: AggregateKind::new("agent-episode").expect("kind"),
            id: AggregateId::new("agent-episode:test").expect("id"),
        }
    }

    fn prepared() -> PreparedModelRequest {
        PreparedModelRequest {
            decision_id: ContentId::<TurnInputDecisionArtifact>::derive(b"decision")
                .expect("decision id"),
            request_id: ContentId::<MaterializedRequestArtifact>::derive(b"request")
                .expect("request id"),
            adapter_version: crate::AdapterVersion::new("v1").expect("adapter"),
            request_bytes: b"request".to_vec(),
        }
    }

    #[test]
    fn started_without_terminal_recovers_as_in_doubt() {
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let stream = stream();
        let attempt = ModelAttemptId::new();
        let authority = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &CommandId::new(),
            attempt,
            cairn_protocol::ObservedAtUnixMillis::new(1),
            prepared(),
        )
        .expect("authorize");
        let history = events.read_stream(&stream, None).expect("read prepared");
        assert_eq!(
            recover_model_attempt(&history, attempt).expect("recover prepared"),
            ModelAttemptState::Authorized
        );

        let _started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let history = events.read_stream(&stream, None).expect("read started");
        assert_eq!(
            recover_model_attempt(&history, attempt).expect("recover started"),
            ModelAttemptState::InDoubt
        );
    }

    #[test]
    fn response_is_archived_before_completion_is_recoverable() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store");
        let stream = stream();
        let attempt = ModelAttemptId::new();
        let authority = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &CommandId::new(),
            attempt,
            cairn_protocol::ObservedAtUnixMillis::new(1),
            prepared(),
        )
        .expect("authorize");
        let started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"provider response".to_vec())
        });
        let completion = execute_model_dispatch(
            &mut events,
            &mut content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("execute");
        let DispatchCompletion::Response(received) = completion else {
            panic!("expected response completion");
        };
        let response_id = received.response_id();
        let mut archived = Vec::new();
        content
            .write_to(&response_id, &mut archived)
            .expect("read response");
        assert_eq!(archived, b"provider response");

        let history = events.read_stream(&stream, None).expect("read completed");
        assert_eq!(
            recover_model_attempt(&history, attempt).expect("recover completed"),
            ModelAttemptState::Completed { response_id }
        );
    }

    #[test]
    fn ambiguous_transport_outcome_is_durable_and_not_a_retry_signal() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store");
        let stream = stream();
        let attempt = ModelAttemptId::new();
        let authority = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &CommandId::new(),
            attempt,
            cairn_protocol::ObservedAtUnixMillis::new(1),
            prepared(),
        )
        .expect("authorize");
        let started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Err(TransportError::Ambiguous("connection lost".to_owned()))
        });
        let completion = execute_model_dispatch(
            &mut events,
            &mut content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("record ambiguous result");
        assert!(matches!(completion, DispatchCompletion::Ambiguous));

        let history = events.read_stream(&stream, None).expect("read ambiguous");
        assert_eq!(
            recover_model_attempt(&history, attempt).expect("recover ambiguous"),
            ModelAttemptState::Ambiguous
        );
    }

    #[test]
    fn repeated_authorization_command_does_not_duplicate_facts() {
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let stream = stream();
        let attempt = ModelAttemptId::new();
        let command = CommandId::new();
        let observed_at = cairn_protocol::ObservedAtUnixMillis::new(1);
        let request = prepared();
        let _first = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &command,
            attempt,
            observed_at,
            request.clone(),
        )
        .expect("first authorization");
        let _replay = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &command,
            attempt,
            observed_at,
            request,
        )
        .expect("replayed authorization");
        assert_eq!(events.read_stream(&stream, None).expect("read").len(), 1);
    }

    #[test]
    fn recovery_rejects_a_broken_causal_chain() {
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let stream = stream();
        let attempt = ModelAttemptId::new();
        let authority = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &CommandId::new(),
            attempt,
            cairn_protocol::ObservedAtUnixMillis::new(1),
            prepared(),
        )
        .expect("authorize");
        let _started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut history = events.read_stream(&stream, None).expect("read");
        history[1].parent_event_id = None;
        assert!(recover_model_attempt(&history, attempt).is_err());
    }
}
