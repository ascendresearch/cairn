//! Readable Controller driver for the durable intent-recovery workflow prefix.

use std::io::Cursor;

use cairn_admission::{
    IntentAdmissionPublicOutcomeArtifact, MigrationIntentContractV1,
    UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1, UserIntentDecisionArtifact,
    UserIntentDecisionV1,
};
use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, MigrationIntentContractArtifact, OracleBuildTestSnapshotArtifact,
    OracleClaimArtifact, OracleClaimV1, OracleCoveragePolicyArtifact, OracleCoveragePolicyV1,
    OracleDocumentationSnapshotArtifact, OracleExperimentToolCatalogArtifact,
    OracleExplorationCapabilityGrantArtifact, OracleExplorationLedgerArtifact,
    OracleExplorationLedgerV1, OracleKnowledgeSnapshotArtifact, OracleResearchToolCatalogArtifact,
    OracleSourceSnapshotArtifact, OracleStrategyCatalogArtifact, OracleStrategyCatalogV1,
    OracleWorkspaceArtifact, OracleWorkspaceV1, ProposalHostPublicationV1,
    ProposalHostRequestArtifact, ProposalHostRequestV1, ProposalHostTerminalArtifact,
    ProposalHostTerminalV1, SirIntentHypothesisSetProposalArtifact,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1, derive_oracle_work_items,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{CommandId, ContentId, ContentType, EpisodeId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Serialize, de::DeserializeOwned};

use crate::controller_state::{
    ControllerWorkflowNextActionV1, ControllerWorkflowV1, FrozenOracleExplorationAuthorityV1,
    FrozenSirAuthorityV1, authorize_intent_admission, authorize_sir_episode,
    freeze_controller_workflow, open_oracle_exploration, record_admitted_intent,
    record_intent_decision_requests, record_sir_proposal, record_user_intent_decision,
    recover_controller_workflow,
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
    AwaitingControllerExperiment {
        experiment: cairn_migration::ProposalHostExperimentRequestV1,
    },
    IntentAdmissionBlocked {
        decision: ContentId<UserIntentDecisionArtifact>,
        reason: IntentAdmissionProcessBlockedV1,
    },
    AwaitingOracleExplorationWorkspace {
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
    },
    OracleExplorationReady {
        authority: FrozenOracleExplorationAuthorityV1,
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

/// Archives and opens the exact initial Oracle Exploration workspace and obligation ledger.
///
/// All workspace edges must already name immutable content in the Controller store. This function
/// derives the ledger itself; callers cannot supply a reduced set of planes or concerns.
///
/// # Errors
///
/// Rejects a non-admitted task, missing referenced material, policy/catalog/workspace/claim drift,
/// an uncovered obligation, or a durable transition failure.
pub fn initialize_controller_oracle_exploration(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    workspace: &OracleWorkspaceV1,
    policy: &OracleCoveragePolicyV1,
    catalog: &OracleStrategyCatalogV1,
    claims: &[OracleClaimV1],
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::AdmittedIntent { authority, .. } = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no admitted intent ready for Oracle Exploration".into(),
        ));
    };
    let mut content = open_content(server)?;
    let recovery_input: IntentRecoveryInputV1 =
        load_canonical(&content, authority.recovery_input())?;
    verify_oracle_workspace_material(&content, workspace)?;
    let policy_id = archive::<OracleCoveragePolicyArtifact, _>(&mut content, policy)?;
    let catalog_id = archive::<OracleStrategyCatalogArtifact, _>(&mut content, catalog)?;
    if workspace.coverage_policy() != policy_id || workspace.strategy_catalog() != catalog_id {
        return Err(ServerError::MigrationWorkflow(
            "Oracle workspace policy or strategy catalog identity changed".into(),
        ));
    }
    let workspace_id = archive::<OracleWorkspaceArtifact, _>(&mut content, workspace)?;
    let mut claim_ids = claims
        .iter()
        .map(|claim| archive::<OracleClaimArtifact, _>(&mut content, claim))
        .collect::<Result<Vec<_>, _>>()?;
    claim_ids.sort_by_key(ContentId::to_wire);
    let work_items = derive_oracle_work_items(&claim_ids, policy).map_err(manager_error)?;
    let ledger = OracleExplorationLedgerV1::open(workspace_id, work_items, catalog)
        .map_err(manager_error)?;
    let _ = archive::<OracleExplorationLedgerArtifact, _>(&mut content, &ledger)?;
    let mut events = open_events(server)?;
    open_oracle_exploration(
        &mut events,
        workflow,
        &recovery_input,
        workspace,
        policy,
        catalog,
        claims,
        &ledger,
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
        ControllerWorkflowNextActionV1::AwaitOracleExplorationWorkspace { outcome, contract } => {
            Ok(
                ControllerWorkflowManagerStatusV1::AwaitingOracleExplorationWorkspace {
                    outcome,
                    contract,
                },
            )
        }
        ControllerWorkflowNextActionV1::RunOracleExploration(authority) => {
            Ok(ControllerWorkflowManagerStatusV1::OracleExplorationReady { authority })
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
    let outcome = match run_proposal_host_process(proposal_host, &request).await {
        Ok(outcome) => outcome,
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
    let cairn_migration::ProposalHostOutcomeV1::Terminal { terminal } = outcome else {
        let cairn_migration::ProposalHostOutcomeV1::AwaitingController { experiment } = outcome
        else {
            unreachable!()
        };
        return Ok(ControllerWorkflowManagerStatusV1::AwaitingControllerExperiment { experiment });
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

fn verify_oracle_workspace_material(
    content: &SqliteContentStore,
    workspace: &OracleWorkspaceV1,
) -> Result<(), ServerError> {
    verify_content::<OracleSourceSnapshotArtifact>(content, workspace.source())?;
    verify_content::<OracleDocumentationSnapshotArtifact>(content, workspace.documentation())?;
    verify_content::<OracleBuildTestSnapshotArtifact>(content, workspace.build_and_tests())?;
    verify_content::<OracleKnowledgeSnapshotArtifact>(content, workspace.knowledge())?;
    verify_content::<OracleResearchToolCatalogArtifact>(content, workspace.research_tools())?;
    verify_content::<OracleExperimentToolCatalogArtifact>(content, workspace.experiment_tools())?;
    verify_content::<OracleExplorationCapabilityGrantArtifact>(
        content,
        workspace.capability_grant(),
    )
}

fn verify_content<T: ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<(), ServerError> {
    content
        .write_to(&id, &mut std::io::sink())
        .map_err(manager_error)?;
    Ok(())
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
