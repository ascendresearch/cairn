//! CUDA migration product composition above the domain-neutral server host.

mod app_api;
mod oracle_control_runner;
mod runtime_agent;

pub use app_api::{
    CudaMigrationProductModuleV1, MigrationAppApiError, MigrationAppApiV1,
    MigrationProductServicesV1, SubmittedMigrationTaskV1, migration_product_boundary,
};
pub use oracle_control_runner::{
    OracleControlRunnerError, OracleControlRunnerV1, OracleControlWorkerConfigV1,
};
pub use runtime_agent::{
    MigrationAgentRuntimeError, MigrationAgentRuntimeExecutorV1, MigrationRuntimeMaterialsV1,
    migration_tool_registry,
};

use std::{collections::BTreeMap, fmt::Display, future::Future, num::NonZeroU16};

use cairn_agent::{
    AgentLoopCheckpointV1, AgentLoopContext, AgentLoopHooks, AgentLoopRunError,
    AgentLoopRunOutcomeV1, AgentLoopStartV1, AgentLoopStepExecutor, AgentLoopStepLimit,
    AgentRegistries, InitializedAgentLoopV1, KnowledgeRegistry, SkillRegistry, ToolRegistry,
    TransportFailureClass, initialize_agent_loop, run_agent_loop,
};
use cairn_migration::{
    CandidateAdmissionAttemptV1, CandidateAdmissionDispositionV1,
    CandidateAdmissionEvidenceArtifact, CandidateAdmissionEvidenceV1,
    CandidateAdmissionOutcomeArtifact, CandidateAdmissionOutcomeV1, CandidateClaimStatusV1,
    CandidateExplorationAgentContextV1, CandidateExplorationRoleHooksV1,
    CandidateMechanismCatalogV1, CandidateOracleContractV1, CandidateProposalArtifact,
    CandidateProposalV1, CandidateReviewAgentContextV1, CandidateReviewRoleHooksV1,
    CandidateRevisionAgentContextV1, CandidateRevisionRoleHooksV1, CandidateWorkspaceV1,
    CudaMigrationWorkflow, IntentAdmissionPublicOutcomeArtifact, IntentDecisionMaterialV1,
    IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    MigrationRoleHooksV1, MigrationRoleStepObservationV1, OracleAcceptedItemV1,
    OracleAdmissionAttemptV1, OracleAdmissionDispositionV1, OracleAdmissionEvidenceV1,
    OracleAdmissionMechanismCatalogV1, OracleAdmissionOutcomeArtifact, OracleAdmissionOutcomeV1,
    OracleAdmissionPolicyV1, OracleClaimAdmissionStatusV1, OracleCoherentPortfolioV1,
    OracleControlReconciliationRequestV1, OracleDimensionItemDiscoveryAgentContextV1,
    OracleDimensionItemDiscoveryRoleHooksV1, OracleDimensionItemSetProposalV1,
    OracleDimensionItemSetReviewDecisionV1, OracleDimensionItemSetReviewV1,
    OracleDimensionItemSetReviewerAgentContextV1, OracleDimensionItemSetReviewerRoleHooksV1,
    OracleDimensionV1, OracleItemDeveloperAgentContextV1, OracleItemDeveloperRoleHooksV1,
    OracleItemDevelopmentLineageV1, OracleItemDiscoveryLineageV1, OracleItemDraftV1,
    OracleItemReviewDecisionV1, OracleItemReviewV1, OracleItemReviewerAgentContextV1,
    OracleItemReviewerRoleHooksV1, OracleItemV1, OraclePortfolioCoherenceDecisionV1,
    OraclePortfolioCoherenceReviewV1, OraclePortfolioCoherenceReviewerAgentContextV1,
    OraclePortfolioCoherenceReviewerRoleHooksV1, OraclePortfolioProposalV1,
    OracleReviewDispositionV1, OracleRevisionRequestV1, OracleWholePortfolioAgentContextV1,
    OracleWholePortfolioLineageV1, OracleWholePortfolioProposalAuthorityV1,
    OracleWholePortfolioRoleHooksV1, OracleWorkspaceV1, PreparedIntentAdmissionV1,
    ReasoningDecompositionPolicyV1, SirAgentContextV1, SirRoleHooksV1, SirTaskWorkspace,
    UserIntentAuthorityGrantV1, UserIntentDecisionRequestV1, UserIntentDecisionV1,
    derive_user_intent_decision_requests, promote_user_intent, recompute_candidate_admission,
    recompute_oracle_admission,
};
use cairn_protocol::{AgentLoopId, ContentId, ContentType, TaskId};
use cairn_server::{ApplicationModule, ApplicationName};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

mod evidence_experiment_runner;
pub use evidence_experiment_runner::EvidenceExperimentWorkerConfigV1;

/// Exact task snapshot admitted at the product composition boundary.
#[derive(Clone)]
pub struct FrozenMigrationTaskV1 {
    task_id: TaskId,
    workspace: SirTaskWorkspace,
    recovery_input: IntentRecoveryInputV1,
    reasoning_decomposition: ReasoningDecompositionPolicyV1,
}

impl FrozenMigrationTaskV1 {
    /// Binds one task lifecycle to its exact source bundle and SIR recovery input.
    ///
    /// # Errors
    ///
    /// Rejects task or bundle identity drift.
    pub fn new(
        task_id: TaskId,
        workspace: SirTaskWorkspace,
        recovery_input: IntentRecoveryInputV1,
        reasoning_decomposition: ReasoningDecompositionPolicyV1,
    ) -> Result<Self, MigrationApplicationError> {
        let bundle = workspace
            .bundle()
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if recovery_input.task_id() != task_id || recovery_input.task_bundle() != bundle {
            return Err(MigrationApplicationError::Binding("frozen task"));
        }
        Ok(Self {
            task_id,
            workspace,
            recovery_input,
            reasoning_decomposition,
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn workspace(&self) -> &SirTaskWorkspace {
        &self.workspace
    }

    #[must_use]
    pub const fn recovery_input(&self) -> &IntentRecoveryInputV1 {
        &self.recovery_input
    }

    /// Returns the exact reasoning topology frozen for this task run.
    #[must_use]
    pub const fn reasoning_decomposition(&self) -> ReasoningDecompositionPolicyV1 {
        self.reasoning_decomposition
    }
}

/// Administrator response plus the exact Controller-issued authority that permits it.
pub struct AuthorizedIntentDecisionV1 {
    request: UserIntentDecisionRequestV1,
    grant: UserIntentAuthorityGrantV1,
    decision: UserIntentDecisionV1,
}

/// Complete, batch-ordered set of administrator decisions required by one SIR proposal.
pub struct AuthorizedIntentDecisionSetV1 {
    decisions: Vec<AuthorizedIntentDecisionV1>,
}

impl AuthorizedIntentDecisionSetV1 {
    fn new(
        requests: &IntentDecisionRequestBatchV1,
        decisions: Vec<AuthorizedIntentDecisionV1>,
    ) -> Result<Self, MigrationApplicationError> {
        if decisions.len() != requests.requests().len()
            || requests
                .requests()
                .iter()
                .zip(&decisions)
                .any(|(request, decision)| request != &decision.request)
        {
            return Err(MigrationApplicationError::Binding(
                "administrator intent decision set",
            ));
        }
        Ok(Self { decisions })
    }
}

/// Product request boundary that exposes only the exact task identity needed for lifecycle facts.
pub trait MigrationTaskRequest {
    fn task_id(&self) -> TaskId;
}

/// Stable, non-sensitive failure class exposed to product lifecycle handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationWorkflowFailureClassV1 {
    ProductService,
    OracleSemanticMechanismUnavailable,
    Domain,
    AuthorityBinding,
    AgentLoopInitialization,
    AgentLoopExecution,
    AgentLoopExhausted,
    AgentLoopSuspended,
    DuplicateAgentLoop,
    UnknownAgentLoop,
    MissingWorkflowState,
}

/// Lets a product service preserve a stable workflow failure class across the
/// generic composition boundary without exposing its concrete error type.
pub trait MigrationProductServiceError: Display {
    fn workflow_failure_class(&self) -> MigrationWorkflowFailureClassV1 {
        MigrationWorkflowFailureClassV1::ProductService
    }

    fn into_workflow_failure(self) -> (MigrationWorkflowFailureClassV1, String)
    where
        Self: Sized,
    {
        (self.workflow_failure_class(), self.to_string())
    }
}

impl MigrationProductServiceError for std::convert::Infallible {
    fn workflow_failure_class(&self) -> MigrationWorkflowFailureClassV1 {
        match *self {}
    }
}

/// Maximum number of independently identified Agent Loops allowed for one migration role call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MigrationRoleAttemptLimitV1(NonZeroU16);

impl MigrationRoleAttemptLimitV1 {
    #[must_use]
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Typed failure boundary used by workflow composition to authorize a fresh role attempt.
pub trait MigrationRoleExecutionError: Display {
    fn model_dispatch_failure_class(&self) -> Option<TransportFailureClass>;
}

impl MigrationRoleExecutionError for std::convert::Infallible {
    fn model_dispatch_failure_class(&self) -> Option<TransportFailureClass> {
        match *self {}
    }
}

fn may_restart_migration_role_attempt(
    class: TransportFailureClass,
    attempt_ordinal: u16,
    attempt_limit: MigrationRoleAttemptLimitV1,
) -> bool {
    matches!(
        class,
        TransportFailureClass::NotSent | TransportFailureClass::Ambiguous
    ) && attempt_ordinal < attempt_limit.get()
}

impl AuthorizedIntentDecisionV1 {
    /// Freezes an administrator decision and validates its immediate identity bindings.
    ///
    /// # Errors
    ///
    /// Rejects a decision bound to another request or grant.
    pub fn new(
        request: UserIntentDecisionRequestV1,
        grant: UserIntentAuthorityGrantV1,
        decision: UserIntentDecisionV1,
    ) -> Result<Self, MigrationApplicationError> {
        let request_id = request
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        let grant_id = grant
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if decision.request() != request_id || decision.authority_grant() != grant_id {
            return Err(MigrationApplicationError::Binding(
                "administrator intent decision",
            ));
        }
        Ok(Self {
            request,
            grant,
            decision,
        })
    }
}

/// Intent authority published only after its restricted Admission record is committed.
pub struct AdmittedIntentV1(PreparedIntentAdmissionV1);

impl AdmittedIntentV1 {
    #[must_use]
    pub const fn prepared(&self) -> &PreparedIntentAdmissionV1 {
        &self.0
    }
}

/// Exact policy, qualified mechanisms, attempt, and trusted evidence for Oracle Admission.
pub struct OracleAdmissionMaterialsV1 {
    policy: OracleAdmissionPolicyV1,
    mechanisms: OracleAdmissionMechanismCatalogV1,
    attempt: OracleAdmissionAttemptV1,
    evidence: OracleAdmissionEvidenceV1,
}

impl OracleAdmissionMaterialsV1 {
    #[must_use]
    pub const fn new(
        policy: OracleAdmissionPolicyV1,
        mechanisms: OracleAdmissionMechanismCatalogV1,
        attempt: OracleAdmissionAttemptV1,
        evidence: OracleAdmissionEvidenceV1,
    ) -> Self {
        Self {
            policy,
            mechanisms,
            attempt,
            evidence,
        }
    }
}

struct OracleAdmissionObservationSummaryV1 {
    admitted: bool,
    unresolved_item_count: usize,
    rejected_item_count: usize,
    failed_control_count: usize,
    artifact_failure_count: usize,
    mechanism_failure_count: usize,
    unavailable_control_count: usize,
    missing_control_count: usize,
}

impl OracleAdmissionObservationSummaryV1 {
    fn derive(
        outcome: &OracleAdmissionOutcomeV1,
        observations: &OracleAdmissionMaterialsV1,
    ) -> Self {
        let receipts = observations.evidence.receipts();
        Self {
            admitted: outcome
                .claims()
                .iter()
                .all(|claim| claim.status() == OracleClaimAdmissionStatusV1::Admitted),
            unresolved_item_count: outcome
                .claims()
                .iter()
                .map(|claim| claim.unresolved_items().len())
                .sum(),
            rejected_item_count: outcome
                .claims()
                .iter()
                .map(|claim| claim.rejected_items().len())
                .sum(),
            failed_control_count: receipts
                .iter()
                .filter(|receipt| {
                    receipt.result() == cairn_migration::OracleControlResultV1::Failed
                })
                .count(),
            artifact_failure_count: receipts
                .iter()
                .filter(|receipt| {
                    receipt.failure_class().is_some_and(
                        cairn_migration::OracleControlFailureClassV1::requires_oracle_revision,
                    )
                })
                .count(),
            mechanism_failure_count: receipts
                .iter()
                .filter(|receipt| {
                    receipt.failure_class().is_some_and(
                        cairn_migration::OracleControlFailureClassV1::requires_control_reconciliation,
                    )
                })
                .count(),
            unavailable_control_count: receipts
                .iter()
                .filter(|receipt| {
                    receipt.result() == cairn_migration::OracleControlResultV1::Unavailable
                })
                .count(),
            missing_control_count: observations
                .attempt
                .required_controls()
                .iter()
                .filter(|obligation| {
                    !receipts.iter().any(|receipt| {
                        receipt.item() == obligation.item()
                            && receipt.control() == obligation.control()
                            && receipt.mechanism() == obligation.mechanism()
                    })
                })
                .count(),
        }
    }

    const fn requires_oracle_revision(&self) -> bool {
        self.artifact_failure_count > 0
            && self.mechanism_failure_count == 0
            && self.unavailable_control_count == 0
            && self.missing_control_count == 0
    }
}

/// Admitted Oracle authority and the exact workspace from which it was proposed.
#[derive(Clone)]
pub struct AdmittedOracleV1 {
    workspace: OracleWorkspaceV1,
    proposal: OraclePortfolioProposalV1,
    coherence_review: Option<OraclePortfolioCoherenceReviewV1>,
    outcome: OracleAdmissionOutcomeV1,
}

impl AdmittedOracleV1 {
    #[must_use]
    pub const fn workspace(&self) -> &OracleWorkspaceV1 {
        &self.workspace
    }

    #[must_use]
    pub const fn proposal(&self) -> &OraclePortfolioProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub const fn coherence_review(&self) -> Option<&OraclePortfolioCoherenceReviewV1> {
        self.coherence_review.as_ref()
    }

    #[must_use]
    pub const fn outcome(&self) -> &OracleAdmissionOutcomeV1 {
        &self.outcome
    }
}

/// Proposal ready for mechanical Oracle controls under its exact decomposition treatment.
#[derive(Clone)]
pub enum OracleAdmissionReadyDraftV1 {
    Structured(OracleCoherentPortfolioV1),
    Minimal(OraclePortfolioProposalV1),
}

impl OracleAdmissionReadyDraftV1 {
    #[must_use]
    pub const fn proposal(&self) -> &OraclePortfolioProposalV1 {
        match self {
            Self::Structured(value) => value.proposal(),
            Self::Minimal(value) => value,
        }
    }

    #[must_use]
    pub const fn coherence_review(&self) -> Option<&OraclePortfolioCoherenceReviewV1> {
        match self {
            Self::Structured(value) => Some(value.review()),
            Self::Minimal(_) => None,
        }
    }
}

/// Exact qualified mechanisms, attempt, and worker receipts for Candidate Admission.
pub struct CandidateAdmissionMaterialsV1 {
    mechanisms: CandidateMechanismCatalogV1,
    attempt: CandidateAdmissionAttemptV1,
    evidence: CandidateAdmissionEvidenceV1,
}

impl CandidateAdmissionMaterialsV1 {
    #[must_use]
    pub const fn new(
        mechanisms: CandidateMechanismCatalogV1,
        attempt: CandidateAdmissionAttemptV1,
        evidence: CandidateAdmissionEvidenceV1,
    ) -> Self {
        Self {
            mechanisms,
            attempt,
            evidence,
        }
    }
}

/// Product build authority paired with the exact Candidate Admission attempt it must observe.
pub struct CandidateBuildAuthorityV1<A> {
    authority: A,
    attempt: CandidateAdmissionAttemptV1,
}

/// Exact Candidate observation lineage; diagnostics cannot substitute for this type.
///
/// ```compile_fail
/// use cairn_migration::CandidateAdmissionOutcomeArtifact;
/// use cairn_migration_app::CandidateObservationLineageV1;
/// use cairn_protocol::ContentId;
/// fn revise(_: CandidateObservationLineageV1) {}
/// let outcome = ContentId::<CandidateAdmissionOutcomeArtifact>::derive(b"outcome").unwrap();
/// revise(outcome);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CandidateObservationLineageV1(ContentId<CandidateAdmissionEvidenceArtifact>);

impl CandidateObservationLineageV1 {
    #[must_use]
    pub const fn evidence(self) -> ContentId<CandidateAdmissionEvidenceArtifact> {
        self.0
    }
}

/// Candidate source tree admitted against exact Oracle authority and worker observations.
pub struct AdmittedCandidateV1 {
    proposal: CandidateProposalV1,
    outcome: CandidateAdmissionOutcomeV1,
}

impl AdmittedCandidateV1 {
    #[must_use]
    pub const fn proposal(&self) -> &CandidateProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub const fn outcome(&self) -> &CandidateAdmissionOutcomeV1 {
        &self.outcome
    }
}

/// Semantic identity of one completed CUDA migration aggregate.
pub enum MigrationTerminalOutcomeArtifact {}

impl ContentType for MigrationTerminalOutcomeArtifact {
    const DOMAIN: &'static str = "migration.terminal-outcome.v1";
}

/// Terminal aggregate containing only exact admitted upstream identities.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "completion", rename_all = "kebab-case")]
pub enum MigrationTerminalOutcomeV1 {
    CandidateAccepted {
        schema_version: u16,
        task_id: TaskId,
        intent: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        oracle: ContentId<OracleAdmissionOutcomeArtifact>,
        candidate: ContentId<CandidateAdmissionOutcomeArtifact>,
    },
}

impl MigrationTerminalOutcomeV1 {
    fn after_candidate(
        task_id: TaskId,
        intent: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        oracle: ContentId<OracleAdmissionOutcomeArtifact>,
        candidate: ContentId<CandidateAdmissionOutcomeArtifact>,
    ) -> Self {
        Self::CandidateAccepted {
            schema_version: 1,
            task_id,
            intent,
            oracle,
            candidate,
        }
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        match self {
            Self::CandidateAccepted { task_id, .. } => *task_id,
        }
    }

    #[must_use]
    pub const fn intent(&self) -> ContentId<IntentAdmissionPublicOutcomeArtifact> {
        match self {
            Self::CandidateAccepted { intent, .. } => *intent,
        }
    }

    #[must_use]
    pub const fn oracle(&self) -> ContentId<OracleAdmissionOutcomeArtifact> {
        match self {
            Self::CandidateAccepted { oracle, .. } => *oracle,
        }
    }

    /// Derives the exact terminal aggregate identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<MigrationTerminalOutcomeArtifact>, MigrationApplicationError> {
        let bytes = cairn_codec::to_vec(self).map_err(MigrationApplicationError::domain)?;
        ContentId::derive(&bytes).map_err(MigrationApplicationError::domain)
    }
}

/// Product effects beneath the concrete workflow. None of these methods may decide Admission.
#[allow(
    clippy::missing_errors_doc,
    reason = "composition ports preserve the concrete product service error"
)]
pub trait CudaMigrationProductServices: Send + 'static {
    type Request: MigrationTaskRequest + Send + 'static;
    type CandidateBuildAuthority: Send + Sync;
    type Error: MigrationProductServiceError + Send + 'static;

    fn freeze_task(
        &mut self,
        request: Self::Request,
    ) -> impl Future<Output = Result<FrozenMigrationTaskV1, Self::Error>> + Send;

    fn await_administrator_intent_decision(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: &IntentHypothesisSetProposalV1,
        requests: &IntentDecisionRequestBatchV1,
    ) -> impl Future<Output = Result<AuthorizedIntentDecisionSetV1, Self::Error>> + Send;

    fn commit_intent_admission(
        &mut self,
        prepared: &PreparedIntentAdmissionV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn commit_suspended_agent_loop(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn ensure_task_active(
        &mut self,
        task_id: TaskId,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn commit_workflow_failure(
        &mut self,
        task_id: TaskId,
        failure: MigrationWorkflowFailureClassV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn prepare_oracle_workspace(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
    ) -> impl Future<Output = Result<OracleWorkspaceV1, Self::Error>> + Send;

    fn derive_required_oracle_dimensions(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
        workspace: &OracleWorkspaceV1,
    ) -> Result<Vec<OracleDimensionV1>, Self::Error>;

    fn commit_oracle_portfolio_review_candidate(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: &OraclePortfolioProposalV1,
    ) -> Result<(), Self::Error>;

    fn commit_oracle_revision_request(
        &mut self,
        task: &FrozenMigrationTaskV1,
        request: &OracleRevisionRequestV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn qualify_oracle_admission_mechanisms(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
        proposal: &OraclePortfolioProposalV1,
        policy: &OracleAdmissionPolicyV1,
    ) -> impl Future<Output = Result<OracleAdmissionMechanismCatalogV1, Self::Error>> + Send;

    fn run_qualified_oracle_controls(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
        proposal: &OraclePortfolioProposalV1,
        attempt: &OracleAdmissionAttemptV1,
    ) -> impl Future<Output = Result<OracleAdmissionEvidenceV1, Self::Error>> + Send;

    fn authorize_candidate_build(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
        oracle: &AdmittedOracleV1,
        contract: &CandidateOracleContractV1,
        candidate: &CandidateProposalV1,
        attempt: &CandidateAdmissionAttemptV1,
    ) -> impl Future<Output = Result<Self::CandidateBuildAuthority, Self::Error>> + Send;

    fn observe_candidate_on_worker(
        &mut self,
        authority: Self::CandidateBuildAuthority,
        attempt: &CandidateAdmissionAttemptV1,
    ) -> impl Future<Output = Result<CandidateAdmissionEvidenceV1, Self::Error>> + Send;

    fn record_terminal_outcome(
        &mut self,
        outcome: &MigrationTerminalOutcomeV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Product-owned application module and concrete readable CUDA migration workflow.
pub struct CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
{
    name: ApplicationName,
    services: S,
    executor: E,
    inbox: mpsc::Receiver<S::Request>,
    tools: ToolRegistry,
    skills: SkillRegistry,
    knowledge: KnowledgeRegistry,
    loop_step_limit: AgentLoopStepLimit,
    role_attempt_limit: MigrationRoleAttemptLimitV1,
    oracle_admission_policy: OracleAdmissionPolicyV1,
    candidate_mechanisms: Option<CandidateMechanismCatalogV1>,
    initialized_loops: BTreeMap<AgentLoopId, (TaskId, InitializedAgentLoopV1)>,
    task_reasoning: BTreeMap<TaskId, ReasoningDecompositionPolicyV1>,
    oracle_workspace: Option<OracleWorkspaceV1>,
    candidate_contract: Option<CandidateOracleContractV1>,
}

impl<S, E> CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
{
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: ApplicationName,
        services: S,
        executor: E,
        inbox: mpsc::Receiver<S::Request>,
        tools: ToolRegistry,
        skills: SkillRegistry,
        knowledge: KnowledgeRegistry,
        loop_step_limit: AgentLoopStepLimit,
        role_attempt_limit: MigrationRoleAttemptLimitV1,
        oracle_admission_policy: OracleAdmissionPolicyV1,
        candidate_mechanisms: Option<CandidateMechanismCatalogV1>,
    ) -> Self {
        Self {
            name,
            services,
            executor,
            inbox,
            tools,
            skills,
            knowledge,
            loop_step_limit,
            role_attempt_limit,
            oracle_admission_policy,
            candidate_mechanisms,
            initialized_loops: BTreeMap::new(),
            task_reasoning: BTreeMap::new(),
            oracle_workspace: None,
            candidate_contract: None,
        }
    }

    fn registries(&self) -> AgentRegistries<'_> {
        AgentRegistries {
            tools: &self.tools,
            skills: &self.skills,
            knowledge: &self.knowledge,
        }
    }

    fn reasoning_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<ReasoningDecompositionPolicyV1, MigrationApplicationError> {
        self.task_reasoning
            .get(&task_id)
            .copied()
            .ok_or(MigrationApplicationError::Binding("task reasoning policy"))
    }

    fn initialize_role_loop<C, H>(
        &mut self,
        task_id: TaskId,
        context: &C,
        hooks: &H,
    ) -> Result<AgentLoopId, MigrationApplicationError>
    where
        C: AgentLoopContext,
        H: AgentLoopHooks<C> + MigrationRoleHooksV1,
        H::Error: Display,
    {
        let loop_id = AgentLoopId::new();
        let start = AgentLoopStartV1::new(
            loop_id,
            cairn_protocol::EpisodeId::new(),
            task_id,
            MigrationRoleHooksV1::role(hooks).clone(),
            MigrationRoleHooksV1::profile(hooks).clone(),
            MigrationRoleHooksV1::version(hooks).clone(),
            context.context_id(),
            self.loop_step_limit,
        );
        let initialized = initialize_agent_loop(start, context, hooks, self.registries())
            .map_err(|_| MigrationApplicationError::AgentLoopInitialization)?;
        if self
            .initialized_loops
            .insert(loop_id, (task_id, initialized))
            .is_some()
        {
            return Err(MigrationApplicationError::DuplicateAgentLoop(loop_id));
        }
        Ok(loop_id)
    }

    fn initialize_replacement_role_loop<C, H>(
        &self,
        task_id: TaskId,
        context: &C,
        hooks: &H,
    ) -> Result<(AgentLoopId, InitializedAgentLoopV1), MigrationApplicationError>
    where
        C: AgentLoopContext,
        H: AgentLoopHooks<C> + MigrationRoleHooksV1,
        H::Error: Display,
    {
        let loop_id = AgentLoopId::new();
        let start = AgentLoopStartV1::new(
            loop_id,
            cairn_protocol::EpisodeId::new(),
            task_id,
            MigrationRoleHooksV1::role(hooks).clone(),
            MigrationRoleHooksV1::profile(hooks).clone(),
            MigrationRoleHooksV1::version(hooks).clone(),
            context.context_id(),
            self.loop_step_limit,
        );
        initialize_agent_loop(start, context, hooks, self.registries())
            .map(|initialized| (loop_id, initialized))
            .map_err(|_| MigrationApplicationError::AgentLoopInitialization)
    }

    async fn run_role_loop<C, H>(
        &mut self,
        loop_id: AgentLoopId,
        context: &C,
        hooks: &H,
    ) -> Result<H::Output, MigrationApplicationError>
    where
        C: AgentLoopContext,
        H: AgentLoopHooks<C> + MigrationRoleHooksV1,
        H::Error: Display,
        E: AgentLoopStepExecutor<C, H::StepObservation>,
        <E as AgentLoopStepExecutor<C, H::StepObservation>>::Error: MigrationRoleExecutionError,
    {
        let (task_id, mut initialized) = self
            .initialized_loops
            .remove(&loop_id)
            .ok_or(MigrationApplicationError::UnknownAgentLoop(loop_id))?;
        let mut current_loop_id = loop_id;
        let mut attempt_ordinal = 1_u16;
        let outcome = loop {
            self.services
                .ensure_task_active(task_id)
                .await
                .map_err(MigrationApplicationError::product)?;
            let registries = AgentRegistries {
                tools: &self.tools,
                skills: &self.skills,
                knowledge: &self.knowledge,
            };
            match run_agent_loop(initialized, context, hooks, registries, &mut self.executor).await
            {
                Ok(outcome) => break outcome,
                Err(AgentLoopRunError::Executor(error)) => {
                    let Some(class) = error.model_dispatch_failure_class() else {
                        return Err(MigrationApplicationError::AgentLoopExecution);
                    };
                    if !may_restart_migration_role_attempt(
                        class,
                        attempt_ordinal,
                        self.role_attempt_limit,
                    ) {
                        tracing::warn!(
                            target: "cairn.migration.application",
                            event = "migration_role_attempts_exhausted",
                            loop_id = %current_loop_id,
                            task_id = %task_id,
                            role = %MigrationRoleHooksV1::role(hooks),
                            transport_failure_class = ?class,
                            attempt_ordinal,
                            attempt_limit = self.role_attempt_limit.get(),
                            "migration role could not recover from model dispatch failure"
                        );
                        return Err(MigrationApplicationError::AgentLoopExecution);
                    }
                    let failed_loop_id = current_loop_id;
                    attempt_ordinal += 1;
                    (current_loop_id, initialized) =
                        self.initialize_replacement_role_loop(task_id, context, hooks)?;
                    tracing::warn!(
                        target: "cairn.migration.application",
                        event = "migration_role_attempt_restarted",
                        failed_loop_id = %failed_loop_id,
                        replacement_loop_id = %current_loop_id,
                        task_id = %task_id,
                        role = %MigrationRoleHooksV1::role(hooks),
                        transport_failure_class = ?class,
                        attempt_ordinal,
                        attempt_limit = self.role_attempt_limit.get(),
                        "migration role restarted with a fresh Agent Loop after model dispatch failure"
                    );
                }
                Err(_) => return Err(MigrationApplicationError::AgentLoopExecution),
            }
        };
        self.services
            .ensure_task_active(task_id)
            .await
            .map_err(MigrationApplicationError::product)?;
        match outcome {
            AgentLoopRunOutcomeV1::Complete { output, .. } => Ok(output),
            AgentLoopRunOutcomeV1::Exhausted(checkpoint) => {
                let loop_id = checkpoint.start().loop_id();
                tracing::warn!(
                    target: "cairn.migration.application",
                    event = "migration_role_goal_not_reached",
                    loop_id = %loop_id,
                    task_id = %checkpoint.start().task_id(),
                    role = %checkpoint.start().role(),
                    status = ?checkpoint.status(),
                    steps_started = checkpoint.steps_started(),
                    "migration role Agent Loop exhausted its budget"
                );
                Err(MigrationApplicationError::AgentLoopExhausted(loop_id))
            }
            AgentLoopRunOutcomeV1::Suspended(checkpoint) => {
                let loop_id = checkpoint.start().loop_id();
                self.services
                    .commit_suspended_agent_loop(&checkpoint)
                    .await
                    .map_err(MigrationApplicationError::product)?;
                tracing::info!(
                    target: "cairn.migration.application",
                    event = "agent_loop_suspension_committed",
                    loop_id = %loop_id,
                    task_id = %checkpoint.start().task_id(),
                    role = %checkpoint.start().role(),
                    "Agent Loop suspension checkpoint committed"
                );
                Err(MigrationApplicationError::AgentLoopSuspended(loop_id))
            }
        }
    }

    fn oracle_workspace(&self) -> Result<&OracleWorkspaceV1, MigrationApplicationError> {
        self.oracle_workspace
            .as_ref()
            .ok_or(MigrationApplicationError::MissingWorkflowState(
                "Oracle workspace",
            ))
    }

    fn candidate_contract(&self) -> Result<&CandidateOracleContractV1, MigrationApplicationError> {
        self.candidate_contract
            .as_ref()
            .ok_or(MigrationApplicationError::MissingWorkflowState(
                "Candidate Oracle contract",
            ))
    }
}

#[derive(Debug, Error)]
pub enum MigrationApplicationError {
    #[error("migration product service failed: {message}")]
    Product {
        failure: MigrationWorkflowFailureClassV1,
        message: String,
    },
    #[error("migration domain operation failed: {0}")]
    Domain(String),
    #[error("migration authority binding failed: {0}")]
    Binding(&'static str),
    #[error("Agent Loop initialization failed")]
    AgentLoopInitialization,
    #[error("Agent Loop execution failed")]
    AgentLoopExecution,
    #[error("Agent Loop exhausted its budget before reaching the role goal: {0}")]
    AgentLoopExhausted(AgentLoopId),
    #[error("Agent Loop yielded and requires durable resumption: {0}")]
    AgentLoopSuspended(AgentLoopId),
    #[error("duplicate Agent Loop identity: {0}")]
    DuplicateAgentLoop(AgentLoopId),
    #[error("unknown Agent Loop identity: {0}")]
    UnknownAgentLoop(AgentLoopId),
    #[error("workflow state is unavailable: {0}")]
    MissingWorkflowState(&'static str),
}

impl MigrationApplicationError {
    fn product(error: impl MigrationProductServiceError) -> Self {
        let (failure, message) = error.into_workflow_failure();
        Self::Product { failure, message }
    }

    fn domain(error: impl Display) -> Self {
        Self::Domain(error.to_string())
    }

    const fn log_class(&self) -> &'static str {
        match self {
            Self::Product {
                failure: MigrationWorkflowFailureClassV1::OracleSemanticMechanismUnavailable,
                ..
            } => "oracle-semantic-mechanism-unavailable",
            Self::Product { .. } => "product-service",
            Self::Domain(_) => "domain",
            Self::Binding(_) => "authority-binding",
            Self::AgentLoopInitialization => "agent-loop-initialization",
            Self::AgentLoopExecution => "agent-loop-execution",
            Self::AgentLoopExhausted(_) => "agent-loop-exhausted",
            Self::AgentLoopSuspended(_) => "agent-loop-suspended",
            Self::DuplicateAgentLoop(_) => "duplicate-agent-loop",
            Self::UnknownAgentLoop(_) => "unknown-agent-loop",
            Self::MissingWorkflowState(_) => "missing-workflow-state",
        }
    }

    const fn failure_class(&self) -> MigrationWorkflowFailureClassV1 {
        match self {
            Self::Product { failure, .. } => *failure,
            Self::Domain(_) => MigrationWorkflowFailureClassV1::Domain,
            Self::Binding(_) => MigrationWorkflowFailureClassV1::AuthorityBinding,
            Self::AgentLoopInitialization => {
                MigrationWorkflowFailureClassV1::AgentLoopInitialization
            }
            Self::AgentLoopExecution => MigrationWorkflowFailureClassV1::AgentLoopExecution,
            Self::AgentLoopExhausted(_) => MigrationWorkflowFailureClassV1::AgentLoopExhausted,
            Self::AgentLoopSuspended(_) => MigrationWorkflowFailureClassV1::AgentLoopSuspended,
            Self::DuplicateAgentLoop(_) => MigrationWorkflowFailureClassV1::DuplicateAgentLoop,
            Self::UnknownAgentLoop(_) => MigrationWorkflowFailureClassV1::UnknownAgentLoop,
            Self::MissingWorkflowState(_) => MigrationWorkflowFailureClassV1::MissingWorkflowState,
        }
    }
}

impl<S, E> CudaMigrationWorkflow for CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
    E: AgentLoopStepExecutor<
            SirAgentContextV1,
            MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleWholePortfolioAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
        > + AgentLoopStepExecutor<
            OracleDimensionItemDiscoveryAgentContextV1,
            MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleDimensionItemSetReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
        > + AgentLoopStepExecutor<
            OracleItemDeveloperAgentContextV1,
            MigrationRoleStepObservationV1<OracleItemDraftV1>,
        > + AgentLoopStepExecutor<
            OracleItemReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OracleItemReviewV1>,
        > + AgentLoopStepExecutor<
            OraclePortfolioCoherenceReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
        > + AgentLoopStepExecutor<
            CandidateExplorationAgentContextV1,
            MigrationRoleStepObservationV1<CandidateProposalV1>,
        > + AgentLoopStepExecutor<
            CandidateReviewAgentContextV1,
            MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
        > + AgentLoopStepExecutor<
            CandidateRevisionAgentContextV1,
            MigrationRoleStepObservationV1<CandidateProposalV1>,
        >,
    <E as AgentLoopStepExecutor<
        SirAgentContextV1,
        MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleWholePortfolioAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleDimensionItemDiscoveryAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleDimensionItemSetReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleItemDeveloperAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemDraftV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleItemReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OraclePortfolioCoherenceReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateExplorationAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateRevisionAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: MigrationRoleExecutionError,
{
    type Error = MigrationApplicationError;
    type Request = S::Request;
    type FrozenTask = FrozenMigrationTaskV1;

    type SirContext = SirAgentContextV1;
    type SirDraft = IntentHypothesisSetProposalV1;
    type IntentDecisionRequests = IntentDecisionRequestBatchV1;
    type AdministratorIntentDecision = AuthorizedIntentDecisionSetV1;
    type AdmittedIntent = AdmittedIntentV1;

    type OracleWorkspace = OracleWorkspaceV1;
    type OracleDimension = OracleDimensionV1;
    type OracleWholePortfolioContext = OracleWholePortfolioAgentContextV1;
    type OracleItemDiscoveryContext = OracleDimensionItemDiscoveryAgentContextV1;
    type OracleItemSet = OracleDimensionItemSetProposalV1;
    type OracleItemSetReviewContext = OracleDimensionItemSetReviewerAgentContextV1;
    type OracleItemSetReview = OracleDimensionItemSetReviewV1;
    type OracleItem = OracleItemV1;
    type OracleItemDevelopmentContext = OracleItemDeveloperAgentContextV1;
    type OracleItemDraft = OracleItemDraftV1;
    type OracleItemReviewContext = OracleItemReviewerAgentContextV1;
    type OracleItemReview = OracleItemReviewV1;
    type AcceptedOracleItem = OracleAcceptedItemV1;
    type OracleDraft = OraclePortfolioProposalV1;
    type OraclePortfolioReviewContext = OraclePortfolioCoherenceReviewerAgentContextV1;
    type OraclePortfolioReview = OraclePortfolioCoherenceReviewV1;
    type ReviewedOracleDraft = OracleAdmissionReadyDraftV1;
    type OracleControlObservations = OracleAdmissionMaterialsV1;
    type OracleRevisionRequest = OracleRevisionRequestV1;
    type OracleControlReconciliationRequest = OracleControlReconciliationRequestV1;
    type AdmittedOracle = AdmittedOracleV1;

    type CandidateExplorationContext = CandidateExplorationAgentContextV1;
    type CandidateDraft = CandidateProposalV1;
    type CandidateReviewContext = CandidateReviewAgentContextV1;
    type CandidateReview = ContentId<CandidateProposalArtifact>;
    type CandidateBuildAuthority = CandidateBuildAuthorityV1<S::CandidateBuildAuthority>;
    type CandidateWorkerObservations = CandidateAdmissionMaterialsV1;
    type CandidateObservationLineage = CandidateObservationLineageV1;
    type CandidateRevisionRequest = CandidateAdmissionOutcomeV1;
    type CandidateRevisionContext = CandidateRevisionAgentContextV1;
    type AdmittedCandidate = AdmittedCandidateV1;
    type TerminalOutcome = MigrationTerminalOutcomeV1;

    async fn freeze_task(
        &mut self,
        request: Self::Request,
    ) -> Result<Self::FrozenTask, Self::Error> {
        self.oracle_workspace = None;
        self.candidate_contract = None;
        let task = self
            .services
            .freeze_task(request)
            .await
            .map_err(MigrationApplicationError::product)?;
        self.task_reasoning
            .insert(task.task_id(), task.reasoning_decomposition());
        Ok(task)
    }

    fn task_id(&self, task: &Self::FrozenTask) -> TaskId {
        task.task_id()
    }

    fn reasoning_decomposition(&self, task: &Self::FrozenTask) -> ReasoningDecompositionPolicyV1 {
        task.reasoning_decomposition()
    }

    async fn prepare_sir_context(
        &mut self,
        task: &Self::FrozenTask,
    ) -> Result<Self::SirContext, Self::Error> {
        SirAgentContextV1::new(
            task.task_id(),
            task.workspace()
                .bundle()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            task.recovery_input()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_sir_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::SirContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &SirRoleHooksV1::for_reasoning_decomposition(task.reasoning_decomposition())
                .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_sir_loop(
        &mut self,
        loop_id: AgentLoopId,
        task: &Self::FrozenTask,
        context: Self::SirContext,
    ) -> Result<Self::SirDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &SirRoleHooksV1::for_reasoning_decomposition(task.reasoning_decomposition())
                .map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn derive_intent_decision_requests(
        &mut self,
        task: &Self::FrozenTask,
        sir: &Self::SirDraft,
    ) -> Result<Self::IntentDecisionRequests, Self::Error> {
        derive_user_intent_decision_requests(
            sir.identity().map_err(MigrationApplicationError::domain)?,
            sir,
            task.recovery_input()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            task.recovery_input(),
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn await_administrator_intent_decision(
        &mut self,
        task: &Self::FrozenTask,
        sir: &Self::SirDraft,
        requests: &Self::IntentDecisionRequests,
    ) -> Result<Self::AdministratorIntentDecision, Self::Error> {
        self.services
            .await_administrator_intent_decision(task, sir, requests)
            .await
            .map_err(MigrationApplicationError::product)
    }

    async fn admit_intent(
        &mut self,
        task: &Self::FrozenTask,
        sir: Self::SirDraft,
        requests: Self::IntentDecisionRequests,
        decision: Self::AdministratorIntentDecision,
    ) -> Result<Self::AdmittedIntent, Self::Error> {
        let materials = decision
            .decisions
            .iter()
            .map(|decision| IntentDecisionMaterialV1 {
                request: &decision.request,
                grant: &decision.grant,
                decision: &decision.decision,
            })
            .collect::<Vec<_>>();
        let prepared = promote_user_intent(
            sir.identity().map_err(MigrationApplicationError::domain)?,
            &sir,
            task.recovery_input()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            task.recovery_input(),
            &requests,
            &materials,
        )
        .map_err(MigrationApplicationError::domain)?;
        self.services
            .commit_intent_admission(&prepared)
            .await
            .map_err(MigrationApplicationError::product)?;
        tracing::info!(
            target: "cairn.migration.admission",
            event = "intent_admission_committed",
            task_id = %task.task_id(),
            proposal_id = %sir.identity().map_err(MigrationApplicationError::domain)?,
            request_batch_id = %requests.identity().map_err(MigrationApplicationError::domain)?,
            request_count = requests.requests().len(),
            outcome_id = %prepared.public_outcome().identity().map_err(MigrationApplicationError::domain)?,
            "Intent Admission committed exact authority lineage"
        );
        Ok(AdmittedIntentV1(prepared))
    }

    async fn prepare_oracle_workspace(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
    ) -> Result<Self::OracleWorkspace, Self::Error> {
        let workspace = self
            .services
            .prepare_oracle_workspace(task, intent)
            .await
            .map_err(MigrationApplicationError::product)?;
        let contract = intent.prepared().public_outcome().contract();
        let contract_id = contract
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if workspace.task_id() != task.task_id()
            || workspace.admitted_intent() != contract_id
            || workspace.sir_input()
                != task
                    .recovery_input()
                    .identity()
                    .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Oracle workspace"));
        }
        self.oracle_workspace = Some(workspace.clone());
        Ok(workspace)
    }

    fn derive_required_oracle_dimensions(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
    ) -> Result<Vec<Self::OracleDimension>, Self::Error> {
        self.services
            .derive_required_oracle_dimensions(task, intent, workspace)
            .map_err(MigrationApplicationError::product)
    }

    fn prepare_oracle_whole_portfolio_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
        dimensions: &[Self::OracleDimension],
        lineage: OracleWholePortfolioLineageV1<
            '_,
            Self::ReviewedOracleDraft,
            Self::OracleRevisionRequest,
        >,
    ) -> Result<Self::OracleWholePortfolioContext, Self::Error> {
        if task.reasoning_decomposition() != ReasoningDecompositionPolicyV1::MinimalDecomposition {
            return Err(MigrationApplicationError::Binding(
                "whole-portfolio decomposition policy",
            ));
        }
        let authority = OracleWholePortfolioProposalAuthorityV1::new(
            workspace,
            dimensions
                .iter()
                .map(OracleDimensionV1::identity)
                .collect::<Result<Vec<_>, _>>()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)?;
        let (previous_portfolio, admission_feedback) = match lineage {
            OracleWholePortfolioLineageV1::Initial => (None, None),
            OracleWholePortfolioLineageV1::AdmissionRevision {
                previous,
                admission,
            } => (
                Some(
                    previous
                        .proposal()
                        .identity()
                        .map_err(MigrationApplicationError::domain)?,
                ),
                Some(
                    admission
                        .identity()
                        .map_err(MigrationApplicationError::domain)?,
                ),
            ),
        };
        OracleWholePortfolioAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            workspace
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            authority
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            previous_portfolio,
            admission_feedback,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_whole_portfolio_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleWholePortfolioContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleWholePortfolioRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_whole_portfolio_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleWholePortfolioContext,
    ) -> Result<Self::OracleDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleWholePortfolioRoleHooksV1::for_reasoning_decomposition(
                self.reasoning_for_task(context.task_id())?,
            )
            .map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    fn accept_oracle_whole_portfolio_proposal(
        &mut self,
        draft: Self::OracleDraft,
    ) -> Result<Self::ReviewedOracleDraft, Self::Error> {
        Ok(OracleAdmissionReadyDraftV1::Minimal(draft))
    }

    fn prepare_oracle_item_discovery_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
        dimension: &Self::OracleDimension,
        lineage: OracleItemDiscoveryLineageV1<'_, Self::OracleItemSet, Self::OracleItemSetReview>,
    ) -> Result<Self::OracleItemDiscoveryContext, Self::Error> {
        let dimension_id = dimension
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        let (previous_item_set, review_feedback) = match lineage {
            OracleItemDiscoveryLineageV1::Initial => (None, None),
            OracleItemDiscoveryLineageV1::ReviewRevision { previous, review } => {
                review
                    .validate_against(previous)
                    .map_err(MigrationApplicationError::domain)?;
                if previous.dimension() != dimension_id
                    || !matches!(
                        review.decision(),
                        OracleDimensionItemSetReviewDecisionV1::NeedsRevision { .. }
                    )
                {
                    return Err(MigrationApplicationError::Binding(
                        "Oracle item discovery revision feedback",
                    ));
                }
                (
                    Some(
                        previous
                            .identity()
                            .map_err(MigrationApplicationError::domain)?,
                    ),
                    Some(
                        review
                            .identity()
                            .map_err(MigrationApplicationError::domain)?,
                    ),
                )
            }
        };
        OracleDimensionItemDiscoveryAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            workspace
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            dimension_id,
            previous_item_set,
            review_feedback,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_item_discovery_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemDiscoveryContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleDimensionItemDiscoveryRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_item_discovery_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemDiscoveryContext,
    ) -> Result<Self::OracleItemSet, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleDimensionItemDiscoveryRoleHooksV1::for_reasoning_decomposition(
                self.reasoning_for_task(context.task_id())?,
            )
            .map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    fn prepare_oracle_item_set_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        item_set: &Self::OracleItemSet,
    ) -> Result<Self::OracleItemSetReviewContext, Self::Error> {
        OracleDimensionItemSetReviewerAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            item_set.dimension(),
            item_set
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_item_set_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemSetReviewContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleDimensionItemSetReviewerRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_item_set_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemSetReviewContext,
    ) -> Result<
        OracleReviewDispositionV1<Self::OracleItemSetReview, Self::OracleItemSetReview>,
        Self::Error,
    > {
        let review = self
            .run_role_loop(
                loop_id,
                &context,
                &OracleDimensionItemSetReviewerRoleHooksV1::for_reasoning_decomposition(
                    self.reasoning_for_task(context.task_id())?,
                )
                .map_err(MigrationApplicationError::domain)?,
            )
            .await?;
        match review.decision() {
            OracleDimensionItemSetReviewDecisionV1::Approved => {
                Ok(OracleReviewDispositionV1::Approved(review))
            }
            OracleDimensionItemSetReviewDecisionV1::NeedsRevision { findings } => {
                tracing::info!(
                    target: "cairn.migration.review",
                    event = "oracle_item_set_review_rejected",
                    dimension_id = %review.dimension(),
                    proposal_id = %review.proposal(),
                    finding_count = findings.len(),
                    "Oracle item-set Review returned typed actionable findings"
                );
                Ok(OracleReviewDispositionV1::Revise(review))
            }
        }
    }

    fn validate_and_expand_oracle_item_set(
        &mut self,
        dimension: &Self::OracleDimension,
        item_set: Self::OracleItemSet,
    ) -> Result<Vec<Self::OracleItem>, Self::Error> {
        if item_set.dimension()
            != dimension
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Oracle item set"));
        }
        Ok(item_set.items().to_vec())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one typed projection keeps every mutually exclusive revision lineage fail-closed"
    )]
    fn prepare_oracle_item_development_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
        item: &Self::OracleItem,
        lineage: OracleItemDevelopmentLineageV1<
            '_,
            Self::OracleItemDraft,
            Self::OracleItemReview,
            Self::OraclePortfolioReview,
            Self::OracleRevisionRequest,
        >,
    ) -> Result<Self::OracleItemDevelopmentContext, Self::Error> {
        let item_id = item.identity().map_err(MigrationApplicationError::domain)?;
        let bind_review = |previous: &OracleItemDraftV1,
                           review: &OracleItemReviewV1|
         -> Result<_, MigrationApplicationError> {
            review
                .validate_against(previous)
                .map_err(MigrationApplicationError::domain)?;
            if !matches!(
                review.decision(),
                OracleItemReviewDecisionV1::NeedsRevision { .. }
            ) {
                return Err(MigrationApplicationError::Binding(
                    "Oracle item revision feedback",
                ));
            }
            review.identity().map_err(MigrationApplicationError::domain)
        };
        let (previous_draft, review_feedback, coherence_feedback, admission_feedback) =
            match lineage {
                OracleItemDevelopmentLineageV1::Initial => (None, None, None, None),
                OracleItemDevelopmentLineageV1::ReviewRevision { previous, review } => {
                    if previous
                        .item()
                        .identity()
                        .map_err(MigrationApplicationError::domain)?
                        != item_id
                    {
                        return Err(MigrationApplicationError::Binding(
                            "Oracle item revision feedback",
                        ));
                    }
                    (
                        Some(
                            previous
                                .identity()
                                .map_err(MigrationApplicationError::domain)?,
                        ),
                        Some(bind_review(previous, review)?),
                        None,
                        None,
                    )
                }
                OracleItemDevelopmentLineageV1::CoherenceRevision {
                    previous,
                    coherence,
                    review,
                } => {
                    if previous
                        .item()
                        .identity()
                        .map_err(MigrationApplicationError::domain)?
                        != item_id
                        || !matches!(
                            coherence.decision(),
                            OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings }
                                if findings.iter().any(|finding| {
                                    finding.affected_items().items().contains(&item_id)
                                })
                        )
                    {
                        return Err(MigrationApplicationError::Binding(
                            "Oracle portfolio coherence revision feedback",
                        ));
                    }
                    (
                        Some(
                            previous
                                .identity()
                                .map_err(MigrationApplicationError::domain)?,
                        ),
                        review
                            .map(|review| bind_review(previous, review))
                            .transpose()?,
                        Some(
                            coherence
                                .identity()
                                .map_err(MigrationApplicationError::domain)?,
                        ),
                        None,
                    )
                }
                OracleItemDevelopmentLineageV1::AdmissionRevision {
                    previous,
                    admission,
                    review,
                } => {
                    if previous
                        .item()
                        .identity()
                        .map_err(MigrationApplicationError::domain)?
                        != item_id
                    {
                        return Err(MigrationApplicationError::Binding(
                            "Oracle item Admission revision feedback",
                        ));
                    }
                    (
                        Some(
                            previous
                                .identity()
                                .map_err(MigrationApplicationError::domain)?,
                        ),
                        review
                            .map(|review| bind_review(previous, review))
                            .transpose()?,
                        None,
                        Some(
                            admission
                                .identity()
                                .map_err(MigrationApplicationError::domain)?,
                        ),
                    )
                }
            };
        OracleItemDeveloperAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            workspace
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            item_id,
            previous_draft,
            review_feedback,
            coherence_feedback,
            admission_feedback,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_item_development_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemDevelopmentContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleItemDeveloperRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_item_development_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemDevelopmentContext,
    ) -> Result<Self::OracleItemDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleItemDeveloperRoleHooksV1::for_reasoning_decomposition(
                self.reasoning_for_task(context.task_id())?,
            )
            .map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    fn prepare_oracle_item_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleItemDraft,
    ) -> Result<Self::OracleItemReviewContext, Self::Error> {
        OracleItemReviewerAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            draft
                .item()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            draft
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_item_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemReviewContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleItemReviewerRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_item_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemReviewContext,
    ) -> Result<
        OracleReviewDispositionV1<Self::OracleItemReview, Self::OracleItemReview>,
        Self::Error,
    > {
        let review = self
            .run_role_loop(
                loop_id,
                &context,
                &OracleItemReviewerRoleHooksV1::for_reasoning_decomposition(
                    self.reasoning_for_task(context.task_id())?,
                )
                .map_err(MigrationApplicationError::domain)?,
            )
            .await?;
        match review.decision() {
            OracleItemReviewDecisionV1::Approved => {
                tracing::info!(
                    target: "cairn.migration.review",
                    event = "oracle_item_review_approved",
                    item_id = %review.item(),
                    draft_id = %review.draft(),
                    "Oracle item Review approved the exact draft revision"
                );
                Ok(OracleReviewDispositionV1::Approved(review))
            }
            OracleItemReviewDecisionV1::NeedsRevision { findings } => {
                tracing::info!(
                    target: "cairn.migration.review",
                    event = "oracle_item_review_rejected",
                    item_id = %review.item(),
                    draft_id = %review.draft(),
                    finding_count = findings.len(),
                    "Oracle item Review returned typed actionable findings"
                );
                Ok(OracleReviewDispositionV1::Revise(review))
            }
        }
    }

    fn accept_reviewed_oracle_item(
        &mut self,
        item: Self::OracleItem,
        draft: Self::OracleItemDraft,
        review: Self::OracleItemReview,
    ) -> Result<Self::AcceptedOracleItem, Self::Error> {
        if item.identity().map_err(MigrationApplicationError::domain)?
            != draft
                .item()
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("accepted Oracle item"));
        }
        OracleAcceptedItemV1::new(&draft, &review).map_err(MigrationApplicationError::domain)
    }

    fn assemble_oracle_portfolio(
        &mut self,
        workspace: &Self::OracleWorkspace,
        dimensions: Vec<Self::OracleDimension>,
        accepted_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error> {
        OraclePortfolioProposalV1::assemble(workspace, dimensions, accepted_items)
            .map_err(MigrationApplicationError::domain)
    }

    fn prepare_oracle_portfolio_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
    ) -> Result<Self::OraclePortfolioReviewContext, Self::Error> {
        self.services
            .commit_oracle_portfolio_review_candidate(task, draft)
            .map_err(MigrationApplicationError::product)?;
        OraclePortfolioCoherenceReviewerAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            draft
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_portfolio_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OraclePortfolioReviewContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OraclePortfolioCoherenceReviewerRoleHooksV1::for_reasoning_decomposition(
                task.reasoning_decomposition(),
            )
            .map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_portfolio_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OraclePortfolioReviewContext,
    ) -> Result<
        OracleReviewDispositionV1<Self::OraclePortfolioReview, Self::OraclePortfolioReview>,
        Self::Error,
    > {
        let review = self
            .run_role_loop(
                loop_id,
                &context,
                &OraclePortfolioCoherenceReviewerRoleHooksV1::for_reasoning_decomposition(
                    self.reasoning_for_task(context.task_id())?,
                )
                .map_err(MigrationApplicationError::domain)?,
            )
            .await?;
        match review.decision() {
            OraclePortfolioCoherenceDecisionV1::Approved => {
                tracing::info!(
                    target: "cairn.migration.review",
                    event = "oracle_portfolio_coherence_review_approved",
                    portfolio_id = %review.portfolio(),
                    "Oracle portfolio cross-item coherence Review approved"
                );
                Ok(OracleReviewDispositionV1::Approved(review))
            }
            OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } => {
                tracing::info!(
                    target: "cairn.migration.review",
                    event = "oracle_portfolio_coherence_review_rejected",
                    portfolio_id = %review.portfolio(),
                    finding_count = findings.len(),
                    "Oracle portfolio coherence Review returned typed affected-item findings"
                );
                Ok(OracleReviewDispositionV1::Revise(review))
            }
        }
    }

    fn prepare_oracle_items_for_coherence_revision(
        &mut self,
        draft: &Self::OracleDraft,
        review: &Self::OraclePortfolioReview,
    ) -> Result<Vec<(Self::OracleItem, Self::OracleItemDraft)>, Self::Error> {
        review
            .validate_against(draft)
            .map_err(MigrationApplicationError::domain)?;
        let OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } = review.decision()
        else {
            return Err(MigrationApplicationError::Binding(
                "Oracle portfolio coherence revision targets",
            ));
        };
        let mut targets = findings
            .iter()
            .flat_map(|finding| finding.affected_items().items().iter().copied())
            .collect::<Vec<_>>();
        targets.sort_by_key(ContentId::to_wire);
        targets.dedup();
        let selected = draft
            .accepted_items()
            .iter()
            .filter(|accepted| {
                accepted
                    .item()
                    .identity()
                    .is_ok_and(|identity| targets.contains(&identity))
            })
            .map(|accepted| (accepted.item().clone(), accepted.draft().clone()))
            .collect::<Vec<_>>();
        if selected.is_empty() || selected.len() != targets.len() {
            return Err(MigrationApplicationError::Binding(
                "Oracle portfolio coherence revision targets",
            ));
        }
        Ok(selected)
    }

    fn accept_oracle_portfolio_review(
        &mut self,
        draft: Self::OracleDraft,
        review: Self::OraclePortfolioReview,
    ) -> Result<Self::ReviewedOracleDraft, Self::Error> {
        OracleCoherentPortfolioV1::new(&draft, &review)
            .map(OracleAdmissionReadyDraftV1::Structured)
            .map_err(MigrationApplicationError::domain)
    }

    fn replace_oracle_items_after_coherence_revision(
        &mut self,
        draft: Self::OracleDraft,
        revised_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error> {
        replace_oracle_items(self.oracle_workspace()?, &draft, revised_items)
    }

    async fn prepare_oracle_items_for_admission_revision(
        &mut self,
        task: &Self::FrozenTask,
        draft: &Self::ReviewedOracleDraft,
        request: &Self::OracleRevisionRequest,
    ) -> Result<Vec<(Self::OracleItem, Self::OracleItemDraft)>, Self::Error> {
        if request.proposal()
            != draft
                .proposal()
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding(
                "Oracle admission revision proposal",
            ));
        }
        let evidence = request.evidence();
        let mut targets = evidence
            .receipts()
            .iter()
            .filter(|receipt| receipt.result() == cairn_migration::OracleControlResultV1::Failed)
            .map(cairn_migration::OracleControlReceiptV1::item)
            .collect::<Vec<_>>();
        targets.sort_by_key(ContentId::to_wire);
        targets.dedup();
        let selected = draft
            .proposal()
            .accepted_items()
            .iter()
            .filter(|accepted| {
                accepted
                    .item()
                    .identity()
                    .is_ok_and(|identity| targets.contains(&identity))
            })
            .map(|accepted| (accepted.item().clone(), accepted.draft().clone()))
            .collect::<Vec<_>>();
        if selected.len() != targets.len() || selected.is_empty() {
            return Err(MigrationApplicationError::Binding(
                "Oracle admission revision targets",
            ));
        }
        self.services
            .commit_oracle_revision_request(task, request)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(selected)
    }

    fn replace_oracle_items_after_admission_revision(
        &mut self,
        draft: Self::ReviewedOracleDraft,
        revised_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error> {
        replace_oracle_items(self.oracle_workspace()?, draft.proposal(), revised_items)
    }

    async fn run_qualified_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::ReviewedOracleDraft,
    ) -> Result<Self::OracleControlObservations, Self::Error> {
        let proposal = draft.proposal();
        let mechanisms = self
            .services
            .qualify_oracle_admission_mechanisms(
                task,
                intent,
                proposal,
                &self.oracle_admission_policy,
            )
            .await
            .map_err(MigrationApplicationError::product)?;
        let attempt =
            OracleAdmissionAttemptV1::new(proposal, &self.oracle_admission_policy, &mechanisms)
                .map_err(MigrationApplicationError::domain)?;
        let evidence = self
            .services
            .run_qualified_oracle_controls(task, intent, proposal, &attempt)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(OracleAdmissionMaterialsV1::new(
            self.oracle_admission_policy.clone(),
            mechanisms,
            attempt,
            evidence,
        ))
    }

    async fn admit_oracle(
        &mut self,
        _task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        draft: Self::ReviewedOracleDraft,
        observations: Self::OracleControlObservations,
    ) -> Result<
        OracleAdmissionDispositionV1<
            Self::AdmittedOracle,
            Self::ReviewedOracleDraft,
            Self::OracleRevisionRequest,
            Self::OracleControlReconciliationRequest,
            Self::OracleControlObservations,
        >,
        Self::Error,
    > {
        let outcome = recompute_oracle_admission(
            draft.proposal(),
            &observations.policy,
            &observations.mechanisms,
            &observations.attempt,
            &observations.evidence,
        )
        .map_err(MigrationApplicationError::domain)?;
        let summary = OracleAdmissionObservationSummaryV1::derive(&outcome, &observations);
        tracing::info!(
            target: "cairn.migration.admission",
            event = "oracle_admission_recomputed",
            proposal_id = %outcome.proposal(),
            evidence_id = %outcome.evidence(),
            admitted = summary.admitted,
            unresolved_item_count = summary.unresolved_item_count,
            rejected_item_count = summary.rejected_item_count,
            failed_control_count = summary.failed_control_count,
            artifact_failure_count = summary.artifact_failure_count,
            mechanism_failure_count = summary.mechanism_failure_count,
            unavailable_control_count = summary.unavailable_control_count,
            missing_control_count = summary.missing_control_count,
            "Oracle Admission mechanically recomputed"
        );
        if summary.admitted {
            let proposal = draft.proposal().clone();
            let coherence_review = draft.coherence_review().cloned();
            Ok(OracleAdmissionDispositionV1::Admitted(AdmittedOracleV1 {
                workspace: self.oracle_workspace()?.clone(),
                proposal,
                coherence_review,
                outcome,
            }))
        } else if summary.requires_oracle_revision() {
            let request = OracleRevisionRequestV1::from_admission(
                observations.attempt.clone(),
                outcome,
                observations.evidence.clone(),
            )
            .map_err(MigrationApplicationError::domain)?;
            Ok(OracleAdmissionDispositionV1::Revise {
                draft,
                request,
                control_observations: observations,
            })
        } else {
            let request = OracleControlReconciliationRequestV1::from_admission(
                observations.attempt.clone(),
                outcome,
                observations.evidence.clone(),
            )
            .map_err(MigrationApplicationError::domain)?;
            Ok(OracleAdmissionDispositionV1::Reconcile {
                draft,
                request,
                control_observations: observations,
            })
        }
    }

    async fn reconcile_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::ReviewedOracleDraft,
        request: &Self::OracleControlReconciliationRequest,
    ) -> Result<Self::OracleControlObservations, Self::Error> {
        let proposal = draft.proposal();
        if request.proposal()
            != proposal
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding(
                "Oracle control reconciliation proposal",
            ));
        }
        let mechanisms = self
            .services
            .qualify_oracle_admission_mechanisms(
                task,
                intent,
                proposal,
                &self.oracle_admission_policy,
            )
            .await
            .map_err(MigrationApplicationError::product)?;
        if mechanisms
            .identity()
            .map_err(MigrationApplicationError::domain)?
            != request.attempt().mechanisms()
        {
            return Err(MigrationApplicationError::Binding(
                "Oracle control reconciliation mechanisms",
            ));
        }
        let evidence = self
            .services
            .run_qualified_oracle_controls(task, intent, proposal, request.attempt())
            .await
            .map_err(MigrationApplicationError::product)?;
        tracing::info!(
            target: "cairn.migration.reconciliation",
            event = "oracle_controls_reconciled",
            task_id = %task.task_id(),
            proposal_id = %request.proposal(),
            receipt_count = evidence.receipts().len(),
            "Oracle control reconciliation produced a fresh exact observation set"
        );
        Ok(OracleAdmissionMaterialsV1::new(
            self.oracle_admission_policy.clone(),
            mechanisms,
            request.attempt().clone(),
            evidence,
        ))
    }

    async fn prepare_candidate_exploration_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
    ) -> Result<Self::CandidateExplorationContext, Self::Error> {
        let contract = CandidateOracleContractV1::derive(&oracle.proposal, &oracle.outcome)
            .map_err(MigrationApplicationError::domain)?;
        let workspace =
            CandidateWorkspaceV1::derive(&oracle.workspace, &oracle.proposal, &contract)
                .map_err(MigrationApplicationError::domain)?;
        let context = CandidateExplorationAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            oracle
                .outcome
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            contract
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            workspace
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)?;
        self.candidate_contract = Some(contract);
        Ok(context)
    }

    async fn initialize_candidate_exploration_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateExplorationContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &CandidateExplorationRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_candidate_exploration_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateExplorationContext,
    ) -> Result<Self::CandidateDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &CandidateExplorationRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn prepare_candidate_review_context(
        &mut self,
        task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        _oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
    ) -> Result<Self::CandidateReviewContext, Self::Error> {
        let contract_id = self
            .candidate_contract()?
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if candidate.oracle_contract() != contract_id {
            return Err(MigrationApplicationError::Binding(
                "Candidate proposal contract",
            ));
        }
        CandidateReviewAgentContextV1::new(
            task.task_id(),
            contract_id,
            candidate
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_candidate_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateReviewContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &CandidateReviewRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_candidate_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateReviewContext,
    ) -> Result<Self::CandidateReview, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &CandidateReviewRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn authorize_candidate_build(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
        review: &Self::CandidateReview,
    ) -> Result<Self::CandidateBuildAuthority, Self::Error> {
        if *review
            != candidate
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Candidate review"));
        }
        let contract = self.candidate_contract()?.clone();
        let mechanisms = self.candidate_mechanisms.as_ref().ok_or(
            MigrationApplicationError::MissingWorkflowState(
                "qualified Candidate Admission mechanisms",
            ),
        )?;
        let attempt = CandidateAdmissionAttemptV1::new(&contract, candidate, mechanisms)
            .map_err(MigrationApplicationError::domain)?;
        let authority = self
            .services
            .authorize_candidate_build(task, intent, oracle, &contract, candidate, &attempt)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(CandidateBuildAuthorityV1 { authority, attempt })
    }

    async fn observe_candidate_on_worker(
        &mut self,
        authority: Self::CandidateBuildAuthority,
    ) -> Result<Self::CandidateWorkerObservations, Self::Error> {
        let evidence = self
            .services
            .observe_candidate_on_worker(authority.authority, &authority.attempt)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(CandidateAdmissionMaterialsV1::new(
            self.candidate_mechanisms.clone().ok_or(
                MigrationApplicationError::MissingWorkflowState(
                    "qualified Candidate Admission mechanisms",
                ),
            )?,
            authority.attempt,
            evidence,
        ))
    }

    async fn admit_candidate(
        &mut self,
        _task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        _oracle: &Self::AdmittedOracle,
        candidate: Self::CandidateDraft,
        review: Self::CandidateReview,
        observations: Self::CandidateWorkerObservations,
    ) -> Result<
        CandidateAdmissionDispositionV1<
            Self::AdmittedCandidate,
            Self::CandidateDraft,
            Self::CandidateObservationLineage,
            Self::CandidateRevisionRequest,
        >,
        Self::Error,
    > {
        if review
            != candidate
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Candidate review"));
        }
        let outcome = recompute_candidate_admission(
            self.candidate_contract()?,
            &candidate,
            &observations.mechanisms,
            &observations.attempt,
            &observations.evidence,
        )
        .map_err(MigrationApplicationError::domain)?;
        let admitted = outcome
            .claims()
            .iter()
            .all(|claim| claim.status() == CandidateClaimStatusV1::Admitted);
        tracing::info!(
            target: "cairn.migration.admission",
            event = "candidate_admission_recomputed",
            proposal_id = %outcome.proposal(),
            evidence_id = %outcome.evidence(),
            admitted,
            "Candidate Admission mechanically recomputed"
        );
        if admitted {
            Ok(CandidateAdmissionDispositionV1::Admitted(
                AdmittedCandidateV1 {
                    proposal: candidate,
                    outcome,
                },
            ))
        } else {
            Ok(CandidateAdmissionDispositionV1::Revise {
                candidate,
                observation_lineage: CandidateObservationLineageV1(outcome.evidence()),
                request: outcome,
            })
        }
    }

    async fn prepare_candidate_revision_context(
        &mut self,
        task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        _oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
        observation_lineage: &Self::CandidateObservationLineage,
        request: &Self::CandidateRevisionRequest,
    ) -> Result<Self::CandidateRevisionContext, Self::Error> {
        let candidate_id = candidate
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if request.proposal() != candidate_id
            || request.evidence() != observation_lineage.evidence()
        {
            return Err(MigrationApplicationError::Binding(
                "Candidate revision observation lineage",
            ));
        }
        CandidateRevisionAgentContextV1::new(
            task.task_id(),
            self.candidate_contract()?
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            candidate_id,
            observation_lineage.evidence(),
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_candidate_revision_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateRevisionContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &CandidateRevisionRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_candidate_revision_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateRevisionContext,
    ) -> Result<Self::CandidateDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &CandidateRevisionRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn record_terminal_outcome(
        &mut self,
        task: Self::FrozenTask,
        intent: Self::AdmittedIntent,
        oracle: Self::AdmittedOracle,
        candidate: Self::AdmittedCandidate,
    ) -> Result<Self::TerminalOutcome, Self::Error> {
        let outcome = MigrationTerminalOutcomeV1::after_candidate(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            oracle
                .outcome()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            candidate
                .outcome()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        );
        self.services
            .record_terminal_outcome(&outcome)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(outcome)
    }
}

fn replace_oracle_items(
    workspace: &OracleWorkspaceV1,
    draft: &OraclePortfolioProposalV1,
    revised_items: Vec<OracleAcceptedItemV1>,
) -> Result<OraclePortfolioProposalV1, MigrationApplicationError> {
    let mut revised_ids = revised_items
        .iter()
        .map(|accepted| accepted.item().identity())
        .collect::<Result<Vec<_>, _>>()
        .map_err(MigrationApplicationError::domain)?;
    revised_ids.sort_by_key(ContentId::to_wire);
    if revised_ids.is_empty()
        || revised_ids.windows(2).any(|pair| pair[0] == pair[1])
        || revised_ids.iter().any(|identity| {
            !draft.accepted_items().iter().any(|accepted| {
                accepted
                    .item()
                    .identity()
                    .is_ok_and(|existing| existing == *identity)
            })
        })
    {
        return Err(MigrationApplicationError::Binding(
            "revised Oracle item set",
        ));
    }
    let mut accepted_items = draft
        .accepted_items()
        .iter()
        .filter(|accepted| {
            accepted
                .item()
                .identity()
                .is_ok_and(|identity| !revised_ids.contains(&identity))
        })
        .cloned()
        .collect::<Vec<_>>();
    accepted_items.extend(revised_items);
    let dimensions = draft
        .entries()
        .iter()
        .map(|entry| entry.dimension().clone())
        .collect();
    OraclePortfolioProposalV1::assemble(workspace, dimensions, accepted_items)
        .map_err(MigrationApplicationError::domain)
}

impl<S, E> ApplicationModule for CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
    E: AgentLoopStepExecutor<
            SirAgentContextV1,
            MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleWholePortfolioAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
        > + AgentLoopStepExecutor<
            OracleDimensionItemDiscoveryAgentContextV1,
            MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleDimensionItemSetReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
        > + AgentLoopStepExecutor<
            OracleItemDeveloperAgentContextV1,
            MigrationRoleStepObservationV1<OracleItemDraftV1>,
        > + AgentLoopStepExecutor<
            OracleItemReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OracleItemReviewV1>,
        > + AgentLoopStepExecutor<
            OraclePortfolioCoherenceReviewerAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
        > + AgentLoopStepExecutor<
            CandidateExplorationAgentContextV1,
            MigrationRoleStepObservationV1<CandidateProposalV1>,
        > + AgentLoopStepExecutor<
            CandidateReviewAgentContextV1,
            MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
        > + AgentLoopStepExecutor<
            CandidateRevisionAgentContextV1,
            MigrationRoleStepObservationV1<CandidateProposalV1>,
        > + 'static,
    <E as AgentLoopStepExecutor<
        SirAgentContextV1,
        MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleWholePortfolioAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleDimensionItemDiscoveryAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleDimensionItemSetReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleDimensionItemSetReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleItemDeveloperAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemDraftV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OracleItemReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OracleItemReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        OraclePortfolioCoherenceReviewerAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioCoherenceReviewV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateExplorationAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
    >>::Error: MigrationRoleExecutionError,
    <E as AgentLoopStepExecutor<
        CandidateRevisionAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: MigrationRoleExecutionError,
{
    type Error = MigrationApplicationError;

    fn name(&self) -> &ApplicationName {
        &self.name
    }

    async fn run(mut self) -> Result<(), Self::Error> {
        while let Some(request) = self.inbox.recv().await {
            let task_id = request.task_id();
            match cairn_migration::run_cuda_migration(&mut self, request).await {
                Ok(terminal) => match terminal.identity() {
                    Ok(terminal_id) => tracing::info!(
                        target: "cairn.migration.application",
                        event = "migration_terminal_recorded",
                        terminal_id = %terminal_id,
                        "CUDA migration terminal outcome recorded"
                    ),
                    Err(error) => tracing::error!(
                        target: "cairn.migration.application",
                        event = "migration_terminal_identity_failed",
                        error_class = error.log_class(),
                        "CUDA migration terminal identity could not be derived"
                    ),
                },
                Err(error) => {
                    if !matches!(error, MigrationApplicationError::AgentLoopSuspended(_))
                        && self
                            .services
                            .commit_workflow_failure(task_id, error.failure_class())
                            .await
                            .is_err()
                    {
                        tracing::error!(
                            target: "cairn.migration.application",
                            event = "migration_task_failure_commit_failed",
                            task_id = %task_id,
                            error_class = "product-service",
                            "CUDA migration task failure could not be committed"
                        );
                    }
                    tracing::warn!(
                        target: "cairn.migration.application",
                        event = "migration_task_failed",
                        task_id = %task_id,
                        error_class = error.log_class(),
                        "CUDA migration task stopped without terminating the application module"
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use cairn_agent::{AgentLoopCheckpointV1, AgentLoopStepExecutionV1, AgentStepAccessV1};

    use super::*;

    struct NeverServices;
    struct NeverExecutor;
    struct TestRequest;
    struct TestBuildAuthority;

    impl MigrationTaskRequest for TestRequest {
        fn task_id(&self) -> TaskId {
            TaskId::new()
        }
    }

    impl<C, O> AgentLoopStepExecutor<C, O> for NeverExecutor
    where
        C: Sync,
        O: Send,
    {
        type Error = Infallible;

        async fn execute_step(
            &mut self,
            _checkpoint: &AgentLoopCheckpointV1,
            _context: &C,
            _access: &AgentStepAccessV1,
        ) -> Result<AgentLoopStepExecutionV1<O>, Self::Error> {
            panic!("compile-time composition test does not execute an Agent Loop")
        }
    }

    impl CudaMigrationProductServices for NeverServices {
        type Request = TestRequest;
        type CandidateBuildAuthority = TestBuildAuthority;
        type Error = Infallible;

        async fn freeze_task(
            &mut self,
            _request: Self::Request,
        ) -> Result<FrozenMigrationTaskV1, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn await_administrator_intent_decision(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _proposal: &IntentHypothesisSetProposalV1,
            _requests: &IntentDecisionRequestBatchV1,
        ) -> Result<AuthorizedIntentDecisionSetV1, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn commit_intent_admission(
            &mut self,
            _prepared: &PreparedIntentAdmissionV1,
        ) -> Result<(), Self::Error> {
            panic!("compile-time composition test")
        }

        async fn commit_suspended_agent_loop(
            &mut self,
            _checkpoint: &AgentLoopCheckpointV1,
        ) -> Result<(), Self::Error> {
            panic!("compile-time composition test")
        }

        async fn ensure_task_active(&mut self, _task_id: TaskId) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn commit_workflow_failure(
            &mut self,
            _task_id: TaskId,
            _failure: MigrationWorkflowFailureClassV1,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn prepare_oracle_workspace(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
        ) -> Result<OracleWorkspaceV1, Self::Error> {
            panic!("compile-time composition test")
        }

        fn derive_required_oracle_dimensions(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
            _workspace: &OracleWorkspaceV1,
        ) -> Result<Vec<OracleDimensionV1>, Self::Error> {
            panic!("compile-time composition test")
        }

        fn commit_oracle_portfolio_review_candidate(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _proposal: &OraclePortfolioProposalV1,
        ) -> Result<(), Self::Error> {
            panic!("compile-time composition test")
        }

        async fn commit_oracle_revision_request(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _request: &OracleRevisionRequestV1,
        ) -> Result<(), Self::Error> {
            panic!("compile-time composition test")
        }

        async fn qualify_oracle_admission_mechanisms(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
            _proposal: &OraclePortfolioProposalV1,
            _policy: &OracleAdmissionPolicyV1,
        ) -> Result<OracleAdmissionMechanismCatalogV1, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn run_qualified_oracle_controls(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
            _proposal: &OraclePortfolioProposalV1,
            _attempt: &OracleAdmissionAttemptV1,
        ) -> Result<OracleAdmissionEvidenceV1, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn authorize_candidate_build(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
            _oracle: &AdmittedOracleV1,
            _contract: &CandidateOracleContractV1,
            _candidate: &CandidateProposalV1,
            _attempt: &CandidateAdmissionAttemptV1,
        ) -> Result<Self::CandidateBuildAuthority, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn observe_candidate_on_worker(
            &mut self,
            _authority: Self::CandidateBuildAuthority,
            _attempt: &CandidateAdmissionAttemptV1,
        ) -> Result<CandidateAdmissionEvidenceV1, Self::Error> {
            panic!("compile-time composition test")
        }

        async fn record_terminal_outcome(
            &mut self,
            _outcome: &MigrationTerminalOutcomeV1,
        ) -> Result<(), Self::Error> {
            panic!("compile-time composition test")
        }
    }

    #[test]
    fn product_composition_implements_server_module_and_readable_workflow() {
        fn require_workflow<T: CudaMigrationWorkflow>() {}
        fn require_application<T: ApplicationModule>() {}

        require_workflow::<CudaMigrationApplication<NeverServices, NeverExecutor>>();
        require_application::<CudaMigrationApplication<NeverServices, NeverExecutor>>();
    }

    #[test]
    fn role_attempt_policy_restarts_only_safe_transport_classes_within_bound() {
        let limit = MigrationRoleAttemptLimitV1::new(NonZeroU16::new(8).expect("non-zero"));

        assert!(may_restart_migration_role_attempt(
            TransportFailureClass::NotSent,
            1,
            limit
        ));
        assert!(may_restart_migration_role_attempt(
            TransportFailureClass::Ambiguous,
            7,
            limit
        ));
        assert!(!may_restart_migration_role_attempt(
            TransportFailureClass::Rejected,
            1,
            limit
        ));
        assert!(!may_restart_migration_role_attempt(
            TransportFailureClass::Ambiguous,
            8,
            limit
        ));
    }

    #[test]
    fn terminal_identity_commits_each_admission_domain_separately() {
        let task_id = TaskId::new();
        let intent = ContentId::derive(b"intent").expect("intent identity");
        let oracle = ContentId::derive(b"oracle").expect("Oracle identity");
        let first_candidate = ContentId::derive(b"candidate-one").expect("Candidate identity");
        let second_candidate = ContentId::derive(b"candidate-two").expect("Candidate identity");
        let first =
            MigrationTerminalOutcomeV1::after_candidate(task_id, intent, oracle, first_candidate);
        let second =
            MigrationTerminalOutcomeV1::after_candidate(task_id, intent, oracle, second_candidate);
        assert_ne!(
            first.identity().expect("terminal identity"),
            second.identity().expect("terminal identity")
        );
    }
}
