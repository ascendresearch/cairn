//! Readable Controller driver for the durable intent-recovery workflow prefix.
#![allow(clippy::missing_errors_doc)]

use std::io::Cursor;

use cairn_admission::{
    IntentAdmissionPublicOutcomeArtifact, MigrationIntentContractV1,
    UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1, UserIntentDecisionArtifact,
    UserIntentDecisionV1,
};
use cairn_execution::{ExecutionReceiptArtifact, JobContractArtifact};
use cairn_migration::{
    AgentRuntimeBindingArtifact, CandidateBuildPlanV1, CandidateBuildRequestArtifact,
    CandidateExplorationError, CandidateOracleContractArtifact, CandidateOracleContractV1,
    CandidateOracleElementMaterialV1, CandidateOracleMaterialV1, CandidateOracleMaterialsV1,
    CandidateWorkspaceArtifact, CandidateWorkspaceV1, IntentDecisionRequestBatchArtifact,
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    MigrationIntentContractArtifact, OracleAdmissionAttemptArtifact, OracleAdmissionAttemptV1,
    OracleAdmissionEvidenceArtifact, OracleAdmissionEvidenceV1,
    OracleAdmissionMechanismCatalogArtifact, OracleAdmissionMechanismCatalogV1,
    OracleAdmissionOutcomeArtifact, OracleAdmissionPolicyArtifact, OracleAdmissionPolicyV1,
    OracleAdversarialPolicyV1, OracleBuildTestSnapshotArtifact, OracleClaimArtifact,
    OracleComparatorProposalArtifact, OracleControlDispatchArtifact, OracleControlDispatchV1,
    OracleControlReceiptV1, OracleControlRunArtifact, OracleControlRunV1,
    OracleControlRunnerArtifact, OracleControlWorker, OracleControlWorkerError,
    OracleCoverageGapArtifact, OracleCoveragePolicyArtifact, OracleCoveragePolicyV1,
    OracleCoverageProfileV1, OracleDocumentationSnapshotArtifact,
    OracleExecutionSafetyProposalArtifact, OracleExperimentArgumentsArtifact,
    OracleExperimentRequestArtifact, OracleExperimentToolCatalogArtifact,
    OracleExplorationBudgetV1, OracleExplorationCapabilityGrantArtifact,
    OracleExplorationLedgerArtifact, OracleExplorationLedgerV1,
    OracleExplorationObservationArtifact, OracleExplorationObservationV1,
    OracleKnowledgeSnapshotArtifact, OracleMechanismQualificationReceiptV1,
    OracleObligationResolutionV1, OracleObservationPayloadArtifact, OraclePortfolioElementArtifact,
    OraclePortfolioElementKindV1, OraclePortfolioElementV1, OraclePortfolioProposalArtifact,
    OraclePortfolioProposalV1, OracleQualifiedMechanismArtifact, OracleResearchToolCatalogArtifact,
    OracleSourceSnapshotArtifact, OracleStrategyCatalogArtifact, OracleStrategyCatalogV1,
    OracleStrategyExecutorV1, OracleStrategyImplementationArtifact, OracleStrategyKindV1,
    OracleStrategyName, OracleStrategyRegistrationV1, OracleStrategyRoleV1,
    OracleStrategyRunArtifact, OracleStrategyRunV1, OracleStrategySubmissionArtifact,
    OracleStrategySubmissionOutcomeV1, OracleStrategySubmissionV1, OracleStrategyToolCatalogV1,
    OracleUnknownEvidenceArtifact, OracleWorkspaceArtifact, OracleWorkspaceInput,
    OracleWorkspaceV1, ProposalStepOracleBuildTestsV1, ProposalStepOracleDocumentationV1,
    ProposalStepOracleKnowledgeV1, ProposalStepOracleMaterialsV1, ProposalStepPublicationV1,
    ProposalStepRequestArtifact, ProposalStepRequestV1, ProposalStepRoleRequestV1,
    ProposalStepRuntimeV1, ProposalStepTaskSnapshotV1, ProposalStepTaskSourceV1,
    ProposalStepTerminalArtifact, ProposalStepTerminalV1, SirIntentHypothesisSetProposalArtifact,
    SirTaskArtifactBytes, SirTaskBundleArtifact, SirTaskBundleV1,
    TrustedOracleControlReceiptArtifact, UserIntentDecisionRequestArtifact,
    UserIntentDecisionRequestV1, WorkflowToolControllerObservationArtifact, WorkflowToolRequestV1,
    WorkflowToolWorker, derive_oracle_claims, derive_oracle_work_items,
    derive_user_intent_decision_requests, prepare_generic_candidate_build_job,
    recompute_oracle_admission,
};
use cairn_protocol::{CommandId, ContentId, ContentType, EpisodeId, JobId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::{
    CorpusCaseArtifact, DomainRefinementArtifact, ModelConfigurationArtifact,
    ObservationPlanArtifact, PropertyRelationArtifact, ReferenceArtifact,
    SourceAdmissionPlanArtifact, ValidFamilyPlanArtifact,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::controller_state::{
    ControllerWorkflowNextActionV1, ControllerWorkflowV1, FrozenCandidateAdmissionAuthorityV1,
    FrozenCandidateBuildAuthorityV1, FrozenCandidateOracleAuthorityV1,
    FrozenCandidateProposalAuthorityV1, FrozenOracleAdmissionAuthorityV1,
    FrozenOracleControlAuthorityV1, FrozenOracleExplorationAuthorityV1,
    FrozenOraclePortfolioAuthorityV1, FrozenOracleStrategyAuthorityV1, FrozenSirAuthorityV1,
    IntentAdmissionBlockReasonV1, OracleStrategyCompletionV1, authorize_candidate_build,
    authorize_candidate_proposal_episode, authorize_intent_admission, authorize_oracle_admission,
    authorize_oracle_control, authorize_oracle_strategy, authorize_sir_episode,
    freeze_candidate_build, freeze_candidate_oracle_contract, freeze_candidate_proposal_request,
    freeze_controller_workflow, freeze_oracle_portfolio, open_oracle_exploration,
    reauthorize_intent_admission, record_admitted_intent, record_candidate_proposal,
    record_intent_admission_blocked, record_intent_decision_requests,
    record_oracle_admission_outcome, record_oracle_control_observation,
    record_oracle_strategy_completion, record_oracle_strategy_observations, record_sir_proposal,
    record_user_intent_decision, recover_controller_workflow,
};
use crate::intent_admission_supervisor::{
    IntentAdmissionProcessBlockedV1, IntentAdmissionProcessConfigV1, run_intent_admission_process,
};
use crate::proposal_step_runner::{
    ProposalStepConfigV1, execute_controller_workflow_tools, run_proposal_step,
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
    WorkerRequest {
        request: cairn_migration::WorkflowToolRequestV1,
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
    OracleStrategyReady {
        authority: FrozenOracleStrategyAuthorityV1,
    },
    AwaitingOracleExperiment {
        ledger: ContentId<OracleExplorationLedgerArtifact>,
        request: ContentId<OracleExperimentRequestArtifact>,
    },
    AwaitingOracleAdmissionMechanisms {
        authority: FrozenOraclePortfolioAuthorityV1,
    },
    OracleControlBlocked {
        run: ContentId<OracleControlRunArtifact>,
        reason: OracleControlWorkerError,
    },
    OracleAdmissionCompleted {
        outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    },
    OracleAdmissionHasNoCandidateAuthority {
        outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    },
    AwaitingCandidateProposalLoop {
        authority: FrozenCandidateOracleAuthorityV1,
    },
    AwaitingCandidateBuild {
        authority: FrozenCandidateProposalAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<cairn_migration::CandidateProposalArtifact>,
    },
    CandidateBuildReady {
        authority: FrozenCandidateBuildAuthorityV1,
    },
    AwaitingCandidateAdmissionMechanisms {
        authority: FrozenCandidateBuildAuthorityV1,
        receipt: ContentId<cairn_execution::ExecutionReceiptArtifact>,
        outcome: cairn_execution::ExecutionOutcome,
    },
    AwaitingCandidateControlReceipts {
        authority: FrozenCandidateAdmissionAuthorityV1,
    },
    Terminal {
        outcome: ContentId<cairn_migration::CandidateAdmissionOutcomeArtifact>,
        status: crate::MigrationTerminalStatusV1,
    },
    OracleExplorationBudgetExhausted {
        ledger: ContentId<OracleExplorationLedgerArtifact>,
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
    request: &ProposalStepRequestV1,
    command_id: &CommandId,
    observed_at: cairn_protocol::ObservedAtUnixMillis,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let recovery_input = request.sir_recovery_input().map_err(manager_error)?;
    let mut content = open_content(server)?;
    let ProposalStepRoleRequestV1::Sir { task, .. } = request.role() else {
        return Err(ServerError::MigrationWorkflow(
            "Controller intake request is not an SIR role".into(),
        ));
    };
    for (source, artifact) in task.sources().iter().zip(task.bundle().artifacts()) {
        let descriptor = content
            .put::<SirTaskArtifactBytes>(&mut Cursor::new(source.source().as_bytes()))
            .map_err(manager_error)?;
        if source.path() != artifact.path() || descriptor.content_id != artifact.identity() {
            return Err(ServerError::MigrationWorkflow(
                "task source changed while archiving the submitted snapshot".into(),
            ));
        }
    }
    let bundle_id = archive::<SirTaskBundleArtifact, _>(&mut content, task.bundle())?;
    if bundle_id != recovery_input.task_bundle() {
        return Err(ServerError::MigrationWorkflow(
            "task bundle changed while archiving the submitted snapshot".into(),
        ));
    }
    let request_id = archive::<ProposalStepRequestArtifact, _>(&mut content, request)?;
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
        command_id,
        observed_at,
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
    command_id: &CommandId,
    observed_at: cairn_protocol::ObservedAtUnixMillis,
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
        command_id,
        observed_at,
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
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::AdmittedIntent {
        authority,
        contract,
        contract_body,
        ..
    } = state
    else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no admitted intent ready for Oracle Exploration".into(),
        ));
    };
    let mut content = open_content(server)?;
    let recovery_input: IntentRecoveryInputV1 =
        load_canonical(&content, authority.recovery_input())?;
    verify_oracle_workspace_material(&content, workspace, catalog)?;
    let policy_id = archive::<OracleCoveragePolicyArtifact, _>(&mut content, policy)?;
    let catalog_id = archive::<OracleStrategyCatalogArtifact, _>(&mut content, catalog)?;
    if workspace.coverage_policy() != policy_id || workspace.strategy_catalog() != catalog_id {
        return Err(ServerError::MigrationWorkflow(
            "Oracle workspace policy or strategy catalog identity changed".into(),
        ));
    }
    let workspace_id = archive::<OracleWorkspaceArtifact, _>(&mut content, workspace)?;
    let claims = derive_oracle_claims(workflow.task_id(), contract, contract_body.admitted_claim());
    let claim_ids = claims
        .iter()
        .map(|claim| archive::<OracleClaimArtifact, _>(&mut content, claim))
        .collect::<Result<Vec<_>, _>>()?;
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
        &ledger,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Constructs and opens the product-owned generic Oracle workspace for an admitted task.
///
/// The catalog assigns one fresh Proposal step episode to each mandatory concern, avoiding shared
/// episode state across independent cells. No task-specific knowledge or fixture rule is added.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "workspace construction validates one closed set of strongly typed Oracle inputs"
)]
pub fn initialize_product_oracle_exploration(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    workflow: &ControllerWorkflowV1,
    coverage_profile: OracleCoverageProfileV1,
    adversarial_policy: OracleAdversarialPolicyV1,
    budget: OracleExplorationBudgetV1,
    documentation: &str,
    build_and_tests: &str,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::AdmittedIntent {
        authority,
        contract,
        ..
    } = state
    else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no admitted intent ready for product Oracle initialization".into(),
        ));
    };
    let mut content = open_content(server)?;
    let recovery_input: IntentRecoveryInputV1 =
        load_canonical(&content, authority.recovery_input())?;
    let policy = OracleCoveragePolicyV1::new(coverage_profile, adversarial_policy);
    let required_roles = if adversarial_policy == OracleAdversarialPolicyV1::RequiredForEveryConcern
    {
        2_usize
    } else {
        1_usize
    };
    let required_runs = policy
        .concerns()
        .len()
        .checked_mul(required_roles)
        .ok_or_else(|| ServerError::MigrationWorkflow("Oracle run budget overflow".into()))?;
    if usize::try_from(budget.strategy_runs.get()).map_err(manager_error)? < required_runs {
        return Err(ServerError::Configuration(
            "Oracle strategy run budget cannot cover every mandatory concern and role".into(),
        ));
    }

    let model_bytes =
        std::fs::read(&proposal_step.resolved_runtime_model).map_err(manager_error)?;
    let authorship_model = archive_bytes::<ModelConfigurationArtifact>(&mut content, &model_bytes)?;
    let strategy_tools = OracleStrategyToolCatalogV1::standard();
    let strategy_tools_id = archive::<cairn_migration::OracleStrategyToolCatalogArtifact, _>(
        &mut content,
        &strategy_tools,
    )?;
    let mut strategies = Vec::new();
    for (index, concern) in policy.concerns().iter().enumerate() {
        let synthesis_runtime = proposal_step.runtime(EpisodeId::new())?;
        let synthesis_invocation =
            archive::<AgentRuntimeBindingArtifact, _>(&mut content, &synthesis_runtime)?;
        strategies.push(
            OracleStrategyRegistrationV1::new(
                OracleStrategyName::new(format!("model-synthesis-{index:02}"))
                    .map_err(manager_error)?,
                OracleStrategyKindV1::ModelBackedSynthesis,
                OracleStrategyExecutorV1::AgentStep {
                    authorship_model,
                    invocation: synthesis_invocation,
                    tools: strategy_tools_id,
                },
                vec![OracleStrategyRoleV1::Synthesis],
                vec![*concern],
            )
            .map_err(manager_error)?,
        );
        if adversarial_policy == OracleAdversarialPolicyV1::RequiredForEveryConcern {
            let adversarial_runtime = proposal_step.runtime(EpisodeId::new())?;
            let adversarial_invocation =
                archive::<AgentRuntimeBindingArtifact, _>(&mut content, &adversarial_runtime)?;
            strategies.push(
                OracleStrategyRegistrationV1::new(
                    OracleStrategyName::new(format!("model-adversarial-{index:02}"))
                        .map_err(manager_error)?,
                    OracleStrategyKindV1::ModelBackedAdversarial,
                    OracleStrategyExecutorV1::AgentStep {
                        authorship_model,
                        invocation: adversarial_invocation,
                        tools: strategy_tools_id,
                    },
                    vec![OracleStrategyRoleV1::Adversarial],
                    vec![*concern],
                )
                .map_err(manager_error)?,
            );
        }
    }
    strategies.sort_by(|left, right| left.name().cmp(right.name()));
    let catalog = OracleStrategyCatalogV1::new(strategies).map_err(manager_error)?;
    let policy_id = archive::<OracleCoveragePolicyArtifact, _>(&mut content, &policy)?;
    let catalog_id = archive::<OracleStrategyCatalogArtifact, _>(&mut content, &catalog)?;
    let source_manifest = format!("submitted-task-bundle:{}", recovery_input.task_bundle());
    let source =
        archive_bytes::<OracleSourceSnapshotArtifact>(&mut content, source_manifest.as_bytes())?;
    let documentation = archive_bytes::<OracleDocumentationSnapshotArtifact>(
        &mut content,
        documentation.as_bytes(),
    )?;
    let build_and_tests =
        archive_bytes::<OracleBuildTestSnapshotArtifact>(&mut content, build_and_tests.as_bytes())?;
    let knowledge = archive_bytes::<OracleKnowledgeSnapshotArtifact>(
        &mut content,
        b"No task-specific knowledge base is configured for this run.",
    )?;
    let research_tools = archive_bytes::<OracleResearchToolCatalogArtifact>(
        &mut content,
        b"{\"schema_version\":1,\"tools\":[]}",
    )?;
    let experiment_tools = archive_bytes::<OracleExperimentToolCatalogArtifact>(
        &mut content,
        b"{\"schema_version\":1,\"tools\":[]}",
    )?;
    let capability_grant = archive_bytes::<OracleExplorationCapabilityGrantArtifact>(
        &mut content,
        b"{\"capabilities\":[\"read-task-artifact\",\"submit-cell-result\"],\"schema_version\":1}",
    )?;
    let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
        task_id: workflow.task_id(),
        admitted_intent: contract,
        sir_input: authority.recovery_input(),
        sir_task_bundle: recovery_input.task_bundle(),
        source,
        documentation,
        build_and_tests,
        knowledge,
        research_tools,
        experiment_tools,
        capability_grant,
        coverage_policy: policy_id,
        strategy_catalog: catalog_id,
        budget,
    });
    drop(content);
    initialize_controller_oracle_exploration(server, workflow, &workspace, &policy, &catalog)
}

/// Freezes qualified independent-control mechanisms before any Oracle Admission control runs.
///
/// Mechanism registrations are typed by control family and must reference material already
/// archived in the Controller store. The attempt and its full item × control obligation matrix
/// are derived mechanically; callers cannot omit a plane or control.
///
/// # Errors
///
/// Rejects missing mechanism material, a noncanonical catalog, portfolio/policy drift, or a
/// durable transition failure.
pub fn authorize_controller_oracle_admission(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    mechanisms: &OracleAdmissionMechanismCatalogV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::OraclePortfolioFrozen(authority) = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no frozen Oracle portfolio ready for Admission".into(),
        ));
    };
    let mut content = open_content(server)?;
    let proposal: OraclePortfolioProposalV1 = load_canonical(&content, authority.proposal())?;
    let policy: OracleAdmissionPolicyV1 = load_canonical(&content, authority.policy())?;
    for registration in mechanisms.mechanisms() {
        verify_content::<OracleQualifiedMechanismArtifact>(&content, registration.mechanism())?;
        verify_content::<OracleControlRunnerArtifact>(&content, registration.runner())?;
        let qualification: OracleMechanismQualificationReceiptV1 =
            load_canonical(&content, registration.qualification())?;
        registration
            .validate_qualification(&qualification)
            .map_err(manager_error)?;
        verify_content::<ExecutionReceiptArtifact>(&content, qualification.evidence())?;
    }
    let _ = archive::<OracleAdmissionMechanismCatalogArtifact, _>(&mut content, mechanisms)?;
    let attempt =
        OracleAdmissionAttemptV1::new(&proposal, &policy, mechanisms).map_err(manager_error)?;
    let _ = archive::<OracleAdmissionAttemptArtifact, _>(&mut content, &attempt)?;
    let mut events = open_events(server)?;
    authorize_oracle_admission(
        &mut events,
        workflow,
        &proposal,
        &policy,
        mechanisms,
        &attempt,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Archives trusted control receipts and independently recomputes the terminal claim portfolio.
///
/// Receipt absence remains explicit and produces a partial result. Receipt references must exist
/// in the Controller store and must match the exact frozen item × control × mechanism obligation.
/// No model verdict participates in the recomputation.
///
/// # Errors
///
/// Rejects missing trusted receipt material, cross-attempt evidence, duplicate controls,
/// noncanonical identities, or a durable transition failure.
fn finalize_controller_oracle_admission(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    receipts: Vec<OracleControlReceiptV1>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::OracleControlsObserved {
        authority,
        receipts: observed,
    } = state
    else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no complete Oracle control observations".into(),
        ));
    };
    if receipts != observed {
        return Err(ServerError::MigrationWorkflow(
            "Oracle Admission evidence differs from durable control observations".into(),
        ));
    }
    let mut content = open_content(server)?;
    let proposal: OraclePortfolioProposalV1 =
        load_canonical(&content, authority.portfolio().proposal())?;
    let policy: OracleAdmissionPolicyV1 = load_canonical(&content, authority.portfolio().policy())?;
    let mechanisms: OracleAdmissionMechanismCatalogV1 =
        load_canonical(&content, authority.mechanisms())?;
    let attempt: OracleAdmissionAttemptV1 = load_canonical(&content, authority.attempt())?;
    for receipt in &receipts {
        verify_content::<TrustedOracleControlReceiptArtifact>(&content, receipt.receipt())?;
    }
    let evidence = OracleAdmissionEvidenceV1::new(&attempt, receipts).map_err(manager_error)?;
    let _ = archive::<OracleAdmissionEvidenceArtifact, _>(&mut content, &evidence)?;
    let outcome = recompute_oracle_admission(&proposal, &policy, &mechanisms, &attempt, &evidence)
        .map_err(manager_error)?;
    let outcome_id = archive::<OracleAdmissionOutcomeArtifact, _>(&mut content, &outcome)?;
    let mut events = open_events(server)?;
    record_oracle_admission_outcome(
        &mut events,
        workflow,
        &proposal,
        &policy,
        &mechanisms,
        &attempt,
        &evidence,
        &outcome,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(
        ControllerWorkflowManagerStatusV1::OracleAdmissionCompleted {
            outcome: outcome_id,
        },
    )
}

fn prepare_or_finalize_oracle_controls<W: OracleControlWorker>(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenOracleAdmissionAuthorityV1,
    receipts: Vec<OracleControlReceiptV1>,
    worker: &mut W,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let mechanisms: OracleAdmissionMechanismCatalogV1 =
        load_canonical(&content, authority.mechanisms())?;
    let attempt: OracleAdmissionAttemptV1 = load_canonical(&content, authority.attempt())?;
    let _ = OracleAdmissionEvidenceV1::new(&attempt, receipts.clone()).map_err(manager_error)?;
    let next = attempt.required_controls().iter().find(|obligation| {
        !receipts.iter().any(|receipt| {
            receipt.item() == obligation.item() && receipt.control() == obligation.control()
        })
    });
    let Some(obligation) = next else {
        drop(content);
        return finalize_controller_oracle_admission(server, workflow, receipts);
    };
    let run = OracleControlRunV1::new(&attempt, &mechanisms, obligation.clone())
        .map_err(manager_error)?;
    let run_id = archive::<OracleControlRunArtifact, _>(&mut content, &run)?;
    let runner_id = run.runner();
    let binding = match worker.prepare(&run) {
        Ok(binding) => binding,
        Err(reason) => {
            let classification = oracle_control_worker_error_class(&reason);
            tracing::warn!(
                target: "cairn.server.controller-workflow",
                event = "oracle_control_prepare_blocked",
                task_id = %workflow.task_id(),
                run_id = %run_id,
                runner_id = %runner_id,
                classification,
                "qualified Oracle control preparation is blocked"
            );
            return Ok(ControllerWorkflowManagerStatusV1::OracleControlBlocked {
                run: run_id,
                reason,
            });
        }
    };
    verify_content::<JobContractArtifact>(&content, binding.contract())?;
    let dispatch = OracleControlDispatchV1::new(&run, binding).map_err(manager_error)?;
    let _ = archive::<OracleControlDispatchArtifact, _>(&mut content, &dispatch)?;
    let mut events = open_events(server)?;
    authorize_oracle_control(
        &mut events,
        workflow,
        &mechanisms,
        &attempt,
        &receipts,
        &run,
        &dispatch,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

#[allow(
    clippy::too_many_lines,
    reason = "the exact load, effect, observation archival, durable commit, and safe terminal log remain visible in one control step"
)]
fn execute_authorized_oracle_control<W: OracleControlWorker>(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenOracleControlAuthorityV1,
    previous_receipts: &[OracleControlReceiptV1],
    worker: &mut W,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let run: OracleControlRunV1 = load_canonical(&content, authority.run())?;
    let dispatch: OracleControlDispatchV1 = load_canonical(&content, authority.dispatch())?;
    if dispatch.run() != authority.run() || dispatch.runner() != run.runner() {
        return Err(ServerError::MigrationWorkflow(
            "Oracle control runner or dispatch changed after durable authorization".into(),
        ));
    }
    let job_id = dispatch.worker().job_id();
    let attempt_id = dispatch.worker().attempt_id();
    let contract_id = dispatch.worker().contract();
    let runner_id = dispatch.runner();
    tracing::info!(
        target: "cairn.server.controller-workflow",
        event = "oracle_control_execution_started",
        task_id = %workflow.task_id(),
        run_id = %authority.run(),
        runner_id = %runner_id,
        job_id = %job_id,
        attempt_id = %attempt_id,
        contract_id = %contract_id,
        "qualified Oracle control execution started"
    );
    let observation = match worker.execute(&dispatch) {
        Ok(observation) => observation,
        Err(reason) => {
            let classification = oracle_control_worker_error_class(&reason);
            tracing::warn!(
                target: "cairn.server.controller-workflow",
                event = "oracle_control_execution_blocked",
                task_id = %workflow.task_id(),
                run_id = %authority.run(),
                runner_id = %runner_id,
                job_id = %job_id,
                attempt_id = %attempt_id,
                contract_id = %contract_id,
                classification,
                "qualified Oracle control execution requires reconciliation"
            );
            return Ok(ControllerWorkflowManagerStatusV1::OracleControlBlocked {
                run: authority.run(),
                reason,
            });
        }
    };
    observation
        .validate_against(&dispatch)
        .map_err(manager_error)?;
    let archived_receipt =
        archive::<ExecutionReceiptArtifact, _>(&mut content, observation.receipt())?;
    if archived_receipt != observation.receipt_id() {
        return Err(ServerError::MigrationWorkflow(
            "Oracle control Worker receipt identity changed during archival".into(),
        ));
    }
    let observation_id =
        archive::<TrustedOracleControlReceiptArtifact, _>(&mut content, &observation)?;
    let receipt = OracleControlReceiptV1::from_trusted_observation(
        authority.admission().portfolio().proposal(),
        &run,
        &observation,
    )
    .map_err(manager_error)?;
    if receipt.receipt() != observation_id {
        return Err(ServerError::MigrationWorkflow(
            "Oracle control trusted observation identity changed during projection".into(),
        ));
    }
    let result = observation.result();
    let elapsed_ms = observation.receipt().elapsed().get();
    let output_count = observation.receipt().outputs().len();
    let exit_code = observation.receipt().exit_code();
    let receipt_id = observation.receipt_id();
    let mut events = open_events(server)?;
    record_oracle_control_observation(
        &mut events,
        workflow,
        &run,
        &dispatch,
        &observation,
        &receipt,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    tracing::info!(
        target: "cairn.server.controller-workflow",
        event = "oracle_control_execution_completed",
        task_id = %workflow.task_id(),
        run_id = %authority.run(),
        runner_id = %runner_id,
        job_id = %job_id,
        attempt_id = %attempt_id,
        contract_id = %contract_id,
        receipt_id = %receipt_id,
        result = ?result,
        exit_code,
        elapsed_ms,
        output_count,
        prior_receipt_count = previous_receipts.len(),
        "qualified Oracle control execution completed"
    );
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

const fn oracle_control_worker_error_class(error: &OracleControlWorkerError) -> &'static str {
    match error {
        OracleControlWorkerError::NotStarted(_) => "not-started",
        OracleControlWorkerError::Rejected(_) => "rejected",
        OracleControlWorkerError::Ambiguous(_) => "ambiguous",
    }
}

/// Archives one strict cell-scoped strategy submission and advances the exact ledger revision.
///
/// # Errors
///
/// Rejects a non-authorized run, missing or cross-cell material, noncanonical identities, ledger
/// lineage drift, or a durable transition failure.
pub fn record_controller_oracle_strategy_submission(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    submission: &OracleStrategySubmissionV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::OracleStrategyAuthorized(authority) = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no authorized Oracle strategy run".into(),
        ));
    };
    let content = open_content(server)?;
    let run: OracleStrategyRunV1 = load_canonical(&content, authority.run())?;
    let OracleStrategyExecutorV1::Deterministic { implementation } = run.executor() else {
        return Err(ServerError::MigrationWorkflow(
            "Agent-backed Oracle strategy requires a Proposal step terminal".into(),
        ));
    };
    let completion = OracleStrategyCompletionV1::Deterministic {
        implementation: *implementation,
        submission: submission.clone(),
    };
    record_controller_oracle_completion(server, workflow, &authority, &completion)
}

/// Records one exact Proposal step terminal as the durable completion of an Agent strategy run.
///
/// # Errors
///
/// Rejects request, invocation, terminal, run, work-item, submission, CAS, or ledger drift.
pub fn record_controller_oracle_strategy_terminal(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    request: &ProposalStepRequestV1,
    terminal: &ProposalStepTerminalV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::OracleStrategyAuthorized(authority) = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no authorized Oracle strategy run".into(),
        ));
    };
    let request_id = request.identity().map_err(manager_error)?;
    let terminal_id = terminal.identity().map_err(manager_error)?;
    let completion = OracleStrategyCompletionV1::AgentStep {
        request_id,
        request: Box::new(request.clone()),
        terminal_id,
        terminal: Box::new(terminal.clone()),
    };
    record_controller_oracle_completion(server, workflow, &authority, &completion)
}

/// Dispatches one Oracle proposal-step Worker request and projects its typed observations.
///
/// The generic Agent runtime state grants start authority before the Worker adapter is invoked. This
/// wrapper then archives the Controller observation, Oracle payload, and exact run-bound Oracle
/// observation in the task Controller store before the same Agent episode may resume.
///
/// # Errors
///
/// Rejects non-Oracle roles, request/run/ledger drift, missing projection bodies, Worker failures,
/// or any Agent/Controller persistence failure.
#[allow(
    clippy::too_many_lines,
    reason = "the Worker round trip validates and archives each authority-bearing lineage edge"
)]
pub fn dispatch_controller_oracle_worker_request<W: WorkflowToolWorker>(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    proposal_request: &ProposalStepRequestV1,
    worker_request: &WorkflowToolRequestV1,
    worker: &mut W,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::OracleStrategyAuthorized(authority) = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller has no authorized Oracle strategy effect".into(),
        ));
    };
    let ProposalStepRoleRequestV1::OracleStrategy { run: requested, .. } = proposal_request.role()
    else {
        return Err(ServerError::MigrationWorkflow(
            "Controller effect request is not an Oracle strategy".into(),
        ));
    };
    let content = open_content(server)?;
    let run: OracleStrategyRunV1 = load_canonical(&content, authority.run())?;
    let ledger: OracleExplorationLedgerV1 =
        load_canonical(&content, authority.exploration().ledger())?;
    if requested != &run
        || worker_request.request() != proposal_request.identity().map_err(manager_error)?
    {
        return Err(ServerError::MigrationWorkflow(
            "Oracle proposal effect changed its authorized request or run".into(),
        ));
    }
    drop(content);
    let mut events = open_events(server)?;
    let mut content = open_content(server)?;
    let executed = execute_controller_workflow_tools(
        &mut events,
        &mut content,
        proposal_request,
        worker_request,
        worker,
    )?;
    let mut observations = Vec::with_capacity(executed.len());
    for value in executed {
        let controller_id = archive::<WorkflowToolControllerObservationArtifact, _>(
            &mut content,
            value.controller(),
        )?;
        if controller_id != value.controller().identity().map_err(manager_error)? {
            return Err(ServerError::MigrationWorkflow(
                "Oracle Controller observation identity changed".into(),
            ));
        }
        let payload = value.oracle_payload().ok_or_else(|| {
            ServerError::MigrationWorkflow("Oracle effect has no Oracle payload projection".into())
        })?;
        let observation = value.oracle_observation().ok_or_else(|| {
            ServerError::MigrationWorkflow(
                "Oracle effect has no run-bound Oracle observation".into(),
            )
        })?;
        let _ = archive::<OracleObservationPayloadArtifact, _>(&mut content, payload)?;
        let _ = archive::<OracleExplorationObservationArtifact, _>(&mut content, observation)?;
        observations.push(observation.clone());
    }
    let run_id = run.identity().map_err(manager_error)?;
    let observation_ids = observations
        .iter()
        .map(OracleExplorationObservationV1::identity)
        .collect::<Result<Vec<_>, _>>()
        .map_err(manager_error)?;
    let existing = ledger
        .entries()
        .iter()
        .find(|entry| entry.item().identity().is_ok_and(|id| id == run.item()))
        .and_then(|entry| match entry.resolution() {
            OracleObligationResolutionV1::Running {
                run: active,
                observations,
                ..
            } if *active == run_id => Some(observations),
            _ => None,
        })
        .ok_or_else(|| {
            ServerError::MigrationWorkflow("Oracle effect has no active run ledger".into())
        })?;
    if observation_ids.iter().all(|id| existing.contains(id)) {
        return Ok(ControllerWorkflowManagerStatusV1::Advanced);
    }
    if observation_ids.iter().any(|id| existing.contains(id)) {
        return Err(ServerError::MigrationWorkflow(
            "Oracle effect observation batch is only partially projected".into(),
        ));
    }
    let next_ledger = ledger
        .record_strategy_observations(run.item(), run_id, &observations)
        .map_err(manager_error)?;
    let _ = archive::<OracleExplorationLedgerArtifact, _>(&mut content, &next_ledger)?;
    let mut events = open_events(server)?;
    record_oracle_strategy_observations(
        &mut events,
        workflow,
        &ledger,
        &run,
        &observations,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn record_controller_oracle_completion(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenOracleStrategyAuthorityV1,
    completion: &OracleStrategyCompletionV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let workspace: OracleWorkspaceV1 =
        load_canonical(&content, authority.exploration().workspace())?;
    let ledger: OracleExplorationLedgerV1 =
        load_canonical(&content, authority.exploration().ledger())?;
    let run: OracleStrategyRunV1 = load_canonical(&content, authority.run())?;
    let submission = match completion {
        OracleStrategyCompletionV1::Deterministic { submission, .. } => submission,
        OracleStrategyCompletionV1::AgentStep {
            request_id,
            request,
            terminal_id,
            terminal,
        } => {
            let archived_request =
                archive::<ProposalStepRequestArtifact, _>(&mut content, request)?;
            let archived_terminal =
                archive::<ProposalStepTerminalArtifact, _>(&mut content, terminal)?;
            if archived_request != *request_id || archived_terminal != *terminal_id {
                return Err(ServerError::MigrationWorkflow(
                    "Oracle Proposal step completion identity changed".into(),
                ));
            }
            let ProposalStepPublicationV1::OracleStrategy { submission, .. } =
                terminal.publication()
            else {
                return Err(ServerError::MigrationWorkflow(
                    "Oracle Proposal step returned a non-Oracle publication".into(),
                ));
            };
            submission
        }
    };
    archive_oracle_submission_material(&mut content, &run, submission)?;
    let _ = archive::<OracleStrategySubmissionArtifact, _>(&mut content, submission)?;
    let next_ledger = ledger
        .apply_strategy_submission(&run, submission, &workspace)
        .map_err(manager_error)?;
    let _ = archive::<OracleExplorationLedgerArtifact, _>(&mut content, &next_ledger)?;
    let mut events = open_events(server)?;
    record_oracle_strategy_completion(
        &mut events,
        workflow,
        &workspace,
        &ledger,
        &run,
        completion,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Materializes one exact Agent-backed Oracle cell as a generic Proposal step request.
///
/// The request carries the structured admitted claim plus exactly one claim × concern × role work
/// item. It performs no model, network, or Worker effect.
///
/// # Errors
///
/// Rejects a non-Agent executor, invocation/run/cell drift, or missing/corrupt task material.
pub fn prepare_oracle_strategy_proposal_step_request(
    server: &ServerConfig,
    authority: &FrozenOracleStrategyAuthorityV1,
) -> Result<ProposalStepRequestV1, ServerError> {
    let content = open_content(server)?;
    let workspace: OracleWorkspaceV1 =
        load_canonical(&content, authority.exploration().workspace())?;
    let ledger: OracleExplorationLedgerV1 =
        load_canonical(&content, authority.exploration().ledger())?;
    let run: OracleStrategyRunV1 = load_canonical(&content, authority.run())?;
    let item = ledger
        .entries()
        .iter()
        .find(|entry| entry.item().identity().is_ok_and(|id| id == run.item()))
        .map(|entry| entry.item().clone())
        .ok_or_else(|| {
            ServerError::MigrationWorkflow("Oracle strategy work item is absent".into())
        })?;
    let claim = authority
        .exploration()
        .claims()
        .iter()
        .find(|claim| claim.identity().is_ok_and(|id| id == item.claim()))
        .cloned()
        .ok_or_else(|| ServerError::MigrationWorkflow("Oracle strategy claim is absent".into()))?;
    let OracleStrategyExecutorV1::AgentStep { invocation, .. } = run.executor() else {
        return Err(ServerError::MigrationWorkflow(
            "Oracle strategy is not assigned to Proposal step".into(),
        ));
    };
    let runtime: ProposalStepRuntimeV1 = load_canonical(&content, *invocation)?;
    let bundle: SirTaskBundleV1 = load_canonical(&content, workspace.sir_task_bundle())?;
    let materials = ProposalStepOracleMaterialsV1::new(
        ProposalStepOracleDocumentationV1::new(
            workspace.documentation(),
            load_utf8_content(&content, workspace.documentation())?,
        )
        .map_err(manager_error)?,
        ProposalStepOracleBuildTestsV1::new(
            workspace.build_and_tests(),
            load_utf8_content(&content, workspace.build_and_tests())?,
        )
        .map_err(manager_error)?,
        ProposalStepOracleKnowledgeV1::new(
            workspace.knowledge(),
            load_utf8_content(&content, workspace.knowledge())?,
        )
        .map_err(manager_error)?,
    );
    let mut sources = Vec::with_capacity(bundle.artifacts().len());
    for artifact in bundle.artifacts() {
        let mut bytes = Vec::new();
        content
            .write_to(&artifact.identity(), &mut bytes)
            .map_err(manager_error)?;
        sources.push(ProposalStepTaskSourceV1::new(
            artifact.path().clone(),
            String::from_utf8(bytes).map_err(manager_error)?,
        ));
    }
    ProposalStepRequestV1::new(
        runtime,
        ProposalStepRoleRequestV1::OracleStrategy {
            workspace,
            claim,
            work_item: item,
            run,
            task: ProposalStepTaskSnapshotV1::new(bundle, sources),
            materials,
        },
    )
    .map_err(manager_error)
}

/// Materializes one exact admitted Candidate authority as a generic Proposal step request.
///
/// The caller supplies the already frozen runtime/model/budget selection. This function loads
/// only public artifacts reachable from the Candidate workspace and admitted Oracle contract;
/// restricted Admission controls and rejected/partial claims have no path into the request.
///
/// # Errors
///
/// Rejects runtime/task/workspace drift, absent portfolio element bodies, a typed material/body
/// mismatch, or any admitted-contract inconsistency.
pub fn prepare_candidate_strategy_proposal_step_request(
    server: &ServerConfig,
    authority: &FrozenCandidateOracleAuthorityV1,
    runtime: ProposalStepRuntimeV1,
) -> Result<ProposalStepRequestV1, ServerError> {
    let content = open_content(server)?;
    let workspace: CandidateWorkspaceV1 = load_canonical(&content, authority.workspace())?;
    let contract: CandidateOracleContractV1 = load_canonical(&content, authority.contract())?;
    let claims = authority
        .oracle()
        .portfolio()
        .exploration()
        .claims()
        .iter()
        .filter(|claim| {
            claim.identity().is_ok_and(|identity| {
                contract
                    .admitted_claims()
                    .iter()
                    .any(|admitted| admitted.claim() == identity)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut elements = Vec::new();
    for claim in contract.admitted_claims() {
        for entry in claim.entries() {
            let OracleObligationResolutionV1::Contributed {
                elements: admitted, ..
            } = entry.resolution()
            else {
                return Err(ServerError::MigrationWorkflow(
                    "admitted Candidate Oracle entry is not a positive contribution".into(),
                ));
            };
            for element_id in admitted {
                let element: OraclePortfolioElementV1 = load_canonical(&content, *element_id)?;
                let bytes = load_candidate_oracle_material(&content, element.kind())?;
                let material =
                    CandidateOracleMaterialV1::from_portfolio_kind(element.kind(), bytes)
                        .map_err(manager_error)?;
                elements.push(
                    CandidateOracleElementMaterialV1::new(element, material)
                        .map_err(manager_error)?,
                );
            }
        }
    }
    let oracle_materials =
        CandidateOracleMaterialsV1::new(&contract, claims, elements).map_err(manager_error)?;
    let public_materials = ProposalStepOracleMaterialsV1::new(
        ProposalStepOracleDocumentationV1::new(
            workspace.documentation(),
            load_utf8_content(&content, workspace.documentation())?,
        )
        .map_err(manager_error)?,
        ProposalStepOracleBuildTestsV1::new(
            workspace.build_and_tests(),
            load_utf8_content(&content, workspace.build_and_tests())?,
        )
        .map_err(manager_error)?,
        ProposalStepOracleKnowledgeV1::new(
            workspace.knowledge(),
            load_utf8_content(&content, workspace.knowledge())?,
        )
        .map_err(manager_error)?,
    );
    let bundle: SirTaskBundleV1 = load_canonical(&content, workspace.task_bundle())?;
    let task = materialize_task_snapshot(&content, bundle)?;
    ProposalStepRequestV1::new(
        runtime,
        ProposalStepRoleRequestV1::CandidateStrategy {
            workspace,
            contract,
            oracle_materials,
            task,
            public_materials,
        },
    )
    .map_err(manager_error)
}

/// Freezes the exact Candidate runtime and all admitted public material into the task aggregate.
///
/// This step performs no model effect. The following manager turn separately commits start
/// authority before the proposal step can run.
///
/// # Errors
///
/// Rejects missing/corrupt admitted material, runtime/request drift, storage failure, or an
/// illegal durable Controller transition.
pub fn initialize_candidate_proposal_loop(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenCandidateOracleAuthorityV1,
    runtime: ProposalStepRuntimeV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let request = prepare_candidate_strategy_proposal_step_request(server, authority, runtime)?;
    let mut content = open_content(server)?;
    let request_id = archive::<ProposalStepRequestArtifact, _>(&mut content, &request)?;
    let mut events = open_events(server)?;
    freeze_candidate_proposal_request(
        &mut events,
        workflow,
        request_id,
        &request,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

/// Materializes and freezes one exact product-owned build plan for the recorded Candidate.
///
/// No Worker effect occurs here; the following aggregate turn commits start authority first.
pub fn initialize_candidate_build(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenCandidateProposalAuthorityV1,
    proposal_id: ContentId<cairn_migration::CandidateProposalArtifact>,
    plan: CandidateBuildPlanV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let proposal_bytes =
        read_content::<cairn_migration::CandidateProposalArtifact>(&content, proposal_id)?;
    let proposal: cairn_migration::CandidateProposalV1 =
        cairn_codec::from_slice(&proposal_bytes).map_err(manager_error)?;
    if proposal.oracle_contract() != authority.candidate().contract() {
        return Err(ServerError::MigrationWorkflow(
            "Candidate build proposal is outside frozen Oracle authority".into(),
        ));
    }
    let prepared =
        prepare_generic_candidate_build_job(JobId::new(), &proposal_bytes, proposal_id, plan)
            .map_err(manager_error)?;
    prepared.archive(&mut content).map_err(manager_error)?;
    let _ = archive::<CandidateBuildRequestArtifact, _>(&mut content, prepared.request())?;
    let mut events = open_events(server)?;
    freeze_candidate_build(
        &mut events,
        workflow,
        &proposal,
        prepared.plan(),
        prepared.request(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn materialize_task_snapshot(
    content: &SqliteContentStore,
    bundle: SirTaskBundleV1,
) -> Result<ProposalStepTaskSnapshotV1, ServerError> {
    let mut sources = Vec::with_capacity(bundle.artifacts().len());
    for artifact in bundle.artifacts() {
        let mut bytes = Vec::new();
        content
            .write_to(&artifact.identity(), &mut bytes)
            .map_err(manager_error)?;
        sources.push(ProposalStepTaskSourceV1::new(
            artifact.path().clone(),
            String::from_utf8(bytes).map_err(manager_error)?,
        ));
    }
    Ok(ProposalStepTaskSnapshotV1::new(bundle, sources))
}

fn load_candidate_oracle_material(
    content: &SqliteContentStore,
    kind: &OraclePortfolioElementKindV1,
) -> Result<Vec<u8>, ServerError> {
    match kind {
        OraclePortfolioElementKindV1::DomainRefinement(id) => {
            read_content::<DomainRefinementArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::CorpusCase(id) => {
            read_content::<CorpusCaseArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::Reference(id) => {
            read_content::<ReferenceArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::PropertyRelation(id) => {
            read_content::<PropertyRelationArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::SourceAdmissionPlan(id) => {
            read_content::<SourceAdmissionPlanArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::ValidFamilyPlan(id) => {
            read_content::<ValidFamilyPlanArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::ObservationPlan(id) => {
            read_content::<ObservationPlanArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::Comparator(id) => {
            read_content::<OracleComparatorProposalArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::ExecutionSafety(id) => {
            read_content::<OracleExecutionSafetyProposalArtifact>(content, *id)
        }
        OraclePortfolioElementKindV1::CoverageGap(id) => {
            read_content::<OracleCoverageGapArtifact>(content, *id)
        }
    }
}

fn read_content<T: ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<Vec<u8>, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(manager_error)?;
    Ok(bytes)
}

/// Consumes one readable business step selected from exact durable Controller state.
///
/// The top-level body deliberately remains the architecture: recover, select, execute.
///
/// # Errors
///
/// Returns configuration, canonical storage, durable workflow, or Proposal step initialization
/// failures without selecting a replacement task, request, or episode.
pub async fn drive_controller_workflow_once<W: OracleControlWorker>(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    intent_admission: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    oracle_controls: &mut W,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let action = select_controller_action(&state);
    execute_controller_action(
        server,
        proposal_step,
        intent_admission,
        workflow,
        oracle_controls,
        action,
    )
    .await
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

#[allow(clippy::too_many_lines)]
async fn execute_controller_action<W: OracleControlWorker>(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    intent_admission: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    oracle_controls: &mut W,
    action: ControllerWorkflowNextActionV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    match action {
        ControllerWorkflowNextActionV1::None => Ok(ControllerWorkflowManagerStatusV1::Idle),
        ControllerWorkflowNextActionV1::AuthorizeSirEpisode(authority) => {
            authorize_sir_episode_start(server, workflow, &authority)
        }
        ControllerWorkflowNextActionV1::RunSirEpisode(authority) => {
            run_authorized_sir_episode(server, proposal_step, workflow, &authority)
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
            authorize_next_oracle_action(server, workflow, &authority)
        }
        ControllerWorkflowNextActionV1::RunOracleStrategy(authority) => {
            run_authorized_oracle_strategy(server, proposal_step, workflow, authority)
        }
        ControllerWorkflowNextActionV1::AwaitOracleAdmissionMechanisms(authority) => {
            Ok(ControllerWorkflowManagerStatusV1::AwaitingOracleAdmissionMechanisms { authority })
        }
        ControllerWorkflowNextActionV1::RunOracleAdmissionControls {
            authority,
            receipts,
        } => prepare_or_finalize_oracle_controls(
            server,
            workflow,
            &authority,
            receipts,
            oracle_controls,
        ),
        ControllerWorkflowNextActionV1::ExecuteOracleAdmissionControl {
            authority,
            previous_receipts,
        } => execute_authorized_oracle_control(
            server,
            workflow,
            &authority,
            &previous_receipts,
            oracle_controls,
        ),
        ControllerWorkflowNextActionV1::PrepareCandidateOracleContract {
            authority,
            outcome,
            ..
        } => freeze_controller_candidate_oracle_contract(server, workflow, &authority, outcome),
        ControllerWorkflowNextActionV1::AwaitCandidateProposalLoop(authority) => {
            Ok(ControllerWorkflowManagerStatusV1::AwaitingCandidateProposalLoop { authority })
        }
        ControllerWorkflowNextActionV1::AuthorizeCandidateProposalEpisode(authority) => {
            authorize_candidate_proposal_start(server, workflow, &authority)
        }
        ControllerWorkflowNextActionV1::RunCandidateProposalEpisode(authority) => {
            run_authorized_candidate_proposal(server, proposal_step, workflow, &authority)
        }
        ControllerWorkflowNextActionV1::AwaitCandidateBuild {
            authority,
            terminal,
            proposal,
        } => Ok(ControllerWorkflowManagerStatusV1::AwaitingCandidateBuild {
            authority,
            terminal,
            proposal,
        }),
        ControllerWorkflowNextActionV1::AuthorizeCandidateBuild(authority) => {
            let mut events = open_events(server)?;
            authorize_candidate_build(
                &mut events,
                workflow,
                authority.request(),
                &CommandId::new(),
                observed_now()?,
            )
            .map_err(manager_error)?;
            Ok(ControllerWorkflowManagerStatusV1::Advanced)
        }
        ControllerWorkflowNextActionV1::RunCandidateBuild(authority) => {
            Ok(ControllerWorkflowManagerStatusV1::CandidateBuildReady { authority })
        }
        ControllerWorkflowNextActionV1::AwaitCandidateAdmissionMechanisms {
            authority,
            receipt,
            outcome,
        } => Ok(
            ControllerWorkflowManagerStatusV1::AwaitingCandidateAdmissionMechanisms {
                authority,
                receipt,
                outcome,
            },
        ),
        ControllerWorkflowNextActionV1::AwaitCandidateControlReceipts(authority) => {
            Ok(ControllerWorkflowManagerStatusV1::AwaitingCandidateControlReceipts { authority })
        }
        ControllerWorkflowNextActionV1::Terminal { outcome, status } => {
            Ok(ControllerWorkflowManagerStatusV1::Terminal { outcome, status })
        }
    }
}

fn authorize_candidate_proposal_start(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenCandidateProposalAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut events = open_events(server)?;
    authorize_candidate_proposal_episode(
        &mut events,
        workflow,
        authority.request(),
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn run_authorized_candidate_proposal(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenCandidateProposalAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let request: ProposalStepRequestV1 = load_canonical(&content, authority.request())?;
    let mut events = open_events(server)?;
    let outcome = run_proposal_step(proposal_step, &mut events, &mut content, &request)?;
    let cairn_migration::ProposalStepOutcomeV1::Terminal { terminal } = outcome else {
        let cairn_migration::ProposalStepOutcomeV1::WorkerRequest { request } = outcome else {
            unreachable!()
        };
        return Ok(ControllerWorkflowManagerStatusV1::WorkerRequest { request });
    };
    let ProposalStepPublicationV1::CandidateStrategy { proposal, .. } = terminal.publication()
    else {
        return Err(ServerError::MigrationWorkflow(
            "Candidate proposal step returned a non-Candidate publication".into(),
        ));
    };
    let _ = archive::<cairn_migration::CandidateProposalArtifact, _>(&mut content, proposal)?;
    let terminal_id = archive::<ProposalStepTerminalArtifact, _>(&mut content, &terminal)?;
    record_candidate_proposal(
        &mut events,
        workflow,
        authority.request(),
        &request,
        terminal_id,
        &terminal,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn freeze_controller_candidate_oracle_contract(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenOracleAdmissionAuthorityV1,
    outcome_id: ContentId<OracleAdmissionOutcomeArtifact>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let proposal: OraclePortfolioProposalV1 =
        load_canonical(&content, authority.portfolio().proposal())?;
    let outcome: cairn_migration::OracleAdmissionOutcomeV1 = load_canonical(&content, outcome_id)?;
    let contract = match CandidateOracleContractV1::derive(&proposal, &outcome) {
        Ok(contract) => contract,
        Err(CandidateExplorationError::NoAdmittedOracleClaims) => {
            return Ok(
                ControllerWorkflowManagerStatusV1::OracleAdmissionHasNoCandidateAuthority {
                    outcome: outcome_id,
                },
            );
        }
        Err(error) => return Err(manager_error(error)),
    };
    let _ = archive::<CandidateOracleContractArtifact, _>(&mut content, &contract)?;
    let oracle_workspace: OracleWorkspaceV1 =
        load_canonical(&content, authority.portfolio().exploration().workspace())?;
    let candidate_workspace = CandidateWorkspaceV1::derive(&oracle_workspace, &proposal, &contract)
        .map_err(manager_error)?;
    let _ = archive::<CandidateWorkspaceArtifact, _>(&mut content, &candidate_workspace)?;
    let mut events = open_events(server)?;
    freeze_candidate_oracle_contract(
        &mut events,
        workflow,
        &oracle_workspace,
        &proposal,
        &outcome,
        &contract,
        &candidate_workspace,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

fn run_authorized_oracle_strategy(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    workflow: &ControllerWorkflowV1,
    authority: FrozenOracleStrategyAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let content = open_content(server)?;
    let run: OracleStrategyRunV1 = load_canonical(&content, authority.run())?;
    if matches!(
        run.executor(),
        OracleStrategyExecutorV1::Deterministic { .. }
    ) {
        return Ok(ControllerWorkflowManagerStatusV1::OracleStrategyReady { authority });
    }
    drop(content);
    let request = prepare_oracle_strategy_proposal_step_request(server, &authority)?;
    let mut events = open_events(server)?;
    let mut content = open_content(server)?;
    let outcome = run_proposal_step(proposal_step, &mut events, &mut content, &request)?;
    match outcome {
        cairn_migration::ProposalStepOutcomeV1::Terminal { terminal } => {
            record_controller_oracle_strategy_terminal(server, workflow, &request, &terminal)
        }
        cairn_migration::ProposalStepOutcomeV1::WorkerRequest { request } => {
            Ok(ControllerWorkflowManagerStatusV1::WorkerRequest { request })
        }
    }
}

fn authorize_next_oracle_action(
    server: &ServerConfig,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenOracleExplorationAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let workspace: OracleWorkspaceV1 = load_canonical(&content, authority.workspace())?;
    let catalog: OracleStrategyCatalogV1 = load_canonical(&content, authority.strategy_catalog())?;
    let ledger: OracleExplorationLedgerV1 = load_canonical(&content, authority.ledger())?;
    match ledger
        .next_action(&catalog, workspace.budget())
        .map_err(manager_error)?
    {
        cairn_migration::OracleExplorationNextActionV1::RunStrategy {
            item,
            eligible_strategies,
        } => {
            let strategy = eligible_strategies.into_iter().next().ok_or_else(|| {
                ServerError::MigrationWorkflow("Oracle strategy set is empty".into())
            })?;
            let run = OracleStrategyRunV1::new(authority.workspace(), &item, strategy, &catalog)
                .map_err(manager_error)?;
            let started_ledger = ledger
                .start_strategy(&run, &catalog, workspace.budget())
                .map_err(manager_error)?;
            let _ = archive::<OracleStrategyRunArtifact, _>(&mut content, &run)?;
            let _ = archive::<OracleExplorationLedgerArtifact, _>(&mut content, &started_ledger)?;
            let mut events = open_events(server)?;
            authorize_oracle_strategy(
                &mut events,
                workflow,
                &workspace,
                &catalog,
                &ledger,
                &run,
                &CommandId::new(),
                observed_now()?,
            )
            .map_err(manager_error)?;
            Ok(ControllerWorkflowManagerStatusV1::Advanced)
        }
        cairn_migration::OracleExplorationNextActionV1::AuthorizeExperiment { request, .. } => Ok(
            ControllerWorkflowManagerStatusV1::AwaitingOracleExperiment {
                ledger: authority.ledger(),
                request,
            },
        ),
        cairn_migration::OracleExplorationNextActionV1::AwaitObservation => {
            Ok(ControllerWorkflowManagerStatusV1::OracleExplorationReady {
                authority: authority.clone(),
            })
        }
        cairn_migration::OracleExplorationNextActionV1::FreezePortfolio => {
            let proposal = OraclePortfolioProposalV1::freeze(&ledger).map_err(manager_error)?;
            let policy = OracleAdmissionPolicyV1::strict();
            let _ = archive::<OraclePortfolioProposalArtifact, _>(&mut content, &proposal)?;
            let _ = archive::<OracleAdmissionPolicyArtifact, _>(&mut content, &policy)?;
            let mut events = open_events(server)?;
            freeze_oracle_portfolio(
                &mut events,
                workflow,
                &ledger,
                &proposal,
                &policy,
                &CommandId::new(),
                observed_now()?,
            )
            .map_err(manager_error)?;
            Ok(ControllerWorkflowManagerStatusV1::Advanced)
        }
        cairn_migration::OracleExplorationNextActionV1::BudgetExhausted => Ok(
            ControllerWorkflowManagerStatusV1::OracleExplorationBudgetExhausted {
                ledger: authority.ledger(),
            },
        ),
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

fn run_authorized_sir_episode(
    server: &ServerConfig,
    proposal_step: &ProposalStepConfigV1,
    workflow: &ControllerWorkflowV1,
    authority: &FrozenSirAuthorityV1,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let request: ProposalStepRequestV1 = load_canonical(&content, authority.request())?;
    let mut events = open_events(server)?;
    let outcome = run_proposal_step(proposal_step, &mut events, &mut content, &request)?;
    let cairn_migration::ProposalStepOutcomeV1::Terminal { terminal } = outcome else {
        let cairn_migration::ProposalStepOutcomeV1::WorkerRequest { request } = outcome else {
            unreachable!()
        };
        return Ok(ControllerWorkflowManagerStatusV1::WorkerRequest { request });
    };
    let ProposalStepPublicationV1::Sir { proposal, .. } = terminal.publication() else {
        return Err(ServerError::MigrationWorkflow(
            "SIR proposal step returned a non-SIR publication".into(),
        ));
    };
    let _ = archive::<SirIntentHypothesisSetProposalArtifact, _>(&mut content, proposal)?;
    let terminal_id = archive::<ProposalStepTerminalArtifact, _>(&mut content, &terminal)?;
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
    terminal_id: ContentId<ProposalStepTerminalArtifact>,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let mut content = open_content(server)?;
    let terminal: ProposalStepTerminalV1 = load_canonical(&content, terminal_id)?;
    let proposal: IntentHypothesisSetProposalV1 = load_canonical(&content, proposal_id)?;
    let recovery_input: IntentRecoveryInputV1 =
        load_canonical(&content, authority.recovery_input())?;
    if !matches!(
        terminal.publication(),
        ProposalStepPublicationV1::Sir { proposal_id: id, .. } if *id == proposal_id
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
                let mut events = open_events(server)?;
                record_intent_admission_blocked(
                    &mut events,
                    workflow,
                    decision,
                    executable,
                    restricted_store,
                    intent_block_reason(failure.reason),
                    &CommandId::new(),
                    observed_now()?,
                )
                .map_err(manager_error)?;
                tracing::warn!(
                    target: "cairn.server.controller-workflow",
                    event = "intent_admission_blocked",
                    task_id = %workflow.task_id(),
                    decision = %decision,
                    reason = ?failure.reason,
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

/// Reauthorizes one durably blocked Intent Admission operation against current configured bytes.
pub fn reauthorize_controller_intent_admission(
    server: &ServerConfig,
    config: &IntentAdmissionProcessConfigV1,
    workflow: &ControllerWorkflowV1,
    command_id: &CommandId,
    observed_at: cairn_protocol::ObservedAtUnixMillis,
) -> Result<ControllerWorkflowManagerStatusV1, ServerError> {
    let state = recover_controller_turn(server, workflow)?;
    let ControllerWorkflowStateV1::IntentAdmissionBlocked { decision, .. } = state else {
        return Err(ServerError::MigrationWorkflow(
            "Controller Intent Admission is not awaiting reconciliation".into(),
        ));
    };
    config.validate(server)?;
    let executable = config.executable_identity()?;
    let restricted_store = config.restricted_store_identity()?;
    let mut events = open_events(server)?;
    reauthorize_intent_admission(
        &mut events,
        workflow,
        decision,
        executable,
        restricted_store,
        command_id,
        observed_at,
    )
    .map_err(manager_error)?;
    Ok(ControllerWorkflowManagerStatusV1::Advanced)
}

const fn intent_block_reason(
    reason: IntentAdmissionProcessBlockedV1,
) -> IntentAdmissionBlockReasonV1 {
    match reason {
        IntentAdmissionProcessBlockedV1::InvocationDrift => {
            IntentAdmissionBlockReasonV1::InvocationDrift
        }
        IntentAdmissionProcessBlockedV1::TimedOut => IntentAdmissionBlockReasonV1::TimedOut,
        IntentAdmissionProcessBlockedV1::ExitFailure => IntentAdmissionBlockReasonV1::ExitFailure,
        IntentAdmissionProcessBlockedV1::StdoutLimitExceeded => {
            IntentAdmissionBlockReasonV1::StdoutLimitExceeded
        }
        IntentAdmissionProcessBlockedV1::StderrLimitExceeded => {
            IntentAdmissionBlockReasonV1::StderrLimitExceeded
        }
        IntentAdmissionProcessBlockedV1::InvalidOutcome => {
            IntentAdmissionBlockReasonV1::InvalidOutcome
        }
    }
}

fn open_events(server: &ServerConfig) -> Result<SqliteEventStore, ServerError> {
    server.validate_schema()?;
    SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)
}

fn verify_oracle_workspace_material(
    content: &SqliteContentStore,
    workspace: &OracleWorkspaceV1,
    catalog: &OracleStrategyCatalogV1,
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
    )?;
    for strategy in catalog.strategies() {
        match strategy.executor() {
            OracleStrategyExecutorV1::Deterministic { implementation } => {
                verify_content::<OracleStrategyImplementationArtifact>(content, *implementation)?;
            }
            OracleStrategyExecutorV1::AgentStep {
                authorship_model,
                invocation,
                tools,
            } => {
                verify_content::<ModelConfigurationArtifact>(content, *authorship_model)?;
                verify_content::<AgentRuntimeBindingArtifact>(content, *invocation)?;
                let tool_catalog: OracleStrategyToolCatalogV1 = load_canonical(content, *tools)?;
                if tool_catalog != OracleStrategyToolCatalogV1::standard() {
                    return Err(ServerError::MigrationWorkflow(
                        "Oracle Agent strategy changed its current-V1 tool surface".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn archive_oracle_submission_material(
    content: &mut SqliteContentStore,
    run: &OracleStrategyRunV1,
    submission: &OracleStrategySubmissionV1,
) -> Result<(), ServerError> {
    match submission.result() {
        OracleStrategySubmissionOutcomeV1::Contribute { elements } => {
            for element in elements {
                verify_oracle_portfolio_material(content, element.kind())?;
                for observation in element.observations() {
                    verify_oracle_observation(content, run, *observation)?;
                }
                let _ = archive::<OraclePortfolioElementArtifact, _>(content, element)?;
            }
        }
        OracleStrategySubmissionOutcomeV1::RequestExperiment { request } => {
            verify_content::<OracleExperimentArgumentsArtifact>(content, request.arguments())?;
            let _ = archive::<OracleExperimentRequestArtifact, _>(content, request)?;
        }
        OracleStrategySubmissionOutcomeV1::PreserveUnknown { evidence } => {
            for value in evidence {
                for observation in value.observations() {
                    verify_oracle_observation(content, run, *observation)?;
                }
                let _ = archive::<OracleUnknownEvidenceArtifact, _>(content, value)?;
            }
        }
    }
    Ok(())
}

fn verify_oracle_observation(
    content: &SqliteContentStore,
    run: &OracleStrategyRunV1,
    id: ContentId<OracleExplorationObservationArtifact>,
) -> Result<(), ServerError> {
    let observation: OracleExplorationObservationV1 = load_canonical(content, id)?;
    if observation.item() != run.item()
        || observation.run() != run.identity().map_err(manager_error)?
    {
        return Err(ServerError::MigrationWorkflow(
            "Oracle submission cited an observation from another cell or run".into(),
        ));
    }
    Ok(())
}

fn verify_oracle_portfolio_material(
    content: &SqliteContentStore,
    material: &OraclePortfolioElementKindV1,
) -> Result<(), ServerError> {
    match material {
        OraclePortfolioElementKindV1::DomainRefinement(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::CorpusCase(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::Reference(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::PropertyRelation(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::SourceAdmissionPlan(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::ValidFamilyPlan(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::ObservationPlan(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::Comparator(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::ExecutionSafety(id) => verify_content(content, *id),
        OraclePortfolioElementKindV1::CoverageGap(id) => verify_content(content, *id),
    }
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

fn archive_bytes<T: ContentType>(
    content: &mut SqliteContentStore,
    bytes: &[u8],
) -> Result<ContentId<T>, ServerError> {
    let expected = ContentId::<T>::derive(bytes).map_err(manager_error)?;
    let actual = content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(manager_error)?
        .content_id;
    if actual != expected {
        return Err(ServerError::MigrationWorkflow(
            "Controller byte artifact changed identity during archival".into(),
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

fn load_utf8_content<T: ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<String, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(manager_error)?;
    String::from_utf8(bytes).map_err(manager_error)
}

fn manager_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
