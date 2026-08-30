//! One task-generic durable Agent Loop used by every Proposal Host profile.

use cairn_agent::{
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

/// Exact trusted tool implementations and effect classes granted to one Proposal Host episode.
#[derive(Debug)]
pub(crate) struct ProposalLoopCapabilityGrantV1(Vec<ToolRegistration>);

impl ProposalLoopCapabilityGrantV1 {
    pub(crate) fn new(registrations: Vec<ToolRegistration>) -> Result<Self, ProposalLoopError> {
        if registrations.is_empty() {
            return Err(ProposalLoopError::InvalidCapabilityGrant(
                "capability grant has no tools".into(),
            ));
        }
        let mut names = std::collections::HashSet::new();
        if registrations
            .iter()
            .any(|registration| !names.insert(registration.name().as_str()))
        {
            return Err(ProposalLoopError::InvalidCapabilityGrant(
                "capability grant repeats a tool name".into(),
            ));
        }
        Ok(Self(registrations))
    }

    fn registration(&self, name: &ToolName) -> Result<ToolRegistration, ProposalLoopError> {
        self.0
            .iter()
            .find(|registration| registration.name() == name)
            .cloned()
            .ok_or_else(|| ProposalLoopError::UnavailableTool(name.as_str().to_owned()))
    }
}

/// Exact model-visible input, profile, tool catalog, budget, and capability registrations frozen
/// before one Proposal Host episode is opened.
pub(crate) struct FrozenProposalLoopV1 {
    pub(crate) task_id: TaskId,
    pub(crate) episode_id: EpisodeId,
    pub(crate) role: AgentRoleName,
    pub(crate) selection: ModelSelection,
    pub(crate) budget: EpisodeBudget,
    pub(crate) native_spec: NativeRequestSpec,
    pub(crate) user_text: String,
    pub(crate) instruction: ContentId<InstructionBlock>,
    pub(crate) tool_catalog: ContentId<ToolCatalog>,
    pub(crate) history: ContentId<HistoryItem>,
    pub(crate) context: ContentId<cairn_agent::ContextBlock>,
    pub(crate) policy: ContentId<PolicyDocument>,
    pub(crate) capability_grant: ProposalLoopCapabilityGrantV1,
}

impl FrozenProposalLoopV1 {
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

/// Durable terminal position reached by the common Agent Loop before domain submission freezing.
#[derive(Debug)]
pub(crate) struct ProposalLoopCompletionV1 {
    pub(crate) reason: EpisodeCompletionReason,
    pub(crate) steps_started: u32,
}

/// Exact non-authoritative external operation yielded to the Controller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProposalLoopExperimentOperationV1 {
    pub(crate) operation_id: OperationId,
    pub(crate) tool: ToolName,
    pub(crate) implementation_version: cairn_agent::ToolImplementationVersion,
    pub(crate) effect: ToolEffectClass,
    pub(crate) arguments_id: ContentId<cairn_agent::ToolArguments>,
    pub(crate) arguments: serde_json::Value,
}

/// Durable same-episode safe point returned before any external effect receives authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProposalLoopExperimentRequestV1 {
    pub(crate) episode_id: EpisodeId,
    pub(crate) step_id: StepId,
    pub(crate) model_attempt_id: ModelAttemptId,
    pub(crate) operations: Vec<ProposalLoopExperimentOperationV1>,
}

#[derive(Debug)]
pub(crate) enum ProposalLoopOutcomeV1 {
    Complete(ProposalLoopCompletionV1),
    AwaitingController(ProposalLoopExperimentRequestV1),
}

pub(crate) enum ProposalProfileOutcomeV1<T> {
    Complete(T),
    AwaitingController(ProposalLoopExperimentRequestV1),
}

/// Failure while driving the common Proposal Host Agent Loop.
#[derive(Debug, Error)]
pub(crate) enum ProposalLoopError {
    #[error("Proposal Host Agent Loop failed: {0}")]
    Agent(String),
    #[error("Proposal Host profile does not grant tool {0}")]
    UnavailableTool(String),
    #[error("Proposal Host capability grant is invalid: {0}")]
    InvalidCapabilityGrant(String),
}

struct OpenedProposalEpisodeV1 {
    episode: AgentEpisode,
    authority: EpisodeStepAuthority,
    native: PreparedNativeRequest,
    pending_results: Vec<ContentId<OperationResult>>,
}

struct DispatchedProposalTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
}

struct SettledProposalTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
    proposed_tools: Vec<ToolName>,
}

struct AdmittedProposalTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
    operations: Vec<PreparedToolOperation>,
}

struct ObservedProposalTurnV1 {
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    continuation: NativeContinuation,
}

struct ProjectedProposalTurnV1 {
    episode: AgentEpisode,
    native: PreparedNativeRequest,
    pending_results: Vec<ContentId<OperationResult>>,
}

enum ProposalLoopStageV1<T> {
    Active(T),
    Complete(ProposalLoopCompletionV1),
}

enum ProposalLoopAdvanceV1 {
    Continue(OpenedProposalEpisodeV1),
    Complete(ProposalLoopCompletionV1),
}

enum ProposalLoopPositionV1 {
    ReadyForAgent(OpenedProposalEpisodeV1),
    ReadyForOperations(AdmittedProposalTurnV1),
    ReadyForProjection(ObservedProposalTurnV1),
    Complete(ProposalLoopCompletionV1),
}

enum ProposalLoopOperationOutcomeV1 {
    Observed(ObservedProposalTurnV1),
    AwaitingController(ProposalLoopExperimentRequestV1),
}

/// Opens and drives one durable Agent Loop from only the frozen profile and capability surface.
///
/// Model dispatch and every Host-local tool operation receive durable start authority before their
/// effect. Canonical tool results are archived as provenance-bearing `OperationResult` artifacts
/// before they are projected into the next native continuation. Mutating or ambiguous external
/// effects are never executed by the Proposal Host.
pub(crate) fn run_proposal_loop<E, C, T, G>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
    gateway: &mut G,
) -> Result<ProposalLoopOutcomeV1, ProposalLoopError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
    G: ToolGateway,
{
    let mut position = open_or_recover_proposal_episode(events, content, codec, frozen)?;

    loop {
        position = match position {
            ProposalLoopPositionV1::ReadyForAgent(opened) => {
                prepare_agent_operations(events, content, transport, codec, frozen, opened)?
            }
            ProposalLoopPositionV1::ReadyForOperations(admitted) => {
                match execute_or_yield_operations(events, content, gateway, admitted)? {
                    ProposalLoopOperationOutcomeV1::Observed(observed) => {
                        ProposalLoopPositionV1::ReadyForProjection(observed)
                    }
                    ProposalLoopOperationOutcomeV1::AwaitingController(request) => {
                        return Ok(ProposalLoopOutcomeV1::AwaitingController(request));
                    }
                }
            }
            ProposalLoopPositionV1::ReadyForProjection(observed) => {
                project_and_advance(events, content, codec, frozen, observed)?
            }
            ProposalLoopPositionV1::Complete(completion) => {
                return Ok(ProposalLoopOutcomeV1::Complete(completion));
            }
        };
    }
}

fn prepare_agent_operations<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
    opened: OpenedProposalEpisodeV1,
) -> Result<ProposalLoopPositionV1, ProposalLoopError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let dispatched = dispatch_agent_turn(events, content, transport, frozen, opened)?;
    let settled = settle_agent_turn(events, content, codec, dispatched)?;
    let admitted = admit_agent_operations(events, content, frozen, settled)?;
    Ok(match admitted {
        ProposalLoopStageV1::Active(admitted) => {
            ProposalLoopPositionV1::ReadyForOperations(admitted)
        }
        ProposalLoopStageV1::Complete(completion) => ProposalLoopPositionV1::Complete(completion),
    })
}

fn project_and_advance<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
    observed: ObservedProposalTurnV1,
) -> Result<ProposalLoopPositionV1, ProposalLoopError> {
    let projected = project_operation_observations(
        events,
        content,
        codec,
        frozen,
        ProposalLoopStageV1::Active(observed),
    )?;
    Ok(
        match advance_proposal_episode(events, content, projected)? {
            ProposalLoopAdvanceV1::Continue(next) => ProposalLoopPositionV1::ReadyForAgent(next),
            ProposalLoopAdvanceV1::Complete(completion) => {
                ProposalLoopPositionV1::Complete(completion)
            }
        },
    )
}

fn open_or_recover_proposal_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
) -> Result<ProposalLoopPositionV1, ProposalLoopError> {
    let episode = AgentEpisode::new(frozen.episode_id).map_err(agent_error)?;
    match recover_agent_episode(events, content, &episode).map_err(agent_error)? {
        AgentEpisodeState::NotFound => open_new_proposal_episode(events, codec, frozen, episode)
            .map(ProposalLoopPositionV1::ReadyForAgent),
        AgentEpisodeState::Active {
            step,
            model_attempt_id,
            step_state,
        } => recover_active_proposal_episode(
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
        } => Ok(ProposalLoopPositionV1::Complete(ProposalLoopCompletionV1 {
            reason,
            steps_started,
        })),
        AgentEpisodeState::ReadyToPrepare(_) => Err(ProposalLoopError::Agent(
            "Proposal Host recovery stopped between episode advance and model preparation".into(),
        )),
    }
}

fn open_new_proposal_episode<E: EventStore>(
    events: &mut E,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
    episode: AgentEpisode,
) -> Result<OpenedProposalEpisodeV1, ProposalLoopError> {
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
    Ok(OpenedProposalEpisodeV1 {
        episode,
        authority,
        native,
        pending_results: Vec::new(),
    })
}

fn recover_active_proposal_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    episode: AgentEpisode,
    step: AgentStep,
    attempt_id: ModelAttemptId,
    step_state: AgentStepState,
) -> Result<ProposalLoopPositionV1, ProposalLoopError> {
    match step_state {
        AgentStepState::OperationsBound(bound) => {
            let continuation =
                recover_native_continuation(events, content, codec, &step, attempt_id)?;
            Ok(ProposalLoopPositionV1::ReadyForOperations(
                AdmittedProposalTurnV1 {
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
            Ok(ProposalLoopPositionV1::ReadyForProjection(
                ObservedProposalTurnV1 {
                    episode,
                    step,
                    attempt_id,
                    continuation,
                },
            ))
        }
        AgentStepState::Yielded { .. } => {
            complete_episode(events, content, &episode).map(ProposalLoopPositionV1::Complete)
        }
        _ => Err(ProposalLoopError::Agent(
            "Proposal Host episode is not at a recoverable external-effect safe point".into(),
        )),
    }
}

fn recover_native_continuation<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    codec: NativeProtocolCodec,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
) -> Result<NativeContinuation, ProposalLoopError> {
    codec
        .recover_recorded(events, content, step.stream_id(), attempt_id)
        .map_err(agent_error)?
        .map(|(_, continuation)| continuation)
        .ok_or_else(|| {
            ProposalLoopError::Agent(
                "active Proposal Host step has no durable native continuation".into(),
            )
        })
}

fn dispatch_agent_turn<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    frozen: &FrozenProposalLoopV1,
    opened: OpenedProposalEpisodeV1,
) -> Result<DispatchedProposalTurnV1, ProposalLoopError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let OpenedProposalEpisodeV1 {
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
        DispatchCompletion::Response(_) => Ok(DispatchedProposalTurnV1 {
            episode,
            step,
            attempt_id,
        }),
        DispatchCompletion::NotSent { diagnostic }
        | DispatchCompletion::Rejected { diagnostic }
        | DispatchCompletion::Ambiguous { diagnostic } => Err(ProposalLoopError::Agent(diagnostic)),
    }
}

fn settle_agent_turn<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    dispatched: DispatchedProposalTurnV1,
) -> Result<ProposalLoopStageV1<SettledProposalTurnV1>, ProposalLoopError> {
    let DispatchedProposalTurnV1 {
        episode,
        step,
        attempt_id,
    } = dispatched;
    let AgentStepState::ReadyToDecode(received) =
        recover_agent_step(events, content, &step, attempt_id).map_err(agent_error)?
    else {
        return Err(ProposalLoopError::Agent(
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
        return complete_episode(events, content, &episode).map(ProposalLoopStageV1::Complete);
    }
    Ok(ProposalLoopStageV1::Active(SettledProposalTurnV1 {
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
    frozen: &FrozenProposalLoopV1,
    settled: ProposalLoopStageV1<SettledProposalTurnV1>,
) -> Result<ProposalLoopStageV1<AdmittedProposalTurnV1>, ProposalLoopError> {
    let settled = match settled {
        ProposalLoopStageV1::Active(settled) => settled,
        ProposalLoopStageV1::Complete(completion) => {
            return Ok(ProposalLoopStageV1::Complete(completion));
        }
    };
    let SettledProposalTurnV1 {
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
        .collect::<Result<Vec<_>, ProposalLoopError>>()?;
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
            Ok(ProposalLoopStageV1::Active(AdmittedProposalTurnV1 {
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
        } => Ok(ProposalLoopStageV1::Complete(ProposalLoopCompletionV1 {
            reason,
            steps_started,
        })),
    }
}

fn execute_or_yield_operations<E, C, G>(
    events: &mut E,
    content: &mut C,
    gateway: &mut G,
    admitted: AdmittedProposalTurnV1,
) -> Result<ProposalLoopOperationOutcomeV1, ProposalLoopError>
where
    E: EventStore,
    C: ContentStore,
    G: ToolGateway,
{
    let AdmittedProposalTurnV1 {
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
                external.push(experiment_operation(&operation)?);
            }
            continue;
        }
        if matches!(state, ToolOperationState::Completed { .. }) {
            continue;
        }
        if !matches!(state, ToolOperationState::NotFound) {
            return Err(ProposalLoopError::Agent(
                "Host-local tool operation requires explicit reconciliation".into(),
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
        return Ok(ProposalLoopOperationOutcomeV1::AwaitingController(
            ProposalLoopExperimentRequestV1 {
                episode_id: episode.episode_id(),
                step_id: step.step_id(),
                model_attempt_id: attempt_id,
                operations: external,
            },
        ));
    }
    Ok(ProposalLoopOperationOutcomeV1::Observed(
        ObservedProposalTurnV1 {
            episode,
            step,
            attempt_id,
            continuation,
        },
    ))
}

fn experiment_operation(
    operation: &PreparedToolOperation,
) -> Result<ProposalLoopExperimentOperationV1, ProposalLoopError> {
    let arguments = cairn_codec::from_slice(operation.argument_bytes()).map_err(agent_error)?;
    Ok(ProposalLoopExperimentOperationV1 {
        operation_id: operation.operation_id(),
        tool: operation.tool().clone(),
        implementation_version: operation.implementation_version().clone(),
        effect: operation.effect(),
        arguments_id: operation.arguments_id(),
        arguments,
    })
}

fn project_operation_observations<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    codec: NativeProtocolCodec,
    frozen: &FrozenProposalLoopV1,
    observed: ProposalLoopStageV1<ObservedProposalTurnV1>,
) -> Result<ProposalLoopStageV1<ProjectedProposalTurnV1>, ProposalLoopError> {
    let observed = match observed {
        ProposalLoopStageV1::Active(observed) => observed,
        ProposalLoopStageV1::Complete(completion) => {
            return Ok(ProposalLoopStageV1::Complete(completion));
        }
    };
    let ObservedProposalTurnV1 {
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
        return Err(ProposalLoopError::Agent(
            "tool operation requires explicit reconciliation".into(),
        ));
    };
    let continuation = codec
        .append_archived_tool_results(content, &continuation, &results)
        .map_err(agent_error)?;
    let native = codec
        .prepare_continuation(&frozen.native_spec, &continuation)
        .map_err(agent_error)?;
    Ok(ProposalLoopStageV1::Active(ProjectedProposalTurnV1 {
        episode,
        native,
        pending_results: results,
    }))
}

fn advance_proposal_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    projected: ProposalLoopStageV1<ProjectedProposalTurnV1>,
) -> Result<ProposalLoopAdvanceV1, ProposalLoopError> {
    let projected = match projected {
        ProposalLoopStageV1::Active(projected) => projected,
        ProposalLoopStageV1::Complete(completion) => {
            return Ok(ProposalLoopAdvanceV1::Complete(completion));
        }
    };
    let ProjectedProposalTurnV1 {
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
        EpisodeAdvance::NextStep(authority) => {
            Ok(ProposalLoopAdvanceV1::Continue(OpenedProposalEpisodeV1 {
                episode,
                authority,
                native,
                pending_results,
            }))
        }
        EpisodeAdvance::Completed {
            reason,
            steps_started,
        } => Ok(ProposalLoopAdvanceV1::Complete(ProposalLoopCompletionV1 {
            reason,
            steps_started,
        })),
    }
}

fn complete_episode<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    episode: &AgentEpisode,
) -> Result<ProposalLoopCompletionV1, ProposalLoopError> {
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
        return Err(ProposalLoopError::Agent(
            "yielded step unexpectedly advanced".into(),
        ));
    };
    Ok(ProposalLoopCompletionV1 {
        reason,
        steps_started,
    })
}

fn observed_now() -> Result<ObservedAtUnixMillis, ProposalLoopError> {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(agent_error)?
        .as_millis();
    let milliseconds = i64::try_from(milliseconds)
        .map_err(|_| ProposalLoopError::Agent("wall clock overflow".into()))?;
    Ok(ObservedAtUnixMillis::new(milliseconds))
}

fn agent_error(error: impl std::fmt::Display) -> ProposalLoopError {
    ProposalLoopError::Agent(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use cairn_agent::{
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

        let error = ProposalLoopCapabilityGrantV1::new(vec![registration.clone(), registration])
            .expect_err("duplicate semantic capability must be rejected");

        assert!(matches!(
            error,
            ProposalLoopError::InvalidCapabilityGrant(message)
                if message == "capability grant repeats a tool name"
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn external_effect_proposal_is_durably_bound_but_never_executed_in_host() {
        let temporary = tempfile::tempdir().expect("temporary state");
        let mut content = SqliteContentStore::open(
            temporary.path().join("content.db"),
            temporary.path().join("cas"),
        )
        .expect("content");
        let mut events =
            SqliteEventStore::open(temporary.path().join("events.db")).expect("events");
        let tool = ToolName::new("request_external_probe").expect("tool");
        let frozen = FrozenProposalLoopV1 {
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
            capability_grant: ProposalLoopCapabilityGrantV1::new(vec![ToolRegistration::new(
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
            move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
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
        let mut gateway = cairn_agent::ScriptedToolGateway::new(
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

        let outcome = run_proposal_loop(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("Host must durably yield external authority to Controller");
        let ProposalLoopOutcomeV1::AwaitingController(request) = outcome else {
            panic!("external effect must yield")
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
        } = recover_agent_episode(&events, &mut content, &episode).expect("recover yield")
        else {
            panic!("yield must preserve bound operations")
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
        let mut controller_gateway = cairn_agent::ScriptedToolGateway::new(
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

        let resumed = run_proposal_loop(
            &mut events,
            &mut content,
            &mut transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .expect("resume exact episode");
        let ProposalLoopOutcomeV1::Complete(completion) = resumed else {
            panic!("recorded observation must resume to terminal")
        };
        assert_eq!(completion.reason, EpisodeCompletionReason::Yielded);
        assert_eq!(completion.steps_started, 2);
        assert_eq!(dispatches.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));
    }
}
