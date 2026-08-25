//! Lossless, protocol-native model continuation and deterministic local request reconstruction.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

use cairn_protocol::{
    CommandId, ContentId, ModelAttemptId, ObservedAtUnixMillis, SchemaName, SchemaVersion,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventStore, EventStoreError, ExpectedRevision, NewEvent,
    StreamId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::semantic::{materialize_turn, semantic_facts};
use crate::{
    AdapterModelTurn, AdapterOutputItem, ChatReasoningReplay, DecodeCoordinatorError,
    DecodedModelTurn, InputAuditError, MaterializedRequestArtifact, ModelName,
    ModelOutputTokenLimit, ModelProtocolConfig, ModelProtocolKind, ModelResponseArtifact,
    NativeContinuationArtifact, NativeRequestStateArtifact, OperationResult, PreparedModelRequest,
    ProviderToolCallId, ReceivedModelResponse, ResponsesReasoningReplay, SemanticModelTurnArtifact,
    ToolName, TurnInputDecision, prepare_model_request,
};

const NATIVE_CONTINUATION_SCHEMA_V1: u16 = 1;
const NATIVE_CONTINUATION_RECORDED: &str = "agent.native-continuation-recorded";

/// One tool definition encoded by a protocol codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeToolDefinition {
    /// Registered tool name.
    pub name: ToolName,
    /// Model-visible description.
    pub description: String,
    /// Model-visible JSON Schema object.
    pub input_schema: Value,
    /// Whether the compatible endpoint should request strict schema enforcement.
    pub strict: bool,
}

/// Stable, secret-free parameters shared by each reconstructed request in an episode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRequestSpec {
    /// Provider-visible model identifier.
    pub wire_model: ModelName,
    /// Model-visible system/developer instruction text.
    pub instructions: String,
    /// Frozen tool catalog for the turn.
    pub tools: Vec<NativeToolDefinition>,
    /// Provider output ceiling.
    pub max_output_tokens: ModelOutputTokenLimit,
}

/// One trusted, model-visible result correlated to a provider-native tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeToolResult {
    /// Exact provider correlation identity.
    pub call_id: ProviderToolCallId,
    /// Model-visible result text.
    pub output: String,
}

/// Versioned local state that must survive restart to continue a native provider conversation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContinuation {
    schema_version: u16,
    history: NativeHistory,
    pending_call_ids: Vec<ProviderToolCallId>,
    source_response_ids: Vec<ContentId<ModelResponseArtifact>>,
}

impl NativeContinuation {
    /// Returns the protocol family encoded by the closed history variant.
    #[must_use]
    pub const fn protocol(&self) -> ModelProtocolKind {
        self.history.protocol()
    }

    /// Returns unresolved native tool-call identities in provider order.
    #[must_use]
    pub fn pending_call_ids(&self) -> &[ProviderToolCallId] {
        &self.pending_call_ids
    }

    /// Returns every archived raw response contributing to this continuation boundary.
    #[must_use]
    pub fn source_response_ids(&self) -> &[ContentId<ModelResponseArtifact>] {
        &self.source_response_ids
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum NativeHistory {
    OpenAiResponses { input: Vec<Value> },
    OpenAiChatCompletions { messages: Vec<Value> },
    AnthropicMessages { messages: Vec<Value> },
}

impl NativeHistory {
    const fn protocol(&self) -> ModelProtocolKind {
        match self {
            Self::OpenAiResponses { .. } => ModelProtocolKind::OpenAiResponses,
            Self::OpenAiChatCompletions { .. } => ModelProtocolKind::OpenAiChatCompletions,
            Self::AnthropicMessages { .. } => ModelProtocolKind::AnthropicMessages,
        }
    }
}

/// Exact, deterministic provider request plus the history boundary it represents.
#[derive(Clone, Debug)]
pub struct PreparedNativeRequest {
    bytes: Vec<u8>,
    base_continuation: NativeContinuation,
    offered_tools: BTreeSet<ToolName>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRequestState {
    schema_version: u16,
    protocol: ModelProtocolKind,
    request_id: ContentId<MaterializedRequestArtifact>,
    base_continuation: NativeContinuation,
    offered_tools: BTreeSet<ToolName>,
}

/// One provider response decoded once into both replay state and provider-neutral semantics.
///
/// Keeping these projections together prevents protocol validation and semantic tool discovery
/// from silently disagreeing about the same immutable response bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolDecodedTurn {
    continuation: NativeContinuation,
    semantic: AdapterModelTurn,
}

impl ProtocolDecodedTurn {
    /// Returns the lossless provider-native continuation boundary.
    #[must_use]
    pub const fn continuation(&self) -> &NativeContinuation {
        &self.continuation
    }

    /// Returns the ordered provider-neutral semantic projection.
    #[must_use]
    pub const fn semantic(&self) -> &AdapterModelTurn {
        &self.semantic
    }

    fn into_parts(self) -> (NativeContinuation, AdapterModelTurn) {
        (self.continuation, self.semantic)
    }
}

/// Atomically published protocol-native and semantic response boundary.
#[derive(Debug)]
pub struct DecodedProtocolModelTurn {
    continuation_id: ContentId<NativeContinuationArtifact>,
    continuation: NativeContinuation,
    semantic: DecodedModelTurn,
}

impl DecodedProtocolModelTurn {
    /// Returns the durable provider-native continuation identity.
    #[must_use]
    pub const fn continuation_id(&self) -> ContentId<NativeContinuationArtifact> {
        self.continuation_id
    }

    /// Returns the validated provider-native continuation.
    #[must_use]
    pub const fn continuation(&self) -> &NativeContinuation {
        &self.continuation
    }

    /// Returns the durable semantic turn and its one-shot tool proposals.
    #[must_use]
    pub const fn semantic(&self) -> &DecodedModelTurn {
        &self.semantic
    }

    /// Consumes the boundary and returns the semantic turn.
    #[must_use]
    pub fn into_semantic(self) -> DecodedModelTurn {
        self.semantic
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRecordedPayload {
    attempt_id: ModelAttemptId,
    response_id: ContentId<ModelResponseArtifact>,
    continuation_id: ContentId<NativeContinuationArtifact>,
    protocol: ModelProtocolKind,
}

impl PreparedNativeRequest {
    /// Returns the exact JSON bytes to dispatch.
    #[must_use]
    pub fn request_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the exact local history boundary encoded into the request.
    #[must_use]
    pub const fn base_continuation(&self) -> &NativeContinuation {
        &self.base_continuation
    }
}

/// Audits a durable turn decision and binds exact protocol-native bytes to dispatch authority.
///
/// This is the bridge from the domain-neutral input completeness audit to a selected protocol
/// codec. The provider-neutral audit projection remains evidence; only `native.request_bytes()` is
/// dispatched.
///
/// # Errors
///
/// Returns [`InputAuditError`] if any cited model-visible fact is missing/corrupt or if the native
/// bytes cannot be archived.
pub fn prepare_native_dispatch_request<S: ContentStore>(
    store: &mut S,
    decision: &TurnInputDecision,
    native: &PreparedNativeRequest,
) -> Result<PreparedModelRequest, InputAuditError> {
    let audited = prepare_model_request(store, decision)?;
    let descriptor = store
        .put::<MaterializedRequestArtifact>(&mut Cursor::new(native.request_bytes()))
        .map_err(|error| crate::audit_from_store("native_request", "unmaterialized", &error))?;
    let state = NativeRequestState {
        schema_version: 1,
        protocol: native.base_continuation.protocol(),
        request_id: descriptor.content_id,
        base_continuation: native.base_continuation.clone(),
        offered_tools: native.offered_tools.clone(),
    };
    let state_bytes = serde_json::to_vec(&state)
        .map_err(|error| crate::audit_from_codec("native_request_state", error.to_string()))?;
    let state_descriptor = store
        .put::<NativeRequestStateArtifact>(&mut Cursor::new(state_bytes))
        .map_err(|error| {
            crate::audit_from_store("native_request_state", "unmaterialized", &error)
        })?;
    Ok(PreparedModelRequest {
        decision_id: audited.decision_id,
        request_id: descriptor.content_id,
        adapter_version: audited.adapter_version,
        native_state_id: Some(Box::new(state_descriptor.content_id)),
        request_bytes: native.request_bytes().to_vec(),
    })
}

pub(crate) fn validate_native_request_state_reference<S: ContentStore>(
    store: &S,
    state_id: &ContentId<NativeRequestStateArtifact>,
    expected_request_id: ContentId<MaterializedRequestArtifact>,
) -> Result<(), NativeCodecError> {
    let mut state_bytes = Vec::new();
    store
        .write_to(state_id, &mut state_bytes)
        .map_err(NativeCodecError::Storage)?;
    let state: NativeRequestState = serde_json::from_slice(&state_bytes)
        .map_err(|error| NativeCodecError::InvalidJson(error.to_string()))?;
    if serde_json::to_vec(&state)
        .map_err(|error| NativeCodecError::Serialization(error.to_string()))?
        != state_bytes
    {
        return Err(NativeCodecError::NonCanonicalRequestState);
    }
    if state.schema_version != 1 {
        return Err(NativeCodecError::UnsupportedRequestSchema(
            state.schema_version,
        ));
    }
    if state.protocol != state.base_continuation.protocol()
        || state.request_id != expected_request_id
    {
        return Err(NativeCodecError::RequestStateMismatch);
    }
    Ok(())
}

/// A closed codec selected from a model template's protocol profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProtocolCodec {
    kind: ModelProtocolKind,
    responses_reasoning: ResponsesReasoningReplay,
    chat_reasoning: ChatReasoningReplay,
    chat_thinking_parameter: bool,
}

impl NativeProtocolCodec {
    /// Builds a codec from repository-owned model characteristics.
    ///
    /// # Errors
    ///
    /// Rejects hosted Responses state because V1 requires a locally reconstructable boundary.
    pub fn from_config(config: &ModelProtocolConfig) -> Result<Self, NativeCodecError> {
        match config {
            ModelProtocolConfig::OpenAiResponses {
                store,
                reasoning_replay,
            } => {
                if *store {
                    return Err(NativeCodecError::HostedResponsesState);
                }
                Ok(Self {
                    kind: ModelProtocolKind::OpenAiResponses,
                    responses_reasoning: *reasoning_replay,
                    chat_reasoning: ChatReasoningReplay::PreserveIfPresent,
                    chat_thinking_parameter: false,
                })
            }
            ModelProtocolConfig::OpenAiChatCompletions {
                thinking_parameter,
                reasoning_replay,
            } => Ok(Self {
                kind: ModelProtocolKind::OpenAiChatCompletions,
                responses_reasoning: ResponsesReasoningReplay::PreserveOutputItems,
                chat_reasoning: *reasoning_replay,
                chat_thinking_parameter: *thinking_parameter,
            }),
            ModelProtocolConfig::AnthropicMessages { .. } => Ok(Self {
                kind: ModelProtocolKind::AnthropicMessages,
                responses_reasoning: ResponsesReasoningReplay::PreserveOutputItems,
                chat_reasoning: ChatReasoningReplay::PreserveIfPresent,
                chat_thinking_parameter: false,
            }),
        }
    }

    /// Returns the selected wire protocol.
    #[must_use]
    pub const fn kind(self) -> ModelProtocolKind {
        self.kind
    }

    /// Materializes the first provider request and its empty response boundary.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid tools, empty input, or serialization failure.
    pub fn prepare_initial(
        self,
        spec: &NativeRequestSpec,
        user_text: &str,
    ) -> Result<PreparedNativeRequest, NativeCodecError> {
        if user_text.trim().is_empty() {
            return Err(NativeCodecError::MissingInitialInput);
        }
        validate_tools(&spec.tools)?;
        let history = match self.kind {
            ModelProtocolKind::OpenAiResponses => NativeHistory::OpenAiResponses {
                input: vec![json!({
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": user_text}]
                })],
            },
            ModelProtocolKind::OpenAiChatCompletions => {
                let mut messages = Vec::new();
                if !spec.instructions.trim().is_empty() {
                    messages.push(json!({"role": "system", "content": spec.instructions}));
                }
                messages.push(json!({"role": "user", "content": user_text}));
                NativeHistory::OpenAiChatCompletions { messages }
            }
            ModelProtocolKind::AnthropicMessages => NativeHistory::AnthropicMessages {
                messages: vec![json!({
                    "role": "user",
                    "content": [{"type": "text", "text": user_text}]
                })],
            },
        };
        self.prepare_from_boundary(
            spec,
            NativeContinuation {
                schema_version: NATIVE_CONTINUATION_SCHEMA_V1,
                history,
                pending_call_ids: Vec::new(),
                source_response_ids: Vec::new(),
            },
        )
    }

    /// Reconstructs the next request from a settled local continuation.
    ///
    /// # Errors
    ///
    /// Rejects protocol mismatch, incomplete tool results, invalid native state, or serialization
    /// failure.
    pub fn prepare_continuation(
        self,
        spec: &NativeRequestSpec,
        continuation: &NativeContinuation,
    ) -> Result<PreparedNativeRequest, NativeCodecError> {
        validate_tools(&spec.tools)?;
        self.validate(continuation)?;
        if !continuation.pending_call_ids.is_empty() {
            return Err(NativeCodecError::PendingToolResults);
        }
        self.prepare_from_boundary(spec, continuation.clone())
    }

    /// Extends a prepared boundary with one exact raw provider response.
    ///
    /// Every protocol-native output item/block is retained; semantic interpretation is a separate
    /// projection.
    ///
    /// # Errors
    ///
    /// Rejects identity mismatch, malformed response shapes, missing required thinking state, or
    /// invalid tool correlation identities.
    pub fn decode_response(
        self,
        request: &PreparedNativeRequest,
        response_id: ContentId<ModelResponseArtifact>,
        response_bytes: &[u8],
    ) -> Result<NativeContinuation, NativeCodecError> {
        self.decode_turn(request, response_id, response_bytes)
            .map(|turn| turn.continuation)
    }

    /// Decodes one immutable response into lossless replay state and semantic output in one pass.
    ///
    /// # Errors
    ///
    /// Rejects identity mismatch, malformed protocol state, invalid semantic tool arguments, or
    /// references to tools that were not offered by the exact request.
    pub fn decode_turn(
        self,
        request: &PreparedNativeRequest,
        response_id: ContentId<ModelResponseArtifact>,
        response_bytes: &[u8],
    ) -> Result<ProtocolDecodedTurn, NativeCodecError> {
        let actual = ContentId::<ModelResponseArtifact>::derive(response_bytes)
            .map_err(|error| NativeCodecError::Identity(error.to_string()))?;
        if actual != response_id {
            return Err(NativeCodecError::ResponseIdentityMismatch);
        }
        self.validate(&request.base_continuation)?;
        if request.base_continuation.protocol() != self.kind {
            return Err(NativeCodecError::ProtocolMismatch);
        }
        let response: Value = serde_json::from_slice(response_bytes)
            .map_err(|error| NativeCodecError::InvalidJson(error.to_string()))?;
        let mut continuation = request.base_continuation.clone();
        let (pending, semantic) = match (&mut continuation.history, self.kind) {
            (NativeHistory::OpenAiResponses { input }, ModelProtocolKind::OpenAiResponses) => {
                let output = required_array(&response, "output")?;
                let decoded =
                    responses_turn(output, &request.offered_tools, self.responses_reasoning)?;
                input.extend(output.iter().cloned());
                decoded
            }
            (
                NativeHistory::OpenAiChatCompletions { messages },
                ModelProtocolKind::OpenAiChatCompletions,
            ) => {
                let choices = required_array(&response, "choices")?;
                if choices.len() != 1 {
                    return Err(NativeCodecError::InvalidShape(
                        "Chat response must contain exactly one choice".to_owned(),
                    ));
                }
                let message = choices[0].get("message").ok_or_else(|| {
                    NativeCodecError::InvalidShape("Chat choice.message is required".to_owned())
                })?;
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    return Err(NativeCodecError::InvalidShape(
                        "Chat response message must have assistant role".to_owned(),
                    ));
                }
                let decoded = chat_turn(message, &request.offered_tools, self.chat_reasoning)?;
                messages.push(message.clone());
                decoded
            }
            (
                NativeHistory::AnthropicMessages { messages },
                ModelProtocolKind::AnthropicMessages,
            ) => {
                if response.get("type").and_then(Value::as_str) != Some("message")
                    || response.get("role").and_then(Value::as_str) != Some("assistant")
                {
                    return Err(NativeCodecError::InvalidShape(
                        "Anthropic response must be an assistant message".to_owned(),
                    ));
                }
                let content = required_array(&response, "content")?;
                let decoded = anthropic_turn(content, &request.offered_tools)?;
                messages.push(json!({"role": "assistant", "content": content}));
                decoded
            }
            _ => return Err(NativeCodecError::ProtocolMismatch),
        };
        continuation.pending_call_ids = pending;
        continuation.source_response_ids.push(response_id);
        self.validate(&continuation)?;
        Ok(ProtocolDecodedTurn {
            continuation,
            semantic: AdapterModelTurn { items: semantic },
        })
    }

    /// Appends exactly one result for every pending native call, in provider call order.
    ///
    /// # Errors
    ///
    /// Rejects missing, duplicate, unknown, or empty tool results.
    pub fn append_tool_results(
        self,
        continuation: &NativeContinuation,
        results: &[NativeToolResult],
    ) -> Result<NativeContinuation, NativeCodecError> {
        self.validate(continuation)?;
        let correlated = correlate_results(&continuation.pending_call_ids, results)?;
        let mut next = continuation.clone();
        match &mut next.history {
            NativeHistory::OpenAiResponses { input } => {
                for call_id in &continuation.pending_call_ids {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id.as_str(),
                        "output": correlated[call_id.as_str()]
                    }));
                }
            }
            NativeHistory::OpenAiChatCompletions { messages } => {
                for call_id in &continuation.pending_call_ids {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id.as_str(),
                        "content": correlated[call_id.as_str()]
                    }));
                }
            }
            NativeHistory::AnthropicMessages { messages } => {
                let content = continuation
                    .pending_call_ids
                    .iter()
                    .map(|call_id| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": call_id.as_str(),
                            "content": correlated[call_id.as_str()]
                        })
                    })
                    .collect::<Vec<_>>();
                messages.push(json!({"role": "user", "content": content}));
            }
        }
        next.pending_call_ids.clear();
        self.validate(&next)?;
        Ok(next)
    }

    /// Loads ordered durable operation results and appends them to the pending native calls.
    ///
    /// The step state machine preserves proposal order when producing `result_ids`; this method
    /// binds that order to the provider-native call IDs and therefore does not accept caller-made
    /// correlation strings.
    ///
    /// # Errors
    ///
    /// Rejects missing/corrupt result artifacts, non-UTF-8 model-visible results, or a cardinality
    /// mismatch with the pending provider calls.
    pub fn append_archived_tool_results<S: ContentStore>(
        self,
        store: &S,
        continuation: &NativeContinuation,
        result_ids: &[ContentId<OperationResult>],
    ) -> Result<NativeContinuation, NativeCodecError> {
        if continuation.pending_call_ids.len() != result_ids.len() {
            return Err(NativeCodecError::ToolResultMismatch);
        }
        let mut results = Vec::with_capacity(result_ids.len());
        for (call_id, result_id) in continuation.pending_call_ids.iter().zip(result_ids) {
            let mut bytes = Vec::new();
            store
                .write_to(result_id, &mut bytes)
                .map_err(NativeCodecError::Storage)?;
            let output = String::from_utf8(bytes).map_err(|_| {
                NativeCodecError::InvalidToolResultEncoding(call_id.as_str().to_owned())
            })?;
            results.push(NativeToolResult {
                call_id: call_id.clone(),
                output,
            });
        }
        self.append_tool_results(continuation, &results)
    }

    /// Appends a new human turn after a settled assistant turn.
    ///
    /// # Errors
    ///
    /// Rejects empty text, protocol mismatch, or a boundary that still awaits tool results.
    pub fn append_user_text(
        self,
        continuation: &NativeContinuation,
        text: &str,
    ) -> Result<NativeContinuation, NativeCodecError> {
        self.validate(continuation)?;
        if !continuation.pending_call_ids.is_empty() {
            return Err(NativeCodecError::PendingToolResults);
        }
        if text.trim().is_empty() {
            return Err(NativeCodecError::MissingUserInput);
        }
        let mut next = continuation.clone();
        match &mut next.history {
            NativeHistory::OpenAiResponses { input } => input.push(json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": text}]
            })),
            NativeHistory::OpenAiChatCompletions { messages } => {
                messages.push(json!({"role": "user", "content": text}));
            }
            NativeHistory::AnthropicMessages { messages } => messages.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": text}]
            })),
        }
        self.validate(&next)?;
        Ok(next)
    }

    /// Stores a sensitive native continuation as a typed CAS artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, serialization, or content storage fails.
    pub fn archive<S: ContentStore>(
        self,
        store: &mut S,
        continuation: &NativeContinuation,
    ) -> Result<ContentId<NativeContinuationArtifact>, NativeCodecError> {
        self.validate(continuation)?;
        let bytes = encode_continuation(continuation)?;
        store
            .put::<NativeContinuationArtifact>(&mut Cursor::new(bytes))
            .map(|descriptor| descriptor.content_id)
            .map_err(NativeCodecError::Storage)
    }

    /// Rehydrates and verifies a typed native continuation from durable storage.
    ///
    /// # Errors
    ///
    /// Returns an error for missing/corrupt bytes, unsupported schema, protocol mismatch, or
    /// invalid reasoning/tool correlation state.
    pub fn recover<S: ContentStore>(
        self,
        store: &S,
        continuation_id: &ContentId<NativeContinuationArtifact>,
    ) -> Result<NativeContinuation, NativeCodecError> {
        let mut bytes = Vec::new();
        store
            .write_to(continuation_id, &mut bytes)
            .map_err(NativeCodecError::Storage)?;
        let continuation: NativeContinuation = serde_json::from_slice(&bytes)
            .map_err(|error| NativeCodecError::InvalidJson(error.to_string()))?;
        if encode_continuation(&continuation)? != bytes {
            return Err(NativeCodecError::NonCanonicalContinuation);
        }
        self.validate(&continuation)?;
        Ok(continuation)
    }

    /// Reconstructs the exact native request and its decode context from a typed CAS artifact.
    ///
    /// # Errors
    ///
    /// Rejects missing/corrupt state, a mismatched protocol, unsupported schema, or request bytes
    /// whose typed identity no longer matches the archived state.
    pub fn recover_request<S: ContentStore>(
        self,
        store: &S,
        state_id: &ContentId<NativeRequestStateArtifact>,
    ) -> Result<PreparedNativeRequest, NativeCodecError> {
        let mut state_bytes = Vec::new();
        store
            .write_to(state_id, &mut state_bytes)
            .map_err(NativeCodecError::Storage)?;
        let state: NativeRequestState = serde_json::from_slice(&state_bytes)
            .map_err(|error| NativeCodecError::InvalidJson(error.to_string()))?;
        if serde_json::to_vec(&state)
            .map_err(|error| NativeCodecError::Serialization(error.to_string()))?
            != state_bytes
        {
            return Err(NativeCodecError::NonCanonicalRequestState);
        }
        if state.schema_version != 1 {
            return Err(NativeCodecError::UnsupportedRequestSchema(
                state.schema_version,
            ));
        }
        if state.protocol != self.kind || state.base_continuation.protocol() != self.kind {
            return Err(NativeCodecError::ProtocolMismatch);
        }
        self.validate(&state.base_continuation)?;
        let mut bytes = Vec::new();
        store
            .write_to(&state.request_id, &mut bytes)
            .map_err(NativeCodecError::Storage)?;
        let actual = ContentId::<MaterializedRequestArtifact>::derive(&bytes)
            .map_err(|error| NativeCodecError::Identity(error.to_string()))?;
        if actual != state.request_id {
            return Err(NativeCodecError::RequestIdentityMismatch);
        }
        Ok(PreparedNativeRequest {
            bytes,
            base_continuation: state.base_continuation,
            offered_tools: state.offered_tools,
        })
    }

    /// Recovers the exact request context carried by a durable response, then atomically decodes
    /// and publishes the response without relying on process memory.
    ///
    /// # Errors
    ///
    /// Returns an error if this was not a protocol-native dispatch or if request recovery and
    /// response publication fail.
    pub fn decode_recovered_received<E: EventStore, C: ContentStore>(
        self,
        events: &mut E,
        content: &mut C,
        received: ReceivedModelResponse,
        command_id: &CommandId,
        observed_at: ObservedAtUnixMillis,
    ) -> Result<DecodedProtocolModelTurn, ProtocolDecodeCoordinatorError> {
        let state_id = received
            .native_state_id
            .as_deref()
            .copied()
            .ok_or(NativeCodecError::MissingNativeRequestState)?;
        let request = self.recover_request(content, &state_id)?;
        self.decode_received(events, content, &request, received, command_id, observed_at)
    }

    /// Decodes a durable response once and atomically publishes its native replay boundary,
    /// semantic turn, and every ordered tool-call proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when response recovery, protocol validation, semantic materialization,
    /// artifact archival, or the indivisible event batch fails. Artifacts written before a failed
    /// append are inert CAS orphans; the durable raw response remains safe to decode again.
    pub fn decode_received<E: EventStore, C: ContentStore>(
        self,
        events: &mut E,
        content: &mut C,
        request: &PreparedNativeRequest,
        received: ReceivedModelResponse,
        command_id: &CommandId,
        observed_at: ObservedAtUnixMillis,
    ) -> Result<DecodedProtocolModelTurn, ProtocolDecodeCoordinatorError> {
        let ReceivedModelResponse {
            attempt_id,
            stream,
            revision,
            response_event_id,
            response_id,
            adapter_version,
            native_state_id: _,
            usage: _,
        } = received;
        let mut response_bytes = Vec::new();
        content
            .write_to(&response_id, &mut response_bytes)
            .map_err(NativeCodecError::Storage)?;
        let decoded = self.decode_turn(request, response_id, &response_bytes)?;
        let (continuation, adapter_turn) = decoded.into_parts();
        let continuation_id = self.archive(content, &continuation)?;
        let (semantic_turn, proposals) = materialize_turn(
            content,
            attempt_id,
            response_id,
            adapter_version,
            adapter_turn,
        )?;
        let turn_bytes = cairn_codec::to_vec(&semantic_turn)
            .map_err(|error| DecodeCoordinatorError::InvalidSemanticTurn(error.to_string()))?;
        let turn_id = content
            .put::<SemanticModelTurnArtifact>(&mut Cursor::new(turn_bytes))
            .map_err(DecodeCoordinatorError::Content)?
            .content_id;

        let native_payload = NativeRecordedPayload {
            attempt_id,
            response_id,
            continuation_id,
            protocol: self.kind,
        };
        let native_fact = NewEvent {
            schema_name: SchemaName::new(NATIVE_CONTINUATION_RECORDED)
                .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?,
            schema_version: SchemaVersion::new(1)
                .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?,
            parent_event_id: Some(response_event_id),
            observed_at_unix_ms: observed_at.get(),
            payload: cairn_codec::to_vec(&native_payload)
                .map_err(|error| NativeCodecError::Serialization(error.to_string()))?,
        };
        let mut facts = vec![native_fact];
        facts.extend(semantic_facts(
            attempt_id,
            response_id,
            response_event_id,
            turn_id,
            observed_at,
            &semantic_turn,
        )?);
        events
            .append(
                &stream,
                ExpectedRevision::Exact(revision),
                command_id,
                &facts,
            )
            .map_err(|record| ProtocolDecodeCoordinatorError::UnrecordedTurn {
                attempt_id,
                continuation_id,
                turn_id,
                record: record.to_string(),
            })?;
        Ok(DecodedProtocolModelTurn {
            continuation_id,
            continuation,
            semantic: DecodedModelTurn { turn_id, proposals },
        })
    }

    /// Finds a native continuation solely from an attempt's durable event history and CAS facts.
    ///
    /// # Errors
    ///
    /// Rejects duplicate facts, broken response causality, missing raw response bytes, protocol
    /// mismatch, or corrupt continuation bytes.
    pub fn recover_recorded<E: EventStore, C: ContentStore>(
        self,
        events: &E,
        content: &C,
        stream: &StreamId,
        attempt_id: ModelAttemptId,
    ) -> Result<Option<(ContentId<NativeContinuationArtifact>, NativeContinuation)>, NativeCodecError>
    {
        let history = events
            .read_stream(stream, None)
            .map_err(NativeCodecError::Event)?;
        let mut found = None;
        for event in &history {
            if event.schema_name.as_str() != NATIVE_CONTINUATION_RECORDED {
                continue;
            }
            if event.schema_version.get() != 1 {
                return Err(NativeCodecError::UnsupportedEventSchema(
                    event.schema_version.get(),
                ));
            }
            let payload: NativeRecordedPayload = cairn_codec::from_slice(&event.payload)
                .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?;
            if payload.attempt_id != attempt_id {
                continue;
            }
            if found.replace((event, payload)).is_some() {
                return Err(NativeCodecError::DuplicateRecordedContinuation);
            }
        }
        let Some((event, payload)) = found else {
            return Ok(None);
        };
        if payload.protocol != self.kind {
            return Err(NativeCodecError::ProtocolMismatch);
        }
        let parent = event.parent_event_id.ok_or_else(|| {
            NativeCodecError::InvalidShape(
                "native continuation event has no response parent".to_owned(),
            )
        })?;
        let Some(response_event) = history.iter().find(|candidate| {
            candidate.event_id == parent
                && candidate.schema_name.as_str() == "agent.model-response-received"
        }) else {
            return Err(NativeCodecError::InvalidShape(
                "native continuation parent is not a response fact".to_owned(),
            ));
        };
        let response_payload: Value = cairn_codec::from_slice(&response_event.payload)
            .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?;
        let parent_attempt: ModelAttemptId = serde_json::from_value(
            response_payload.get("attempt_id").cloned().ok_or_else(|| {
                NativeCodecError::InvalidShape("response fact has no attempt identity".to_owned())
            })?,
        )
        .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?;
        let parent_response: ContentId<ModelResponseArtifact> =
            serde_json::from_value(response_payload.get("response_id").cloned().ok_or_else(
                || {
                    NativeCodecError::InvalidShape(
                        "response fact has no response identity".to_owned(),
                    )
                },
            )?)
            .map_err(|error| NativeCodecError::InvalidShape(error.to_string()))?;
        if parent_attempt != payload.attempt_id || parent_response != payload.response_id {
            return Err(NativeCodecError::InvalidShape(
                "native continuation disagrees with its response parent".to_owned(),
            ));
        }
        let mut raw_response = Vec::new();
        content
            .write_to(&payload.response_id, &mut raw_response)
            .map_err(NativeCodecError::Storage)?;
        let continuation = self.recover(content, &payload.continuation_id)?;
        if continuation.source_response_ids.last() != Some(&payload.response_id) {
            return Err(NativeCodecError::InvalidShape(
                "native continuation does not end at its recorded response".to_owned(),
            ));
        }
        Ok(Some((payload.continuation_id, continuation)))
    }

    fn prepare_from_boundary(
        self,
        spec: &NativeRequestSpec,
        continuation: NativeContinuation,
    ) -> Result<PreparedNativeRequest, NativeCodecError> {
        let tools = native_tools(self.kind, &spec.tools);
        let mut body = match &continuation.history {
            NativeHistory::OpenAiResponses { input } => json!({
                "model": spec.wire_model.as_str(),
                "instructions": spec.instructions,
                "input": input,
                "tools": tools,
                "max_output_tokens": spec.max_output_tokens.get(),
                "store": false
            }),
            NativeHistory::OpenAiChatCompletions { messages } => json!({
                "model": spec.wire_model.as_str(),
                "messages": messages,
                "tools": tools,
                "max_tokens": spec.max_output_tokens.get()
            }),
            NativeHistory::AnthropicMessages { messages } => json!({
                "model": spec.wire_model.as_str(),
                "system": spec.instructions,
                "messages": messages,
                "tools": tools,
                "max_tokens": spec.max_output_tokens.get()
            }),
        };
        if self.kind == ModelProtocolKind::OpenAiResponses
            && self.responses_reasoning == ResponsesReasoningReplay::RequestEncryptedContent
        {
            body.as_object_mut()
                .expect("request body is an object")
                .insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
        }
        if self.kind == ModelProtocolKind::OpenAiChatCompletions && self.chat_thinking_parameter {
            body.as_object_mut()
                .expect("request body is an object")
                .insert("thinking".to_owned(), json!({"type": "enabled"}));
        }
        let bytes = serde_json::to_vec(&body)
            .map_err(|error| NativeCodecError::Serialization(error.to_string()))?;
        Ok(PreparedNativeRequest {
            bytes,
            base_continuation: continuation,
            offered_tools: spec.tools.iter().map(|tool| tool.name.clone()).collect(),
        })
    }

    fn validate(self, continuation: &NativeContinuation) -> Result<(), NativeCodecError> {
        if continuation.schema_version != NATIVE_CONTINUATION_SCHEMA_V1 {
            return Err(NativeCodecError::UnsupportedSchema(
                continuation.schema_version,
            ));
        }
        if continuation.protocol() != self.kind {
            return Err(NativeCodecError::ProtocolMismatch);
        }
        let mut pending = BTreeSet::new();
        for call_id in &continuation.pending_call_ids {
            if !pending.insert(call_id.as_str()) {
                return Err(NativeCodecError::DuplicateCallId(
                    call_id.as_str().to_owned(),
                ));
            }
        }
        match &continuation.history {
            NativeHistory::OpenAiResponses { input } => {
                validate_responses_history(input, self.responses_reasoning)?;
            }
            NativeHistory::OpenAiChatCompletions { messages } => {
                validate_chat_history(messages, self.chat_reasoning)?;
            }
            NativeHistory::AnthropicMessages { messages } => validate_anthropic_history(messages)?,
        }
        Ok(())
    }
}

/// Protocol-native continuation failure with no retry authority.
#[derive(Debug, Error)]
pub enum NativeCodecError {
    #[error("Responses hosted storage cannot be the V1 reconstruction authority")]
    HostedResponsesState,
    #[error("initial user input must not be empty")]
    MissingInitialInput,
    #[error("new user input must not be empty")]
    MissingUserInput,
    #[error("native continuation protocol does not match the selected codec")]
    ProtocolMismatch,
    #[error("native continuation schema {0} is unsupported")]
    UnsupportedSchema(u16),
    #[error("native request-state schema {0} is unsupported")]
    UnsupportedRequestSchema(u16),
    #[error("native continuation event schema {0} is unsupported")]
    UnsupportedEventSchema(u32),
    #[error("native continuation still has pending tool results")]
    PendingToolResults,
    #[error("raw response bytes do not match their typed content identity")]
    ResponseIdentityMismatch,
    #[error("native request bytes do not match their typed content identity")]
    RequestIdentityMismatch,
    #[error("model response has no archived protocol-native request context")]
    MissingNativeRequestState,
    #[error("provider JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("provider-native shape is invalid: {0}")]
    InvalidShape(String),
    #[error("provider-native call ID is duplicated: {0}")]
    DuplicateCallId(String),
    #[error("attempt has multiple recorded native continuations")]
    DuplicateRecordedContinuation,
    #[error("provider-native call references a tool not offered in this request: {0}")]
    UnknownTool(String),
    #[error("tool results do not exactly match all pending provider-native calls")]
    ToolResultMismatch,
    #[error("tool result must not be empty: {0}")]
    EmptyToolResult(String),
    #[error("tool result is not valid UTF-8: {0}")]
    InvalidToolResultEncoding(String),
    #[error("required reasoning/thinking continuation material is missing: {0}")]
    MissingThinkingState(String),
    #[error("tool definition is invalid: {0}")]
    InvalidTool(String),
    #[error("native continuation serialization failed: {0}")]
    Serialization(String),
    #[error("native continuation bytes are not the stable V1 encoding")]
    NonCanonicalContinuation,
    #[error("native request-state bytes are not the stable V1 encoding")]
    NonCanonicalRequestState,
    #[error("native request state does not match its prepared request fact")]
    RequestStateMismatch,
    #[error("native continuation identity derivation failed: {0}")]
    Identity(String),
    #[error("native continuation storage failed: {0}")]
    Storage(#[source] ContentStoreError),
    #[error("native continuation event storage failed: {0}")]
    Event(#[source] EventStoreError),
}

/// Failure while publishing the native replay boundary and semantic tool proposals together.
#[derive(Debug, Error)]
pub enum ProtocolDecodeCoordinatorError {
    /// Protocol-native validation or archival failed.
    #[error(transparent)]
    Native(#[from] NativeCodecError),
    /// Semantic materialization or archival failed.
    #[error(transparent)]
    Semantic(#[from] DecodeCoordinatorError),
    /// Both artifacts exist, but their indivisible fact batch could not be committed.
    #[error(
        "attempt {attempt_id} archived native continuation {continuation_id} and semantic turn {turn_id}, but recording their fact batch failed ({record})"
    )]
    UnrecordedTurn {
        /// Attempt whose durable raw response remains safe to decode again.
        attempt_id: ModelAttemptId,
        /// Recoverable provider-native replay artifact.
        continuation_id: ContentId<NativeContinuationArtifact>,
        /// Recoverable semantic artifact.
        turn_id: ContentId<SemanticModelTurnArtifact>,
        /// Event-store failure diagnostic.
        record: String,
    },
}

fn encode_continuation(continuation: &NativeContinuation) -> Result<Vec<u8>, NativeCodecError> {
    serde_json::to_vec(continuation)
        .map_err(|error| NativeCodecError::Serialization(error.to_string()))
}

fn validate_tools(tools: &[NativeToolDefinition]) -> Result<(), NativeCodecError> {
    let mut names = BTreeSet::new();
    for tool in tools {
        if tool.description.trim().is_empty() || !tool.input_schema.is_object() {
            return Err(NativeCodecError::InvalidTool(tool.name.as_str().to_owned()));
        }
        if !names.insert(&tool.name) {
            return Err(NativeCodecError::InvalidTool(tool.name.as_str().to_owned()));
        }
    }
    Ok(())
}

fn native_tools(kind: ModelProtocolKind, tools: &[NativeToolDefinition]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| match kind {
                ModelProtocolKind::OpenAiResponses => {
                    let mut object = Map::new();
                    object.insert("type".to_owned(), Value::String("function".to_owned()));
                    object.insert(
                        "name".to_owned(),
                        Value::String(tool.name.as_str().to_owned()),
                    );
                    object.insert(
                        "description".to_owned(),
                        Value::String(tool.description.clone()),
                    );
                    object.insert("parameters".to_owned(), tool.input_schema.clone());
                    if tool.strict {
                        object.insert("strict".to_owned(), Value::Bool(true));
                    }
                    Value::Object(object)
                }
                ModelProtocolKind::OpenAiChatCompletions => json!({
                    "type": "function",
                    "function": {
                        "name": tool.name.as_str(),
                        "description": tool.description,
                        "parameters": tool.input_schema,
                        "strict": tool.strict
                    }
                }),
                ModelProtocolKind::AnthropicMessages => json!({
                    "name": tool.name.as_str(),
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                    "strict": tool.strict
                }),
            })
            .collect(),
    )
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], NativeCodecError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| NativeCodecError::InvalidShape(format!("{field} must be an array")))
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, NativeCodecError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            NativeCodecError::InvalidShape(format!("{field} must be a non-empty string"))
        })
}

fn validate_offered_tool(offered: &BTreeSet<ToolName>, name: &str) -> Result<(), NativeCodecError> {
    if offered.iter().any(|offered| offered.as_str() == name) {
        Ok(())
    } else {
        Err(NativeCodecError::UnknownTool(name.to_owned()))
    }
}

fn collect_call(
    calls: &mut Vec<ProviderToolCallId>,
    seen: &mut BTreeSet<String>,
    id: &str,
) -> Result<(), NativeCodecError> {
    if !seen.insert(id.to_owned()) {
        return Err(NativeCodecError::DuplicateCallId(id.to_owned()));
    }
    calls.push(
        ProviderToolCallId::new(id)
            .map_err(|_| NativeCodecError::InvalidShape("call ID is invalid".to_owned()))?,
    );
    Ok(())
}

fn responses_calls(
    output: &[Value],
    offered: &BTreeSet<ToolName>,
    reasoning_policy: ResponsesReasoningReplay,
) -> Result<Vec<ProviderToolCallId>, NativeCodecError> {
    let mut calls = Vec::new();
    let mut seen = BTreeSet::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let id = required_string(item, "call_id")?;
                validate_offered_tool(offered, required_string(item, "name")?)?;
                let _ = required_string(item, "arguments")?;
                collect_call(&mut calls, &mut seen, id)?;
            }
            Some("reasoning") => {
                if let Some(encrypted) = item.get("encrypted_content") {
                    if encrypted.as_str().is_none() {
                        return Err(NativeCodecError::InvalidShape(
                            "reasoning.encrypted_content must be a string".to_owned(),
                        ));
                    }
                } else if reasoning_policy == ResponsesReasoningReplay::RequestEncryptedContent {
                    return Err(NativeCodecError::MissingThinkingState(
                        "Responses reasoning item has no encrypted_content".to_owned(),
                    ));
                }
            }
            Some(_) => {}
            None => {
                return Err(NativeCodecError::InvalidShape(
                    "Responses output item has no type".to_owned(),
                ));
            }
        }
    }
    Ok(calls)
}

fn responses_turn(
    output: &[Value],
    offered: &BTreeSet<ToolName>,
    reasoning_policy: ResponsesReasoningReplay,
) -> Result<(Vec<ProviderToolCallId>, Vec<AdapterOutputItem>), NativeCodecError> {
    let calls = responses_calls(output, offered, reasoning_policy)?;
    let mut semantic = Vec::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for content in required_array(item, "content")? {
                    match content.get("type").and_then(Value::as_str) {
                        Some("output_text") => semantic.push(AdapterOutputItem::Text {
                            text: required_string(content, "text")?.to_owned(),
                        }),
                        Some("refusal") => semantic.push(AdapterOutputItem::Text {
                            text: required_string(content, "refusal")?.to_owned(),
                        }),
                        Some(_) => {}
                        None => {
                            return Err(NativeCodecError::InvalidShape(
                                "Responses message content has no type".to_owned(),
                            ));
                        }
                    }
                }
            }
            Some("function_call") => semantic.push(AdapterOutputItem::ToolCall {
                provider_call_id: provider_call_id(required_string(item, "call_id")?)?,
                tool: tool_name(required_string(item, "name")?)?,
                arguments: json_object_string(required_string(item, "arguments")?)?,
            }),
            Some(_) => {}
            None => unreachable!("responses_calls validated item types"),
        }
    }
    Ok((calls, semantic))
}

fn chat_calls(
    message: &Value,
    offered: &BTreeSet<ToolName>,
    reasoning_policy: ChatReasoningReplay,
) -> Result<Vec<ProviderToolCallId>, NativeCodecError> {
    let Some(tool_calls) = message.get("tool_calls") else {
        return Ok(Vec::new());
    };
    if tool_calls.is_null() {
        return Ok(Vec::new());
    }
    let tool_calls = tool_calls.as_array().ok_or_else(|| {
        NativeCodecError::InvalidShape("Chat message.tool_calls must be an array".to_owned())
    })?;
    if !tool_calls.is_empty()
        && reasoning_policy == ChatReasoningReplay::RequiredWithToolCalls
        && message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(NativeCodecError::MissingThinkingState(
            "DeepSeek Chat tool call requires reasoning_content".to_owned(),
        ));
    }
    let mut calls = Vec::new();
    let mut seen = BTreeSet::new();
    for call in tool_calls {
        if call.get("type").and_then(Value::as_str) != Some("function") {
            return Err(NativeCodecError::InvalidShape(
                "Chat tool call must have type=function".to_owned(),
            ));
        }
        let function = call.get("function").ok_or_else(|| {
            NativeCodecError::InvalidShape("Chat tool call has no function".to_owned())
        })?;
        let id = required_string(call, "id")?;
        validate_offered_tool(offered, required_string(function, "name")?)?;
        let _ = required_string(function, "arguments")?;
        collect_call(&mut calls, &mut seen, id)?;
    }
    Ok(calls)
}

fn chat_turn(
    message: &Value,
    offered: &BTreeSet<ToolName>,
    reasoning_policy: ChatReasoningReplay,
) -> Result<(Vec<ProviderToolCallId>, Vec<AdapterOutputItem>), NativeCodecError> {
    let calls = chat_calls(message, offered, reasoning_policy)?;
    let mut semantic = Vec::new();
    if let Some(content) = message.get("content") {
        if !content.is_null() {
            let text = content.as_str().ok_or_else(|| {
                NativeCodecError::InvalidShape(
                    "Chat message.content must be a string or null".to_owned(),
                )
            })?;
            if !text.is_empty() {
                semantic.push(AdapterOutputItem::Text {
                    text: text.to_owned(),
                });
            }
        }
    }
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
        if !refusal.is_empty() {
            semantic.push(AdapterOutputItem::Text {
                text: refusal.to_owned(),
            });
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let function = call.get("function").expect("chat_calls validated function");
            semantic.push(AdapterOutputItem::ToolCall {
                provider_call_id: provider_call_id(required_string(call, "id")?)?,
                tool: tool_name(required_string(function, "name")?)?,
                arguments: json_object_string(required_string(function, "arguments")?)?,
            });
        }
    }
    Ok((calls, semantic))
}

fn anthropic_calls(
    content: &[Value],
    offered: &BTreeSet<ToolName>,
) -> Result<Vec<ProviderToolCallId>, NativeCodecError> {
    let mut calls = Vec::new();
    let mut seen = BTreeSet::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                let _ = required_string(block, "thinking")?;
                let _ = required_string(block, "signature").map_err(|_| {
                    NativeCodecError::MissingThinkingState(
                        "Anthropic thinking block has no signature".to_owned(),
                    )
                })?;
            }
            Some("redacted_thinking") => {
                let _ = required_string(block, "data").map_err(|_| {
                    NativeCodecError::MissingThinkingState(
                        "Anthropic redacted_thinking block has no opaque data".to_owned(),
                    )
                })?;
            }
            Some("tool_use") => {
                let id = required_string(block, "id")?;
                validate_offered_tool(offered, required_string(block, "name")?)?;
                if !block.get("input").is_some_and(Value::is_object) {
                    return Err(NativeCodecError::InvalidShape(
                        "Anthropic tool_use.input must be an object".to_owned(),
                    ));
                }
                collect_call(&mut calls, &mut seen, id)?;
            }
            Some(_) => {}
            None => {
                return Err(NativeCodecError::InvalidShape(
                    "Anthropic content block has no type".to_owned(),
                ));
            }
        }
    }
    Ok(calls)
}

fn anthropic_turn(
    content: &[Value],
    offered: &BTreeSet<ToolName>,
) -> Result<(Vec<ProviderToolCallId>, Vec<AdapterOutputItem>), NativeCodecError> {
    let calls = anthropic_calls(content, offered)?;
    let mut semantic = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => semantic.push(AdapterOutputItem::Text {
                text: required_string(block, "text")?.to_owned(),
            }),
            Some("tool_use") => semantic.push(AdapterOutputItem::ToolCall {
                provider_call_id: provider_call_id(required_string(block, "id")?)?,
                tool: tool_name(required_string(block, "name")?)?,
                arguments: block.get("input").cloned().expect("tool input validated"),
            }),
            Some(_) => {}
            None => unreachable!("anthropic_calls validated block types"),
        }
    }
    Ok((calls, semantic))
}

fn provider_call_id(value: &str) -> Result<ProviderToolCallId, NativeCodecError> {
    ProviderToolCallId::new(value)
        .map_err(|_| NativeCodecError::InvalidShape("call ID is invalid".to_owned()))
}

fn tool_name(value: &str) -> Result<ToolName, NativeCodecError> {
    ToolName::new(value)
        .map_err(|_| NativeCodecError::InvalidShape("tool name is invalid".to_owned()))
}

fn json_object_string(value: &str) -> Result<Value, NativeCodecError> {
    let arguments: Value = serde_json::from_str(value).map_err(|error| {
        NativeCodecError::InvalidShape(format!("tool arguments are invalid JSON: {error}"))
    })?;
    if !arguments.is_object() {
        return Err(NativeCodecError::InvalidShape(
            "tool arguments must decode to an object".to_owned(),
        ));
    }
    Ok(arguments)
}

fn correlate_results<'a>(
    pending: &[ProviderToolCallId],
    results: &'a [NativeToolResult],
) -> Result<BTreeMap<&'a str, &'a str>, NativeCodecError> {
    if pending.len() != results.len() {
        return Err(NativeCodecError::ToolResultMismatch);
    }
    let pending = pending
        .iter()
        .map(ProviderToolCallId::as_str)
        .collect::<BTreeSet<_>>();
    let mut correlated = BTreeMap::new();
    for result in results {
        if result.output.is_empty() {
            return Err(NativeCodecError::EmptyToolResult(
                result.call_id.as_str().to_owned(),
            ));
        }
        if !pending.contains(result.call_id.as_str())
            || correlated
                .insert(result.call_id.as_str(), result.output.as_str())
                .is_some()
        {
            return Err(NativeCodecError::ToolResultMismatch);
        }
    }
    Ok(correlated)
}

fn validate_responses_history(
    input: &[Value],
    policy: ResponsesReasoningReplay,
) -> Result<(), NativeCodecError> {
    for item in input {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            if let Some(encrypted) = item.get("encrypted_content") {
                if encrypted.as_str().is_none() {
                    return Err(NativeCodecError::InvalidShape(
                        "reasoning.encrypted_content must be a string".to_owned(),
                    ));
                }
            } else if policy == ResponsesReasoningReplay::RequestEncryptedContent {
                return Err(NativeCodecError::MissingThinkingState(
                    "Responses reasoning item has no encrypted_content".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_chat_history(
    messages: &[Value],
    policy: ChatReasoningReplay,
) -> Result<(), NativeCodecError> {
    for message in messages {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            // History validation checks reasoning shape; offered-name validation happened at decode.
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                if !calls.is_empty()
                    && policy == ChatReasoningReplay::RequiredWithToolCalls
                    && message
                        .get("reasoning_content")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                {
                    return Err(NativeCodecError::MissingThinkingState(
                        "DeepSeek Chat history lost reasoning_content".to_owned(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_anthropic_history(messages: &[Value]) -> Result<(), NativeCodecError> {
    for message in messages {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            let content = required_array(message, "content")?;
            for block in content {
                match block.get("type").and_then(Value::as_str) {
                    Some("thinking") => {
                        let _ = required_string(block, "thinking")?;
                        let _ = required_string(block, "signature").map_err(|_| {
                            NativeCodecError::MissingThinkingState(
                                "Anthropic thinking history lost its signature".to_owned(),
                            )
                        })?;
                    }
                    Some("redacted_thinking") => {
                        let _ = required_string(block, "data").map_err(|_| {
                            NativeCodecError::MissingThinkingState(
                                "Anthropic redacted history lost its opaque data".to_owned(),
                            )
                        })?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::ContentId;
    use cairn_record::ContentStore;
    use cairn_store_sqlite::SqliteContentStore;

    use super::{
        NativeCodecError, NativeProtocolCodec, NativeRequestSpec, NativeRequestState,
        NativeToolDefinition, NativeToolResult, validate_native_request_state_reference,
    };
    use crate::{
        AdapterOutputItem, ChatReasoningReplay, MaterializedRequestArtifact, ModelName,
        ModelOutputTokenLimit, ModelProtocolConfig, ModelResponseArtifact,
        NativeRequestStateArtifact, ProviderToolCallId, ResponsesReasoningReplay, ToolName,
    };

    fn spec() -> NativeRequestSpec {
        NativeRequestSpec {
            wire_model: ModelName::new("fixture-model").expect("model"),
            instructions: "Use tools when needed.".to_owned(),
            tools: vec![NativeToolDefinition {
                name: ToolName::new("lookup").expect("tool"),
                description: "Look up one key".to_owned(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"key": {"type": "string"}},
                    "required": ["key"]
                }),
                strict: true,
            }],
            max_output_tokens: ModelOutputTokenLimit::new(2048).expect("tokens"),
        }
    }

    fn tool_result(call_id: &str) -> NativeToolResult {
        NativeToolResult {
            call_id: ProviderToolCallId::new(call_id).expect("call id"),
            output: "fixture-result".to_owned(),
        }
    }

    fn response_id(bytes: &[u8]) -> ContentId<ModelResponseArtifact> {
        ContentId::derive(bytes).expect("response id")
    }

    fn round_trip(
        codec: NativeProtocolCodec,
        response: &[u8],
        call_id: &str,
        assertion: impl Fn(&serde_json::Value),
    ) {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("content.db");
        let cas = directory.path().join("cas");
        let spec = spec();
        let initial = codec
            .prepare_initial(&spec, "find the value")
            .expect("initial request");
        let decoded = codec
            .decode_turn(&initial, response_id(response), response)
            .expect("decode response");
        assert!(matches!(
            decoded.semantic().items.as_slice(),
            [AdapterOutputItem::ToolCall {
                provider_call_id,
                tool,
                arguments,
            }] if provider_call_id.as_str() == call_id
                && tool.as_str() == "lookup"
                && arguments == &serde_json::json!({"key":"x"})
        ));
        let continuation = decoded.continuation().clone();
        let settled = codec
            .append_tool_results(&continuation, &[tool_result(call_id)])
            .expect("append result");
        let before_restart = codec
            .prepare_continuation(&spec, &settled)
            .expect("prepare before restart");
        let continuation_id = {
            let mut store = SqliteContentStore::open(&database, &cas).expect("open store");
            codec.archive(&mut store, &settled).expect("archive")
        };
        let store = SqliteContentStore::open(database, cas).expect("reopen store");
        let recovered = codec.recover(&store, &continuation_id).expect("recover");
        let after_restart = codec
            .prepare_continuation(&spec, &recovered)
            .expect("prepare after restart");
        assert_eq!(settled, recovered);
        assert_eq!(
            before_restart.request_bytes(),
            after_restart.request_bytes()
        );
        let request: serde_json::Value =
            serde_json::from_slice(after_restart.request_bytes()).expect("request JSON");
        assertion(&request);
    }

    #[test]
    fn responses_continuation_survives_restart_with_reasoning_item() {
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let response = br#"{
            "id":"resp_fixture",
            "output":[
                {"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque-state"},
                {"type":"function_call","call_id":"call_responses","name":"lookup","arguments":"{\"key\":\"x\"}"}
            ]
        }"#;
        round_trip(codec, response, "call_responses", |request| {
            let input = request["input"].as_array().expect("Responses input");
            assert_eq!(input[1]["type"], "reasoning");
            assert_eq!(input[1]["encrypted_content"], "opaque-state");
            assert_eq!(input[2]["call_id"], "call_responses");
            assert_eq!(input[3]["type"], "function_call_output");
            assert_eq!(request["store"], false);
        });
    }

    #[test]
    fn responses_completed_turn_accepts_a_new_user_message_without_losing_phase() {
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let initial = codec
            .prepare_initial(&spec(), "first turn")
            .expect("initial");
        let response = br#"{
            "output":[
                {"type":"reasoning","id":"rs_1","status":"completed","summary":[],"content":[{"type":"reasoning_text","text":"private"}],"encrypted_content":"opaque"},
                {"type":"message","id":"msg_1","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"first"}]}
            ]
        }"#;
        let continuation = codec
            .decode_response(&initial, response_id(response), response)
            .expect("decode");
        let next = codec
            .append_user_text(&continuation, "second turn")
            .expect("append user turn");
        let request = codec
            .prepare_continuation(&spec(), &next)
            .expect("prepare continuation");
        let body: serde_json::Value =
            serde_json::from_slice(request.request_bytes()).expect("request JSON");
        assert_eq!(body["input"][2]["phase"], "final_answer");
        assert_eq!(body["input"][3]["role"], "user");
        assert_eq!(body["input"][3]["content"][0]["text"], "second turn");
    }

    #[test]
    fn chat_continuation_survives_restart_with_deepseek_reasoning_content() {
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiChatCompletions {
            thinking_parameter: true,
            reasoning_replay: ChatReasoningReplay::RequiredWithToolCalls,
        })
        .expect("codec");
        let response = br#"{
            "id":"chat_fixture",
            "choices":[{"message":{
                "role":"assistant",
                "content":null,
                "reasoning_content":"deepseek-thinking-state",
                "tool_calls":[{"id":"call_chat","type":"function","function":{"name":"lookup","arguments":"{\"key\":\"x\"}"}}]
            }}]
        }"#;
        round_trip(codec, response, "call_chat", |request| {
            let messages = request["messages"].as_array().expect("Chat messages");
            assert_eq!(messages[2]["reasoning_content"], "deepseek-thinking-state");
            assert_eq!(messages[3]["tool_call_id"], "call_chat");
            assert_eq!(request["thinking"]["type"], "enabled");
        });
    }

    #[test]
    fn anthropic_continuation_survives_restart_without_mutating_thinking_blocks() {
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::AnthropicMessages {
            api_version: "2023-06-01".to_owned(),
        })
        .expect("codec");
        let response = br#"{
            "id":"msg_fixture","type":"message","role":"assistant",
            "content":[
                {"type":"thinking","thinking":"private thought","signature":"signed-state"},
                {"type":"redacted_thinking","data":"redacted-state"},
                {"type":"tool_use","id":"call_anthropic","name":"lookup","input":{"key":"x"}}
            ],
            "stop_reason":"tool_use"
        }"#;
        round_trip(codec, response, "call_anthropic", |request| {
            let messages = request["messages"].as_array().expect("Anthropic messages");
            let blocks = messages[1]["content"].as_array().expect("assistant blocks");
            assert_eq!(blocks[0]["type"], "thinking");
            assert_eq!(blocks[0]["signature"], "signed-state");
            assert_eq!(blocks[1]["type"], "redacted_thinking");
            assert_eq!(blocks[2]["id"], "call_anthropic");
            assert_eq!(messages[2]["content"][0]["tool_use_id"], "call_anthropic");
        });
    }

    #[test]
    fn required_thinking_state_fails_before_a_bad_followup_can_be_sent() {
        let chat = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiChatCompletions {
            thinking_parameter: true,
            reasoning_replay: ChatReasoningReplay::RequiredWithToolCalls,
        })
        .expect("chat codec");
        let initial = chat
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let missing_chat_reasoning = br#"{"choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call_chat","type":"function","function":{"name":"lookup","arguments":"{}"}}]}}]}"#;
        assert!(matches!(
            chat.decode_response(
                &initial,
                response_id(missing_chat_reasoning),
                missing_chat_reasoning
            ),
            Err(NativeCodecError::MissingThinkingState(_))
        ));

        let anthropic = NativeProtocolCodec::from_config(&ModelProtocolConfig::AnthropicMessages {
            api_version: "2023-06-01".to_owned(),
        })
        .expect("Anthropic codec");
        let initial = anthropic
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let missing_signature = br#"{"type":"message","role":"assistant","content":[{"type":"thinking","thinking":"thought"},{"type":"tool_use","id":"call_a","name":"lookup","input":{}}]}"#;
        assert!(matches!(
            anthropic.decode_response(&initial, response_id(missing_signature), missing_signature),
            Err(NativeCodecError::MissingThinkingState(_))
        ));

        let responses = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::RequestEncryptedContent,
        })
        .expect("Responses codec");
        let initial = responses
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let missing_encrypted = br#"{"output":[{"type":"reasoning","id":"rs_1","summary":[]}]}"#;
        assert!(matches!(
            responses.decode_response(&initial, response_id(missing_encrypted), missing_encrypted),
            Err(NativeCodecError::MissingThinkingState(_))
        ));
        let request: serde_json::Value =
            serde_json::from_slice(initial.request_bytes()).expect("request");
        assert_eq!(request["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn malformed_tool_arguments_fail_before_semantic_proposals_exist() {
        let responses = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("Responses codec");
        let initial = responses
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let invalid_json = br#"{"output":[{"type":"function_call","call_id":"call-r","name":"lookup","arguments":"{"}]}"#;
        assert!(matches!(
            responses.decode_turn(&initial, response_id(invalid_json), invalid_json),
            Err(NativeCodecError::InvalidShape(_))
        ));

        let chat = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiChatCompletions {
            thinking_parameter: false,
            reasoning_replay: ChatReasoningReplay::PreserveIfPresent,
        })
        .expect("Chat codec");
        let initial = chat
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let non_object = br#"{"choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call-c","type":"function","function":{"name":"lookup","arguments":"[]"}}]}}]}"#;
        assert!(matches!(
            chat.decode_turn(&initial, response_id(non_object), non_object),
            Err(NativeCodecError::InvalidShape(_))
        ));

        let anthropic = NativeProtocolCodec::from_config(&ModelProtocolConfig::AnthropicMessages {
            api_version: "2023-06-01".to_owned(),
        })
        .expect("Anthropic codec");
        let initial = anthropic
            .prepare_initial(&spec(), "find the value")
            .expect("initial");
        let non_object = br#"{"type":"message","role":"assistant","content":[{"type":"tool_use","id":"call-a","name":"lookup","input":[]}]}"#;
        assert!(matches!(
            anthropic.decode_turn(&initial, response_id(non_object), non_object),
            Err(NativeCodecError::InvalidShape(_))
        ));
    }

    #[test]
    fn native_request_state_cannot_be_rebound_to_different_request_bytes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let prepared = codec
            .prepare_initial(&spec(), "find the value")
            .expect("request");
        let original_request_id = content
            .put::<MaterializedRequestArtifact>(&mut Cursor::new(prepared.request_bytes()))
            .expect("request bytes")
            .content_id;
        let state = NativeRequestState {
            schema_version: 1,
            protocol: codec.kind(),
            request_id: original_request_id,
            base_continuation: prepared.base_continuation.clone(),
            offered_tools: prepared.offered_tools.clone(),
        };
        let state_id = content
            .put::<NativeRequestStateArtifact>(&mut Cursor::new(
                serde_json::to_vec(&state).expect("state bytes"),
            ))
            .expect("state")
            .content_id;
        let different_request_id = content
            .put::<MaterializedRequestArtifact>(&mut Cursor::new(b"different"))
            .expect("different request")
            .content_id;
        assert!(matches!(
            validate_native_request_state_reference(&content, &state_id, different_request_id),
            Err(NativeCodecError::RequestStateMismatch)
        ));
    }
}
