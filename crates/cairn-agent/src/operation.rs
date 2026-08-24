use std::{
    collections::{HashSet, VecDeque},
    io::Cursor,
};

use cairn_protocol::{
    AggregateId, AggregateKind, AttemptId, CommandId, ContentId, EventId, ObservedAtUnixMillis,
    OperationId, SchemaName, SchemaVersion, StreamRevision,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    OperationReconciliationEvidence, OperationResult, ToolArguments, ToolCallId,
    ToolImplementationVersion, ToolName,
};

const PREPARED: &str = "agent.tool-operation-prepared";
const STARTED: &str = "agent.tool-operation-started";
const COMPLETED: &str = "agent.tool-operation-completed";
const NOT_STARTED: &str = "agent.tool-operation-not-started";
const REJECTED: &str = "agent.tool-operation-rejected";
const AMBIGUOUS: &str = "agent.tool-operation-ambiguous";
const RETRY_AUTHORIZED: &str = "agent.tool-operation-retry-authorized";
const RECONCILED_NOT_OCCURRED: &str = "agent.tool-operation-reconciled-not-occurred";
const RECONCILED_COMPLETED: &str = "agent.tool-operation-reconciled-completed";

/// External-effect semantics declared by trusted tool registration, never by model text.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolEffectClass {
    /// Deterministic local computation with no external effect.
    Pure,
    /// Observation that does not mutate the external subject.
    ReadOnly,
    /// Mutation protected by the stable [`OperationId`] as an idempotency key.
    Idempotent,
    /// Effect that may happen at most once and cannot safely be repeated after ambiguity.
    AtMostOnce,
    /// External mutation whose outcome requires explicit reconciliation.
    AmbiguousExternal,
}

/// Recovery action derived from effect semantics rather than diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationRecovery {
    /// The same logical operation may be attempted again under explicit runtime policy.
    RetrySameOperation,
    /// External evidence or caller authority is required before another attempt.
    ReconcileRequired,
}

impl ToolEffectClass {
    /// Returns the recovery class for an interrupted or ambiguous attempt.
    #[must_use]
    pub const fn recovery(self) -> OperationRecovery {
        match self {
            Self::Pure | Self::ReadOnly | Self::Idempotent => OperationRecovery::RetrySameOperation,
            Self::AtMostOnce | Self::AmbiguousExternal => OperationRecovery::ReconcileRequired,
        }
    }
}

/// Canonical model-visible tool result bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalToolResult(Vec<u8>);

impl CanonicalToolResult {
    /// Validates strict canonical JSON V1 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ToolResultEncodingError`] when bytes are invalid or non-canonical.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ToolResultEncodingError> {
        cairn_codec::from_slice::<serde_json::Value>(&bytes)
            .map_err(|error| ToolResultEncodingError(error.to_string()))?;
        Ok(Self(bytes))
    }

    /// Encodes a value as canonical JSON V1.
    ///
    /// # Errors
    ///
    /// Returns [`ToolResultEncodingError`] when the value contains an unsupported representation.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, ToolResultEncodingError> {
        cairn_codec::to_vec(value)
            .map(Self)
            .map_err(|error| ToolResultEncodingError(error.to_string()))
    }

    /// Returns the exact archived bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Invalid model-visible tool-result representation.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("tool result is not canonical JSON V1: {0}")]
pub struct ToolResultEncodingError(String);

/// Prepared immutable operation. Only [`prepare_tool_operation`] can construct it.
///
/// ```compile_fail
/// use cairn_agent::PreparedToolOperation;
///
/// let forged = PreparedToolOperation {
///     operation_id: todo!(),
///     tool: todo!(),
///     implementation_version: todo!(),
///     effect: todo!(),
///     arguments_id: todo!(),
///     argument_bytes: Vec::new(),
/// };
/// ```
#[derive(Clone, Debug)]
pub struct PreparedToolOperation {
    operation_id: OperationId,
    source_tool_call_id: Option<ToolCallId>,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
    argument_bytes: Vec<u8>,
}

impl PreparedToolOperation {
    /// Returns the stable logical operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Returns the decoded model tool call that proposed this operation, when present.
    #[must_use]
    pub const fn source_tool_call_id(&self) -> Option<ToolCallId> {
        self.source_tool_call_id
    }

    /// Returns the trusted registered tool name.
    #[must_use]
    pub fn tool(&self) -> &ToolName {
        &self.tool
    }

    /// Returns the pinned tool implementation version.
    #[must_use]
    pub fn implementation_version(&self) -> &ToolImplementationVersion {
        &self.implementation_version
    }

    /// Returns the trusted external-effect class.
    #[must_use]
    pub const fn effect(&self) -> ToolEffectClass {
        self.effect
    }

    /// Returns the semantic identity of the exact argument bytes.
    #[must_use]
    pub const fn arguments_id(&self) -> ContentId<ToolArguments> {
        self.arguments_id
    }

    /// Returns the exact canonical argument bytes.
    #[must_use]
    pub fn argument_bytes(&self) -> &[u8] {
        &self.argument_bytes
    }

    pub(crate) fn from_tool_call(
        operation_id: OperationId,
        source_tool_call_id: ToolCallId,
        tool: ToolName,
        implementation_version: ToolImplementationVersion,
        effect: ToolEffectClass,
        arguments_id: ContentId<ToolArguments>,
        argument_bytes: Vec<u8>,
    ) -> Self {
        Self {
            operation_id,
            source_tool_call_id: Some(source_tool_call_id),
            tool,
            implementation_version,
            effect,
            arguments_id,
            argument_bytes,
        }
    }
}

/// Canonicalizes and archives exact tool arguments before operation authorization.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when arguments cannot be encoded or archived.
pub fn prepare_tool_operation<C: ContentStore>(
    content: &mut C,
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments: &serde_json::Value,
) -> Result<PreparedToolOperation, OperationCoordinatorError> {
    let argument_bytes = cairn_codec::to_vec(arguments)
        .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?;
    let descriptor = content.put::<ToolArguments>(&mut Cursor::new(&argument_bytes))?;
    Ok(PreparedToolOperation {
        operation_id,
        source_tool_call_id: None,
        tool,
        implementation_version,
        effect,
        arguments_id: descriptor.content_id,
        argument_bytes,
    })
}

/// Tool invocation failure classified at the gateway boundary.
#[derive(Debug, Error)]
pub enum ToolGatewayError {
    /// Recorded operation does not match the dispatched argument identity.
    #[error("recorded tool arguments do not match")]
    RequestMismatch,
    /// No scripted or recorded result remains.
    #[error("tool fixture is exhausted")]
    Exhausted,
    /// Gateway proves the tool implementation did not begin.
    #[error("tool operation did not start: {0}")]
    NotStarted(String),
    /// Policy or implementation definitively rejected the operation.
    #[error("tool operation was rejected: {0}")]
    Rejected(String),
    /// Gateway cannot determine whether the tool effect occurred.
    #[error("tool operation outcome is ambiguous: {0}")]
    Ambiguous(String),
    /// Scripted gateway failure with no stronger evidence.
    #[error("scripted tool gateway failed: {0}")]
    Scripted(String),
}

/// Durable gateway failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolGatewayFailureClass {
    /// The external implementation did not begin.
    NotStarted,
    /// The operation was definitively rejected.
    Rejected,
    /// The effect may have occurred.
    Ambiguous,
}

impl ToolGatewayError {
    /// Classifies effect outcome without parsing the diagnostic string.
    #[must_use]
    pub const fn failure_class(&self) -> ToolGatewayFailureClass {
        match self {
            Self::RequestMismatch | Self::Exhausted | Self::NotStarted(_) => {
                ToolGatewayFailureClass::NotStarted
            }
            Self::Rejected(_) => ToolGatewayFailureClass::Rejected,
            Self::Ambiguous(_) | Self::Scripted(_) => ToolGatewayFailureClass::Ambiguous,
        }
    }
}

/// Replaceable tool capability used identically by live, recorded, and scripted implementations.
pub trait ToolGateway {
    /// Executes one prepared operation and returns canonical model-visible result bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ToolGatewayError`] with typed external-effect semantics.
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError>;
}

/// One exact argument/result exchange for deterministic tool replay.
pub struct RecordedToolExchange {
    /// Argument identity required by this exchange.
    pub arguments_id: ContentId<ToolArguments>,
    /// Canonical archived result.
    pub result: CanonicalToolResult,
}

/// FIFO recorded tool provider with no replay branch in the operation coordinator.
pub struct RecordedToolGateway {
    exchanges: VecDeque<RecordedToolExchange>,
}

impl RecordedToolGateway {
    /// Creates a provider from ordered recorded exchanges.
    pub fn new(exchanges: impl IntoIterator<Item = RecordedToolExchange>) -> Self {
        Self {
            exchanges: exchanges.into_iter().collect(),
        }
    }
}

impl ToolGateway for RecordedToolGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        let exchange = self
            .exchanges
            .pop_front()
            .ok_or(ToolGatewayError::Exhausted)?;
        if exchange.arguments_id != operation.arguments_id {
            return Err(ToolGatewayError::RequestMismatch);
        }
        Ok(exchange.result)
    }
}

/// Closure-backed tool provider for tests and embedders.
pub struct ScriptedToolGateway<F>(F);

impl<F> ScriptedToolGateway<F> {
    /// Wraps a scripted tool implementation.
    pub const fn new(script: F) -> Self {
        Self(script)
    }
}

impl<F> ToolGateway for ScriptedToolGateway<F>
where
    F: FnMut(&PreparedToolOperation) -> Result<CanonicalToolResult, ToolGatewayError>,
{
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        (self.0)(operation)
    }
}

/// Failure of durable tool-operation coordination.
#[derive(Debug, Error)]
pub enum OperationCoordinatorError {
    /// Durable event transition failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Arguments or result bytes could not be archived.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Input, encoding, or persisted state violated the operation contract.
    #[error("invalid tool operation: {0}")]
    InvalidOperation(String),
    /// Gateway failed but its terminal fact could not be committed.
    #[error(
        "operation {operation_id} gateway failed ({gateway}); recording the outcome also failed ({record})"
    )]
    UnrecordedGatewayFailure {
        /// Operation requiring reconciliation.
        operation_id: OperationId,
        /// Original gateway diagnostic.
        gateway: String,
        /// Record failure diagnostic.
        record: String,
    },
    /// Result bytes were archived but their terminal fact could not be committed.
    #[error(
        "operation {operation_id} archived result {result_id}, but recording completion failed ({record})"
    )]
    UnrecordedResult {
        /// Operation requiring reconciliation.
        operation_id: OperationId,
        /// Recoverable result artifact identity.
        result_id: ContentId<OperationResult>,
        /// Record failure diagnostic.
        record: String,
    },
}

/// One-shot proof that the prepared operation has durable authority.
pub struct ToolOperationAuthority {
    stream: StreamId,
    revision: StreamRevision,
    prepared_event_id: EventId,
    used_attempt_ids: HashSet<String>,
    operation: PreparedToolOperation,
}

/// One-shot proof that external invocation was durably marked started.
pub struct StartedToolOperation {
    stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
    attempt_id: AttemptId,
    operation: PreparedToolOperation,
}

impl StartedToolOperation {
    /// Returns the logical operation identity for diagnostics and reconciliation.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation.operation_id
    }

    /// Returns the identity of this concrete invocation attempt.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }
}

/// Fully recorded operation outcome.
#[derive(Debug)]
pub enum ToolOperationCompletion {
    /// Canonical result bytes were archived and cited.
    Completed {
        /// Concrete invocation that produced the result.
        attempt_id: AttemptId,
        /// Typed result identity.
        result_id: ContentId<OperationResult>,
    },
    /// Gateway proved execution never began.
    NotStarted {
        /// Concrete invocation that did not begin.
        attempt_id: AttemptId,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
    /// Gateway definitively rejected the operation.
    Rejected {
        /// Concrete invocation that was rejected.
        attempt_id: AttemptId,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
    /// External outcome is unknown; recovery follows the declared effect class.
    Ambiguous {
        /// Concrete invocation whose outcome is unknown.
        attempt_id: AttemptId,
        /// Typed recovery policy.
        recovery: OperationRecovery,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedPayload {
    operation_id: OperationId,
    source_tool_call_id: Option<ToolCallId>,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "persisted identity fields intentionally use explicit _id suffixes"
)]
struct StartedPayload {
    operation_id: OperationId,
    attempt_id: AttemptId,
    arguments_id: ContentId<ToolArguments>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OutcomePayload {
    operation_id: OperationId,
    attempt_id: AttemptId,
    result_id: Option<ContentId<OperationResult>>,
    diagnostic: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetryAuthorizedPayload {
    operation_id: OperationId,
    previous_attempt_id: AttemptId,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "persisted identity fields intentionally use explicit _id suffixes"
)]
struct ReconciledNotOccurredPayload {
    operation_id: OperationId,
    ambiguous_attempt_id: AttemptId,
    evidence_id: ContentId<OperationReconciliationEvidence>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "persisted identity fields intentionally use explicit _id suffixes"
)]
struct ReconciledCompletedPayload {
    operation_id: OperationId,
    ambiguous_attempt_id: AttemptId,
    evidence_id: ContentId<OperationReconciliationEvidence>,
    result_id: ContentId<OperationResult>,
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

/// Commits the prepared fact and grants one-shot operation authority.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when the operation stream or fact cannot be committed.
pub fn authorize_tool_operation<E: EventStore>(
    events: &mut E,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    operation: PreparedToolOperation,
) -> Result<ToolOperationAuthority, OperationCoordinatorError> {
    let stream = operation_stream(operation.operation_id)?;
    let payload = PreparedPayload {
        operation_id: operation.operation_id,
        source_tool_call_id: operation.source_tool_call_id,
        tool: operation.tool.clone(),
        implementation_version: operation.implementation_version.clone(),
        effect: operation.effect,
        arguments_id: operation.arguments_id,
    };
    let outcome = append_fact(
        events,
        &Fact {
            stream: &stream,
            expected: ExpectedRevision::NoStream,
            command_id,
            schema: PREPARED,
            parent_event_id: None,
            observed_at,
            payload: &payload,
        },
    )?;
    Ok(ToolOperationAuthority {
        stream,
        revision: revision(outcome.last_sequence)?,
        prepared_event_id: outcome.event_ids[0],
        used_attempt_ids: HashSet::new(),
        operation,
    })
}

/// Recovers unconsumed durable authority for an already prepared logical operation.
///
/// The supplied operation must exactly match the initial prepared metadata. This lets a caller
/// reconstruct it from the durable step binding without trusting mutable registry state.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when history is invalid or prepared metadata differs.
pub fn recover_tool_operation_authority<E: EventStore>(
    events: &E,
    operation: PreparedToolOperation,
) -> Result<Option<ToolOperationAuthority>, OperationCoordinatorError> {
    let stream = operation_stream(operation.operation_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_operation_details(&history, operation.operation_id)?;
    if !matches!(projection.state, ToolOperationState::Authorized { .. }) {
        return Ok(None);
    }
    into_recovered_authority(stream, &history, projection, operation).map(Some)
}

/// Commits explicit authority to retry the same logical operation.
///
/// A proven-not-started attempt is always retryable. Interrupted or ambiguous attempts are only
/// retryable when their trusted effect class yields [`OperationRecovery::RetrySameOperation`].
/// The subsequent [`begin_tool_operation`] call must supply a fresh [`AttemptId`].
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when retry is unsafe, metadata differs, or commit fails.
pub fn authorize_tool_operation_retry<E: EventStore>(
    events: &mut E,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    operation: PreparedToolOperation,
) -> Result<ToolOperationAuthority, OperationCoordinatorError> {
    let stream = operation_stream(operation.operation_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_operation_details(&history, operation.operation_id)?;
    validate_prepared_operation(projection.prepared.as_ref(), &operation)?;
    if authority_command_replays(&history, &projection.state, command_id, RETRY_AUTHORIZED)? {
        return into_recovered_authority(stream, &history, projection, operation);
    }
    let previous_attempt_id = retryable_attempt(&projection.state)?;
    let last = history
        .last()
        .ok_or_else(|| OperationCoordinatorError::InvalidOperation("operation is empty".into()))?;
    let payload = RetryAuthorizedPayload {
        operation_id: operation.operation_id,
        previous_attempt_id,
    };
    let outcome = append_fact(
        events,
        &Fact {
            stream: &stream,
            expected: ExpectedRevision::Exact(revision(last.sequence)?),
            command_id,
            schema: RETRY_AUTHORIZED,
            parent_event_id: Some(last.event_id),
            observed_at,
            payload: &payload,
        },
    )?;
    Ok(ToolOperationAuthority {
        stream,
        revision: revision(outcome.last_sequence)?,
        prepared_event_id: outcome.event_ids[0],
        used_attempt_ids: projection.used_attempt_ids,
        operation,
    })
}

/// Records evidence that a reconcile-required attempt did not occur and grants retry authority.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when evidence is unavailable, reconciliation is not
/// required, metadata differs, or commit fails.
pub fn reconcile_tool_operation_not_occurred<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    operation: PreparedToolOperation,
    evidence_id: ContentId<OperationReconciliationEvidence>,
) -> Result<ToolOperationAuthority, OperationCoordinatorError> {
    verify_reconciliation_evidence(content, evidence_id)?;
    let stream = operation_stream(operation.operation_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_operation_details(&history, operation.operation_id)?;
    validate_prepared_operation(projection.prepared.as_ref(), &operation)?;
    if authority_command_replays(
        &history,
        &projection.state,
        command_id,
        RECONCILED_NOT_OCCURRED,
    )? {
        let event = history.last().ok_or_else(|| {
            OperationCoordinatorError::InvalidOperation("operation is empty".into())
        })?;
        let last: ReconciledNotOccurredPayload = decode_payload(event)?;
        if last.evidence_id != evidence_id {
            return invalid_operation("replayed reconciliation cites different evidence");
        }
        return into_recovered_authority(stream, &history, projection, operation);
    }
    let ambiguous_attempt_id = reconcilable_attempt(&projection.state)?;
    let last = history
        .last()
        .ok_or_else(|| OperationCoordinatorError::InvalidOperation("operation is empty".into()))?;
    let payload = ReconciledNotOccurredPayload {
        operation_id: operation.operation_id,
        ambiguous_attempt_id,
        evidence_id,
    };
    let outcome = append_fact(
        events,
        &Fact {
            stream: &stream,
            expected: ExpectedRevision::Exact(revision(last.sequence)?),
            command_id,
            schema: RECONCILED_NOT_OCCURRED,
            parent_event_id: Some(last.event_id),
            observed_at,
            payload: &payload,
        },
    )?;
    Ok(ToolOperationAuthority {
        stream,
        revision: revision(outcome.last_sequence)?,
        prepared_event_id: outcome.event_ids[0],
        used_attempt_ids: projection.used_attempt_ids,
        operation,
    })
}

/// Records evidence that a reconcile-required attempt completed and publishes its canonical result.
///
/// No gateway is invoked again. The original ambiguous attempt remains the attempt that produced
/// the externally reconciled result.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when evidence/result archival, validation, or commit fails.
pub fn reconcile_tool_operation_completed<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    operation: &PreparedToolOperation,
    evidence_id: ContentId<OperationReconciliationEvidence>,
    result: &CanonicalToolResult,
) -> Result<ToolOperationCompletion, OperationCoordinatorError> {
    verify_reconciliation_evidence(content, evidence_id)?;
    let expected_result_id = ContentId::<OperationResult>::derive(result.as_bytes())
        .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?;
    let stream = operation_stream(operation.operation_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_operation_details(&history, operation.operation_id)?;
    validate_prepared_operation(projection.prepared.as_ref(), operation)?;
    if let Some(event) = command_event(&history, command_id, RECONCILED_COMPLETED) {
        if history.last().map(|last| last.event_id) != Some(event.event_id) {
            return invalid_operation("replayed reconciliation is not the terminal operation fact");
        }
        let last: ReconciledCompletedPayload = decode_payload(event)?;
        if last.evidence_id != evidence_id || last.result_id != expected_result_id {
            return invalid_operation("replayed reconciliation cites different evidence or result");
        }
        return Ok(ToolOperationCompletion::Completed {
            attempt_id: last.ambiguous_attempt_id,
            result_id: last.result_id,
        });
    }
    let ambiguous_attempt_id = reconcilable_attempt(&projection.state)?;
    let last = history
        .last()
        .ok_or_else(|| OperationCoordinatorError::InvalidOperation("operation is empty".into()))?;
    let descriptor = content.put::<OperationResult>(&mut Cursor::new(result.as_bytes()))?;
    let payload = ReconciledCompletedPayload {
        operation_id: operation.operation_id,
        ambiguous_attempt_id,
        evidence_id,
        result_id: descriptor.content_id,
    };
    append_fact(
        events,
        &Fact {
            stream: &stream,
            expected: ExpectedRevision::Exact(revision(last.sequence)?),
            command_id,
            schema: RECONCILED_COMPLETED,
            parent_event_id: Some(last.event_id),
            observed_at,
            payload: &payload,
        },
    )
    .map_err(|record| OperationCoordinatorError::UnrecordedResult {
        operation_id: operation.operation_id,
        result_id: descriptor.content_id,
        record: record.to_string(),
    })?;
    Ok(ToolOperationCompletion::Completed {
        attempt_id: ambiguous_attempt_id,
        result_id: descriptor.content_id,
    })
}

fn verify_reconciliation_evidence<C: ContentStore>(
    content: &C,
    evidence_id: ContentId<OperationReconciliationEvidence>,
) -> Result<(), OperationCoordinatorError> {
    content.write_to(&evidence_id, &mut std::io::sink())?;
    Ok(())
}

fn authority_command_replays(
    history: &[EventEnvelope],
    state: &ToolOperationState,
    command_id: &CommandId,
    schema: &str,
) -> Result<bool, OperationCoordinatorError> {
    let Some(event) = command_event(history, command_id, schema) else {
        return Ok(false);
    };
    if history.last().map(|last| last.event_id) == Some(event.event_id)
        && matches!(state, ToolOperationState::Authorized { .. })
    {
        Ok(true)
    } else {
        invalid_operation("replayed operation authority was already consumed")
    }
}

fn command_event<'a>(
    history: &'a [EventEnvelope],
    command_id: &CommandId,
    schema: &str,
) -> Option<&'a EventEnvelope> {
    history
        .iter()
        .find(|event| event.command_id == *command_id && event.schema_name.as_str() == schema)
}

fn into_recovered_authority(
    stream: StreamId,
    history: &[EventEnvelope],
    projection: OperationProjection,
    operation: PreparedToolOperation,
) -> Result<ToolOperationAuthority, OperationCoordinatorError> {
    validate_prepared_operation(projection.prepared.as_ref(), &operation)?;
    if !matches!(projection.state, ToolOperationState::Authorized { .. }) {
        return invalid_operation("operation has no unconsumed authority");
    }
    let last = history
        .last()
        .ok_or_else(|| OperationCoordinatorError::InvalidOperation("operation is empty".into()))?;
    Ok(ToolOperationAuthority {
        stream,
        revision: revision(last.sequence)?,
        prepared_event_id: projection.authority_event_id.ok_or_else(|| {
            OperationCoordinatorError::InvalidOperation(
                "authorized operation has no authority event".into(),
            )
        })?,
        used_attempt_ids: projection.used_attempt_ids,
        operation,
    })
}

fn validate_prepared_operation(
    prepared: Option<&PreparedPayload>,
    operation: &PreparedToolOperation,
) -> Result<(), OperationCoordinatorError> {
    let Some(prepared) = prepared else {
        return invalid_operation("operation has no prepared metadata");
    };
    if prepared.operation_id == operation.operation_id
        && prepared.source_tool_call_id == operation.source_tool_call_id
        && prepared.tool == operation.tool
        && prepared.implementation_version == operation.implementation_version
        && prepared.effect == operation.effect
        && prepared.arguments_id == operation.arguments_id
    {
        Ok(())
    } else {
        invalid_operation("prepared operation differs from durable metadata")
    }
}

/// Commits the started fact before a gateway can be invoked.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when the started fact cannot be committed.
pub fn begin_tool_operation<E: EventStore>(
    events: &mut E,
    authority: ToolOperationAuthority,
    attempt_id: AttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StartedToolOperation, OperationCoordinatorError> {
    let ToolOperationAuthority {
        stream,
        revision: current_revision,
        prepared_event_id,
        mut used_attempt_ids,
        operation,
    } = authority;
    if !used_attempt_ids.insert(attempt_id.to_string()) {
        return invalid_operation("attempt identity was already used by this operation");
    }
    let payload = StartedPayload {
        operation_id: operation.operation_id,
        attempt_id,
        arguments_id: operation.arguments_id,
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
    Ok(StartedToolOperation {
        stream,
        revision: revision(outcome.last_sequence)?,
        started_event_id: outcome.event_ids[0],
        attempt_id,
        operation,
    })
}

/// Invokes a gateway exactly once, archives its result, and records a terminal fact.
///
/// ```compile_fail
/// use cairn_agent::{StartedToolOperation, ToolGateway, execute_tool_operation};
/// use cairn_protocol::{CommandId, ObservedAtUnixMillis};
/// use cairn_record::{ContentStore, EventStore};
///
/// fn invoke_twice<E, C, G>(
///     events: &mut E,
///     content: &mut C,
///     gateway: &mut G,
///     started: StartedToolOperation,
///     command: &CommandId,
/// ) where
///     E: EventStore,
///     C: ContentStore,
///     G: ToolGateway,
/// {
///     let _ = execute_tool_operation(
///         events, content, gateway, started, command, ObservedAtUnixMillis::new(1),
///     );
///     let _ = execute_tool_operation(
///         events, content, gateway, started, command, ObservedAtUnixMillis::new(2),
///     );
/// }
/// ```
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when archival or terminal recording fails.
pub fn execute_tool_operation<E: EventStore, C: ContentStore, G: ToolGateway>(
    events: &mut E,
    content: &mut C,
    gateway: &mut G,
    started: StartedToolOperation,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ToolOperationCompletion, OperationCoordinatorError> {
    let StartedToolOperation {
        stream,
        revision,
        started_event_id,
        attempt_id,
        operation,
    } = started;
    let terminal = TerminalContext {
        stream,
        revision,
        started_event_id,
    };
    match gateway.invoke(&operation) {
        Ok(result) => {
            let descriptor = content.put::<OperationResult>(&mut Cursor::new(result.as_bytes()))?;
            let payload = OutcomePayload {
                operation_id: operation.operation_id,
                attempt_id,
                result_id: Some(descriptor.content_id),
                diagnostic: None,
            };
            append_terminal(
                events,
                &terminal,
                command_id,
                observed_at,
                COMPLETED,
                &payload,
            )
            .map_err(|record| OperationCoordinatorError::UnrecordedResult {
                operation_id: operation.operation_id,
                result_id: descriptor.content_id,
                record: record.to_string(),
            })?;
            Ok(ToolOperationCompletion::Completed {
                attempt_id,
                result_id: descriptor.content_id,
            })
        }
        Err(error) => {
            let class = error.failure_class();
            let schema = match class {
                ToolGatewayFailureClass::NotStarted => NOT_STARTED,
                ToolGatewayFailureClass::Rejected => REJECTED,
                ToolGatewayFailureClass::Ambiguous => AMBIGUOUS,
            };
            let payload = OutcomePayload {
                operation_id: operation.operation_id,
                attempt_id,
                result_id: None,
                diagnostic: Some(error.to_string()),
            };
            append_terminal(events, &terminal, command_id, observed_at, schema, &payload).map_err(
                |record| OperationCoordinatorError::UnrecordedGatewayFailure {
                    operation_id: operation.operation_id,
                    gateway: error.to_string(),
                    record: record.to_string(),
                },
            )?;
            let diagnostic = error.to_string();
            Ok(match class {
                ToolGatewayFailureClass::NotStarted => ToolOperationCompletion::NotStarted {
                    attempt_id,
                    diagnostic,
                },
                ToolGatewayFailureClass::Rejected => ToolOperationCompletion::Rejected {
                    attempt_id,
                    diagnostic,
                },
                ToolGatewayFailureClass::Ambiguous => ToolOperationCompletion::Ambiguous {
                    attempt_id,
                    recovery: operation.effect.recovery(),
                    diagnostic,
                },
            })
        }
    }
}

struct TerminalContext {
    stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
}

fn append_terminal<E: EventStore>(
    events: &mut E,
    terminal: &TerminalContext,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &OutcomePayload,
) -> Result<(), OperationCoordinatorError> {
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
    )?;
    Ok(())
}

fn append_fact<E: EventStore, P: Serialize>(
    events: &mut E,
    fact: &Fact<'_, P>,
) -> Result<cairn_record::AppendOutcome, OperationCoordinatorError> {
    let payload = cairn_codec::to_vec(fact.payload)
        .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?;
    let event = NewEvent {
        schema_name: SchemaName::new(fact.schema)
            .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?,
        parent_event_id: fact.parent_event_id,
        observed_at_unix_ms: fact.observed_at.get(),
        payload,
    };
    events
        .append(fact.stream, fact.expected, fact.command_id, &[event])
        .map_err(Into::into)
}

fn operation_stream(operation_id: OperationId) -> Result<StreamId, OperationCoordinatorError> {
    Ok(StreamId {
        kind: AggregateKind::new("tool-operation")
            .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?,
        id: AggregateId::new(operation_id.to_string())
            .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))?,
    })
}

fn revision(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, OperationCoordinatorError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))
}

/// Durable projection of one tool operation.
#[derive(Debug, Eq, PartialEq)]
pub enum ToolOperationState {
    /// No committed fact exists.
    NotFound,
    /// Durable authority exists but invocation was not marked started.
    Authorized {
        /// Declared effect semantics.
        effect: ToolEffectClass,
    },
    /// Invocation was started with no terminal fact, for example after process loss.
    Interrupted {
        /// Concrete invocation with no terminal fact.
        attempt_id: AttemptId,
        /// Declared effect semantics.
        effect: ToolEffectClass,
        /// Safe next action derived from the effect class.
        recovery: OperationRecovery,
    },
    /// Canonical result bytes were archived.
    Completed {
        /// Concrete invocation that produced the result.
        attempt_id: AttemptId,
        /// Typed result identity.
        result_id: ContentId<OperationResult>,
    },
    /// Gateway proved the implementation never began.
    NotStarted {
        /// Concrete invocation that did not begin.
        attempt_id: AttemptId,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
    /// Gateway definitively rejected the operation.
    Rejected {
        /// Concrete invocation that was rejected.
        attempt_id: AttemptId,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
    /// Gateway explicitly reported an unknown external outcome.
    Ambiguous {
        /// Concrete invocation whose outcome is unknown.
        attempt_id: AttemptId,
        /// Safe next action derived from the effect class.
        recovery: OperationRecovery,
        /// Durable gateway diagnostic.
        diagnostic: String,
    },
}

/// Loads and validates the complete durable state for one operation.
///
/// # Errors
///
/// Returns [`OperationCoordinatorError`] when storage fails or the causal state history is invalid.
pub fn recover_tool_operation<E: EventStore>(
    events: &E,
    operation_id: OperationId,
) -> Result<ToolOperationState, OperationCoordinatorError> {
    let stream = operation_stream(operation_id)?;
    let history = events.read_stream(&stream, None)?;
    Ok(project_operation_details(&history, operation_id)?.state)
}

#[cfg(test)]
fn project_operation(
    history: &[EventEnvelope],
    operation_id: OperationId,
) -> Result<ToolOperationState, OperationCoordinatorError> {
    Ok(project_operation_details(history, operation_id)?.state)
}

struct OperationProjection {
    state: ToolOperationState,
    prepared: Option<PreparedPayload>,
    authority_event_id: Option<EventId>,
    used_attempt_ids: HashSet<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "the linear projector keeps persisted causal transitions auditable in one place"
)]
fn project_operation_details(
    history: &[EventEnvelope],
    operation_id: OperationId,
) -> Result<OperationProjection, OperationCoordinatorError> {
    let mut state = ToolOperationState::NotFound;
    let mut prepared: Option<PreparedPayload> = None;
    let mut authority_event_id = None;
    let mut started_event_id = None;
    let mut started_attempt_id = None;
    let mut used_attempt_ids = HashSet::new();
    let mut last_event_id = None;
    for event in history {
        let schema = event.schema_name.as_str();
        if schema == PREPARED {
            let payload: PreparedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            transition_is(&state, &ToolOperationState::NotFound, PREPARED)?;
            if event.parent_event_id.is_some() {
                return invalid_operation("prepared event must not have a causal parent");
            }
            state = ToolOperationState::Authorized {
                effect: payload.effect,
            };
            authority_event_id = Some(event.event_id);
            prepared = Some(payload);
        } else if schema == STARTED {
            let payload: StartedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            let Some(prepared) = prepared.as_ref() else {
                return invalid_operation("started event has no prepared effect class");
            };
            transition_is(
                &state,
                &ToolOperationState::Authorized {
                    effect: prepared.effect,
                },
                STARTED,
            )?;
            if payload.arguments_id != prepared.arguments_id {
                return invalid_operation("started event cites different arguments");
            }
            if event.parent_event_id != authority_event_id {
                return invalid_operation("started event does not cite its authority event");
            }
            if !used_attempt_ids.insert(payload.attempt_id.to_string()) {
                return invalid_operation("attempt identity is reused within one operation");
            }
            started_event_id = Some(event.event_id);
            started_attempt_id = Some(payload.attempt_id);
            state = ToolOperationState::Interrupted {
                attempt_id: payload.attempt_id,
                effect: prepared.effect,
                recovery: prepared.effect.recovery(),
            };
        } else if [COMPLETED, NOT_STARTED, REJECTED, AMBIGUOUS].contains(&schema) {
            let effect = prepared.as_ref().map(|payload| payload.effect);
            state = project_terminal(
                event,
                schema,
                operation_id,
                &state,
                effect,
                started_attempt_id,
                started_event_id,
            )?;
        } else if schema == RETRY_AUTHORIZED {
            let payload: RetryAuthorizedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            if payload.previous_attempt_id != retryable_attempt(&state)? {
                return invalid_operation("retry fact cites a different previous attempt");
            }
            require_parent(event, last_event_id, "retry authority")?;
            let effect = prepared_effect(prepared.as_ref())?;
            state = ToolOperationState::Authorized { effect };
            authority_event_id = Some(event.event_id);
            started_event_id = None;
            started_attempt_id = None;
        } else if schema == RECONCILED_NOT_OCCURRED {
            let payload: ReconciledNotOccurredPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            if payload.ambiguous_attempt_id != reconcilable_attempt(&state)? {
                return invalid_operation("reconciliation cites a different ambiguous attempt");
            }
            require_parent(event, last_event_id, "reconciliation")?;
            let effect = prepared_effect(prepared.as_ref())?;
            state = ToolOperationState::Authorized { effect };
            authority_event_id = Some(event.event_id);
            started_event_id = None;
            started_attempt_id = None;
        } else if schema == RECONCILED_COMPLETED {
            let payload: ReconciledCompletedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            if payload.ambiguous_attempt_id != reconcilable_attempt(&state)? {
                return invalid_operation("reconciliation cites a different ambiguous attempt");
            }
            require_parent(event, last_event_id, "reconciliation")?;
            state = ToolOperationState::Completed {
                attempt_id: payload.ambiguous_attempt_id,
                result_id: payload.result_id,
            };
        } else {
            return invalid_operation("unsupported tool-operation event schema");
        }
        last_event_id = Some(event.event_id);
    }
    Ok(OperationProjection {
        state,
        prepared,
        authority_event_id,
        used_attempt_ids,
    })
}

fn prepared_effect(
    prepared: Option<&PreparedPayload>,
) -> Result<ToolEffectClass, OperationCoordinatorError> {
    prepared.map(|payload| payload.effect).ok_or_else(|| {
        OperationCoordinatorError::InvalidOperation(
            "operation fact has no prepared metadata".to_owned(),
        )
    })
}

fn retryable_attempt(state: &ToolOperationState) -> Result<AttemptId, OperationCoordinatorError> {
    match state {
        ToolOperationState::NotStarted { attempt_id, .. }
        | ToolOperationState::Interrupted {
            attempt_id,
            recovery: OperationRecovery::RetrySameOperation,
            ..
        }
        | ToolOperationState::Ambiguous {
            attempt_id,
            recovery: OperationRecovery::RetrySameOperation,
            ..
        } => Ok(*attempt_id),
        _ => invalid_operation("operation state does not permit retry"),
    }
}

fn reconcilable_attempt(
    state: &ToolOperationState,
) -> Result<AttemptId, OperationCoordinatorError> {
    match state {
        ToolOperationState::Interrupted {
            attempt_id,
            recovery: OperationRecovery::ReconcileRequired,
            ..
        }
        | ToolOperationState::Ambiguous {
            attempt_id,
            recovery: OperationRecovery::ReconcileRequired,
            ..
        } => Ok(*attempt_id),
        _ => invalid_operation("operation state does not require reconciliation"),
    }
}

fn require_parent(
    event: &EventEnvelope,
    expected: Option<EventId>,
    fact: &str,
) -> Result<(), OperationCoordinatorError> {
    if event.parent_event_id == expected && expected.is_some() {
        Ok(())
    } else {
        invalid_operation(&format!("{fact} does not cite the previous operation fact"))
    }
}

fn project_terminal(
    event: &EventEnvelope,
    schema: &str,
    operation_id: OperationId,
    state: &ToolOperationState,
    effect: Option<ToolEffectClass>,
    started_attempt_id: Option<AttemptId>,
    started_event_id: Option<EventId>,
) -> Result<ToolOperationState, OperationCoordinatorError> {
    let payload: OutcomePayload = decode_payload(event)?;
    require_operation(payload.operation_id, operation_id)?;
    let Some(effect) = effect else {
        return invalid_operation("terminal event has no prepared effect class");
    };
    let Some(attempt_id) = started_attempt_id else {
        return invalid_operation("terminal event has no started attempt identity");
    };
    if payload.attempt_id != attempt_id {
        return invalid_operation("terminal event cites a different attempt");
    }
    transition_is(
        state,
        &ToolOperationState::Interrupted {
            attempt_id,
            effect,
            recovery: effect.recovery(),
        },
        schema,
    )?;
    if event.parent_event_id != started_event_id {
        return invalid_operation("terminal event does not cite its started event");
    }
    match schema {
        COMPLETED if payload.diagnostic.is_some() => {
            invalid_operation("completed event unexpectedly has a failure diagnostic")
        }
        COMPLETED => Ok(ToolOperationState::Completed {
            attempt_id,
            result_id: payload.result_id.ok_or_else(|| {
                OperationCoordinatorError::InvalidOperation(
                    "completed event lacks result identity".to_owned(),
                )
            })?,
        }),
        NOT_STARTED | REJECTED | AMBIGUOUS if payload.result_id.is_some() => {
            invalid_operation("failure event unexpectedly cites a result")
        }
        NOT_STARTED | REJECTED | AMBIGUOUS if payload.diagnostic.is_none() => {
            invalid_operation("failure event lacks a diagnostic")
        }
        NOT_STARTED => Ok(ToolOperationState::NotStarted {
            attempt_id,
            diagnostic: payload.diagnostic.expect("checked above"),
        }),
        REJECTED => Ok(ToolOperationState::Rejected {
            attempt_id,
            diagnostic: payload.diagnostic.expect("checked above"),
        }),
        AMBIGUOUS => Ok(ToolOperationState::Ambiguous {
            attempt_id,
            recovery: effect.recovery(),
            diagnostic: payload.diagnostic.expect("checked above"),
        }),
        _ => unreachable!("schema was filtered by the caller"),
    }
}

fn decode_payload<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, OperationCoordinatorError> {
    if event.schema_version.get() != 1 {
        return invalid_operation("unsupported tool-operation event schema version");
    }
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| OperationCoordinatorError::InvalidOperation(error.to_string()))
}

fn require_operation(
    actual: OperationId,
    expected: OperationId,
) -> Result<(), OperationCoordinatorError> {
    if actual == expected {
        Ok(())
    } else {
        invalid_operation("event operation identity does not match its stream")
    }
}

fn transition_is(
    actual: &ToolOperationState,
    expected: &ToolOperationState,
    schema: &str,
) -> Result<(), OperationCoordinatorError> {
    if actual == expected {
        Ok(())
    } else {
        invalid_operation(&format!(
            "event {schema} follows state {actual:?}, expected {expected:?}"
        ))
    }
}

fn invalid_operation<T>(message: &str) -> Result<T, OperationCoordinatorError> {
    Err(OperationCoordinatorError::InvalidOperation(
        message.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{AttemptId, CommandId, ContentId, OperationId};
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        CanonicalToolResult, OperationRecovery, PreparedToolOperation, RecordedToolExchange,
        RecordedToolGateway, ScriptedToolGateway, ToolEffectClass, ToolGatewayError,
        ToolOperationCompletion, ToolOperationState, authorize_tool_operation,
        authorize_tool_operation_retry, begin_tool_operation, execute_tool_operation,
        operation_stream, prepare_tool_operation, project_operation,
        reconcile_tool_operation_completed, reconcile_tool_operation_not_occurred,
        recover_tool_operation, recover_tool_operation_authority,
    };
    use crate::{OperationReconciliationEvidence, ToolImplementationVersion, ToolName};

    fn content_store(directory: &tempfile::TempDir) -> SqliteContentStore {
        SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store")
    }

    fn prepare(
        content: &mut SqliteContentStore,
        operation_id: OperationId,
        effect: ToolEffectClass,
    ) -> PreparedToolOperation {
        prepare_tool_operation(
            content,
            operation_id,
            ToolName::new("read_source").expect("tool"),
            ToolImplementationVersion::new("v1").expect("version"),
            effect,
            &serde_json::json!({"path":"src/main.rs"}),
        )
        .expect("prepare")
    }

    fn reconciliation_evidence(
        content: &mut SqliteContentStore,
        conclusion: &str,
    ) -> ContentId<OperationReconciliationEvidence> {
        let bytes = cairn_codec::to_vec(&serde_json::json!({
            "conclusion": conclusion,
            "source": "trusted-test-probe"
        }))
        .expect("evidence bytes");
        content
            .put::<OperationReconciliationEvidence>(&mut Cursor::new(bytes))
            .expect("archive evidence")
            .content_id
    }

    fn ambiguous_operation(
        content: &mut SqliteContentStore,
        events: &mut SqliteEventStore,
        effect: ToolEffectClass,
    ) -> (OperationId, PreparedToolOperation, AttemptId) {
        let operation_id = OperationId::new();
        let operation = prepare(content, operation_id, effect);
        let authority = authorize_tool_operation(
            events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
            operation.clone(),
        )
        .expect("authorize");
        let attempt_id = AttemptId::new();
        let started = begin_tool_operation(
            events,
            authority,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut gateway = ScriptedToolGateway::new(|_: &PreparedToolOperation| {
            Err(ToolGatewayError::Ambiguous("connection lost".to_owned()))
        });
        execute_tool_operation(
            events,
            content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("record ambiguity");
        (operation_id, operation, attempt_id)
    }

    #[test]
    fn canonical_result_rejects_noncanonical_or_ambiguous_json() {
        assert!(CanonicalToolResult::from_bytes(b"{ \"ok\": true }".to_vec()).is_err());
        assert!(CanonicalToolResult::from_value(&serde_json::json!({"value": 1.5})).is_err());
        assert!(CanonicalToolResult::from_bytes(b"{\"ok\":true}".to_vec()).is_ok());
    }

    #[test]
    fn completed_result_is_archived_and_recoverable_with_recorded_gateway() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let operation_id = OperationId::new();
        let operation = prepare(&mut content, operation_id, ToolEffectClass::ReadOnly);
        let arguments_id = operation.arguments_id();
        let authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
            operation,
        )
        .expect("authorize");
        let started = begin_tool_operation(
            &mut events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let result = CanonicalToolResult::from_value(&serde_json::json!({
            "contents":"fn main() {}"
        }))
        .expect("result");
        let expected_bytes = result.as_bytes().to_vec();
        let mut gateway = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id,
            result,
        }]);
        let completion = execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("execute");
        let ToolOperationCompletion::Completed {
            attempt_id,
            result_id,
        } = completion
        else {
            panic!("expected completed result");
        };
        let mut archived = Vec::new();
        content
            .write_to(&result_id, &mut archived)
            .expect("read archived result");
        assert_eq!(archived, expected_bytes);
        assert_eq!(
            recover_tool_operation(&events, operation_id).expect("recover"),
            ToolOperationState::Completed {
                attempt_id,
                result_id
            }
        );
        let stream = operation_stream(operation_id).expect("stream");
        let mut corrupted = events.read_stream(&stream, None).expect("history");
        let mut terminal: super::OutcomePayload =
            cairn_codec::from_slice(&corrupted[2].payload).expect("terminal payload");
        terminal.attempt_id = AttemptId::new();
        corrupted[2].payload = cairn_codec::to_vec(&terminal).expect("encode corruption");
        assert!(project_operation(&corrupted, operation_id).is_err());
    }

    #[test]
    fn interrupted_recovery_is_derived_from_effect_class() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        for (effect, expected_recovery) in [
            (
                ToolEffectClass::ReadOnly,
                OperationRecovery::RetrySameOperation,
            ),
            (
                ToolEffectClass::AtMostOnce,
                OperationRecovery::ReconcileRequired,
            ),
        ] {
            let operation_id = OperationId::new();
            let operation = prepare(&mut content, operation_id, effect);
            let authority = authorize_tool_operation(
                &mut events,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(1),
                operation,
            )
            .expect("authorize");
            let _started = begin_tool_operation(
                &mut events,
                authority,
                AttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(2),
            )
            .expect("begin");
            assert!(matches!(
                recover_tool_operation(&events, operation_id).expect("recover"),
                ToolOperationState::Interrupted {
                    effect: actual_effect,
                    recovery: actual_recovery,
                    ..
                } if actual_effect == effect && actual_recovery == expected_recovery
            ));
        }
    }

    #[test]
    fn ambiguous_at_most_once_operation_requires_reconciliation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let operation_id = OperationId::new();
        let operation = prepare(&mut content, operation_id, ToolEffectClass::AtMostOnce);
        let authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
            operation,
        )
        .expect("authorize");
        let started = begin_tool_operation(
            &mut events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut gateway = ScriptedToolGateway::new(|_: &PreparedToolOperation| {
            Err(ToolGatewayError::Ambiguous("connection lost".to_owned()))
        });
        let completion = execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("record ambiguity");
        assert!(matches!(
            completion,
            ToolOperationCompletion::Ambiguous {
                recovery: OperationRecovery::ReconcileRequired,
                ..
            }
        ));
        assert!(matches!(
            recover_tool_operation(&events, operation_id).expect("recover"),
            ToolOperationState::Ambiguous {
                recovery: OperationRecovery::ReconcileRequired,
                ..
            }
        ));
    }

    #[test]
    fn safe_retry_preserves_operation_and_rejects_attempt_identity_reuse() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let (operation_id, operation, first_attempt_id) =
            ambiguous_operation(&mut content, &mut events, ToolEffectClass::ReadOnly);
        let retry_command = CommandId::new();
        let _lost = authorize_tool_operation_retry(
            &mut events,
            &retry_command,
            cairn_protocol::ObservedAtUnixMillis::new(4),
            operation.clone(),
        )
        .expect("authorize retry");
        let replayed = authorize_tool_operation_retry(
            &mut events,
            &retry_command,
            cairn_protocol::ObservedAtUnixMillis::new(4),
            operation.clone(),
        )
        .expect("replay retry authority");
        assert!(
            begin_tool_operation(
                &mut events,
                replayed,
                first_attempt_id,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(5),
            )
            .is_err()
        );
        let recovered = recover_tool_operation_authority(&events, operation.clone())
            .expect("recover authority")
            .expect("retry authority");
        let second_attempt_id = AttemptId::new();
        let started = begin_tool_operation(
            &mut events,
            recovered,
            second_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(5),
        )
        .expect("begin retry");
        assert!(
            authorize_tool_operation_retry(
                &mut events,
                &retry_command,
                cairn_protocol::ObservedAtUnixMillis::new(4),
                operation.clone(),
            )
            .is_err()
        );
        let mut gateway = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id: operation.arguments_id(),
            result: CanonicalToolResult::from_value(&serde_json::json!({"ok":true}))
                .expect("result"),
        }]);
        execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("complete retry");
        assert!(matches!(
            recover_tool_operation(&events, operation_id).expect("recover"),
            ToolOperationState::Completed { attempt_id, .. }
                if attempt_id == second_attempt_id && attempt_id != first_attempt_id
        ));
        let stream = operation_stream(operation_id).expect("stream");
        let history = events.read_stream(&stream, None).expect("history");
        assert_eq!(history.len(), 6);
        let mut broken_retry_parent = history;
        broken_retry_parent[3].parent_event_id = None;
        assert!(project_operation(&broken_retry_parent, operation_id).is_err());
    }

    #[test]
    fn reconciliation_proving_no_effect_grants_a_fresh_attempt() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let (operation_id, operation, first_attempt_id) =
            ambiguous_operation(&mut content, &mut events, ToolEffectClass::AtMostOnce);
        assert!(
            authorize_tool_operation_retry(
                &mut events,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(4),
                operation.clone(),
            )
            .is_err()
        );
        let evidence_id = reconciliation_evidence(&mut content, "not-occurred");
        let authority = reconcile_tool_operation_not_occurred(
            &mut events,
            &content,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(5),
            operation.clone(),
            evidence_id,
        )
        .expect("reconcile not occurred");
        let second_attempt_id = AttemptId::new();
        let started = begin_tool_operation(
            &mut events,
            authority,
            second_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("begin after reconciliation");
        let mut gateway = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id: operation.arguments_id(),
            result: CanonicalToolResult::from_value(&serde_json::json!({"ok":true}))
                .expect("result"),
        }]);
        execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("complete reconciled retry");
        assert!(matches!(
            recover_tool_operation(&events, operation_id).expect("recover"),
            ToolOperationState::Completed { attempt_id, .. }
                if attempt_id == second_attempt_id && attempt_id != first_attempt_id
        ));
    }

    #[test]
    fn reconciliation_can_publish_the_original_attempt_result_without_retry() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let (operation_id, operation, attempt_id) =
            ambiguous_operation(&mut content, &mut events, ToolEffectClass::AtMostOnce);
        let evidence_id = reconciliation_evidence(&mut content, "completed");
        let result = CanonicalToolResult::from_value(&serde_json::json!({"receipt":"remote-1"}))
            .expect("result");
        let command = CommandId::new();
        let ToolOperationCompletion::Completed { result_id, .. } =
            reconcile_tool_operation_completed(
                &mut events,
                &mut content,
                &command,
                cairn_protocol::ObservedAtUnixMillis::new(4),
                &operation,
                evidence_id,
                &result,
            )
            .expect("reconcile completion")
        else {
            panic!("completed reconciliation");
        };
        let replay_result = CanonicalToolResult::from_value(&serde_json::json!({
            "receipt":"remote-1"
        }))
        .expect("replay result");
        reconcile_tool_operation_completed(
            &mut events,
            &mut content,
            &command,
            cairn_protocol::ObservedAtUnixMillis::new(4),
            &operation,
            evidence_id,
            &replay_result,
        )
        .expect("replay reconciliation");
        assert_eq!(
            recover_tool_operation(&events, operation_id).expect("recover"),
            ToolOperationState::Completed {
                attempt_id,
                result_id,
            }
        );
        let stream = operation_stream(operation_id).expect("stream");
        assert_eq!(events.read_stream(&stream, None).expect("history").len(), 4);
    }

    #[test]
    fn repeated_authorization_command_does_not_duplicate_authority() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let operation_id = OperationId::new();
        let operation = prepare(&mut content, operation_id, ToolEffectClass::Pure);
        let command = CommandId::new();
        let observed_at = cairn_protocol::ObservedAtUnixMillis::new(1);
        let _first =
            authorize_tool_operation(&mut events, &command, observed_at, operation.clone())
                .expect("first authorization");
        let _replay = authorize_tool_operation(&mut events, &command, observed_at, operation)
            .expect("replayed authorization");
        let stream = operation_stream(operation_id).expect("stream");
        assert_eq!(events.read_stream(&stream, None).expect("read").len(), 1);
    }

    #[test]
    fn recovery_rejects_broken_operation_causality() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = content_store(&directory);
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let operation_id = OperationId::new();
        let operation = prepare(&mut content, operation_id, ToolEffectClass::ReadOnly);
        let authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
            operation,
        )
        .expect("authorize");
        let _started = begin_tool_operation(
            &mut events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let stream = operation_stream(operation_id).expect("stream");
        let mut history = events.read_stream(&stream, None).expect("read");
        history[1].parent_event_id = None;
        assert!(project_operation(&history, operation_id).is_err());
    }
}
