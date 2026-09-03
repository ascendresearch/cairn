use std::future::Future;

use cairn_protocol::{AgentLoopId, TaskId};

use crate::{
    CandidateBuildOutcomeV1, CandidateIterationOrdinal, CandidateIterationsRemaining,
    CandidateSearchNextActionV1, CandidateSearchNoticeV1, CandidateSearchParentV1,
    CandidateSearchStateV1, CandidateSearchTerminalV1, ReasoningDecompositionPolicyV1,
};

/// Exact prior lineage exposed to one dimension item-discovery Agent Loop.
pub enum OracleItemDiscoveryLineageV1<'a, S, R> {
    Initial,
    ReviewRevision { previous: &'a S, review: &'a R },
}

/// Exact prior lineage exposed to one Oracle item development loop.
///
/// Invalid combinations such as feedback without a prior draft or Admission feedback on an
/// initial draft are intentionally unrepresentable at the workflow port.
pub enum OracleItemDevelopmentLineageV1<'a, D, R, C, A> {
    Initial,
    ReviewRevision {
        previous: &'a D,
        review: &'a R,
    },
    CoherenceRevision {
        previous: &'a D,
        coherence: &'a C,
        review: Option<&'a R>,
    },
    AdmissionRevision {
        previous: &'a D,
        admission: &'a A,
        review: Option<&'a R>,
    },
}

/// Exact whole-portfolio lineage for the minimal-decomposition ablation arm.
pub enum OracleWholePortfolioLineageV1<'a, D, A> {
    Initial,
    AdmissionRevision { previous: &'a D, admission: &'a A },
}

/// Product-owned ports beneath the readable CUDA migration workflow.
///
/// Cognitive activities are split into role-scoped Agent Loops. Admission and authority-granting
/// activities remain independent mechanical calls.
#[allow(
    clippy::missing_errors_doc,
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "workflow ports preserve product errors and explicit authority-boundary types"
)]
pub trait CudaMigrationWorkflow: Send {
    type Error: Send;
    type Request: Send;
    type FrozenTask: Send + Sync;

    type SirContext: Send + Sync;
    type SirDraft: Send + Sync;
    type IntentDecisionRequests: Send + Sync;
    type AdministratorIntentDecision: Send + Sync;
    type AdmittedIntent: Send + Sync;

    type OracleWorkspace: Send + Sync;
    type OracleDimension: Send + Sync;
    type OracleWholePortfolioContext: Send + Sync;
    type OracleItemDiscoveryContext: Send + Sync;
    type OracleItemSet: Send + Sync;
    type OracleItemSetReviewContext: Send + Sync;
    type OracleItemSetReview: Send + Sync;
    type OracleItem: Send + Sync;
    type OracleItemDevelopmentContext: Send + Sync;
    type OracleItemDraft: Send + Sync;
    type OracleItemReviewContext: Send + Sync;
    type OracleItemReview: Send + Sync;
    type AcceptedOracleItem: Send;
    type OracleDraft: Send + Sync;
    type OraclePortfolioReviewContext: Send + Sync;
    type OraclePortfolioReview: Send + Sync;
    type ReviewedOracleDraft: Send + Sync;
    type OracleControlObservations: Send + Sync;
    type OracleRevisionRequest: Send + Sync;
    type OracleControlReconciliationRequest: Send + Sync;
    type AdmittedOracle: Send + Sync;

    type CandidateExplorationContext: Send + Sync;
    type CandidateDraft: Send + Sync;
    type CandidateBuildAuthority: Send + Sync;
    type CandidateRevisionContext: Send + Sync;
    type AdmittedCandidate: Send + Sync;
    type TerminalOutcome: Send;

    fn freeze_task(
        &mut self,
        request: Self::Request,
    ) -> impl Future<Output = Result<Self::FrozenTask, Self::Error>> + Send;

    fn task_id(&self, task: &Self::FrozenTask) -> TaskId;

    fn reasoning_decomposition(&self, task: &Self::FrozenTask) -> ReasoningDecompositionPolicyV1;

    fn prepare_sir_context(
        &mut self,
        task: &Self::FrozenTask,
    ) -> impl Future<Output = Result<Self::SirContext, Self::Error>> + Send;

    fn initialize_sir_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::SirContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_sir_loop(
        &mut self,
        loop_id: AgentLoopId,
        task: &Self::FrozenTask,
        context: Self::SirContext,
    ) -> impl Future<Output = Result<Self::SirDraft, Self::Error>> + Send;

    fn derive_intent_decision_requests(
        &mut self,
        task: &Self::FrozenTask,
        sir: &Self::SirDraft,
    ) -> impl Future<Output = Result<Self::IntentDecisionRequests, Self::Error>> + Send;

    fn await_administrator_intent_decision(
        &mut self,
        task: &Self::FrozenTask,
        sir: &Self::SirDraft,
        requests: &Self::IntentDecisionRequests,
    ) -> impl Future<Output = Result<Self::AdministratorIntentDecision, Self::Error>> + Send;

    fn admit_intent(
        &mut self,
        task: &Self::FrozenTask,
        sir: Self::SirDraft,
        requests: Self::IntentDecisionRequests,
        decision: Self::AdministratorIntentDecision,
    ) -> impl Future<Output = Result<Self::AdmittedIntent, Self::Error>> + Send;

    fn prepare_oracle_workspace(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
    ) -> impl Future<Output = Result<Self::OracleWorkspace, Self::Error>> + Send;

    fn derive_required_oracle_dimensions(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
    ) -> Result<Vec<Self::OracleDimension>, Self::Error>;

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
    ) -> Result<Self::OracleWholePortfolioContext, Self::Error>;

    fn initialize_oracle_whole_portfolio_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleWholePortfolioContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_whole_portfolio_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleWholePortfolioContext,
    ) -> impl Future<Output = Result<Self::OracleDraft, Self::Error>> + Send;

    fn accept_oracle_whole_portfolio_proposal(
        &mut self,
        draft: Self::OracleDraft,
    ) -> Result<Self::ReviewedOracleDraft, Self::Error>;

    fn prepare_oracle_item_discovery_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        workspace: &Self::OracleWorkspace,
        dimension: &Self::OracleDimension,
        lineage: OracleItemDiscoveryLineageV1<'_, Self::OracleItemSet, Self::OracleItemSetReview>,
    ) -> Result<Self::OracleItemDiscoveryContext, Self::Error>;

    fn initialize_oracle_item_discovery_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemDiscoveryContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_item_discovery_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemDiscoveryContext,
    ) -> impl Future<Output = Result<Self::OracleItemSet, Self::Error>> + Send;

    fn prepare_oracle_item_set_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        item_set: &Self::OracleItemSet,
    ) -> Result<Self::OracleItemSetReviewContext, Self::Error>;

    fn initialize_oracle_item_set_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemSetReviewContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_item_set_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemSetReviewContext,
    ) -> impl Future<
        Output = Result<
            OracleReviewDispositionV1<Self::OracleItemSetReview, Self::OracleItemSetReview>,
            Self::Error,
        >,
    > + Send;

    fn validate_and_expand_oracle_item_set(
        &mut self,
        dimension: &Self::OracleDimension,
        item_set: Self::OracleItemSet,
    ) -> Result<Vec<Self::OracleItem>, Self::Error>;

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
    ) -> Result<Self::OracleItemDevelopmentContext, Self::Error>;

    fn initialize_oracle_item_development_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemDevelopmentContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_item_development_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemDevelopmentContext,
    ) -> impl Future<Output = Result<Self::OracleItemDraft, Self::Error>> + Send;

    fn prepare_oracle_item_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleItemDraft,
    ) -> Result<Self::OracleItemReviewContext, Self::Error>;

    fn initialize_oracle_item_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleItemReviewContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_item_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleItemReviewContext,
    ) -> impl Future<
        Output = Result<
            OracleReviewDispositionV1<Self::OracleItemReview, Self::OracleItemReview>,
            Self::Error,
        >,
    > + Send;

    fn accept_reviewed_oracle_item(
        &mut self,
        item: Self::OracleItem,
        draft: Self::OracleItemDraft,
        review: Self::OracleItemReview,
    ) -> Result<Self::AcceptedOracleItem, Self::Error>;

    fn assemble_oracle_portfolio(
        &mut self,
        workspace: &Self::OracleWorkspace,
        dimensions: Vec<Self::OracleDimension>,
        accepted_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error>;

    fn prepare_oracle_portfolio_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
    ) -> Result<Self::OraclePortfolioReviewContext, Self::Error>;

    fn initialize_oracle_portfolio_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OraclePortfolioReviewContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_portfolio_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OraclePortfolioReviewContext,
    ) -> impl Future<
        Output = Result<
            OracleReviewDispositionV1<Self::OraclePortfolioReview, Self::OraclePortfolioReview>,
            Self::Error,
        >,
    > + Send;

    fn prepare_oracle_items_for_coherence_revision(
        &mut self,
        draft: &Self::OracleDraft,
        review: &Self::OraclePortfolioReview,
    ) -> Result<Vec<(Self::OracleItem, Self::OracleItemDraft)>, Self::Error>;

    fn accept_oracle_portfolio_review(
        &mut self,
        draft: Self::OracleDraft,
        review: Self::OraclePortfolioReview,
    ) -> Result<Self::ReviewedOracleDraft, Self::Error>;

    fn replace_oracle_items_after_coherence_revision(
        &mut self,
        draft: Self::OracleDraft,
        revised_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error>;

    fn prepare_oracle_items_for_admission_revision(
        &mut self,
        task: &Self::FrozenTask,
        draft: &Self::ReviewedOracleDraft,
        request: &Self::OracleRevisionRequest,
    ) -> impl Future<Output = Result<Vec<(Self::OracleItem, Self::OracleItemDraft)>, Self::Error>> + Send;

    fn replace_oracle_items_after_admission_revision(
        &mut self,
        draft: Self::ReviewedOracleDraft,
        revised_items: Vec<Self::AcceptedOracleItem>,
    ) -> Result<Self::OracleDraft, Self::Error>;

    fn run_qualified_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::ReviewedOracleDraft,
    ) -> impl Future<Output = Result<Self::OracleControlObservations, Self::Error>> + Send;

    #[expect(
        clippy::type_complexity,
        reason = "the explicit admitted, rejected, revision, and observation types are authority boundaries"
    )]
    fn admit_oracle(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: Self::ReviewedOracleDraft,
        observations: Self::OracleControlObservations,
    ) -> impl Future<
        Output = Result<
            OracleAdmissionDispositionV1<
                Self::AdmittedOracle,
                Self::ReviewedOracleDraft,
                Self::OracleRevisionRequest,
                Self::OracleControlReconciliationRequest,
                Self::OracleControlObservations,
            >,
            Self::Error,
        >,
    > + Send;

    fn reconcile_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::ReviewedOracleDraft,
        request: &Self::OracleControlReconciliationRequest,
    ) -> impl Future<Output = Result<Self::OracleControlObservations, Self::Error>> + Send;

    /// Opens or recovers the Controller-owned durable search loop for this task.
    fn open_candidate_search(
        &mut self,
        task: &Self::FrozenTask,
    ) -> impl Future<Output = Result<CandidateSearchStateV1, Self::Error>> + Send;

    fn prepare_candidate_exploration_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        iteration: CandidateIterationOrdinal,
        remaining: CandidateIterationsRemaining,
        notice: Option<CandidateSearchNoticeV1>,
    ) -> impl Future<Output = Result<Self::CandidateExplorationContext, Self::Error>> + Send;

    fn initialize_candidate_exploration_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateExplorationContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    /// Runs one proposal episode. `None` is an episode that produced no proposal at all, which is
    /// a failed attempt the Controller counts, never a finished search.
    fn run_candidate_exploration_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateExplorationContext,
    ) -> impl Future<Output = Result<Option<Self::CandidateDraft>, Self::Error>> + Send;

    fn prepare_candidate_revision_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        parent: CandidateSearchParentV1,
        iteration: CandidateIterationOrdinal,
        remaining: CandidateIterationsRemaining,
        notice: Option<CandidateSearchNoticeV1>,
    ) -> impl Future<Output = Result<Self::CandidateRevisionContext, Self::Error>> + Send;

    fn initialize_candidate_revision_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateRevisionContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_candidate_revision_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateRevisionContext,
    ) -> impl Future<Output = Result<Option<Self::CandidateDraft>, Self::Error>> + Send;

    fn record_candidate_proposal(
        &mut self,
        task: &Self::FrozenTask,
        candidate: &Self::CandidateDraft,
    ) -> impl Future<Output = Result<CandidateSearchStateV1, Self::Error>> + Send;

    fn record_missing_candidate_submission(
        &mut self,
        task: &Self::FrozenTask,
    ) -> impl Future<Output = Result<CandidateSearchStateV1, Self::Error>> + Send;

    fn authorize_candidate_build(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
    ) -> impl Future<Output = Result<Self::CandidateBuildAuthority, Self::Error>> + Send;

    /// Observes one build. The outcome is a search signal and never an admission verdict.
    fn observe_candidate_build(
        &mut self,
        authority: Self::CandidateBuildAuthority,
    ) -> impl Future<Output = Result<CandidateBuildOutcomeV1, Self::Error>> + Send;

    fn record_candidate_build_observation(
        &mut self,
        task: &Self::FrozenTask,
        outcome: CandidateBuildOutcomeV1,
    ) -> impl Future<Output = Result<CandidateSearchStateV1, Self::Error>> + Send;

    fn admit_candidate(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: Self::CandidateDraft,
    ) -> impl Future<Output = Result<Self::AdmittedCandidate, Self::Error>> + Send;

    /// Records the honest terminal for a search that stopped without a compiling candidate.
    fn record_candidate_search_stop(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        terminal: CandidateSearchTerminalV1,
    ) -> impl Future<Output = Result<Self::TerminalOutcome, Self::Error>> + Send;

    fn record_terminal_outcome(
        &mut self,
        task: Self::FrozenTask,
        intent: Self::AdmittedIntent,
        oracle: Self::AdmittedOracle,
        candidate: Self::AdmittedCandidate,
    ) -> impl Future<Output = Result<Self::TerminalOutcome, Self::Error>> + Send;
}

/// Mechanical Oracle admission either grants authority or returns the exact rejected evidence for
/// a revision loop. The Agent cannot construct an admitted value.
pub enum OracleAdmissionDispositionV1<A, D, R, Q, O> {
    Admitted(A),
    Revise {
        draft: D,
        request: R,
        control_observations: O,
    },
    Reconcile {
        draft: D,
        request: Q,
        control_observations: O,
    },
}

/// Independent Oracle Review either approves the exact proposal or returns actionable feedback
/// for the revision loop. A bare rejection is not representable.
pub enum OracleReviewDispositionV1<A, R> {
    Approved(A),
    Revise(R),
}

/// How the candidate stage resolved. A stopped search is an outcome, not an error.
pub enum CandidateResolutionV1<A> {
    Admitted(A),
    SearchStopped(CandidateSearchTerminalV1),
}

/// Complete product workflow, intentionally written in business order.
pub async fn run_cuda_migration<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    request: W::Request,
) -> Result<W::TerminalOutcome, W::Error> {
    let task = workflow.freeze_task(request).await?;
    let task_id = workflow.task_id(&task);
    tracing::info!(
        target: "cairn.migration.workflow",
        event = "cuda_migration_started",
        task_id = %task_id,
        "CUDA migration workflow started"
    );
    let intent = establish_intent(workflow, &task).await?;
    let oracle = establish_oracle(workflow, &task, &intent).await?;
    match establish_candidate(workflow, &task, &intent, &oracle).await? {
        CandidateResolutionV1::Admitted(candidate) => {
            complete_cuda_migration(workflow, task, intent, oracle, candidate).await
        }
        CandidateResolutionV1::SearchStopped(terminal) => {
            workflow
                .record_candidate_search_stop(&task, &intent, &oracle, terminal)
                .await
        }
    }
}

/// Runs SIR as a role-scoped loop, obtains administrator decisions, then applies Intent Admission.
async fn establish_intent<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
) -> Result<W::AdmittedIntent, W::Error> {
    let context = workflow.prepare_sir_context(task).await?;
    let loop_id = workflow.initialize_sir_loop(task, &context).await?;
    tracing::info!(
        target: "cairn.migration.workflow",
        event = "sir_agent_loop_initialized",
        task_id = %workflow.task_id(task),
        loop_id = %loop_id,
        "SIR Agent Loop initialized"
    );
    let sir = workflow.run_sir_loop(loop_id, task, context).await?;
    let requests = workflow.derive_intent_decision_requests(task, &sir).await?;
    let decision = workflow
        .await_administrator_intent_decision(task, &sir, &requests)
        .await?;
    let admitted = workflow.admit_intent(task, sir, requests, decision).await?;
    tracing::info!(
        target: "cairn.migration.workflow",
        event = "migration_intent_admitted",
        task_id = %workflow.task_id(task),
        "migration intent admitted"
    );
    Ok(admitted)
}

/// Discovers, develops, and independently reviews every Oracle item before portfolio controls and
/// mechanical Admission. A rejected item alone is regenerated; accepted siblings stay frozen.
#[allow(
    clippy::too_many_lines,
    reason = "the readable product workflow intentionally keeps the complete nested authority order visible"
)]
async fn establish_oracle<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
    intent: &W::AdmittedIntent,
) -> Result<W::AdmittedOracle, W::Error> {
    let workspace = workflow.prepare_oracle_workspace(task, intent).await?;
    let dimensions = workflow.derive_required_oracle_dimensions(task, intent, &workspace)?;
    if workflow.reasoning_decomposition(task)
        == ReasoningDecompositionPolicyV1::MinimalDecomposition
    {
        return establish_minimal_oracle(workflow, task, intent, &workspace, &dimensions).await;
    }
    let mut accepted_items = Vec::new();
    for dimension in &dimensions {
        let mut previous_item_set = None;
        let mut item_set_review = None;
        let item_set = loop {
            let discovery_context = workflow.prepare_oracle_item_discovery_context(
                task,
                intent,
                &workspace,
                dimension,
                match (&previous_item_set, &item_set_review) {
                    (None, None) => OracleItemDiscoveryLineageV1::Initial,
                    (Some(previous), Some(review)) => {
                        OracleItemDiscoveryLineageV1::ReviewRevision { previous, review }
                    }
                    _ => unreachable!("Controller owns exact item-set Review lineage"),
                },
            )?;
            let discovery_loop = workflow
                .initialize_oracle_item_discovery_loop(task, &discovery_context)
                .await?;
            let proposed = workflow
                .run_oracle_item_discovery_loop(discovery_loop, discovery_context)
                .await?;
            let review_context =
                workflow.prepare_oracle_item_set_review_context(task, intent, &proposed)?;
            let review_loop = workflow
                .initialize_oracle_item_set_review_loop(task, &review_context)
                .await?;
            match workflow
                .run_oracle_item_set_review_loop(review_loop, review_context)
                .await?
            {
                OracleReviewDispositionV1::Approved(_) => break proposed,
                OracleReviewDispositionV1::Revise(review) => {
                    previous_item_set = Some(proposed);
                    item_set_review = Some(review);
                }
            }
        };
        let items = workflow.validate_and_expand_oracle_item_set(dimension, item_set)?;
        tracing::info!(
            target: "cairn.migration.workflow",
            event = "oracle_dimension_items_discovered",
            task_id = %workflow.task_id(task),
            item_count = items.len(),
            "Oracle dimension expanded into independently developed items"
        );
        for item in items {
            let mut previous_draft = None;
            let mut review_feedback = None;
            loop {
                let development_context = workflow.prepare_oracle_item_development_context(
                    task,
                    intent,
                    &workspace,
                    &item,
                    match (&previous_draft, &review_feedback) {
                        (None, None) => OracleItemDevelopmentLineageV1::Initial,
                        (Some(previous), Some(review)) => {
                            OracleItemDevelopmentLineageV1::ReviewRevision { previous, review }
                        }
                        _ => unreachable!("Controller owns exact item Review lineage"),
                    },
                )?;
                let development_loop = workflow
                    .initialize_oracle_item_development_loop(task, &development_context)
                    .await?;
                let draft = workflow
                    .run_oracle_item_development_loop(development_loop, development_context)
                    .await?;
                let review_context =
                    workflow.prepare_oracle_item_review_context(task, intent, &draft)?;
                let review_loop = workflow
                    .initialize_oracle_item_review_loop(task, &review_context)
                    .await?;
                match workflow
                    .run_oracle_item_review_loop(review_loop, review_context)
                    .await?
                {
                    OracleReviewDispositionV1::Approved(review) => {
                        accepted_items
                            .push(workflow.accept_reviewed_oracle_item(item, draft, review)?);
                        break;
                    }
                    OracleReviewDispositionV1::Revise(review) => {
                        tracing::info!(
                            target: "cairn.migration.workflow",
                            event = "oracle_item_review_requested_revision",
                            task_id = %workflow.task_id(task),
                            "Oracle item Review returned exact actionable feedback"
                        );
                        previous_draft = Some(draft);
                        review_feedback = Some(review);
                    }
                }
            }
        }
    }
    let mut draft = workflow.assemble_oracle_portfolio(&workspace, dimensions, accepted_items)?;
    loop {
        let reviewed = loop {
            let context = workflow.prepare_oracle_portfolio_review_context(task, intent, &draft)?;
            let review_loop = workflow
                .initialize_oracle_portfolio_review_loop(task, &context)
                .await?;
            match workflow
                .run_oracle_portfolio_review_loop(review_loop, context)
                .await?
            {
                OracleReviewDispositionV1::Approved(review) => {
                    break workflow.accept_oracle_portfolio_review(draft, review)?;
                }
                OracleReviewDispositionV1::Revise(coherence) => {
                    let targets =
                        workflow.prepare_oracle_items_for_coherence_revision(&draft, &coherence)?;
                    let mut revised_items = Vec::new();
                    for (item, initial_draft) in targets {
                        let mut previous_draft = initial_draft;
                        let mut review_feedback = None;
                        loop {
                            let context = workflow.prepare_oracle_item_development_context(
                                task,
                                intent,
                                &workspace,
                                &item,
                                OracleItemDevelopmentLineageV1::CoherenceRevision {
                                    previous: &previous_draft,
                                    coherence: &coherence,
                                    review: review_feedback.as_ref(),
                                },
                            )?;
                            let development_loop = workflow
                                .initialize_oracle_item_development_loop(task, &context)
                                .await?;
                            let revised = workflow
                                .run_oracle_item_development_loop(development_loop, context)
                                .await?;
                            let review_context = workflow
                                .prepare_oracle_item_review_context(task, intent, &revised)?;
                            let item_review_loop = workflow
                                .initialize_oracle_item_review_loop(task, &review_context)
                                .await?;
                            match workflow
                                .run_oracle_item_review_loop(item_review_loop, review_context)
                                .await?
                            {
                                OracleReviewDispositionV1::Approved(review) => {
                                    revised_items.push(
                                        workflow
                                            .accept_reviewed_oracle_item(item, revised, review)?,
                                    );
                                    break;
                                }
                                OracleReviewDispositionV1::Revise(review) => {
                                    previous_draft = revised;
                                    review_feedback = Some(review);
                                }
                            }
                        }
                    }
                    draft = workflow
                        .replace_oracle_items_after_coherence_revision(draft, revised_items)?;
                }
            }
        };
        let mut reviewed = reviewed;
        let mut observations = workflow
            .run_qualified_oracle_controls(task, intent, &reviewed)
            .await?;
        loop {
            match workflow
                .admit_oracle(task, intent, reviewed, observations)
                .await?
            {
                OracleAdmissionDispositionV1::Admitted(oracle) => {
                    tracing::info!(
                        target: "cairn.migration.workflow",
                        event = "oracle_admitted",
                        task_id = %workflow.task_id(task),
                        "Oracle portfolio admitted"
                    );
                    return Ok(oracle);
                }
                OracleAdmissionDispositionV1::Reconcile {
                    draft: unresolved,
                    request,
                    control_observations: _,
                } => {
                    observations = workflow
                        .reconcile_oracle_controls(task, intent, &unresolved, &request)
                        .await?;
                    reviewed = unresolved;
                }
                OracleAdmissionDispositionV1::Revise {
                    draft: rejected,
                    request,
                    control_observations: _,
                } => {
                    let targets = workflow
                        .prepare_oracle_items_for_admission_revision(task, &rejected, &request)
                        .await?;
                    let mut revised_items = Vec::new();
                    for (item, initial_draft) in targets {
                        let mut previous_draft = initial_draft;
                        let mut review_feedback = None;
                        loop {
                            let context = workflow.prepare_oracle_item_development_context(
                                task,
                                intent,
                                &workspace,
                                &item,
                                OracleItemDevelopmentLineageV1::AdmissionRevision {
                                    previous: &previous_draft,
                                    admission: &request,
                                    review: review_feedback.as_ref(),
                                },
                            )?;
                            let loop_id = workflow
                                .initialize_oracle_item_development_loop(task, &context)
                                .await?;
                            let revised = workflow
                                .run_oracle_item_development_loop(loop_id, context)
                                .await?;
                            let review_context = workflow
                                .prepare_oracle_item_review_context(task, intent, &revised)?;
                            let review_loop = workflow
                                .initialize_oracle_item_review_loop(task, &review_context)
                                .await?;
                            match workflow
                                .run_oracle_item_review_loop(review_loop, review_context)
                                .await?
                            {
                                OracleReviewDispositionV1::Approved(review) => {
                                    revised_items.push(
                                        workflow
                                            .accept_reviewed_oracle_item(item, revised, review)?,
                                    );
                                    break;
                                }
                                OracleReviewDispositionV1::Revise(review) => {
                                    previous_draft = revised;
                                    review_feedback = Some(review);
                                }
                            }
                        }
                    }
                    draft = workflow
                        .replace_oracle_items_after_admission_revision(rejected, revised_items)?;
                    break;
                }
            }
        }
    }
}

/// Runs the A-arm topology without manufacturing independent Review facts.
async fn establish_minimal_oracle<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
    intent: &W::AdmittedIntent,
    workspace: &W::OracleWorkspace,
    dimensions: &[W::OracleDimension],
) -> Result<W::AdmittedOracle, W::Error> {
    let mut previous = None;
    let mut admission_feedback = None;
    loop {
        let context = workflow.prepare_oracle_whole_portfolio_context(
            task,
            intent,
            workspace,
            dimensions,
            match (&previous, &admission_feedback) {
                (None, None) => OracleWholePortfolioLineageV1::Initial,
                (Some(previous), Some(admission)) => {
                    OracleWholePortfolioLineageV1::AdmissionRevision {
                        previous,
                        admission,
                    }
                }
                _ => unreachable!("Controller owns exact whole-portfolio lineage"),
            },
        )?;
        let loop_id = workflow
            .initialize_oracle_whole_portfolio_loop(task, &context)
            .await?;
        let draft = workflow
            .run_oracle_whole_portfolio_loop(loop_id, context)
            .await?;
        let mut reviewed = workflow.accept_oracle_whole_portfolio_proposal(draft)?;
        let mut observations = workflow
            .run_qualified_oracle_controls(task, intent, &reviewed)
            .await?;
        loop {
            match workflow
                .admit_oracle(task, intent, reviewed, observations)
                .await?
            {
                OracleAdmissionDispositionV1::Admitted(oracle) => return Ok(oracle),
                OracleAdmissionDispositionV1::Reconcile {
                    draft,
                    request,
                    control_observations: _,
                } => {
                    observations = workflow
                        .reconcile_oracle_controls(task, intent, &draft, &request)
                        .await?;
                    reviewed = draft;
                }
                OracleAdmissionDispositionV1::Revise {
                    draft,
                    request,
                    control_observations: _,
                } => {
                    previous = Some(draft);
                    admission_feedback = Some(request);
                    break;
                }
            }
        }
    }
}

/// Explores, reviews, builds, observes, and mechanically admits a Candidate.
/// Runs the Controller-owned candidate search loop, then reviews and admits what it settled on.
///
/// The loop belongs to the Controller, not to the model. Every transition is decided from durable
/// state the actor can neither see nor write, which is what lets the Controller notice a repeated
/// proposal, an episode that submitted nothing, and a budget about to run out. An actor cannot
/// observe any of those about itself, so being told is the only way it can change course.
async fn establish_candidate<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
    intent: &W::AdmittedIntent,
    oracle: &W::AdmittedOracle,
) -> Result<CandidateResolutionV1<W::AdmittedCandidate>, W::Error> {
    let mut state = workflow.open_candidate_search(task).await?;
    let mut current: Option<W::CandidateDraft> = None;
    let candidate = loop {
        match state.next_action() {
            CandidateSearchNextActionV1::None => {
                unreachable!("Controller opens the search loop before reading its next action")
            }
            CandidateSearchNextActionV1::RequestProposal {
                iteration,
                remaining,
                parent,
                notice,
            } => {
                let proposal = match parent {
                    None => {
                        let context = workflow
                            .prepare_candidate_exploration_context(
                                task, intent, oracle, iteration, remaining, notice,
                            )
                            .await?;
                        let loop_id = workflow
                            .initialize_candidate_exploration_loop(task, &context)
                            .await?;
                        workflow
                            .run_candidate_exploration_loop(loop_id, context)
                            .await?
                    }
                    Some(parent) => {
                        let context = workflow
                            .prepare_candidate_revision_context(
                                task, intent, oracle, parent, iteration, remaining, notice,
                            )
                            .await?;
                        let loop_id = workflow
                            .initialize_candidate_revision_loop(task, &context)
                            .await?;
                        workflow
                            .run_candidate_revision_loop(loop_id, context)
                            .await?
                    }
                };
                state = match &proposal {
                    Some(draft) => workflow.record_candidate_proposal(task, draft).await?,
                    None => workflow.record_missing_candidate_submission(task).await?,
                };
                current = proposal;
            }
            CandidateSearchNextActionV1::RequestBuild { iteration, .. } => {
                let Some(draft) = current.as_ref() else {
                    unreachable!("a build is only requested for the proposal just recorded")
                };
                let authority = workflow
                    .authorize_candidate_build(task, intent, oracle, draft)
                    .await?;
                let outcome = workflow.observe_candidate_build(authority).await?;
                tracing::info!(
                    target: "cairn.migration.workflow",
                    event = "candidate_build_observed",
                    task_id = %workflow.task_id(task),
                    iteration = iteration.get(),
                    compiled = outcome.compiled(),
                    "candidate search folded one build observation back into durable state"
                );
                state = workflow
                    .record_candidate_build_observation(task, outcome)
                    .await?;
            }
            CandidateSearchNextActionV1::Terminal(CandidateSearchTerminalV1::Compiled {
                ..
            }) => {
                let Some(draft) = current else {
                    unreachable!("a compiled terminal names the proposal just built")
                };
                break draft;
            }
            CandidateSearchNextActionV1::Terminal(terminal) => {
                tracing::info!(
                    target: "cairn.migration.workflow",
                    event = "candidate_search_stopped",
                    task_id = %workflow.task_id(task),
                    "candidate search stopped without a compiling candidate"
                );
                return Ok(CandidateResolutionV1::SearchStopped(terminal));
            }
        }
    };
    workflow
        .admit_candidate(task, intent, oracle, candidate)
        .await
        .map(CandidateResolutionV1::Admitted)
}

async fn complete_cuda_migration<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: W::FrozenTask,
    intent: W::AdmittedIntent,
    oracle: W::AdmittedOracle,
    candidate: W::AdmittedCandidate,
) -> Result<W::TerminalOutcome, W::Error> {
    let task_id = workflow.task_id(&task);
    let terminal = workflow
        .record_terminal_outcome(task, intent, oracle, candidate)
        .await?;
    tracing::info!(
        target: "cairn.migration.workflow",
        event = "cuda_migration_completed",
        task_id = %task_id,
        "CUDA migration workflow completed"
    );
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, future::ready};

    use cairn_protocol::ContentId;

    use super::*;
    use crate::{CandidateSearchLoopV1, CandidateSearchPolicyV1};

    struct FrozenTask(TaskId);

    struct Terminal;

    struct RecordedWorkflow {
        trace: Vec<&'static str>,
        oracle_revised: bool,
        reasoning_decomposition: Option<ReasoningDecompositionPolicyV1>,
        events: cairn_store_sqlite::SqliteEventStore,
        search: Option<CandidateSearchLoopV1>,
        proposals: u32,
        builds: u32,
        compile_on: u32,
        last_proposal: Option<ContentId<crate::CandidateProposalArtifact>>,
    }

    impl Default for RecordedWorkflow {
        fn default() -> Self {
            Self {
                trace: Vec::new(),
                oracle_revised: false,
                reasoning_decomposition: None,
                events: cairn_store_sqlite::SqliteEventStore::in_memory()
                    .expect("in-memory event store"),
                search: None,
                proposals: 0,
                builds: 0,
                // The first build fails and the second compiles, so the readable order below has
                // to show one full compile-diagnostic-revision round rather than a single pass.
                compile_on: 2,
                last_proposal: None,
            }
        }
    }

    fn test_policy() -> CandidateSearchPolicyV1 {
        CandidateSearchPolicyV1 {
            iteration_limit: crate::CandidateIterationLimit::new(4).expect("iteration limit"),
            empty_submission_limit: crate::CandidateEmptySubmissionLimit::new(2)
                .expect("empty submission limit"),
            repeat_window: crate::CandidateRepeatWindow::new(4).expect("repeat window"),
            budget_notice_threshold: crate::CandidateBudgetNoticeThreshold::new(1)
                .expect("budget notice threshold"),
        }
    }

    fn observed_at() -> cairn_protocol::ObservedAtUnixMillis {
        cairn_protocol::ObservedAtUnixMillis::new(1)
    }

    impl RecordedWorkflow {
        fn mark<T>(
            &mut self,
            stage: &'static str,
            value: T,
        ) -> std::future::Ready<Result<T, Infallible>> {
            self.trace.push(stage);
            ready(Ok(value))
        }
    }

    impl CudaMigrationWorkflow for RecordedWorkflow {
        type Error = Infallible;
        type Request = ();
        type FrozenTask = FrozenTask;
        type SirContext = ();
        type SirDraft = ();
        type IntentDecisionRequests = ();
        type AdministratorIntentDecision = ();
        type AdmittedIntent = ();
        type OracleWorkspace = ();
        type OracleDimension = ();
        type OracleWholePortfolioContext = ();
        type OracleItemDiscoveryContext = ();
        type OracleItemSet = Vec<()>;
        type OracleItemSetReviewContext = ();
        type OracleItemSetReview = ();
        type OracleItem = ();
        type OracleItemDevelopmentContext = bool;
        type OracleItemDraft = bool;
        type OracleItemReviewContext = bool;
        type OracleItemReview = bool;
        type AcceptedOracleItem = ();
        type OracleDraft = ();
        type OraclePortfolioReviewContext = ();
        type OraclePortfolioReview = ();
        type ReviewedOracleDraft = ();
        type OracleControlObservations = ();
        type OracleRevisionRequest = ();
        type OracleControlReconciliationRequest = ();
        type AdmittedOracle = ();
        type CandidateExplorationContext = ();
        type CandidateDraft = u32;
        type CandidateBuildAuthority = ();
        type CandidateRevisionContext = ();
        type AdmittedCandidate = ();
        type TerminalOutcome = Terminal;

        fn freeze_task(
            &mut self,
            _request: (),
        ) -> impl Future<Output = Result<FrozenTask, Infallible>> + Send {
            self.mark("freeze-task", FrozenTask(TaskId::new()))
        }

        fn task_id(&self, task: &FrozenTask) -> TaskId {
            task.0
        }

        fn reasoning_decomposition(&self, _task: &FrozenTask) -> ReasoningDecompositionPolicyV1 {
            self.reasoning_decomposition
                .unwrap_or(ReasoningDecompositionPolicyV1::StructuredReview)
        }

        fn prepare_sir_context(
            &mut self,
            _task: &FrozenTask,
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-sir-context", ())
        }

        fn initialize_sir_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-sir-loop", AgentLoopId::new())
        }

        fn run_sir_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _task: &FrozenTask,
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-sir-loop", ())
        }

        fn derive_intent_decision_requests(
            &mut self,
            _task: &FrozenTask,
            _sir: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("derive-intent-decisions", ())
        }

        fn await_administrator_intent_decision(
            &mut self,
            _task: &FrozenTask,
            _sir: &(),
            _requests: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("await-administrator", ())
        }

        fn admit_intent(
            &mut self,
            _task: &FrozenTask,
            _sir: (),
            _requests: (),
            _decision: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("admit-intent", ())
        }

        fn prepare_oracle_workspace(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-oracle-workspace", ())
        }

        fn derive_required_oracle_dimensions(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _workspace: &(),
        ) -> Result<Vec<()>, Infallible> {
            self.trace.push("derive-oracle-dimensions");
            Ok(vec![()])
        }

        fn prepare_oracle_whole_portfolio_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _workspace: &(),
            _dimensions: &[()],
            _lineage: OracleWholePortfolioLineageV1<'_, (), ()>,
        ) -> Result<(), Infallible> {
            self.trace.push("prepare-oracle-whole-portfolio");
            Ok(())
        }

        fn initialize_oracle_whole_portfolio_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-whole-portfolio", AgentLoopId::new())
        }

        fn run_oracle_whole_portfolio_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-whole-portfolio", ())
        }

        fn accept_oracle_whole_portfolio_proposal(&mut self, _draft: ()) -> Result<(), Infallible> {
            self.trace.push("accept-oracle-whole-portfolio");
            Ok(())
        }

        fn prepare_oracle_item_discovery_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _workspace: &(),
            _dimension: &(),
            _lineage: OracleItemDiscoveryLineageV1<'_, Vec<()>, ()>,
        ) -> Result<(), Infallible> {
            self.trace.push("prepare-oracle-item-discovery");
            Ok(())
        }

        fn initialize_oracle_item_discovery_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-item-discovery", AgentLoopId::new())
        }

        fn run_oracle_item_discovery_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<Vec<()>, Infallible>> + Send {
            self.mark("run-oracle-item-discovery", vec![()])
        }

        fn prepare_oracle_item_set_review_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _item_set: &Vec<()>,
        ) -> Result<(), Infallible> {
            self.trace.push("prepare-oracle-item-set-review");
            Ok(())
        }

        fn initialize_oracle_item_set_review_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-item-set-review", AgentLoopId::new())
        }

        fn run_oracle_item_set_review_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<OracleReviewDispositionV1<(), ()>, Infallible>> + Send
        {
            self.mark(
                "run-oracle-item-set-review",
                OracleReviewDispositionV1::Approved(()),
            )
        }

        fn validate_and_expand_oracle_item_set(
            &mut self,
            _dimension: &(),
            item_set: Vec<()>,
        ) -> Result<Vec<()>, Infallible> {
            self.trace.push("validate-oracle-item-set");
            Ok(item_set)
        }

        fn prepare_oracle_item_development_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _workspace: &(),
            _item: &(),
            lineage: OracleItemDevelopmentLineageV1<'_, bool, bool, (), ()>,
        ) -> Result<bool, Infallible> {
            self.trace.push("prepare-oracle-item-development");
            Ok(!matches!(lineage, OracleItemDevelopmentLineageV1::Initial))
        }

        fn initialize_oracle_item_development_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &bool,
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-item-development", AgentLoopId::new())
        }

        fn run_oracle_item_development_loop(
            &mut self,
            _loop_id: AgentLoopId,
            context: bool,
        ) -> impl Future<Output = Result<bool, Infallible>> + Send {
            self.mark("run-oracle-item-development", context)
        }

        fn prepare_oracle_item_review_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            draft: &bool,
        ) -> Result<bool, Infallible> {
            self.trace.push("prepare-oracle-item-review");
            Ok(*draft)
        }

        fn initialize_oracle_item_review_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &bool,
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-item-review", AgentLoopId::new())
        }

        fn run_oracle_item_review_loop(
            &mut self,
            _loop_id: AgentLoopId,
            context: bool,
        ) -> impl Future<Output = Result<OracleReviewDispositionV1<bool, bool>, Infallible>> + Send
        {
            self.trace.push("run-oracle-item-review");
            ready(Ok(if context {
                OracleReviewDispositionV1::Approved(true)
            } else {
                OracleReviewDispositionV1::Revise(true)
            }))
        }

        fn accept_reviewed_oracle_item(
            &mut self,
            _item: (),
            draft: bool,
            review: bool,
        ) -> Result<(), Infallible> {
            assert!(draft && review);
            self.trace.push("accept-oracle-item");
            Ok(())
        }

        fn assemble_oracle_portfolio(
            &mut self,
            _workspace: &(),
            _dimensions: Vec<()>,
            _accepted_items: Vec<()>,
        ) -> Result<(), Infallible> {
            self.trace.push("assemble-oracle-portfolio");
            Ok(())
        }

        fn prepare_oracle_portfolio_review_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
        ) -> Result<(), Infallible> {
            self.trace.push("prepare-oracle-portfolio-review");
            Ok(())
        }

        fn initialize_oracle_portfolio_review_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-portfolio-review", AgentLoopId::new())
        }

        fn run_oracle_portfolio_review_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<OracleReviewDispositionV1<(), ()>, Infallible>> + Send
        {
            self.mark(
                "run-oracle-portfolio-review",
                OracleReviewDispositionV1::Approved(()),
            )
        }

        fn prepare_oracle_items_for_coherence_revision(
            &mut self,
            _draft: &(),
            _review: &(),
        ) -> Result<Vec<((), bool)>, Infallible> {
            self.trace.push("prepare-oracle-coherence-items");
            Ok(vec![((), true)])
        }

        fn accept_oracle_portfolio_review(
            &mut self,
            _draft: (),
            _review: (),
        ) -> Result<(), Infallible> {
            self.trace.push("accept-oracle-portfolio-review");
            Ok(())
        }

        fn replace_oracle_items_after_coherence_revision(
            &mut self,
            _draft: (),
            _revised_items: Vec<()>,
        ) -> Result<(), Infallible> {
            self.trace.push("replace-oracle-coherence-items");
            Ok(())
        }

        fn prepare_oracle_items_for_admission_revision(
            &mut self,
            _task: &FrozenTask,
            _draft: &(),
            _request: &(),
        ) -> impl Future<Output = Result<Vec<((), bool)>, Infallible>> + Send {
            self.mark("prepare-oracle-admission-items", vec![((), true)])
        }

        fn replace_oracle_items_after_admission_revision(
            &mut self,
            _draft: (),
            _revised_items: Vec<()>,
        ) -> Result<(), Infallible> {
            self.trace.push("replace-oracle-admission-items");
            Ok(())
        }

        fn run_qualified_oracle_controls(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-controls", ())
        }

        fn admit_oracle(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: (),
            _observations: (),
        ) -> impl Future<
            Output = Result<OracleAdmissionDispositionV1<(), (), (), (), ()>, Infallible>,
        > + Send {
            self.trace.push("admit-oracle");
            if self.oracle_revised {
                ready(Ok(OracleAdmissionDispositionV1::Admitted(())))
            } else {
                self.oracle_revised = true;
                ready(Ok(OracleAdmissionDispositionV1::Revise {
                    draft: (),
                    request: (),
                    control_observations: (),
                }))
            }
        }

        fn reconcile_oracle_controls(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
            _request: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("reconcile-oracle-controls", ())
        }

        fn open_candidate_search(
            &mut self,
            task: &FrozenTask,
        ) -> impl Future<Output = Result<CandidateSearchStateV1, Infallible>> + Send {
            self.trace.push("open-candidate-search");
            let search = CandidateSearchLoopV1::new(task.0).expect("search loop");
            let state = crate::open_candidate_search(
                &mut self.events,
                &search,
                test_policy(),
                &cairn_protocol::CommandId::new(),
                observed_at(),
            )
            .expect("open candidate search");
            self.search = Some(search);
            ready(Ok(state))
        }

        fn prepare_candidate_exploration_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _iteration: CandidateIterationOrdinal,
            _remaining: CandidateIterationsRemaining,
            _notice: Option<CandidateSearchNoticeV1>,
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-candidate-context", ())
        }

        fn initialize_candidate_exploration_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-candidate-loop", AgentLoopId::new())
        }

        fn run_candidate_exploration_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<Option<u32>, Infallible>> + Send {
            self.proposals = self.proposals.saturating_add(1);
            let ordinal = self.proposals;
            self.mark("run-candidate-loop", Some(ordinal))
        }

        fn prepare_candidate_revision_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _parent: CandidateSearchParentV1,
            _iteration: CandidateIterationOrdinal,
            _remaining: CandidateIterationsRemaining,
            _notice: Option<CandidateSearchNoticeV1>,
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-candidate-revision", ())
        }

        fn initialize_candidate_revision_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-candidate-revision", AgentLoopId::new())
        }

        fn run_candidate_revision_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<Option<u32>, Infallible>> + Send {
            self.proposals = self.proposals.saturating_add(1);
            let ordinal = self.proposals;
            self.mark("run-candidate-revision", Some(ordinal))
        }

        fn record_candidate_proposal(
            &mut self,
            _task: &FrozenTask,
            candidate: &u32,
        ) -> impl Future<Output = Result<CandidateSearchStateV1, Infallible>> + Send {
            self.trace.push("record-candidate-proposal");
            let proposal = ContentId::derive(&candidate.to_be_bytes()).expect("proposal identity");
            self.last_proposal = Some(proposal);
            let state = crate::record_candidate_proposal(
                &mut self.events,
                self.search.as_ref().expect("search loop"),
                proposal,
                &cairn_protocol::CommandId::new(),
                observed_at(),
            )
            .expect("record candidate proposal");
            ready(Ok(state))
        }

        fn record_missing_candidate_submission(
            &mut self,
            _task: &FrozenTask,
        ) -> impl Future<Output = Result<CandidateSearchStateV1, Infallible>> + Send {
            self.trace.push("record-missing-candidate-submission");
            let state = crate::record_missing_submission(
                &mut self.events,
                self.search.as_ref().expect("search loop"),
                &cairn_protocol::CommandId::new(),
                observed_at(),
            )
            .expect("record missing submission");
            ready(Ok(state))
        }

        fn authorize_candidate_build(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: &u32,
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("authorize-candidate-build", ())
        }

        fn observe_candidate_build(
            &mut self,
            _authority: (),
        ) -> impl Future<Output = Result<CandidateBuildOutcomeV1, Infallible>> + Send {
            self.builds = self.builds.saturating_add(1);
            let compiled = self.builds >= self.compile_on;
            let outcome = CandidateBuildOutcomeV1::new(
                self.last_proposal.expect("built proposal"),
                ContentId::derive(&self.builds.to_be_bytes()).expect("receipt identity"),
                compiled,
            );
            self.mark("observe-candidate-build", outcome)
        }

        fn record_candidate_build_observation(
            &mut self,
            _task: &FrozenTask,
            outcome: CandidateBuildOutcomeV1,
        ) -> impl Future<Output = Result<CandidateSearchStateV1, Infallible>> + Send {
            self.trace.push("record-candidate-build");
            let state = crate::record_candidate_build_observation(
                &mut self.events,
                self.search.as_ref().expect("search loop"),
                outcome.proposal(),
                outcome.receipt(),
                outcome.compiled(),
                &cairn_protocol::CommandId::new(),
                observed_at(),
            )
            .expect("record build observation");
            ready(Ok(state))
        }

        fn admit_candidate(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: u32,
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("admit-candidate", ())
        }

        fn record_candidate_search_stop(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _terminal: CandidateSearchTerminalV1,
        ) -> impl Future<Output = Result<Terminal, Infallible>> + Send {
            self.mark("record-candidate-search-stop", Terminal)
        }

        fn record_terminal_outcome(
            &mut self,
            _task: FrozenTask,
            _intent: (),
            _oracle: (),
            _candidate: (),
        ) -> impl Future<Output = Result<Terminal, Infallible>> + Send {
            self.mark("record-terminal", Terminal)
        }
    }

    #[tokio::test]
    async fn readable_workflow_preserves_role_loops_mechanical_gates_and_revision_lineage() {
        let mut workflow = RecordedWorkflow::default();
        run_cuda_migration(&mut workflow, ())
            .await
            .expect("workflow");

        assert_eq!(
            workflow.trace,
            [
                "freeze-task",
                "prepare-sir-context",
                "initialize-sir-loop",
                "run-sir-loop",
                "derive-intent-decisions",
                "await-administrator",
                "admit-intent",
                "prepare-oracle-workspace",
                "derive-oracle-dimensions",
                "prepare-oracle-item-discovery",
                "initialize-oracle-item-discovery",
                "run-oracle-item-discovery",
                "prepare-oracle-item-set-review",
                "initialize-oracle-item-set-review",
                "run-oracle-item-set-review",
                "validate-oracle-item-set",
                "prepare-oracle-item-development",
                "initialize-oracle-item-development",
                "run-oracle-item-development",
                "prepare-oracle-item-review",
                "initialize-oracle-item-review",
                "run-oracle-item-review",
                "prepare-oracle-item-development",
                "initialize-oracle-item-development",
                "run-oracle-item-development",
                "prepare-oracle-item-review",
                "initialize-oracle-item-review",
                "run-oracle-item-review",
                "accept-oracle-item",
                "assemble-oracle-portfolio",
                "prepare-oracle-portfolio-review",
                "initialize-oracle-portfolio-review",
                "run-oracle-portfolio-review",
                "accept-oracle-portfolio-review",
                "run-oracle-controls",
                "admit-oracle",
                "prepare-oracle-admission-items",
                "prepare-oracle-item-development",
                "initialize-oracle-item-development",
                "run-oracle-item-development",
                "prepare-oracle-item-review",
                "initialize-oracle-item-review",
                "run-oracle-item-review",
                "accept-oracle-item",
                "replace-oracle-admission-items",
                "prepare-oracle-portfolio-review",
                "initialize-oracle-portfolio-review",
                "run-oracle-portfolio-review",
                "accept-oracle-portfolio-review",
                "run-oracle-controls",
                "admit-oracle",
                // The search loop is the compile-diagnostic-revision cycle, and nothing else.
                "open-candidate-search",
                "prepare-candidate-context",
                "initialize-candidate-loop",
                "run-candidate-loop",
                "record-candidate-proposal",
                "authorize-candidate-build",
                "observe-candidate-build",
                "record-candidate-build",
                "prepare-candidate-revision",
                "initialize-candidate-revision",
                "run-candidate-revision",
                "record-candidate-proposal",
                "authorize-candidate-build",
                "observe-candidate-build",
                "record-candidate-build",
                "admit-candidate",
                "record-terminal",
            ]
        );
    }

    #[tokio::test]
    async fn minimal_decomposition_uses_one_whole_portfolio_loop_and_no_review_loop() {
        let mut workflow = RecordedWorkflow {
            reasoning_decomposition: Some(ReasoningDecompositionPolicyV1::MinimalDecomposition),
            oracle_revised: true,
            ..RecordedWorkflow::default()
        };
        run_cuda_migration(&mut workflow, ())
            .await
            .expect("minimal workflow");

        assert!(workflow.trace.contains(&"prepare-oracle-whole-portfolio"));
        assert!(workflow.trace.contains(&"run-oracle-whole-portfolio"));
        assert!(workflow.trace.contains(&"accept-oracle-whole-portfolio"));
        assert!(!workflow.trace.contains(&"prepare-oracle-item-discovery"));
        assert!(!workflow.trace.contains(&"prepare-oracle-item-review"));
        assert!(!workflow.trace.contains(&"prepare-oracle-portfolio-review"));
        assert_eq!(workflow.trace.last(), Some(&"record-terminal"));
    }
}
