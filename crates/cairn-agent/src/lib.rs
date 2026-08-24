//! Domain-neutral model-input projection, durable dispatch, and transport capabilities.

use std::{collections::VecDeque, io::Cursor};

use cairn_protocol::{ContentId, ContentType};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod dispatch;
mod operation;
mod semantic;
mod step;
mod step_operation;

pub use dispatch::{
    DispatchAuthority, DispatchCompletion, DispatchCoordinatorError, ModelAttemptState,
    ReceivedModelResponse, StartedDispatch, authorize_model_request, begin_model_dispatch,
    execute_model_dispatch, recover_dispatch_authority, recover_model_attempt,
    recover_received_model_response,
};
pub use operation::{
    CanonicalToolResult, OperationCoordinatorError, OperationRecovery, PreparedToolOperation,
    RecordedToolExchange, RecordedToolGateway, ScriptedToolGateway, StartedToolOperation,
    ToolEffectClass, ToolGateway, ToolGatewayError, ToolGatewayFailureClass,
    ToolOperationAuthority, ToolOperationCompletion, ToolOperationState, ToolResultEncodingError,
    authorize_tool_operation, authorize_tool_operation_retry, begin_tool_operation,
    execute_tool_operation, prepare_tool_operation, reconcile_tool_operation_completed,
    reconcile_tool_operation_not_occurred, recover_tool_operation,
    recover_tool_operation_authority,
};
pub use semantic::{
    AdapterModelTurn, AdapterOutputItem, DecodeCoordinatorError, DecodedModelTurn, ModelAdapter,
    ModelAdapterError, OutputOrdinal, RecordedAdapterExchange, RecordedModelAdapter,
    ScriptedModelAdapter, SemanticModelTurn, SemanticOutputItem, ToolCallId, ToolCallProposal,
    decode_model_response, recover_decoded_model_turn,
};
pub use step::{
    AgentStep, AgentStepState, SettledAgentStep, StepCoordinatorError, prepare_agent_step,
    recover_agent_step, settle_decoded_step,
};
pub use step_operation::{
    BoundStepOperations, StepOperationBlocker, StepOperationSettlement, ToolOperationAssignment,
    ToolRegistration, bind_step_operations, settle_step_operations,
};

macro_rules! label_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Creates a non-empty agent-boundary label.
            ///
            /// # Errors
            ///
            /// Returns [`InputTypeError`] for empty, surrounding-whitespace, or control-containing
            /// values.
            pub fn new(value: impl Into<String>) -> Result<Self, InputTypeError> {
                let value = value.into();
                if value.is_empty()
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(InputTypeError);
                }
                Ok(Self(value))
            }
        }

        impl TryFrom<String> for $name {
            type Error = InputTypeError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

/// Invalid agent-boundary label.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("agent label must be non-empty, trimmed, and contain no control characters")]
pub struct InputTypeError;

label_type!(/// Model-provider identity.
ProviderName);
label_type!(/// Provider model identity.
ModelName);
label_type!(/// Provider deployment identity.
DeploymentName);
label_type!(/// Semantic adapter version.
AdapterVersion);
label_type!(/// Registered tool identity.
ToolName);
label_type!(/// Registered tool implementation version.
ToolImplementationVersion);
label_type!(/// Provider-native tool-call correlation identity.
ProviderToolCallId);

macro_rules! content_type {
    ($name:ident, $domain:literal) => {
        /// Marker for a durable semantic content domain.
        pub struct $name;
        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

content_type!(InstructionBlock, "agent.instruction-block.v1");
content_type!(ToolCatalog, "agent.tool-catalog.v1");
content_type!(HistoryItem, "agent.history-item.v1");
content_type!(ContextBlock, "agent.context-block.v1");
content_type!(OperationResult, "agent.operation-result.v1");
content_type!(
    OperationReconciliationEvidence,
    "agent.operation-reconciliation-evidence.v1"
);
content_type!(PolicyDocument, "agent.policy-document.v1");
content_type!(TurnInputDecisionArtifact, "agent.turn-input-decision.v1");
content_type!(MaterializedRequestArtifact, "agent.materialized-request.v1");
content_type!(ModelResponseArtifact, "agent.model-response.v1");
content_type!(ToolArguments, "agent.tool-arguments.v1");
content_type!(SemanticModelTurnArtifact, "agent.semantic-model-turn.v1");
content_type!(ToolCallIdentity, "agent.tool-call-identity.v1");

/// Pinned provider/model/adapter selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    /// Provider family.
    pub provider: ProviderName,
    /// Model identifier.
    pub model: ModelName,
    /// Deployment/account-visible endpoint label.
    pub deployment: DeploymentName,
    /// Adapter semantics used to materialize bytes.
    pub adapter_version: AdapterVersion,
}

/// Complete ordered selection decision made before provider dispatch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnInputDecision {
    /// Pinned provider selection.
    pub selection: ModelSelection,
    /// Ordered system/developer instruction blocks.
    pub instructions: Vec<ContentId<InstructionBlock>>,
    /// Exact tool catalog and schemas.
    pub tool_catalog: ContentId<ToolCatalog>,
    /// Ordered semantic history.
    pub history: Vec<ContentId<HistoryItem>>,
    /// Ordered injected context.
    pub context: Vec<ContentId<ContextBlock>>,
    /// Ordered pending operation outcomes.
    pub pending_results: Vec<ContentId<OperationResult>>,
    /// Role/data/approval policy visible to projection.
    pub policy: ContentId<PolicyDocument>,
}

/// Typed completeness-gap category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputGapKind {
    /// Referenced semantic content has no metadata binding.
    MissingContent,
    /// Stored bytes fail identity verification.
    IntegrityMismatch,
    /// Storage could not complete the audit.
    StorageUnavailable,
}

/// One unresolved input reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputGap {
    /// Decision field containing the reference.
    pub role: &'static str,
    /// Tagged semantic identity.
    pub content_id: String,
    /// Failure category.
    pub kind: InputGapKind,
    /// Adapter diagnostic retained for repair/audit.
    pub diagnostic: String,
}

/// Failed backwards walk. No replacement bytes are synthesized.
#[derive(Debug, Error)]
#[error("model input completeness audit found {gaps_len} gap(s)")]
pub struct InputAuditError {
    /// Number cached for concise display.
    gaps_len: usize,
    /// Every observed gap, not merely the first.
    pub gaps: Vec<InputGap>,
}

#[derive(Serialize)]
struct MaterializedRequestBody {
    selection: ModelSelection,
    instructions: Vec<serde_json::Value>,
    tool_catalog: serde_json::Value,
    history: Vec<serde_json::Value>,
    context: Vec<serde_json::Value>,
    pending_results: Vec<serde_json::Value>,
    policy: serde_json::Value,
}

/// Durable decision and exact canonical request identities prepared before dispatch. Construction
/// is restricted to [`prepare_model_request`] so dispatch authority cannot wrap unaudited bytes.
///
/// ```compile_fail
/// use cairn_agent::PreparedModelRequest;
///
/// let forged = PreparedModelRequest {
///     decision_id: todo!(),
///     request_id: todo!(),
///     adapter_version: todo!(),
///     request_bytes: Vec::new(),
/// };
/// ```
#[derive(Clone, Debug)]
pub struct PreparedModelRequest {
    /// Stored input decision.
    decision_id: ContentId<TurnInputDecisionArtifact>,
    /// Stored exact provider-neutral request bytes.
    request_id: ContentId<MaterializedRequestArtifact>,
    /// Semantic adapter version pinned when the input decision was made.
    adapter_version: AdapterVersion,
    /// Exact bytes supplied to the transport adapter.
    request_bytes: Vec<u8>,
}

impl PreparedModelRequest {
    /// Returns the durable input-decision identity.
    #[must_use]
    pub const fn decision_id(&self) -> ContentId<TurnInputDecisionArtifact> {
        self.decision_id
    }

    /// Returns the identity of the exact materialized request bytes.
    #[must_use]
    pub const fn request_id(&self) -> ContentId<MaterializedRequestArtifact> {
        self.request_id
    }

    /// Returns the semantic adapter version pinned before provider dispatch.
    #[must_use]
    pub fn adapter_version(&self) -> &AdapterVersion {
        &self.adapter_version
    }

    /// Returns the exact bytes to pass to a transport adapter.
    #[must_use]
    pub fn request_bytes(&self) -> &[u8] {
        &self.request_bytes
    }
}

/// Persists the decision, walks every reference, and materializes exact canonical request bytes.
///
/// # Errors
///
/// Returns [`InputAuditError`] containing all missing, corrupt, or unavailable inputs. The decision
/// remains durable even when materialization fails.
pub fn prepare_model_request<S: ContentStore>(
    store: &mut S,
    decision: &TurnInputDecision,
) -> Result<PreparedModelRequest, InputAuditError> {
    let decision_bytes = cairn_codec::to_vec(decision)
        .map_err(|error| audit_from_codec("decision", error.to_string()))?;
    let decision_descriptor = store
        .put::<TurnInputDecisionArtifact>(&mut Cursor::new(&decision_bytes))
        .map_err(|error| audit_from_store("decision", "unmaterialized", &error))?;

    let mut gaps = Vec::new();
    let instructions = resolve_many(store, "instructions", &decision.instructions, &mut gaps);
    let tool_catalog = resolve_one(store, "tool_catalog", &decision.tool_catalog, &mut gaps);
    let history = resolve_many(store, "history", &decision.history, &mut gaps);
    let context = resolve_many(store, "context", &decision.context, &mut gaps);
    let pending_results = resolve_many(
        store,
        "pending_results",
        &decision.pending_results,
        &mut gaps,
    );
    let policy = resolve_one(store, "policy", &decision.policy, &mut gaps);
    if !gaps.is_empty() {
        return Err(InputAuditError {
            gaps_len: gaps.len(),
            gaps,
        });
    }

    let (Some(tool_catalog), Some(policy)) = (tool_catalog, policy) else {
        return Err(InputAuditError {
            gaps_len: gaps.len(),
            gaps,
        });
    };
    let body = MaterializedRequestBody {
        selection: decision.selection.clone(),
        instructions,
        tool_catalog,
        history,
        context,
        pending_results,
        policy,
    };
    let request_bytes = cairn_codec::to_vec(&body)
        .map_err(|error| audit_from_codec("materialized_request", error.to_string()))?;
    let request_descriptor = store
        .put::<MaterializedRequestArtifact>(&mut Cursor::new(&request_bytes))
        .map_err(|error| audit_from_store("materialized_request", "unmaterialized", &error))?;
    Ok(PreparedModelRequest {
        decision_id: decision_descriptor.content_id,
        request_id: request_descriptor.content_id,
        adapter_version: decision.selection.adapter_version.clone(),
        request_bytes,
    })
}

fn resolve_many<T: ContentType, S: ContentStore>(
    store: &S,
    role: &'static str,
    ids: &[ContentId<T>],
    gaps: &mut Vec<InputGap>,
) -> Vec<serde_json::Value> {
    ids.iter()
        .filter_map(|id| resolve_one(store, role, id, gaps))
        .collect()
}

fn resolve_one<T: ContentType, S: ContentStore>(
    store: &S,
    role: &'static str,
    id: &ContentId<T>,
    gaps: &mut Vec<InputGap>,
) -> Option<serde_json::Value> {
    let mut bytes = Vec::new();
    match store.write_to(id, &mut bytes) {
        Ok(_) => match cairn_codec::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(error) => {
                gaps.push(InputGap {
                    role,
                    content_id: id.to_wire(),
                    kind: InputGapKind::IntegrityMismatch,
                    diagnostic: error.to_string(),
                });
                None
            }
        },
        Err(error) => {
            gaps.push(gap_from_store(role, id.to_wire(), &error));
            None
        }
    }
}

fn gap_from_store(role: &'static str, content_id: String, error: &ContentStoreError) -> InputGap {
    let kind = match error {
        ContentStoreError::NotFound { .. } => InputGapKind::MissingContent,
        ContentStoreError::Integrity { .. } => InputGapKind::IntegrityMismatch,
        ContentStoreError::Io { .. } | ContentStoreError::Metadata { .. } => {
            InputGapKind::StorageUnavailable
        }
    };
    InputGap {
        role,
        content_id,
        kind,
        diagnostic: error.to_string(),
    }
}

fn audit_from_store(
    role: &'static str,
    content_id: &str,
    error: &ContentStoreError,
) -> InputAuditError {
    InputAuditError {
        gaps_len: 1,
        gaps: vec![gap_from_store(role, content_id.to_owned(), error)],
    }
}

fn audit_from_codec(role: &'static str, diagnostic: String) -> InputAuditError {
    InputAuditError {
        gaps_len: 1,
        gaps: vec![InputGap {
            role,
            content_id: "unmaterialized".to_owned(),
            kind: InputGapKind::IntegrityMismatch,
            diagnostic,
        }],
    }
}

/// Provider transport failure.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Recorded exchange does not match the dispatched request.
    #[error("recorded request mismatch")]
    RequestMismatch,
    /// No scripted/recorded response remains.
    #[error("transport fixture is exhausted")]
    Exhausted,
    /// Provider call is known not to have left the process.
    #[error("model request was not sent: {0}")]
    NotSent(String),
    /// Provider definitively rejected the request without producing a response.
    #[error("model request was rejected: {0}")]
    Rejected(String),
    /// The caller cannot determine whether the provider executed the request.
    #[error("model request outcome is ambiguous: {0}")]
    Ambiguous(String),
    /// Scripted provider failure.
    #[error("scripted transport failed: {0}")]
    Scripted(String),
}

/// Durable classification used after a transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportFailureClass {
    /// Safe to authorize a distinct later attempt; this attempt did not leave the process.
    NotSent,
    /// Provider gave a definitive rejection.
    Rejected,
    /// External execution may have occurred and must not be blindly retried.
    Ambiguous,
}

impl TransportError {
    /// Classifies the external effect independently from diagnostic text.
    #[must_use]
    pub const fn failure_class(&self) -> TransportFailureClass {
        match self {
            Self::RequestMismatch | Self::Exhausted | Self::NotSent(_) => {
                TransportFailureClass::NotSent
            }
            Self::Rejected(_) => TransportFailureClass::Rejected,
            Self::Ambiguous(_) | Self::Scripted(_) => TransportFailureClass::Ambiguous,
        }
    }
}

/// Replaceable byte transport used identically by live, recorded, and scripted providers.
pub trait ModelTransport {
    /// Dispatches exact prepared bytes.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] for provider or fixture failure.
    fn dispatch(&mut self, request: &PreparedModelRequest) -> Result<Vec<u8>, TransportError>;
}

/// One exact request/response exchange used for deterministic byte replay.
pub struct RecordedExchange {
    /// Request identity required by this exchange.
    pub request_id: ContentId<MaterializedRequestArtifact>,
    /// Archived provider response bytes.
    pub response_bytes: Vec<u8>,
}

/// FIFO recorded provider; no replay flag is needed in an agent loop.
pub struct RecordedModelTransport {
    exchanges: VecDeque<RecordedExchange>,
}

impl RecordedModelTransport {
    /// Creates a recorded provider from ordered exchanges.
    #[must_use]
    pub fn new(exchanges: impl IntoIterator<Item = RecordedExchange>) -> Self {
        Self {
            exchanges: exchanges.into_iter().collect(),
        }
    }
}

impl ModelTransport for RecordedModelTransport {
    fn dispatch(&mut self, request: &PreparedModelRequest) -> Result<Vec<u8>, TransportError> {
        let exchange = self
            .exchanges
            .pop_front()
            .ok_or(TransportError::Exhausted)?;
        if exchange.request_id != request.request_id {
            return Err(TransportError::RequestMismatch);
        }
        Ok(exchange.response_bytes)
    }
}

/// Closure-backed provider for deterministic tests and fault injection.
pub struct ScriptedModelTransport<F>(F);

impl<F> ScriptedModelTransport<F> {
    /// Wraps a request-aware script.
    pub fn new(script: F) -> Self {
        Self(script)
    }
}

impl<F> ModelTransport for ScriptedModelTransport<F>
where
    F: FnMut(&PreparedModelRequest) -> Result<Vec<u8>, TransportError>,
{
    fn dispatch(&mut self, request: &PreparedModelRequest) -> Result<Vec<u8>, TransportError> {
        (self.0)(request)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{ContentId, ContentType};
    use cairn_record::ContentStore;
    use cairn_store_sqlite::SqliteContentStore;

    use super::{
        AdapterVersion, ContextBlock, DeploymentName, HistoryItem, InputGapKind, InstructionBlock,
        MaterializedRequestArtifact, ModelName, ModelSelection, ModelTransport, OperationResult,
        PolicyDocument, PreparedModelRequest, ProviderName, RecordedExchange,
        RecordedModelTransport, ScriptedModelTransport, ToolCatalog, TransportError,
        TurnInputDecision, prepare_model_request,
    };

    fn put_json<T: ContentType>(
        store: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(&value).expect("encode fixture");
        store
            .put::<T>(&mut Cursor::new(bytes))
            .expect("store fixture")
            .content_id
    }

    fn selection() -> ModelSelection {
        ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("fixture-model").expect("model"),
            deployment: DeploymentName::new("local").expect("deployment"),
            adapter_version: AdapterVersion::new("v1").expect("adapter"),
        }
    }

    fn complete_decision(store: &mut SqliteContentStore) -> TurnInputDecision {
        TurnInputDecision {
            selection: selection(),
            instructions: vec![put_json::<InstructionBlock>(
                store,
                &serde_json::json!({"text":"be exact"}),
            )],
            tool_catalog: put_json::<ToolCatalog>(store, &serde_json::json!({"tools":[]})),
            history: vec![put_json::<HistoryItem>(
                store,
                &serde_json::json!({"role":"user","content":"work"}),
            )],
            context: vec![put_json::<ContextBlock>(
                store,
                &serde_json::json!({"kind":"source","text":"input"}),
            )],
            pending_results: vec![put_json::<OperationResult>(
                store,
                &serde_json::json!({"operation":"one","result":"ok"}),
            )],
            policy: put_json::<PolicyDocument>(store, &serde_json::json!({"network":"deny"})),
        }
    }

    #[test]
    fn restart_reconstructs_byte_identical_request() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("agent.db");
        let cas = directory.path().join("cas");
        let (decision, first) = {
            let mut store = SqliteContentStore::open(&database, &cas).expect("store");
            let decision = complete_decision(&mut store);
            let prepared = prepare_model_request(&mut store, &decision).expect("prepare");
            (decision, prepared)
        };
        let mut store = SqliteContentStore::open(database, cas).expect("reopen");
        let second = prepare_model_request(&mut store, &decision).expect("reconstruct");
        assert_eq!(first.decision_id, second.decision_id);
        assert_eq!(first.request_id, second.request_id);
        assert_eq!(first.request_bytes, second.request_bytes);
    }

    #[test]
    fn completeness_audit_reports_missing_reference_without_synthesis() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut store = SqliteContentStore::open(
            directory.path().join("agent.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let mut decision = complete_decision(&mut store);
        decision.history.push(
            ContentId::<HistoryItem>::derive(b"{\"missing\":true}").expect("missing identity"),
        );
        let error = prepare_model_request(&mut store, &decision).expect_err("audit must fail");
        assert_eq!(error.gaps.len(), 1);
        assert_eq!(error.gaps[0].role, "history");
        assert_eq!(error.gaps[0].kind, InputGapKind::MissingContent);
    }

    #[test]
    fn recorded_and_scripted_transports_share_the_same_seam() {
        let request_id =
            ContentId::<MaterializedRequestArtifact>::derive(b"{}").expect("request identity");
        let request = PreparedModelRequest {
            decision_id: ContentId::derive(b"{}").expect("decision identity"),
            request_id,
            adapter_version: AdapterVersion::new("v1").expect("adapter"),
            request_bytes: b"{}".to_vec(),
        };
        let mut recorded = RecordedModelTransport::new([RecordedExchange {
            request_id,
            response_bytes: b"recorded".to_vec(),
        }]);
        assert_eq!(recorded.dispatch(&request).expect("recorded"), b"recorded");

        let mut first = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"first-live-result".to_vec())
        });
        let mut second = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"different-live-result".to_vec())
        });
        assert_ne!(
            first.dispatch(&request).expect("first"),
            second.dispatch(&request).expect("second")
        );
    }

    #[test]
    fn persisted_labels_cannot_bypass_validation() {
        assert!(serde_json::from_str::<ProviderName>("\" valid-looking \"").is_err());
        assert!(serde_json::from_str::<ModelName>("\"\"").is_err());
    }
}
