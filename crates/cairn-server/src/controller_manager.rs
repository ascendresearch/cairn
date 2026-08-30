//! Readable Controller driver for the durable intent-recovery workflow prefix.

use std::io::Cursor;

use cairn_admission::{
    IntentAdmissionPublicOutcomeArtifact, MigrationIntentContractV1,
    UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1, UserIntentDecisionArtifact,
    UserIntentDecisionV1,
};
use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, MigrationIntentContractArtifact, ProposalHostPublicationV1,
    ProposalHostRequestArtifact, ProposalHostRequestV1, ProposalHostTerminalArtifact,
    ProposalHostTerminalV1, SirIntentHypothesisSetProposalArtifact,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{CommandId, ContentId, ContentType, EpisodeId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Serialize, de::DeserializeOwned};

use crate::controller_state::{
    ControllerWorkflowNextActionV1, ControllerWorkflowV1, FrozenSirAuthorityV1,
    authorize_intent_admission, authorize_sir_episode, freeze_controller_workflow,
    record_admitted_intent, record_intent_decision_requests, record_sir_proposal,
    record_user_intent_decision, recover_controller_workflow,
};
use crate::intent_admission_supervisor::{
    IntentAdmissionProcessBlockedV1, IntentAdmissionProcessConfigV1, run_intent_admission_process,
};
use crate::proposal_host_supervisor::{
    ProposalHostProcessBlockedV1, ProposalHostProcessConfigV1, initialize_proposal_host_operation,
    run_proposal_host_process,
};
use crate::{ControllerWorkflowStateV1, ServerConfig, ServerError, observed_now};

/// Outcome of consuming at most one durable Controller-prefix action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerWorkflowManagerStatusV1 {
    Idle,
    Advanced,
    AwaitingUserIntentDecision {
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
    },
    ProposalHostBlocked {
        episode_id: EpisodeId,
        reason: ProposalHostProcessBlockedV1,
    },
    IntentAdmissionBlocked {
        decision: ContentId<UserIntentDecisionArtifact>,
        reason: IntentAdmissionProcessBlockedV1,
    },
    AwaitingOracleWorkflow {
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
    },
}

/// Archives and durably freezes one exact SIR request before any model effect.
///
/// # Errors
///
/// Rejects noncanonical, non-SIR, cross-task, storage, or workflow material.
pub fn freeze_sir_controller_request(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    request: &ProposalHostRequestV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let recovery_input = request.sir_recovery_input().map_err(manager_error)?;
    let mut content = open_content(server)?;
    let request_id = archive::<ProposalHostRequestArtifact, _>(&mut content, request)?;
    let recovery_input_id =
        archive::<IntentRecoveryInputArtifact, _>(&mut content, &recovery_input)?;
    let mut events = open_events(server)?;
    freeze_controller_workflow(
        &mut events,
        workflow,
        request_id,
        request,
        recovery_input_id,
        &recovery_input,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Records an authenticated user decision as Controller input without admitting it.
///
/// # Errors
///
/// Rejects missing, cross-task, noncanonical, or incorrectly bound decision material.
pub fn record_controller_user_intent_decision(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    grant: &UserIntentAuthorityGrantV1,
    decision: &UserIntentDecisionV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::AwaitingUserIntentDecision { requests, .. } = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller is not awaiting a user intent decision".into(),
        ));
    };
    let mut content = open_content(server)?;
    let batch: cairn_migration::IntentDecisionRequestBatchV1 = load_canonical(&content, requests)?;
    let request: UserIntentDecisionRequestV1 = load_canonical(&content, decision.request())?;
    let grant_id = archive::<UserIntentAuthorityGrantArtifact, _>(&mut content, grant)?;
    let decision_id = archive::<UserIntentDecisionArtifact, _>(&mut content, decision)?;
    let mut events = open_events(server)?;
    record_user_intent_decision(
        &mut events,
        workflow,
        &batch,
        decision.request(),
        &request,
        grant_id,
        grant,
        decision_id,
        decision,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Consumes one readable business step selected from exact durable Controller state.
///
/// The top-level body deliberately remains the architecture: recover, select, execute.
///
/// # Errors
///
/// Returns configuration, canonical storage, durable workflow, or Proposal Host initialization
/// failures without selecting a replacement task, request, or episode.
pub async fn drive_controller_workflow_once(
    server: &ServerConfig,
    proposal_host: &ProposalHostProcessConfigV1,
    intent_admission: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let action = select_controller_action(&state);
    execute_controller_action(server, proposal_host, intent_admission, workflow, action).await
}

fn recover_controller_turn(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
) -> Result<ControllerWorkflowStateV1, ServerError> {
    let events = open_events(server)?;
    recover_controller_workflow(&events, workflow).map_err(manager_error)
}

fn select_controller_action(state: &ControllerWorkflowStateV1) -> ControllerWorkflowNextActionV1 {
    state.next_action()
}

async fn execute_controller_action(
    server: &ServerConfig,
    proposal_host: &ProposalHostProcessConfigV1,
    intent_admission: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    action: ControllerWorkflowNextActionV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    match action {
        ControllerWorkflowNextActionV1::None => Ok(ControllerWorkflowManagerStatusV1::Idle),
        ControllerWorkflowNextActionV1::AuthorizeSirEpisode(authority) => {
            authorize_sir_episode_start(server, workflow, &authority)
        }
        ControllerWorkflowNextActionV1::RunSirEpisode(authority) => {
            run_authorized_sir_episode(server, proposal_host, workflow, &authority).await
        }
        ControllerWorkflowNextActionV1::DeriveIntentDecisionRequests {
            authority,
            terminal,
            proposal,
        } => derive_and_record_intent_decision_requests(
            server, workflow, &authority, terminal, proposal,
        ),
        ControllerWorkflowNextActionV1::AwaitUserIntentDecision { proposal, requests } => Ok(
            ControllerWorkflowManagerStatusV1::AwaitingUserIntentDecision { proposal, requests },
        ),
        ControllerWorkflowNextActionV1::AuthorizeIntentAdmission { decision } => {
            authorize_intent_admission_start(server, intent_admission, workflow, decision)
        }
        ControllerWorkflowNextActionV1::RunIntentAdmission {
            decision,
            executable,
            restricted_store,
        } => {
            run_authorized_intent_admission(
                server,
                intent_admission,
                workflow,
                decision,
                executable,
                restricted_store,
            )
            .await
        }
        ControllerWorkflowNextActionV1::AwaitOracleWorkflow { outcome, contract } => {
            Ok(ControllerWorkflowManagerStatusV1::AwaitingOracleWorkflow { outcome, contract })
        }
    }
}

fn authorize_sir_episode_start(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenSirAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut events = open_events(server)?;
    authorize_sir_episode(
        &mut events,
        workflow,
        authority.request(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

async fn run_authorized_sir_episode(
    server: &ServerConfig,
    proposal_host: &ProposalHostProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenSirAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let request: ProposalHostRequestV1 = load_canonical(&content, authority.request())?;
    initialize_proposal_host_operation(proposal_host, request.runtime())?;
    let terminal = match run_proposal_host_process(proposal_host, &request).await {
        Ok(terminal) => terminal,
        Err(failure) => {
            tracing::warn!(
                target: "cairn.server.controller-workflow",
                event = "sir_proposal_host_blocked",
                task_id = %workflow.task_id(),
                episode_id = %authority.episode_id(),
                reason = ?failure.reason,
                diagnostic = %failure.diagnostic,
                "SIR Proposal Host operation requires reconciliation"
            );
            return Ok(ControllerWorkflowManagerStatusV1::ProposalHostBlocked {
                episode_id: authority.episode_id(),
                reason: failure.reason,
            });
        }
    };
    let ProposalHostPublicationV1::Sir { proposal, .. } = terminal.publication() else {
        return Err(ServerError::MigrationWorkflow(
            "SIR Host returned a non-SIR publication".into(),
        ));
    };
    let _ = archive::<SirIntentHypothesisSetProposalArtifact, _>(&mut content, proposal)?;
    let terminal_id = archive::<ProposalHostTerminalArtifact, _>(&mut content, &terminal)?;
    let mut events = open_events(server)?;
    record_sir_proposal(
        &mut events,
        workflow,
        &request,
        terminal_id,
        &terminal,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn derive_and_record_intent_decision_requests(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenSirAuthorityV1,
    terminal_id: ContentId<ProposalHostTerminalArtifact>,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let terminal: ProposalHostTerminalV1 = load_canonical(&content, terminal_id)?;
    let proposal: IntentHypothesisSetProposalV1 = load_canonical(&content, proposal_id)?;
    let recovery_input: IntentRecoveryInputV1 =
        load_canonical(&content, authority.recovery_input())?;
    if !matches!(
        terminal.publication(),
        ProposalHostPublicationV1::Sir { proposal_id: id, .. } if *id == proposal_id
    ) {
        return Err(ServerError::MigrationWorkflow(
            "SIR terminal changed the durable proposal observation".into(),
        ));
    }
    let requests = derive_user_intent_decision_requests(
        proposal_id,
        &proposal,
        authority.recovery_input(),
        &recovery_input,
    )
    .map_err(manager_error)?;
    for request in requests.requests() {
        let _ = archive::<UserIntentDecisionRequestArtifact, _>(&mut content, request)?;
    }
    let requests_id = archive::<IntentDecisionRequestBatchArtifact, _>(&mut content, &requests)?;
    let mut events = open_events(server)?;
    record_intent_decision_requests(
        &mut events,
        workflow,
        requests_id,
        &requests,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn authorize_intent_admission_start(
    server: &ServerConfig,
    config: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    decision: ContentId<UserIntentDecisionArtifact>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    config.validate(server)?;
    let executable = config.executable_identity()?;
    let restricted_store = config.restricted_store_identity()?;
    let mut events = open_events(server)?;
    authorize_intent_admission(
        &mut events,
        workflow,
        decision,
        executable,
        restricted_store,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

async fn run_authorized_intent_admission(
    server: &ServerConfig,
    config: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<cairn_admission::IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<cairn_admission::IntentAdmissionRestrictedStoreArtifact>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let outcome =
        match run_intent_admission_process(config, server, decision, executable, restricted_store)
            .await
        {
            Ok(outcome) => outcome,
            Err(failure) => {
                tracing::warn!(
                    target: "cairn.server.controller-workflow",
                    event = "intent_admission_blocked",
                    task_id = %workflow.task_id(),
                    decision = %decision,
                    reason = ?failure.reason,
                    diagnostic = %failure.diagnostic,
                    "Intent Admission operation requires reconciliation"
                );
                return Ok(ControllerWorkflowManagerStatusV1::IntentAdmissionBlocked {
                    decision,
                    reason: failure.reason,
                });
            }
        };
    let mut content = open_content(server)?;
    let contract: &MigrationIntentContractV1 = outcome.contract();
    let _ = archive::<MigrationIntentContractArtifact, _>(&mut content, contract)?;
    let outcome_id = archive::<IntentAdmissionPublicOutcomeArtifact, _>(&mut content, &outcome)?;
    let mut events = open_events(server)?;
    record_admitted_intent(
        &mut events,
        workflow,
        outcome_id,
        &outcome,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn open_events(server: &ServerConfig) -> Result<SqliteEventStore, ServerError> {
    server.validate_schema()?;
    SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)
}

fn open_content(server: &ServerConfig) -> Result<SqliteContentStore, ServerError> {
    server.validate_schema()?;
    SqliteContentStore::open(
        &server.storage.content_database,
        &server.storage.content_directory,
    )
    .map_err(manager_error)
}

fn archive<T: ContentType, V: Serialize>(
    content: &mut SqliteContentStore,
    value: &V,
) -> Result<ContentId<T>, ServerError> {
    let bytes = cairn_codec::to_vec(value).map_err(manager_error)?;
    let expected = ContentId::<T>::derive(&bytes).map_err(manager_error)?;
    let actual = content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(manager_error)?
        .content_id;
    if actual != expected {
        return Err(ServerError::MigrationWorkflow(
            "Controller artifact changed its canonical typed identity during archival".into(),
        ));
    }
    Ok(actual)
}

fn load_canonical<T: ContentType, V: DeserializeOwned + Serialize>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<V, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(manager_error)?;
    let value = cairn_codec::from_slice(&bytes).map_err(manager_error)?;
    if cairn_codec::to_vec(&value).map_err(manager_error)? != bytes
        || ContentId::<T>::derive(&bytes).map_err(manager_error)? != id
    {
        return Err(ServerError::MigrationWorkflow(
            "Controller artifact changed its canonical typed identity".into(),
        ));
    }
    Ok(value)
}

fn manager_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
