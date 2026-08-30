//! Durable Controller workflow state from exact request freeze through admitted intent.

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EventId, ObservedAtUnixMillis, SchemaName,
    SchemaVersion, StreamRevision, TaskId,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use cairn_admission::{
    IntentAdmissionExecutableArtifact, IntentAdmissionPublicOutcomeArtifact,
    IntentAdmissionPublicOutcomeV1, IntentAdmissionRestrictedStoreArtifact,
    UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1, UserIntentDecisionArtifact,
    UserIntentDecisionV1,
};
use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentDecisionRequestBatchV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, MigrationIntentContractArtifact, ProposalHostPublicationV1,
    ProposalHostRequestArtifact, ProposalHostRequestV1, ProposalHostTerminalArtifact,
    ProposalHostTerminalV1, SirIntentHypothesisSetProposalArtifact,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1,
};

const WORKFLOW_FROZEN: &str = "migration.controller-workflow-frozen";
const SIR_EPISODE_AUTHORIZED: &str = "migration.controller-sir-episode-authorized";
const SIR_PROPOSAL_RECORDED: &str = "migration.controller-sir-proposal-recorded";
const INTENT_DECISION_REQUESTS_RECORDED: &str =
    "migration.controller-intent-decision-requests-recorded";
const USER_INTENT_DECISION_RECORDED: &str = "migration.controller-user-intent-decision-recorded";
const INTENT_ADMISSION_AUTHORIZED: &str = "migration.controller-intent-admission-authorized";
const ADMITTED_INTENT_RECORDED: &str = "migration.controller-admitted-intent-recorded";

/// Exact SIR authority frozen before the Controller may start the Proposal Host effect.
///
/// A SIR proposal identity cannot be substituted for the exact Host request authority.
///
/// ```compile_fail
/// use cairn_migration::SirIntentHypothesisSetProposalArtifact;
/// use cairn_server::FrozenSirAuthorityV1;
/// use cairn_protocol::ContentId;
/// fn require_request(authority: &FrozenSirAuthorityV1, proposal: ContentId<SirIntentHypothesisSetProposalArtifact>) {
///     let _: cairn_protocol::ContentId<cairn_migration::ProposalHostRequestArtifact> = proposal;
///     let _ = authority;
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSirAuthorityV1 {
    task_id: TaskId,
    request: ContentId<ProposalHostRequestArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    episode_id: cairn_protocol::EpisodeId,
}

impl FrozenSirAuthorityV1 {
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalHostRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }

    #[must_use]
    pub const fn episode_id(&self) -> cairn_protocol::EpisodeId {
        self.episode_id
    }
}

/// Durable Controller state reconstructed only from the current-V1 event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerWorkflowStateV1 {
    NotFound,
    Frozen(FrozenSirAuthorityV1),
    SirEpisodeAuthorized(FrozenSirAuthorityV1),
    SirProposed {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    },
    AwaitingUserIntentDecision {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
    },
    UserIntentDecisionRecorded {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
    },
    IntentAdmissionAuthorized {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    },
    AdmittedIntent {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
    },
}

/// One business action selected from recovered durable Controller state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerWorkflowNextActionV1 {
    None,
    AuthorizeSirEpisode(FrozenSirAuthorityV1),
    RunSirEpisode(FrozenSirAuthorityV1),
    DeriveIntentDecisionRequests {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalHostTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    },
    AwaitUserIntentDecision {
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
    },
    AuthorizeIntentAdmission {
        decision: ContentId<UserIntentDecisionArtifact>,
    },
    RunIntentAdmission {
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    },
    AwaitOracleWorkflow {
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
    },
}

impl ControllerWorkflowStateV1 {
    /// Selects the next architectural step without performing it.
    #[must_use]
    pub fn next_action(&self) -> ControllerWorkflowNextActionV1 {
        match self {
            Self::NotFound => ControllerWorkflowNextActionV1::None,
            Self::Frozen(authority) => {
                ControllerWorkflowNextActionV1::AuthorizeSirEpisode(authority.clone())
            }
            Self::SirEpisodeAuthorized(authority) => {
                ControllerWorkflowNextActionV1::RunSirEpisode(authority.clone())
            }
            Self::SirProposed {
                authority,
                terminal,
                proposal,
            } => ControllerWorkflowNextActionV1::DeriveIntentDecisionRequests {
                authority: authority.clone(),
                terminal: *terminal,
                proposal: *proposal,
            },
            Self::AwaitingUserIntentDecision {
                proposal, requests, ..
            } => ControllerWorkflowNextActionV1::AwaitUserIntentDecision {
                proposal: *proposal,
                requests: *requests,
            },
            Self::UserIntentDecisionRecorded { decision, .. } => {
                ControllerWorkflowNextActionV1::AuthorizeIntentAdmission {
                    decision: *decision,
                }
            }
            Self::IntentAdmissionAuthorized {
                decision,
                executable,
                restricted_store,
                ..
            } => ControllerWorkflowNextActionV1::RunIntentAdmission {
                decision: *decision,
                executable: *executable,
                restricted_store: *restricted_store,
            },
            Self::AdmittedIntent {
                outcome, contract, ..
            } => ControllerWorkflowNextActionV1::AwaitOracleWorkflow {
                outcome: *outcome,
                contract: *contract,
            },
        }
    }
}

/// Task-owned aggregate for the readable end-to-end Controller architecture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerWorkflowV1 {
    task_id: TaskId,
    stream: StreamId,
}

impl ControllerWorkflowV1 {
    /// Creates the current-V1 task-owned Controller aggregate.
    ///
    /// # Errors
    ///
    /// Rejects an identity that cannot be represented at the record boundary.
    pub fn new(task_id: TaskId) -> Result<Self, ControllerWorkflowError> {
        Ok(Self {
            task_id,
            stream: StreamId {
                kind: AggregateKind::new("controller-workflow")
                    .map_err(|error| invalid_history(error.to_string()))?,
                id: AggregateId::new(task_id.to_string())
                    .map_err(|error| invalid_history(error.to_string()))?,
            },
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkflowFrozenPayload {
    authority: FrozenSirAuthorityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SirEpisodeAuthorizedPayload {
    request: ContentId<ProposalHostRequestArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SirProposalRecordedPayload {
    terminal: ContentId<ProposalHostTerminalArtifact>,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentDecisionRequestsRecordedPayload {
    requests: ContentId<IntentDecisionRequestBatchArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct UserIntentDecisionRecordedPayload {
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    decision: ContentId<UserIntentDecisionArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentAdmissionAuthorizedPayload {
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AdmittedIntentRecordedPayload {
    outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
}

struct Projection {
    state: ControllerWorkflowStateV1,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    history: Vec<EventEnvelope>,
}

/// Freezes the exact task, Host request, input, model, tool/capability and episode authority.
///
/// # Errors
///
/// Rejects non-SIR roles, cross-task material, identity drift, replay conflicts, and persistence
/// failures.
#[allow(
    clippy::too_many_arguments,
    reason = "request/input identities, values, command authority, and observation time remain explicit"
)]
pub fn freeze_controller_workflow<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: &ProposalHostRequestV1,
    recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
    recovery_input: &IntentRecoveryInputV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    validate_frozen_material(
        workflow,
        request_id,
        request,
        recovery_input_id,
        recovery_input,
    )?;
    let projection = project(events, workflow)?;
    let payload = WorkflowFrozenPayload {
        authority: FrozenSirAuthorityV1 {
            task_id: workflow.task_id,
            request: request_id,
            recovery_input: recovery_input_id,
            episode_id: request.runtime().episode_id(),
        },
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        WORKFLOW_FROZEN,
        &payload,
    )? {
        return Ok(state);
    }
    if projection.state != ControllerWorkflowStateV1::NotFound {
        return Err(ControllerWorkflowError::InvalidTransition);
    }
    append_transition(
        events,
        workflow,
        ExpectedRevision::NoStream,
        command_id,
        observed_at,
        WORKFLOW_FROZEN,
        None,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits durable start authority before the external Proposal Host effect may run.
///
/// # Errors
///
/// Rejects an illegal transition, mismatched request, replay conflict, or persistence failure.
pub fn authorize_sir_episode<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: ContentId<ProposalHostRequestArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = SirEpisodeAuthorizedPayload { request };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        SIR_EPISODE_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::Frozen(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if authority.request != request {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        SIR_EPISODE_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records a strictly validated SIR Host terminal as the proposal observation.
///
/// # Errors
///
/// Rejects request, terminal, recovery-input, model, episode, role, identity, replay, and durable
/// state drift.
#[allow(clippy::too_many_arguments)]
pub fn record_sir_proposal<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: &ProposalHostRequestV1,
    terminal_id: ContentId<ProposalHostTerminalArtifact>,
    terminal: &ProposalHostTerminalV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    terminal.validate_against(request).map_err(binding_error)?;
    if terminal.identity().map_err(binding_error)? != terminal_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let ProposalHostPublicationV1::Sir { proposal_id, .. } = terminal.publication() else {
        return Err(ControllerWorkflowError::BindingMismatch);
    };
    let payload = SirProposalRecordedPayload {
        terminal: terminal_id,
        proposal: *proposal_id,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        SIR_PROPOSAL_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::SirEpisodeAuthorized(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    validate_terminal(authority, request, terminal_id, terminal)?;
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        SIR_PROPOSAL_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records model-free decision requests and stops before user intent authority exists.
///
/// # Errors
///
/// Rejects proposal/input/batch identity drift, an illegal transition, replay conflicts, or
/// persistence failures.
pub fn record_intent_decision_requests<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    requests_id: ContentId<IntentDecisionRequestBatchArtifact>,
    requests: &IntentDecisionRequestBatchV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    if requests.identity().map_err(binding_error)? != requests_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = IntentDecisionRequestsRecordedPayload {
        requests: requests_id,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        INTENT_DECISION_REQUESTS_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::SirProposed {
        authority,
        proposal,
        ..
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if requests.proposal() != *proposal || requests.recovery_input() != authority.recovery_input {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        INTENT_DECISION_REQUESTS_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records an authenticated task-authority decision without interpreting it as admitted intent.
///
/// # Errors
///
/// Rejects cross-task, request/batch/grant/decision identity drift, illegal transitions, replay
/// conflicts, or persistence failures.
#[allow(
    clippy::too_many_arguments,
    reason = "batch, request, grant, decision, command, and observation authorities remain explicit"
)]
pub fn record_user_intent_decision<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    batch: &IntentDecisionRequestBatchV1,
    request_id: ContentId<UserIntentDecisionRequestArtifact>,
    request: &UserIntentDecisionRequestV1,
    grant_id: ContentId<UserIntentAuthorityGrantArtifact>,
    grant: &UserIntentAuthorityGrantV1,
    decision_id: ContentId<UserIntentDecisionArtifact>,
    decision: &UserIntentDecisionV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if request.identity().map_err(binding_error)? != request_id
        || grant.identity().map_err(binding_error)? != grant_id
        || decision.identity().map_err(binding_error)? != decision_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let projection = project(events, workflow)?;
    let payload = UserIntentDecisionRecordedPayload {
        request: request_id,
        authority_grant: grant_id,
        decision: decision_id,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        USER_INTENT_DECISION_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::AwaitingUserIntentDecision {
        authority,
        proposal,
        requests,
        ..
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if batch.identity().map_err(binding_error)? != *requests
        || batch.proposal() != *proposal
        || batch.recovery_input() != authority.recovery_input
        || !batch.requests().iter().any(|candidate| {
            candidate == request
                && candidate
                    .identity()
                    .is_ok_and(|candidate_id| candidate_id == request_id)
        })
        || request.proposal() != *proposal
        || request.recovery_input() != authority.recovery_input
        || grant.task_id() != authority.task_id
        || decision.request() != request_id
        || decision.authority_grant() != grant_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        USER_INTENT_DECISION_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits the exact independent Admission executable before the restricted effect may start.
///
/// # Errors
///
/// Rejects decision drift, illegal transitions, replay conflicts, or persistence failures.
pub fn authorize_intent_admission<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = IntentAdmissionAuthorizedPayload {
        decision,
        executable,
        restricted_store,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        INTENT_ADMISSION_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::UserIntentDecisionRecorded {
        decision: recorded, ..
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if *recorded != decision {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        INTENT_ADMISSION_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records the public outcome returned after the independent Admission restricted commit.
///
/// # Errors
///
/// Rejects executable/decision/contract/outcome binding drift, illegal transitions, replay
/// conflicts, or persistence failures.
pub fn record_admitted_intent<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    outcome_id: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    outcome: &IntentAdmissionPublicOutcomeV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if outcome.identity().map_err(binding_error)? != outcome_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let contract = outcome.contract();
    let contract_id = contract.identity().map_err(binding_error)?;
    let projection = project(events, workflow)?;
    let payload = AdmittedIntentRecordedPayload {
        outcome: outcome_id,
        contract: contract_id,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ADMITTED_INTENT_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::IntentAdmissionAuthorized {
        authority,
        proposal,
        request,
        authority_grant,
        decision,
        ..
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if contract.task_id() != authority.task_id
        || contract.recovery_input() != authority.recovery_input
        || contract.proposal() != *proposal
        || contract.request() != *request
        || contract.authority_grant() != *authority_grant
        || contract.user_decision() != *decision
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ADMITTED_INTENT_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Recovers the exact current Controller state and rejects illegal/non-V1 history.
///
/// # Errors
///
/// Rejects persistence failures and any non-V1, noncanonical, causally broken, cross-task, or
/// illegal event history.
pub fn recover_controller_workflow<E: EventStore>(
    events: &E,
    workflow: &ControllerWorkflowV1,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    Ok(project(events, workflow)?.state)
}

fn validate_frozen_material(
    workflow: &ControllerWorkflowV1,
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: &ProposalHostRequestV1,
    recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
    recovery_input: &IntentRecoveryInputV1,
) -> Result<(), ControllerWorkflowError> {
    let expected_input = request.sir_recovery_input().map_err(binding_error)?;
    if request.identity().map_err(binding_error)? != request_id
        || recovery_input.identity().map_err(binding_error)? != recovery_input_id
        || expected_input != *recovery_input
        || expected_input.task_id() != workflow.task_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    Ok(())
}

fn validate_terminal(
    authority: &FrozenSirAuthorityV1,
    request: &ProposalHostRequestV1,
    terminal_id: ContentId<ProposalHostTerminalArtifact>,
    terminal: &ProposalHostTerminalV1,
) -> Result<(), ControllerWorkflowError> {
    terminal.validate_against(request).map_err(binding_error)?;
    if request.identity().map_err(binding_error)? != authority.request
        || request
            .sir_recovery_input()
            .map_err(binding_error)?
            .identity()
            .map_err(binding_error)?
            != authority.recovery_input
        || terminal.identity().map_err(binding_error)? != terminal_id
        || terminal.episode_id() != authority.episode_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let ProposalHostPublicationV1::Sir { proposal, .. } = terminal.publication() else {
        return Err(ControllerWorkflowError::BindingMismatch);
    };
    if proposal.recovery_input() != authority.recovery_input
        || proposal.episode_id() != authority.episode_id
        || proposal.model_configuration() != request.runtime().model_configuration()
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    Ok(())
}

fn project<E: EventStore>(
    events: &E,
    workflow: &ControllerWorkflowV1,
) -> Result<Projection, ControllerWorkflowError> {
    let history = events.read_stream(&workflow.stream, None)?;
    let mut state = ControllerWorkflowStateV1::NotFound;
    let mut parent_event_id = None;
    for event in &history {
        if event.schema_version != schema_v1() || event.parent_event_id != parent_event_id {
            return Err(invalid_history(
                "Controller event version or causal parent changed",
            ));
        }
        state = apply(
            workflow.task_id,
            state,
            event.schema_name.as_str(),
            &event.payload,
        )?;
        parent_event_id = Some(event.event_id);
    }
    let last = history.last();
    Ok(Projection {
        state,
        revision: last
            .map(|event| StreamRevision::new(event.sequence.get()))
            .transpose()
            .map_err(|error| invalid_history(error.to_string()))?,
        last_event_id: last.map(|event| event.event_id),
        history,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the aggregate reducer intentionally keeps the legal Controller stage sequence visible"
)]
fn apply(
    task_id: TaskId,
    state: ControllerWorkflowStateV1,
    schema: &str,
    bytes: &[u8],
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    match (state, schema) {
        (ControllerWorkflowStateV1::NotFound, WORKFLOW_FROZEN) => {
            let authority = decode::<WorkflowFrozenPayload>(bytes)?.authority;
            if authority.task_id != task_id {
                return Err(invalid_history("Controller task authority changed"));
            }
            Ok(ControllerWorkflowStateV1::Frozen(authority))
        }
        (ControllerWorkflowStateV1::Frozen(authority), SIR_EPISODE_AUTHORIZED) => {
            let payload: SirEpisodeAuthorizedPayload = decode(bytes)?;
            if payload.request != authority.request {
                return Err(invalid_history("authorized SIR request changed"));
            }
            Ok(ControllerWorkflowStateV1::SirEpisodeAuthorized(authority))
        }
        (ControllerWorkflowStateV1::SirEpisodeAuthorized(authority), SIR_PROPOSAL_RECORDED) => {
            let payload: SirProposalRecordedPayload = decode(bytes)?;
            Ok(ControllerWorkflowStateV1::SirProposed {
                authority,
                terminal: payload.terminal,
                proposal: payload.proposal,
            })
        }
        (
            ControllerWorkflowStateV1::SirProposed {
                authority,
                terminal,
                proposal,
            },
            INTENT_DECISION_REQUESTS_RECORDED,
        ) => Ok(ControllerWorkflowStateV1::AwaitingUserIntentDecision {
            authority,
            terminal,
            proposal,
            requests: decode::<IntentDecisionRequestsRecordedPayload>(bytes)?.requests,
        }),
        (
            ControllerWorkflowStateV1::AwaitingUserIntentDecision {
                authority,
                terminal,
                proposal,
                requests,
            },
            USER_INTENT_DECISION_RECORDED,
        ) => {
            let payload: UserIntentDecisionRecordedPayload = decode(bytes)?;
            Ok(ControllerWorkflowStateV1::UserIntentDecisionRecorded {
                authority,
                terminal,
                proposal,
                requests,
                request: payload.request,
                authority_grant: payload.authority_grant,
                decision: payload.decision,
            })
        }
        (
            ControllerWorkflowStateV1::UserIntentDecisionRecorded {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
            },
            INTENT_ADMISSION_AUTHORIZED,
        ) => {
            let payload: IntentAdmissionAuthorizedPayload = decode(bytes)?;
            if payload.decision != decision {
                return Err(invalid_history("Intent Admission decision changed"));
            }
            Ok(ControllerWorkflowStateV1::IntentAdmissionAuthorized {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
                executable: payload.executable,
                restricted_store: payload.restricted_store,
            })
        }
        (
            ControllerWorkflowStateV1::IntentAdmissionAuthorized {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
                executable,
                restricted_store,
            },
            ADMITTED_INTENT_RECORDED,
        ) => {
            let payload: AdmittedIntentRecordedPayload = decode(bytes)?;
            Ok(ControllerWorkflowStateV1::AdmittedIntent {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
                executable,
                restricted_store,
                outcome: payload.outcome,
                contract: payload.contract,
            })
        }
        (_, _) => Err(invalid_history(
            "illegal Controller workflow event transition",
        )),
    }
}

fn append_current<E: EventStore, P: Serialize>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    projection: &Projection,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &P,
) -> Result<(), ControllerWorkflowError> {
    append_transition(
        events,
        workflow,
        projection
            .revision
            .map(ExpectedRevision::Exact)
            .ok_or(ControllerWorkflowError::InvalidTransition)?,
        command_id,
        observed_at,
        schema,
        projection.last_event_id,
        payload,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_transition<E: EventStore, P: Serialize>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    expected: ExpectedRevision,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    parent_event_id: Option<EventId>,
    payload: &P,
) -> Result<(), ControllerWorkflowError> {
    events.append(
        &workflow.stream,
        expected,
        command_id,
        &[NewEvent {
            schema_name: SchemaName::new(schema)
                .map_err(|error| invalid_history(error.to_string()))?,
            schema_version: schema_v1(),
            parent_event_id,
            observed_at_unix_ms: observed_at.get(),
            payload: cairn_codec::to_vec(payload).map_err(codec)?,
        }],
    )?;
    Ok(())
}

fn exact_replay<P: Serialize>(
    projection: &Projection,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &P,
) -> Result<Option<ControllerWorkflowStateV1>, ControllerWorkflowError> {
    let Some(prior) = projection
        .history
        .iter()
        .find(|event| event.command_id == *command_id)
    else {
        return Ok(None);
    };
    if prior.schema_name.as_str() == schema
        && prior.schema_version == schema_v1()
        && prior.observed_at_unix_ms == observed_at.get()
        && prior.payload == cairn_codec::to_vec(payload).map_err(codec)?
    {
        Ok(Some(projection.state.clone()))
    } else {
        Err(ControllerWorkflowError::CommandConflict)
    }
}

fn decode<T: for<'de> Deserialize<'de> + Serialize>(
    bytes: &[u8],
) -> Result<T, ControllerWorkflowError> {
    let value = cairn_codec::from_slice(bytes).map_err(codec)?;
    if cairn_codec::to_vec(&value).map_err(codec)? != bytes {
        return Err(invalid_history("noncanonical Controller event payload"));
    }
    Ok(value)
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("schema version one is valid")
}

fn codec(error: impl std::fmt::Display) -> ControllerWorkflowError {
    ControllerWorkflowError::Codec(error.to_string())
}

fn binding_error(error: impl std::fmt::Display) -> ControllerWorkflowError {
    ControllerWorkflowError::Binding(error.to_string())
}

fn invalid_history(message: impl Into<String>) -> ControllerWorkflowError {
    ControllerWorkflowError::InvalidHistory(message.into())
}

/// Fail-closed Controller prefix transition error.
#[derive(Debug, Error)]
pub enum ControllerWorkflowError {
    #[error("Controller workflow transition is illegal from the current state")]
    InvalidTransition,
    #[error("Controller workflow exact artifact binding changed")]
    BindingMismatch,
    #[error("Controller workflow command was already used with different input")]
    CommandConflict,
    #[error("Controller workflow binding validation failed: {0}")]
    Binding(String),
    #[error("invalid Controller workflow history: {0}")]
    InvalidHistory(String),
    #[error("Controller workflow codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Event(#[from] EventStoreError),
}

#[cfg(test)]
mod tests {
    use cairn_agent::{
        AdapterVersion, DeploymentName, EpisodeBudget, EpisodeCompletionReason, EpisodeStepLimit,
        EpisodeToolOperationLimit, ModelName, ModelOutputTokenLimit, ModelSelection, ProviderName,
    };
    use cairn_protocol::{ContentType, EpisodeId};
    use cairn_store_sqlite::SqliteEventStore;
    use serde_json::{Value, json};

    use super::*;
    use cairn_admission::{
        AuthoritativeIntentClaimV1, CollectionOutputIntentV1, CollectionOutputOrderContractV1,
        IntentAdmissionRestrictedStoreArtifact, TaskIntentAuthoritySubject,
        UserIntentAuthorityGrantV1, UserIntentAuthorityScopeV1, UserIntentDecisionResponseV1,
        UserIntentDecisionV1, promote_user_intent,
    };
    use cairn_migration::{
        AgentResolvedRuntimeModelArtifact, IntentHypothesisSetProposalV1, IntentRecoveryRequestV1,
        ProposalHostBinaryIdentity, ProposalHostRoleRequestV1, ProposalHostRuntimeV1,
        ProposalHostTaskSnapshotV1, ProposalHostTaskSourceV1, SirCallerClaimId, SirHypothesisId,
        SirSourceLineCount, SirTaskArtifactBytes, SirTaskArtifactPath, SirTaskArtifactV1,
        SirTaskBundleV1, SirTaskLimits, derive_user_intent_decision_requests,
    };

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn request(task_id: TaskId) -> (ProposalHostRequestV1, IntentRecoveryInputV1) {
        let source = "// generic source line\n".repeat(24);
        let path = SirTaskArtifactPath::new("src/compact_above.cu").expect("path");
        let artifact: SirTaskArtifactV1 = serde_json::from_value(json!({
            "path":path,
            "identity":id::<SirTaskArtifactBytes>(source.as_bytes()),
            "line_count":SirSourceLineCount::new(24)
        }))
        .expect("artifact");
        let bundle: SirTaskBundleV1 = serde_json::from_value(json!({
            "schema_version":1,
            "artifacts":[artifact]
        }))
        .expect("bundle");
        let recovery_request: IntentRecoveryRequestV1 = serde_json::from_str(include_str!(
            "../../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json"
        ))
        .expect("caller intent");
        let runtime = ProposalHostRuntimeV1::new(
            EpisodeId::new(),
            ProposalHostBinaryIdentity::new(format!("sha256:{}", "a".repeat(64))).expect("binary"),
            id::<AgentResolvedRuntimeModelArtifact>(b"generic model"),
            ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("recorded-model").expect("model"),
                deployment: DeploymentName::new("isolated").expect("deployment"),
                adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
            },
            EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(4).expect("steps")),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(8)),
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            },
            ModelOutputTokenLimit::new(4_096).expect("output"),
            SirTaskLimits::default(),
        );
        let request = ProposalHostRequestV1::new(
            runtime,
            ProposalHostRoleRequestV1::Sir {
                task_id,
                recovery_request,
                task: ProposalHostTaskSnapshotV1::new(
                    bundle,
                    vec![ProposalHostTaskSourceV1::new(path, source)],
                ),
            },
        )
        .expect("SIR request");
        let recovery_input = request.sir_recovery_input().expect("frozen input");
        (request, recovery_input)
    }

    fn proposal(
        recovery_input: ContentId<IntentRecoveryInputArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    ) -> IntentHypothesisSetProposalV1 {
        serde_json::from_value(json!({
            "schema_version":1,
            "recovery_input":recovery_input,
            "episode_id":episode_id,
            "model_configuration":model_configuration,
            "submission":submission_value()
        }))
        .expect("proposal")
    }

    fn submission_value() -> Value {
        json!({
            "schema_version":1,
            "observed_facts":[{
                "id":"atomic-slot-allocation",
                "statement":"The source allocates output slots atomically.",
                "citations":[{"path":"src/compact_above.cu","start_line":16,"end_line":20}]
            }],
            "hypotheses":[
                {
                    "id":"order-unspecified","layer":"observable-contract",
                    "claim":"Any permutation of qualifying values is acceptable.",
                    "domain":"Successful calls with sufficient output capacity.",
                    "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
                    "counter_evidence":[]
                },
                {
                    "id":"stable-order","layer":"observable-contract",
                    "claim":"Qualifying values retain input-relative order.",
                    "domain":"Successful calls with sufficient output capacity.",
                    "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
                    "counter_evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
                }
            ],
            "conflicts":[{
                "id":"output-order-conflict",
                "statement":"The two proposed output-order contracts are incompatible.",
                "claims":[
                    {"source":"hypothesis","hypothesis":"order-unspecified"},
                    {"source":"hypothesis","hypothesis":"stable-order"}
                ],
                "evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
            }],
            "unknowns":[{
                "id":"output-order","kind":"desired-semantics",
                "question":"Must output preserve input-relative order?",
                "evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
            }],
            "invariants":[{
                "id":"copied-values","statement":"Every copied value came from input.",
                "evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}]
            }],
            "optimization_freedoms":[],
            "source_dispositions":[],
            "disambiguation_experiments":[{
                "id":"decide-output-order",
                "targets":[
                    {"kind":"conflict","conflict":"output-order-conflict"},
                    {"kind":"unknown","unknown":"output-order"}
                ],
                "plan":"Ask the task authority whether output ordering is observable.",
                "predictions":["Stable use selects stable order.","Insensitive use permits either order."]
            }]
        })
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one linear control keeps the complete durable prefix and its fail-closed probes visible"
    )]
    fn durable_intent_path_stops_at_oracle_and_rejects_authority_drift() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut events =
            SqliteEventStore::open(temporary.path().join("events.db")).expect("event store");
        let task_id = TaskId::new();
        let workflow = ControllerWorkflowV1::new(task_id).expect("workflow");
        let (request, recovery_input) = request(task_id);
        let request_id = request.identity().expect("request id");
        let recovery_input_id = recovery_input.identity().expect("input id");
        let observed_at = ObservedAtUnixMillis::new(1);
        let freeze_command = CommandId::new();

        assert!(matches!(
            freeze_controller_workflow(
                &mut events,
                &ControllerWorkflowV1::new(TaskId::new()).expect("other workflow"),
                request_id,
                &request,
                recovery_input_id,
                &recovery_input,
                &CommandId::new(),
                observed_at,
            ),
            Err(ControllerWorkflowError::BindingMismatch)
        ));

        let state = freeze_controller_workflow(
            &mut events,
            &workflow,
            request_id,
            &request,
            recovery_input_id,
            &recovery_input,
            &freeze_command,
            observed_at,
        )
        .expect("freeze");
        let ControllerWorkflowNextActionV1::AuthorizeSirEpisode(authority) = state.next_action()
        else {
            panic!("expected durable start authorization");
        };
        let authorize_command = CommandId::new();
        let state = authorize_sir_episode(
            &mut events,
            &workflow,
            authority.request(),
            &authorize_command,
            ObservedAtUnixMillis::new(2),
        )
        .expect("authorize");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunSirEpisode(_)
        ));

        let drifted_proposal = proposal(
            recovery_input_id,
            request.runtime().episode_id(),
            id::<AgentResolvedRuntimeModelArtifact>(b"drifted model"),
        );
        let drifted_proposal_id = drifted_proposal.identity().expect("proposal id");
        let drifted_terminal: ProposalHostTerminalV1 = serde_json::from_value(json!({
            "schema_version":1,
            "request":request_id,
            "episode_id":request.runtime().episode_id(),
            "publication":{
                "role":"sir",
                "proposal_id":drifted_proposal_id,
                "proposal":drifted_proposal
            },
            "completion_reason":EpisodeCompletionReason::Yielded,
            "steps_started":1
        }))
        .expect("terminal");
        assert!(matches!(
            record_sir_proposal(
                &mut events,
                &workflow,
                &request,
                drifted_terminal.identity().expect("terminal id"),
                &drifted_terminal,
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(ControllerWorkflowError::BindingMismatch)
        ));

        let proposal = proposal(
            recovery_input_id,
            request.runtime().episode_id(),
            request.runtime().model_configuration(),
        );
        let proposal_id = proposal.identity().expect("proposal id");
        let terminal: ProposalHostTerminalV1 = serde_json::from_value(json!({
            "schema_version":1,
            "request":request_id,
            "episode_id":request.runtime().episode_id(),
            "publication":{
                "role":"sir",
                "proposal_id":proposal_id,
                "proposal":proposal
            },
            "completion_reason":EpisodeCompletionReason::Yielded,
            "steps_started":1
        }))
        .expect("terminal");
        let terminal_id = terminal.identity().expect("terminal id");
        let proposal_command = CommandId::new();
        let state = record_sir_proposal(
            &mut events,
            &workflow,
            &request,
            terminal_id,
            &terminal,
            &proposal_command,
            ObservedAtUnixMillis::new(3),
        )
        .expect("record proposal");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::DeriveIntentDecisionRequests { .. }
        ));

        let batch = derive_user_intent_decision_requests(
            proposal_id,
            match terminal.publication() {
                ProposalHostPublicationV1::Sir { proposal, .. } => proposal,
                _ => unreachable!(),
            },
            recovery_input_id,
            &recovery_input,
        )
        .expect("derive decision requests");
        let batch_id = batch.identity().expect("batch id");
        let decision_requests_command = CommandId::new();
        let state = record_intent_decision_requests(
            &mut events,
            &workflow,
            batch_id,
            &batch,
            &decision_requests_command,
            ObservedAtUnixMillis::new(4),
        )
        .expect("record requests");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::AwaitUserIntentDecision {
                proposal: proposal_id,
                requests: batch_id,
            }
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("restart recovery"),
            state
        );
        assert_eq!(
            record_intent_decision_requests(
                &mut events,
                &workflow,
                batch_id,
                &batch,
                &decision_requests_command,
                ObservedAtUnixMillis::new(4),
            )
            .expect("exact command replay"),
            state
        );
        assert!(matches!(
            record_intent_decision_requests(
                &mut events,
                &workflow,
                batch_id,
                &batch,
                &decision_requests_command,
                ObservedAtUnixMillis::new(5),
            ),
            Err(ControllerWorkflowError::CommandConflict)
        ));

        let user_request = batch.requests()[0].clone();
        let user_request_id = user_request.identity().expect("user request id");
        let selection_claim =
            SirCallerClaimId::new("copies-strictly-above").expect("selection claim");
        let cross_task_grant = UserIntentAuthorityGrantV1::new(
            TaskId::new(),
            TaskIntentAuthoritySubject::new("task-authority:cross-task").expect("subject"),
            UserIntentAuthorityScopeV1::CollectionOutput {
                selection_claim: selection_claim.clone(),
            },
        );
        let cross_task_grant_id = cross_task_grant.identity().expect("cross-task grant id");
        let cross_task_decision = UserIntentDecisionV1::new(
            user_request_id,
            cross_task_grant_id,
            UserIntentDecisionResponseV1::SelectHypothesis {
                hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
                authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                    CollectionOutputIntentV1::exact_selected_occurrences(
                        selection_claim.clone(),
                        CollectionOutputOrderContractV1::UnspecifiedPermutation,
                    ),
                ),
            },
        );
        assert!(matches!(
            record_user_intent_decision(
                &mut events,
                &workflow,
                &batch,
                user_request_id,
                &user_request,
                cross_task_grant_id,
                &cross_task_grant,
                cross_task_decision
                    .identity()
                    .expect("cross-task decision id"),
                &cross_task_decision,
                &CommandId::new(),
                ObservedAtUnixMillis::new(5),
            ),
            Err(ControllerWorkflowError::BindingMismatch)
        ));

        let grant = UserIntentAuthorityGrantV1::new(
            task_id,
            TaskIntentAuthoritySubject::new("task-authority:user").expect("subject"),
            UserIntentAuthorityScopeV1::CollectionOutput {
                selection_claim: selection_claim.clone(),
            },
        );
        let grant_id = grant.identity().expect("grant id");
        let decision = UserIntentDecisionV1::new(
            user_request_id,
            grant_id,
            UserIntentDecisionResponseV1::SelectHypothesis {
                hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
                authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                    CollectionOutputIntentV1::exact_selected_occurrences(
                        selection_claim,
                        CollectionOutputOrderContractV1::UnspecifiedPermutation,
                    ),
                ),
            },
        );
        let decision_id = decision.identity().expect("decision id");
        let decision_command = CommandId::new();
        let state = record_user_intent_decision(
            &mut events,
            &workflow,
            &batch,
            user_request_id,
            &user_request,
            grant_id,
            &grant,
            decision_id,
            &decision,
            &decision_command,
            ObservedAtUnixMillis::new(5),
        )
        .expect("record user decision");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::AuthorizeIntentAdmission {
                decision: decision_id
            }
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("decision restart recovery"),
            state
        );

        let executable = id::<IntentAdmissionExecutableArtifact>(b"admission executable");
        let restricted_store = id::<IntentAdmissionRestrictedStoreArtifact>(b"restricted store");
        let admission_command = CommandId::new();
        let state = authorize_intent_admission(
            &mut events,
            &workflow,
            decision_id,
            executable,
            restricted_store,
            &admission_command,
            ObservedAtUnixMillis::new(6),
        )
        .expect("authorize Admission");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunIntentAdmission {
                decision: decision_id,
                executable,
                restricted_store,
            }
        );
        assert_eq!(
            authorize_intent_admission(
                &mut events,
                &workflow,
                decision_id,
                executable,
                restricted_store,
                &admission_command,
                ObservedAtUnixMillis::new(6),
            )
            .expect("exact Admission authority replay"),
            state
        );

        let admitted = promote_user_intent(
            proposal_id,
            match terminal.publication() {
                ProposalHostPublicationV1::Sir { proposal, .. } => proposal,
                _ => unreachable!(),
            },
            recovery_input_id,
            &recovery_input,
            user_request_id,
            &user_request,
            grant_id,
            &grant,
            decision_id,
            &decision,
        )
        .expect("prepare admitted intent");
        let outcome = admitted.public_outcome();
        let outcome_id = outcome.identity().expect("outcome id");
        assert!(matches!(
            record_admitted_intent(
                &mut events,
                &workflow,
                id::<IntentAdmissionPublicOutcomeArtifact>(b"drifted outcome"),
                outcome,
                &CommandId::new(),
                ObservedAtUnixMillis::new(7),
            ),
            Err(ControllerWorkflowError::BindingMismatch)
        ));
        let state = record_admitted_intent(
            &mut events,
            &workflow,
            outcome_id,
            outcome,
            &CommandId::new(),
            ObservedAtUnixMillis::new(7),
        )
        .expect("record admitted intent");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::AwaitOracleWorkflow {
                outcome: outcome_id,
                contract: outcome.contract().identity().expect("contract id"),
            }
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("admission restart recovery"),
            state
        );
    }
}
