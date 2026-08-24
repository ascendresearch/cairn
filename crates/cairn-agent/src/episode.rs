use std::collections::HashSet;

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EpisodeId, EventId, ModelAttemptId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StepId, TaskId,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dispatch::recover_turn_input_decision;
use crate::{
    AgentRoleName, AgentStep, AgentStepState, DispatchAuthority, OperationResult,
    StepCoordinatorError, TurnInputDecision, prepare_agent_step, recover_agent_step,
};

const EPISODE_OPENED: &str = "agent.episode-opened";
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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeBudget {
    /// Maximum number of model steps that may start.
    pub step_limit: EpisodeStepLimit,
    /// Optional absolute deadline checked at safe step boundaries.
    pub deadline_unix_ms: Option<EpisodeDeadlineUnixMillis>,
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
}

#[derive(Clone)]
struct StepEntry {
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    expected_pending_results: Vec<ContentId<OperationResult>>,
}

struct EpisodeProjection {
    opened: OpenedPayload,
    steps: Vec<StepEntry>,
    completion: Option<CompletedPayload>,
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
        );
    }
    let event = episode_fact(EPISODE_OPENED, None, observed_at, &payload)?;
    events.append(
        &episode.stream,
        ExpectedRevision::NoStream,
        command_id,
        &[event],
    )?;
    step_authority(
        episode.episode_id,
        first_step_id,
        first_model_attempt_id,
        Vec::new(),
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
    if let Some(completion) = projection.completion {
        validate_completion(&projection.opened, &completion, &step_state)?;
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
    let steps_started = u32::try_from(projection.steps.len())
        .map_err(|_| EpisodeCoordinatorError::InvalidEpisode("too many episode steps".into()))?;
    if let Some(completion) = projection.completion {
        validate_completion(&projection.opened, &completion, &step_state)?;
        return Ok(EpisodeAdvance::Completed {
            reason: completion.reason,
            steps_started: completion.steps_started,
        });
    }
    if let Some(last) = history.last()
        && last.command_id == *command_id
        && last.schema_name.as_str() == STEP_ADVANCED
    {
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
    let reason = if projection
        .opened
        .budget
        .deadline_unix_ms
        .is_some_and(|deadline| deadline.is_reached(observed_at))
    {
        Some(EpisodeCompletionReason::DeadlineReached)
    } else if steps_started >= projection.opened.budget.step_limit.get() {
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
fn complete_episode<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    history: &[EventEnvelope],
    last_step_id: StepId,
    reason: EpisodeCompletionReason,
    steps_started: u32,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<EpisodeAdvance, EpisodeCoordinatorError> {
    let last = history
        .last()
        .ok_or_else(|| EpisodeCoordinatorError::InvalidEpisode("episode is empty".into()))?;
    let payload = CompletedPayload {
        episode_id: episode.episode_id,
        last_step_id,
        reason,
        steps_started,
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
) -> Result<EpisodeStepAuthority, EpisodeCoordinatorError> {
    Ok(EpisodeStepAuthority {
        episode_id,
        step: AgentStep::new(step_id)?,
        model_attempt_id,
        expected_pending_results,
    })
}

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
    let mut steps = vec![StepEntry {
        step_id: opened.first_step_id,
        model_attempt_id: opened.first_model_attempt_id,
        expected_pending_results: Vec::new(),
    }];
    let mut step_ids = HashSet::from([opened.first_step_id.to_string()]);
    let mut attempt_ids = HashSet::from([opened.first_model_attempt_id.to_string()]);
    let mut completion = None;
    let mut parent = first.event_id;
    for event in &history[1..] {
        if event.parent_event_id != Some(parent) {
            return invalid_episode("episode fact does not cite the previous fact");
        }
        match event.schema_name.as_str() {
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
                match payload.reason {
                    EpisodeCompletionReason::StepLimitReached
                        if expected_steps < opened.budget.step_limit.get() =>
                    {
                        return invalid_episode("episode completed before reaching its step limit");
                    }
                    EpisodeCompletionReason::DeadlineReached
                        if opened.budget.deadline_unix_ms.is_none_or(|deadline| {
                            !deadline
                                .is_reached(ObservedAtUnixMillis::new(event.observed_at_unix_ms))
                        }) =>
                    {
                        return invalid_episode("episode completed before its deadline");
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
        match recover_agent_step(events, content, &step, previous.model_attempt_id)? {
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
    completion: &CompletedPayload,
    step_state: &AgentStepState,
) -> Result<(), EpisodeCoordinatorError> {
    match completion.reason {
        EpisodeCompletionReason::Yielded
            if matches!(step_state, AgentStepState::Yielded { .. }) =>
        {
            Ok(())
        }
        EpisodeCompletionReason::StepLimitReached
            if completion.steps_started >= opened.budget.step_limit.get()
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
        AttemptId, CommandId, ContentId, ContentType, EpisodeId, ModelAttemptId, OperationId,
        StepId, TaskId,
    };
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        AdvancedPayload, AgentEpisode, AgentEpisodeState, EpisodeAdvance, EpisodeBudget,
        EpisodeCompletionReason, EpisodeDeadlineUnixMillis, EpisodeStepLimit,
        advance_agent_episode, open_agent_episode, prepare_episode_step, project_episode,
        recover_agent_episode, validate_previous_steps,
    };
    use crate::{
        AdapterModelTurn, AdapterOutputItem, AdapterVersion, AgentRoleName, AgentStep,
        CanonicalToolResult, ContextBlock, DeploymentName, DispatchAuthority, DispatchCompletion,
        HistoryItem, InstructionBlock, ModelName, ModelSelection, OperationResult, PolicyDocument,
        PreparedModelRequest, ProviderName, ProviderToolCallId, RecordedAdapterExchange,
        RecordedModelAdapter, RecordedToolExchange, RecordedToolGateway, ScriptedModelTransport,
        SettledAgentStep, StepOperationSettlement, ToolCatalog, ToolEffectClass,
        ToolImplementationVersion, ToolName, ToolOperationAssignment, ToolRegistration,
        TransportError, TurnInputDecision, authorize_tool_operation, begin_model_dispatch,
        begin_tool_operation, bind_step_operations, decode_model_response, execute_model_dispatch,
        execute_tool_operation, prepare_agent_step, settle_decoded_step, settle_step_operations,
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

    fn settle_model_turn(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
        items: Vec<AdapterOutputItem>,
    ) -> SettledAgentStep {
        let started = begin_model_dispatch(
            events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("begin model");
        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"raw-response".to_vec())
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

    fn complete_tool_step(
        events: &mut SqliteEventStore,
        content: &mut SqliteContentStore,
        step: &AgentStep,
        attempt_id: ModelAttemptId,
        authority: DispatchAuthority,
    ) -> Vec<ContentId<OperationResult>> {
        let settled = settle_model_turn(
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
        );
        assert!(matches!(
            settled,
            SettledAgentStep::AwaitingOperations { .. }
        ));
        let bound = bind_step_operations(
            events,
            content,
            step,
            attempt_id,
            vec![ToolOperationAssignment::new(
                OperationId::new(),
                ToolRegistration::new(
                    ToolName::new("read_source").expect("tool"),
                    ToolImplementationVersion::new("v1").expect("version"),
                    ToolEffectClass::ReadOnly,
                ),
            )],
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
        )
        .expect("bind");
        let operation = bound.into_operations().pop().expect("operation");
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

    fn ready_episode(
        step_limit: u32,
        deadline: Option<i64>,
    ) -> (ReadyEpisode, Vec<ContentId<OperationResult>>) {
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
        let budget = EpisodeBudget {
            step_limit: EpisodeStepLimit::new(step_limit).expect("limit"),
            deadline_unix_ms: deadline.map(EpisodeDeadlineUnixMillis::new),
        };
        let role = AgentRoleName::new("candidate-author").expect("role");
        let _lost = open_agent_episode(
            &mut events,
            &episode,
            task_id,
            role.clone(),
            budget,
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
            budget,
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
        let results = complete_tool_step(&mut events, &mut content, &step, attempt_id, dispatch);
        (
            ReadyEpisode {
                _directory: directory,
                content,
                events,
                episode,
            },
            results,
        )
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the end-to-end test keeps two-step authority and result lineage together"
    )]
    fn episode_runs_multiple_steps_and_recovers_advance_authority() {
        let (mut fixture, expected_results) = ready_episode(3, None);
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
    fn step_limit_and_deadline_stop_before_granting_another_step() {
        let (mut limited, _) = ready_episode(1, None);
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

        let (mut deadline, _) = ready_episode(3, Some(12));
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
    }

    #[test]
    fn recovery_rejects_broken_episode_parent_and_result_lineage() {
        let (mut fixture, _) = ready_episode(3, None);
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

        let mut broken_results = history;
        let mut payload: AdvancedPayload =
            cairn_codec::from_slice(&broken_results[1].payload).expect("advance payload");
        payload.pending_results.clear();
        broken_results[1].payload = cairn_codec::to_vec(&payload).expect("corrupt payload");
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
}
