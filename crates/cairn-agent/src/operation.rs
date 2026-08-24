use std::{collections::VecDeque, io::Cursor};

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

use crate::{OperationResult, ToolArguments, ToolCallId, ToolImplementationVersion, ToolName};

const PREPARED: &str = "agent.tool-operation-prepared";
const STARTED: &str = "agent.tool-operation-started";
const COMPLETED: &str = "agent.tool-operation-completed";
const NOT_STARTED: &str = "agent.tool-operation-not-started";
const REJECTED: &str = "agent.tool-operation-rejected";
const AMBIGUOUS: &str = "agent.tool-operation-ambiguous";

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

#[derive(Deserialize, Serialize)]
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
        operation,
    })
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
        operation,
    } = authority;
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
    project_operation(&history, operation_id)
}

fn project_operation(
    history: &[EventEnvelope],
    operation_id: OperationId,
) -> Result<ToolOperationState, OperationCoordinatorError> {
    let mut state = ToolOperationState::NotFound;
    let mut effect = None;
    let mut arguments_id = None;
    let mut prepared_event_id = None;
    let mut started_event_id = None;
    let mut started_attempt_id = None;
    for event in history {
        let schema = event.schema_name.as_str();
        if schema == PREPARED {
            let payload: PreparedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            transition_is(&state, &ToolOperationState::NotFound, PREPARED)?;
            if event.parent_event_id.is_some() {
                return invalid_operation("prepared event must not have a causal parent");
            }
            effect = Some(payload.effect);
            arguments_id = Some(payload.arguments_id);
            prepared_event_id = Some(event.event_id);
            state = ToolOperationState::Authorized {
                effect: payload.effect,
            };
        } else if schema == STARTED {
            let payload: StartedPayload = decode_payload(event)?;
            require_operation(payload.operation_id, operation_id)?;
            let Some(effect_value) = effect else {
                return invalid_operation("started event has no prepared effect class");
            };
            transition_is(
                &state,
                &ToolOperationState::Authorized {
                    effect: effect_value,
                },
                STARTED,
            )?;
            if Some(payload.arguments_id) != arguments_id {
                return invalid_operation("started event cites different arguments");
            }
            if event.parent_event_id != prepared_event_id {
                return invalid_operation("started event does not cite its prepared event");
            }
            started_event_id = Some(event.event_id);
            started_attempt_id = Some(payload.attempt_id);
            state = ToolOperationState::Interrupted {
                attempt_id: payload.attempt_id,
                effect: effect_value,
                recovery: effect_value.recovery(),
            };
        } else if [COMPLETED, NOT_STARTED, REJECTED, AMBIGUOUS].contains(&schema) {
            state = project_terminal(
                event,
                schema,
                operation_id,
                &state,
                effect,
                started_attempt_id,
                started_event_id,
            )?;
        }
    }
    Ok(state)
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
    use cairn_protocol::{AttemptId, CommandId, OperationId};
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        CanonicalToolResult, OperationRecovery, PreparedToolOperation, RecordedToolExchange,
        RecordedToolGateway, ScriptedToolGateway, ToolEffectClass, ToolGatewayError,
        ToolOperationCompletion, ToolOperationState, authorize_tool_operation,
        begin_tool_operation, execute_tool_operation, operation_stream, prepare_tool_operation,
        project_operation, recover_tool_operation,
    };
    use crate::{ToolImplementationVersion, ToolName};

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
