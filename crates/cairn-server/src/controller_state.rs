//! Durable Controller workflow state from exact request freeze through admitted intent.
#![allow(clippy::missing_errors_doc)]

use cairn_execution::{ExecutionOutcome, ExecutionReceipt, ExecutionReceiptArtifact};
use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EventId, ObservedAtUnixMillis, SchemaName,
    SchemaVersion, StreamRevision, TaskId,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize, de};
use thiserror::Error;

use cairn_admission::{
    IntentAdmissionExecutableArtifact, IntentAdmissionPublicOutcomeArtifact,
    IntentAdmissionPublicOutcomeV1, IntentAdmissionRestrictedStoreArtifact,
    MigrationIntentContractV1, UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1,
    UserIntentDecisionArtifact, UserIntentDecisionV1,
};
use cairn_migration::{
    CandidateAdmissionAttemptArtifact, CandidateAdmissionAttemptV1,
    CandidateAdmissionEvidenceArtifact, CandidateAdmissionEvidenceV1,
    CandidateAdmissionOutcomeArtifact, CandidateAdmissionOutcomeV1, CandidateBuildPlanArtifact,
    CandidateBuildPlanV1, CandidateBuildRequestArtifact, CandidateBuildRequestV1,
    CandidateClaimStatusV1, CandidateMechanismCatalogArtifact, CandidateMechanismCatalogV1,
    CandidateOracleContractArtifact, CandidateOracleContractV1, CandidateProposalArtifact,
    CandidateProposalV1, CandidateWorkspaceArtifact, CandidateWorkspaceV1,
    IntentDecisionRequestBatchArtifact, IntentDecisionRequestBatchV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, MigrationIntentContractArtifact, OracleAdmissionAttemptArtifact,
    OracleAdmissionAttemptV1, OracleAdmissionEvidenceArtifact, OracleAdmissionEvidenceV1,
    OracleAdmissionMechanismCatalogArtifact, OracleAdmissionMechanismCatalogV1,
    OracleAdmissionOutcomeArtifact, OracleAdmissionOutcomeV1, OracleAdmissionPolicyArtifact,
    OracleAdmissionPolicyV1, OracleClaimV1, OracleControlDispatchArtifact, OracleControlDispatchV1,
    OracleControlReceiptV1, OracleControlRunArtifact, OracleControlRunV1,
    OracleCoveragePolicyArtifact, OracleCoveragePolicyV1, OracleExplorationLedgerArtifact,
    OracleExplorationLedgerV1, OracleExplorationObservationV1, OracleExplorationRevision,
    OraclePortfolioProposalArtifact, OraclePortfolioProposalV1, OracleStrategyCatalogArtifact,
    OracleStrategyCatalogV1, OracleStrategyExecutorV1, OracleStrategyImplementationArtifact,
    OracleStrategyRunArtifact, OracleStrategyRunV1, OracleStrategySubmissionV1,
    OracleWorkspaceArtifact, OracleWorkspaceV1, ProposalStepPublicationV1,
    ProposalStepRequestArtifact, ProposalStepRequestV1, ProposalStepRoleRequestV1,
    ProposalStepTerminalArtifact, ProposalStepTerminalV1, SirIntentHypothesisSetProposalArtifact,
    TrustedOracleControlObservationV1, UserIntentDecisionRequestArtifact,
    UserIntentDecisionRequestV1, derive_oracle_claims, derive_oracle_work_items,
    recompute_candidate_admission, recompute_oracle_admission,
};

const WORKFLOW_FROZEN: &str = "migration.controller-workflow-frozen";
const WORKFLOW_CANCELLED: &str = "migration.controller-workflow-cancelled";
const SIR_EPISODE_AUTHORIZED: &str = "migration.controller-sir-episode-authorized";
const SIR_PROPOSAL_RECORDED: &str = "migration.controller-sir-proposal-recorded";
const INTENT_DECISION_REQUESTS_RECORDED: &str =
    "migration.controller-intent-decision-requests-recorded";
const USER_INTENT_DECISION_RECORDED: &str = "migration.controller-user-intent-decision-recorded";
const INTENT_ADMISSION_AUTHORIZED: &str = "migration.controller-intent-admission-authorized";
const INTENT_ADMISSION_BLOCKED: &str = "migration.controller-intent-admission-blocked";
const ADMITTED_INTENT_RECORDED: &str = "migration.controller-admitted-intent-recorded";
const ORACLE_EXPLORATION_OPENED: &str = "migration.controller-oracle-exploration-opened";
const ORACLE_STRATEGY_AUTHORIZED: &str = "migration.controller-oracle-strategy-authorized";
const ORACLE_STRATEGY_OBSERVATIONS_RECORDED: &str =
    "migration.controller-oracle-strategy-observations-recorded";
const ORACLE_STRATEGY_SUBMISSION_RECORDED: &str =
    "migration.controller-oracle-strategy-submission-recorded";
const ORACLE_PORTFOLIO_FROZEN: &str = "migration.controller-oracle-portfolio-frozen";
const ORACLE_ADMISSION_AUTHORIZED: &str = "migration.controller-oracle-admission-authorized";
const ORACLE_CONTROL_AUTHORIZED: &str = "migration.controller-oracle-control-authorized";
const ORACLE_CONTROL_OBSERVED: &str = "migration.controller-oracle-control-observed";
const ORACLE_ADMISSION_RECORDED: &str = "migration.controller-oracle-admission-recorded";
const CANDIDATE_ORACLE_CONTRACT_FROZEN: &str =
    "migration.controller-candidate-oracle-contract-frozen";
const CANDIDATE_PROPOSAL_REQUEST_FROZEN: &str =
    "migration.controller-candidate-proposal-request-frozen";
const CANDIDATE_PROPOSAL_EPISODE_AUTHORIZED: &str =
    "migration.controller-candidate-proposal-episode-authorized";
const CANDIDATE_PROPOSAL_RECORDED: &str = "migration.controller-candidate-proposal-recorded";
const CANDIDATE_BUILD_FROZEN: &str = "migration.controller-candidate-build-frozen";
const CANDIDATE_BUILD_AUTHORIZED: &str = "migration.controller-candidate-build-authorized";
const CANDIDATE_BUILD_OBSERVED: &str = "migration.controller-candidate-build-observed";
const CANDIDATE_ADMISSION_AUTHORIZED: &str = "migration.controller-candidate-admission-authorized";
const CANDIDATE_ADMISSION_RECORDED: &str = "migration.controller-candidate-admission-recorded";

/// Exact SIR authority frozen before the Controller may start the Proposal step effect.
///
/// A SIR proposal identity cannot be substituted for the exact proposal-step request authority.
///
/// ```compile_fail
/// use cairn_migration::SirIntentHypothesisSetProposalArtifact;
/// use cairn_server::FrozenSirAuthorityV1;
/// use cairn_protocol::ContentId;
/// fn require_request(authority: &FrozenSirAuthorityV1, proposal: ContentId<SirIntentHypothesisSetProposalArtifact>) {
///     let _: cairn_protocol::ContentId<cairn_migration::ProposalStepRequestArtifact> = proposal;
///     let _ = authority;
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSirAuthorityV1 {
    task_id: TaskId,
    request: ContentId<ProposalStepRequestArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    episode_id: cairn_protocol::EpisodeId,
}

impl FrozenSirAuthorityV1 {
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalStepRequestArtifact> {
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

/// Exact archived authority for the initial, task-owned Oracle Exploration ledger.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOracleExplorationAuthorityV1 {
    task_id: TaskId,
    admitted_intent_outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    workspace: ContentId<OracleWorkspaceArtifact>,
    coverage_policy: ContentId<OracleCoveragePolicyArtifact>,
    strategy_catalog: ContentId<OracleStrategyCatalogArtifact>,
    claims: Vec<OracleClaimV1>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    ledger_revision: OracleExplorationRevision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenOracleExplorationAuthorityWire {
    task_id: TaskId,
    admitted_intent_outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    workspace: ContentId<OracleWorkspaceArtifact>,
    coverage_policy: ContentId<OracleCoveragePolicyArtifact>,
    strategy_catalog: ContentId<OracleStrategyCatalogArtifact>,
    claims: Vec<OracleClaimV1>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    ledger_revision: OracleExplorationRevision,
}

impl<'de> Deserialize<'de> for FrozenOracleExplorationAuthorityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = FrozenOracleExplorationAuthorityWire::deserialize(deserializer)?;
        let identities = wire
            .claims
            .iter()
            .map(OracleClaimV1::identity)
            .collect::<Result<Vec<_>, _>>()
            .map_err(de::Error::custom)?;
        if identities.is_empty()
            || identities
                .windows(2)
                .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
        {
            return Err(de::Error::custom(
                "Oracle Exploration claims must be nonempty and strictly canonical",
            ));
        }
        Ok(Self {
            task_id: wire.task_id,
            admitted_intent_outcome: wire.admitted_intent_outcome,
            admitted_intent: wire.admitted_intent,
            recovery_input: wire.recovery_input,
            workspace: wire.workspace,
            coverage_policy: wire.coverage_policy,
            strategy_catalog: wire.strategy_catalog,
            claims: wire.claims,
            ledger: wire.ledger,
            ledger_revision: wire.ledger_revision,
        })
    }
}

impl FrozenOracleExplorationAuthorityV1 {
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
    #[must_use]
    pub const fn admitted_intent_outcome(&self) -> ContentId<IntentAdmissionPublicOutcomeArtifact> {
        self.admitted_intent_outcome
    }
    #[must_use]
    pub const fn admitted_intent(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.admitted_intent
    }
    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }
    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }
    #[must_use]
    pub const fn coverage_policy(&self) -> ContentId<OracleCoveragePolicyArtifact> {
        self.coverage_policy
    }
    #[must_use]
    pub const fn strategy_catalog(&self) -> ContentId<OracleStrategyCatalogArtifact> {
        self.strategy_catalog
    }
    #[must_use]
    pub fn claims(&self) -> &[OracleClaimV1] {
        &self.claims
    }
    #[must_use]
    pub const fn ledger(&self) -> ContentId<OracleExplorationLedgerArtifact> {
        self.ledger
    }
    #[must_use]
    pub const fn ledger_revision(&self) -> OracleExplorationRevision {
        self.ledger_revision
    }
}

/// Durable start authority for one exact strategy run over one exact ledger revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOracleStrategyAuthorityV1 {
    exploration: FrozenOracleExplorationAuthorityV1,
    previous_ledger: ContentId<OracleExplorationLedgerArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
}

impl FrozenOracleStrategyAuthorityV1 {
    #[must_use]
    pub const fn exploration(&self) -> &FrozenOracleExplorationAuthorityV1 {
        &self.exploration
    }
    #[must_use]
    pub const fn previous_ledger(&self) -> ContentId<OracleExplorationLedgerArtifact> {
        self.previous_ledger
    }
    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }
}

/// Frozen portfolio and strict policy awaiting a qualified mechanism catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOraclePortfolioAuthorityV1 {
    exploration: FrozenOracleExplorationAuthorityV1,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    policy: ContentId<OracleAdmissionPolicyArtifact>,
}

impl FrozenOraclePortfolioAuthorityV1 {
    #[must_use]
    pub const fn exploration(&self) -> &FrozenOracleExplorationAuthorityV1 {
        &self.exploration
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn policy(&self) -> ContentId<OracleAdmissionPolicyArtifact> {
        self.policy
    }
}

/// Durable independent Admission authority over one exact portfolio/control inventory.
///
/// A portfolio identity cannot be substituted for its mechanically derived Admission attempt.
///
/// ```compile_fail
/// use cairn_migration::{OracleAdmissionAttemptArtifact, OraclePortfolioProposalArtifact};
/// use cairn_protocol::ContentId;
/// fn require_attempt(proposal: ContentId<OraclePortfolioProposalArtifact>) {
///     let _: ContentId<OracleAdmissionAttemptArtifact> = proposal;
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOracleAdmissionAuthorityV1 {
    portfolio: FrozenOraclePortfolioAuthorityV1,
    mechanisms: ContentId<OracleAdmissionMechanismCatalogArtifact>,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    mechanism_catalog: Box<OracleAdmissionMechanismCatalogV1>,
    admission_attempt: Box<OracleAdmissionAttemptV1>,
}

impl FrozenOracleAdmissionAuthorityV1 {
    #[must_use]
    pub const fn portfolio(&self) -> &FrozenOraclePortfolioAuthorityV1 {
        &self.portfolio
    }

    #[must_use]
    pub const fn mechanisms(&self) -> ContentId<OracleAdmissionMechanismCatalogArtifact> {
        self.mechanisms
    }

    #[must_use]
    pub const fn attempt(&self) -> ContentId<OracleAdmissionAttemptArtifact> {
        self.attempt
    }
}

/// Durable start authority for one exact qualified Oracle control execution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOracleControlAuthorityV1 {
    admission: FrozenOracleAdmissionAuthorityV1,
    run: ContentId<OracleControlRunArtifact>,
    dispatch: ContentId<OracleControlDispatchArtifact>,
}

impl FrozenOracleControlAuthorityV1 {
    #[must_use]
    pub const fn admission(&self) -> &FrozenOracleAdmissionAuthorityV1 {
        &self.admission
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleControlRunArtifact> {
        self.run
    }

    #[must_use]
    pub const fn dispatch(&self) -> ContentId<OracleControlDispatchArtifact> {
        self.dispatch
    }
}

/// Exact admitted Oracle subset authorized as the immutable Candidate input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCandidateOracleAuthorityV1 {
    oracle: FrozenOracleAdmissionAuthorityV1,
    evidence: ContentId<OracleAdmissionEvidenceArtifact>,
    outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    contract: ContentId<CandidateOracleContractArtifact>,
    workspace: ContentId<CandidateWorkspaceArtifact>,
}

impl FrozenCandidateOracleAuthorityV1 {
    #[must_use]
    pub const fn oracle(&self) -> &FrozenOracleAdmissionAuthorityV1 {
        &self.oracle
    }

    #[must_use]
    pub const fn evidence(&self) -> ContentId<OracleAdmissionEvidenceArtifact> {
        self.evidence
    }

    #[must_use]
    pub const fn outcome(&self) -> ContentId<OracleAdmissionOutcomeArtifact> {
        self.outcome
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<CandidateOracleContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<CandidateWorkspaceArtifact> {
        self.workspace
    }
}

/// Exact Candidate proposal step request frozen before any model effect may start.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCandidateProposalAuthorityV1 {
    candidate: FrozenCandidateOracleAuthorityV1,
    request: ContentId<ProposalStepRequestArtifact>,
    episode_id: cairn_protocol::EpisodeId,
}

impl FrozenCandidateProposalAuthorityV1 {
    #[must_use]
    pub const fn candidate(&self) -> &FrozenCandidateOracleAuthorityV1 {
        &self.candidate
    }

    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalStepRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn episode_id(&self) -> cairn_protocol::EpisodeId {
        self.episode_id
    }
}

/// Exact product-owned Candidate build operation frozen before any Worker effect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCandidateBuildAuthorityV1 {
    candidate: FrozenCandidateProposalAuthorityV1,
    terminal: ContentId<ProposalStepTerminalArtifact>,
    proposal: ContentId<CandidateProposalArtifact>,
    plan: ContentId<CandidateBuildPlanArtifact>,
    request: ContentId<CandidateBuildRequestArtifact>,
}

impl FrozenCandidateBuildAuthorityV1 {
    #[must_use]
    pub const fn candidate(&self) -> &FrozenCandidateProposalAuthorityV1 {
        &self.candidate
    }
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }
    #[must_use]
    pub const fn plan(&self) -> ContentId<CandidateBuildPlanArtifact> {
        self.plan
    }
    #[must_use]
    pub const fn request(&self) -> ContentId<CandidateBuildRequestArtifact> {
        self.request
    }
}

/// Exact build observation and complete Candidate Admission matrix authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCandidateAdmissionAuthorityV1 {
    build: FrozenCandidateBuildAuthorityV1,
    receipt: ContentId<ExecutionReceiptArtifact>,
    mechanisms: ContentId<CandidateMechanismCatalogArtifact>,
    attempt: ContentId<CandidateAdmissionAttemptArtifact>,
}

impl FrozenCandidateAdmissionAuthorityV1 {
    #[must_use]
    pub const fn build(&self) -> &FrozenCandidateBuildAuthorityV1 {
        &self.build
    }
    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }
    #[must_use]
    pub const fn mechanisms(&self) -> ContentId<CandidateMechanismCatalogArtifact> {
        self.mechanisms
    }
    #[must_use]
    pub const fn attempt(&self) -> ContentId<CandidateAdmissionAttemptArtifact> {
        self.attempt
    }
}

/// Final task outcome; partial evidence is never promoted to success.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationTerminalStatusV1 {
    Admitted,
    Partial,
    Rejected,
}

/// Exact executor-specific completion evidence committed with one strategy submission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "executor", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleStrategyCompletionV1 {
    Deterministic {
        implementation: ContentId<OracleStrategyImplementationArtifact>,
        submission: OracleStrategySubmissionV1,
    },
    AgentStep {
        request_id: ContentId<ProposalStepRequestArtifact>,
        request: Box<ProposalStepRequestV1>,
        terminal_id: ContentId<ProposalStepTerminalArtifact>,
        terminal: Box<ProposalStepTerminalV1>,
    },
}

impl OracleStrategyCompletionV1 {
    fn submission<'a>(
        &'a self,
        run: &OracleStrategyRunV1,
    ) -> Result<&'a OracleStrategySubmissionV1, ControllerWorkflowError> {
        let submission = match self {
            Self::Deterministic {
                implementation,
                submission,
            } => {
                if run.executor()
                    != &(OracleStrategyExecutorV1::Deterministic {
                        implementation: *implementation,
                    })
                {
                    return Err(ControllerWorkflowError::BindingMismatch);
                }
                submission
            }
            Self::AgentStep {
                request_id,
                request,
                terminal_id,
                terminal,
            } => {
                if request.identity().map_err(binding_error)? != *request_id
                    || terminal.identity().map_err(binding_error)? != *terminal_id
                    || terminal.validate_against(request).is_err()
                {
                    return Err(ControllerWorkflowError::BindingMismatch);
                }
                let ProposalStepRoleRequestV1::OracleStrategy {
                    run: requested_run, ..
                } = request.role()
                else {
                    return Err(ControllerWorkflowError::BindingMismatch);
                };
                let ProposalStepPublicationV1::OracleStrategy { submission, .. } =
                    terminal.publication()
                else {
                    return Err(ControllerWorkflowError::BindingMismatch);
                };
                if requested_run != run {
                    return Err(ControllerWorkflowError::BindingMismatch);
                }
                submission
            }
        };
        if submission.run() != run.identity().map_err(binding_error)?
            || submission.item() != run.item()
        {
            return Err(ControllerWorkflowError::BindingMismatch);
        }
        Ok(submission)
    }
}

/// Durable Controller state reconstructed only from the current-V1 event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerWorkflowStateV1 {
    NotFound,
    Cancelled,
    Frozen(FrozenSirAuthorityV1),
    SirEpisodeAuthorized(FrozenSirAuthorityV1),
    SirProposed {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    },
    AwaitingUserIntentDecision {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
    },
    UserIntentDecisionRecorded {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
    },
    IntentAdmissionAuthorized {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    },
    IntentAdmissionBlocked {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
        reason: IntentAdmissionBlockReasonV1,
    },
    AdmittedIntent {
        authority: FrozenSirAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        requests: ContentId<IntentDecisionRequestBatchArtifact>,
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        decision: ContentId<UserIntentDecisionArtifact>,
        executable: ContentId<IntentAdmissionExecutableArtifact>,
        restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
        contract_body: Box<MigrationIntentContractV1>,
    },
    OracleExplorationOpened(FrozenOracleExplorationAuthorityV1),
    OracleStrategyAuthorized(FrozenOracleStrategyAuthorityV1),
    OraclePortfolioFrozen(FrozenOraclePortfolioAuthorityV1),
    OracleAdmissionAuthorized(FrozenOracleAdmissionAuthorityV1),
    OracleControlAuthorized {
        authority: FrozenOracleControlAuthorityV1,
        previous_receipts: Vec<OracleControlReceiptV1>,
    },
    OracleControlsObserved {
        authority: FrozenOracleAdmissionAuthorityV1,
        receipts: Vec<OracleControlReceiptV1>,
    },
    OracleAdmitted {
        authority: FrozenOracleAdmissionAuthorityV1,
        evidence: ContentId<OracleAdmissionEvidenceArtifact>,
        outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    },
    CandidateOracleContractFrozen(FrozenCandidateOracleAuthorityV1),
    CandidateProposalRequestFrozen(FrozenCandidateProposalAuthorityV1),
    CandidateProposalEpisodeAuthorized(FrozenCandidateProposalAuthorityV1),
    CandidateProposed {
        authority: FrozenCandidateProposalAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<CandidateProposalArtifact>,
    },
    CandidateBuildFrozen(FrozenCandidateBuildAuthorityV1),
    CandidateBuildAuthorized(FrozenCandidateBuildAuthorityV1),
    CandidateBuildObserved {
        authority: FrozenCandidateBuildAuthorityV1,
        receipt: ContentId<ExecutionReceiptArtifact>,
        outcome: ExecutionOutcome,
    },
    CandidateAdmissionAuthorized(FrozenCandidateAdmissionAuthorityV1),
    Terminal {
        authority: FrozenCandidateAdmissionAuthorityV1,
        evidence: ContentId<CandidateAdmissionEvidenceArtifact>,
        outcome: ContentId<CandidateAdmissionOutcomeArtifact>,
        status: MigrationTerminalStatusV1,
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
        terminal: ContentId<ProposalStepTerminalArtifact>,
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
    AwaitOracleExplorationWorkspace {
        outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        contract: ContentId<MigrationIntentContractArtifact>,
    },
    RunOracleExploration(FrozenOracleExplorationAuthorityV1),
    RunOracleStrategy(FrozenOracleStrategyAuthorityV1),
    AwaitOracleAdmissionMechanisms(FrozenOraclePortfolioAuthorityV1),
    RunOracleAdmissionControls {
        authority: FrozenOracleAdmissionAuthorityV1,
        receipts: Vec<OracleControlReceiptV1>,
    },
    ExecuteOracleAdmissionControl {
        authority: FrozenOracleControlAuthorityV1,
        previous_receipts: Vec<OracleControlReceiptV1>,
    },
    PrepareCandidateOracleContract {
        authority: FrozenOracleAdmissionAuthorityV1,
        evidence: ContentId<OracleAdmissionEvidenceArtifact>,
        outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    },
    AwaitCandidateProposalLoop(FrozenCandidateOracleAuthorityV1),
    AuthorizeCandidateProposalEpisode(FrozenCandidateProposalAuthorityV1),
    RunCandidateProposalEpisode(FrozenCandidateProposalAuthorityV1),
    AwaitCandidateBuild {
        authority: FrozenCandidateProposalAuthorityV1,
        terminal: ContentId<ProposalStepTerminalArtifact>,
        proposal: ContentId<CandidateProposalArtifact>,
    },
    AuthorizeCandidateBuild(FrozenCandidateBuildAuthorityV1),
    RunCandidateBuild(FrozenCandidateBuildAuthorityV1),
    AwaitCandidateAdmissionMechanisms {
        authority: FrozenCandidateBuildAuthorityV1,
        receipt: ContentId<ExecutionReceiptArtifact>,
        outcome: ExecutionOutcome,
    },
    AwaitCandidateControlReceipts(FrozenCandidateAdmissionAuthorityV1),
    Terminal {
        outcome: ContentId<CandidateAdmissionOutcomeArtifact>,
        status: MigrationTerminalStatusV1,
    },
}

impl ControllerWorkflowStateV1 {
    /// Selects the next architectural step without performing it.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn next_action(&self) -> ControllerWorkflowNextActionV1 {
        match self {
            Self::NotFound
            | Self::Cancelled
            | Self::IntentAdmissionBlocked { .. }
            | Self::OracleAdmitted { .. } => ControllerWorkflowNextActionV1::None,
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
            } => ControllerWorkflowNextActionV1::AwaitOracleExplorationWorkspace {
                outcome: *outcome,
                contract: *contract,
            },
            Self::OracleExplorationOpened(authority) => {
                ControllerWorkflowNextActionV1::RunOracleExploration(authority.clone())
            }
            Self::OracleStrategyAuthorized(authority) => {
                ControllerWorkflowNextActionV1::RunOracleStrategy(authority.clone())
            }
            Self::OraclePortfolioFrozen(authority) => {
                ControllerWorkflowNextActionV1::AwaitOracleAdmissionMechanisms(authority.clone())
            }
            Self::OracleAdmissionAuthorized(authority) => {
                ControllerWorkflowNextActionV1::RunOracleAdmissionControls {
                    authority: authority.clone(),
                    receipts: Vec::new(),
                }
            }
            Self::OracleControlAuthorized {
                authority,
                previous_receipts,
            } => ControllerWorkflowNextActionV1::ExecuteOracleAdmissionControl {
                authority: authority.clone(),
                previous_receipts: previous_receipts.clone(),
            },
            Self::OracleControlsObserved {
                authority,
                receipts,
            } => ControllerWorkflowNextActionV1::RunOracleAdmissionControls {
                authority: authority.clone(),
                receipts: receipts.clone(),
            },
            Self::CandidateOracleContractFrozen(authority) => {
                ControllerWorkflowNextActionV1::AwaitCandidateProposalLoop(authority.clone())
            }
            Self::CandidateProposalRequestFrozen(authority) => {
                ControllerWorkflowNextActionV1::AuthorizeCandidateProposalEpisode(authority.clone())
            }
            Self::CandidateProposalEpisodeAuthorized(authority) => {
                ControllerWorkflowNextActionV1::RunCandidateProposalEpisode(authority.clone())
            }
            Self::CandidateProposed {
                authority,
                terminal,
                proposal,
            } => ControllerWorkflowNextActionV1::AwaitCandidateBuild {
                authority: authority.clone(),
                terminal: *terminal,
                proposal: *proposal,
            },
            Self::CandidateBuildFrozen(authority) => {
                ControllerWorkflowNextActionV1::AuthorizeCandidateBuild(authority.clone())
            }
            Self::CandidateBuildAuthorized(authority) => {
                ControllerWorkflowNextActionV1::RunCandidateBuild(authority.clone())
            }
            Self::CandidateBuildObserved {
                authority,
                receipt,
                outcome,
            } => ControllerWorkflowNextActionV1::AwaitCandidateAdmissionMechanisms {
                authority: authority.clone(),
                receipt: *receipt,
                outcome: *outcome,
            },
            Self::CandidateAdmissionAuthorized(authority) => {
                ControllerWorkflowNextActionV1::AwaitCandidateControlReceipts(authority.clone())
            }
            Self::Terminal {
                outcome, status, ..
            } => ControllerWorkflowNextActionV1::Terminal {
                outcome: *outcome,
                status: *status,
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
struct WorkflowCancelledPayload {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SirEpisodeAuthorizedPayload {
    request: ContentId<ProposalStepRequestArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SirProposalRecordedPayload {
    terminal: ContentId<ProposalStepTerminalArtifact>,
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

/// Durable, non-diagnostic classification of an independently authorized Admission failure.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentAdmissionBlockReasonV1 {
    InvocationDrift,
    TimedOut,
    ExitFailure,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    InvalidOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentAdmissionBlockedPayload {
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    reason: IntentAdmissionBlockReasonV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AdmittedIntentRecordedPayload {
    outcome: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    contract_body: MigrationIntentContractV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleExplorationOpenedPayload {
    authority: FrozenOracleExplorationAuthorityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyAuthorizedPayload {
    workspace: OracleWorkspaceV1,
    catalog: OracleStrategyCatalogV1,
    previous_ledger: OracleExplorationLedgerV1,
    run: OracleStrategyRunV1,
    started_ledger: OracleExplorationLedgerV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategySubmissionRecordedPayload {
    workspace: OracleWorkspaceV1,
    previous_ledger: OracleExplorationLedgerV1,
    run: OracleStrategyRunV1,
    completion: OracleStrategyCompletionV1,
    next_ledger: OracleExplorationLedgerV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyObservationsRecordedPayload {
    previous_ledger: OracleExplorationLedgerV1,
    run: OracleStrategyRunV1,
    observations: Vec<OracleExplorationObservationV1>,
    next_ledger: OracleExplorationLedgerV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioFrozenPayload {
    ledger: OracleExplorationLedgerV1,
    proposal: OraclePortfolioProposalV1,
    policy: OracleAdmissionPolicyV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionAuthorizedPayload {
    proposal: OraclePortfolioProposalV1,
    policy: OracleAdmissionPolicyV1,
    mechanisms: OracleAdmissionMechanismCatalogV1,
    attempt: OracleAdmissionAttemptV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleControlAuthorizedPayload {
    run: OracleControlRunV1,
    dispatch: OracleControlDispatchV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleControlObservedPayload {
    run: OracleControlRunV1,
    dispatch: OracleControlDispatchV1,
    observation: TrustedOracleControlObservationV1,
    receipt: OracleControlReceiptV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionRecordedPayload {
    proposal: OraclePortfolioProposalV1,
    policy: OracleAdmissionPolicyV1,
    mechanisms: OracleAdmissionMechanismCatalogV1,
    attempt: OracleAdmissionAttemptV1,
    evidence: OracleAdmissionEvidenceV1,
    outcome: OracleAdmissionOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateOracleContractFrozenPayload {
    oracle_workspace: OracleWorkspaceV1,
    proposal: OraclePortfolioProposalV1,
    outcome: OracleAdmissionOutcomeV1,
    contract: CandidateOracleContractV1,
    workspace: CandidateWorkspaceV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateProposalRequestFrozenPayload {
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: Box<ProposalStepRequestV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateProposalEpisodeAuthorizedPayload {
    request: ContentId<ProposalStepRequestArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateProposalRecordedPayload {
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: Box<ProposalStepRequestV1>,
    terminal_id: ContentId<ProposalStepTerminalArtifact>,
    terminal: Box<ProposalStepTerminalV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateBuildFrozenPayload {
    plan: CandidateBuildPlanV1,
    request: CandidateBuildRequestV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateBuildAuthorizedPayload {
    request: ContentId<CandidateBuildRequestArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateBuildObservedPayload {
    request: CandidateBuildRequestV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmissionAuthorizedPayload {
    contract: Box<CandidateOracleContractV1>,
    proposal: Box<CandidateProposalV1>,
    mechanisms: CandidateMechanismCatalogV1,
    attempt: CandidateAdmissionAttemptV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmissionRecordedPayload {
    contract: Box<CandidateOracleContractV1>,
    proposal: Box<CandidateProposalV1>,
    mechanisms: CandidateMechanismCatalogV1,
    attempt: CandidateAdmissionAttemptV1,
    evidence: CandidateAdmissionEvidenceV1,
    outcome: CandidateAdmissionOutcomeV1,
}

struct Projection {
    state: ControllerWorkflowStateV1,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    history: Vec<EventEnvelope>,
}

/// Freezes the exact task, proposal-step request, input, model, tool/capability and episode authority.
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
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: &ProposalStepRequestV1,
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

/// Durably cancels a task aggregate without deleting its history.
///
/// Oracle-admitted and Candidate-terminal tasks are already terminal and cannot be relabelled.
/// Reusing the same command identity is an idempotent replay.
pub fn cancel_controller_workflow<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = WorkflowCancelledPayload {};
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        WORKFLOW_CANCELLED,
        &payload,
    )? {
        return Ok(state);
    }
    if matches!(
        projection.state,
        ControllerWorkflowStateV1::NotFound
            | ControllerWorkflowStateV1::Cancelled
            | ControllerWorkflowStateV1::OracleAdmitted { .. }
            | ControllerWorkflowStateV1::Terminal { .. }
    ) {
        return Err(ControllerWorkflowError::InvalidTransition);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        WORKFLOW_CANCELLED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits durable start authority before the external Proposal step effect may run.
///
/// # Errors
///
/// Rejects an illegal transition, mismatched request, replay conflict, or persistence failure.
pub fn authorize_sir_episode<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: ContentId<ProposalStepRequestArtifact>,
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

/// Records a strictly validated SIR proposal step terminal as the proposal observation.
///
/// # Errors
///
/// Rejects request, terminal, recovery-input, model, episode, role, identity, replay, and durable
/// state drift.
#[allow(clippy::too_many_arguments)]
pub fn record_sir_proposal<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: &ProposalStepRequestV1,
    terminal_id: ContentId<ProposalStepTerminalArtifact>,
    terminal: &ProposalStepTerminalV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    terminal.validate_against(request).map_err(binding_error)?;
    if terminal.identity().map_err(binding_error)? != terminal_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let ProposalStepPublicationV1::Sir { proposal_id, .. } = terminal.publication() else {
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

/// Records that the exact authorized Intent Admission effect failed before publication.
///
/// # Errors
///
/// Rejects authority drift, illegal transitions, replay conflicts, or persistence failures.
#[allow(
    clippy::too_many_arguments,
    reason = "the event records every distinct frozen Intent Admission authority identity"
)]
pub fn record_intent_admission_blocked<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
    reason: IntentAdmissionBlockReasonV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = IntentAdmissionBlockedPayload {
        decision,
        executable,
        restricted_store,
        reason,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        INTENT_ADMISSION_BLOCKED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::IntentAdmissionAuthorized {
        decision: recorded_decision,
        executable: recorded_executable,
        restricted_store: recorded_store,
        ..
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if *recorded_decision != decision
        || *recorded_executable != executable
        || *recorded_store != restricted_store
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        INTENT_ADMISSION_BLOCKED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Reauthorizes a blocked Intent Admission operation against a newly frozen executable/store.
///
/// # Errors
///
/// Rejects decision drift, non-blocked state, replay conflicts, or persistence failures.
pub fn reauthorize_intent_admission<E: EventStore>(
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
    let ControllerWorkflowStateV1::IntentAdmissionBlocked {
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
        contract_body: contract.clone(),
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

/// Opens the exact task-owned Oracle Exploration workspace and initial obligation ledger.
///
/// This transition derives every claim × concern × required-role work item again at the
/// Controller boundary. Strategies may consume the resulting ledger only after this event is
/// durable.
///
/// # Errors
///
/// Rejects cross-task, admitted-intent, SIR-input, policy/catalog, claim, initial-ledger, replay,
/// or persistence drift.
#[allow(
    clippy::too_many_arguments,
    reason = "all authority-bearing Oracle artifacts and durable command metadata remain explicit"
)]
pub fn open_oracle_exploration<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    recovery_input: &IntentRecoveryInputV1,
    workspace: &OracleWorkspaceV1,
    policy: &OracleCoveragePolicyV1,
    catalog: &OracleStrategyCatalogV1,
    ledger: &OracleExplorationLedgerV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let (admitted_intent_outcome, admitted_intent, claims, authorized_recovery_input) =
        match &projection.state {
            ControllerWorkflowStateV1::AdmittedIntent {
                authority,
                outcome,
                contract,
                contract_body,
                ..
            } => (
                *outcome,
                *contract,
                derive_oracle_claims(workflow.task_id, *contract, contract_body.admitted_claim()),
                authority.recovery_input,
            ),
            ControllerWorkflowStateV1::OracleExplorationOpened(authority) => (
                authority.admitted_intent_outcome,
                authority.admitted_intent,
                authority.claims.clone(),
                authority.recovery_input,
            ),
            _ => return Err(ControllerWorkflowError::InvalidTransition),
        };
    let recovery_input_id = recovery_input.identity().map_err(binding_error)?;
    let workspace_id = workspace.identity().map_err(binding_error)?;
    let policy_id = policy.identity().map_err(binding_error)?;
    let catalog_id = catalog.identity().map_err(binding_error)?;
    let claim_ids = claims
        .iter()
        .map(|claim| claim.identity().map_err(binding_error))
        .collect::<Result<Vec<_>, _>>()?;
    if recovery_input_id != authorized_recovery_input
        || recovery_input.task_id() != workflow.task_id
        || workspace.task_id() != workflow.task_id
        || workspace.admitted_intent() != admitted_intent
        || workspace.sir_input() != recovery_input_id
        || workspace.sir_task_bundle() != recovery_input.task_bundle()
        || workspace.coverage_policy() != policy_id
        || workspace.strategy_catalog() != catalog_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let work_items = derive_oracle_work_items(&claim_ids, policy).map_err(binding_error)?;
    let expected_ledger = OracleExplorationLedgerV1::open(workspace_id, work_items, catalog)
        .map_err(binding_error)?;
    if *ledger != expected_ledger {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = OracleExplorationOpenedPayload {
        authority: FrozenOracleExplorationAuthorityV1 {
            task_id: workflow.task_id,
            admitted_intent_outcome,
            admitted_intent,
            recovery_input: recovery_input_id,
            workspace: workspace_id,
            coverage_policy: policy_id,
            strategy_catalog: catalog_id,
            claims,
            ledger: ledger.identity().map_err(binding_error)?,
            ledger_revision: ledger.revision(),
        },
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_EXPLORATION_OPENED,
        &payload,
    )? {
        return Ok(state);
    }
    if !matches!(
        projection.state,
        ControllerWorkflowStateV1::AdmittedIntent { .. }
    ) {
        return Err(ControllerWorkflowError::InvalidTransition);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_EXPLORATION_OPENED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits one exact cell-scoped strategy run and its running ledger revision before execution.
///
/// # Errors
///
/// Rejects workspace, catalog, ledger, item, strategy/executor, budget, replay, or persistence
/// drift.
#[allow(
    clippy::too_many_arguments,
    reason = "workspace, catalog, ledger, run, and command authority remain explicit"
)]
pub fn authorize_oracle_strategy<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    workspace: &OracleWorkspaceV1,
    catalog: &OracleStrategyCatalogV1,
    ledger: &OracleExplorationLedgerV1,
    run: &OracleStrategyRunV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let started_ledger = ledger
        .start_strategy(run, catalog, workspace.budget())
        .map_err(binding_error)?;
    let payload = OracleStrategyAuthorizedPayload {
        workspace: workspace.clone(),
        catalog: catalog.clone(),
        previous_ledger: ledger.clone(),
        run: run.clone(),
        started_ledger,
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleExplorationOpened(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if workspace.identity().map_err(binding_error)? != authority.workspace
        || catalog.identity().map_err(binding_error)? != authority.strategy_catalog
        || ledger.identity().map_err(binding_error)? != authority.ledger
        || ledger.revision() != authority.ledger_revision
        || run.workspace() != authority.workspace
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Durably projects Controller-produced effect observations into the active Oracle run.
///
/// # Errors
///
/// Rejects observation/run/ledger drift, illegal transitions, replay conflicts, or persistence
/// failure.
pub fn record_oracle_strategy_observations<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    ledger: &OracleExplorationLedgerV1,
    run: &OracleStrategyRunV1,
    observations: &[OracleExplorationObservationV1],
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let next_ledger = ledger
        .record_strategy_observations(
            run.item(),
            run.identity().map_err(binding_error)?,
            observations,
        )
        .map_err(binding_error)?;
    let payload = OracleStrategyObservationsRecordedPayload {
        previous_ledger: ledger.clone(),
        run: run.clone(),
        observations: observations.to_vec(),
        next_ledger,
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_OBSERVATIONS_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleStrategyAuthorized(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if ledger.identity().map_err(binding_error)? != authority.exploration.ledger
        || ledger.revision() != authority.exploration.ledger_revision
        || run.identity().map_err(binding_error)? != authority.run
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_OBSERVATIONS_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Atomically records one strict strategy submission and the resulting ledger revision.
///
/// # Errors
///
/// Rejects run/submission/workspace/ledger lineage drift, an illegal transition, replay conflict,
/// or persistence failure.
#[allow(
    clippy::too_many_arguments,
    reason = "exact active values and durable command authority remain explicit"
)]
pub fn record_oracle_strategy_completion<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    workspace: &OracleWorkspaceV1,
    ledger: &OracleExplorationLedgerV1,
    run: &OracleStrategyRunV1,
    completion: &OracleStrategyCompletionV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let submission = completion.submission(run)?;
    let next_ledger = ledger
        .apply_strategy_submission(run, submission, workspace)
        .map_err(binding_error)?;
    let payload = OracleStrategySubmissionRecordedPayload {
        workspace: workspace.clone(),
        previous_ledger: ledger.clone(),
        run: run.clone(),
        completion: completion.clone(),
        next_ledger,
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_SUBMISSION_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleStrategyAuthorized(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if workspace.identity().map_err(binding_error)? != authority.exploration.workspace
        || ledger.identity().map_err(binding_error)? != authority.exploration.ledger
        || ledger.revision() != authority.exploration.ledger_revision
        || run.identity().map_err(binding_error)? != authority.run
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_STRATEGY_SUBMISSION_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes a terminal exploration ledger as the exact independent Admission candidate.
///
/// # Errors
///
/// Rejects incomplete/changed ledger lineage, proposal or policy drift, illegal transitions,
/// replay conflicts, or persistence failure.
pub fn freeze_oracle_portfolio<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    ledger: &OracleExplorationLedgerV1,
    proposal: &OraclePortfolioProposalV1,
    policy: &OracleAdmissionPolicyV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if proposal != &OraclePortfolioProposalV1::freeze(ledger).map_err(binding_error)?
        || policy != &OracleAdmissionPolicyV1::strict()
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = OraclePortfolioFrozenPayload {
        ledger: ledger.clone(),
        proposal: proposal.clone(),
        policy: policy.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_PORTFOLIO_FROZEN,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleExplorationOpened(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if ledger.identity().map_err(binding_error)? != authority.ledger
        || ledger.revision() != authority.ledger_revision
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_PORTFOLIO_FROZEN,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes the exact qualified mechanisms and mechanically derived control obligations.
///
/// # Errors
///
/// Rejects portfolio/policy/catalog/attempt drift, illegal transitions, replay conflicts, or
/// persistence failure.
#[allow(
    clippy::too_many_arguments,
    reason = "all exact Admission authority bodies remain explicit at the durable boundary"
)]
pub fn authorize_oracle_admission<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    proposal: &OraclePortfolioProposalV1,
    policy: &OracleAdmissionPolicyV1,
    mechanisms: &OracleAdmissionMechanismCatalogV1,
    attempt: &OracleAdmissionAttemptV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if attempt
        != &OracleAdmissionAttemptV1::new(proposal, policy, mechanisms).map_err(binding_error)?
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = OracleAdmissionAuthorizedPayload {
        proposal: proposal.clone(),
        policy: policy.clone(),
        mechanisms: mechanisms.clone(),
        attempt: attempt.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_ADMISSION_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OraclePortfolioFrozen(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if proposal.identity().map_err(binding_error)? != authority.proposal
        || policy.identity().map_err(binding_error)? != authority.policy
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_ADMISSION_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits durable start authority for one exact qualified Oracle control.
#[allow(
    clippy::too_many_arguments,
    reason = "the event keeps the exact Admission, runner, prior evidence, and dispatch binding"
)]
pub(crate) fn authorize_oracle_control<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    mechanisms: &OracleAdmissionMechanismCatalogV1,
    attempt: &OracleAdmissionAttemptV1,
    previous_receipts: &[OracleControlReceiptV1],
    run: &OracleControlRunV1,
    dispatch: &OracleControlDispatchV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    dispatch.validate_against(run).map_err(binding_error)?;
    if run.attempt() != attempt.identity().map_err(binding_error)?
        || dispatch.run() != run.identity().map_err(binding_error)?
        || run
            != &OracleControlRunV1::new(attempt, mechanisms, run.obligation().clone())
                .map_err(binding_error)?
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let _ = OracleAdmissionEvidenceV1::new(attempt, previous_receipts.to_vec())
        .map_err(binding_error)?;
    let payload = OracleControlAuthorizedPayload {
        run: run.clone(),
        dispatch: dispatch.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_CONTROL_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let authority = match &projection.state {
        ControllerWorkflowStateV1::OracleAdmissionAuthorized(authority)
            if previous_receipts.is_empty() =>
        {
            authority
        }
        ControllerWorkflowStateV1::OracleControlsObserved {
            authority,
            receipts,
        } if receipts == previous_receipts => authority,
        _ => return Err(ControllerWorkflowError::InvalidTransition),
    };
    if mechanisms.identity().map_err(binding_error)? != authority.mechanisms
        || attempt.identity().map_err(binding_error)? != authority.attempt
        || mechanisms != authority.mechanism_catalog.as_ref()
        || attempt != authority.admission_attempt.as_ref()
        || previous_receipts.iter().any(|receipt| {
            receipt.item() == run.obligation().item()
                && receipt.control() == run.obligation().control()
        })
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_CONTROL_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records the trusted observation produced by the exact authorized Oracle control dispatch.
#[allow(
    clippy::too_many_arguments,
    reason = "the observation commit retains run, dispatch, receipt, and command authority"
)]
pub(crate) fn record_oracle_control_observation<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    run: &OracleControlRunV1,
    dispatch: &OracleControlDispatchV1,
    observation: &TrustedOracleControlObservationV1,
    receipt: &OracleControlReceiptV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    dispatch.validate_against(run).map_err(binding_error)?;
    observation
        .validate_against(dispatch)
        .map_err(binding_error)?;
    let payload = OracleControlObservedPayload {
        run: run.clone(),
        dispatch: dispatch.clone(),
        observation: observation.clone(),
        receipt: receipt.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_CONTROL_OBSERVED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleControlAuthorized {
        authority,
        previous_receipts,
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if authority.run != run.identity().map_err(binding_error)?
        || authority.dispatch != dispatch.identity().map_err(binding_error)?
        || receipt.proposal() != authority.admission.portfolio.proposal
        || receipt
            != &OracleControlReceiptV1::from_trusted_observation(
                authority.admission.portfolio.proposal,
                run,
                observation,
            )
            .map_err(binding_error)?
        || previous_receipts
            .iter()
            .any(|prior| prior.item() == receipt.item() && prior.control() == receipt.control())
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_CONTROL_OBSERVED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records trusted control evidence and the independently recomputed terminal claim portfolio.
///
/// # Errors
///
/// Rejects attempt/evidence/outcome drift, unqualified mechanisms, illegal transitions, replay
/// conflicts, or persistence failure.
#[allow(
    clippy::too_many_arguments,
    reason = "all exact Admission authority and evidence bodies remain explicit"
)]
pub fn record_oracle_admission_outcome<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    proposal: &OraclePortfolioProposalV1,
    policy: &OracleAdmissionPolicyV1,
    mechanisms: &OracleAdmissionMechanismCatalogV1,
    attempt: &OracleAdmissionAttemptV1,
    evidence: &OracleAdmissionEvidenceV1,
    outcome: &OracleAdmissionOutcomeV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let expected = recompute_oracle_admission(proposal, policy, mechanisms, attempt, evidence)
        .map_err(binding_error)?;
    if outcome != &expected {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = OracleAdmissionRecordedPayload {
        proposal: proposal.clone(),
        policy: policy.clone(),
        mechanisms: mechanisms.clone(),
        attempt: attempt.clone(),
        evidence: evidence.clone(),
        outcome: outcome.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        ORACLE_ADMISSION_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleControlsObserved {
        authority,
        receipts,
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if proposal.identity().map_err(binding_error)? != authority.portfolio.proposal
        || policy.identity().map_err(binding_error)? != authority.portfolio.policy
        || mechanisms.identity().map_err(binding_error)? != authority.mechanisms
        || attempt.identity().map_err(binding_error)? != authority.attempt
        || evidence.receipts() != receipts
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        ORACLE_ADMISSION_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes the exact admitted Oracle subset as immutable Candidate authority.
///
/// # Errors
///
/// Rejects partial/rejected claim promotion, proposal/outcome drift, illegal transitions, replay
/// conflicts, or persistence failure.
#[allow(
    clippy::too_many_arguments,
    reason = "exact Oracle workspace, proposal, outcome, Candidate contract, workspace, and durable command remain explicit"
)]
pub fn freeze_candidate_oracle_contract<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    oracle_workspace: &OracleWorkspaceV1,
    proposal: &OraclePortfolioProposalV1,
    outcome: &OracleAdmissionOutcomeV1,
    contract: &CandidateOracleContractV1,
    workspace: &CandidateWorkspaceV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if contract != &CandidateOracleContractV1::derive(proposal, outcome).map_err(binding_error)? {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    if workspace
        != &CandidateWorkspaceV1::derive(oracle_workspace, proposal, contract)
            .map_err(binding_error)?
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = CandidateOracleContractFrozenPayload {
        oracle_workspace: oracle_workspace.clone(),
        proposal: proposal.clone(),
        outcome: outcome.clone(),
        contract: contract.clone(),
        workspace: workspace.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ORACLE_CONTRACT_FROZEN,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::OracleAdmitted {
        authority,
        evidence: _,
        outcome: outcome_id,
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if proposal.identity().map_err(binding_error)? != authority.portfolio.proposal
        || oracle_workspace.identity().map_err(binding_error)?
            != authority.portfolio.exploration.workspace
        || outcome.identity().map_err(binding_error)? != *outcome_id
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ORACLE_CONTRACT_FROZEN,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes the exact Candidate proposal step request before a model effect may be authorized.
///
/// # Errors
///
/// Rejects identity, role, task, admitted-material, replay, or transition drift.
pub fn freeze_candidate_proposal_request<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: &ProposalStepRequestV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if request.identity().map_err(binding_error)? != request_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let projection = project(events, workflow)?;
    let payload = CandidateProposalRequestFrozenPayload {
        request_id,
        request: Box::new(request.clone()),
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PROPOSAL_REQUEST_FROZEN,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateOracleContractFrozen(authority) = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    validate_candidate_request(workflow, authority, request)?;
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PROPOSAL_REQUEST_FROZEN,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits Candidate Proposal step start authority before its external model effect.
///
/// # Errors
///
/// Rejects request drift, replay conflict, persistence failure, or an illegal transition.
pub fn authorize_candidate_proposal_episode<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: ContentId<ProposalStepRequestArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = CandidateProposalEpisodeAuthorizedPayload { request };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PROPOSAL_EPISODE_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateProposalRequestFrozen(authority) = &projection.state
    else {
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
        CANDIDATE_PROPOSAL_EPISODE_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records one strict task-generic Candidate proposal returned by the authorized Agent episode.
///
/// # Errors
///
/// Rejects request, terminal, role, episode, model, Oracle, proposal, replay, or state drift.
#[allow(clippy::too_many_arguments)]
pub fn record_candidate_proposal<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: &ProposalStepRequestV1,
    terminal_id: ContentId<ProposalStepTerminalArtifact>,
    terminal: &ProposalStepTerminalV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if request.identity().map_err(binding_error)? != request_id
        || terminal.identity().map_err(binding_error)? != terminal_id
        || terminal.validate_against(request).is_err()
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let ProposalStepPublicationV1::CandidateStrategy { .. } = terminal.publication() else {
        return Err(ControllerWorkflowError::BindingMismatch);
    };
    let projection = project(events, workflow)?;
    let payload = CandidateProposalRecordedPayload {
        request_id,
        request: Box::new(request.clone()),
        terminal_id,
        terminal: Box::new(terminal.clone()),
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PROPOSAL_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateProposalEpisodeAuthorized(authority) =
        &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if authority.request != request_id || authority.episode_id != terminal.episode_id() {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    validate_candidate_request(workflow, &authority.candidate, request)?;
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PROPOSAL_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes the exact product-owned Candidate build operation before Worker authorization.
#[allow(clippy::too_many_arguments)]
pub fn freeze_candidate_build<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    proposal: &CandidateProposalV1,
    plan: &CandidateBuildPlanV1,
    request: &CandidateBuildRequestV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let proposal_id = proposal.identity().map_err(binding_error)?;
    let plan_id = plan.identity().map_err(binding_error)?;
    if request.proposal() != proposal_id || request.plan() != plan_id {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = CandidateBuildFrozenPayload {
        plan: plan.clone(),
        request: request.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_BUILD_FROZEN,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateProposed {
        authority,
        terminal,
        proposal: expected,
    } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if *expected != proposal_id || proposal.oracle_contract() != authority.candidate.contract {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let _ = terminal;
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_BUILD_FROZEN,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Commits durable start authority for one exact Candidate build effect.
pub fn authorize_candidate_build<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: ContentId<CandidateBuildRequestArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let payload = CandidateBuildAuthorizedPayload { request };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_BUILD_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateBuildFrozen(authority) = &projection.state else {
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
        CANDIDATE_BUILD_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Records a trusted Worker receipt as an observation; it is not itself an admission verdict.
#[allow(clippy::too_many_arguments)]
pub fn record_candidate_build_observation<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    request: &CandidateBuildRequestV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    if ContentId::derive(&cairn_codec::to_vec(receipt).map_err(binding_error)?)
        .map_err(binding_error)?
        != receipt_id
        || receipt.job_id() != request.job_id()
        || receipt.contract_id() != request.contract()
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = CandidateBuildObservedPayload {
        request: request.clone(),
        receipt_id,
        receipt: receipt.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_BUILD_OBSERVED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateBuildAuthorized(authority) = &projection.state else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if request.identity().map_err(binding_error)? != authority.request {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_BUILD_OBSERVED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Freezes the complete claim × item × plane control matrix before Candidate evaluation effects.
#[allow(clippy::too_many_arguments)]
pub fn authorize_candidate_admission<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    contract: &CandidateOracleContractV1,
    proposal: &CandidateProposalV1,
    mechanisms: &CandidateMechanismCatalogV1,
    attempt: &CandidateAdmissionAttemptV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let expected =
        CandidateAdmissionAttemptV1::new(contract, proposal, mechanisms).map_err(binding_error)?;
    if attempt != &expected {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = CandidateAdmissionAuthorizedPayload {
        contract: Box::new(contract.clone()),
        proposal: Box::new(proposal.clone()),
        mechanisms: mechanisms.clone(),
        attempt: attempt.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ADMISSION_AUTHORIZED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateBuildObserved { authority, .. } = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if authority.proposal != proposal.identity().map_err(binding_error)?
        || authority.candidate.candidate.contract != contract.identity().map_err(binding_error)?
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ADMISSION_AUTHORIZED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

/// Recomputes and records the terminal migration outcome from trusted Candidate controls.
#[allow(clippy::too_many_arguments)]
pub fn record_candidate_admission_outcome<E: EventStore>(
    events: &mut E,
    workflow: &ControllerWorkflowV1,
    contract: &CandidateOracleContractV1,
    proposal: &CandidateProposalV1,
    mechanisms: &CandidateMechanismCatalogV1,
    attempt: &CandidateAdmissionAttemptV1,
    evidence: &CandidateAdmissionEvidenceV1,
    outcome: &CandidateAdmissionOutcomeV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let expected = recompute_candidate_admission(contract, proposal, mechanisms, attempt, evidence)
        .map_err(binding_error)?;
    if outcome != &expected {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    let payload = CandidateAdmissionRecordedPayload {
        contract: Box::new(contract.clone()),
        proposal: Box::new(proposal.clone()),
        mechanisms: mechanisms.clone(),
        attempt: attempt.clone(),
        evidence: evidence.clone(),
        outcome: outcome.clone(),
    };
    let projection = project(events, workflow)?;
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ADMISSION_RECORDED,
        &payload,
    )? {
        return Ok(state);
    }
    let ControllerWorkflowStateV1::CandidateAdmissionAuthorized(authority) = &projection.state
    else {
        return Err(ControllerWorkflowError::InvalidTransition);
    };
    if authority.mechanisms != mechanisms.identity().map_err(binding_error)?
        || authority.attempt != attempt.identity().map_err(binding_error)?
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    append_current(
        events,
        workflow,
        &projection,
        command_id,
        observed_at,
        CANDIDATE_ADMISSION_RECORDED,
        &payload,
    )?;
    recover_controller_workflow(events, workflow)
}

fn validate_candidate_request(
    workflow: &ControllerWorkflowV1,
    authority: &FrozenCandidateOracleAuthorityV1,
    request: &ProposalStepRequestV1,
) -> Result<(), ControllerWorkflowError> {
    let ProposalStepRoleRequestV1::CandidateStrategy {
        workspace,
        contract,
        oracle_materials,
        ..
    } = request.role()
    else {
        return Err(ControllerWorkflowError::BindingMismatch);
    };
    if workspace.task_id() != workflow.task_id
        || workspace.identity().map_err(binding_error)? != authority.workspace
        || contract.identity().map_err(binding_error)? != authority.contract
        || oracle_materials.validate_against(contract).is_err()
    {
        return Err(ControllerWorkflowError::BindingMismatch);
    }
    Ok(())
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
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: &ProposalStepRequestV1,
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
    request: &ProposalStepRequestV1,
    terminal_id: ContentId<ProposalStepTerminalArtifact>,
    terminal: &ProposalStepTerminalV1,
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
    let ProposalStepPublicationV1::Sir { proposal, .. } = terminal.publication() else {
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
        (state, WORKFLOW_CANCELLED)
            if !matches!(
                state,
                ControllerWorkflowStateV1::NotFound
                    | ControllerWorkflowStateV1::Cancelled
                    | ControllerWorkflowStateV1::OracleAdmitted { .. }
                    | ControllerWorkflowStateV1::Terminal { .. }
            ) =>
        {
            let _: WorkflowCancelledPayload = decode(bytes)?;
            Ok(ControllerWorkflowStateV1::Cancelled)
        }
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
            INTENT_ADMISSION_BLOCKED,
        ) => {
            let payload: IntentAdmissionBlockedPayload = decode(bytes)?;
            if payload.decision != decision
                || payload.executable != executable
                || payload.restricted_store != restricted_store
            {
                return Err(invalid_history(
                    "Intent Admission blocked authority changed",
                ));
            }
            Ok(ControllerWorkflowStateV1::IntentAdmissionBlocked {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
                executable,
                restricted_store,
                reason: payload.reason,
            })
        }
        (
            ControllerWorkflowStateV1::IntentAdmissionBlocked {
                authority,
                terminal,
                proposal,
                requests,
                request,
                authority_grant,
                decision,
                ..
            },
            INTENT_ADMISSION_AUTHORIZED,
        ) => {
            let payload: IntentAdmissionAuthorizedPayload = decode(bytes)?;
            if payload.decision != decision {
                return Err(invalid_history(
                    "Intent Admission reauthorization changed decision",
                ));
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
                contract_body: Box::new(payload.contract_body),
            })
        }
        (
            ControllerWorkflowStateV1::AdmittedIntent {
                authority,
                outcome,
                contract,
                contract_body,
                ..
            },
            ORACLE_EXPLORATION_OPENED,
        ) => {
            let payload: OracleExplorationOpenedPayload = decode(bytes)?;
            let oracle = payload.authority;
            if contract_body.identity().map_err(binding_error)? != contract
                || contract_body.task_id() != task_id
            {
                return Err(invalid_history("admitted intent contract body changed"));
            }
            let expected_claims =
                derive_oracle_claims(task_id, contract, contract_body.admitted_claim());
            if oracle.task_id != task_id
                || oracle.admitted_intent_outcome != outcome
                || oracle.admitted_intent != contract
                || oracle.recovery_input != authority.recovery_input
                || oracle.claims != expected_claims
                || oracle.ledger_revision.get() != 1
            {
                return Err(invalid_history(
                    "Oracle Exploration authority changed or is noncanonical",
                ));
            }
            Ok(ControllerWorkflowStateV1::OracleExplorationOpened(oracle))
        }
        (
            ControllerWorkflowStateV1::OracleExplorationOpened(mut authority),
            ORACLE_STRATEGY_AUTHORIZED,
        ) => {
            let payload: OracleStrategyAuthorizedPayload = decode(bytes)?;
            let workspace_id = payload
                .workspace
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let catalog_id = payload
                .catalog
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let previous_id = payload
                .previous_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let run_id = payload
                .run
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected = payload
                .previous_ledger
                .start_strategy(&payload.run, &payload.catalog, payload.workspace.budget())
                .map_err(|error| invalid_history(error.to_string()))?;
            if workspace_id != authority.workspace
                || catalog_id != authority.strategy_catalog
                || previous_id != authority.ledger
                || payload.previous_ledger.revision() != authority.ledger_revision
                || payload.run.workspace() != authority.workspace
                || payload.started_ledger != expected
            {
                return Err(invalid_history("Oracle strategy start authority changed"));
            }
            let started_id = payload
                .started_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let previous_ledger = authority.ledger;
            authority.ledger = started_id;
            authority.ledger_revision = payload.started_ledger.revision();
            Ok(ControllerWorkflowStateV1::OracleStrategyAuthorized(
                FrozenOracleStrategyAuthorityV1 {
                    exploration: authority,
                    previous_ledger,
                    run: run_id,
                },
            ))
        }
        (
            ControllerWorkflowStateV1::OracleStrategyAuthorized(mut strategy_authority),
            ORACLE_STRATEGY_OBSERVATIONS_RECORDED,
        ) => {
            let payload: OracleStrategyObservationsRecordedPayload = decode(bytes)?;
            let previous_id = payload
                .previous_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let run_id = payload
                .run
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected = payload
                .previous_ledger
                .record_strategy_observations(payload.run.item(), run_id, &payload.observations)
                .map_err(|error| invalid_history(error.to_string()))?;
            if previous_id != strategy_authority.exploration.ledger
                || payload.previous_ledger.revision()
                    != strategy_authority.exploration.ledger_revision
                || run_id != strategy_authority.run
                || payload.next_ledger != expected
            {
                return Err(invalid_history("Oracle strategy observations changed"));
            }
            strategy_authority.exploration.ledger = payload
                .next_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            strategy_authority.exploration.ledger_revision = payload.next_ledger.revision();
            Ok(ControllerWorkflowStateV1::OracleStrategyAuthorized(
                strategy_authority,
            ))
        }
        (
            ControllerWorkflowStateV1::OracleStrategyAuthorized(strategy_authority),
            ORACLE_STRATEGY_SUBMISSION_RECORDED,
        ) => {
            let payload: OracleStrategySubmissionRecordedPayload = decode(bytes)?;
            let workspace_id = payload
                .workspace
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let previous_id = payload
                .previous_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let run_id = payload
                .run
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected = payload
                .previous_ledger
                .apply_strategy_submission(
                    &payload.run,
                    payload.completion.submission(&payload.run)?,
                    &payload.workspace,
                )
                .map_err(|error| invalid_history(error.to_string()))?;
            if workspace_id != strategy_authority.exploration.workspace
                || previous_id != strategy_authority.exploration.ledger
                || payload.previous_ledger.revision()
                    != strategy_authority.exploration.ledger_revision
                || run_id != strategy_authority.run
                || payload.next_ledger != expected
            {
                return Err(invalid_history("Oracle strategy submission changed"));
            }
            let mut authority = strategy_authority.exploration;
            authority.ledger = payload
                .next_ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            authority.ledger_revision = payload.next_ledger.revision();
            Ok(ControllerWorkflowStateV1::OracleExplorationOpened(
                authority,
            ))
        }
        (
            ControllerWorkflowStateV1::OracleExplorationOpened(exploration),
            ORACLE_PORTFOLIO_FROZEN,
        ) => {
            let payload: OraclePortfolioFrozenPayload = decode(bytes)?;
            let ledger_id = payload
                .ledger
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected_proposal = OraclePortfolioProposalV1::freeze(&payload.ledger)
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected_policy = OracleAdmissionPolicyV1::strict();
            if ledger_id != exploration.ledger
                || payload.ledger.revision() != exploration.ledger_revision
                || payload.proposal != expected_proposal
                || payload.policy != expected_policy
            {
                return Err(invalid_history(
                    "frozen Oracle portfolio or admission policy changed",
                ));
            }
            Ok(ControllerWorkflowStateV1::OraclePortfolioFrozen(
                FrozenOraclePortfolioAuthorityV1 {
                    exploration,
                    proposal: payload
                        .proposal
                        .identity()
                        .map_err(|error| invalid_history(error.to_string()))?,
                    policy: payload
                        .policy
                        .identity()
                        .map_err(|error| invalid_history(error.to_string()))?,
                },
            ))
        }
        (
            ControllerWorkflowStateV1::OraclePortfolioFrozen(portfolio),
            ORACLE_ADMISSION_AUTHORIZED,
        ) => {
            let payload: OracleAdmissionAuthorizedPayload = decode(bytes)?;
            let proposal_id = payload
                .proposal
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let policy_id = payload
                .policy
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let mechanism_id = payload
                .mechanisms
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected_attempt = OracleAdmissionAttemptV1::new(
                &payload.proposal,
                &payload.policy,
                &payload.mechanisms,
            )
            .map_err(|error| invalid_history(error.to_string()))?;
            if proposal_id != portfolio.proposal
                || policy_id != portfolio.policy
                || payload.attempt != expected_attempt
            {
                return Err(invalid_history("Oracle Admission start authority changed"));
            }
            Ok(ControllerWorkflowStateV1::OracleAdmissionAuthorized(
                FrozenOracleAdmissionAuthorityV1 {
                    portfolio,
                    mechanisms: mechanism_id,
                    attempt: payload.attempt.identity().map_err(binding_error)?,
                    mechanism_catalog: Box::new(payload.mechanisms),
                    admission_attempt: Box::new(payload.attempt),
                },
            ))
        }
        (
            ControllerWorkflowStateV1::OracleAdmissionAuthorized(authority),
            ORACLE_CONTROL_AUTHORIZED,
        ) => apply_oracle_control_authorized(authority, Vec::new(), bytes),
        (
            ControllerWorkflowStateV1::OracleControlsObserved {
                authority,
                receipts,
            },
            ORACLE_CONTROL_AUTHORIZED,
        ) => apply_oracle_control_authorized(authority, receipts, bytes),
        (
            ControllerWorkflowStateV1::OracleControlAuthorized {
                authority,
                previous_receipts,
            },
            ORACLE_CONTROL_OBSERVED,
        ) => {
            let payload: OracleControlObservedPayload = decode(bytes)?;
            payload
                .dispatch
                .validate_against(&payload.run)
                .map_err(binding_error)?;
            payload
                .observation
                .validate_against(&payload.dispatch)
                .map_err(binding_error)?;
            let expected_receipt = OracleControlReceiptV1::from_trusted_observation(
                authority.admission.portfolio.proposal,
                &payload.run,
                &payload.observation,
            )
            .map_err(binding_error)?;
            if payload.run.identity().map_err(binding_error)? != authority.run
                || payload.dispatch.identity().map_err(binding_error)? != authority.dispatch
                || payload.receipt != expected_receipt
                || previous_receipts.iter().any(|receipt| {
                    receipt.item() == payload.receipt.item()
                        && receipt.control() == payload.receipt.control()
                })
            {
                return Err(invalid_history("Oracle control observation changed"));
            }
            let mut receipts = previous_receipts;
            receipts.push(payload.receipt);
            let attempt = payload.run.attempt();
            if attempt != authority.admission.attempt {
                return Err(invalid_history("Oracle control attempt changed"));
            }
            Ok(ControllerWorkflowStateV1::OracleControlsObserved {
                authority: authority.admission,
                receipts,
            })
        }
        (
            ControllerWorkflowStateV1::OracleControlsObserved {
                authority,
                receipts,
            },
            ORACLE_ADMISSION_RECORDED,
        ) => {
            let payload: OracleAdmissionRecordedPayload = decode(bytes)?;
            let proposal_id = payload
                .proposal
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let policy_id = payload
                .policy
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let mechanisms_id = payload
                .mechanisms
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let attempt_id = payload
                .attempt
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected_outcome = recompute_oracle_admission(
                &payload.proposal,
                &payload.policy,
                &payload.mechanisms,
                &payload.attempt,
                &payload.evidence,
            )
            .map_err(|error| invalid_history(error.to_string()))?;
            if proposal_id != authority.portfolio.proposal
                || policy_id != authority.portfolio.policy
                || mechanisms_id != authority.mechanisms
                || attempt_id != authority.attempt
                || payload.evidence.receipts() != receipts
                || payload.outcome != expected_outcome
            {
                return Err(invalid_history("Oracle Admission outcome changed"));
            }
            Ok(ControllerWorkflowStateV1::OracleAdmitted {
                authority,
                evidence: payload
                    .evidence
                    .identity()
                    .map_err(|error| invalid_history(error.to_string()))?,
                outcome: payload
                    .outcome
                    .identity()
                    .map_err(|error| invalid_history(error.to_string()))?,
            })
        }
        (
            ControllerWorkflowStateV1::OracleAdmitted {
                authority,
                evidence,
                outcome,
            },
            CANDIDATE_ORACLE_CONTRACT_FROZEN,
        ) => {
            let payload: CandidateOracleContractFrozenPayload = decode(bytes)?;
            let proposal_id = payload
                .proposal
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let outcome_id = payload
                .outcome
                .identity()
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected = CandidateOracleContractV1::derive(&payload.proposal, &payload.outcome)
                .map_err(|error| invalid_history(error.to_string()))?;
            let expected_workspace = CandidateWorkspaceV1::derive(
                &payload.oracle_workspace,
                &payload.proposal,
                &payload.contract,
            )
            .map_err(|error| invalid_history(error.to_string()))?;
            if proposal_id != authority.portfolio.proposal
                || payload
                    .oracle_workspace
                    .identity()
                    .map_err(|error| invalid_history(error.to_string()))?
                    != authority.portfolio.exploration.workspace
                || outcome_id != outcome
                || payload.contract != expected
                || payload.workspace != expected_workspace
            {
                return Err(invalid_history("Candidate Oracle authority changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateOracleContractFrozen(
                FrozenCandidateOracleAuthorityV1 {
                    oracle: authority,
                    evidence,
                    outcome,
                    contract: payload
                        .contract
                        .identity()
                        .map_err(|error| invalid_history(error.to_string()))?,
                    workspace: payload
                        .workspace
                        .identity()
                        .map_err(|error| invalid_history(error.to_string()))?,
                },
            ))
        }
        (
            ControllerWorkflowStateV1::CandidateOracleContractFrozen(candidate),
            CANDIDATE_PROPOSAL_REQUEST_FROZEN,
        ) => {
            let payload: CandidateProposalRequestFrozenPayload = decode(bytes)?;
            if payload.request.identity().map_err(binding_error)? != payload.request_id {
                return Err(invalid_history(
                    "Candidate proposal step request identity changed",
                ));
            }
            let workflow = ControllerWorkflowV1::new(task_id)?;
            validate_candidate_request(&workflow, &candidate, &payload.request)?;
            Ok(ControllerWorkflowStateV1::CandidateProposalRequestFrozen(
                FrozenCandidateProposalAuthorityV1 {
                    candidate,
                    request: payload.request_id,
                    episode_id: payload.request.runtime().episode_id(),
                },
            ))
        }
        (
            ControllerWorkflowStateV1::CandidateProposalRequestFrozen(authority),
            CANDIDATE_PROPOSAL_EPISODE_AUTHORIZED,
        ) => {
            let payload: CandidateProposalEpisodeAuthorizedPayload = decode(bytes)?;
            if payload.request != authority.request {
                return Err(invalid_history("authorized Candidate request changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateProposalEpisodeAuthorized(authority))
        }
        (
            ControllerWorkflowStateV1::CandidateProposalEpisodeAuthorized(authority),
            CANDIDATE_PROPOSAL_RECORDED,
        ) => {
            let payload: CandidateProposalRecordedPayload = decode(bytes)?;
            if payload.request_id != authority.request
                || payload.request.identity().map_err(binding_error)? != payload.request_id
                || payload.terminal.identity().map_err(binding_error)? != payload.terminal_id
                || payload.terminal.validate_against(&payload.request).is_err()
                || payload.terminal.episode_id() != authority.episode_id
            {
                return Err(invalid_history("Candidate proposal step terminal changed"));
            }
            let workflow = ControllerWorkflowV1::new(task_id)?;
            validate_candidate_request(&workflow, &authority.candidate, &payload.request)?;
            let ProposalStepPublicationV1::CandidateStrategy { proposal_id, .. } =
                payload.terminal.publication()
            else {
                return Err(invalid_history(
                    "Candidate proposal step returned another role",
                ));
            };
            Ok(ControllerWorkflowStateV1::CandidateProposed {
                authority,
                terminal: payload.terminal_id,
                proposal: *proposal_id,
            })
        }
        (
            ControllerWorkflowStateV1::CandidateProposed {
                authority,
                terminal,
                proposal,
            },
            CANDIDATE_BUILD_FROZEN,
        ) => {
            let payload: CandidateBuildFrozenPayload = decode(bytes)?;
            let plan = payload.plan.identity().map_err(binding_error)?;
            let request = payload.request.identity().map_err(binding_error)?;
            if payload.request.proposal() != proposal || payload.request.plan() != plan {
                return Err(invalid_history("Candidate build authority changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateBuildFrozen(
                FrozenCandidateBuildAuthorityV1 {
                    candidate: authority,
                    terminal,
                    proposal,
                    plan,
                    request,
                },
            ))
        }
        (
            ControllerWorkflowStateV1::CandidateBuildFrozen(authority),
            CANDIDATE_BUILD_AUTHORIZED,
        ) => {
            let payload: CandidateBuildAuthorizedPayload = decode(bytes)?;
            if payload.request != authority.request {
                return Err(invalid_history("authorized Candidate build changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateBuildAuthorized(
                authority,
            ))
        }
        (
            ControllerWorkflowStateV1::CandidateBuildAuthorized(authority),
            CANDIDATE_BUILD_OBSERVED,
        ) => {
            let payload: CandidateBuildObservedPayload = decode(bytes)?;
            let receipt: ContentId<ExecutionReceiptArtifact> =
                ContentId::derive(&cairn_codec::to_vec(&payload.receipt).map_err(binding_error)?)
                    .map_err(binding_error)?;
            if payload.request.identity().map_err(binding_error)? != authority.request
                || receipt != payload.receipt_id
                || payload.receipt.job_id() != payload.request.job_id()
                || payload.receipt.contract_id() != payload.request.contract()
            {
                return Err(invalid_history("Candidate build observation changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateBuildObserved {
                authority,
                receipt,
                outcome: payload.receipt.outcome(),
            })
        }
        (
            ControllerWorkflowStateV1::CandidateBuildObserved {
                authority, receipt, ..
            },
            CANDIDATE_ADMISSION_AUTHORIZED,
        ) => {
            let payload: CandidateAdmissionAuthorizedPayload = decode(bytes)?;
            let expected_attempt = CandidateAdmissionAttemptV1::new(
                &payload.contract,
                &payload.proposal,
                &payload.mechanisms,
            )
            .map_err(binding_error)?;
            if payload.contract.identity().map_err(binding_error)?
                != authority.candidate.candidate.contract
                || payload.proposal.identity().map_err(binding_error)? != authority.proposal
                || payload.attempt != expected_attempt
                || payload.mechanisms.identity().map_err(binding_error)?
                    != payload.attempt.mechanisms()
            {
                return Err(invalid_history("Candidate Admission authority changed"));
            }
            Ok(ControllerWorkflowStateV1::CandidateAdmissionAuthorized(
                FrozenCandidateAdmissionAuthorityV1 {
                    build: authority,
                    receipt,
                    mechanisms: payload.mechanisms.identity().map_err(binding_error)?,
                    attempt: payload.attempt.identity().map_err(binding_error)?,
                },
            ))
        }
        (
            ControllerWorkflowStateV1::CandidateAdmissionAuthorized(authority),
            CANDIDATE_ADMISSION_RECORDED,
        ) => {
            let payload: CandidateAdmissionRecordedPayload = decode(bytes)?;
            let expected_outcome = recompute_candidate_admission(
                &payload.contract,
                &payload.proposal,
                &payload.mechanisms,
                &payload.attempt,
                &payload.evidence,
            )
            .map_err(binding_error)?;
            if payload.contract.identity().map_err(binding_error)?
                != authority.build.candidate.candidate.contract
                || payload.proposal.identity().map_err(binding_error)? != authority.build.proposal
                || payload.mechanisms.identity().map_err(binding_error)? != authority.mechanisms
                || payload.attempt.identity().map_err(binding_error)? != authority.attempt
                || payload.evidence.identity().map_err(binding_error)? != payload.outcome.evidence()
                || payload.outcome.proposal() != authority.build.proposal
                || payload.outcome != expected_outcome
            {
                return Err(invalid_history("Candidate Admission outcome changed"));
            }
            let status = terminal_status(&payload.outcome);
            Ok(ControllerWorkflowStateV1::Terminal {
                authority,
                evidence: payload.evidence.identity().map_err(binding_error)?,
                outcome: payload.outcome.identity().map_err(binding_error)?,
                status,
            })
        }
        (_, _) => Err(invalid_history(
            "illegal Controller workflow event transition",
        )),
    }
}

fn apply_oracle_control_authorized(
    authority: FrozenOracleAdmissionAuthorityV1,
    current_receipts: Vec<OracleControlReceiptV1>,
    bytes: &[u8],
) -> Result<ControllerWorkflowStateV1, ControllerWorkflowError> {
    let payload: OracleControlAuthorizedPayload = decode(bytes)?;
    let run_id = payload.run.identity().map_err(binding_error)?;
    let dispatch_id = payload.dispatch.identity().map_err(binding_error)?;
    let expected_run = OracleControlRunV1::new(
        &authority.admission_attempt,
        &authority.mechanism_catalog,
        payload.run.obligation().clone(),
    )
    .map_err(binding_error)?;
    payload
        .dispatch
        .validate_against(&payload.run)
        .map_err(binding_error)?;
    let _ = OracleAdmissionEvidenceV1::new(&authority.admission_attempt, current_receipts.clone())
        .map_err(binding_error)?;
    if payload.run != expected_run || payload.dispatch.run() != run_id {
        return Err(invalid_history("Oracle control start authority changed"));
    }
    Ok(ControllerWorkflowStateV1::OracleControlAuthorized {
        authority: FrozenOracleControlAuthorityV1 {
            admission: authority,
            run: run_id,
            dispatch: dispatch_id,
        },
        previous_receipts: current_receipts,
    })
}

fn terminal_status(outcome: &CandidateAdmissionOutcomeV1) -> MigrationTerminalStatusV1 {
    if outcome
        .claims()
        .iter()
        .any(|claim| claim.status() == CandidateClaimStatusV1::Rejected)
    {
        MigrationTerminalStatusV1::Rejected
    } else if outcome
        .claims()
        .iter()
        .any(|claim| claim.status() == CandidateClaimStatusV1::Partial)
    {
        MigrationTerminalStatusV1::Partial
    } else {
        MigrationTerminalStatusV1::Admitted
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
    let outcome = events.append(
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
    let event_id = outcome
        .event_ids
        .first()
        .copied()
        .ok_or_else(|| invalid_history("event store returned an empty append outcome"))?;
    let sequence = outcome.first_sequence;
    let sequence_number = sequence.get();
    let was_replay = outcome.was_replay;
    tracing::info!(
        target: "cairn.server.controller-workflow",
        event = "controller_workflow_event_committed",
        task_id = %workflow.task_id,
        command_id = %command_id,
        event_id = %event_id,
        sequence = sequence_number,
        schema,
        was_replay,
        "Controller workflow transition committed"
    );
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
    use cairn_execution::{
        CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, DiagnosticByteLimit,
        DockerImageId, EvidenceByteLimit, ExecutionElapsedMillis, ExecutionEvidenceArtifact,
        ExecutionReceipt, ExecutionStderrArtifact, ExecutionStdoutArtifact, ExecutionTimeoutMillis,
        JobContractArtifact, NetworkPolicy, OutputByteLimit, WorkerPoolName,
    };
    use cairn_protocol::{AttemptId, ContentType, EpisodeId, JobId};
    use cairn_store_sqlite::SqliteEventStore;
    use cairn_verification::ReferenceArtifact;
    use serde_json::{Value, json};

    use super::*;
    use cairn_admission::{
        IntentAdmissionRestrictedStoreArtifact, TaskIntentAuthoritySubject,
        UserIntentAuthorityGrantV1, UserIntentAuthorityScopeV1, UserIntentDecisionResponseV1,
        UserIntentDecisionV1, promote_user_intent,
    };
    use cairn_migration::{
        AgentResolvedRuntimeModelArtifact, CandidateBuildPlanV1, CandidateControlFamilyV1,
        CandidateControlImplementationArtifact, CandidateControlReceiptV1,
        CandidateControlResultV1, CandidateMechanismCatalogV1, CandidateMechanismProvenanceV1,
        CandidateOracleElementMaterialV1, CandidateOracleMaterialV1, CandidateOracleMaterialsV1,
        CandidateProposalSubmissionV1, CandidateProposalV1, CandidateQualifiedMechanismV1,
        IntentHypothesisSetProposalV1, IntentRecoveryRequestV1, OracleAdversarialPolicyV1,
        OracleBuildTestSnapshotArtifact, OracleControlFamilyV1, OracleControlReceiptV1,
        OracleControlResultV1, OracleControlRunnerArtifact, OracleCoverageProfileV1,
        OracleDocumentationSnapshotArtifact, OracleExperimentLimit,
        OracleExperimentToolCatalogArtifact, OracleExplorationBudgetV1,
        OracleExplorationCapabilityGrantArtifact, OracleKnowledgeSnapshotArtifact,
        OracleMechanismQualificationReceiptArtifact, OracleObservationPayloadV1,
        OraclePortfolioElementKindV1, OraclePortfolioElementV1, OracleQualifiedMechanismArtifact,
        OracleQualifiedMechanismRegistrationV1, OracleResearchToolCatalogArtifact,
        OracleSourceSnapshotArtifact, OracleStrategyExecutorV1,
        OracleStrategyImplementationArtifact, OracleStrategyKindV1, OracleStrategyName,
        OracleStrategyRegistrationV1, OracleStrategyRoleV1, OracleStrategyRunLimit,
        OracleStrategySubmissionOutcomeV1, OracleStrategySubmissionV1, OracleWorkspaceInput,
        ProposalStepOracleBuildTestsV1, ProposalStepOracleDocumentationV1,
        ProposalStepOracleKnowledgeV1, ProposalStepOracleMaterialsV1, ProposalStepRoleRequestV1,
        ProposalStepRuntimeV1, ProposalStepTaskSnapshotV1, ProposalStepTaskSourceV1,
        SirCallerClaimId, SirHypothesisId, SirSourceLineCount, SirTaskArtifactBytes,
        SirTaskArtifactPath, SirTaskArtifactV1, SirTaskBundleV1, SirTaskLimits,
        TrustedCandidateControlReceiptArtifact, WorkflowToolControllerObservationArtifact,
        derive_user_intent_decision_requests, prepare_generic_candidate_build_job,
    };

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    #[derive(Serialize)]
    struct ExecutionReceiptWire {
        schema_version: u16,
        job_id: JobId,
        attempt_id: AttemptId,
        contract_id: ContentId<JobContractArtifact>,
        outcome: ExecutionOutcome,
        exit_code: Option<i32>,
        elapsed_ms: ExecutionElapsedMillis,
        stdout_id: ContentId<ExecutionStdoutArtifact>,
        stderr_id: ContentId<ExecutionStderrArtifact>,
        evidence_id: ContentId<ExecutionEvidenceArtifact>,
        outputs: Vec<cairn_execution::ArchivedOutput>,
    }

    fn execution_receipt(
        job_id: JobId,
        attempt_id: AttemptId,
        contract_id: ContentId<JobContractArtifact>,
        label: &[u8],
    ) -> ExecutionReceipt {
        let wire = ExecutionReceiptWire {
            schema_version: 1,
            job_id,
            attempt_id,
            contract_id,
            outcome: ExecutionOutcome::Succeeded,
            exit_code: Some(0),
            elapsed_ms: ExecutionElapsedMillis::new(1),
            stdout_id: id(label),
            stderr_id: id(label),
            evidence_id: id(label),
            outputs: Vec::new(),
        };
        cairn_codec::from_slice(&cairn_codec::to_vec(&wire).expect("encode receipt"))
            .expect("decode receipt")
    }

    fn request(task_id: TaskId) -> (ProposalStepRequestV1, IntentRecoveryInputV1) {
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
        let runtime = ProposalStepRuntimeV1::new(
            EpisodeId::new(),
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
        let request = ProposalStepRequestV1::new(
            runtime,
            ProposalStepRoleRequestV1::Sir {
                task_id,
                recovery_request,
                task: ProposalStepTaskSnapshotV1::new(
                    bundle,
                    vec![ProposalStepTaskSourceV1::new(path, source)],
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
    fn durable_intent_path_opens_oracle_and_rejects_authority_drift() {
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
        let drifted_terminal: ProposalStepTerminalV1 = serde_json::from_value(json!({
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
        let terminal: ProposalStepTerminalV1 = serde_json::from_value(json!({
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
                ProposalStepPublicationV1::Sir { proposal, .. } => proposal,
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
            UserIntentAuthorityScopeV1::new(vec![selection_claim.clone()]).expect("scope"),
        );
        let cross_task_grant_id = cross_task_grant.identity().expect("cross-task grant id");
        let cross_task_decision = UserIntentDecisionV1::new(
            user_request_id,
            cross_task_grant_id,
            UserIntentDecisionResponseV1::SelectHypothesis {
                hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
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
            UserIntentAuthorityScopeV1::new(vec![selection_claim.clone()]).expect("scope"),
        );
        let grant_id = grant.identity().expect("grant id");
        let decision = UserIntentDecisionV1::new(
            user_request_id,
            grant_id,
            UserIntentDecisionResponseV1::SelectHypothesis {
                hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
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
        assert!(matches!(
            reauthorize_intent_admission(
                &mut events,
                &workflow,
                decision_id,
                id::<IntentAdmissionExecutableArtifact>(b"new executable"),
                restricted_store,
                &CommandId::new(),
                ObservedAtUnixMillis::new(6),
            ),
            Err(ControllerWorkflowError::InvalidTransition)
        ));
        let state = record_intent_admission_blocked(
            &mut events,
            &workflow,
            decision_id,
            executable,
            restricted_store,
            IntentAdmissionBlockReasonV1::InvocationDrift,
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("record blocked Admission");
        assert_eq!(state.next_action(), ControllerWorkflowNextActionV1::None);
        let executable = id::<IntentAdmissionExecutableArtifact>(b"new executable");
        let state = reauthorize_intent_admission(
            &mut events,
            &workflow,
            decision_id,
            executable,
            restricted_store,
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("reauthorize blocked Admission");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunIntentAdmission {
                decision: decision_id,
                executable,
                restricted_store,
            }
        );

        let admitted = promote_user_intent(
            proposal_id,
            match terminal.publication() {
                ProposalStepPublicationV1::Sir { proposal, .. } => proposal,
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
            ControllerWorkflowNextActionV1::AwaitOracleExplorationWorkspace {
                outcome: outcome_id,
                contract: outcome.contract().identity().expect("contract id"),
            }
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("admission restart recovery"),
            state
        );

        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::NotRequired,
        );
        let catalog = OracleStrategyCatalogV1::new(vec![
            OracleStrategyRegistrationV1::new(
                OracleStrategyName::new("deterministic-synthesis").expect("strategy name"),
                OracleStrategyKindV1::DeterministicAnalyzer,
                OracleStrategyExecutorV1::Deterministic {
                    implementation: id::<OracleStrategyImplementationArtifact>(
                        b"deterministic implementation",
                    ),
                },
                vec![OracleStrategyRoleV1::Synthesis],
                policy.concerns().to_vec(),
            )
            .expect("strategy registration"),
        ])
        .expect("strategy catalog");
        let contract_id = outcome.contract().identity().expect("contract id");
        let claim = derive_oracle_claims(task_id, contract_id, outcome.contract().admitted_claim())
            .into_iter()
            .next()
            .expect("current V1 admitted claim");
        let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
            task_id,
            admitted_intent: contract_id,
            sir_input: recovery_input_id,
            sir_task_bundle: recovery_input.task_bundle(),
            source: id::<OracleSourceSnapshotArtifact>(b"source snapshot"),
            documentation: id::<OracleDocumentationSnapshotArtifact>(b"documentation snapshot"),
            build_and_tests: id::<OracleBuildTestSnapshotArtifact>(b"build and tests snapshot"),
            knowledge: id::<OracleKnowledgeSnapshotArtifact>(b"knowledge snapshot"),
            research_tools: id::<OracleResearchToolCatalogArtifact>(b"research tools"),
            experiment_tools: id::<OracleExperimentToolCatalogArtifact>(b"experiment tools"),
            capability_grant: id::<OracleExplorationCapabilityGrantArtifact>(b"capability grant"),
            coverage_policy: policy.identity().expect("policy id"),
            strategy_catalog: catalog.identity().expect("catalog id"),
            budget: OracleExplorationBudgetV1 {
                strategy_runs: OracleStrategyRunLimit::new(64).expect("strategy budget"),
                experiments: OracleExperimentLimit::new(32).expect("experiment budget"),
            },
        });
        let claim_id = claim.identity().expect("claim id");
        let work_items = derive_oracle_work_items(&[claim_id], &policy).expect("work items");
        let ledger = OracleExplorationLedgerV1::open(
            workspace.identity().expect("workspace id"),
            work_items,
            &catalog,
        )
        .expect("initial ledger");
        let open_command = CommandId::new();
        let state = open_oracle_exploration(
            &mut events,
            &workflow,
            &recovery_input,
            &workspace,
            &policy,
            &catalog,
            &ledger,
            &open_command,
            ObservedAtUnixMillis::new(8),
        )
        .expect("open Oracle Exploration");
        let ControllerWorkflowNextActionV1::RunOracleExploration(oracle) = state.next_action()
        else {
            panic!("expected Oracle Exploration strategy consumer");
        };
        assert_eq!(oracle.task_id(), task_id);
        assert_eq!(
            oracle.workspace(),
            workspace.identity().expect("workspace id")
        );
        assert_eq!(oracle.ledger(), ledger.identity().expect("ledger id"));
        assert_eq!(oracle.claims(), std::slice::from_ref(&claim));
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("Oracle restart recovery"),
            state
        );
        assert_eq!(
            open_oracle_exploration(
                &mut events,
                &workflow,
                &recovery_input,
                &workspace,
                &policy,
                &catalog,
                &ledger,
                &open_command,
                ObservedAtUnixMillis::new(8),
            )
            .expect("exact Oracle opening replay"),
            state
        );

        let work_item = ledger.entries()[0].item().clone();
        let run = OracleStrategyRunV1::new(
            workspace.identity().expect("workspace id"),
            &work_item,
            OracleStrategyName::new("deterministic-synthesis").expect("strategy"),
            &catalog,
        )
        .expect("strategy run");
        let authorize_strategy_command = CommandId::new();
        let state = authorize_oracle_strategy(
            &mut events,
            &workflow,
            &workspace,
            &catalog,
            &ledger,
            &run,
            &authorize_strategy_command,
            ObservedAtUnixMillis::new(9),
        )
        .expect("authorize Oracle strategy");
        let ControllerWorkflowNextActionV1::RunOracleStrategy(strategy_authority) =
            state.next_action()
        else {
            panic!("expected one authorized Oracle strategy run");
        };
        assert_eq!(strategy_authority.run(), run.identity().expect("run id"));
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("strategy restart recovery"),
            state
        );
        assert_eq!(
            authorize_oracle_strategy(
                &mut events,
                &workflow,
                &workspace,
                &catalog,
                &ledger,
                &run,
                &authorize_strategy_command,
                ObservedAtUnixMillis::new(9),
            )
            .expect("exact strategy authority replay"),
            state
        );

        let started_ledger = ledger
            .start_strategy(&run, &catalog, workspace.budget())
            .expect("started ledger");
        let controller_observation =
            id::<WorkflowToolControllerObservationArtifact>(b"Controller observation");
        let payload = OracleObservationPayloadV1::new(
            controller_observation,
            json!({"observed": "typed effect result"}),
        );
        let observation = OracleExplorationObservationV1::workflow_tool(
            run.item(),
            run.identity().expect("run id"),
            controller_observation,
            &payload,
        )
        .expect("Oracle observation");
        let observation_command = CommandId::new();
        let state = record_oracle_strategy_observations(
            &mut events,
            &workflow,
            &started_ledger,
            &run,
            std::slice::from_ref(&observation),
            &observation_command,
            ObservedAtUnixMillis::new(10),
        )
        .expect("record typed strategy observation");
        assert!(matches!(
            state,
            ControllerWorkflowStateV1::OracleStrategyAuthorized(_)
        ));
        let observed_ledger = started_ledger
            .record_strategy_observations(
                run.item(),
                run.identity().expect("run id"),
                std::slice::from_ref(&observation),
            )
            .expect("observed ledger");
        assert_eq!(
            record_oracle_strategy_observations(
                &mut events,
                &workflow,
                &started_ledger,
                &run,
                std::slice::from_ref(&observation),
                &observation_command,
                ObservedAtUnixMillis::new(10),
            )
            .expect("exact observation replay"),
            state
        );
        let element = OraclePortfolioElementV1::new(
            run.item(),
            run.identity().expect("run id"),
            OraclePortfolioElementKindV1::Reference(id::<ReferenceArtifact>(b"reference")),
            vec![observation.identity().expect("observation id")],
        )
        .expect("portfolio element");
        let mut candidate_elements = vec![element.clone()];
        let submission = OracleStrategySubmissionV1::new(
            &run,
            OracleStrategySubmissionOutcomeV1::Contribute {
                elements: vec![element],
            },
        )
        .expect("strategy submission");
        let OracleStrategyExecutorV1::Deterministic { implementation } = run.executor() else {
            panic!("expected deterministic test strategy");
        };
        let completion = OracleStrategyCompletionV1::Deterministic {
            implementation: *implementation,
            submission,
        };
        let submission_command = CommandId::new();
        let state = record_oracle_strategy_completion(
            &mut events,
            &workflow,
            &workspace,
            &observed_ledger,
            &run,
            &completion,
            &submission_command,
            ObservedAtUnixMillis::new(11),
        )
        .expect("record strategy submission");
        let ControllerWorkflowNextActionV1::RunOracleExploration(oracle) = state.next_action()
        else {
            panic!("expected next Oracle cell selection");
        };
        assert_eq!(oracle.ledger_revision().get(), 4);
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("submission restart recovery"),
            state
        );
        assert_eq!(
            record_oracle_strategy_completion(
                &mut events,
                &workflow,
                &workspace,
                &observed_ledger,
                &run,
                &completion,
                &submission_command,
                ObservedAtUnixMillis::new(11),
            )
            .expect("exact strategy submission replay"),
            state
        );

        let mut terminal_ledger = observed_ledger
            .apply_strategy_submission(
                &run,
                completion.submission(&run).expect("first submission"),
                &workspace,
            )
            .expect("first terminal cell");
        let mut observed_millis = 12;
        loop {
            let next = terminal_ledger
                .next_action(&catalog, workspace.budget())
                .expect("select remaining Oracle work");
            let cairn_migration::OracleExplorationNextActionV1::RunStrategy {
                item,
                eligible_strategies,
            } = next
            else {
                assert_eq!(
                    next,
                    cairn_migration::OracleExplorationNextActionV1::FreezePortfolio
                );
                break;
            };
            let next_run = OracleStrategyRunV1::new(
                workspace.identity().expect("workspace id"),
                &item,
                eligible_strategies[0].clone(),
                &catalog,
            )
            .expect("remaining strategy run");
            authorize_oracle_strategy(
                &mut events,
                &workflow,
                &workspace,
                &catalog,
                &terminal_ledger,
                &next_run,
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("authorize remaining strategy");
            observed_millis += 1;
            let started = terminal_ledger
                .start_strategy(&next_run, &catalog, workspace.budget())
                .expect("start remaining strategy");
            let gap = OraclePortfolioElementV1::new(
                next_run.item(),
                next_run.identity().expect("run id"),
                OraclePortfolioElementKindV1::Reference(id::<ReferenceArtifact>(
                    b"remaining reference",
                )),
                vec![],
            )
            .expect("remaining coverage gap");
            candidate_elements.push(gap.clone());
            let next_submission = OracleStrategySubmissionV1::new(
                &next_run,
                OracleStrategySubmissionOutcomeV1::Contribute {
                    elements: vec![gap],
                },
            )
            .expect("remaining submission");
            let OracleStrategyExecutorV1::Deterministic { implementation } = next_run.executor()
            else {
                panic!("expected deterministic strategy");
            };
            let next_completion = OracleStrategyCompletionV1::Deterministic {
                implementation: *implementation,
                submission: next_submission,
            };
            let state = record_oracle_strategy_completion(
                &mut events,
                &workflow,
                &workspace,
                &started,
                &next_run,
                &next_completion,
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("record remaining strategy");
            observed_millis += 1;
            assert!(matches!(
                state,
                ControllerWorkflowStateV1::OracleExplorationOpened(_)
            ));
            terminal_ledger = started
                .apply_strategy_submission(
                    &next_run,
                    next_completion
                        .submission(&next_run)
                        .expect("remaining completion"),
                    &workspace,
                )
                .expect("advance remaining ledger");
        }

        let portfolio =
            OraclePortfolioProposalV1::freeze(&terminal_ledger).expect("freeze complete portfolio");
        let admission_policy = OracleAdmissionPolicyV1::strict();
        let freeze_portfolio_command = CommandId::new();
        let state = freeze_oracle_portfolio(
            &mut events,
            &workflow,
            &terminal_ledger,
            &portfolio,
            &admission_policy,
            &freeze_portfolio_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("durably freeze portfolio");
        let ControllerWorkflowNextActionV1::AwaitOracleAdmissionMechanisms(portfolio_authority) =
            state.next_action()
        else {
            panic!("expected qualified Admission mechanisms");
        };
        assert_eq!(
            portfolio_authority.proposal(),
            portfolio.identity().expect("portfolio id")
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("portfolio restart"),
            state
        );
        assert_eq!(
            freeze_oracle_portfolio(
                &mut events,
                &workflow,
                &terminal_ledger,
                &portfolio,
                &admission_policy,
                &freeze_portfolio_command,
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("exact portfolio replay"),
            state
        );
        observed_millis += 1;

        let mechanisms = OracleAdmissionMechanismCatalogV1::new(
            admission_policy
                .required_controls()
                .iter()
                .map(|control| {
                    let label = match control {
                        OracleControlFamilyV1::MechanismQualification => {
                            b"qualification".as_slice()
                        }
                        OracleControlFamilyV1::Honest => b"honest".as_slice(),
                        OracleControlFamilyV1::Mutant => b"mutant".as_slice(),
                        OracleControlFamilyV1::Hidden => b"hidden".as_slice(),
                        OracleControlFamilyV1::Bypass => b"bypass".as_slice(),
                    };
                    OracleQualifiedMechanismRegistrationV1::new(
                        *control,
                        id::<OracleQualifiedMechanismArtifact>(label),
                        id::<OracleControlRunnerArtifact>(label),
                        id::<OracleMechanismQualificationReceiptArtifact>(label),
                    )
                })
                .collect(),
        )
        .expect("qualified mechanisms");
        let attempt = OracleAdmissionAttemptV1::new(&portfolio, &admission_policy, &mechanisms)
            .expect("Admission attempt");
        let authorize_admission_command = CommandId::new();
        let state = authorize_oracle_admission(
            &mut events,
            &workflow,
            &portfolio,
            &admission_policy,
            &mechanisms,
            &attempt,
            &authorize_admission_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("authorize independent Admission");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunOracleAdmissionControls { receipts, .. }
                if receipts.is_empty()
        ));
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("attempt restart"),
            state
        );
        observed_millis += 1;

        let proposal_id = portfolio.identity().expect("proposal id");
        assert!(!attempt.required_controls().is_empty());
        let mut receipts = Vec::new();
        for obligation in attempt.required_controls() {
            let run = OracleControlRunV1::new(&attempt, &mechanisms, obligation.clone())
                .expect("qualified control run");
            let receipt_label =
                format!("{}:{:?}", obligation.item().to_wire(), obligation.control());
            let job_id = JobId::new();
            let worker_attempt_id = AttemptId::new();
            let contract_id = id::<JobContractArtifact>(receipt_label.as_bytes());
            let dispatch = OracleControlDispatchV1::new(
                &run,
                cairn_migration::OracleControlWorkerBindingV1::new(
                    job_id,
                    worker_attempt_id,
                    contract_id,
                ),
            )
            .expect("qualified control dispatch");
            let state = authorize_oracle_control(
                &mut events,
                &workflow,
                &mechanisms,
                &attempt,
                &receipts,
                &run,
                &dispatch,
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("commit control start authority");
            assert!(matches!(
                state.next_action(),
                ControllerWorkflowNextActionV1::ExecuteOracleAdmissionControl { .. }
            ));
            observed_millis += 1;

            let execution_receipt = execution_receipt(
                job_id,
                worker_attempt_id,
                contract_id,
                receipt_label.as_bytes(),
            );
            let execution_receipt_id = ContentId::<ExecutionReceiptArtifact>::derive(
                &cairn_codec::to_vec(&execution_receipt).expect("encode trusted receipt"),
            )
            .expect("trusted execution receipt id");
            let observation = TrustedOracleControlObservationV1::new(
                &dispatch,
                execution_receipt_id,
                execution_receipt,
                OracleControlResultV1::Passed,
            )
            .expect("trusted control observation");
            let receipt =
                OracleControlReceiptV1::from_trusted_observation(proposal_id, &run, &observation)
                    .expect("mechanical Admission receipt");
            let state = record_oracle_control_observation(
                &mut events,
                &workflow,
                &run,
                &dispatch,
                &observation,
                &receipt,
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("record trusted control observation");
            observed_millis += 1;
            receipts.push(receipt);
            assert!(matches!(
                state.next_action(),
                ControllerWorkflowNextActionV1::RunOracleAdmissionControls {
                    receipts: durable,
                    ..
                } if durable == receipts
            ));
        }
        let evidence =
            OracleAdmissionEvidenceV1::new(&attempt, receipts).expect("complete control receipts");
        let admission_outcome = recompute_oracle_admission(
            &portfolio,
            &admission_policy,
            &mechanisms,
            &attempt,
            &evidence,
        )
        .expect("independent recomputation");
        let record_admission_command = CommandId::new();
        let state = record_oracle_admission_outcome(
            &mut events,
            &workflow,
            &portfolio,
            &admission_policy,
            &mechanisms,
            &attempt,
            &evidence,
            &admission_outcome,
            &record_admission_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("record independent Admission outcome");
        assert_eq!(state.next_action(), ControllerWorkflowNextActionV1::None);
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("Admission outcome restart"),
            state
        );
        assert_eq!(
            record_oracle_admission_outcome(
                &mut events,
                &workflow,
                &portfolio,
                &admission_policy,
                &mechanisms,
                &attempt,
                &evidence,
                &admission_outcome,
                &record_admission_command,
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("exact Admission outcome replay"),
            state
        );
        observed_millis += 1;

        let candidate_contract = CandidateOracleContractV1::derive(&portfolio, &admission_outcome)
            .expect("derive admitted Candidate authority");
        let candidate_workspace =
            CandidateWorkspaceV1::derive(&workspace, &portfolio, &candidate_contract)
                .expect("derive Candidate workspace");
        let freeze_candidate_command = CommandId::new();
        let state = freeze_candidate_oracle_contract(
            &mut events,
            &workflow,
            &workspace,
            &portfolio,
            &admission_outcome,
            &candidate_contract,
            &candidate_workspace,
            &freeze_candidate_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("freeze Candidate Oracle contract");
        let ControllerWorkflowNextActionV1::AwaitCandidateProposalLoop(candidate_authority) =
            state.next_action()
        else {
            panic!("expected Candidate Proposal Loop boundary");
        };
        assert_eq!(
            candidate_authority.contract(),
            candidate_contract
                .identity()
                .expect("Candidate contract id")
        );
        assert_eq!(
            candidate_authority.workspace(),
            candidate_workspace
                .identity()
                .expect("Candidate workspace id")
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("Candidate authority restart"),
            state
        );
        assert_eq!(
            freeze_candidate_oracle_contract(
                &mut events,
                &workflow,
                &workspace,
                &portfolio,
                &admission_outcome,
                &candidate_contract,
                &candidate_workspace,
                &freeze_candidate_command,
                ObservedAtUnixMillis::new(observed_millis),
            )
            .expect("exact Candidate authority replay"),
            state
        );
        observed_millis += 1;

        let candidate_materials = CandidateOracleMaterialsV1::new(
            &candidate_contract,
            vec![claim.clone()],
            candidate_elements
                .into_iter()
                .map(|element| {
                    let bytes = match element.kind() {
                        OraclePortfolioElementKindV1::Reference(identity)
                            if *identity == id::<ReferenceArtifact>(b"reference") =>
                        {
                            b"reference".to_vec()
                        }
                        OraclePortfolioElementKindV1::Reference(identity)
                            if *identity == id::<ReferenceArtifact>(b"remaining reference") =>
                        {
                            b"remaining reference".to_vec()
                        }
                        _ => panic!("unexpected test Oracle material"),
                    };
                    let material =
                        CandidateOracleMaterialV1::from_portfolio_kind(element.kind(), bytes)
                            .expect("typed Candidate Oracle material");
                    CandidateOracleElementMaterialV1::new(element, material)
                        .expect("Candidate element material")
                })
                .collect(),
        )
        .expect("complete Candidate Oracle bodies");
        let ProposalStepRoleRequestV1::Sir { task, .. } = request.role() else {
            unreachable!()
        };
        let candidate_episode = EpisodeId::new();
        let candidate_model = id::<AgentResolvedRuntimeModelArtifact>(b"Candidate model");
        let candidate_runtime = ProposalStepRuntimeV1::new(
            candidate_episode,
            candidate_model,
            ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("recorded-candidate-model").expect("model"),
                deployment: DeploymentName::new("candidate-isolated").expect("deployment"),
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
        let candidate_request = ProposalStepRequestV1::new(
            candidate_runtime,
            ProposalStepRoleRequestV1::CandidateStrategy {
                workspace: candidate_workspace.clone(),
                contract: candidate_contract.clone(),
                oracle_materials: candidate_materials,
                task: task.clone(),
                public_materials: ProposalStepOracleMaterialsV1::new(
                    ProposalStepOracleDocumentationV1::new(
                        workspace.documentation(),
                        "documentation snapshot".into(),
                    )
                    .expect("Candidate documentation"),
                    ProposalStepOracleBuildTestsV1::new(
                        workspace.build_and_tests(),
                        "build and tests snapshot".into(),
                    )
                    .expect("Candidate build/tests"),
                    ProposalStepOracleKnowledgeV1::new(
                        workspace.knowledge(),
                        "knowledge snapshot".into(),
                    )
                    .expect("Candidate knowledge"),
                ),
            },
        )
        .expect("Candidate proposal step request");
        let candidate_request_id = candidate_request.identity().expect("Candidate request id");
        let freeze_request_command = CommandId::new();
        let state = freeze_candidate_proposal_request(
            &mut events,
            &workflow,
            candidate_request_id,
            &candidate_request,
            &freeze_request_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("freeze Candidate request");
        let ControllerWorkflowNextActionV1::AuthorizeCandidateProposalEpisode(
            candidate_proposal_authority,
        ) = state.next_action()
        else {
            panic!("expected Candidate start authorization");
        };
        assert_eq!(candidate_proposal_authority.request(), candidate_request_id);
        observed_millis += 1;

        let authorize_candidate_command = CommandId::new();
        let state = authorize_candidate_proposal_episode(
            &mut events,
            &workflow,
            candidate_request_id,
            &authorize_candidate_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("authorize Candidate episode");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunCandidateProposalEpisode(_)
        ));
        observed_millis += 1;

        let candidate_submission: CandidateProposalSubmissionV1 = serde_json::from_value(json!({
            "schema_version": 1,
            "files": [{
                "path": "src/operator.asc",
                "source": "#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void operator_kernel() {}\n"
            }],
            "primary_source": "src/operator.asc",
            "explanation": "Complete proposal bound only to the frozen admitted portfolio."
        }))
        .expect("Candidate submission");
        let candidate_proposal = CandidateProposalV1::new(
            candidate_contract
                .identity()
                .expect("Candidate contract id"),
            candidate_episode,
            candidate_model,
            candidate_submission,
        )
        .expect("Candidate proposal");
        let candidate_proposal_id = candidate_proposal
            .identity()
            .expect("Candidate proposal id");
        let candidate_terminal: ProposalStepTerminalV1 = serde_json::from_value(json!({
            "schema_version": 1,
            "request": candidate_request_id,
            "episode_id": candidate_episode,
            "publication": {
                "role": "candidate-strategy",
                "proposal_id": candidate_proposal_id,
                "proposal": candidate_proposal,
            },
            "completion_reason": "yielded",
            "steps_started": 2
        }))
        .expect("Candidate terminal");
        let candidate_terminal_id = candidate_terminal
            .identity()
            .expect("Candidate terminal id");
        let record_candidate_command = CommandId::new();
        let state = record_candidate_proposal(
            &mut events,
            &workflow,
            candidate_request_id,
            &candidate_request,
            candidate_terminal_id,
            &candidate_terminal,
            &record_candidate_command,
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("record Candidate proposal");
        assert_eq!(
            state.next_action(),
            ControllerWorkflowNextActionV1::AwaitCandidateBuild {
                authority: candidate_proposal_authority,
                terminal: candidate_terminal_id,
                proposal: candidate_proposal_id,
            }
        );
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("Candidate terminal restart"),
            state
        );

        observed_millis += 1;
        let plan = CandidateBuildPlanV1::new(
            DockerImageId::new(format!("sha256:{}", "c".repeat(64))).expect("image"),
            b"#!/bin/sh\nset -eu\ntrue\n".to_vec(),
            vec![WorkerPoolName::new("generic-build").expect("pool")],
            vec![CapabilityRequirement {
                name: CapabilityName::new("execution.role").expect("capability"),
                value: CapabilityValue::new("build").expect("value"),
            }],
            ExecutionTimeoutMillis::new(30_000).expect("timeout"),
            CapturePolicy::new(
                OutputByteLimit::new(4_096).expect("stdout"),
                OutputByteLimit::new(4_096).expect("stderr"),
                DiagnosticByteLimit::new(4_096).expect("diagnostic"),
                EvidenceByteLimit::new(4_096).expect("evidence"),
                Vec::new(),
            )
            .expect("capture"),
            NetworkPolicy::Disabled,
        )
        .expect("generic build plan");
        let proposal_bytes = cairn_codec::to_vec(&candidate_proposal).expect("proposal bytes");
        let prepared = prepare_generic_candidate_build_job(
            cairn_protocol::JobId::new(),
            &proposal_bytes,
            candidate_proposal_id,
            plan.clone(),
        )
        .expect("generic build material");
        let state = freeze_candidate_build(
            &mut events,
            &workflow,
            &candidate_proposal,
            &plan,
            prepared.request(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("freeze build");
        let ControllerWorkflowNextActionV1::AuthorizeCandidateBuild(build_authority) =
            state.next_action()
        else {
            panic!("expected build authorization");
        };
        observed_millis += 1;
        let state = authorize_candidate_build(
            &mut events,
            &workflow,
            build_authority.request(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("authorize build");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::RunCandidateBuild(_)
        ));

        observed_millis += 1;
        let receipt_value = json!({
            "schema_version":1,
            "job_id":prepared.request().job_id(),
            "attempt_id":cairn_protocol::AttemptId::new(),
            "contract_id":prepared.request().contract(),
            "outcome":"succeeded",
            "exit_code":0,
            "elapsed_ms":10,
            "stdout_id":id::<ExecutionStdoutArtifact>(b"candidate stdout"),
            "stderr_id":id::<ExecutionStderrArtifact>(b"candidate stderr"),
            "evidence_id":id::<ExecutionEvidenceArtifact>(b"candidate build evidence"),
            "outputs":[]
        });
        let receipt_bytes = cairn_codec::to_vec(&receipt_value).expect("receipt bytes");
        let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
        let receipt_id = ContentId::derive(&receipt_bytes).expect("receipt id");
        let state = record_candidate_build_observation(
            &mut events,
            &workflow,
            prepared.request(),
            receipt_id,
            &receipt,
            &CommandId::new(),
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("record build observation");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::AwaitCandidateAdmissionMechanisms { .. }
        ));

        let families = [
            CandidateControlFamilyV1::SourceBuild,
            CandidateControlFamilyV1::StaticAnalysis,
            CandidateControlFamilyV1::ExecuteObservation,
            CandidateControlFamilyV1::SemanticComparison,
            CandidateControlFamilyV1::Safety,
            CandidateControlFamilyV1::Performance,
        ];
        let mechanisms = CandidateMechanismCatalogV1::new(
            families
                .into_iter()
                .map(|family| {
                    CandidateQualifiedMechanismV1::new(
                        family,
                        id::<CandidateControlImplementationArtifact>(
                            format!("{family:?}").as_bytes(),
                        ),
                        CandidateMechanismProvenanceV1::Worker,
                    )
                })
                .collect(),
        )
        .expect("mechanisms");
        let attempt =
            CandidateAdmissionAttemptV1::new(&candidate_contract, &candidate_proposal, &mechanisms)
                .expect("attempt");
        observed_millis += 1;
        let state = authorize_candidate_admission(
            &mut events,
            &workflow,
            &candidate_contract,
            &candidate_proposal,
            &mechanisms,
            &attempt,
            &CommandId::new(),
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("authorize Candidate Admission");
        let ControllerWorkflowNextActionV1::AwaitCandidateControlReceipts(admission_authority) =
            state.next_action()
        else {
            panic!("expected Candidate controls");
        };
        assert_eq!(admission_authority.receipt(), receipt_id);

        let receipts = attempt
            .obligations()
            .iter()
            .enumerate()
            .map(|(index, obligation)| {
                CandidateControlReceiptV1::new(
                    obligation.item(),
                    obligation.family(),
                    obligation.mechanism(),
                    id::<TrustedCandidateControlReceiptArtifact>(
                        format!("candidate control {index}").as_bytes(),
                    ),
                    CandidateControlResultV1::Passed,
                )
            })
            .collect();
        let evidence =
            CandidateAdmissionEvidenceV1::new(&attempt, receipts).expect("Candidate evidence");
        let outcome = recompute_candidate_admission(
            &candidate_contract,
            &candidate_proposal,
            &mechanisms,
            &attempt,
            &evidence,
        )
        .expect("Candidate outcome");
        observed_millis += 1;
        let state = record_candidate_admission_outcome(
            &mut events,
            &workflow,
            &candidate_contract,
            &candidate_proposal,
            &mechanisms,
            &attempt,
            &evidence,
            &outcome,
            &CommandId::new(),
            ObservedAtUnixMillis::new(observed_millis),
        )
        .expect("record Candidate outcome");
        assert!(matches!(
            state.next_action(),
            ControllerWorkflowNextActionV1::Terminal {
                status: MigrationTerminalStatusV1::Admitted,
                ..
            }
        ));
        assert_eq!(
            recover_controller_workflow(&events, &workflow).expect("terminal restart"),
            state
        );
    }
}
