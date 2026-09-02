use std::future::Future;

use cairn_protocol::{AgentLoopId, TaskId};

use crate::ReasoningDecompositionPolicyV1;

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
    type CandidateReviewContext: Send + Sync;
    type CandidateReview: Send + Sync;
    type CandidateBuildAuthority: Send + Sync;
    type CandidateWorkerObservations: Send + Sync;
    type CandidateObservationLineage: Send + Sync;
    type CandidateRevisionRequest: Send + Sync;
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

    fn prepare_candidate_exploration_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
    ) -> impl Future<Output = Result<Self::CandidateExplorationContext, Self::Error>> + Send;

    fn initialize_candidate_exploration_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateExplorationContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_candidate_exploration_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateExplorationContext,
    ) -> impl Future<Output = Result<Self::CandidateDraft, Self::Error>> + Send;

    fn prepare_candidate_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
    ) -> impl Future<Output = Result<Self::CandidateReviewContext, Self::Error>> + Send;

    fn initialize_candidate_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::CandidateReviewContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_candidate_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::CandidateReviewContext,
    ) -> impl Future<Output = Result<Self::CandidateReview, Self::Error>> + Send;

    fn authorize_candidate_build(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
        review: &Self::CandidateReview,
    ) -> impl Future<Output = Result<Self::CandidateBuildAuthority, Self::Error>> + Send;

    fn observe_candidate_on_worker(
        &mut self,
        authority: Self::CandidateBuildAuthority,
    ) -> impl Future<Output = Result<Self::CandidateWorkerObservations, Self::Error>> + Send;

    #[expect(
        clippy::type_complexity,
        reason = "the explicit candidate, admission, revision, and lineage types prevent authority erasure"
    )]
    fn admit_candidate(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: Self::CandidateDraft,
        review: Self::CandidateReview,
        observations: Self::CandidateWorkerObservations,
    ) -> impl Future<
        Output = Result<
            CandidateAdmissionDispositionV1<
                Self::AdmittedCandidate,
                Self::CandidateDraft,
                Self::CandidateObservationLineage,
                Self::CandidateRevisionRequest,
            >,
            Self::Error,
        >,
    > + Send;

    fn prepare_candidate_revision_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        oracle: &Self::AdmittedOracle,
        candidate: &Self::CandidateDraft,
        observation_lineage: &Self::CandidateObservationLineage,
        request: &Self::CandidateRevisionRequest,
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
    ) -> impl Future<Output = Result<Self::CandidateDraft, Self::Error>> + Send;

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

/// Mechanical Candidate admission either grants authority or binds revision to exact observation
/// lineage. A free-form diagnostic cannot replace this lineage value.
pub enum CandidateAdmissionDispositionV1<A, C, L, R> {
    Admitted(A),
    Revise {
        candidate: C,
        observation_lineage: L,
        request: R,
    },
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
    let candidate = establish_candidate(workflow, &task, &intent, &oracle).await?;
    complete_cuda_migration(workflow, task, intent, oracle, candidate).await
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
async fn establish_candidate<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
    intent: &W::AdmittedIntent,
    oracle: &W::AdmittedOracle,
) -> Result<W::AdmittedCandidate, W::Error> {
    let context = workflow
        .prepare_candidate_exploration_context(task, intent, oracle)
        .await?;
    let loop_id = workflow
        .initialize_candidate_exploration_loop(task, &context)
        .await?;
    let mut candidate = workflow
        .run_candidate_exploration_loop(loop_id, context)
        .await?;
    loop {
        let review_context = workflow
            .prepare_candidate_review_context(task, intent, oracle, &candidate)
            .await?;
        let review_loop = workflow
            .initialize_candidate_review_loop(task, &review_context)
            .await?;
        let review = workflow
            .run_candidate_review_loop(review_loop, review_context)
            .await?;
        let authority = workflow
            .authorize_candidate_build(task, intent, oracle, &candidate, &review)
            .await?;
        let observations = workflow.observe_candidate_on_worker(authority).await?;
        match workflow
            .admit_candidate(task, intent, oracle, candidate, review, observations)
            .await?
        {
            CandidateAdmissionDispositionV1::Admitted(admitted) => return Ok(admitted),
            CandidateAdmissionDispositionV1::Revise {
                candidate: rejected,
                observation_lineage,
                request,
            } => {
                let revision_context = workflow
                    .prepare_candidate_revision_context(
                        task,
                        intent,
                        oracle,
                        &rejected,
                        &observation_lineage,
                        &request,
                    )
                    .await?;
                let revision_loop = workflow
                    .initialize_candidate_revision_loop(task, &revision_context)
                    .await?;
                candidate = workflow
                    .run_candidate_revision_loop(revision_loop, revision_context)
                    .await?;
            }
        }
    }
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

    use super::*;

    struct FrozenTask(TaskId);

    struct CandidateLineage;
    struct Terminal;

    #[derive(Default)]
    struct RecordedWorkflow {
        trace: Vec<&'static str>,
        oracle_revised: bool,
        candidate_revised: bool,
        reasoning_decomposition: Option<ReasoningDecompositionPolicyV1>,
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
        type CandidateDraft = ();
        type CandidateReviewContext = ();
        type CandidateReview = ();
        type CandidateBuildAuthority = ();
        type CandidateWorkerObservations = ();
        type CandidateObservationLineage = CandidateLineage;
        type CandidateRevisionRequest = ();
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

        fn prepare_candidate_exploration_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
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
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-candidate-loop", ())
        }

        fn prepare_candidate_review_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-candidate-review", ())
        }

        fn initialize_candidate_review_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-candidate-review", AgentLoopId::new())
        }

        fn run_candidate_review_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-candidate-review", ())
        }

        fn authorize_candidate_build(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: &(),
            _review: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("authorize-candidate-build", ())
        }

        fn observe_candidate_on_worker(
            &mut self,
            _authority: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("observe-candidate", ())
        }

        fn admit_candidate(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: (),
            _review: (),
            _observations: (),
        ) -> impl Future<
            Output = Result<
                CandidateAdmissionDispositionV1<(), (), CandidateLineage, ()>,
                Infallible,
            >,
        > + Send {
            self.trace.push("admit-candidate");
            if self.candidate_revised {
                ready(Ok(CandidateAdmissionDispositionV1::Admitted(())))
            } else {
                self.candidate_revised = true;
                ready(Ok(CandidateAdmissionDispositionV1::Revise {
                    candidate: (),
                    observation_lineage: CandidateLineage,
                    request: (),
                }))
            }
        }

        fn prepare_candidate_revision_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _oracle: &(),
            _candidate: &(),
            _observation_lineage: &CandidateLineage,
            _request: &(),
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
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-candidate-revision", ())
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
                "prepare-candidate-context",
                "initialize-candidate-loop",
                "run-candidate-loop",
                "prepare-candidate-review",
                "initialize-candidate-review",
                "run-candidate-review",
                "authorize-candidate-build",
                "observe-candidate",
                "admit-candidate",
                "prepare-candidate-revision",
                "initialize-candidate-revision",
                "run-candidate-revision",
                "prepare-candidate-review",
                "initialize-candidate-review",
                "run-candidate-review",
                "authorize-candidate-build",
                "observe-candidate",
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
