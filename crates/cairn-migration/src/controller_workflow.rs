use std::future::Future;

use cairn_protocol::{AgentLoopId, TaskId};

/// Product-owned ports beneath the readable CUDA migration workflow.
///
/// Cognitive activities are split into role-scoped Agent Loops. Admission and authority-granting
/// activities remain independent mechanical calls.
pub trait CudaMigrationWorkflow: Send {
    type Error: Send;
    type Request: Send;
    type FrozenTask: Send + Sync;

    type SirContext: Send + Sync;
    type SirDraft: Send + Sync;
    type IntentDecisionRequests: Send + Sync;
    type AdministratorIntentDecision: Send + Sync;
    type AdmittedIntent: Send + Sync;

    type OracleExplorationContext: Send + Sync;
    type OracleDraft: Send + Sync;
    type OracleReviewContext: Send + Sync;
    type OracleReview: Send + Sync;
    type OracleControlObservations: Send + Sync;
    type OracleRevisionRequest: Send + Sync;
    type OracleRevisionContext: Send + Sync;
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

    fn prepare_oracle_exploration_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
    ) -> impl Future<Output = Result<Self::OracleExplorationContext, Self::Error>> + Send;

    fn initialize_oracle_exploration_loop(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        context: &Self::OracleExplorationContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_exploration_loop(
        &mut self,
        loop_id: AgentLoopId,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        context: Self::OracleExplorationContext,
    ) -> impl Future<Output = Result<Self::OracleDraft, Self::Error>> + Send;

    fn prepare_oracle_review_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
    ) -> impl Future<Output = Result<Self::OracleReviewContext, Self::Error>> + Send;

    fn initialize_oracle_review_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleReviewContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_review_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleReviewContext,
    ) -> impl Future<Output = Result<Self::OracleReview, Self::Error>> + Send;

    fn run_qualified_oracle_controls(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
        review: &Self::OracleReview,
    ) -> impl Future<Output = Result<Self::OracleControlObservations, Self::Error>> + Send;

    #[expect(
        clippy::type_complexity,
        reason = "the explicit admitted, rejected, revision, and observation types are authority boundaries"
    )]
    fn admit_oracle(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: Self::OracleDraft,
        review: Self::OracleReview,
        observations: Self::OracleControlObservations,
    ) -> impl Future<
        Output = Result<
            OracleAdmissionDispositionV1<
                Self::AdmittedOracle,
                Self::OracleDraft,
                Self::OracleRevisionRequest,
                Self::OracleControlObservations,
            >,
            Self::Error,
        >,
    > + Send;

    fn prepare_oracle_revision_context(
        &mut self,
        task: &Self::FrozenTask,
        intent: &Self::AdmittedIntent,
        draft: &Self::OracleDraft,
        request: &Self::OracleRevisionRequest,
        observations: &Self::OracleControlObservations,
    ) -> impl Future<Output = Result<Self::OracleRevisionContext, Self::Error>> + Send;

    fn initialize_oracle_revision_loop(
        &mut self,
        task: &Self::FrozenTask,
        context: &Self::OracleRevisionContext,
    ) -> impl Future<Output = Result<AgentLoopId, Self::Error>> + Send;

    fn run_oracle_revision_loop(
        &mut self,
        loop_id: AgentLoopId,
        context: Self::OracleRevisionContext,
    ) -> impl Future<Output = Result<Self::OracleDraft, Self::Error>> + Send;

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
pub enum OracleAdmissionDispositionV1<A, D, R, O> {
    Admitted(A),
    Revise {
        draft: D,
        request: R,
        control_observations: O,
    },
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

/// Explores, independently reviews, controls, and mechanically admits an Oracle portfolio.
async fn establish_oracle<W: CudaMigrationWorkflow>(
    workflow: &mut W,
    task: &W::FrozenTask,
    intent: &W::AdmittedIntent,
) -> Result<W::AdmittedOracle, W::Error> {
    let context = workflow
        .prepare_oracle_exploration_context(task, intent)
        .await?;
    let loop_id = workflow
        .initialize_oracle_exploration_loop(task, intent, &context)
        .await?;
    let mut draft = workflow
        .run_oracle_exploration_loop(loop_id, task, intent, context)
        .await?;
    loop {
        let review_context = workflow
            .prepare_oracle_review_context(task, intent, &draft)
            .await?;
        let review_loop = workflow
            .initialize_oracle_review_loop(task, &review_context)
            .await?;
        let review = workflow
            .run_oracle_review_loop(review_loop, review_context)
            .await?;
        let observations = workflow
            .run_qualified_oracle_controls(task, intent, &draft, &review)
            .await?;
        match workflow
            .admit_oracle(task, intent, draft, review, observations)
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
            OracleAdmissionDispositionV1::Revise {
                draft: rejected,
                request,
                control_observations,
            } => {
                let revision_context = workflow
                    .prepare_oracle_revision_context(
                        task,
                        intent,
                        &rejected,
                        &request,
                        &control_observations,
                    )
                    .await?;
                let revision_loop = workflow
                    .initialize_oracle_revision_loop(task, &revision_context)
                    .await?;
                draft = workflow
                    .run_oracle_revision_loop(revision_loop, revision_context)
                    .await?;
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
        type OracleExplorationContext = ();
        type OracleDraft = ();
        type OracleReviewContext = ();
        type OracleReview = ();
        type OracleControlObservations = ();
        type OracleRevisionRequest = ();
        type OracleRevisionContext = ();
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

        fn prepare_oracle_exploration_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-oracle-context", ())
        }

        fn initialize_oracle_exploration_loop(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-loop", AgentLoopId::new())
        }

        fn run_oracle_exploration_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _task: &FrozenTask,
            _intent: &(),
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-loop", ())
        }

        fn prepare_oracle_review_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-oracle-review", ())
        }

        fn initialize_oracle_review_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-review", AgentLoopId::new())
        }

        fn run_oracle_review_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-review", ())
        }

        fn run_qualified_oracle_controls(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
            _review: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-controls", ())
        }

        fn admit_oracle(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: (),
            _review: (),
            _observations: (),
        ) -> impl Future<Output = Result<OracleAdmissionDispositionV1<(), (), (), ()>, Infallible>> + Send
        {
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

        fn prepare_oracle_revision_context(
            &mut self,
            _task: &FrozenTask,
            _intent: &(),
            _draft: &(),
            _request: &(),
            _observations: &(),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("prepare-oracle-revision", ())
        }

        fn initialize_oracle_revision_loop(
            &mut self,
            _task: &FrozenTask,
            _context: &(),
        ) -> impl Future<Output = Result<AgentLoopId, Infallible>> + Send {
            self.mark("initialize-oracle-revision", AgentLoopId::new())
        }

        fn run_oracle_revision_loop(
            &mut self,
            _loop_id: AgentLoopId,
            _context: (),
        ) -> impl Future<Output = Result<(), Infallible>> + Send {
            self.mark("run-oracle-revision", ())
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
                "prepare-oracle-context",
                "initialize-oracle-loop",
                "run-oracle-loop",
                "prepare-oracle-review",
                "initialize-oracle-review",
                "run-oracle-review",
                "run-oracle-controls",
                "admit-oracle",
                "prepare-oracle-revision",
                "initialize-oracle-revision",
                "run-oracle-revision",
                "prepare-oracle-review",
                "initialize-oracle-review",
                "run-oracle-review",
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
}
