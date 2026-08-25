use std::{collections::VecDeque, io::Cursor};

use cairn_protocol::{
    CommandId, ContentId, DerivedId, ModelAttemptId, ObservedAtUnixMillis, OperationId, SchemaName,
    SchemaVersion,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AdapterVersion, ModelResponseArtifact, PreparedToolOperation, ProviderToolCallId,
    ReceivedModelResponse, SemanticModelTurnArtifact, ToolArguments, ToolCallIdentity,
    ToolEffectClass, ToolImplementationVersion, ToolName,
};

const RESPONSE_DECODED: &str = "agent.model-response-decoded";
const TOOL_CALL_PROPOSED: &str = "agent.tool-call-proposed";

/// One-based position of an item in a provider-neutral model turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OutputOrdinal(u32);

impl OutputOrdinal {
    /// Returns the one-based wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    fn from_index(index: usize) -> Result<Self, DecodeCoordinatorError> {
        let value = index
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                DecodeCoordinatorError::InvalidSemanticTurn(
                    "model output has too many items".to_owned(),
                )
            })?;
        Ok(Self(value))
    }
}

/// Provider-specific response interpreted at the [`ModelAdapter`] boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterModelTurn {
    /// Ordered semantic output items.
    pub items: Vec<AdapterOutputItem>,
}

/// Provider-neutral item emitted by an adapter before Cairn assigns durable identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterOutputItem {
    /// Assistant-authored text.
    Text {
        /// Exact semantic text after provider decoding.
        text: String,
    },
    /// Proposed invocation; it has no execution authority.
    ToolCall {
        /// Provider-native correlation identity.
        provider_call_id: ProviderToolCallId,
        /// Requested registered tool name.
        tool: ToolName,
        /// Structured arguments, canonicalized by Cairn after decoding.
        arguments: serde_json::Value,
    },
}

/// Pure semantic adapter failure. Retrying it never repeats the provider request.
#[derive(Debug, Error)]
pub enum ModelAdapterError {
    /// Recorded response identity differs from the input.
    #[error("recorded adapter response identity does not match")]
    ResponseMismatch,
    /// No recorded adapter exchange remains.
    #[error("recorded adapter fixture is exhausted")]
    Exhausted,
    /// Provider bytes cannot be interpreted under this adapter version.
    #[error("provider response is invalid: {0}")]
    InvalidResponse(String),
    /// Scripted adapter failed.
    #[error("scripted model adapter failed: {0}")]
    Scripted(String),
}

/// Replaceable pure interpretation of raw provider bytes.
pub trait ModelAdapter {
    /// Returns the pinned semantic adapter version.
    fn adapter_version(&self) -> &AdapterVersion;

    /// Decodes exact raw response bytes into provider-neutral semantics.
    ///
    /// # Errors
    ///
    /// Returns [`ModelAdapterError`] when bytes do not satisfy the pinned adapter contract.
    fn decode(
        &mut self,
        response_id: ContentId<ModelResponseArtifact>,
        response_bytes: &[u8],
    ) -> Result<AdapterModelTurn, ModelAdapterError>;
}

/// One recorded raw-response/semantic-turn exchange.
pub struct RecordedAdapterExchange {
    /// Required raw response identity.
    pub response_id: ContentId<ModelResponseArtifact>,
    /// Previously decoded semantic result.
    pub turn: AdapterModelTurn,
}

/// FIFO recorded semantic adapter.
pub struct RecordedModelAdapter {
    version: AdapterVersion,
    exchanges: VecDeque<RecordedAdapterExchange>,
}

impl RecordedModelAdapter {
    /// Creates an adapter with a pinned version and ordered exchanges.
    pub fn new(
        version: AdapterVersion,
        exchanges: impl IntoIterator<Item = RecordedAdapterExchange>,
    ) -> Self {
        Self {
            version,
            exchanges: exchanges.into_iter().collect(),
        }
    }
}

impl ModelAdapter for RecordedModelAdapter {
    fn adapter_version(&self) -> &AdapterVersion {
        &self.version
    }

    fn decode(
        &mut self,
        response_id: ContentId<ModelResponseArtifact>,
        _response_bytes: &[u8],
    ) -> Result<AdapterModelTurn, ModelAdapterError> {
        let exchange = self
            .exchanges
            .pop_front()
            .ok_or(ModelAdapterError::Exhausted)?;
        if exchange.response_id != response_id {
            return Err(ModelAdapterError::ResponseMismatch);
        }
        Ok(exchange.turn)
    }
}

/// Closure-backed semantic adapter for tests and embedders.
pub struct ScriptedModelAdapter<F> {
    version: AdapterVersion,
    script: F,
}

impl<F> ScriptedModelAdapter<F> {
    /// Creates a scripted adapter with an explicit semantic version.
    pub const fn new(version: AdapterVersion, script: F) -> Self {
        Self { version, script }
    }
}

impl<F> ModelAdapter for ScriptedModelAdapter<F>
where
    F: FnMut(
        ContentId<ModelResponseArtifact>,
        &[u8],
    ) -> Result<AdapterModelTurn, ModelAdapterError>,
{
    fn adapter_version(&self) -> &AdapterVersion {
        &self.version
    }

    fn decode(
        &mut self,
        response_id: ContentId<ModelResponseArtifact>,
        response_bytes: &[u8],
    ) -> Result<AdapterModelTurn, ModelAdapterError> {
        (self.script)(response_id, response_bytes)
    }
}

/// Stable derived identity of one tool-call proposal.
pub type ToolCallId = DerivedId<ToolCallIdentity>;

/// Archived provider-neutral turn schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticModelTurn {
    /// Raw response from which this turn was decoded.
    pub source_response_id: ContentId<ModelResponseArtifact>,
    /// Exact interpretation semantics.
    pub adapter_version: AdapterVersion,
    /// Ordered normalized output.
    pub items: Vec<SemanticOutputItem>,
}

/// Durable semantic turn item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SemanticOutputItem {
    /// Assistant-authored text.
    Text {
        /// Exact semantic text.
        text: String,
    },
    /// Immutable tool-call proposal with no execution authority.
    ToolCall {
        /// Cairn-derived call identity.
        tool_call_id: ToolCallId,
        /// Position in the semantic turn.
        ordinal: OutputOrdinal,
        /// Provider-native correlation identity.
        provider_call_id: ProviderToolCallId,
        /// Requested tool.
        tool: ToolName,
        /// Archived exact canonical arguments.
        arguments_id: ContentId<ToolArguments>,
    },
}

/// Unforgeable decoded tool-call proposal.
///
/// ```compile_fail
/// use cairn_agent::ToolCallProposal;
///
/// let forged = ToolCallProposal {
///     tool_call_id: todo!(),
///     tool: todo!(),
///     arguments_id: todo!(),
///     argument_bytes: Vec::new(),
/// };
/// ```
///
/// ```compile_fail
/// use cairn_agent::ToolCallProposal;
///
/// fn duplicate(proposal: ToolCallProposal) {
///     let _second = proposal.clone();
/// }
/// ```
#[derive(Debug)]
pub struct ToolCallProposal {
    tool_call_id: ToolCallId,
    tool: ToolName,
    arguments_id: ContentId<ToolArguments>,
    argument_bytes: Vec<u8>,
}

impl ToolCallProposal {
    /// Returns the stable derived proposal identity.
    #[must_use]
    pub const fn tool_call_id(&self) -> ToolCallId {
        self.tool_call_id
    }

    /// Returns the requested tool name.
    #[must_use]
    pub fn tool(&self) -> &ToolName {
        &self.tool
    }

    /// Returns the archived argument identity.
    #[must_use]
    pub const fn arguments_id(&self) -> ContentId<ToolArguments> {
        self.arguments_id
    }

    /// Converts a proposal into a prepared operation using trusted registry semantics. This grants
    /// no execution authority; [`crate::authorize_tool_operation`] remains a separate commit.
    #[must_use]
    pub fn into_prepared_operation(
        self,
        operation_id: OperationId,
        implementation_version: ToolImplementationVersion,
        effect: ToolEffectClass,
    ) -> PreparedToolOperation {
        PreparedToolOperation::from_tool_call(
            operation_id,
            self.tool_call_id,
            self.tool,
            implementation_version,
            effect,
            self.arguments_id,
            self.argument_bytes,
        )
    }
}

/// Durable semantic turn plus ordered unforgeable tool-call proposals.
#[derive(Debug)]
pub struct DecodedModelTurn {
    turn_id: ContentId<SemanticModelTurnArtifact>,
    proposals: Vec<ToolCallProposal>,
}

impl DecodedModelTurn {
    /// Returns the archived provider-neutral turn identity.
    #[must_use]
    pub const fn turn_id(&self) -> ContentId<SemanticModelTurnArtifact> {
        self.turn_id
    }

    /// Borrows tool-call proposals in model output order.
    #[must_use]
    pub fn proposals(&self) -> &[ToolCallProposal] {
        &self.proposals
    }

    /// Consumes the decoded turn and returns ordered proposals.
    #[must_use]
    pub fn into_proposals(self) -> Vec<ToolCallProposal> {
        self.proposals
    }
}

/// Failure of semantic decoding and durable proposal publication.
#[derive(Debug, Error)]
pub enum DecodeCoordinatorError {
    /// Reading or archiving content failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Pure provider interpretation failed.
    #[error(transparent)]
    Adapter(#[from] ModelAdapterError),
    /// Durable semantic facts could not be committed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Adapter output cannot be represented by the stable semantic schema.
    #[error("invalid semantic model turn: {0}")]
    InvalidSemanticTurn(String),
    /// Runtime adapter differs from the version pinned before provider dispatch.
    #[error("model adapter version does not match the prepared request")]
    AdapterVersionMismatch {
        /// Version committed before dispatch.
        expected: AdapterVersion,
        /// Runtime adapter offered for decoding.
        actual: AdapterVersion,
    },
    /// Semantic artifacts exist but their fact batch could not be committed.
    #[error(
        "attempt {attempt_id} archived semantic turn {turn_id}, but recording decoded facts failed ({record})"
    )]
    UnrecordedSemanticTurn {
        /// Attempt whose response remains safely re-decodable.
        attempt_id: ModelAttemptId,
        /// Recoverable semantic artifact identity.
        turn_id: ContentId<SemanticModelTurnArtifact>,
        /// Record failure diagnostic.
        record: String,
    },
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolCallIdentityMaterial<'a> {
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    ordinal: OutputOrdinal,
    provider_call_id: &'a ProviderToolCallId,
    tool: &'a ToolName,
    arguments_id: ContentId<ToolArguments>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DecodedPayload {
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    adapter_version: AdapterVersion,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProposedPayload {
    attempt_id: ModelAttemptId,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    tool_call_id: ToolCallId,
    ordinal: OutputOrdinal,
    provider_call_id: ProviderToolCallId,
    tool: ToolName,
    arguments_id: ContentId<ToolArguments>,
}

/// Purely decodes one durable raw response, archives the semantic turn, and atomically publishes
/// its decoded fact plus every ordered tool-call proposal.
///
/// # Errors
///
/// Returns [`DecodeCoordinatorError`] when raw bytes are unavailable, adapter interpretation fails,
/// semantic artifacts cannot be archived, or the complete fact batch cannot commit.
pub fn decode_model_response<E: EventStore, C: ContentStore, A: ModelAdapter>(
    events: &mut E,
    content: &mut C,
    adapter: &mut A,
    received: ReceivedModelResponse,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<DecodedModelTurn, DecodeCoordinatorError> {
    let ReceivedModelResponse {
        attempt_id,
        stream,
        revision,
        response_event_id,
        response_id,
        adapter_version: expected_adapter_version,
        usage: _,
    } = received;
    let actual_adapter_version = adapter.adapter_version().clone();
    if actual_adapter_version != expected_adapter_version {
        return Err(DecodeCoordinatorError::AdapterVersionMismatch {
            expected: expected_adapter_version,
            actual: actual_adapter_version,
        });
    }
    let mut response_bytes = Vec::new();
    content.write_to(&response_id, &mut response_bytes)?;
    let adapter_turn = adapter.decode(response_id, &response_bytes)?;
    let (semantic_turn, proposals) = materialize_turn(
        content,
        attempt_id,
        response_id,
        expected_adapter_version,
        adapter_turn,
    )?;
    let turn_bytes = cairn_codec::to_vec(&semantic_turn)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?;
    let turn_descriptor = content.put::<SemanticModelTurnArtifact>(&mut Cursor::new(turn_bytes))?;
    let facts = semantic_facts(
        attempt_id,
        response_id,
        response_event_id,
        turn_descriptor.content_id,
        observed_at,
        &semantic_turn,
    )?;
    events
        .append(
            &stream,
            ExpectedRevision::Exact(revision),
            command_id,
            &facts,
        )
        .map_err(|record| DecodeCoordinatorError::UnrecordedSemanticTurn {
            attempt_id,
            turn_id: turn_descriptor.content_id,
            record: record.to_string(),
        })?;
    Ok(DecodedModelTurn {
        turn_id: turn_descriptor.content_id,
        proposals,
    })
}

fn materialize_turn<C: ContentStore>(
    content: &mut C,
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    adapter_version: AdapterVersion,
    adapter_turn: AdapterModelTurn,
) -> Result<(SemanticModelTurn, Vec<ToolCallProposal>), DecodeCoordinatorError> {
    let mut semantic_items = Vec::with_capacity(adapter_turn.items.len());
    let mut proposals = Vec::new();
    for (index, item) in adapter_turn.items.into_iter().enumerate() {
        match item {
            AdapterOutputItem::Text { text } => {
                semantic_items.push(SemanticOutputItem::Text { text });
            }
            AdapterOutputItem::ToolCall {
                provider_call_id,
                tool,
                arguments,
            } => {
                let ordinal = OutputOrdinal::from_index(index)?;
                let argument_bytes = cairn_codec::to_vec(&arguments).map_err(|error| {
                    DecodeCoordinatorError::InvalidSemanticTurn(error.to_string())
                })?;
                let descriptor = content.put::<ToolArguments>(&mut Cursor::new(&argument_bytes))?;
                let tool_call_id = derive_tool_call_id(
                    attempt_id,
                    response_id,
                    ordinal,
                    &provider_call_id,
                    &tool,
                    descriptor.content_id,
                )?;
                semantic_items.push(SemanticOutputItem::ToolCall {
                    tool_call_id,
                    ordinal,
                    provider_call_id,
                    tool: tool.clone(),
                    arguments_id: descriptor.content_id,
                });
                proposals.push(ToolCallProposal {
                    tool_call_id,
                    tool,
                    arguments_id: descriptor.content_id,
                    argument_bytes,
                });
            }
        }
    }
    Ok((
        SemanticModelTurn {
            source_response_id: response_id,
            adapter_version,
            items: semantic_items,
        },
        proposals,
    ))
}

fn semantic_facts(
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    response_event_id: cairn_protocol::EventId,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    observed_at: ObservedAtUnixMillis,
    semantic_turn: &SemanticModelTurn,
) -> Result<Vec<NewEvent>, DecodeCoordinatorError> {
    let decoded_payload = DecodedPayload {
        attempt_id,
        response_id,
        turn_id,
        adapter_version: semantic_turn.adapter_version.clone(),
    };
    let mut facts = vec![new_fact(
        RESPONSE_DECODED,
        response_event_id,
        observed_at,
        &decoded_payload,
    )?];
    for item in &semantic_turn.items {
        if let SemanticOutputItem::ToolCall {
            tool_call_id,
            ordinal,
            provider_call_id,
            tool,
            arguments_id,
        } = item
        {
            facts.push(new_fact(
                TOOL_CALL_PROPOSED,
                response_event_id,
                observed_at,
                &ProposedPayload {
                    attempt_id,
                    turn_id,
                    tool_call_id: *tool_call_id,
                    ordinal: *ordinal,
                    provider_call_id: provider_call_id.clone(),
                    tool: tool.clone(),
                    arguments_id: *arguments_id,
                },
            )?);
        }
    }
    Ok(facts)
}

/// Rebuilds a committed semantic turn and its unforgeable tool-call proposals from events and
/// verified content.
///
/// # Errors
///
/// Returns [`DecodeCoordinatorError`] when storage fails or event, turn, call, and argument
/// identities do not form one consistent projection.
pub fn recover_decoded_model_turn<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    stream: &StreamId,
    attempt_id: ModelAttemptId,
) -> Result<Option<DecodedModelTurn>, DecodeCoordinatorError> {
    let history = events.read_stream(stream, None)?;
    project_decoded_turn(content, &history, attempt_id)
}

fn project_decoded_turn<C: ContentStore>(
    content: &C,
    history: &[EventEnvelope],
    attempt_id: ModelAttemptId,
) -> Result<Option<DecodedModelTurn>, DecodeCoordinatorError> {
    let Some(facts) = collect_decoded_facts(history, attempt_id)? else {
        return Ok(None);
    };
    let DecodedFacts {
        decoded_event,
        decoded_payload,
        proposed,
    } = facts;
    let Some(response_event_id) = decoded_event.parent_event_id else {
        return invalid_turn("decoded turn has no response-event parent");
    };
    let response_exists = history.iter().any(|event| {
        event.event_id == response_event_id
            && event.schema_name.as_str() == "agent.model-response-received"
    });
    if !response_exists {
        return invalid_turn("decoded turn parent is not a response-received event");
    }

    let semantic_turn = read_semantic_turn(content, decoded_payload.turn_id)?;
    if semantic_turn.source_response_id != decoded_payload.response_id
        || semantic_turn.adapter_version != decoded_payload.adapter_version
    {
        return invalid_turn("semantic turn disagrees with its decoded event");
    }
    let expected_calls: Vec<_> = semantic_turn
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            SemanticOutputItem::ToolCall {
                tool_call_id,
                ordinal,
                provider_call_id,
                tool,
                arguments_id,
            } => Some((
                index,
                *tool_call_id,
                *ordinal,
                provider_call_id,
                tool,
                *arguments_id,
            )),
            SemanticOutputItem::Text { .. } => None,
        })
        .collect();
    if proposed.len() != expected_calls.len() {
        return invalid_turn("tool-call proposal count differs from semantic turn");
    }

    let mut proposals = Vec::with_capacity(proposed.len());
    for ((event, actual), expected) in proposed.into_iter().zip(expected_calls) {
        let (index, call_id, ordinal, provider_call_id, tool, arguments_id) = expected;
        if event.parent_event_id != Some(response_event_id)
            || actual.turn_id != decoded_payload.turn_id
            || actual.tool_call_id != call_id
            || actual.ordinal != ordinal
            || &actual.provider_call_id != provider_call_id
            || &actual.tool != tool
            || actual.arguments_id != arguments_id
        {
            return invalid_turn("tool-call event disagrees with semantic turn");
        }
        if ordinal != OutputOrdinal::from_index(index)? {
            return invalid_turn("tool-call ordinal does not match output position");
        }
        let derived = derive_tool_call_id(
            attempt_id,
            decoded_payload.response_id,
            ordinal,
            provider_call_id,
            tool,
            arguments_id,
        )?;
        if derived != call_id {
            return invalid_turn("tool-call identity does not match its relationship material");
        }
        let mut argument_bytes = Vec::new();
        content.write_to(&arguments_id, &mut argument_bytes)?;
        cairn_codec::from_slice::<serde_json::Value>(&argument_bytes)
            .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?;
        proposals.push(ToolCallProposal {
            tool_call_id: call_id,
            tool: tool.clone(),
            arguments_id,
            argument_bytes,
        });
    }
    Ok(Some(DecodedModelTurn {
        turn_id: decoded_payload.turn_id,
        proposals,
    }))
}

struct DecodedFacts<'a> {
    decoded_event: &'a EventEnvelope,
    decoded_payload: DecodedPayload,
    proposed: Vec<(&'a EventEnvelope, ProposedPayload)>,
}

fn collect_decoded_facts(
    history: &[EventEnvelope],
    attempt_id: ModelAttemptId,
) -> Result<Option<DecodedFacts<'_>>, DecodeCoordinatorError> {
    let mut decoded = None;
    let mut proposed = Vec::new();
    for event in history {
        match event.schema_name.as_str() {
            RESPONSE_DECODED => {
                let payload: DecodedPayload = decode_fact(event)?;
                if payload.attempt_id != attempt_id {
                    continue;
                }
                if decoded.replace((event, payload)).is_some() {
                    return invalid_turn("attempt has multiple semantic-turn facts");
                }
            }
            TOOL_CALL_PROPOSED => {
                let payload: ProposedPayload = decode_fact(event)?;
                if payload.attempt_id == attempt_id {
                    proposed.push((event, payload));
                }
            }
            _ => {}
        }
    }
    match decoded {
        Some((decoded_event, decoded_payload)) => Ok(Some(DecodedFacts {
            decoded_event,
            decoded_payload,
            proposed,
        })),
        None if proposed.is_empty() => Ok(None),
        None => invalid_turn("tool-call proposal exists without a decoded turn"),
    }
}

fn read_semantic_turn<C: ContentStore>(
    content: &C,
    turn_id: ContentId<SemanticModelTurnArtifact>,
) -> Result<SemanticModelTurn, DecodeCoordinatorError> {
    let mut bytes = Vec::new();
    content.write_to(&turn_id, &mut bytes)?;
    cairn_codec::from_slice(&bytes)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))
}

fn decode_fact<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, DecodeCoordinatorError> {
    if event.schema_version.get() != 1 {
        return invalid_turn("unsupported semantic event schema version");
    }
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))
}

fn derive_tool_call_id(
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    ordinal: OutputOrdinal,
    provider_call_id: &ProviderToolCallId,
    tool: &ToolName,
    arguments_id: ContentId<ToolArguments>,
) -> Result<ToolCallId, DecodeCoordinatorError> {
    let identity_material = ToolCallIdentityMaterial {
        attempt_id,
        response_id,
        ordinal,
        provider_call_id,
        tool,
        arguments_id,
    };
    let identity_bytes = cairn_codec::to_vec(&identity_material)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?;
    ToolCallId::derive(&identity_bytes)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))
}

fn invalid_turn<T>(message: &str) -> Result<T, DecodeCoordinatorError> {
    Err(DecodeCoordinatorError::InvalidSemanticTurn(
        message.to_owned(),
    ))
}

fn new_fact<P: Serialize>(
    schema: &str,
    parent_event_id: cairn_protocol::EventId,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, DecodeCoordinatorError> {
    let payload = cairn_codec::to_vec(payload)
        .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?;
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?,
        parent_event_id: Some(parent_event_id),
        observed_at_unix_ms: observed_at.get(),
        payload,
    })
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{
        AggregateId, AggregateKind, CommandId, ContentId, ModelAttemptId, OperationId,
    };
    use cairn_record::{ContentStore, EventStore, ExpectedRevision, StreamId};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        AdapterModelTurn, AdapterOutputItem, ModelAdapterError, RecordedAdapterExchange,
        RecordedModelAdapter, ScriptedModelAdapter, SemanticModelTurn, decode_model_response,
        materialize_turn, recover_decoded_model_turn,
    };
    use crate::{
        AdapterVersion, DispatchCompletion, MaterializedRequestArtifact, ModelResponseArtifact,
        ModelTransportResponse, PreparedModelRequest, ProviderToolCallId, ScriptedModelTransport,
        ToolEffectClass, ToolImplementationVersion, ToolName, TransportError,
        TurnInputDecisionArtifact, authorize_model_request, authorize_tool_operation,
        begin_model_dispatch, execute_model_dispatch, recover_received_model_response,
        recover_tool_operation,
    };

    struct ReceivedFixture {
        _directory: tempfile::TempDir,
        events: SqliteEventStore,
        content: SqliteContentStore,
        stream: StreamId,
        attempt_id: ModelAttemptId,
        received: crate::ReceivedModelResponse,
    }

    fn receive_response() -> ReceivedFixture {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store");
        let mut events = SqliteEventStore::in_memory().expect("event store");
        let stream = StreamId {
            kind: AggregateKind::new("agent-episode").expect("kind"),
            id: AggregateId::new("agent-episode:semantic-test").expect("id"),
        };
        let attempt_id = ModelAttemptId::new();
        let request = PreparedModelRequest {
            decision_id: ContentId::<TurnInputDecisionArtifact>::derive(b"decision")
                .expect("decision"),
            request_id: ContentId::<MaterializedRequestArtifact>::derive(b"request")
                .expect("request"),
            adapter_version: AdapterVersion::new("recorded-v1").expect("adapter"),
            request_bytes: b"request".to_vec(),
        };
        let authority = authorize_model_request(
            &mut events,
            &stream,
            ExpectedRevision::NoStream,
            &CommandId::new(),
            attempt_id,
            cairn_protocol::ObservedAtUnixMillis::new(1),
            request,
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
            Ok::<_, TransportError>(ModelTransportResponse::without_usage(
                b"provider-native-response".to_vec(),
            ))
        });
        let completion = execute_model_dispatch(
            &mut events,
            &mut content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("dispatch");
        let DispatchCompletion::Response(received) = completion else {
            panic!("expected response");
        };
        ReceivedFixture {
            _directory: directory,
            events,
            content,
            stream,
            attempt_id,
            received,
        }
    }

    fn tool_call(provider_call_id: &str, path: &str) -> AdapterOutputItem {
        AdapterOutputItem::ToolCall {
            provider_call_id: ProviderToolCallId::new(provider_call_id).expect("provider call"),
            tool: ToolName::new("read_source").expect("tool"),
            arguments: serde_json::json!({"path":path}),
        }
    }

    #[test]
    fn decoded_turn_and_tool_proposals_are_atomic_and_prepare_operations() {
        let mut fixture = receive_response();
        let response_id = fixture.received.response_id();
        let adapter_turn = AdapterModelTurn {
            items: vec![
                AdapterOutputItem::Text {
                    text: "I will inspect both files.".to_owned(),
                },
                tool_call("call-1", "src/lib.rs"),
                tool_call("call-2", "Cargo.toml"),
            ],
        };
        let mut adapter = RecordedModelAdapter::new(
            AdapterVersion::new("recorded-v1").expect("version"),
            [RecordedAdapterExchange {
                response_id,
                turn: adapter_turn,
            }],
        );
        let decoded = decode_model_response(
            &mut fixture.events,
            &mut fixture.content,
            &mut adapter,
            fixture.received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("decode");
        assert_eq!(decoded.proposals().len(), 2);

        let mut turn_bytes = Vec::new();
        fixture
            .content
            .write_to(&decoded.turn_id(), &mut turn_bytes)
            .expect("read semantic turn");
        let turn: SemanticModelTurn = cairn_codec::from_slice(&turn_bytes).expect("decode turn");
        assert_eq!(turn.items.len(), 3);

        let history = fixture
            .events
            .read_stream(&fixture.stream, None)
            .expect("read episode");
        assert_eq!(history.len(), 6);
        assert_eq!(
            history[3].schema_name.as_str(),
            "agent.model-response-decoded"
        );
        assert_eq!(history[4].schema_name.as_str(), "agent.tool-call-proposed");
        assert_eq!(history[5].schema_name.as_str(), "agent.tool-call-proposed");
        assert_eq!(history[3].parent_event_id, Some(history[2].event_id));
        assert_eq!(history[4].parent_event_id, Some(history[2].event_id));
        let recovered = recover_decoded_model_turn(
            &fixture.events,
            &fixture.content,
            &fixture.stream,
            fixture.attempt_id,
        )
        .expect("recover decoded turn")
        .expect("decoded turn");
        assert_eq!(recovered.turn_id(), decoded.turn_id());
        assert_eq!(recovered.proposals().len(), 2);
        assert_eq!(
            recovered.proposals()[0].tool_call_id(),
            decoded.proposals()[0].tool_call_id()
        );

        let proposal = decoded
            .into_proposals()
            .into_iter()
            .next()
            .expect("proposal");
        let tool_call_id = proposal.tool_call_id();
        let operation = proposal.into_prepared_operation(
            OperationId::new(),
            ToolImplementationVersion::new("v1").expect("tool version"),
            ToolEffectClass::ReadOnly,
        );
        assert_eq!(operation.source_tool_call_id(), Some(tool_call_id));
        let operation_id = operation.operation_id();
        let _authority = authorize_tool_operation(
            &mut fixture.events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(5),
            operation,
        )
        .expect("authorize operation");
        assert!(matches!(
            recover_tool_operation(&fixture.events, operation_id).expect("recover operation"),
            crate::ToolOperationState::Authorized {
                effect: ToolEffectClass::ReadOnly
            }
        ));
    }

    #[test]
    fn adapter_failure_leaves_response_recoverable_without_new_facts() {
        let mut fixture = receive_response();
        let mut adapter = ScriptedModelAdapter::new(
            AdapterVersion::new("recorded-v1").expect("version"),
            |_: ContentId<ModelResponseArtifact>, _: &[u8]| {
                Err(ModelAdapterError::InvalidResponse(
                    "missing output".to_owned(),
                ))
            },
        );
        let error = decode_model_response(
            &mut fixture.events,
            &mut fixture.content,
            &mut adapter,
            fixture.received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect_err("decode must fail");
        assert!(matches!(error, super::DecodeCoordinatorError::Adapter(_)));
        let history = fixture
            .events
            .read_stream(&fixture.stream, None)
            .expect("read episode");
        assert_eq!(history.len(), 3);
        let recovered = recover_received_model_response(&history, fixture.attempt_id)
            .expect("recover")
            .expect("response authority");
        assert_eq!(recovered.attempt_id(), fixture.attempt_id);
    }

    #[test]
    fn adapter_version_mismatch_is_rejected_before_interpretation() {
        let mut fixture = receive_response();
        let mut adapter = ScriptedModelAdapter::new(
            AdapterVersion::new("wrong-v2").expect("version"),
            |_: ContentId<ModelResponseArtifact>, _: &[u8]| -> Result<_, ModelAdapterError> {
                panic!("mismatched adapter must not inspect response bytes")
            },
        );
        let error = decode_model_response(
            &mut fixture.events,
            &mut fixture.content,
            &mut adapter,
            fixture.received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect_err("version mismatch");
        assert!(matches!(
            error,
            super::DecodeCoordinatorError::AdapterVersionMismatch { .. }
        ));
        assert_eq!(
            fixture
                .events
                .read_stream(&fixture.stream, None)
                .expect("read")
                .len(),
            3
        );
    }

    #[test]
    fn tool_call_identity_commits_to_output_position() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store");
        let attempt_id = ModelAttemptId::new();
        let response_id =
            ContentId::<ModelResponseArtifact>::derive(b"response").expect("response");
        let version = AdapterVersion::new("v1").expect("version");
        let (_, first) = materialize_turn(
            &mut content,
            attempt_id,
            response_id,
            version.clone(),
            AdapterModelTurn {
                items: vec![tool_call("call-1", "src/lib.rs")],
            },
        )
        .expect("first");
        let (_, shifted) = materialize_turn(
            &mut content,
            attempt_id,
            response_id,
            version,
            AdapterModelTurn {
                items: vec![
                    AdapterOutputItem::Text {
                        text: "first".to_owned(),
                    },
                    tool_call("call-1", "src/lib.rs"),
                ],
            },
        )
        .expect("shifted");
        assert_ne!(first[0].tool_call_id(), shifted[0].tool_call_id());
    }

    #[test]
    fn lost_decode_acknowledgement_replays_without_duplicate_facts() {
        let mut fixture = receive_response();
        let response_id = fixture.received.response_id();
        let adapter_turn = AdapterModelTurn {
            items: vec![tool_call("call-1", "src/lib.rs")],
        };
        let command = CommandId::new();
        let observed_at = cairn_protocol::ObservedAtUnixMillis::new(4);
        let mut first_adapter = RecordedModelAdapter::new(
            AdapterVersion::new("recorded-v1").expect("version"),
            [RecordedAdapterExchange {
                response_id,
                turn: adapter_turn.clone(),
            }],
        );
        let first = decode_model_response(
            &mut fixture.events,
            &mut fixture.content,
            &mut first_adapter,
            fixture.received,
            &command,
            observed_at,
        )
        .expect("first decode");

        let history = fixture
            .events
            .read_stream(&fixture.stream, None)
            .expect("read after first");
        let recovered = recover_received_model_response(&history, fixture.attempt_id)
            .expect("recover response")
            .expect("response authority");
        let mut replay_adapter = RecordedModelAdapter::new(
            AdapterVersion::new("recorded-v1").expect("version"),
            [RecordedAdapterExchange {
                response_id,
                turn: adapter_turn,
            }],
        );
        let replay = decode_model_response(
            &mut fixture.events,
            &mut fixture.content,
            &mut replay_adapter,
            recovered,
            &command,
            observed_at,
        )
        .expect("idempotent replay");
        assert_eq!(replay.turn_id(), first.turn_id());
        assert_eq!(
            replay.proposals()[0].tool_call_id(),
            first.proposals()[0].tool_call_id()
        );
        assert_eq!(
            fixture
                .events
                .read_stream(&fixture.stream, None)
                .expect("read replay")
                .len(),
            5
        );
    }
}
