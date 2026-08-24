use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EventId, ModelAttemptId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StepId,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventStore, EventStoreError, ExpectedRevision, NewEvent,
    StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::step_operation::{StepOperationProjection, project_step_operations};
use crate::{
    DecodeCoordinatorError, DecodedModelTurn, DispatchAuthority, DispatchCoordinatorError,
    InputAuditError, ModelAttemptState, ReceivedModelResponse, SemanticModelTurnArtifact,
    ToolCallId, ToolCallProposal, TurnInputDecision, authorize_model_request,
    prepare_model_request, recover_decoded_model_turn, recover_dispatch_authority,
    recover_model_attempt, recover_received_model_response,
};

const STEP_SETTLED: &str = "agent.step-settled";
const RESPONSE_DECODED: &str = "agent.model-response-decoded";

/// Typed aggregate boundary for one model request and the operations it proposes.
///
/// ```compile_fail
/// use cairn_agent::AgentStep;
/// use cairn_protocol::ModelAttemptId;
///
/// let _step = AgentStep::new(ModelAttemptId::new());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentStep {
    step_id: StepId,
    stream: StreamId,
}

impl AgentStep {
    /// Creates the canonical aggregate stream for a step identity.
    ///
    /// # Errors
    ///
    /// Returns [`StepCoordinatorError`] if the protocol stream representation cannot be formed.
    pub fn new(step_id: StepId) -> Result<Self, StepCoordinatorError> {
        let stream = StreamId {
            kind: AggregateKind::new("agent-step")
                .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
            id: AggregateId::new(step_id.to_string())
                .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        };
        Ok(Self { step_id, stream })
    }

    /// Returns the stable step identity.
    #[must_use]
    pub const fn step_id(&self) -> StepId {
        self.step_id
    }

    /// Returns the canonical event stream identity.
    #[must_use]
    pub const fn stream_id(&self) -> &StreamId {
        &self.stream
    }
}

/// Failure while preparing, projecting, or settling a neutral agent step.
#[derive(Debug, Error)]
pub enum StepCoordinatorError {
    /// Model-input completeness audit failed.
    #[error(transparent)]
    InputAudit(#[from] InputAuditError),
    /// Model dispatch state failed.
    #[error(transparent)]
    Dispatch(#[from] DispatchCoordinatorError),
    /// Semantic response state failed.
    #[error(transparent)]
    Decode(#[from] DecodeCoordinatorError),
    /// Tool-operation aggregate recovery failed.
    #[error(transparent)]
    Operation(#[from] crate::OperationCoordinatorError),
    /// Bound or result content could not be verified or archived.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Direct step event storage failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Durable step facts contradict the neutral state machine.
    #[error("invalid agent step: {0}")]
    InvalidStep(String),
}

/// Durable neutral-loop position. Variants carrying authority are intentionally one-shot.
#[derive(Debug)]
pub enum AgentStepState {
    /// No model request has been prepared.
    NotStarted,
    /// Complete input was committed and may be marked started.
    ReadyToStart(DispatchAuthority),
    /// Provider execution may have occurred; never blind-dispatch.
    ModelInDoubt,
    /// Raw response is durable and may be interpreted without another provider call.
    ReadyToDecode(ReceivedModelResponse),
    /// Semantic facts are durable but the step boundary has not yet been settled.
    Decoded(DecodedModelTurn),
    /// The step yielded text with no operations to execute.
    Yielded {
        /// Archived provider-neutral turn.
        turn_id: ContentId<SemanticModelTurnArtifact>,
    },
    /// Tool calls were durably proposed and can be resolved through trusted registrations.
    AwaitingOperations {
        /// Archived provider-neutral turn.
        turn_id: ContentId<SemanticModelTurnArtifact>,
        /// Ordered, reconstructed, non-authoritative proposals.
        proposals: Vec<ToolCallProposal>,
    },
    /// Every proposal has a durable, trusted, uniquely identified operation binding.
    OperationsBound(crate::BoundStepOperations),
    /// Every operation produced an ordered model-visible input artifact.
    ReadyForNextStep {
        /// Archived provider-neutral turn that proposed the operations.
        turn_id: ContentId<SemanticModelTurnArtifact>,
        /// Operation results in original model output order.
        pending_results: Vec<ContentId<crate::OperationResult>>,
    },
    /// Transport proved no provider request was sent.
    ModelNotSent,
    /// Provider definitively rejected the request.
    ModelRejected,
    /// Provider outcome is unknown and requires reconciliation.
    ModelAmbiguous,
}

/// Stable boundary returned immediately after settling decoded semantics.
#[derive(Debug)]
pub enum SettledAgentStep {
    /// No tool operation is pending.
    Yielded {
        /// Archived provider-neutral turn.
        turn_id: ContentId<SemanticModelTurnArtifact>,
    },
    /// Ordered proposals await trusted tool registration and operation authority.
    AwaitingOperations {
        /// Archived provider-neutral turn.
        turn_id: ContentId<SemanticModelTurnArtifact>,
        /// Non-authoritative calls in model output order.
        proposals: Vec<ToolCallProposal>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettledPayload {
    step_id: StepId,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    tool_call_ids: Vec<ToolCallId>,
}

/// Audits and archives model input, then commits the first step fact and returns dispatch authority.
///
/// # Errors
///
/// Returns [`StepCoordinatorError`] when input is incomplete or the prepared fact cannot commit.
pub fn prepare_agent_step<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    step: &AgentStep,
    decision: &TurnInputDecision,
    attempt_id: ModelAttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<DispatchAuthority, StepCoordinatorError> {
    let request = prepare_model_request(content, decision)?;
    authorize_model_request(
        events,
        &step.stream,
        ExpectedRevision::NoStream,
        command_id,
        attempt_id,
        observed_at,
        request,
    )
    .map_err(Into::into)
}

/// Rebuilds the neutral-loop position exclusively from durable facts and verified content.
///
/// # Errors
///
/// Returns [`StepCoordinatorError`] when storage fails, complete-input audit fails, or facts and
/// content disagree.
pub fn recover_agent_step<E: EventStore, C: ContentStore>(
    events: &E,
    content: &mut C,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
) -> Result<AgentStepState, StepCoordinatorError> {
    let history = events.read_stream(&step.stream, None)?;
    let model_state = recover_model_attempt(&history, attempt_id)?;
    match model_state {
        ModelAttemptState::NotFound if history.is_empty() => Ok(AgentStepState::NotStarted),
        ModelAttemptState::NotFound => Err(StepCoordinatorError::InvalidStep(
            "step facts exist without a model attempt".into(),
        )),
        ModelAttemptState::Authorized => {
            let authority = recover_dispatch_authority(&history, content, attempt_id)?
                .ok_or_else(|| StepCoordinatorError::InvalidStep("authority disappeared".into()))?;
            Ok(AgentStepState::ReadyToStart(authority))
        }
        ModelAttemptState::InDoubt => Ok(AgentStepState::ModelInDoubt),
        ModelAttemptState::Completed { .. } => {
            recover_completed_step(events, content, step, attempt_id, &history)
        }
        ModelAttemptState::NotSent => Ok(AgentStepState::ModelNotSent),
        ModelAttemptState::Rejected => Ok(AgentStepState::ModelRejected),
        ModelAttemptState::Ambiguous => Ok(AgentStepState::ModelAmbiguous),
    }
}

fn recover_completed_step<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
    history: &[cairn_record::EventEnvelope],
) -> Result<AgentStepState, StepCoordinatorError> {
    let settled = settled_payload(history, step.step_id)?;
    let Some(decoded) = recover_decoded_model_turn(events, content, &step.stream, attempt_id)?
    else {
        if settled.is_some() {
            return Err(StepCoordinatorError::InvalidStep(
                "settled step has no durable semantic turn".into(),
            ));
        }
        let response = recover_received_model_response(history, attempt_id)?.ok_or_else(|| {
            StepCoordinatorError::InvalidStep("response authority disappeared".into())
        })?;
        return Ok(AgentStepState::ReadyToDecode(response));
    };
    let Some(settled) = settled else {
        return Ok(AgentStepState::Decoded(decoded));
    };
    validate_settled(&decoded, &settled)?;
    if settled.tool_call_ids.is_empty() {
        Ok(AgentStepState::Yielded {
            turn_id: settled.turn_id,
        })
    } else {
        match project_step_operations(
            events,
            content,
            history,
            step,
            settled.turn_id,
            decoded.into_proposals(),
        )? {
            StepOperationProjection::Unbound(proposals) => Ok(AgentStepState::AwaitingOperations {
                turn_id: settled.turn_id,
                proposals,
            }),
            StepOperationProjection::Bound(bound) => Ok(AgentStepState::OperationsBound(bound)),
            StepOperationProjection::Ready {
                turn_id,
                pending_results,
            } => Ok(AgentStepState::ReadyForNextStep {
                turn_id,
                pending_results,
            }),
        }
    }
}

/// Commits the durable boundary between semantic decoding and optional tool execution.
///
/// # Errors
///
/// Returns [`StepCoordinatorError`] when decoded content is not the step's durable projection or
/// the settled fact cannot commit.
pub fn settle_decoded_step<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
    decoded: DecodedModelTurn,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<SettledAgentStep, StepCoordinatorError> {
    let history = events.read_stream(&step.stream, None)?;
    if let Some(settled) = settled_payload(&history, step.step_id)? {
        validate_settled(&decoded, &settled)?;
        return Ok(into_settled(decoded));
    }
    let recovered = recover_decoded_model_turn(events, content, &step.stream, attempt_id)?
        .ok_or_else(|| StepCoordinatorError::InvalidStep("semantic turn is not durable".into()))?;
    if recovered.turn_id() != decoded.turn_id() || call_ids(&recovered) != call_ids(&decoded) {
        return Err(StepCoordinatorError::InvalidStep(
            "decoded authority differs from durable projection".into(),
        ));
    }
    let last = history
        .last()
        .ok_or_else(|| StepCoordinatorError::InvalidStep("step history is empty".into()))?;
    let decoded_event_id = history
        .iter()
        .find(|event| event.schema_name.as_str() == RESPONSE_DECODED)
        .map(|event| event.event_id)
        .ok_or_else(|| StepCoordinatorError::InvalidStep("decoded event is missing".into()))?;
    let payload = SettledPayload {
        step_id: step.step_id,
        turn_id: decoded.turn_id(),
        tool_call_ids: call_ids(&decoded),
    };
    let fact = step_fact(decoded_event_id, observed_at, &payload)?;
    events.append(
        &step.stream,
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        ),
        command_id,
        &[fact],
    )?;
    Ok(into_settled(decoded))
}

fn settled_payload(
    history: &[cairn_record::EventEnvelope],
    step_id: StepId,
) -> Result<Option<SettledPayload>, StepCoordinatorError> {
    let mut found = None;
    for event in history {
        if event.schema_name.as_str() != STEP_SETTLED {
            continue;
        }
        if event.schema_version.get() != 1 {
            return Err(StepCoordinatorError::InvalidStep(
                "unsupported step-settled schema version".into(),
            ));
        }
        let payload: SettledPayload = cairn_codec::from_slice(&event.payload)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?;
        if payload.step_id != step_id {
            return Err(StepCoordinatorError::InvalidStep(
                "settled event cites another step".into(),
            ));
        }
        let parent_is_decoded = event.parent_event_id.is_some_and(|parent_id| {
            history.iter().any(|candidate| {
                candidate.event_id == parent_id
                    && candidate.sequence < event.sequence
                    && candidate.schema_name.as_str() == RESPONSE_DECODED
            })
        });
        if !parent_is_decoded {
            return Err(StepCoordinatorError::InvalidStep(
                "settled event parent is not its decoded event".into(),
            ));
        }
        if found.replace(payload).is_some() {
            return Err(StepCoordinatorError::InvalidStep(
                "step has multiple settled facts".into(),
            ));
        }
    }
    Ok(found)
}

fn validate_settled(
    decoded: &DecodedModelTurn,
    settled: &SettledPayload,
) -> Result<(), StepCoordinatorError> {
    if decoded.turn_id() == settled.turn_id && call_ids(decoded) == settled.tool_call_ids {
        Ok(())
    } else {
        Err(StepCoordinatorError::InvalidStep(
            "settled fact differs from decoded semantics".into(),
        ))
    }
}

fn call_ids(decoded: &DecodedModelTurn) -> Vec<ToolCallId> {
    decoded
        .proposals()
        .iter()
        .map(ToolCallProposal::tool_call_id)
        .collect()
}

fn into_settled(decoded: DecodedModelTurn) -> SettledAgentStep {
    let turn_id = decoded.turn_id();
    let proposals = decoded.into_proposals();
    if proposals.is_empty() {
        SettledAgentStep::Yielded { turn_id }
    } else {
        SettledAgentStep::AwaitingOperations { turn_id, proposals }
    }
}

fn step_fact<P: Serialize>(
    parent_event_id: EventId,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, StepCoordinatorError> {
    let payload = cairn_codec::to_vec(payload)
        .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?;
    Ok(NewEvent {
        schema_name: SchemaName::new(STEP_SETTLED)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        parent_event_id: Some(parent_event_id),
        observed_at_unix_ms: observed_at.get(),
        payload,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{CommandId, ContentId, ContentType, ModelAttemptId, StepId};
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        AgentStep, AgentStepState, SettledAgentStep, prepare_agent_step, recover_agent_step,
        settle_decoded_step,
    };
    use crate::{
        AdapterModelTurn, AdapterOutputItem, AdapterVersion, ContextBlock, DeploymentName,
        DispatchCompletion, HistoryItem, InstructionBlock, ModelName, ModelSelection,
        OperationResult, PolicyDocument, PreparedModelRequest, ProviderName, ProviderToolCallId,
        RecordedAdapterExchange, RecordedModelAdapter, ScriptedModelTransport, ToolCatalog,
        ToolName, TransportError, TurnInputDecision, begin_model_dispatch, decode_model_response,
        execute_model_dispatch, recover_decoded_model_turn,
    };

    fn put_json<T: ContentType>(
        content: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(value).expect("encode");
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put")
            .content_id
    }

    fn decision(content: &mut SqliteContentStore) -> TurnInputDecision {
        TurnInputDecision {
            selection: ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("fixture").expect("model"),
                deployment: DeploymentName::new("local").expect("deployment"),
                adapter_version: AdapterVersion::new("v1").expect("adapter"),
            },
            instructions: vec![put_json::<InstructionBlock>(
                content,
                &serde_json::json!({"text":"be exact"}),
            )],
            tool_catalog: put_json::<ToolCatalog>(
                content,
                &serde_json::json!({"tools":["read_source"]}),
            ),
            history: vec![put_json::<HistoryItem>(
                content,
                &serde_json::json!({"role":"user","content":"inspect"}),
            )],
            context: vec![put_json::<ContextBlock>(
                content,
                &serde_json::json!({"kind":"task","value":"one"}),
            )],
            pending_results: Vec::<ContentId<OperationResult>>::new(),
            policy: put_json::<PolicyDocument>(content, &serde_json::json!({"network":"deny"})),
        }
    }

    #[test]
    fn restart_projection_advances_only_through_durable_authority() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let step = AgentStep::new(StepId::new()).expect("step");
        let attempt_id = ModelAttemptId::new();
        let decision = decision(&mut content);
        let _lost_authority = prepare_agent_step(
            &mut events,
            &mut content,
            &step,
            &decision,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("prepare step");
        let AgentStepState::ReadyToStart(authority) =
            recover_agent_step(&events, &mut content, &step, attempt_id).expect("recover prepared")
        else {
            panic!("expected recovered start authority");
        };
        let started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        assert!(matches!(
            recover_agent_step(&events, &mut content, &step, attempt_id).expect("recover started"),
            AgentStepState::ModelInDoubt
        ));

        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"raw-response".to_vec())
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
        let response_id = received.response_id();
        let AgentStepState::ReadyToDecode(received) =
            recover_agent_step(&events, &mut content, &step, attempt_id).expect("recover response")
        else {
            panic!("expected recovered decode authority");
        };
        let mut adapter = RecordedModelAdapter::new(
            AdapterVersion::new("v1").expect("adapter"),
            [RecordedAdapterExchange {
                response_id,
                turn: AdapterModelTurn {
                    items: vec![AdapterOutputItem::ToolCall {
                        provider_call_id: ProviderToolCallId::new("call-1").expect("call"),
                        tool: ToolName::new("read_source").expect("tool"),
                        arguments: serde_json::json!({"path":"src/lib.rs"}),
                    }],
                },
            }],
        );
        let decoded = decode_model_response(
            &mut events,
            &mut content,
            &mut adapter,
            received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("decode");
        assert!(matches!(
            recover_agent_step(&events, &mut content, &step, attempt_id).expect("recover decoded"),
            AgentStepState::Decoded(_)
        ));

        assert_operation_boundary_recovery(&mut events, &mut content, &step, attempt_id, decoded);
    }

    fn assert_operation_boundary_recovery(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        decoded: crate::DecodedModelTurn,
    ) {
        let settle_command = CommandId::new();
        let settled = settle_decoded_step(
            events,
            content,
            step,
            attempt_id,
            decoded,
            &settle_command,
            cairn_protocol::ObservedAtUnixMillis::new(5),
        )
        .expect("settle");
        let SettledAgentStep::AwaitingOperations { proposals, .. } = settled else {
            panic!("expected operation boundary");
        };
        assert_eq!(proposals.len(), 1);

        let AgentStepState::AwaitingOperations { turn_id, proposals } =
            recover_agent_step(events, content, step, attempt_id).expect("recover settled")
        else {
            panic!("expected awaiting operations");
        };
        assert_eq!(proposals.len(), 1);
        let decoded_again =
            recover_decoded_model_turn(events, content, step.stream_id(), attempt_id)
                .expect("recover semantic")
                .expect("semantic");
        assert_eq!(decoded_again.turn_id(), turn_id);
        let before = events
            .read_stream(step.stream_id(), None)
            .expect("read before replay")
            .len();
        let replayed = settle_decoded_step(
            events,
            content,
            step,
            attempt_id,
            decoded_again,
            &settle_command,
            cairn_protocol::ObservedAtUnixMillis::new(5),
        )
        .expect("settle replay");
        assert!(matches!(
            replayed,
            SettledAgentStep::AwaitingOperations { .. }
        ));
        assert_eq!(
            events
                .read_stream(step.stream_id(), None)
                .expect("read replay")
                .len(),
            before
        );
    }

    #[test]
    fn empty_tool_set_yields_the_step() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let step = AgentStep::new(StepId::new()).expect("step");
        let attempt_id = ModelAttemptId::new();
        let decision = decision(&mut content);
        let authority = prepare_agent_step(
            &mut events,
            &mut content,
            &step,
            &decision,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("prepare");
        let started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"raw".to_vec())
        });
        let DispatchCompletion::Response(received) = execute_model_dispatch(
            &mut events,
            &mut content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("dispatch") else {
            panic!("response");
        };
        let response_id = received.response_id();
        let mut adapter = RecordedModelAdapter::new(
            AdapterVersion::new("v1").expect("adapter"),
            [RecordedAdapterExchange {
                response_id,
                turn: AdapterModelTurn {
                    items: vec![AdapterOutputItem::Text {
                        text: "done".to_owned(),
                    }],
                },
            }],
        );
        let decoded = decode_model_response(
            &mut events,
            &mut content,
            &mut adapter,
            received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("decode");
        assert!(matches!(
            settle_decoded_step(
                &mut events,
                &content,
                &step,
                attempt_id,
                decoded,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(5),
            )
            .expect("settle"),
            SettledAgentStep::Yielded { .. }
        ));
        assert!(matches!(
            recover_agent_step(&events, &mut content, &step, attempt_id).expect("recover"),
            AgentStepState::Yielded { .. }
        ));
    }
}
