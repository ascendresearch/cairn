use std::collections::HashSet;

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EpisodeId, EventId, ModelAttemptId,
    ObservedAtUnixMillis, OperationId, SchemaName, SchemaVersion, StepId, TaskId,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dispatch::recover_turn_input_decision;
use crate::{
    AgentRoleName, AgentStep, AgentStepState, BoundStepOperations, DispatchAuthority,
    ModelAttemptState, OperationResult, PreparedNativeRequest, PreparedToolOperation,
    StepCoordinatorError, ToolCallProposal, ToolEffectClass, ToolImplementationVersion, ToolName,
    ToolOperationAssignment, TurnInputDecision, bind_step_operations, prepare_agent_step,
    prepare_native_agent_step, recover_agent_step, recover_model_attempt,
};

const EPISODE_OPENED: &str = "agent.episode-opened";
const OPERATIONS_ADMITTED: &str = "agent.episode-operations-admitted";
const STEP_ADVANCED: &str = "agent.episode-step-advanced";
const EPISODE_COMPLETED: &str = "agent.episode-completed";

/// Invalid bounded episode configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("episode step limit must be greater than zero")]
pub struct EpisodeValueError;

/// Positive maximum number of model steps an episode may start.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct EpisodeStepLimit(u32);

impl EpisodeStepLimit {
    /// Creates a positive step limit.
    ///
    /// # Errors
    ///
    /// Returns [`EpisodeValueError`] when `value` is zero.
    pub const fn new(value: u32) -> Result<Self, EpisodeValueError> {
        if value == 0 {
            Err(EpisodeValueError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the configured maximum.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for EpisodeStepLimit {
    type Error = EpisodeValueError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<EpisodeStepLimit> for u32 {
    fn from(value: EpisodeStepLimit) -> Self {
        value.0
    }
}

/// Maximum number of logical tool operations an episode may admit.
///
/// Zero is valid and creates a model-only episode. Retries are attempts of an already admitted
/// logical operation and are deliberately not charged against this dimension.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EpisodeToolOperationLimit(u32);

impl EpisodeToolOperationLimit {
    /// Creates a logical tool-operation limit.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the configured maximum.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for EpisodeToolOperationLimit {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<EpisodeToolOperationLimit> for u32 {
    fn from(value: EpisodeToolOperationLimit) -> Self {
        value.0
    }
}

/// Invalid observed provider-token limit.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("episode provider-token limit must be greater than zero")]
pub struct EpisodeProviderTokenLimitError;

/// Positive observed provider-token threshold that blocks the next model step.
///
/// A response may cross this threshold because its usage is unknowable before dispatch. Once the
/// cumulative provider receipt reaches it, Cairn grants no further model authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct EpisodeProviderTokenLimit(u64);

impl EpisodeProviderTokenLimit {
    /// Creates a positive observed token threshold.
    ///
    /// # Errors
    ///
    /// Returns [`EpisodeProviderTokenLimitError`] when `value` is zero.
    pub const fn new(value: u64) -> Result<Self, EpisodeProviderTokenLimitError> {
        if value == 0 {
            Err(EpisodeProviderTokenLimitError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the configured threshold.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for EpisodeProviderTokenLimit {
    type Error = EpisodeProviderTokenLimitError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<EpisodeProviderTokenLimit> for u64 {
    fn from(value: EpisodeProviderTokenLimit) -> Self {
        value.0
    }
}

/// Absolute wall-clock safe-point deadline for an episode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EpisodeDeadlineUnixMillis(i64);

impl EpisodeDeadlineUnixMillis {
    /// Creates an absolute deadline.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the wire timestamp.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    const fn is_reached(self, observed_at: ObservedAtUnixMillis) -> bool {
        observed_at.get() >= self.0
    }
}

/// V1 episode budgets enforce only dimensions currently derived from trusted durable facts.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeBudget {
    /// Optional maximum number of model steps that may start; `None` disables the dimension.
    #[serde(default)]
    pub step_limit: Option<EpisodeStepLimit>,
    /// Optional logical-operation limit; `None` disables the dimension.
    #[serde(default)]
    pub tool_operation_limit: Option<EpisodeToolOperationLimit>,
    /// Optional observed provider-token threshold. Missing receipts fail closed when configured.
    #[serde(default)]
    pub provider_token_limit: Option<EpisodeProviderTokenLimit>,
    /// Optional absolute deadline; `None` disables wall-clock budget checks.
    #[serde(default)]
    pub deadline_unix_ms: Option<EpisodeDeadlineUnixMillis>,
    /// Optional per-meter reservation ceilings; `None` disables external-meter enforcement.
    ///
    /// An enabled empty list rejects every external meter. Each meter may appear at most once.
    #[serde(default)]
    pub external_meter_limits: Option<Vec<crate::EpisodeExternalMeterLimit>>,
}

/// Durable episode aggregate boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentEpisode {
    episode_id: EpisodeId,
    stream: StreamId,
}

impl AgentEpisode {
    /// Creates the canonical stream for an episode identity.
    ///
    /// # Errors
    ///
    /// Returns [`EpisodeCoordinatorError`] if its stream representation is invalid.
    pub fn new(episode_id: EpisodeId) -> Result<Self, EpisodeCoordinatorError> {
        let stream = StreamId {
            kind: AggregateKind::new("agent-episode")
                .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
            id: AggregateId::new(episode_id.to_string())
                .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        };
        Ok(Self { episode_id, stream })
    }

    /// Returns the stable episode identity.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the canonical event stream.
    #[must_use]
    pub const fn stream_id(&self) -> &StreamId {
        &self.stream
    }
}

/// Why an episode stopped granting new step authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpisodeCompletionReason {
    /// The model yielded without proposing more operations.
    Yielded,
    /// Starting another step would exceed the configured step limit.
    StepLimitReached,
    /// The deadline was reached at a durable safe boundary.
    DeadlineReached,
    /// The current proposals would exceed the logical tool-operation limit.
    ToolOperationLimitReached,
    /// Reported provider tokens reached the threshold for granting another model step.
    ProviderTokenLimitReached,
    /// A configured provider-token budget could not be enforced because a receipt was absent.
    ProviderUsageUnavailable,
}

/// Budget-backed permit containing the durable tool bindings for the current episode step.
///
/// ```compile_fail
/// use cairn_agent::EpisodeOperationAdmission;
///
/// let forged = EpisodeOperationAdmission {
///     episode_id: todo!(),
///     step_id: todo!(),
///     bound: todo!(),
/// };
/// ```
#[derive(Debug)]
pub struct EpisodeOperationAdmission {
    episode_id: EpisodeId,
    step_id: StepId,
    bound: BoundStepOperations,
}

impl EpisodeOperationAdmission {
    /// Returns the episode that admitted these operations.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the step whose proposals were admitted.
    #[must_use]
    pub const fn step_id(&self) -> StepId {
        self.step_id
    }

    /// Borrows the admitted, durable operations in proposal order.
    #[must_use]
    pub fn operations(&self) -> &[PreparedToolOperation] {
        self.bound.operations()
    }

    /// Consumes the permit into independently authorizable operations.
    #[must_use]
    pub fn into_operations(self) -> Vec<PreparedToolOperation> {
        self.bound.into_operations()
    }
}

/// Result of applying episode tool-operation policy at the pre-authority boundary.
#[derive(Debug)]
pub enum EpisodeOperationAdmissionOutcome {
    /// The logical operations fit the budget and have durable step bindings.
    Admitted(EpisodeOperationAdmission),
    /// The proposals exceeded the budget and durably completed the episode.
    Completed {
        /// Terminal reason.
        reason: EpisodeCompletionReason,
        /// Number of model steps started.
        steps_started: u32,
    },
}

/// One-shot authority to prepare the current episode step.
///
/// ```compile_fail
/// use cairn_agent::EpisodeStepAuthority;
///
/// let forged = EpisodeStepAuthority {
///     episode_id: todo!(),
///     step: todo!(),
///     model_attempt_id: todo!(),
///     expected_pending_results: Vec::new(),
/// };
/// ```
#[derive(Debug)]
pub struct EpisodeStepAuthority {
    episode_id: EpisodeId,
    step: AgentStep,
    model_attempt_id: ModelAttemptId,
    expected_pending_results: Vec<ContentId<OperationResult>>,
    previous_step: Option<PreviousEpisodeStepV1>,
}

/// Durable predecessor needed to reconstruct a protocol-native continuation at a step boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviousEpisodeStepV1 {
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
}

impl PreviousEpisodeStepV1 {
    /// Returns the preceding durable step identity.
    #[must_use]
    pub const fn step_id(self) -> StepId {
        self.step_id
    }

    /// Returns the preceding model attempt identity.
    #[must_use]
    pub const fn model_attempt_id(self) -> ModelAttemptId {
        self.model_attempt_id
    }
}

impl EpisodeStepAuthority {
    /// Returns the episode that granted this step authority.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the step identity granted by this authority.
    #[must_use]
    pub const fn step_id(&self) -> StepId {
        self.step.step_id()
    }

    /// Returns the concrete model attempt identity reserved for the step.
    #[must_use]
    pub const fn model_attempt_id(&self) -> ModelAttemptId {
        self.model_attempt_id
    }

    /// Returns the exact operation results the next input decision must carry.
    #[must_use]
    pub fn expected_pending_results(&self) -> &[ContentId<OperationResult>] {
        &self.expected_pending_results
    }

    /// Returns the predecessor whose recorded native continuation produced this step.
    #[must_use]
    pub const fn previous_step(&self) -> Option<PreviousEpisodeStepV1> {
        self.previous_step
    }
}

/// Recovered durable episode position.
#[derive(Debug)]
pub enum AgentEpisodeState {
    /// No episode fact exists.
    NotFound,
    /// Current step is authorized by the episode but has not been prepared.
    ReadyToPrepare(EpisodeStepAuthority),
    /// Current step owns the next safe action.
    Active {
        /// Current step aggregate.
        step: AgentStep,
        /// Model attempt reserved for this step.
        model_attempt_id: ModelAttemptId,
        /// Recovered step-local position.
        step_state: AgentStepState,
    },
    /// Episode has a durable terminal fact.
    Completed {
        /// Terminal reason.
        reason: EpisodeCompletionReason,
        /// Number of model steps started in this episode.
        steps_started: u32,
    },
}

/// Result of driving one completed step boundary.
pub enum EpisodeAdvance {
    /// A new step has durable episode authority.
    NextStep(EpisodeStepAuthority),
    /// Episode terminated instead of granting another step.
    Completed {
        /// Terminal reason.
        reason: EpisodeCompletionReason,
        /// Number of model steps started.
        steps_started: u32,
    },
}

/// Failure while coordinating or rebuilding an episode.
#[derive(Debug, Error)]
pub enum EpisodeCoordinatorError {
    /// Episode event storage failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Step-local coordination failed.
    #[error(transparent)]
    Step(#[from] StepCoordinatorError),
    /// Durable episode facts or requested transitions violate the protocol.
    #[error("invalid agent episode: {0}")]
    InvalidEpisode(String),
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OpenedPayload {
    episode_id: EpisodeId,
    task_id: TaskId,
    role: AgentRoleName,
    budget: EpisodeBudget,
    first_step_id: StepId,
    first_model_attempt_id: ModelAttemptId,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AdvancedPayload {
    episode_id: EpisodeId,
    previous_step_id: StepId,
    next_step_id: StepId,
    next_model_attempt_id: ModelAttemptId,
    step_ordinal: u32,
    pending_results: Vec<ContentId<OperationResult>>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CompletedPayload {
    episode_id: EpisodeId,
    last_step_id: StepId,
    reason: EpisodeCompletionReason,
    steps_started: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requested_tool_operations: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observed_provider_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    missing_provider_usage_attempt_id: Option<ModelAttemptId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AdmittedAssignmentPayload {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationsAdmittedPayload {
    episode_id: EpisodeId,
    step_id: StepId,
    assignments: Vec<AdmittedAssignmentPayload>,
}

#[derive(Clone)]
struct StepEntry {
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    expected_pending_results: Vec<ContentId<OperationResult>>,
    admitted_operations: Option<Vec<AdmittedAssignmentPayload>>,
    admission_command_id: Option<CommandId>,
    admission_observed_at_unix_ms: Option<i64>,
}

struct EpisodeProjection {
    opened: OpenedPayload,
    steps: Vec<StepEntry>,
    completion: Option<CompletedPayload>,
}

pub(crate) struct EpisodeBudgetSnapshot {
    pub(crate) budget: EpisodeBudget,
    pub(crate) completed: bool,
}

pub(crate) fn recover_budget_snapshot<E: EventStore>(
    events: &E,
    episode: &AgentEpisode,
) -> Result<EpisodeBudgetSnapshot, EpisodeCoordinatorError> {
    let history = events.read_stream(episode.stream_id(), None)?;
    if history.is_empty() {
        return invalid_episode("metering episode does not exist");
    }
    let projection = project_episode(&history, episode.episode_id())?;
    Ok(EpisodeBudgetSnapshot {
        budget: projection.opened.budget,
        completed: projection.completion.is_some(),
    })
}

#[derive(Clone, Copy)]
enum CompletionEvidence {
    None,
    ToolOperationsRequested(u32),
    ProviderTokensObserved(u64),
    ProviderUsageMissing(ModelAttemptId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EpisodeProviderUsage {
    total_tokens: u64,
    missing_attempt_id: Option<ModelAttemptId>,
}

/// Opens an episode and grants authority for its first step.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] if the deadline has already elapsed or commit fails.
#[allow(clippy::too_many_arguments)]
pub fn open_agent_episode<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    task_id: TaskId,
    role: AgentRoleName,
    budget: EpisodeBudget,
    first_step_id: StepId,
    first_model_attempt_id: ModelAttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<EpisodeStepAuthority, EpisodeCoordinatorError> {
    crate::metering::validate_external_meter_limits(budget.external_meter_limits.as_deref())
        .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?;
    if budget
        .deadline_unix_ms
        .is_some_and(|deadline| deadline.is_reached(observed_at))
    {
        return invalid_episode("episode deadline must be later than its open observation");
    }
    let payload = OpenedPayload {
        episode_id: episode.episode_id,
        task_id,
        role,
        budget,
        first_step_id,
        first_model_attempt_id,
    };
    let history = events.read_stream(&episode.stream, None)?;
    if !history.is_empty() {
        let projection = project_episode(&history, episode.episode_id)?;
        let first = history.first().ok_or_else(|| {
            EpisodeCoordinatorError::InvalidEpisode("episode history disappeared".into())
        })?;
        if first.command_id != *command_id
            || first.observed_at_unix_ms != observed_at.get()
            || projection.opened != payload
            || projection.steps.len() != 1
            || projection.completion.is_some()
        {
            return invalid_episode("episode-open command conflicts with durable history");
        }
        let step = AgentStep::new(first_step_id)?;
        if !events.read_stream(step.stream_id(), None)?.is_empty() {
            return invalid_episode("replayed first-step authority was already consumed");
        }
        return step_authority(
            episode.episode_id,
            first_step_id,
            first_model_attempt_id,
            Vec::new(),
            None,
        );
    }
    let event = episode_fact(EPISODE_OPENED, None, observed_at, &payload)?;
    events.append(
        &episode.stream,
        ExpectedRevision::NoStream,
        command_id,
        &[event],
    )?;
    tracing::info!(
        target: "cairn.agent.episode",
        event = "agent_episode_opened",
        episode_id = %episode.episode_id,
        task_id = %task_id,
        role = %payload.role.as_str(),
        first_step_id = %first_step_id,
        first_model_attempt_id = %first_model_attempt_id,
        step_limit = payload.budget.step_limit.map(EpisodeStepLimit::get),
        tool_operation_limit = payload.budget.tool_operation_limit.map(EpisodeToolOperationLimit::get),
        provider_token_limit = payload.budget.provider_token_limit.map(EpisodeProviderTokenLimit::get),
        deadline_unix_ms = payload.budget.deadline_unix_ms.map(EpisodeDeadlineUnixMillis::get),
        "agent episode opened"
    );
    step_authority(
        episode.episode_id,
        first_step_id,
        first_model_attempt_id,
        Vec::new(),
        None,
    )
}

/// Audits an input decision against episode linkage and prepares its model step.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] when pending results differ, input is incomplete, or the
/// step fact cannot commit.
pub fn prepare_episode_step<E: EventStore, C: cairn_record::ContentStore>(
    events: &mut E,
    content: &mut C,
    authority: EpisodeStepAuthority,
    decision: &TurnInputDecision,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<DispatchAuthority, EpisodeCoordinatorError> {
    let EpisodeStepAuthority {
        episode_id: _,
        step,
        model_attempt_id,
        expected_pending_results,
        previous_step: _,
    } = authority;
    if decision.pending_results != expected_pending_results {
        return invalid_episode("step input does not carry the episode's ordered pending results");
    }
    prepare_agent_step(
        events,
        content,
        &step,
        decision,
        model_attempt_id,
        command_id,
        observed_at,
    )
    .map_err(Into::into)
}

/// Prepares an episode step with an exact protocol-native request and durable recovery context.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] when pending results differ, input/native state is
/// incomplete, or the step fact cannot commit.
pub fn prepare_native_episode_step<E: EventStore, C: cairn_record::ContentStore>(
    events: &mut E,
    content: &mut C,
    authority: EpisodeStepAuthority,
    decision: &TurnInputDecision,
    native: &PreparedNativeRequest,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<DispatchAuthority, EpisodeCoordinatorError> {
    let EpisodeStepAuthority {
        episode_id: _,
        step,
        model_attempt_id,
        expected_pending_results,
        previous_step: _,
    } = authority;
    if decision.pending_results != expected_pending_results {
        return invalid_episode("step input does not carry the episode's ordered pending results");
    }
    prepare_native_agent_step(
        events,
        content,
        &step,
        decision,
        native,
        model_attempt_id,
        command_id,
        observed_at,
    )
    .map_err(Into::into)
}

/// Reserves episode tool budget before creating durable step bindings.
///
/// The admission fact is committed to the episode first. If the process stops before the step
/// binding commits, calling this function again with the same assignments completes the binding
/// without charging the budget twice. No [`PreparedToolOperation`] is returned unless both facts
/// agree.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] when assignments differ from durable proposals or prior
/// admission, the episode is terminal, binding was bypassed, or a storage operation fails.
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::too_many_lines,
    reason = "the pre-authority coordinator keeps audit, replay, budget, and binding order explicit"
)]
pub fn admit_episode_operations<E: EventStore, C: cairn_record::ContentStore>(
    events: &mut E,
    content: &mut C,
    episode: &AgentEpisode,
    assignments: Vec<ToolOperationAssignment>,
    admission_command_id: &CommandId,
    binding_command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<EpisodeOperationAdmissionOutcome, EpisodeCoordinatorError> {
    let history = events.read_stream(&episode.stream, None)?;
    let projection = project_episode(&history, episode.episode_id)?;
    validate_previous_steps(events, content, &projection)?;
    let current = projection
        .steps
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode has no step".into()))?;
    let step = AgentStep::new(current.step_id)?;
    let step_state = recover_agent_step(events, content, &step, current.model_attempt_id)?;
    if !matches!(step_state, AgentStepState::NotStarted) {
        validate_step_input(
            events,
            content,
            &step,
            current.model_attempt_id,
            &current.expected_pending_results,
        )?;
    }
    validate_step_admission(&step_state, current)?;
    let steps_started = u32::try_from(projection.steps.len())
        .map_err(|_| EpisodeCoordinatorError::InvalidEpisode("too many episode steps".into()))?;
    if let Some(ref completion) = projection.completion {
        let provider_usage = recover_episode_provider_usage(events, &projection)?;
        validate_completion(
            &projection.opened,
            &projection.steps,
            completion,
            &step_state,
            provider_usage,
        )?;
        return Ok(EpisodeOperationAdmissionOutcome::Completed {
            reason: completion.reason,
            steps_started: completion.steps_started,
        });
    }

    let requested = admission_payload_from_assignments(&assignments);
    if requested.is_empty() {
        return invalid_episode("operation admission must contain at least one assignment");
    }
    let proposals = match &step_state {
        AgentStepState::AwaitingOperations { proposals, .. } => Some(proposals.as_slice()),
        AgentStepState::OperationsBound(_) if current.admitted_operations.is_some() => None,
        _ => return invalid_episode("episode step is not awaiting operation admission"),
    };
    if let Some(proposals) = proposals {
        validate_assignments_against_proposals(proposals, &assignments)?;
    }

    if let Some(durable) = &current.admitted_operations {
        if durable != &requested {
            return invalid_episode("operation assignments differ from durable episode admission");
        }
        if current.admission_command_id != Some(*admission_command_id)
            || current.admission_observed_at_unix_ms != Some(observed_at.get())
        {
            return invalid_episode("replayed operation admission differs from durable command");
        }
    } else {
        validate_new_operation_ids(&projection.steps, &requested)?;
        let admitted = admitted_operation_count(&projection.steps)?;
        let requested_count = u32::try_from(requested.len()).map_err(|_| {
            EpisodeCoordinatorError::InvalidEpisode("too many requested tool operations".into())
        })?;
        if projection
            .opened
            .budget
            .tool_operation_limit
            .is_some_and(|limit| {
                admitted
                    .checked_add(requested_count)
                    .is_none_or(|total| total > limit.get())
            })
        {
            let advance = complete_episode(
                events,
                episode,
                &history,
                current.step_id,
                EpisodeCompletionReason::ToolOperationLimitReached,
                steps_started,
                CompletionEvidence::ToolOperationsRequested(requested_count),
                admission_command_id,
                observed_at,
            )?;
            let EpisodeAdvance::Completed {
                reason,
                steps_started,
            } = advance
            else {
                unreachable!("completion helper always returns a terminal outcome")
            };
            return Ok(EpisodeOperationAdmissionOutcome::Completed {
                reason,
                steps_started,
            });
        }
        append_operation_admission(
            events,
            episode,
            &history,
            current.step_id,
            requested.clone(),
            admission_command_id,
            observed_at,
        )?;
        let refreshed_history = events.read_stream(&episode.stream, None)?;
        let refreshed = project_episode(&refreshed_history, episode.episode_id)?;
        let refreshed_current = refreshed
            .steps
            .last()
            .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode has no step".into()))?;
        if refreshed_current.admitted_operations.as_ref()
            != Some(&admission_payload_from_assignments(&assignments))
        {
            return invalid_episode("committed operation admission could not be recovered");
        }
    }

    let bound = bind_step_operations(
        events,
        content,
        &step,
        current.model_attempt_id,
        assignments,
        binding_command_id,
        observed_at,
    )?;
    validate_bound_admission(&bound, &requested)?;
    Ok(EpisodeOperationAdmissionOutcome::Admitted(
        EpisodeOperationAdmission {
            episode_id: episode.episode_id,
            step_id: current.step_id,
            bound,
        },
    ))
}

/// Rebuilds episode and current-step state exclusively from durable facts and verified content.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] when episode/step history, lineage, or content is invalid.
pub fn recover_agent_episode<E: EventStore, C: cairn_record::ContentStore>(
    events: &E,
    content: &mut C,
    episode: &AgentEpisode,
) -> Result<AgentEpisodeState, EpisodeCoordinatorError> {
    let history = events.read_stream(&episode.stream, None)?;
    if history.is_empty() {
        return Ok(AgentEpisodeState::NotFound);
    }
    let projection = project_episode(&history, episode.episode_id)?;
    validate_previous_steps(events, content, &projection)?;
    let current = projection
        .steps
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode has no step".into()))?;
    let step = AgentStep::new(current.step_id)?;
    let step_state = recover_agent_step(events, content, &step, current.model_attempt_id)?;
    if !matches!(step_state, AgentStepState::NotStarted) {
        validate_step_input(
            events,
            content,
            &step,
            current.model_attempt_id,
            &current.expected_pending_results,
        )?;
    }
    validate_step_admission(&step_state, current)?;
    if let Some(ref completion) = projection.completion {
        let provider_usage = recover_episode_provider_usage(events, &projection)?;
        validate_completion(
            &projection.opened,
            &projection.steps,
            completion,
            &step_state,
            provider_usage,
        )?;
        return Ok(AgentEpisodeState::Completed {
            reason: completion.reason,
            steps_started: completion.steps_started,
        });
    }
    if matches!(step_state, AgentStepState::NotStarted) {
        Ok(AgentEpisodeState::ReadyToPrepare(EpisodeStepAuthority {
            episode_id: episode.episode_id,
            step,
            model_attempt_id: current.model_attempt_id,
            expected_pending_results: current.expected_pending_results.clone(),
            previous_step: projection.steps.iter().rev().nth(1).map(|previous| {
                PreviousEpisodeStepV1 {
                    step_id: previous.step_id,
                    model_attempt_id: previous.model_attempt_id,
                }
            }),
        }))
    } else {
        Ok(AgentEpisodeState::Active {
            step,
            model_attempt_id: current.model_attempt_id,
            step_state,
        })
    }
}

/// Closes a yielded episode or grants the next step after checking durable budgets.
///
/// `next_step_id` and `next_model_attempt_id` are ignored when the current step yields or budget
/// terminates the episode; no event cites them in those cases.
///
/// # Errors
///
/// Returns [`EpisodeCoordinatorError`] unless the current step is yielded or ready for next input.
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::too_many_lines,
    reason = "the coordinator keeps replay, safe-point, and budget decisions visibly ordered"
)]
pub fn advance_agent_episode<E: EventStore, C: cairn_record::ContentStore>(
    events: &mut E,
    content: &mut C,
    episode: &AgentEpisode,
    next_step_id: StepId,
    next_model_attempt_id: ModelAttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<EpisodeAdvance, EpisodeCoordinatorError> {
    let history = events.read_stream(&episode.stream, None)?;
    let projection = project_episode(&history, episode.episode_id)?;
    validate_previous_steps(events, content, &projection)?;
    let current = projection
        .steps
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode has no step".into()))?;
    let step = AgentStep::new(current.step_id)?;
    let step_state = recover_agent_step(events, content, &step, current.model_attempt_id)?;
    if !matches!(step_state, AgentStepState::NotStarted) {
        validate_step_input(
            events,
            content,
            &step,
            current.model_attempt_id,
            &current.expected_pending_results,
        )?;
    }
    validate_step_admission(&step_state, current)?;
    let steps_started = u32::try_from(projection.steps.len())
        .map_err(|_| EpisodeCoordinatorError::InvalidEpisode("too many episode steps".into()))?;
    if let Some(ref completion) = projection.completion {
        let provider_usage = recover_episode_provider_usage(events, &projection)?;
        validate_completion(
            &projection.opened,
            &projection.steps,
            completion,
            &step_state,
            provider_usage,
        )?;
        return Ok(EpisodeAdvance::Completed {
            reason: completion.reason,
            steps_started: completion.steps_started,
        });
    }
    let replayed_advance = history.last().filter(|last| {
        last.command_id == *command_id && last.schema_name.as_str() == STEP_ADVANCED
    });
    if let Some(last) = replayed_advance {
        if last.observed_at_unix_ms != observed_at.get()
            || current.step_id != next_step_id
            || current.model_attempt_id != next_model_attempt_id
        {
            return invalid_episode("replayed step-advance command differs from durable fact");
        }
        if !matches!(step_state, AgentStepState::NotStarted) {
            return invalid_episode("replayed next-step authority was already consumed");
        }
        return step_authority(
            episode.episode_id,
            current.step_id,
            current.model_attempt_id,
            current.expected_pending_results.clone(),
            projection
                .steps
                .iter()
                .rev()
                .nth(1)
                .map(|previous| PreviousEpisodeStepV1 {
                    step_id: previous.step_id,
                    model_attempt_id: previous.model_attempt_id,
                }),
        )
        .map(EpisodeAdvance::NextStep);
    }
    if matches!(step_state, AgentStepState::Yielded { .. }) {
        return complete_episode(
            events,
            episode,
            &history,
            current.step_id,
            EpisodeCompletionReason::Yielded,
            steps_started,
            CompletionEvidence::None,
            command_id,
            observed_at,
        );
    }
    let AgentStepState::ReadyForNextStep {
        pending_results, ..
    } = step_state
    else {
        return invalid_episode("current step is not at a continuable safe boundary");
    };
    if let Some(limit) = projection.opened.budget.provider_token_limit {
        let usage = recover_episode_provider_usage(events, &projection)?;
        if let Some(missing_attempt_id) = usage.missing_attempt_id {
            return complete_episode(
                events,
                episode,
                &history,
                current.step_id,
                EpisodeCompletionReason::ProviderUsageUnavailable,
                steps_started,
                CompletionEvidence::ProviderUsageMissing(missing_attempt_id),
                command_id,
                observed_at,
            );
        }
        if usage.total_tokens >= limit.get() {
            return complete_episode(
                events,
                episode,
                &history,
                current.step_id,
                EpisodeCompletionReason::ProviderTokenLimitReached,
                steps_started,
                CompletionEvidence::ProviderTokensObserved(usage.total_tokens),
                command_id,
                observed_at,
            );
        }
    }
    let reason = if projection
        .opened
        .budget
        .deadline_unix_ms
        .is_some_and(|deadline| deadline.is_reached(observed_at))
    {
        Some(EpisodeCompletionReason::DeadlineReached)
    } else if projection
        .opened
        .budget
        .step_limit
        .is_some_and(|limit| steps_started >= limit.get())
    {
        Some(EpisodeCompletionReason::StepLimitReached)
    } else {
        None
    };
    if let Some(reason) = reason {
        return complete_episode(
            events,
            episode,
            &history,
            current.step_id,
            reason,
            steps_started,
            CompletionEvidence::None,
            command_id,
            observed_at,
        );
    }
    append_advanced_step(
        events,
        episode,
        &history,
        current.step_id,
        next_step_id,
        next_model_attempt_id,
        steps_started + 1,
        pending_results.clone(),
        command_id,
        observed_at,
    )?;
    step_authority(
        episode.episode_id,
        next_step_id,
        next_model_attempt_id,
        pending_results,
        Some(PreviousEpisodeStepV1 {
            step_id: current.step_id,
            model_attempt_id: current.model_attempt_id,
        }),
    )
    .map(EpisodeAdvance::NextStep)
}

#[allow(clippy::too_many_arguments)]
fn append_advanced_step<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    history: &[EventEnvelope],
    previous_step_id: StepId,
    next_step_id: StepId,
    next_model_attempt_id: ModelAttemptId,
    step_ordinal: u32,
    pending_results: Vec<ContentId<OperationResult>>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), EpisodeCoordinatorError> {
    let last = history
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode is empty".into()))?;
    let payload = AdvancedPayload {
        episode_id: episode.episode_id,
        previous_step_id,
        next_step_id,
        next_model_attempt_id,
        step_ordinal,
        pending_results,
    };
    let event = episode_fact(STEP_ADVANCED, Some(last.event_id), observed_at, &payload)?;
    events.append(
        &episode.stream,
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        ),
        command_id,
        &[event],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_operation_admission<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    history: &[EventEnvelope],
    step_id: StepId,
    assignments: Vec<AdmittedAssignmentPayload>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), EpisodeCoordinatorError> {
    let last = history
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode is empty".into()))?;
    let payload = OperationsAdmittedPayload {
        episode_id: episode.episode_id,
        step_id,
        assignments,
    };
    let event = episode_fact(
        OPERATIONS_ADMITTED,
        Some(last.event_id),
        observed_at,
        &payload,
    )?;
    events.append(
        &episode.stream,
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        ),
        command_id,
        &[event],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn complete_episode<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    history: &[EventEnvelope],
    last_step_id: StepId,
    reason: EpisodeCompletionReason,
    steps_started: u32,
    evidence: CompletionEvidence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<EpisodeAdvance, EpisodeCoordinatorError> {
    let last = history
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode is empty".into()))?;
    let (requested_tool_operations, observed_provider_tokens, missing_provider_usage_attempt_id) =
        match evidence {
            CompletionEvidence::None => (None, None, None),
            CompletionEvidence::ToolOperationsRequested(requested) => (Some(requested), None, None),
            CompletionEvidence::ProviderTokensObserved(observed) => (None, Some(observed), None),
            CompletionEvidence::ProviderUsageMissing(attempt_id) => (None, None, Some(attempt_id)),
        };
    let payload = CompletedPayload {
        episode_id: episode.episode_id,
        last_step_id,
        reason,
        steps_started,
        requested_tool_operations,
        observed_provider_tokens,
        missing_provider_usage_attempt_id,
    };
    let event = episode_fact(
        EPISODE_COMPLETED,
        Some(last.event_id),
        observed_at,
        &payload,
    )?;
    events.append(
        &episode.stream,
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        ),
        command_id,
        &[event],
    )?;
    tracing::info!(
        target: "cairn.agent.episode",
        event = "agent_episode_completed",
        episode_id = %episode.episode_id,
        last_step_id = %last_step_id,
        reason = ?reason,
        steps_started,
        requested_tool_operations,
        observed_provider_tokens,
        missing_provider_usage_attempt_id = missing_provider_usage_attempt_id.map(|value| value.to_string()),
        "agent episode completed"
    );
    Ok(EpisodeAdvance::Completed {
        reason,
        steps_started,
    })
}

fn step_authority(
    episode_id: EpisodeId,
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    expected_pending_results: Vec<ContentId<OperationResult>>,
    previous_step: Option<PreviousEpisodeStepV1>,
) -> Result<EpisodeStepAuthority, EpisodeCoordinatorError> {
    Ok(EpisodeStepAuthority {
        episode_id,
        step: AgentStep::new(step_id)?,
        model_attempt_id,
        expected_pending_results,
        previous_step,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the episode projector keeps its small event protocol and invariants in one pass"
)]
fn project_episode(
    history: &[EventEnvelope],
    episode_id: EpisodeId,
) -> Result<EpisodeProjection, EpisodeCoordinatorError> {
    let first = history
        .first()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode is empty".into()))?;
    require_schema(first, EPISODE_OPENED)?;
    if first.parent_event_id.is_some() {
        return invalid_episode("episode-opened fact must not have a parent");
    }
    let opened: OpenedPayload = decode(first)?;
    if opened.episode_id != episode_id {
        return invalid_episode("opened fact cites another episode");
    }
    crate::metering::validate_external_meter_limits(opened.budget.external_meter_limits.as_deref())
        .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?;
    let mut steps = vec![StepEntry {
        step_id: opened.first_step_id,
        model_attempt_id: opened.first_model_attempt_id,
        expected_pending_results: Vec::new(),
        admitted_operations: None,
        admission_command_id: None,
        admission_observed_at_unix_ms: None,
    }];
    let mut step_ids = HashSet::from([opened.first_step_id.to_string()]);
    let mut attempt_ids = HashSet::from([opened.first_model_attempt_id.to_string()]);
    let mut admitted_operation_ids = HashSet::new();
    let mut completion = None;
    let mut parent = first.event_id;
    for event in &history[1..] {
        if event.parent_event_id != Some(parent) {
            return invalid_episode("episode fact does not cite the previous fact");
        }
        match event.schema_name.as_str() {
            OPERATIONS_ADMITTED => {
                if completion.is_some() {
                    return invalid_episode("episode admits operations after completion");
                }
                let payload: OperationsAdmittedPayload = decode(event)?;
                let current = steps.last_mut().expect("opened episode has a step");
                if payload.episode_id != episode_id || payload.step_id != current.step_id {
                    return invalid_episode("operation admission cites another episode step");
                }
                if current.admitted_operations.is_some() {
                    return invalid_episode("episode step has multiple operation admissions");
                }
                if payload.assignments.is_empty() {
                    return invalid_episode("operation admission must not be empty");
                }
                for assignment in &payload.assignments {
                    if !admitted_operation_ids.insert(assignment.operation_id.to_string()) {
                        return invalid_episode("episode reuses an admitted operation identity");
                    }
                }
                current.admitted_operations = Some(payload.assignments);
                current.admission_command_id = Some(event.command_id);
                current.admission_observed_at_unix_ms = Some(event.observed_at_unix_ms);
            }
            STEP_ADVANCED => {
                if completion.is_some() {
                    return invalid_episode("episode advances after completion");
                }
                let payload: AdvancedPayload = decode(event)?;
                let previous = steps.last().expect("opened episode has a step");
                let expected_ordinal = u32::try_from(steps.len() + 1).map_err(|_| {
                    EpisodeCoordinatorError::InvalidEpisode("too many episode steps".into())
                })?;
                if payload.episode_id != episode_id
                    || payload.previous_step_id != previous.step_id
                    || payload.step_ordinal != expected_ordinal
                {
                    return invalid_episode("advanced fact breaks episode step lineage");
                }
                if !step_ids.insert(payload.next_step_id.to_string())
                    || !attempt_ids.insert(payload.next_model_attempt_id.to_string())
                {
                    return invalid_episode("episode reuses a step or model-attempt identity");
                }
                steps.push(StepEntry {
                    step_id: payload.next_step_id,
                    model_attempt_id: payload.next_model_attempt_id,
                    expected_pending_results: payload.pending_results,
                    admitted_operations: None,
                    admission_command_id: None,
                    admission_observed_at_unix_ms: None,
                });
            }
            EPISODE_COMPLETED => {
                if completion.is_some() {
                    return invalid_episode("episode has multiple completion facts");
                }
                let payload: CompletedPayload = decode(event)?;
                let current = steps.last().expect("opened episode has a step");
                let expected_steps = u32::try_from(steps.len()).map_err(|_| {
                    EpisodeCoordinatorError::InvalidEpisode("too many episode steps".into())
                })?;
                if payload.episode_id != episode_id
                    || payload.last_step_id != current.step_id
                    || payload.steps_started != expected_steps
                {
                    return invalid_episode("completion fact breaks episode step lineage");
                }
                let admitted = admitted_operation_count(&steps)?;
                let has_non_tool_evidence = payload.observed_provider_tokens.is_some()
                    || payload.missing_provider_usage_attempt_id.is_some();
                match payload.reason {
                    EpisodeCompletionReason::StepLimitReached
                        if payload.requested_tool_operations.is_some()
                            || has_non_tool_evidence
                            || opened
                                .budget
                                .step_limit
                                .is_none_or(|limit| expected_steps < limit.get()) =>
                    {
                        return invalid_episode("episode completed before reaching its step limit");
                    }
                    EpisodeCompletionReason::DeadlineReached
                        if payload.requested_tool_operations.is_some()
                            || has_non_tool_evidence
                            || opened.budget.deadline_unix_ms.is_none_or(|deadline| {
                                !deadline.is_reached(ObservedAtUnixMillis::new(
                                    event.observed_at_unix_ms,
                                ))
                            }) =>
                    {
                        return invalid_episode("episode completed before its deadline");
                    }
                    EpisodeCompletionReason::Yielded
                        if payload.requested_tool_operations.is_some() || has_non_tool_evidence =>
                    {
                        return invalid_episode(
                            "yield completion carries unexpected budget evidence",
                        );
                    }
                    EpisodeCompletionReason::ToolOperationLimitReached => {
                        let requested = payload.requested_tool_operations.ok_or_else(|| {
                            EpisodeCoordinatorError::InvalidEpisode(
                                "tool-budget completion lacks requested operation count".into(),
                            )
                        })?;
                        if has_non_tool_evidence
                            || requested == 0
                            || current.admitted_operations.is_some()
                            || opened.budget.tool_operation_limit.is_none_or(|limit| {
                                admitted
                                    .checked_add(requested)
                                    .is_some_and(|total| total <= limit.get())
                            })
                        {
                            return invalid_episode(
                                "tool-budget completion does not prove a budget overrun",
                            );
                        }
                    }
                    EpisodeCompletionReason::ProviderTokenLimitReached => {
                        let observed = payload.observed_provider_tokens.ok_or_else(|| {
                            EpisodeCoordinatorError::InvalidEpisode(
                                "provider-token completion lacks observed usage".into(),
                            )
                        })?;
                        if payload.requested_tool_operations.is_some()
                            || payload.missing_provider_usage_attempt_id.is_some()
                            || opened
                                .budget
                                .provider_token_limit
                                .is_none_or(|limit| observed < limit.get())
                        {
                            return invalid_episode(
                                "provider-token completion does not prove budget exhaustion",
                            );
                        }
                    }
                    EpisodeCompletionReason::ProviderUsageUnavailable => {
                        let missing =
                            payload.missing_provider_usage_attempt_id.ok_or_else(|| {
                                EpisodeCoordinatorError::InvalidEpisode(
                                    "missing-usage completion lacks model-attempt identity".into(),
                                )
                            })?;
                        if payload.requested_tool_operations.is_some()
                            || payload.observed_provider_tokens.is_some()
                            || opened.budget.provider_token_limit.is_none()
                            || !steps.iter().any(|step| step.model_attempt_id == missing)
                        {
                            return invalid_episode(
                                "missing-usage completion does not cite a budgeted episode attempt",
                            );
                        }
                    }
                    _ => {}
                }
                completion = Some(payload);
            }
            _ => return invalid_episode("unsupported agent-episode event schema"),
        }
        parent = event.event_id;
    }
    Ok(EpisodeProjection {
        opened,
        steps,
        completion,
    })
}

fn admission_payload_from_assignments(
    assignments: &[ToolOperationAssignment],
) -> Vec<AdmittedAssignmentPayload> {
    assignments
        .iter()
        .map(|assignment| AdmittedAssignmentPayload {
            operation_id: assignment.operation_id(),
            tool: assignment.registration().name().clone(),
            implementation_version: assignment.registration().implementation_version().clone(),
            effect: assignment.registration().effect(),
        })
        .collect()
}

fn validate_assignments_against_proposals(
    proposals: &[ToolCallProposal],
    assignments: &[ToolOperationAssignment],
) -> Result<(), EpisodeCoordinatorError> {
    if proposals.len() != assignments.len() {
        return invalid_episode("every proposal must have exactly one admitted operation");
    }
    let mut operation_ids = HashSet::new();
    for (proposal, assignment) in proposals.iter().zip(assignments) {
        if proposal.tool() != assignment.registration().name() {
            return invalid_episode("admitted registration differs from the proposed tool");
        }
        if !operation_ids.insert(assignment.operation_id().to_string()) {
            return invalid_episode("operation admission contains a duplicate identity");
        }
    }
    Ok(())
}

fn validate_new_operation_ids(
    steps: &[StepEntry],
    requested: &[AdmittedAssignmentPayload],
) -> Result<(), EpisodeCoordinatorError> {
    let prior: HashSet<String> = steps
        .iter()
        .filter_map(|step| step.admitted_operations.as_ref())
        .flatten()
        .map(|assignment| assignment.operation_id.to_string())
        .collect();
    if requested
        .iter()
        .any(|assignment| prior.contains(&assignment.operation_id.to_string()))
    {
        return invalid_episode("episode cannot reuse a prior logical operation identity");
    }
    Ok(())
}

fn admitted_operation_count(steps: &[StepEntry]) -> Result<u32, EpisodeCoordinatorError> {
    steps.iter().try_fold(0_u32, |total, step| {
        let count = step.admitted_operations.as_ref().map_or(0, Vec::len);
        let count = u32::try_from(count).map_err(|_| {
            EpisodeCoordinatorError::InvalidEpisode("too many admitted operations".into())
        })?;
        total.checked_add(count).ok_or_else(|| {
            EpisodeCoordinatorError::InvalidEpisode("admitted operation count overflow".into())
        })
    })
}

fn recover_episode_provider_usage<E: EventStore>(
    events: &E,
    projection: &EpisodeProjection,
) -> Result<EpisodeProviderUsage, EpisodeCoordinatorError> {
    let mut total_tokens = 0_u64;
    let mut missing_attempt_id = None;
    for step in &projection.steps {
        let aggregate = AgentStep::new(step.step_id)?;
        let history = events.read_stream(aggregate.stream_id(), None)?;
        if let ModelAttemptState::Completed { usage, .. } =
            recover_model_attempt(&history, step.model_attempt_id)
                .map_err(StepCoordinatorError::from)?
        {
            match usage {
                Some(usage) => {
                    total_tokens =
                        total_tokens
                            .checked_add(usage.total_tokens())
                            .ok_or_else(|| {
                                EpisodeCoordinatorError::InvalidEpisode(
                                    "episode provider-token total overflow".into(),
                                )
                            })?;
                }
                None if missing_attempt_id.is_none() => {
                    missing_attempt_id = Some(step.model_attempt_id);
                }
                None => {}
            }
        }
    }
    Ok(EpisodeProviderUsage {
        total_tokens,
        missing_attempt_id,
    })
}

fn validate_bound_admission(
    bound: &BoundStepOperations,
    admitted: &[AdmittedAssignmentPayload],
) -> Result<(), EpisodeCoordinatorError> {
    if bound.operations().len() != admitted.len()
        || bound
            .operations()
            .iter()
            .zip(admitted)
            .any(|(operation, assignment)| {
                operation.operation_id() != assignment.operation_id
                    || operation.tool() != &assignment.tool
                    || operation.implementation_version() != &assignment.implementation_version
                    || operation.effect() != assignment.effect
            })
    {
        return invalid_episode("step bindings differ from durable episode admission");
    }
    Ok(())
}

fn validate_step_admission(
    step_state: &AgentStepState,
    entry: &StepEntry,
) -> Result<(), EpisodeCoordinatorError> {
    match step_state {
        AgentStepState::AwaitingOperations { proposals, .. } => {
            if let Some(admitted) = &entry.admitted_operations {
                if proposals.len() != admitted.len()
                    || proposals
                        .iter()
                        .zip(admitted)
                        .any(|(proposal, assignment)| proposal.tool() != &assignment.tool)
                {
                    return invalid_episode(
                        "durable episode admission differs from step proposals",
                    );
                }
            }
            Ok(())
        }
        AgentStepState::OperationsBound(bound) => entry
            .admitted_operations
            .as_ref()
            .ok_or_else(|| {
                EpisodeCoordinatorError::InvalidEpisode(
                    "step operations were bound without episode admission".into(),
                )
            })
            .and_then(|admitted| validate_bound_admission(bound, admitted)),
        AgentStepState::ReadyForNextStep { operations, .. } => {
            let admitted = entry.admitted_operations.as_ref().ok_or_else(|| {
                EpisodeCoordinatorError::InvalidEpisode(
                    "settled operations lack durable episode admission".into(),
                )
            })?;
            if operations.len() != admitted.len()
                || operations
                    .iter()
                    .zip(admitted)
                    .any(|(operation, assignment)| {
                        operation.operation_id() != assignment.operation_id
                            || operation.tool() != &assignment.tool
                            || operation.implementation_version()
                                != &assignment.implementation_version
                            || operation.effect() != assignment.effect
                    })
            {
                return invalid_episode("settled operations differ from episode admission");
            }
            Ok(())
        }
        _ if entry.admitted_operations.is_some() => {
            invalid_episode("episode admitted operations before durable tool proposals")
        }
        _ => Ok(()),
    }
}

fn validate_previous_steps<E: EventStore, C: cairn_record::ContentStore>(
    events: &E,
    content: &mut C,
    projection: &EpisodeProjection,
) -> Result<(), EpisodeCoordinatorError> {
    for pair in projection.steps.windows(2) {
        let previous = &pair[0];
        let next = &pair[1];
        let step = AgentStep::new(previous.step_id)?;
        validate_step_input(
            events,
            content,
            &step,
            previous.model_attempt_id,
            &previous.expected_pending_results,
        )?;
        let step_state = recover_agent_step(events, content, &step, previous.model_attempt_id)?;
        validate_step_admission(&step_state, previous)?;
        match step_state {
            AgentStepState::ReadyForNextStep {
                pending_results, ..
            } if pending_results == next.expected_pending_results => {}
            _ => return invalid_episode("advanced episode step lacks its durable prior results"),
        }
    }
    Ok(())
}

fn validate_step_input<E: EventStore, C: cairn_record::ContentStore>(
    events: &E,
    content: &mut C,
    step: &AgentStep,
    attempt_id: ModelAttemptId,
    expected_pending_results: &[ContentId<OperationResult>],
) -> Result<(), EpisodeCoordinatorError> {
    let history = events.read_stream(step.stream_id(), None)?;
    let decision = recover_turn_input_decision(&history, content, attempt_id)
        .map_err(StepCoordinatorError::from)?
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("step input is missing".into()))?;
    if decision.pending_results == expected_pending_results {
        Ok(())
    } else {
        invalid_episode("step input differs from episode result lineage")
    }
}

fn validate_completion(
    opened: &OpenedPayload,
    steps: &[StepEntry],
    completion: &CompletedPayload,
    step_state: &AgentStepState,
    provider_usage: EpisodeProviderUsage,
) -> Result<(), EpisodeCoordinatorError> {
    match completion.reason {
        EpisodeCompletionReason::Yielded
            if matches!(step_state, AgentStepState::Yielded { .. }) =>
        {
            Ok(())
        }
        EpisodeCompletionReason::StepLimitReached
            if opened
                .budget
                .step_limit
                .is_some_and(|limit| completion.steps_started >= limit.get())
                && matches!(step_state, AgentStepState::ReadyForNextStep { .. }) =>
        {
            Ok(())
        }
        EpisodeCompletionReason::DeadlineReached
            if opened.budget.deadline_unix_ms.is_some()
                && matches!(step_state, AgentStepState::ReadyForNextStep { .. }) =>
        {
            Ok(())
        }
        EpisodeCompletionReason::ToolOperationLimitReached => {
            let requested = completion.requested_tool_operations.ok_or_else(|| {
                EpisodeCoordinatorError::InvalidEpisode(
                    "tool-budget completion lacks requested operation count".into(),
                )
            })?;
            let AgentStepState::AwaitingOperations { proposals, .. } = step_state else {
                return invalid_episode(
                    "tool-budget completion is not at the proposal admission boundary",
                );
            };
            let proposal_count = u32::try_from(proposals.len()).map_err(|_| {
                EpisodeCoordinatorError::InvalidEpisode("too many proposed operations".into())
            })?;
            let admitted = admitted_operation_count(steps)?;
            if requested == proposal_count
                && opened.budget.tool_operation_limit.is_some_and(|limit| {
                    admitted
                        .checked_add(requested)
                        .is_none_or(|total| total > limit.get())
                })
            {
                Ok(())
            } else {
                invalid_episode("tool-budget completion carries false overrun evidence")
            }
        }
        EpisodeCompletionReason::ProviderTokenLimitReached => {
            let observed = completion.observed_provider_tokens.ok_or_else(|| {
                EpisodeCoordinatorError::InvalidEpisode(
                    "provider-token completion lacks observed usage".into(),
                )
            })?;
            if matches!(step_state, AgentStepState::ReadyForNextStep { .. })
                && provider_usage.missing_attempt_id.is_none()
                && observed == provider_usage.total_tokens
                && opened
                    .budget
                    .provider_token_limit
                    .is_some_and(|limit| observed >= limit.get())
            {
                Ok(())
            } else {
                invalid_episode("provider-token completion contradicts durable usage receipts")
            }
        }
        EpisodeCompletionReason::ProviderUsageUnavailable => {
            let missing = completion
                .missing_provider_usage_attempt_id
                .ok_or_else(|| {
                    EpisodeCoordinatorError::InvalidEpisode(
                        "missing-usage completion lacks model-attempt identity".into(),
                    )
                })?;
            if matches!(step_state, AgentStepState::ReadyForNextStep { .. })
                && opened.budget.provider_token_limit.is_some()
                && provider_usage.missing_attempt_id == Some(missing)
            {
                Ok(())
            } else {
                invalid_episode("missing-usage completion contradicts durable provider receipt")
            }
        }
        _ => invalid_episode("episode completion reason contradicts its final step"),
    }
}

fn require_schema(event: &EventEnvelope, schema: &str) -> Result<(), EpisodeCoordinatorError> {
    if event.schema_name.as_str() == schema {
        Ok(())
    } else {
        invalid_episode("episode stream does not start with episode-opened")
    }
}

fn decode<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, EpisodeCoordinatorError> {
    if event.schema_version.get() != 1 {
        return invalid_episode("unsupported agent-episode schema version");
    }
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))
}

fn episode_fact<P: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, EpisodeCoordinatorError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| EpisodeCoordinatorError::InvalidEpisode(error.to_string()))?,
    })
}

fn invalid_episode<T>(message: &str) -> Result<T, EpisodeCoordinatorError> {
    Err(EpisodeCoordinatorError::InvalidEpisode(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{
        AggregateId, AggregateKind, AttemptId, CommandId, ContentId, ContentType, EpisodeId,
        ModelAttemptId, OperationId, StepId, TaskId,
    };
    use cairn_record::{ContentStore, EventStore, StreamId};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        AdvancedPayload, AgentEpisode, AgentEpisodeState, EpisodeAdvance, EpisodeBudget,
        EpisodeCompletionReason, EpisodeDeadlineUnixMillis, EpisodeOperationAdmissionOutcome,
        EpisodeProviderTokenLimit, EpisodeStepLimit, EpisodeToolOperationLimit,
        OperationsAdmittedPayload, admission_payload_from_assignments, admit_episode_operations,
        advance_agent_episode, append_operation_admission, open_agent_episode,
        prepare_episode_step, prepare_native_episode_step, project_episode, recover_agent_episode,
        validate_previous_steps,
    };
    use crate::{
        AdapterModelTurn, AdapterOutputItem, AdapterVersion, AgentRoleName, AgentStep,
        AgentStepState, CanonicalToolResult, ContextBlock, DeploymentName, DispatchAuthority,
        DispatchCompletion, HistoryItem, InstructionBlock, ModelName, ModelOutputTokenLimit,
        ModelProtocolConfig, ModelSelection, ModelTransportResponse, NativeProtocolCodec,
        NativeRequestSpec, NativeToolDefinition, OperationResult, PolicyDocument,
        PreparedModelRequest, ProviderName, ProviderTokenCount, ProviderTokenUsage,
        ProviderToolCallId, RecordedAdapterExchange, RecordedModelAdapter, RecordedToolExchange,
        RecordedToolGateway, ResponsesReasoningReplay, ScriptedModelTransport, SettledAgentStep,
        StepOperationSettlement, ToolCatalog, ToolEffectClass, ToolImplementationVersion, ToolName,
        ToolOperationAssignment, ToolRegistration, TransportError, TurnInputDecision,
        authorize_tool_operation, begin_model_dispatch, begin_tool_operation, bind_step_operations,
        decode_model_response, execute_model_dispatch, execute_tool_operation, prepare_agent_step,
        recover_agent_step, settle_decoded_step, settle_step_operations,
    };

    struct ReadyEpisode {
        _directory: tempfile::TempDir,
        content: SqliteContentStore,
        events: SqliteEventStore,
        episode: AgentEpisode,
    }

    fn put_json<T: ContentType>(
        content: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(value).expect("fixture bytes");
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("fixture content")
            .content_id
    }

    fn decision(
        content: &mut SqliteContentStore,
        pending_results: Vec<ContentId<OperationResult>>,
    ) -> TurnInputDecision {
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
                &serde_json::json!({"role":"user","text":"inspect"}),
            )],
            context: vec![put_json::<ContextBlock>(
                content,
                &serde_json::json!({"scope":"workspace"}),
            )],
            pending_results,
            policy: put_json::<PolicyDocument>(content, &serde_json::json!({"approval":"ask"})),
        }
    }

    fn settle_model_turn_with_usage(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
        items: Vec<AdapterOutputItem>,
        usage: Option<ProviderTokenUsage>,
    ) -> SettledAgentStep {
        let started = begin_model_dispatch(
            events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("begin model");
        let mut transport = ScriptedModelTransport::new(move |_: &PreparedModelRequest| {
            Ok::<_, TransportError>(ModelTransportResponse::new(
                b"raw-response".to_vec(),
                usage.clone(),
            ))
        });
        let DispatchCompletion::Response(received) = execute_model_dispatch(
            events,
            content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("model response") else {
            panic!("response");
        };
        let response_id = received.response_id();
        let mut adapter = RecordedModelAdapter::new(
            AdapterVersion::new("v1").expect("adapter"),
            [RecordedAdapterExchange {
                response_id,
                turn: AdapterModelTurn { items },
            }],
        );
        let decoded = decode_model_response(
            events,
            content,
            &mut adapter,
            received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(5),
        )
        .expect("decode");
        settle_decoded_step(
            events,
            content,
            step,
            attempt_id,
            decoded,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("settle turn")
    }

    fn settle_model_turn(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
        items: Vec<AdapterOutputItem>,
    ) -> SettledAgentStep {
        settle_model_turn_with_usage(events, content, step, attempt_id, authority, items, None)
    }

    fn complete_tool_step_with_usage(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        episode: &AgentEpisode,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
        usage: Option<ProviderTokenUsage>,
    ) -> Vec<ContentId<OperationResult>> {
        let settled = settle_model_turn_with_usage(
            events,
            content,
            step,
            attempt_id,
            authority,
            vec![AdapterOutputItem::ToolCall {
                provider_call_id: ProviderToolCallId::new("call-1").expect("call"),
                tool: ToolName::new("read_source").expect("tool"),
                arguments: serde_json::json!({"path":"src/lib.rs"}),
            }],
            usage,
        );
        assert!(matches!(
            settled,
            SettledAgentStep::AwaitingOperations { .. }
        ));
        let EpisodeOperationAdmissionOutcome::Admitted(admission) = admit_episode_operations(
            events,
            content,
            episode,
            vec![ToolOperationAssignment::new(
                OperationId::new(),
                ToolRegistration::new(
                    ToolName::new("read_source").expect("tool"),
                    ToolImplementationVersion::new("v1").expect("version"),
                    ToolEffectClass::ReadOnly,
                ),
            )],
            &CommandId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("admit") else {
            panic!("admitted operations");
        };
        let operation = admission.into_operations().pop().expect("operation");
        let arguments_id = operation.arguments_id();
        let operation_authority = authorize_tool_operation(
            events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(8),
            operation,
        )
        .expect("authorize operation");
        let started = begin_tool_operation(
            events,
            operation_authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(9),
        )
        .expect("begin operation");
        let mut gateway = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id,
            result: CanonicalToolResult::from_value(&serde_json::json!({"value":1}))
                .expect("result"),
        }]);
        execute_tool_operation(
            events,
            content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(10),
        )
        .expect("execute operation");
        let StepOperationSettlement::ReadyForNextStep {
            pending_results, ..
        } = settle_step_operations(
            events,
            content,
            step,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(11),
        )
        .expect("settle operations")
        else {
            panic!("ready next step");
        };
        pending_results
    }

    fn complete_tool_step(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        episode: &AgentEpisode,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
    ) -> Vec<ContentId<OperationResult>> {
        complete_tool_step_with_usage(events, content, episode, step, attempt_id, authority, None)
    }

    fn prepared_episode_with_budget(
        budget: EpisodeBudget,
    ) -> (ReadyEpisode, AgentStep, ModelAttemptId, DispatchAuthority) {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = AgentEpisode::new(EpisodeId::new()).expect("episode");
        let step_id = StepId::new();
        let attempt_id = ModelAttemptId::new();
        let task_id = TaskId::new();
        let command = CommandId::new();
        let role = AgentRoleName::new("candidate-author").expect("role");
        let _lost = open_agent_episode(
            &mut events,
            &episode,
            task_id,
            role.clone(),
            budget.clone(),
            step_id,
            attempt_id,
            &command,
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("open");
        let authority = open_agent_episode(
            &mut events,
            &episode,
            task_id,
            role.clone(),
            budget.clone(),
            step_id,
            attempt_id,
            &command,
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("replay open");
        let input = decision(&mut content, Vec::new());
        let dispatch = prepare_episode_step(
            &mut events,
            &mut content,
            authority,
            &input,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("prepare first step");
        assert!(
            open_agent_episode(
                &mut events,
                &episode,
                task_id,
                role,
                budget,
                step_id,
                attempt_id,
                &command,
                cairn_protocol::ObservedAtUnixMillis::new(1),
            )
            .is_err(),
            "consumed first-step authority must not replay"
        );
        let step = AgentStep::new(step_id).expect("step");
        (
            ReadyEpisode {
                _directory: directory,
                content,
                events,
                episode,
            },
            step,
            attempt_id,
            dispatch,
        )
    }

    fn prepared_episode_with_token_limit(
        step_limit: u32,
        tool_operation_limit: u32,
        provider_token_limit: Option<u64>,
        deadline: Option<i64>,
    ) -> (ReadyEpisode, AgentStep, ModelAttemptId, DispatchAuthority) {
        prepared_episode_with_budget(EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(step_limit).expect("limit")),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(tool_operation_limit)),
            provider_token_limit: provider_token_limit
                .map(|limit| EpisodeProviderTokenLimit::new(limit).expect("provider token limit")),
            deadline_unix_ms: deadline.map(EpisodeDeadlineUnixMillis::new),
            external_meter_limits: None,
        })
    }

    fn prepared_episode(
        step_limit: u32,
        tool_operation_limit: u32,
        deadline: Option<i64>,
    ) -> (ReadyEpisode, AgentStep, ModelAttemptId, DispatchAuthority) {
        prepared_episode_with_token_limit(step_limit, tool_operation_limit, None, deadline)
    }

    fn ready_episode(
        step_limit: u32,
        tool_operation_limit: u32,
        deadline: Option<i64>,
    ) -> (ReadyEpisode, Vec<ContentId<OperationResult>>) {
        let (mut fixture, step, attempt_id, dispatch) =
            prepared_episode(step_limit, tool_operation_limit, deadline);
        let results = complete_tool_step(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            &step,
            attempt_id,
            dispatch,
        );
        (fixture, results)
    }

    fn read_source_assignment(operation_id: OperationId) -> ToolOperationAssignment {
        ToolOperationAssignment::new(
            operation_id,
            ToolRegistration::new(
                ToolName::new("read_source").expect("tool"),
                ToolImplementationVersion::new("v1").expect("version"),
                ToolEffectClass::ReadOnly,
            ),
        )
    }

    fn tool_call(call_id: &str) -> AdapterOutputItem {
        AdapterOutputItem::ToolCall {
            provider_call_id: ProviderToolCallId::new(call_id).expect("call"),
            tool: ToolName::new("read_source").expect("tool"),
            arguments: serde_json::json!({"path":"src/lib.rs"}),
        }
    }

    fn operation_stream(operation_id: OperationId) -> StreamId {
        StreamId {
            kind: AggregateKind::new("tool-operation").expect("kind"),
            id: AggregateId::new(operation_id.to_string()).expect("id"),
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the end-to-end test keeps two-step authority and result lineage together"
    )]
    fn episode_runs_multiple_steps_and_recovers_advance_authority() {
        let (mut fixture, expected_results) = ready_episode(3, 3, None);
        let next_step_id = StepId::new();
        let next_attempt_id = ModelAttemptId::new();
        let command = CommandId::new();
        let _lost = advance_agent_episode(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            next_step_id,
            next_attempt_id,
            &command,
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("advance");
        let EpisodeAdvance::NextStep(replayed) = advance_agent_episode(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            next_step_id,
            next_attempt_id,
            &command,
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("replay advance") else {
            panic!("next step");
        };
        assert_eq!(replayed.expected_pending_results(), expected_results);
        let wrong_input = decision(&mut fixture.content, Vec::new());
        assert!(
            prepare_episode_step(
                &mut fixture.events,
                &mut fixture.content,
                replayed,
                &wrong_input,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(13),
            )
            .is_err()
        );
        let AgentEpisodeState::ReadyToPrepare(recovered) =
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode)
                .expect("recover next authority")
        else {
            panic!("ready next step");
        };
        let next_input = decision(&mut fixture.content, expected_results);
        let dispatch = prepare_episode_step(
            &mut fixture.events,
            &mut fixture.content,
            recovered,
            &next_input,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(13),
        )
        .expect("prepare second step");
        assert!(
            advance_agent_episode(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &command,
                cairn_protocol::ObservedAtUnixMillis::new(12),
            )
            .is_err(),
            "consumed next-step authority must not replay"
        );
        let next_step = AgentStep::new(next_step_id).expect("step");
        assert!(matches!(
            settle_model_turn(
                &mut fixture.events,
                &mut fixture.content,
                &next_step,
                next_attempt_id,
                dispatch,
                vec![AdapterOutputItem::Text {
                    text: "done".to_owned(),
                }],
            ),
            SettledAgentStep::Yielded { .. }
        ));
        assert!(matches!(
            advance_agent_episode(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(14),
            )
            .expect("finish"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::Yielded,
                steps_started: 2,
            }
        ));
        assert!(matches!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode,)
                .expect("recover complete"),
            AgentEpisodeState::Completed {
                reason: EpisodeCompletionReason::Yielded,
                steps_started: 2,
            }
        ));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the protocol-native end-to-end test intentionally keeps the complete two-step lineage visible"
    )]
    fn protocol_native_episode_closes_tool_loop_and_yields_on_second_step() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = AgentEpisode::new(EpisodeId::new()).expect("episode");
        let first_step_id = StepId::new();
        let first_attempt_id = ModelAttemptId::new();
        let first_authority = open_agent_episode(
            &mut events,
            &episode,
            TaskId::new(),
            AgentRoleName::new("candidate-author").expect("role"),
            EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(3).expect("limit")),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(3)),
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            },
            first_step_id,
            first_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("open");
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let native_spec = NativeRequestSpec {
            wire_model: ModelName::new("fixture").expect("model"),
            instructions: "Use the registered tool, then answer.".to_owned(),
            tools: vec![NativeToolDefinition {
                name: ToolName::new("read_source").expect("tool"),
                description: "Read one source path".to_owned(),
                input_schema: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"]
                }),
                strict: true,
            }],
            max_output_tokens: ModelOutputTokenLimit::new(1024).expect("tokens"),
        };
        let first_native = codec
            .prepare_initial(&native_spec, "inspect src/lib.rs")
            .expect("first native request");
        let first_decision = decision(&mut content, Vec::new());
        let first_dispatch = prepare_native_episode_step(
            &mut events,
            &mut content,
            first_authority,
            &first_decision,
            &first_native,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("prepare first");
        let first_started = begin_model_dispatch(
            &mut events,
            first_dispatch,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("begin first");
        let first_response = br#"{
            "output":[
                {"type":"reasoning","id":"rs-1","encrypted_content":"opaque-state"},
                {"type":"function_call","call_id":"call-1","name":"read_source","arguments":"{\"path\":\"src/lib.rs\"}"}
            ]
        }"#;
        let mut first_transport = ScriptedModelTransport::new(move |_: &PreparedModelRequest| {
            Ok::<_, TransportError>(ModelTransportResponse::without_usage(
                first_response.to_vec(),
            ))
        });
        execute_model_dispatch(
            &mut events,
            &mut content,
            &mut first_transport,
            first_started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("first response");
        let first_step = AgentStep::new(first_step_id).expect("first step");
        let AgentStepState::ReadyToDecode(first_received) =
            recover_agent_step(&events, &mut content, &first_step, first_attempt_id)
                .expect("recover first response")
        else {
            panic!("first decode authority");
        };
        let first_decoded = codec
            .decode_recovered_received(
                &mut events,
                &mut content,
                first_received,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(5),
            )
            .expect("decode first");
        let first_continuation = first_decoded.continuation().clone();
        let settled = settle_decoded_step(
            &mut events,
            &content,
            &first_step,
            first_attempt_id,
            first_decoded.into_semantic(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("settle first");
        assert!(matches!(
            settled,
            SettledAgentStep::AwaitingOperations { .. }
        ));

        let EpisodeOperationAdmissionOutcome::Admitted(admission) = admit_episode_operations(
            &mut events,
            &mut content,
            &episode,
            vec![read_source_assignment(OperationId::new())],
            &CommandId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("admit") else {
            panic!("operation admission");
        };
        let operation = admission.into_operations().pop().expect("operation");
        let arguments_id = operation.arguments_id();
        let operation_authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(8),
            operation,
        )
        .expect("authorize operation");
        let operation_started = begin_tool_operation(
            &mut events,
            operation_authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(9),
        )
        .expect("begin operation");
        let mut gateway = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id,
            result: CanonicalToolResult::from_value(&serde_json::json!({"source":"ok"}))
                .expect("result"),
        }]);
        execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            operation_started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(10),
        )
        .expect("execute operation");
        let StepOperationSettlement::ReadyForNextStep {
            pending_results, ..
        } = settle_step_operations(
            &mut events,
            &mut content,
            &first_step,
            first_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(11),
        )
        .expect("settle operation")
        else {
            panic!("results ready");
        };
        let settled_native = codec
            .append_archived_tool_results(&content, &first_continuation, &pending_results)
            .expect("append archived results");
        let second_native = codec
            .prepare_continuation(&native_spec, &settled_native)
            .expect("second native request");
        let expected_second_bytes = second_native.request_bytes().to_vec();

        let second_step_id = StepId::new();
        let second_attempt_id = ModelAttemptId::new();
        let EpisodeAdvance::NextStep(second_authority) = advance_agent_episode(
            &mut events,
            &mut content,
            &episode,
            second_step_id,
            second_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("advance") else {
            panic!("second authority");
        };
        let second_decision = decision(&mut content, pending_results);
        let second_dispatch = prepare_native_episode_step(
            &mut events,
            &mut content,
            second_authority,
            &second_decision,
            &second_native,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(13),
        )
        .expect("prepare second");
        drop(second_native);
        let second_started = begin_model_dispatch(
            &mut events,
            second_dispatch,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(14),
        )
        .expect("begin second");
        let second_response = br#"{
            "output":[
                {"type":"reasoning","id":"rs-2","encrypted_content":"opaque-final"},
                {"type":"message","id":"msg-2","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"done"}]}
            ]
        }"#;
        let mut second_transport =
            ScriptedModelTransport::new(move |request: &PreparedModelRequest| {
                assert_eq!(request.request_bytes(), expected_second_bytes);
                let body: serde_json::Value =
                    serde_json::from_slice(request.request_bytes()).expect("request JSON");
                assert_eq!(body["input"][1]["encrypted_content"], "opaque-state");
                assert_eq!(body["input"][3]["type"], "function_call_output");
                assert_eq!(body["input"][3]["output"], "{\"source\":\"ok\"}");
                Ok::<_, TransportError>(ModelTransportResponse::without_usage(
                    second_response.to_vec(),
                ))
            });
        execute_model_dispatch(
            &mut events,
            &mut content,
            &mut second_transport,
            second_started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(15),
        )
        .expect("second response");
        let second_step = AgentStep::new(second_step_id).expect("second step");
        let AgentStepState::ReadyToDecode(second_received) =
            recover_agent_step(&events, &mut content, &second_step, second_attempt_id)
                .expect("recover second response")
        else {
            panic!("second decode authority");
        };
        let second_decoded = codec
            .decode_recovered_received(
                &mut events,
                &mut content,
                second_received,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(16),
            )
            .expect("decode second");
        assert!(matches!(
            settle_decoded_step(
                &mut events,
                &content,
                &second_step,
                second_attempt_id,
                second_decoded.into_semantic(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(17),
            )
            .expect("settle second"),
            SettledAgentStep::Yielded { .. }
        ));
        assert!(matches!(
            advance_agent_episode(
                &mut events,
                &mut content,
                &episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(18),
            )
            .expect("complete"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::Yielded,
                steps_started: 2,
            }
        ));
    }

    #[test]
    fn step_limit_and_deadline_stop_before_granting_another_step() {
        let (mut limited, _) = ready_episode(1, 3, None);
        assert!(matches!(
            advance_agent_episode(
                &mut limited.events,
                &mut limited.content,
                &limited.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(12),
            )
            .expect("limit"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::StepLimitReached,
                steps_started: 1,
            }
        ));

        let (mut deadline, _) = ready_episode(3, 3, Some(12));
        assert!(matches!(
            advance_agent_episode(
                &mut deadline.events,
                &mut deadline.content,
                &deadline.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(12),
            )
            .expect("deadline"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::DeadlineReached,
                steps_started: 1,
            }
        ));
        assert!(EpisodeStepLimit::new(0).is_err());
        let partially_disabled_budget: EpisodeBudget = serde_json::from_value(serde_json::json!({
            "step_limit": 2,
            "deadline_unix_ms": null
        }))
        .expect("partially disabled budget");
        assert_eq!(
            partially_disabled_budget
                .step_limit
                .map(EpisodeStepLimit::get),
            Some(2)
        );
        assert_eq!(partially_disabled_budget.tool_operation_limit, None);
        assert_eq!(partially_disabled_budget.provider_token_limit, None);
        assert_eq!(partially_disabled_budget.external_meter_limits, None);
        assert!(EpisodeProviderTokenLimit::new(0).is_err());
    }

    #[test]
    fn serialized_budget_dimensions_can_all_be_disabled() {
        let budget: EpisodeBudget = serde_json::from_value(serde_json::json!({
            "step_limit": null,
            "tool_operation_limit": null,
            "provider_token_limit": null,
            "deadline_unix_ms": null,
            "external_meter_limits": null
        }))
        .expect("disabled budget config");
        assert_eq!(
            budget,
            EpisodeBudget {
                step_limit: None,
                tool_operation_limit: None,
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            }
        );
        let omitted: EpisodeBudget =
            serde_json::from_value(serde_json::json!({})).expect("omitted budget config");
        assert_eq!(omitted, budget);
        let enabled: EpisodeBudget = serde_json::from_value(serde_json::json!({
            "step_limit": 4,
            "tool_operation_limit": 7,
            "provider_token_limit": 1000,
            "deadline_unix_ms": 2000,
            "external_meter_limits": [{"meter": "usd-micros", "units": 2500}]
        }))
        .expect("enabled budget config");
        assert_eq!(enabled.step_limit.map(EpisodeStepLimit::get), Some(4));
        assert_eq!(
            enabled
                .tool_operation_limit
                .map(EpisodeToolOperationLimit::get),
            Some(7)
        );
        assert_eq!(
            enabled
                .provider_token_limit
                .map(EpisodeProviderTokenLimit::get),
            Some(1000)
        );
        assert_eq!(
            enabled.deadline_unix_ms.map(EpisodeDeadlineUnixMillis::get),
            Some(2000)
        );
        let limits = enabled
            .external_meter_limits
            .as_deref()
            .expect("meter limits");
        assert_eq!(limits.len(), 1);
        assert_eq!(
            limits[0].meter(),
            &crate::ExternalMeterName::new("usd-micros").unwrap()
        );
        assert_eq!(limits[0].units().get(), 2500);
        let (mut fixture, step, attempt_id, dispatch) = prepared_episode_with_budget(budget);
        let results = complete_tool_step(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            &step,
            attempt_id,
            dispatch,
        );
        assert!(matches!(
            advance_agent_episode(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(i64::MAX),
            )
            .expect("all budget dimensions disabled"),
            EpisodeAdvance::NextStep(authority)
                if authority.expected_pending_results() == results
        ));
    }

    #[test]
    fn provider_token_receipts_accumulate_before_next_model_authority() {
        let (mut fixture, first_step, first_attempt, first_dispatch) =
            prepared_episode_with_token_limit(3, 3, Some(15), None);
        let first_usage =
            ProviderTokenUsage::new(ProviderTokenCount::new(7), ProviderTokenCount::new(3))
                .expect("first usage");
        let first_results = complete_tool_step_with_usage(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            &first_step,
            first_attempt,
            first_dispatch,
            Some(first_usage),
        );
        let second_step_id = StepId::new();
        let second_attempt_id = ModelAttemptId::new();
        let EpisodeAdvance::NextStep(authority) = advance_agent_episode(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            second_step_id,
            second_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("usage below limit") else {
            panic!("second step authority");
        };
        let second_input = decision(&mut fixture.content, first_results);
        let second_dispatch = prepare_episode_step(
            &mut fixture.events,
            &mut fixture.content,
            authority,
            &second_input,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(13),
        )
        .expect("prepare second step");
        let second_step = AgentStep::new(second_step_id).expect("second step");
        let second_usage =
            ProviderTokenUsage::new(ProviderTokenCount::new(4), ProviderTokenCount::new(1))
                .expect("second usage");
        complete_tool_step_with_usage(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            &second_step,
            second_attempt_id,
            second_dispatch,
            Some(second_usage),
        );
        let unused_step_id = StepId::new();
        assert!(matches!(
            advance_agent_episode(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                unused_step_id,
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(14),
            )
            .expect("token limit"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::ProviderTokenLimitReached,
                steps_started: 2,
            }
        ));
        assert!(
            fixture
                .events
                .read_stream(
                    AgentStep::new(unused_step_id)
                        .expect("unused step")
                        .stream_id(),
                    None,
                )
                .expect("unused history")
                .is_empty()
        );
        assert!(matches!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode)
                .expect("recover token completion"),
            AgentEpisodeState::Completed {
                reason: EpisodeCompletionReason::ProviderTokenLimitReached,
                steps_started: 2,
            }
        ));
    }

    #[test]
    fn configured_provider_token_budget_fails_closed_without_usage() {
        let (mut fixture, step, attempt_id, dispatch) =
            prepared_episode_with_token_limit(3, 1, Some(100), None);
        complete_tool_step(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            &step,
            attempt_id,
            dispatch,
        );
        assert!(matches!(
            advance_agent_episode(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(12),
            )
            .expect("missing usage"),
            EpisodeAdvance::Completed {
                reason: EpisodeCompletionReason::ProviderUsageUnavailable,
                steps_started: 1,
            }
        ));
        assert!(matches!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode)
                .expect("recover missing usage"),
            AgentEpisodeState::Completed {
                reason: EpisodeCompletionReason::ProviderUsageUnavailable,
                steps_started: 1,
            }
        ));
    }

    #[test]
    fn recovery_rejects_broken_episode_parent_and_result_lineage() {
        let (mut fixture, _) = ready_episode(3, 3, None);
        let next_step_id = StepId::new();
        let next_attempt_id = ModelAttemptId::new();
        advance_agent_episode(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            next_step_id,
            next_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("advance");
        let history = fixture
            .events
            .read_stream(fixture.episode.stream_id(), None)
            .expect("history");
        let mut broken_parent = history.clone();
        broken_parent[1].parent_event_id = None;
        assert!(project_episode(&broken_parent, fixture.episode.episode_id()).is_err());

        let mut broken_admission = history.clone();
        let admission_index = broken_admission
            .iter()
            .position(|event| event.schema_name.as_str() == super::OPERATIONS_ADMITTED)
            .expect("admission event");
        let mut admission: OperationsAdmittedPayload =
            cairn_codec::from_slice(&broken_admission[admission_index].payload)
                .expect("admission payload");
        admission.assignments[0].effect = ToolEffectClass::AmbiguousExternal;
        broken_admission[admission_index].payload =
            cairn_codec::to_vec(&admission).expect("corrupt admission");
        let projection = project_episode(&broken_admission, fixture.episode.episode_id())
            .expect("corrupt admission remains shaped");
        assert!(
            validate_previous_steps(&fixture.events, &mut fixture.content, &projection).is_err(),
            "settled registration metadata must match episode admission"
        );

        let mut broken_results = history;
        let advanced_index = broken_results
            .iter()
            .position(|event| event.schema_name.as_str() == super::STEP_ADVANCED)
            .expect("advanced event");
        let mut payload: AdvancedPayload =
            cairn_codec::from_slice(&broken_results[advanced_index].payload)
                .expect("advance payload");
        payload.pending_results.clear();
        broken_results[advanced_index].payload =
            cairn_codec::to_vec(&payload).expect("corrupt payload");
        let projection = project_episode(&broken_results, fixture.episode.episode_id())
            .expect("local episode facts remain shaped");
        assert!(
            validate_previous_steps(&fixture.events, &mut fixture.content, &projection).is_err()
        );

        let wrong_input = decision(&mut fixture.content, Vec::new());
        let next_step = AgentStep::new(next_step_id).expect("next step");
        prepare_agent_step(
            &mut fixture.events,
            &mut fixture.content,
            &next_step,
            &wrong_input,
            next_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(13),
        )
        .expect("raw step preparation");
        assert!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode,)
                .is_err(),
            "episode recovery must reject a step that bypassed pending-result lineage"
        );
    }

    #[test]
    fn tool_budget_exhaustion_completes_before_binding_or_authority() {
        let (mut fixture, step, attempt_id, dispatch) = prepared_episode(3, 0, None);
        assert!(matches!(
            settle_model_turn(
                &mut fixture.events,
                &mut fixture.content,
                &step,
                attempt_id,
                dispatch,
                vec![tool_call("call-budget")],
            ),
            SettledAgentStep::AwaitingOperations { .. }
        ));
        let operation_id = OperationId::new();
        let outcome = admit_episode_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            vec![read_source_assignment(operation_id)],
            &CommandId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("budget decision");
        assert!(matches!(
            outcome,
            EpisodeOperationAdmissionOutcome::Completed {
                reason: EpisodeCompletionReason::ToolOperationLimitReached,
                steps_started: 1,
            }
        ));
        let step_history = fixture
            .events
            .read_stream(step.stream_id(), None)
            .expect("step history");
        assert!(
            step_history
                .iter()
                .all(|event| event.schema_name.as_str()
                    != crate::step_operation::STEP_OPERATION_BOUND)
        );
        assert!(
            fixture
                .events
                .read_stream(&operation_stream(operation_id), None)
                .expect("operation history")
                .is_empty()
        );
        assert!(matches!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode)
                .expect("recover exhausted episode"),
            AgentEpisodeState::Completed {
                reason: EpisodeCompletionReason::ToolOperationLimitReached,
                steps_started: 1,
            }
        ));
    }

    #[test]
    fn admission_replay_closes_crash_window_before_step_binding() {
        let (mut fixture, step, attempt_id, dispatch) = prepared_episode(3, 1, None);
        settle_model_turn(
            &mut fixture.events,
            &mut fixture.content,
            &step,
            attempt_id,
            dispatch,
            vec![tool_call("call-crash")],
        );
        let operation_id = OperationId::new();
        let assignments = vec![read_source_assignment(operation_id)];
        let admission_command = CommandId::new();
        let binding_command = CommandId::new();
        let observed_at = cairn_protocol::ObservedAtUnixMillis::new(7);
        let history = fixture
            .events
            .read_stream(fixture.episode.stream_id(), None)
            .expect("episode history");
        append_operation_admission(
            &mut fixture.events,
            &fixture.episode,
            &history,
            step.step_id(),
            admission_payload_from_assignments(&assignments),
            &admission_command,
            observed_at,
        )
        .expect("admission fact");

        let EpisodeOperationAdmissionOutcome::Admitted(first) = admit_episode_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            assignments.clone(),
            &admission_command,
            &binding_command,
            observed_at,
        )
        .expect("finish binding") else {
            panic!("admitted");
        };
        assert_eq!(first.operations()[0].operation_id(), operation_id);
        let EpisodeOperationAdmissionOutcome::Admitted(replayed) = admit_episode_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            assignments,
            &admission_command,
            &binding_command,
            observed_at,
        )
        .expect("replay admission and binding") else {
            panic!("replayed admission");
        };
        assert_eq!(replayed.operations()[0].operation_id(), operation_id);
        assert!(matches!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode)
                .expect("recover bound episode"),
            AgentEpisodeState::Active {
                step_state: crate::AgentStepState::OperationsBound(_),
                ..
            }
        ));
    }

    #[test]
    fn episode_recovery_rejects_binding_that_bypassed_admission() {
        let (mut fixture, step, attempt_id, dispatch) = prepared_episode(3, 1, None);
        settle_model_turn(
            &mut fixture.events,
            &mut fixture.content,
            &step,
            attempt_id,
            dispatch,
            vec![tool_call("call-bypass")],
        );
        bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &step,
            attempt_id,
            vec![read_source_assignment(OperationId::new())],
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("raw binding");
        assert!(
            recover_agent_episode(&fixture.events, &mut fixture.content, &fixture.episode).is_err()
        );
    }

    #[test]
    fn tool_budget_accumulates_across_steps() {
        let (mut fixture, first_results) = ready_episode(3, 1, None);
        let next_step_id = StepId::new();
        let next_attempt_id = ModelAttemptId::new();
        let EpisodeAdvance::NextStep(authority) = advance_agent_episode(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.episode,
            next_step_id,
            next_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("advance") else {
            panic!("next step");
        };
        let input = decision(&mut fixture.content, first_results);
        let dispatch = prepare_episode_step(
            &mut fixture.events,
            &mut fixture.content,
            authority,
            &input,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(13),
        )
        .expect("prepare second step");
        let next_step = AgentStep::new(next_step_id).expect("next step");
        settle_model_turn(
            &mut fixture.events,
            &mut fixture.content,
            &next_step,
            next_attempt_id,
            dispatch,
            vec![tool_call("call-second")],
        );
        let second_operation_id = OperationId::new();
        assert!(matches!(
            admit_episode_operations(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.episode,
                vec![read_source_assignment(second_operation_id)],
                &CommandId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(14),
            )
            .expect("second budget decision"),
            EpisodeOperationAdmissionOutcome::Completed {
                reason: EpisodeCompletionReason::ToolOperationLimitReached,
                steps_started: 2,
            }
        ));
        let step_history = fixture
            .events
            .read_stream(next_step.stream_id(), None)
            .expect("second step history");
        assert!(
            step_history
                .iter()
                .all(|event| event.schema_name.as_str()
                    != crate::step_operation::STEP_OPERATION_BOUND)
        );
    }
}
