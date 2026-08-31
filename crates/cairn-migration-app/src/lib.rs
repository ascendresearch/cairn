//! CUDA migration product composition above the domain-neutral server host.

use std::{collections::BTreeMap, fmt::Display, future::Future};

use cairn_agent::{
    AgentLoopCheckpointV1, AgentLoopContext, AgentLoopHooks, AgentLoopRunOutcomeV1,
    AgentLoopStartV1, AgentLoopStepExecutor, AgentLoopStepLimit, AgentRegistries,
    InitializedAgentLoopV1, KnowledgeRegistry, SkillRegistry, ToolRegistry, initialize_agent_loop,
    run_agent_loop,
};
use cairn_migration::{
    CandidateAdmissionAttemptV1, CandidateAdmissionDispositionV1,
    CandidateAdmissionEvidenceArtifact, CandidateAdmissionEvidenceV1,
    CandidateAdmissionOutcomeArtifact, CandidateAdmissionOutcomeV1, CandidateClaimStatusV1,
    CandidateExplorationAgentContextV1, CandidateExplorationRoleHooksV1,
    CandidateMechanismCatalogV1, CandidateOracleContractV1, CandidateProposalArtifact,
    CandidateProposalV1, CandidateReviewAgentContextV1, CandidateReviewRoleHooksV1,
    CandidateRevisionAgentContextV1, CandidateRevisionRoleHooksV1, CandidateWorkspaceV1,
    CudaMigrationWorkflow, IntentAdmissionPublicOutcomeArtifact, IntentDecisionRequestBatchV1,
    IntentHypothesisSetProposalV1, IntentRecoveryInputV1, MigrationRoleHooksV1,
    MigrationRoleStepObservationV1, OracleAdmissionAttemptV1, OracleAdmissionDispositionV1,
    OracleAdmissionEvidenceV1, OracleAdmissionMechanismCatalogV1, OracleAdmissionOutcomeArtifact,
    OracleAdmissionOutcomeV1, OracleAdmissionPolicyV1, OracleClaimAdmissionStatusV1,
    OracleExplorationAgentContextV1, OracleExplorationRoleHooksV1, OraclePortfolioProposalArtifact,
    OraclePortfolioProposalV1, OracleReviewAgentContextV1, OracleReviewRoleHooksV1,
    OracleRevisionAgentContextV1, OracleRevisionRoleHooksV1, OracleWorkspaceV1,
    PreparedIntentAdmissionV1, SirAgentContextV1, SirRoleHooksV1, SirTaskWorkspace,
    UserIntentAuthorityGrantV1, UserIntentDecisionRequestV1, UserIntentDecisionV1,
    derive_user_intent_decision_requests, promote_user_intent, recompute_candidate_admission,
    recompute_oracle_admission,
};
use cairn_protocol::{AgentLoopId, ContentId, ContentType, TaskId};
use cairn_server::{ApplicationModule, ApplicationName};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::mpsc;

/// Exact task snapshot admitted at the product composition boundary.
#[derive(Clone)]
pub struct FrozenMigrationTaskV1 {
    task_id: TaskId,
    workspace: SirTaskWorkspace,
    recovery_input: IntentRecoveryInputV1,
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
}

/// Administrator response plus the exact Controller-issued authority that permits it.
pub struct AuthorizedIntentDecisionV1 {
    request: UserIntentDecisionRequestV1,
    grant: UserIntentAuthorityGrantV1,
    decision: UserIntentDecisionV1,
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

/// Admitted Oracle authority and the exact workspace from which it was proposed.
#[derive(Clone)]
pub struct AdmittedOracleV1 {
    workspace: OracleWorkspaceV1,
    proposal: OraclePortfolioProposalV1,
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
    pub const fn outcome(&self) -> &OracleAdmissionOutcomeV1 {
        &self.outcome
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
pub struct MigrationTerminalOutcomeV1 {
    schema_version: u16,
    task_id: TaskId,
    intent: ContentId<IntentAdmissionPublicOutcomeArtifact>,
    oracle: ContentId<OracleAdmissionOutcomeArtifact>,
    candidate: ContentId<CandidateAdmissionOutcomeArtifact>,
}

impl MigrationTerminalOutcomeV1 {
    fn new(
        task_id: TaskId,
        intent: ContentId<IntentAdmissionPublicOutcomeArtifact>,
        oracle: ContentId<OracleAdmissionOutcomeArtifact>,
        candidate: ContentId<CandidateAdmissionOutcomeArtifact>,
    ) -> Self {
        Self {
            schema_version: 1,
            task_id,
            intent,
            oracle,
            candidate,
        }
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn intent(&self) -> ContentId<IntentAdmissionPublicOutcomeArtifact> {
        self.intent
    }

    #[must_use]
    pub const fn oracle(&self) -> ContentId<OracleAdmissionOutcomeArtifact> {
        self.oracle
    }

    #[must_use]
    pub const fn candidate(&self) -> ContentId<CandidateAdmissionOutcomeArtifact> {
        self.candidate
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
pub trait CudaMigrationProductServices: Send + 'static {
    type Request: Send + 'static;
    type CandidateBuildAuthority: Send + Sync;
    type Error: Display + Send + 'static;

    fn freeze_task(
        &mut self,
        request: Self::Request,
    ) -> impl Future<Output = Result<FrozenMigrationTaskV1, Self::Error>> + Send;

    fn await_administrator_intent_decision(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: &IntentHypothesisSetProposalV1,
        requests: &IntentDecisionRequestBatchV1,
    ) -> impl Future<Output = Result<AuthorizedIntentDecisionV1, Self::Error>> + Send;

    fn commit_intent_admission(
        &mut self,
        prepared: &PreparedIntentAdmissionV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn commit_suspended_agent_loop(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn prepare_oracle_workspace(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
    ) -> impl Future<Output = Result<OracleWorkspaceV1, Self::Error>> + Send;

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
    oracle_admission_policy: OracleAdmissionPolicyV1,
    oracle_mechanisms: OracleAdmissionMechanismCatalogV1,
    candidate_mechanisms: CandidateMechanismCatalogV1,
    initialized_loops: BTreeMap<AgentLoopId, InitializedAgentLoopV1>,
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
        oracle_admission_policy: OracleAdmissionPolicyV1,
        oracle_mechanisms: OracleAdmissionMechanismCatalogV1,
        candidate_mechanisms: CandidateMechanismCatalogV1,
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
            oracle_admission_policy,
            oracle_mechanisms,
            candidate_mechanisms,
            initialized_loops: BTreeMap::new(),
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
            .insert(loop_id, initialized)
            .is_some()
        {
            return Err(MigrationApplicationError::DuplicateAgentLoop(loop_id));
        }
        Ok(loop_id)
    }

    async fn run_role_loop<C, H>(
        &mut self,
        loop_id: AgentLoopId,
        context: &C,
        hooks: &H,
    ) -> Result<H::Output, MigrationApplicationError>
    where
        C: AgentLoopContext,
        H: AgentLoopHooks<C>,
        H::Error: Display,
        E: AgentLoopStepExecutor<C, H::StepObservation>,
        <E as AgentLoopStepExecutor<C, H::StepObservation>>::Error: Display,
    {
        let initialized = self
            .initialized_loops
            .remove(&loop_id)
            .ok_or(MigrationApplicationError::UnknownAgentLoop(loop_id))?;
        let registries = AgentRegistries {
            tools: &self.tools,
            skills: &self.skills,
            knowledge: &self.knowledge,
        };
        match run_agent_loop(initialized, context, hooks, registries, &mut self.executor)
            .await
            .map_err(|_| MigrationApplicationError::AgentLoopExecution)?
        {
            AgentLoopRunOutcomeV1::Complete { output, .. } => Ok(output),
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
    #[error("migration product service failed: {0}")]
    Product(String),
    #[error("migration domain operation failed: {0}")]
    Domain(String),
    #[error("migration authority binding failed: {0}")]
    Binding(&'static str),
    #[error("Agent Loop initialization failed")]
    AgentLoopInitialization,
    #[error("Agent Loop execution failed")]
    AgentLoopExecution,
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
    fn product(error: impl Display) -> Self {
        Self::Product(error.to_string())
    }

    fn domain(error: impl Display) -> Self {
        Self::Domain(error.to_string())
    }
}

impl<S, E> CudaMigrationWorkflow for CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
    E: AgentLoopStepExecutor<
            SirAgentContextV1,
            MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleExplorationAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
        > + AgentLoopStepExecutor<
            OracleReviewAgentContextV1,
            MigrationRoleStepObservationV1<ContentId<OraclePortfolioProposalArtifact>>,
        > + AgentLoopStepExecutor<
            OracleRevisionAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
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
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleExplorationAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<OraclePortfolioProposalArtifact>>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleRevisionAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateExplorationAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateRevisionAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: Display,
{
    type Error = MigrationApplicationError;
    type Request = S::Request;
    type FrozenTask = FrozenMigrationTaskV1;

    type SirContext = SirAgentContextV1;
    type SirDraft = IntentHypothesisSetProposalV1;
    type IntentDecisionRequests = IntentDecisionRequestBatchV1;
    type AdministratorIntentDecision = AuthorizedIntentDecisionV1;
    type AdmittedIntent = AdmittedIntentV1;

    type OracleExplorationContext = OracleExplorationAgentContextV1;
    type OracleDraft = OraclePortfolioProposalV1;
    type OracleReviewContext = OracleReviewAgentContextV1;
    type OracleReview = ContentId<OraclePortfolioProposalArtifact>;
    type OracleControlObservations = OracleAdmissionMaterialsV1;
    type OracleRevisionRequest = OracleAdmissionOutcomeV1;
    type OracleRevisionContext = OracleRevisionAgentContextV1;
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
        self.services
            .freeze_task(request)
            .await
            .map_err(MigrationApplicationError::product)
    }

    fn task_id(&self, task: &Self::FrozenTask) -> TaskId {
        task.task_id()
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
            &SirRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_sir_loop(
        &mut self,
        loop_id: AgentLoopId,
        _task: &Self::FrozenTask,
        context: Self::SirContext,
    ) -> Result<Self::SirDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &SirRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
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
        let request_id = decision
            .request
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if !requests.requests().iter().any(|request| {
            request == &decision.request
                && request
                    .identity()
                    .is_ok_and(|identity| identity == request_id)
        }) {
            return Err(MigrationApplicationError::Binding(
                "intent decision request batch",
            ));
        }
        let prepared = promote_user_intent(
            sir.identity().map_err(MigrationApplicationError::domain)?,
            &sir,
            task.recovery_input()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            task.recovery_input(),
            request_id,
            &decision.request,
            decision
                .grant
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            &decision.grant,
            decision
                .decision
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            &decision.decision,
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
            request_id = %request_id,
            outcome_id = %prepared.public_outcome().identity().map_err(MigrationApplicationError::domain)?,
            "Intent Admission committed exact authority lineage"
        );
        Ok(AdmittedIntentV1(prepared))
    }

    async fn prepare_oracle_exploration_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
    ) -> Result<Self::OracleExplorationContext, Self::Error> {
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
        let context = OracleExplorationAgentContextV1::new(
            task.task_id(),
            contract_id,
            workspace
                .identity()
                .map_err(MigrationApplicationError::domain)?,
        )
        .map_err(MigrationApplicationError::domain)?;
        self.oracle_workspace = Some(workspace);
        Ok(context)
    }

    async fn initialize_oracle_exploration_loop(
        &mut self,
        task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        context: &Self::OracleExplorationContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleExplorationRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_exploration_loop(
        &mut self,
        loop_id: AgentLoopId,
        _task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        context: Self::OracleExplorationContext,
    ) -> Result<Self::OracleDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleExplorationRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn prepare_oracle_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
    ) -> Result<Self::OracleReviewContext, Self::Error> {
        let workspace_id = self
            .oracle_workspace()?
            .identity()
            .map_err(MigrationApplicationError::domain)?;
        if draft.workspace() != workspace_id {
            return Err(MigrationApplicationError::Binding(
                "Oracle proposal workspace",
            ));
        }
        OracleReviewAgentContextV1::new(
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

    async fn initialize_oracle_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleReviewContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleReviewRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleReviewContext,
    ) -> Result<Self::OracleReview, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleReviewRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
    }

    async fn run_qualified_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
        review: &Self::OracleReview,
    ) -> Result<Self::OracleControlObservations, Self::Error> {
        if *review
            != draft
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Oracle review"));
        }
        let attempt = OracleAdmissionAttemptV1::new(
            draft,
            &self.oracle_admission_policy,
            &self.oracle_mechanisms,
        )
        .map_err(MigrationApplicationError::domain)?;
        let evidence = self
            .services
            .run_qualified_oracle_controls(task, intent, draft, &attempt)
            .await
            .map_err(MigrationApplicationError::product)?;
        Ok(OracleAdmissionMaterialsV1::new(
            self.oracle_admission_policy.clone(),
            self.oracle_mechanisms.clone(),
            attempt,
            evidence,
        ))
    }

    async fn admit_oracle(
        &mut self,
        _task: &Self::FrozenTask,
        _intent: &Self::AdmittedIntent,
        draft: Self::OracleDraft,
        review: Self::OracleReview,
        observations: Self::OracleControlObservations,
    ) -> Result<
        OracleAdmissionDispositionV1<
            Self::AdmittedOracle,
            Self::OracleDraft,
            Self::OracleRevisionRequest,
            Self::OracleControlObservations,
        >,
        Self::Error,
    > {
        if review
            != draft
                .identity()
                .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding("Oracle review"));
        }
        let outcome = recompute_oracle_admission(
            &draft,
            &observations.policy,
            &observations.mechanisms,
            &observations.attempt,
            &observations.evidence,
        )
        .map_err(MigrationApplicationError::domain)?;
        let admitted = outcome
            .claims()
            .iter()
            .all(|claim| claim.status() == OracleClaimAdmissionStatusV1::Admitted);
        tracing::info!(
            target: "cairn.migration.admission",
            event = "oracle_admission_recomputed",
            proposal_id = %outcome.proposal(),
            evidence_id = %outcome.evidence(),
            admitted,
            "Oracle Admission mechanically recomputed"
        );
        if admitted {
            Ok(OracleAdmissionDispositionV1::Admitted(AdmittedOracleV1 {
                workspace: self.oracle_workspace()?.clone(),
                proposal: draft,
                outcome,
            }))
        } else {
            Ok(OracleAdmissionDispositionV1::Revise {
                draft,
                request: outcome,
                control_observations: observations,
            })
        }
    }

    async fn prepare_oracle_revision_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
        request: &Self::OracleRevisionRequest,
        observations: &Self::OracleControlObservations,
    ) -> Result<Self::OracleRevisionContext, Self::Error> {
        if request.proposal()
            != draft
                .identity()
                .map_err(MigrationApplicationError::domain)?
            || request.evidence()
                != observations
                    .evidence
                    .identity()
                    .map_err(MigrationApplicationError::domain)?
        {
            return Err(MigrationApplicationError::Binding(
                "Oracle revision evidence",
            ));
        }
        OracleRevisionAgentContextV1::new(
            task.task_id(),
            intent
                .prepared()
                .public_outcome()
                .contract()
                .identity()
                .map_err(MigrationApplicationError::domain)?,
            request.proposal(),
            request.evidence(),
        )
        .map_err(MigrationApplicationError::domain)
    }

    async fn initialize_oracle_revision_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleRevisionContext,
    ) -> Result<AgentLoopId, Self::Error> {
        self.initialize_role_loop(
            task.task_id(),
            context,
            &OracleRevisionRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
    }

    async fn run_oracle_revision_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleRevisionContext,
    ) -> Result<Self::OracleDraft, Self::Error> {
        self.run_role_loop(
            loop_id,
            &context,
            &OracleRevisionRoleHooksV1::new().map_err(MigrationApplicationError::domain)?,
        )
        .await
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
        let attempt =
            CandidateAdmissionAttemptV1::new(&contract, candidate, &self.candidate_mechanisms)
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
            self.candidate_mechanisms.clone(),
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
        let outcome = MigrationTerminalOutcomeV1::new(
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

impl<S, E> ApplicationModule for CudaMigrationApplication<S, E>
where
    S: CudaMigrationProductServices,
    E: AgentLoopStepExecutor<
            SirAgentContextV1,
            MigrationRoleStepObservationV1<IntentHypothesisSetProposalV1>,
        > + AgentLoopStepExecutor<
            OracleExplorationAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
        > + AgentLoopStepExecutor<
            OracleReviewAgentContextV1,
            MigrationRoleStepObservationV1<ContentId<OraclePortfolioProposalArtifact>>,
        > + AgentLoopStepExecutor<
            OracleRevisionAgentContextV1,
            MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
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
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleExplorationAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<OraclePortfolioProposalArtifact>>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        OracleRevisionAgentContextV1,
        MigrationRoleStepObservationV1<OraclePortfolioProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateExplorationAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateReviewAgentContextV1,
        MigrationRoleStepObservationV1<ContentId<CandidateProposalArtifact>>,
    >>::Error: Display,
    <E as AgentLoopStepExecutor<
        CandidateRevisionAgentContextV1,
        MigrationRoleStepObservationV1<CandidateProposalV1>,
    >>::Error: Display,
{
    type Error = MigrationApplicationError;

    fn name(&self) -> &ApplicationName {
        &self.name
    }

    async fn run(mut self) -> Result<(), Self::Error> {
        while let Some(request) = self.inbox.recv().await {
            let terminal = cairn_migration::run_cuda_migration(&mut self, request).await?;
            tracing::info!(
                target: "cairn.migration.application",
                event = "migration_terminal_recorded",
                terminal_id = %terminal.identity()?,
                "CUDA migration terminal outcome recorded"
            );
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
        ) -> Result<AuthorizedIntentDecisionV1, Self::Error> {
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

        async fn prepare_oracle_workspace(
            &mut self,
            _task: &FrozenMigrationTaskV1,
            _intent: &AdmittedIntentV1,
        ) -> Result<OracleWorkspaceV1, Self::Error> {
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
    fn terminal_identity_commits_each_admission_domain_separately() {
        let task_id = TaskId::new();
        let intent = ContentId::derive(b"intent").expect("intent identity");
        let oracle = ContentId::derive(b"oracle").expect("Oracle identity");
        let first_candidate = ContentId::derive(b"candidate-one").expect("Candidate identity");
        let second_candidate = ContentId::derive(b"candidate-two").expect("Candidate identity");
        let first = MigrationTerminalOutcomeV1::new(task_id, intent, oracle, first_candidate);
        let second = MigrationTerminalOutcomeV1::new(task_id, intent, oracle, second_candidate);
        assert_ne!(
            first.identity().expect("terminal identity"),
            second.identity().expect("terminal identity")
        );
    }
}
