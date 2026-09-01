//! One task-generic durable Agent episode driver used by every Agent profile.

use crate::{
    AgentEpisode, AgentEpisodeState, AgentRoleName, AgentStep, AgentStepState, DispatchCompletion,
    EpisodeAdvance, EpisodeBudget, EpisodeCompletionReason, EpisodeOperationAdmissionOutcome,
    EpisodeStepAuthority, HistoryItem, InstructionBlock, ModelSelection, ModelTransport,
    NativeContinuation, NativeProtocolCodec, NativeRequestSpec, OperationResult, PolicyDocument,
    PreparedNativeRequest, PreparedToolOperation, SettledAgentStep, StepOperationSettlement,
    ToolCatalog, ToolEffectClass, ToolGateway, ToolName, ToolOperationAssignment,
    ToolOperationState, ToolRegistration, TurnInputDecision, admit_episode_operations,
    advance_agent_episode, authorize_tool_operation, begin_model_dispatch, begin_tool_operation,
    execute_model_dispatch, execute_tool_operation, open_agent_episode,
    prepare_native_episode_step, recover_agent_episode, recover_agent_step, recover_tool_operation,
    settle_decoded_step, settle_step_operations,
};
use cairn_protocol::{
    AttemptId, CommandId, ContentId, EpisodeId, ModelAttemptId, ObservedAtUnixMillis, OperationId,
    StepId, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use thiserror::Error;

/// Exact trusted tool implementations and effect classes granted to one Agent episode.
#[derive(Debug)]
pub struct AgentStepCapabilityGrantV1(Vec<ToolRegistration>);

impl AgentStepCapabilityGrantV1 {
    /// Constructs the exact non-empty tool capability set for one Agent step.
    ///
    /// # Errors
    ///
    /// Rejects an empty set or duplicate tool names.
    pub fn new(registrations: Vec<ToolRegistration>) -> Result<Self, AgentEpisodeDriverError> {
        if registrations.is_empty() {
            return Err(AgentEpisodeDriverError::InvalidCapabilityGrant(
                "capability grant has no tools".into(),
            ));
        }
        let mut names = std::collections::HashSet::new();
        if registrations
            .iter()
            .any(|registration| !names.insert(registration.name().as_str()))
        {
            return Err(AgentEpisodeDriverError::InvalidCapabilityGrant(
                "capability grant repeats a tool name".into(),
            ));
        }
        Ok(Self(registrations))
    }

    fn registration(&self, name: &ToolName) -> Result<ToolRegistration, AgentEpisodeDriverError> {
        self.0
            .iter()
            .find(|registration| registration.name() == name)
            .cloned()
            .ok_or_else(|| AgentEpisodeDriverError::UnavailableTool(name.as_str().to_owned()))
    }
}

/// Exact model-visible input, profile, tool catalog, budget, and capability registrations frozen
/// before one Agent episode is opened.
pub struct FrozenAgentEpisodeDriverV1 {
    pub task_id: TaskId,
    pub episode_id: EpisodeId,
    pub role: AgentRoleName,
    pub selection: ModelSelection,
    pub budget: EpisodeBudget,
    pub native_spec: NativeRequestSpec,
    pub user_text: String,
    pub instruction: ContentId<InstructionBlock>,
    pub tool_catalog: ContentId<ToolCatalog>,
    pub history: ContentId<HistoryItem>,
    pub context: ContentId<crate::ContextBlock>,
    pub policy: ContentId<PolicyDocument>,
    pub capability_grant: AgentStepCapabilityGrantV1,
}

impl FrozenAgentEpisodeDriverV1 {
    fn turn_input_decision(
        &self,
        pending_results: Vec<ContentId<OperationResult>>,
    ) -> TurnInputDecision {
        TurnInputDecision {
            selection: self.selection.clone(),
            instructions: vec![self.instruction],
            tool_catalog: self.tool_catalog,
            history: vec![self.history],
            context: vec![self.context],
            pending_results,
            policy: self.policy,
        }
    }
}

/// Durable terminal position reached by the common episode driver before domain submission freezing.
#[derive(Debug)]
pub struct AgentEpisodeDriverCompletionV1 {
    pub reason: EpisodeCompletionReason,
    pub steps_started: u32,
}

/// Exact non-authoritative Worker operation requested by an Agent step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentWorkerOperationRequestV1 {
    pub operation_id: OperationId,
    pub source_tool_call_id: crate::ToolCallId,
    pub tool: ToolName,
    pub implementation_version: crate::ToolImplementationVersion,
    pub effect: ToolEffectClass,
    pub arguments_id: ContentId<crate::ToolArguments>,
    pub arguments: serde_json::Value,
}

/// Same-episode request returned before any Worker execution receives Controller authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentWorkerRequestV1 {
    pub episode_id: EpisodeId,
    pub step_id: StepId,
    pub model_attempt_id: ModelAttemptId,
    pub operations: Vec<AgentWorkerOperationRequestV1>,
}

/// Boundary reached after one durable model step and its workflow-local tools have settled.
#[derive(Debug)]
pub enum AgentEpisodeDriverStepOutcomeV1 {
    Continue,
    Complete(AgentEpisodeDriverCompletionV1),
    WorkerRequest(AgentWorkerRequestV1),
}

pub enum AgentProfileEpisodeOutcomeV1<T> {
    Complete(T),
    WorkerRequest(AgentWorkerRequestV1),
}

/// Failure while driving the common episode driver.
#[derive(Debug, Error)]
pub enum AgentEpisodeDriverError {
    #[error("Agent episode driver failed: {0}")]
    Agent(String),
    #[error("Agent episode model dispatch failed ({class:?}): {diagnostic}")]
    ModelDispatch {
        class: crate::TransportFailureClass,
        diagnostic: String,
    },
    #[error("Agent profile does not grant tool {0}")]
    UnavailableTool(String),
    #[error("Agent capability grant is invalid: {0}")]
    InvalidCapabilityGrant(String),
}

struct OpenedAgentEpisodeV1 {
    episode: AgentEpisode,
    authority: EpisodeStepAuthority,
    native: PreparedNativeRequest,
    pending_results: Vec<ContentId<OperationResult>>,
}

struct DispatchedAgentTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
}

struct SettledAgentTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
    proposed_tools: Vec<ToolName>,
}

struct AdmittedAgentTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
    operations: Vec<PreparedToolOperation>,
}

struct ObservedAgentTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
}

struct ProjectedAgentTurnV1 {
    episode: AgentEpisode,
    native: PreparedNativeRequest,
    pending_results: Vec<ContentId<OperationResult>>,
}

enum AgentEpisodeDriverStageV1<T> {
    Active(T),
    Complete(AgentEpisodeDriverCompletionV1),
}

enum AgentEpisodeDriverAdvanceV1 {
    Continue(OpenedAgentEpisodeV1),
    Complete(AgentEpisodeDriverCompletionV1),
}

enum AgentEpisodeDriverPositionV1 {
    ReadyForAgent(OpenedAgentEpisodeV1),
    ReadyForOperations(AdmittedAgentTurnV1),
    ReadyForProjection(ObservedAgentTurnV1),
    Complete(AgentEpisodeDriverCompletionV1),
}

enum AgentEpisodeWorkerTransitionV1 {
    Observed(ObservedAgentTurnV1),
    WorkerRequest(AgentWorkerRequestV1),
}

/// Drives at most one real model/tool step of a durable Agent episode.
///
/// Workflow-local tool calls belong to the model step that requested them. `Continue` is returned
/// only after their results are durably projected and the next model step has authority. External
/// Worker effects still yield before execution.
///
/// # Errors
///
/// Returns an error when the durable position or the authorized step cannot be driven safely.
pub fn drive_agent_episode_step<E, C, T, G>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    gateway: &mut G,
) -> Result<AgentEpisodeDriverStepOutcomeV1, AgentEpisodeDriverError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
    G: ToolGateway,
{
    let mut position = open_or_recover_agent_episode(events, content, codec, frozen)?;
    loop {
        position = match position {
            AgentEpisodeDriverPositionV1::ReadyForAgent(opened) => {
                prepare_agent_operations(events, content, transport, codec, frozen, opened)?
            }
            AgentEpisodeDriverPositionV1::ReadyForOperations(admitted) => {
                match execute_or_request_worker_operations(events, content, gateway, admitted)? {
                    AgentEpisodeWorkerTransitionV1::Observed(observed) => {
                        AgentEpisodeDriverPositionV1::ReadyForProjection(observed)
                    }
                    AgentEpisodeWorkerTransitionV1::WorkerRequest(request) => {
                        return Ok(AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request));
                    }
                }
            }
            AgentEpisodeDriverPositionV1::ReadyForProjection(observed) => {
                match project_and_advance(events, content, codec, frozen, observed)? {
                    AgentEpisodeDriverPositionV1::ReadyForAgent(_) => {
                        return Ok(AgentEpisodeDriverStepOutcomeV1::Continue);
                    }
                    other => other,
                }
            }
            AgentEpisodeDriverPositionV1::Complete(completion) => {
                return Ok(AgentEpisodeDriverStepOutcomeV1::Complete(completion));
            }
        };
    }
}

fn prepare_agent_operations<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    opened: OpenedAgentEpisodeV1,
) -> Result<AgentEpisodeDriverPositionV1, AgentEpisodeDriverError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let dispatched = dispatch_agent_turn(events, content, transport, frozen, opened)?;
    let settled = settle_agent_turn(events, content, codec, dispatched)?;
    let admitted = admit_agent_operations(events, content, frozen, settled)?;
    Ok(match admitted {
        AgentEpisodeDriverStageV1::Active(admitted) => {
            AgentEpisodeDriverPositionV1::ReadyForOperations(admitted)
        }
        AgentEpisodeDriverStageV1::Complete(completion) => {
            AgentEpisodeDriverPositionV1::Complete(completion)
        }
    })
}

fn project_and_advance<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    observed: ObservedAgentTurnV1,
) -> Result<AgentEpisodeDriverPositionV1, AgentEpisodeDriverError> {
    let projected = project_operation_observations(
        events,
        content,
        codec,
        frozen,
        AgentEpisodeDriverStageV1::Active(observed),
    )?;
    Ok(match advance_runtime_episode(events, content, projected)? {
        AgentEpisodeDriverAdvanceV1::Continue(next) => {
            AgentEpisodeDriverPositionV1::ReadyForAgent(next)
        }
        AgentEpisodeDriverAdvanceV1::Complete(completion) => {
            AgentEpisodeDriverPositionV1::Complete(completion)
        }
    })
}

fn open_or_recover_agent_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
) -> Result<AgentEpisodeDriverPositionV1, AgentEpisodeDriverError> {
    let episode = AgentEpisode::new(frozen.episode_id).map_err(agent_error)?;
    match recover_agent_episode(events, content, &episode).map_err(agent_error)? {
        AgentEpisodeState::NotFound => open_new_runtime_episode(events, codec, frozen, episode)
            .map(AgentEpisodeDriverPositionV1::ReadyForAgent),
        AgentEpisodeState::Active {
            step,
            model_attempt_id,
            step_state,
        } => recover_active_runtime_episode(
            events,
            content,
            codec,
            episode,
            step,
            model_attempt_id,
            step_state,
        ),
        AgentEpisodeState::Completed {
            reason,
            steps_started,
        } => Ok(AgentEpisodeDriverPositionV1::Complete(
            AgentEpisodeDriverCompletionV1 {
                reason,
                steps_started,
            },
        )),
        AgentEpisodeState::ReadyToPrepare(authority) => {
            recover_ready_runtime_episode(events, content, codec, frozen, episode, authority)
                .map(AgentEpisodeDriverPositionV1::ReadyForAgent)
        }
    }
}

fn recover_ready_runtime_episode<E: EventStore, C: ContentStore>(
    events: &E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    episode: AgentEpisode,
    authority: EpisodeStepAuthority,
) -> Result<OpenedAgentEpisodeV1, AgentEpisodeDriverError> {
    let pending_results = authority.expected_pending_results().to_vec();
    let native = if let Some(previous) = authority.previous_step() {
        let previous_step = AgentStep::new(previous.step_id()).map_err(agent_error)?;
        let continuation = recover_native_continuation(
            events,
            content,
            codec,
            &previous_step,
            previous.model_attempt_id(),
        )?;
        let continuation = codec
            .append_archived_tool_results(content, &continuation, &pending_results)
            .map_err(agent_error)?;
        codec
            .prepare_continuation(&frozen.native_spec, &continuation)
            .map_err(agent_error)?
    } else {
        codec
            .prepare_initial(&frozen.native_spec, &frozen.user_text)
            .map_err(agent_error)?
    };
    Ok(OpenedAgentEpisodeV1 {
        episode,
        authority,
        native,
        pending_results,
    })
}

fn open_new_runtime_episode<E: EventStore>(
    events: &mut E,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    episode: AgentEpisode,
) -> Result<OpenedAgentEpisodeV1, AgentEpisodeDriverError> {
    let authority = open_agent_episode(
        events,
        &episode,
        frozen.task_id,
        frozen.role.clone(),
        frozen.budget.clone(),
        StepId::new(),
        ModelAttemptId::new(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?;
    let native = codec
        .prepare_initial(&frozen.native_spec, &frozen.user_text)
        .map_err(agent_error)?;
    Ok(OpenedAgentEpisodeV1 {
        episode,
        authority,
        native,
        pending_results: Vec::new(),
    })
}

fn recover_active_runtime_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    step_state: AgentStepState,
) -> Result<AgentEpisodeDriverPositionV1, AgentEpisodeDriverError> {
    match step_state {
        AgentStepState::OperationsBound(bound) => {
            let continuation =
                recover_native_continuation(events, content, codec, &step, attempt_id)?;
            Ok(AgentEpisodeDriverPositionV1::ReadyForOperations(
                AdmittedAgentTurnV1 {
                    episode,
                    step,
                    attempt_id,
                    continuation,
                    operations: bound.into_operations(),
                },
            ))
        }
        AgentStepState::ReadyForNextStep { .. } => {
            let continuation =
                recover_native_continuation(events, content, codec, &step, attempt_id)?;
            Ok(AgentEpisodeDriverPositionV1::ReadyForProjection(
                ObservedAgentTurnV1 {
                    episode,
                    step,
                    attempt_id,
                    continuation,
                },
            ))
        }
        AgentStepState::Yielded { .. } => {
            complete_episode(events, content, &episode).map(AgentEpisodeDriverPositionV1::Complete)
        }
        _ => Err(AgentEpisodeDriverError::Agent(
            "Agent episode is not at a recoverable external-effect safe point".into(),
        )),
    }
}

fn recover_native_continuation<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    codec: NativeProtocolCodec,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
) -> Result<NativeContinuation, AgentEpisodeDriverError> {
    codec
        .recover_recorded(events, content, step.stream_id(), attempt_id)
        .map_err(agent_error)?
        .map(|(_, continuation)| continuation)
        .ok_or_else(|| {
            AgentEpisodeDriverError::Agent(
                "active Agent step has no durable native continuation".into(),
            )
        })
}

fn dispatch_agent_turn<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    frozen: &FrozenAgentEpisodeDriverV1,
    opened: OpenedAgentEpisodeV1,
) -> Result<DispatchedAgentTurnV1, AgentEpisodeDriverError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let OpenedAgentEpisodeV1 {
        episode,
        authority,
        native,
        pending_results,
    } = opened;
    let step = AgentStep::new(authority.step_id()).map_err(agent_error)?;
    let attempt_id = authority.model_attempt_id();
    let decision = frozen.turn_input_decision(pending_results);
    let dispatch = prepare_native_episode_step(
        events,
        content,
        authority,
        &decision,
        &native,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?;
    let started = begin_model_dispatch(events, dispatch, &CommandId::new(), observed_now()?)
        .map_err(agent_error)?;
    match execute_model_dispatch(
        events,
        content,
        transport,
        started,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?
    {
        DispatchCompletion::Response(_) => Ok(DispatchedAgentTurnV1 {
            episode,
            step,
            attempt_id,
        }),
        DispatchCompletion::NotSent { diagnostic } => Err(AgentEpisodeDriverError::ModelDispatch {
            class: crate::TransportFailureClass::NotSent,
            diagnostic,
        }),
        DispatchCompletion::Rejected { diagnostic } => {
            Err(AgentEpisodeDriverError::ModelDispatch {
                class: crate::TransportFailureClass::Rejected,
                diagnostic,
            })
        }
        DispatchCompletion::Ambiguous { diagnostic } => {
            Err(AgentEpisodeDriverError::ModelDispatch {
                class: crate::TransportFailureClass::Ambiguous,
                diagnostic,
            })
        }
    }
}

fn settle_agent_turn<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    dispatched: DispatchedAgentTurnV1,
) -> Result<AgentEpisodeDriverStageV1<SettledAgentTurnV1>, AgentEpisodeDriverError> {
    let DispatchedAgentTurnV1 {
        episode,
        step,
        attempt_id,
    } = dispatched;
    let AgentStepState::ReadyToDecode(received) =
        recover_agent_step(events, content, &step, attempt_id).map_err(agent_error)?
    else {
        return Err(AgentEpisodeDriverError::Agent(
            "model response did not recover at the decode boundary".into(),
        ));
    };
    let decoded = codec
        .decode_recovered_received(
            events,
            content,
            received,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(agent_error)?;
    let continuation = decoded.continuation().clone();
    let proposed_tools = decoded
        .semantic()
        .proposals()
        .iter()
        .map(|proposal| proposal.tool().clone())
        .collect();
    let settled = settle_decoded_step(
        events,
        content,
        &step,
        attempt_id,
        decoded.into_semantic(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?;
    if matches!(settled, SettledAgentStep::Yielded { .. }) {
        return complete_episode(events, content, &episode)
            .map(AgentEpisodeDriverStageV1::Complete);
    }
    Ok(AgentEpisodeDriverStageV1::Active(SettledAgentTurnV1 {
        episode,
        step,
        attempt_id,
        continuation,
        proposed_tools,
    }))
}

fn admit_agent_operations<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    frozen: &FrozenAgentEpisodeDriverV1,
    settled: AgentEpisodeDriverStageV1<SettledAgentTurnV1>,
) -> Result<AgentEpisodeDriverStageV1<AdmittedAgentTurnV1>, AgentEpisodeDriverError> {
    let settled = match settled {
        AgentEpisodeDriverStageV1::Active(settled) => settled,
        AgentEpisodeDriverStageV1::Complete(completion) => {
            return Ok(AgentEpisodeDriverStageV1::Complete(completion));
        }
    };
    let SettledAgentTurnV1 {
        episode,
        step,
        attempt_id,
        continuation,
        proposed_tools,
    } = settled;
    let assignments = proposed_tools
        .iter()
        .map(|name| {
            Ok(ToolOperationAssignment::new(
                OperationId::new(),
                frozen.capability_grant.registration(name)?,
            ))
        })
        .collect::<Result<Vec<_>, AgentEpisodeDriverError>>()?;
    match admit_episode_operations(
        events,
        content,
        &episode,
        assignments,
        &CommandId::new(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?
    {
        EpisodeOperationAdmissionOutcome::Admitted(admission) => {
            Ok(AgentEpisodeDriverStageV1::Active(AdmittedAgentTurnV1 {
                episode,
                step,
                attempt_id,
                continuation,
                operations: admission.into_operations(),
            }))
        }
        EpisodeOperationAdmissionOutcome::Completed {
            reason,
            steps_started,
        } => Ok(AgentEpisodeDriverStageV1::Complete(
            AgentEpisodeDriverCompletionV1 {
                reason,
                steps_started,
            },
        )),
    }
}

fn execute_or_request_worker_operations<E, C, G>(
    events: &mut E,
    content: &mut C,
    gateway: &mut G,
    admitted: AdmittedAgentTurnV1,
) -> Result<AgentEpisodeWorkerTransitionV1, AgentEpisodeDriverError>
where
    E: EventStore,
    C: ContentStore,
    G: ToolGateway,
{
    let AdmittedAgentTurnV1 {
        episode,
        step,
        attempt_id,
        continuation,
        operations,
    } = admitted;
    let mut external = Vec::new();
    for operation in operations {
        let state =
            recover_tool_operation(events, operation.operation_id()).map_err(agent_error)?;
        if !matches!(
            operation.effect(),
            ToolEffectClass::Pure | ToolEffectClass::ReadOnly
        ) {
            if !matches!(state, ToolOperationState::Completed { .. }) {
                external.push(worker_operation_request(&operation)?);
            }
            continue;
        }
        if matches!(state, ToolOperationState::Completed { .. }) {
            continue;
        }
        if !matches!(state, ToolOperationState::NotFound) {
            return Err(AgentEpisodeDriverError::Agent(
                "workflow-local tool operation requires explicit reconciliation".into(),
            ));
        }
        let authority =
            authorize_tool_operation(events, &CommandId::new(), observed_now()?, operation)
                .map_err(agent_error)?;
        let started = begin_tool_operation(
            events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(agent_error)?;
        let _ = execute_tool_operation(
            events,
            content,
            gateway,
            started,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(agent_error)?;
    }
    if !external.is_empty() {
        return Ok(AgentEpisodeWorkerTransitionV1::WorkerRequest(
            AgentWorkerRequestV1 {
                episode_id: episode.episode_id(),
                step_id: step.step_id(),
                model_attempt_id: attempt_id,
                operations: external,
            },
        ));
    }
    Ok(AgentEpisodeWorkerTransitionV1::Observed(
        ObservedAgentTurnV1 {
            episode,
            step,
            attempt_id,
            continuation,
        },
    ))
}

fn worker_operation_request(
    operation: &PreparedToolOperation,
) -> Result<AgentWorkerOperationRequestV1, AgentEpisodeDriverError> {
    let arguments = cairn_codec::from_slice(operation.argument_bytes()).map_err(agent_error)?;
    Ok(AgentWorkerOperationRequestV1 {
        operation_id: operation.operation_id(),
        source_tool_call_id: operation.source_tool_call_id().ok_or_else(|| {
            AgentEpisodeDriverError::Agent(
                "external Worker operation has no decoded tool-call lineage".into(),
            )
        })?,
        tool: operation.tool().clone(),
        implementation_version: operation.implementation_version().clone(),
        effect: operation.effect(),
        arguments_id: operation.arguments_id(),
        arguments,
    })
}

/// Executes the exact external operations previously yielded by [`drive_agent_episode_step`].
///
/// The Controller supplies the trusted gateway only after deciding that the request is allowed and
/// that an appropriate Worker is available. Re-entering the episode driver afterwards projects the
/// durable results back into the same model continuation.
///
/// # Errors
///
/// Rejects altered request metadata, an already-started operation, or a gateway/record failure.
pub fn execute_agent_worker_request<E, C, G>(
    events: &mut E,
    content: &mut C,
    gateway: &mut G,
    request: &AgentWorkerRequestV1,
) -> Result<(), AgentEpisodeDriverError>
where
    E: EventStore,
    C: ContentStore,
    G: ToolGateway,
{
    for requested in &request.operations {
        let argument_bytes = cairn_codec::to_vec(&requested.arguments).map_err(agent_error)?;
        let arguments_id =
            ContentId::<crate::ToolArguments>::derive(&argument_bytes).map_err(agent_error)?;
        if arguments_id != requested.arguments_id {
            return Err(AgentEpisodeDriverError::Agent(
                "Worker request arguments changed after episode yield".into(),
            ));
        }
        let operation = PreparedToolOperation::from_tool_call(
            requested.operation_id,
            requested.source_tool_call_id,
            requested.tool.clone(),
            requested.implementation_version.clone(),
            requested.effect,
            requested.arguments_id,
            argument_bytes,
        );
        if !matches!(
            recover_tool_operation(events, requested.operation_id).map_err(agent_error)?,
            ToolOperationState::NotFound
        ) {
            return Err(AgentEpisodeDriverError::Agent(
                "Worker request operation is not awaiting first execution".into(),
            ));
        }
        let authority =
            authorize_tool_operation(events, &CommandId::new(), observed_now()?, operation)
                .map_err(agent_error)?;
        let started = begin_tool_operation(
            events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(agent_error)?;
        let _completion = execute_tool_operation(
            events,
            content,
            gateway,
            started,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(agent_error)?;
    }
    Ok(())
}

fn project_operation_observations<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenAgentEpisodeDriverV1,
    observed: AgentEpisodeDriverStageV1<ObservedAgentTurnV1>,
) -> Result<AgentEpisodeDriverStageV1<ProjectedAgentTurnV1>, AgentEpisodeDriverError> {
    let observed = match observed {
        AgentEpisodeDriverStageV1::Active(observed) => observed,
        AgentEpisodeDriverStageV1::Complete(completion) => {
            return Ok(AgentEpisodeDriverStageV1::Complete(completion));
        }
    };
    let ObservedAgentTurnV1 {
        episode,
        step,
        attempt_id,
        continuation,
    } = observed;
    let StepOperationSettlement::ReadyForNextStep {
        pending_results: results,
        ..
    } = settle_step_operations(
        events,
        content,
        &step,
        attempt_id,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?
    else {
        return Err(AgentEpisodeDriverError::Agent(
            "tool operation requires explicit reconciliation".into(),
        ));
    };
    let continuation = codec
        .append_archived_tool_results(content, &continuation, &results)
        .map_err(agent_error)?;
    let native = codec
        .prepare_continuation(&frozen.native_spec, &continuation)
        .map_err(agent_error)?;
    Ok(AgentEpisodeDriverStageV1::Active(ProjectedAgentTurnV1 {
        episode,
        native,
        pending_results: results,
    }))
}

fn advance_runtime_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    projected: AgentEpisodeDriverStageV1<ProjectedAgentTurnV1>,
) -> Result<AgentEpisodeDriverAdvanceV1, AgentEpisodeDriverError> {
    let projected = match projected {
        AgentEpisodeDriverStageV1::Active(projected) => projected,
        AgentEpisodeDriverStageV1::Complete(completion) => {
            return Ok(AgentEpisodeDriverAdvanceV1::Complete(completion));
        }
    };
    let ProjectedAgentTurnV1 {
        episode,
        native,
        pending_results,
    } = projected;
    match advance_agent_episode(
        events,
        content,
        &episode,
        StepId::new(),
        ModelAttemptId::new(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?
    {
        EpisodeAdvance::NextStep(authority) => Ok(AgentEpisodeDriverAdvanceV1::Continue(
            OpenedAgentEpisodeV1 {
                episode,
                authority,
                native,
                pending_results,
            },
        )),
        EpisodeAdvance::Completed {
            reason,
            steps_started,
        } => Ok(AgentEpisodeDriverAdvanceV1::Complete(
            AgentEpisodeDriverCompletionV1 {
                reason,
                steps_started,
            },
        )),
    }
}

fn complete_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    episode: &AgentEpisode,
) -> Result<AgentEpisodeDriverCompletionV1, AgentEpisodeDriverError> {
    let EpisodeAdvance::Completed {
        reason,
        steps_started,
    } = advance_agent_episode(
        events,
        content,
        episode,
        StepId::new(),
        ModelAttemptId::new(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?
    else {
        return Err(AgentEpisodeDriverError::Agent(
            "yielded step unexpectedly advanced".into(),
        ));
    };
    Ok(AgentEpisodeDriverCompletionV1 {
        reason,
        steps_started,
    })
}

fn observed_now() -> Result<ObservedAtUnixMillis, AgentEpisodeDriverError> {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(agent_error)?
        .as_millis();
    let milliseconds = i64::try_from(milliseconds)
        .map_err(|_| AgentEpisodeDriverError::Agent("wall clock overflow".into()))?;
    Ok(ObservedAtUnixMillis::new(milliseconds))
}

fn agent_error(error: impl std::fmt::Display) -> AgentEpisodeDriverError {
    AgentEpisodeDriverError::Agent(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use crate::{
        AdapterVersion, CanonicalToolResult, ContextBlock, DeploymentName, EpisodeStepLimit,
        EpisodeToolOperationLimit, ModelName, ModelOutputTokenLimit, ModelProtocolConfig,
        ModelSelection, ModelTransportResponse, NativeRequestSpec, NativeToolDefinition,
        PolicyDocument, PreparedToolOperation, ProviderName, ResponsesReasoningReplay,
        ScriptedModelTransport, ToolCatalog, ToolGatewayError, ToolImplementationVersion, ToolName,
        TransportError,
    };
    use cairn_protocol::{ContentType, EpisodeId};
    use cairn_record::ContentStore;
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::*;

    fn put<T: ContentType>(
        content: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(
                cairn_codec::to_vec(&value).expect("canonical profile content"),
            ))
            .expect("profile content")
            .content_id
    }

    #[test]
    fn capability_grant_rejects_duplicate_tool_names() {
        let tool = ToolName::new("read_task_source").expect("tool");
        let registration = ToolRegistration::new(
            tool,
            ToolImplementationVersion::new("task-source-v1").expect("version"),
            ToolEffectClass::ReadOnly,
        );

        let error = AgentStepCapabilityGrantV1::new(vec![registration.clone(), registration])
            .expect_err("duplicate semantic capability must be rejected");

        assert!(matches!(
            error,
            AgentEpisodeDriverError::InvalidCapabilityGrant(message)
                if message == "capability grant repeats a tool name"
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn one_step_driver_returns_to_hooks_between_real_model_turns() {
        let temporary = tempfile::tempdir().expect("temporary state");
        let mut content = SqliteContentStore::open(
            temporary.path().join("content.db"),
            temporary.path().join("cas"),
        )
        .expect("content");
        let mut events =
            SqliteEventStore::open(temporary.path().join("events.db")).expect("events");
        let tool = ToolName::new("read_task_source").expect("tool");
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: TaskId::new(),
            episode_id: EpisodeId::new(),
            role: AgentRoleName::new("generic-step-boundary-control").expect("role"),
            selection: ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("recorded-model").expect("model"),
                deployment: DeploymentName::new("isolated").expect("deployment"),
                adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
            },
            budget: EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(2).expect("steps")),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(2)),
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            },
            native_spec: NativeRequestSpec {
                wire_model: ModelName::new("recorded-model").expect("model"),
                instructions: "Read once, then finish.".into(),
                tools: vec![NativeToolDefinition {
                    name: tool.clone(),
                    description: "Read one bounded task-local source range.".into(),
                    input_schema: serde_json::json!({
                        "type":"object",
                        "properties":{},
                        "required":[],
                        "additionalProperties":false
                    }),
                    strict: true,
                }],
                max_output_tokens: ModelOutputTokenLimit::new(128).expect("output limit"),
            },
            user_text: "Inspect the offered task.".into(),
            instruction: put::<InstructionBlock>(
                &mut content,
                &serde_json::json!({"text":"Read once, then finish."}),
            ),
            tool_catalog: put::<ToolCatalog>(
                &mut content,
                &serde_json::json!({"schema_version":1,"tools":["read_task_source"]}),
            ),
            history: put::<HistoryItem>(
                &mut content,
                &serde_json::json!({"role":"user","content":"Inspect the offered task."}),
            ),
            context: put::<ContextBlock>(
                &mut content,
                &serde_json::json!({"schema_version":1,"knowledge_snapshot":{"kind":"empty"}}),
            ),
            policy: put::<PolicyDocument>(
                &mut content,
                &serde_json::json!({"schema_version":1,"task_reads":"bounded"}),
            ),
            capability_grant: AgentStepCapabilityGrantV1::new(vec![ToolRegistration::new(
                tool,
                ToolImplementationVersion::new("task-source-v1").expect("version"),
                ToolEffectClass::ReadOnly,
            )])
            .expect("capability grant"),
        };
        let tool_response = serde_json::to_vec(&serde_json::json!({
            "output":[{
                "type":"function_call",
                "call_id":"read-source",
                "name":"read_task_source",
                "arguments":"{}"
            }]
        }))
        .expect("tool response");
        let terminal_response = serde_json::to_vec(&serde_json::json!({
            "output":[{
                "type":"message",
                "id":"final",
                "phase":"final_answer",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":"done"}]
            }]
        }))
        .expect("terminal response");
        let dispatches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transport_dispatches = Arc::clone(&dispatches);
        let mut transport = ScriptedModelTransport::new(
            move |_: &crate::PreparedModelRequest| -> Result<_, TransportError> {
                let index = transport_dispatches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ModelTransportResponse::without_usage(if index == 0 {
                    tool_response.clone()
                } else {
                    terminal_response.clone()
                }))
            },
        );
        let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gateway_invocations = Arc::clone(&invocations);
        let mut gateway = crate::ScriptedToolGateway::new(
            move |_: &PreparedToolOperation| -> Result<_, ToolGatewayError> {
                gateway_invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                CanonicalToolResult::from_value(&serde_json::json!({"lines":["source"]}))
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            },
        );
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");

        let first = drive_agent_episode_step(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("first real step");
        assert!(matches!(first, AgentEpisodeDriverStepOutcomeV1::Continue));
        assert_eq!(dispatches.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 1);

        let second = drive_agent_episode_step(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("second real step");
        let AgentEpisodeDriverStepOutcomeV1::Complete(completion) = second else {
            panic!("second real model step must terminate the episode")
        };
        assert_eq!(completion.reason, EpisodeCompletionReason::Yielded);
        assert_eq!(completion.steps_started, 2);
        assert_eq!(dispatches.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn external_effect_proposal_is_durably_bound_but_never_executed_in_workflow() {
        let temporary = tempfile::tempdir().expect("temporary state");
        let mut content = SqliteContentStore::open(
            temporary.path().join("content.db"),
            temporary.path().join("cas"),
        )
        .expect("content");
        let mut events =
            SqliteEventStore::open(temporary.path().join("events.db")).expect("events");
        let tool = ToolName::new("request_external_probe").expect("tool");
        let frozen = FrozenAgentEpisodeDriverV1 {
            task_id: TaskId::new(),
            episode_id: EpisodeId::new(),
            role: AgentRoleName::new("generic-effect-control").expect("role"),
            selection: ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("recorded-model").expect("model"),
                deployment: DeploymentName::new("isolated").expect("deployment"),
                adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
            },
            budget: EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(2).expect("steps")),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(2)),
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            },
            native_spec: NativeRequestSpec {
                wire_model: ModelName::new("recorded-model").expect("model"),
                instructions: "Request the offered external probe.".into(),
                tools: vec![NativeToolDefinition {
                    name: tool.clone(),
                    description: "Request one Controller-owned external probe.".into(),
                    input_schema: serde_json::json!({
                        "type":"object",
                        "properties":{},
                        "required":[],
                        "additionalProperties":false
                    }),
                    strict: true,
                }],
                max_output_tokens: ModelOutputTokenLimit::new(128).expect("output limit"),
            },
            user_text: "Request the external probe.".into(),
            instruction: put::<InstructionBlock>(
                &mut content,
                &serde_json::json!({"text":"Request the external probe."}),
            ),
            tool_catalog: put::<ToolCatalog>(
                &mut content,
                &serde_json::json!({"schema_version":1,"tools":["request_external_probe"]}),
            ),
            history: put::<HistoryItem>(
                &mut content,
                &serde_json::json!({"role":"user","content":"Request the external probe."}),
            ),
            context: put::<ContextBlock>(
                &mut content,
                &serde_json::json!({"schema_version":1,"knowledge_snapshot":{"kind":"empty"}}),
            ),
            policy: put::<PolicyDocument>(
                &mut content,
                &serde_json::json!({"schema_version":1,"external_effect":"controller-only"}),
            ),
            capability_grant: AgentStepCapabilityGrantV1::new(vec![ToolRegistration::new(
                tool,
                ToolImplementationVersion::new("external-probe-v1").expect("version"),
                ToolEffectClass::Idempotent,
            )])
            .expect("capability grant"),
        };
        let request_response = serde_json::to_vec(&serde_json::json!({
            "output":[{
                "type":"function_call",
                "call_id":"external-probe",
                "name":"request_external_probe",
                "arguments":"{}"
            }]
        }))
        .expect("response");
        let terminal_response = serde_json::to_vec(&serde_json::json!({
            "output":[{
                "type":"message",
                "id":"external-probe-final",
                "phase":"final_answer",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":"probe observed"}]
            }]
        }))
        .expect("terminal response");
        let dispatches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transport_dispatches = Arc::clone(&dispatches);
        let mut transport = ScriptedModelTransport::new(
            move |_: &crate::PreparedModelRequest| -> Result<_, TransportError> {
                let index = transport_dispatches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ModelTransportResponse::without_usage(if index == 0 {
                    request_response.clone()
                } else {
                    terminal_response.clone()
                }))
            },
        );
        let invoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let invoked_by_gateway = Arc::clone(&invoked);
        let mut gateway = crate::ScriptedToolGateway::new(
            move |_: &PreparedToolOperation| -> Result<_, ToolGatewayError> {
                invoked_by_gateway.store(true, std::sync::atomic::Ordering::SeqCst);
                CanonicalToolResult::from_value(&serde_json::json!({"unexpected":true}))
                    .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            },
        );
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");

        let outcome = drive_agent_episode_step(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("workflow must return a Worker request before external authority");
        let AgentEpisodeDriverStepOutcomeV1::WorkerRequest(request) = outcome else {
            panic!("external effect must produce a Worker request")
        };
        assert_eq!(request.episode_id, frozen.episode_id);
        assert_eq!(request.operations.len(), 1);
        assert_eq!(
            request.operations[0].tool.as_str(),
            "request_external_probe"
        );
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));

        let episode = AgentEpisode::new(frozen.episode_id).expect("episode");
        let AgentEpisodeState::Active {
            step_state: AgentStepState::OperationsBound(bound),
            ..
        } = recover_agent_episode(&events, &mut content, &episode).expect("recover Worker request")
        else {
            panic!("Worker request must preserve bound operations")
        };
        let operation = bound.into_operations().pop().expect("external operation");
        let authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            observed_now().expect("time"),
            operation,
        )
        .expect("Controller authorization");
        let started = begin_tool_operation(
            &mut events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            observed_now().expect("time"),
        )
        .expect("durable start");
        let controller_invoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let controller_gateway_invoked = Arc::clone(&controller_invoked);
        let mut controller_gateway = crate::ScriptedToolGateway::new(
            move |_: &PreparedToolOperation| -> Result<_, ToolGatewayError> {
                controller_gateway_invoked.store(true, std::sync::atomic::Ordering::SeqCst);
                CanonicalToolResult::from_value(&serde_json::json!({
                    "schema_version":1,
                    "provenance":{"worker_receipt":"recorded-controller-test"},
                    "observation":{"reachable":true}
                }))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
            },
        );
        let _ = execute_tool_operation(
            &mut events,
            &mut content,
            &mut controller_gateway,
            started,
            &CommandId::new(),
            observed_now().expect("time"),
        )
        .expect("Controller records observation");
        assert!(controller_invoked.load(std::sync::atomic::Ordering::SeqCst));

        let resumed = drive_agent_episode_step(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("resume exact episode");
        assert!(matches!(resumed, AgentEpisodeDriverStepOutcomeV1::Continue));
        let terminal = drive_agent_episode_step(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("next exact model step");
        let AgentEpisodeDriverStepOutcomeV1::Complete(completion) = terminal else {
            panic!("recorded observation must continue to a terminal model step")
        };
        assert_eq!(completion.reason, EpisodeCompletionReason::Yielded);
        assert_eq!(completion.steps_started, 2);
        assert_eq!(dispatches.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));
    }
}
