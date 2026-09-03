use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use cairn_agent::AgentLoopCheckpointV1;
use cairn_migration::{
    CandidateAdmissionAttemptV1, CandidateAdmissionEvidenceV1, CandidateOracleContractV1,
    CandidateProposalV1, IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1,
    IntentRecoveryInputV1, OracleAdmissionAttemptV1, OracleAdmissionEvidenceV1,
    OracleCoveragePolicyV1, OracleDimensionV1, OraclePortfolioProposalV1, OracleStrategyCatalogV1,
    OracleWorkspaceV1, PreparedIntentAdmissionV1, ReasoningDecompositionPolicyV1,
    SirCapabilityManifestV1, SirTaskLimits, SirTaskWorkspace, TaskIntentAuthoritySubject,
    UserIntentAuthorityGrantV1, UserIntentDecisionResponseV1, UserIntentDecisionV1,
    derive_oracle_claims, derive_oracle_dimensions,
};
use cairn_protocol::{ContentId, EventId, EventSequence, ObservedAtUnixMillis, TaskId};
use cairn_sdk::{
    AppApiErrorCodeV1, CairnRequestV1, CairnResponseV1, IntentReviewRequestResourceV1,
    IntentReviewResourceV1, TaskAttentionV1, TaskPhaseV1, TaskProgressItemV1, TaskProgressPageV1,
    TaskResourceV1, TaskSubmissionV1, read_frame, write_frame,
};
use cairn_server::{ApplicationModule, ApplicationName};
use thiserror::Error;
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Notify, mpsc},
};

use crate::{
    AdmittedIntentV1, AdmittedOracleV1, AuthorizedCandidateBuildV1, AuthorizedIntentDecisionV1,
    CandidateBuildRunnerV1, CudaMigrationApplication, CudaMigrationProductServices,
    FrozenMigrationTaskV1, MigrationAgentRuntimeExecutorV1, MigrationProductServiceError,
    MigrationRuntimeMaterialsV1, MigrationTaskRequest, MigrationTerminalOutcomeV1,
    MigrationWorkflowFailureClassV1, OracleControlRunnerError, OracleControlRunnerV1,
};

/// Exact request admitted by the App API and sent to the product workflow inbox.
pub struct SubmittedMigrationTaskV1 {
    task_id: TaskId,
    submission: TaskSubmissionV1,
}

impl SubmittedMigrationTaskV1 {
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
}

impl MigrationTaskRequest for SubmittedMigrationTaskV1 {
    fn task_id(&self) -> TaskId {
        self.task_id
    }
}

struct TaskStateV1 {
    task_id: TaskId,
    phase: TaskPhaseV1,
    attention: Option<TaskAttentionV1>,
    progress: Vec<TaskProgressItemV1>,
    intent_review: Option<IntentReviewResourceV1>,
    decisions: Vec<AuthorizedIntentDecisionV1>,
    cancelled: bool,
    notify: Arc<Notify>,
}

impl TaskStateV1 {
    fn new(task_id: TaskId) -> Result<Self, MigrationAppApiError> {
        let mut value = Self {
            task_id,
            phase: TaskPhaseV1::Submitted,
            attention: None,
            progress: Vec::new(),
            intent_review: None,
            decisions: Vec::new(),
            cancelled: false,
            notify: Arc::new(Notify::new()),
        };
        value.transition(TaskPhaseV1::Submitted, None)?;
        Ok(value)
    }

    fn transition(
        &mut self,
        phase: TaskPhaseV1,
        attention: Option<TaskAttentionV1>,
    ) -> Result<(), MigrationAppApiError> {
        let ordinal = u64::try_from(self.progress.len())
            .map_err(MigrationAppApiError::internal)?
            .checked_add(1)
            .ok_or_else(|| MigrationAppApiError::Internal("task progress overflow".to_owned()))?;
        self.phase = phase;
        self.attention = attention;
        let observed_at = observed_now()?;
        let sequence = EventSequence::new(ordinal).map_err(MigrationAppApiError::internal)?;
        let event_id = EventId::derive(
            &cairn_codec::to_vec(&(self.task_id, sequence, observed_at, phase))
                .map_err(MigrationAppApiError::internal)?,
        )
        .map_err(MigrationAppApiError::internal)?;
        self.progress.push(TaskProgressItemV1 {
            sequence,
            event_id,
            observed_at,
            phase,
        });
        Ok(())
    }

    fn transition_if_active(
        &mut self,
        phase: TaskPhaseV1,
        attention: Option<TaskAttentionV1>,
    ) -> Result<(), MigrationAppApiError> {
        if self.cancelled {
            return Err(MigrationAppApiError::Cancelled);
        }
        self.transition(phase, attention)
    }

    fn commit_workflow_failure(
        &mut self,
        failure: MigrationWorkflowFailureClassV1,
    ) -> Result<Option<TaskAttentionV1>, MigrationAppApiError> {
        if self.cancelled {
            return Ok(None);
        }
        let attention = workflow_failure_attention(failure);
        self.transition(TaskPhaseV1::Blocked, Some(attention))?;
        Ok(Some(attention))
    }

    fn resource(&self, task_id: TaskId) -> TaskResourceV1 {
        TaskResourceV1::new(
            task_id,
            self.progress
                .last()
                .expect("a task always has its submitted progress fact")
                .sequence,
            self.phase,
            self.attention,
        )
    }
}

fn pause_for_intent_reconciliation(
    state: &mut TaskStateV1,
    unresolved_decision_count: usize,
) -> Result<bool, MigrationAppApiError> {
    if unresolved_decision_count == 0
        || (state.phase == TaskPhaseV1::Blocked
            && state.attention == Some(TaskAttentionV1::IntentAdmissionReconciliation))
    {
        return Ok(false);
    }
    state.transition(
        TaskPhaseV1::Blocked,
        Some(TaskAttentionV1::IntentAdmissionReconciliation),
    )?;
    Ok(true)
}

enum IntentDecisionReadinessV1 {
    Pending,
    Reconciliation {
        newly_paused: bool,
        unresolved_decision_count: usize,
    },
    Ready(Vec<AuthorizedIntentDecisionV1>),
}

fn take_resolved_intent_decisions(
    state: &mut TaskStateV1,
    request_ids: &[ContentId<cairn_migration::UserIntentDecisionRequestArtifact>],
) -> Result<IntentDecisionReadinessV1, MigrationAppApiError> {
    if !request_ids.iter().all(|request_id| {
        state.decisions.iter().any(|decision| {
            decision
                .request
                .identity()
                .is_ok_and(|identity| identity == *request_id)
        })
    }) {
        return Ok(IntentDecisionReadinessV1::Pending);
    }
    let unresolved_decision_count = state
        .decisions
        .iter()
        .filter(|decision| {
            matches!(
                decision.decision.response(),
                UserIntentDecisionResponseV1::KeepUnknown
            )
        })
        .count();
    if unresolved_decision_count != 0 {
        return Ok(IntentDecisionReadinessV1::Reconciliation {
            newly_paused: pause_for_intent_reconciliation(state, unresolved_decision_count)?,
            unresolved_decision_count,
        });
    }
    let decisions = request_ids
        .iter()
        .map(|request_id| {
            let position = state
                .decisions
                .iter()
                .position(|decision| {
                    decision
                        .request
                        .identity()
                        .is_ok_and(|identity| identity == *request_id)
                })
                .ok_or(MigrationAppApiError::Conflict)?;
            Ok::<_, MigrationAppApiError>(state.decisions.remove(position))
        })
        .collect::<Result<Vec<_>, _>>()?;
    state.transition(TaskPhaseV1::AdmittingIntent, None)?;
    Ok(IntentDecisionReadinessV1::Ready(decisions))
}

#[derive(Clone, Default)]
struct SharedTasksV1(Arc<Mutex<BTreeMap<TaskId, TaskStateV1>>>);

impl SharedTasksV1 {
    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<TaskId, TaskStateV1>>, MigrationAppApiError>
    {
        self.0
            .lock()
            .map_err(|_| MigrationAppApiError::StatePoisoned)
    }
}

/// Product-owned local App API. It translates SDK requests into the same workflow inbox used by
/// normal application execution.
pub struct MigrationAppApiV1 {
    unix_socket: PathBuf,
    sender: mpsc::Sender<SubmittedMigrationTaskV1>,
    tasks: SharedTasksV1,
    authority_subject: TaskIntentAuthoritySubject,
}

impl MigrationAppApiV1 {
    /// Binds the current-V1 Unix App API and serves one canonical request per connection.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe socket target, listener failure, or accept failure.
    pub async fn run(self) -> Result<(), MigrationAppApiError> {
        prepare_socket(&self.unix_socket)?;
        let listener = UnixListener::bind(&self.unix_socket).map_err(MigrationAppApiError::io)?;
        tracing::info!(
            target: "cairn.migration.app-api",
            event = "migration_app_api_listener_ready",
            "CUDA migration App API listener ready"
        );
        loop {
            let (stream, _) = listener.accept().await.map_err(MigrationAppApiError::io)?;
            let sender = self.sender.clone();
            let tasks = self.tasks.clone();
            let authority_subject = self.authority_subject.clone();
            tokio::spawn(async move {
                if let Err(error) =
                    handle_connection(stream, sender, tasks, authority_subject).await
                {
                    tracing::warn!(
                        target: "cairn.migration.app-api",
                        event = "migration_app_api_connection_failed",
                        error_class = error.log_class(),
                        "CUDA migration App API connection failed"
                    );
                }
            });
        }
    }
}

/// Concrete product effects shared by the App API and readable CUDA migration workflow.
pub struct MigrationProductServicesV1 {
    tasks: SharedTasksV1,
    materials: MigrationRuntimeMaterialsV1,
    task_limits: SirTaskLimits,
    archive_limits: cairn_migration::TaskArchiveLimits,
    oracle_policy: OracleCoveragePolicyV1,
    oracle_catalog: OracleStrategyCatalogV1,
    oracle_controls: OracleControlRunnerV1,
    candidate_build: Option<CandidateBuildRunnerV1>,
    reasoning_decomposition: ReasoningDecompositionPolicyV1,
}

/// CUDA migration App API and workflow composed as one product module above the generic host.
pub struct CudaMigrationProductModuleV1 {
    name: ApplicationName,
    api: MigrationAppApiV1,
    workflow: CudaMigrationApplication<MigrationProductServicesV1, MigrationAgentRuntimeExecutorV1>,
}

impl CudaMigrationProductModuleV1 {
    #[must_use]
    pub fn new(
        name: ApplicationName,
        api: MigrationAppApiV1,
        workflow: CudaMigrationApplication<
            MigrationProductServicesV1,
            MigrationAgentRuntimeExecutorV1,
        >,
    ) -> Self {
        Self {
            name,
            api,
            workflow,
        }
    }
}

impl ApplicationModule for CudaMigrationProductModuleV1 {
    type Error = MigrationAppApiError;

    fn name(&self) -> &ApplicationName {
        &self.name
    }

    async fn run(self) -> Result<(), Self::Error> {
        let Self {
            name: _,
            api,
            workflow,
        } = self;
        let workflow_task = async move {
            Box::pin(ApplicationModule::run(workflow))
                .await
                .map_err(MigrationAppApiError::internal)
        };
        run_product_tasks(Box::pin(api.run()), Box::pin(workflow_task)).await
    }
}

type ProductTask = Pin<Box<dyn Future<Output = Result<(), MigrationAppApiError>> + Send + 'static>>;

async fn run_product_tasks(
    api: ProductTask,
    workflow: ProductTask,
) -> Result<(), MigrationAppApiError> {
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(api);
    tasks.spawn(workflow);
    let result = tasks
        .join_next()
        .await
        .ok_or_else(|| MigrationAppApiError::internal("product task set is empty"))?;
    tasks.abort_all();
    result.map_err(MigrationAppApiError::internal)?
}

/// Creates the App API, workflow product services, and their single normal submission channel.
///
/// # Errors
///
/// Rejects an invalid authority subject or zero inbox capacity.
#[allow(
    clippy::too_many_arguments,
    reason = "the product boundary keeps every independently owned runtime dependency explicit"
)]
pub fn migration_product_boundary(
    unix_socket: PathBuf,
    authority_subject: &TaskIntentAuthoritySubject,
    task_limits: SirTaskLimits,
    archive_limits: cairn_migration::TaskArchiveLimits,
    materials: MigrationRuntimeMaterialsV1,
    oracle_policy: OracleCoveragePolicyV1,
    oracle_catalog: OracleStrategyCatalogV1,
    oracle_controls: OracleControlRunnerV1,
    candidate_build: Option<CandidateBuildRunnerV1>,
    reasoning_decomposition: ReasoningDecompositionPolicyV1,
    inbox_capacity: usize,
) -> Result<
    (
        MigrationAppApiV1,
        MigrationProductServicesV1,
        mpsc::Receiver<SubmittedMigrationTaskV1>,
    ),
    MigrationAppApiError,
> {
    if !unix_socket.is_absolute() || inbox_capacity == 0 {
        return Err(MigrationAppApiError::InvalidConfiguration);
    }
    let tasks = SharedTasksV1::default();
    let (sender, receiver) = mpsc::channel(inbox_capacity);
    Ok((
        MigrationAppApiV1 {
            unix_socket,
            sender,
            tasks: tasks.clone(),
            authority_subject: authority_subject.clone(),
        },
        MigrationProductServicesV1 {
            tasks,
            materials,
            task_limits,
            archive_limits,
            oracle_policy,
            oracle_catalog,
            oracle_controls,
            candidate_build,
            reasoning_decomposition,
        },
        receiver,
    ))
}

impl CudaMigrationProductServices for MigrationProductServicesV1 {
    type Request = SubmittedMigrationTaskV1;
    type CandidateBuildAuthority = AuthorizedCandidateBuildV1;
    type Error = MigrationAppApiError;

    async fn freeze_task(
        &mut self,
        request: Self::Request,
    ) -> Result<FrozenMigrationTaskV1, Self::Error> {
        // The archive is transport. What the system keeps is the per-path source bundle, and a
        // submission that cannot convert entirely is refused rather than trimmed, so the bundle
        // always describes exactly what was uploaded.
        let sources = cairn_migration::extract_task_sources(
            request.submission.archive(),
            self.archive_limits,
        )
        .map_err(MigrationAppApiError::internal)?;
        let workspace = SirTaskWorkspace::from_sources(sources, self.task_limits)
            .map_err(MigrationAppApiError::internal)?;
        let recovery_input = IntentRecoveryInputV1::new(
            request.task_id,
            workspace
                .bundle()
                .identity()
                .map_err(MigrationAppApiError::internal)?,
            request.submission.recovery_request().clone(),
            SirCapabilityManifestV1::proposal_only(self.task_limits),
        )
        .map_err(MigrationAppApiError::internal)?;
        self.materials.register_task(
            request.task_id,
            workspace.clone(),
            recovery_input.clone(),
            self.task_limits,
            self.reasoning_decomposition,
        )?;
        self.transition(request.task_id, TaskPhaseV1::RecoveringIntent, None)?;
        tracing::info!(
            target: "cairn.migration.ablation",
            event = "reasoning_decomposition_policy_frozen",
            task_id = %request.task_id,
            reasoning_decomposition = %self.reasoning_decomposition,
            "migration reasoning decomposition frozen"
        );
        FrozenMigrationTaskV1::new(
            request.task_id,
            workspace,
            recovery_input,
            self.reasoning_decomposition,
        )
        .map_err(MigrationAppApiError::internal)
    }

    async fn await_administrator_intent_decision(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: &IntentHypothesisSetProposalV1,
        requests: &IntentDecisionRequestBatchV1,
    ) -> Result<crate::AuthorizedIntentDecisionSetV1, Self::Error> {
        let notify = {
            let mut tasks = self.tasks.lock()?;
            let state = tasks
                .get_mut(&task.task_id())
                .ok_or(MigrationAppApiError::TaskNotFound)?;
            state.intent_review = Some(IntentReviewResourceV1 {
                task_id: task.task_id(),
                proposal_id: proposal
                    .identity()
                    .map_err(MigrationAppApiError::internal)?,
                proposal: proposal.clone(),
                requests_id: Some(
                    requests
                        .identity()
                        .map_err(MigrationAppApiError::internal)?,
                ),
                requests: requests
                    .requests()
                    .iter()
                    .map(|request| {
                        Ok(IntentReviewRequestResourceV1 {
                            request_id: request
                                .identity()
                                .map_err(MigrationAppApiError::internal)?,
                            request: request.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, MigrationAppApiError>>()?,
            });
            state.transition(
                TaskPhaseV1::AwaitingIntentReview,
                Some(TaskAttentionV1::IntentReview),
            )?;
            Arc::clone(&state.notify)
        };
        let proposal_id = proposal
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        tracing::info!(
            target: "cairn.migration.app-api",
            event = "intent_review_published",
            task_id = %task.task_id(),
            proposal_id = %proposal_id,
            request_count = requests.requests().len(),
            "SIR proposal published for task-authority review"
        );
        loop {
            let notified = notify.notified();
            {
                let mut tasks = self.tasks.lock()?;
                let state = tasks
                    .get_mut(&task.task_id())
                    .ok_or(MigrationAppApiError::TaskNotFound)?;
                if state.cancelled {
                    return Err(MigrationAppApiError::Cancelled);
                }
                let request_ids = requests
                    .requests()
                    .iter()
                    .map(cairn_migration::UserIntentDecisionRequestV1::identity)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(MigrationAppApiError::internal)?;
                match take_resolved_intent_decisions(state, &request_ids)? {
                    IntentDecisionReadinessV1::Reconciliation {
                        newly_paused: true,
                        unresolved_decision_count,
                    } => {
                        tracing::info!(
                            target: "cairn.migration.admission",
                            event = "intent_admission_waiting_for_reconciliation",
                            task_id = %task.task_id(),
                            unresolved_decision_count,
                            "Intent Admission paused on explicit task-authority unknowns"
                        );
                    }
                    IntentDecisionReadinessV1::Ready(decisions) => {
                        return crate::AuthorizedIntentDecisionSetV1::new(requests, decisions)
                            .map_err(MigrationAppApiError::internal);
                    }
                    IntentDecisionReadinessV1::Pending
                    | IntentDecisionReadinessV1::Reconciliation {
                        newly_paused: false,
                        ..
                    } => {}
                }
            }
            notified.await;
        }
    }

    async fn commit_intent_admission(
        &mut self,
        prepared: &PreparedIntentAdmissionV1,
    ) -> Result<(), Self::Error> {
        self.transition(
            prepared.public_outcome().contract().task_id(),
            TaskPhaseV1::PreparingOracle,
            None,
        )
    }

    async fn commit_suspended_agent_loop(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
    ) -> Result<(), Self::Error> {
        self.transition(
            checkpoint.start().task_id(),
            TaskPhaseV1::Blocked,
            Some(TaskAttentionV1::Reconciliation),
        )
    }

    async fn ensure_task_active(&mut self, task_id: TaskId) -> Result<(), Self::Error> {
        let tasks = self.tasks.lock()?;
        let state = tasks
            .get(&task_id)
            .ok_or(MigrationAppApiError::TaskNotFound)?;
        if state.cancelled {
            Err(MigrationAppApiError::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn commit_workflow_failure(
        &mut self,
        task_id: TaskId,
        failure: MigrationWorkflowFailureClassV1,
    ) -> Result<(), Self::Error> {
        let mut tasks = self.tasks.lock()?;
        let state = tasks
            .get_mut(&task_id)
            .ok_or(MigrationAppApiError::TaskNotFound)?;
        let Some(attention) = state.commit_workflow_failure(failure)? else {
            return Ok(());
        };
        tracing::warn!(
            target: "cairn.migration.lifecycle",
            event = "migration_task_failure_committed",
            task_id = %task_id,
            failure_class = ?failure,
            attention = ?attention,
            "CUDA migration task failure committed as operator-visible attention"
        );
        Ok(())
    }

    async fn prepare_oracle_workspace(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
    ) -> Result<OracleWorkspaceV1, Self::Error> {
        let contract = intent.prepared().public_outcome().contract();
        let workspace = self.materials.register_oracle(
            task.task_id(),
            contract
                .identity()
                .map_err(MigrationAppApiError::internal)?,
            contract.admitted_claims().cloned().collect(),
            self.oracle_policy.clone(),
            self.oracle_catalog.clone(),
        )?;
        self.transition(task.task_id(), TaskPhaseV1::ExploringOracle, None)?;
        let workspace_id = workspace
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        let policy_id = self
            .oracle_policy
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        let strategy_catalog_id = self
            .oracle_catalog
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        tracing::info!(
            target: "cairn.migration.oracle",
            event = "oracle_workspace_frozen",
            task_id = %task.task_id(),
            workspace_id = %workspace_id,
            policy_id = %policy_id,
            strategy_catalog_id = %strategy_catalog_id,
            "Oracle Exploration workspace frozen with exact authority bindings"
        );
        Ok(workspace)
    }

    fn derive_required_oracle_dimensions(
        &mut self,
        task: &FrozenMigrationTaskV1,
        intent: &AdmittedIntentV1,
        workspace: &OracleWorkspaceV1,
    ) -> Result<Vec<OracleDimensionV1>, Self::Error> {
        let contract = intent.prepared().public_outcome().contract();
        if workspace.task_id() != task.task_id()
            || workspace.admitted_intent()
                != contract
                    .identity()
                    .map_err(MigrationAppApiError::internal)?
        {
            return Err(MigrationAppApiError::internal(
                "Oracle dimension binding drift",
            ));
        }
        let claims = derive_oracle_claims(
            task.task_id(),
            workspace.admitted_intent(),
            &contract.admitted_claims().cloned().collect::<Vec<_>>(),
        );
        let mut claim_ids = claims
            .iter()
            .map(cairn_migration::OracleClaimV1::identity)
            .collect::<Result<Vec<_>, _>>()
            .map_err(MigrationAppApiError::internal)?;
        claim_ids.sort_by_key(cairn_protocol::ContentId::to_wire);
        derive_oracle_dimensions(&claim_ids, &self.oracle_policy)
            .map_err(MigrationAppApiError::internal)
    }

    fn commit_oracle_portfolio_review_candidate(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: &cairn_migration::OraclePortfolioProposalV1,
    ) -> Result<(), Self::Error> {
        self.materials
            .record_oracle_portfolio(task.task_id(), proposal)?;
        let portfolio_id = proposal
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        tracing::info!(
            target: "cairn.migration.review",
            event = "oracle_portfolio_review_candidate_committed",
            task_id = %task.task_id(),
            portfolio_id = %portfolio_id,
            item_count = proposal.accepted_items().len(),
            "exact mechanically assembled Oracle portfolio committed for coherence Review"
        );
        Ok(())
    }

    async fn commit_oracle_revision_request(
        &mut self,
        task: &FrozenMigrationTaskV1,
        request: &cairn_migration::OracleRevisionRequestV1,
    ) -> Result<(), Self::Error> {
        self.materials
            .record_oracle_revision_request(task.task_id(), request)?;
        let failed = request
            .evidence()
            .receipts()
            .iter()
            .filter(|receipt| receipt.result() == cairn_migration::OracleControlResultV1::Failed)
            .count();
        let unresolved = request
            .outcome()
            .claims()
            .iter()
            .map(|claim| claim.unresolved_items().len() + claim.rejected_items().len())
            .sum::<usize>();
        let issue_count = failed + unresolved;
        tracing::info!(
            target: "cairn.migration.revision",
            event = "oracle_revision_feedback_committed",
            task_id = %task.task_id(),
            proposal_id = %request.proposal(),
            gate = "admission",
            issue_count,
            "exact gate feedback committed for Oracle Revision"
        );
        Ok(())
    }

    async fn run_qualified_oracle_controls(
        &mut self,
        task: &FrozenMigrationTaskV1,
        _intent: &AdmittedIntentV1,
        proposal: &OraclePortfolioProposalV1,
        attempt: &OracleAdmissionAttemptV1,
    ) -> Result<OracleAdmissionEvidenceV1, Self::Error> {
        let plans = self
            .materials
            .oracle_check_plans(task.task_id(), proposal)?;
        self.transition(task.task_id(), TaskPhaseV1::RunningOracleControls, None)?;
        self.oracle_controls
            .execute_controls(task.task_id(), proposal, attempt, &plans)
            .await
            .map_err(MigrationAppApiError::internal)
    }

    async fn qualify_oracle_admission_mechanisms(
        &mut self,
        task: &FrozenMigrationTaskV1,
        _intent: &AdmittedIntentV1,
        proposal: &OraclePortfolioProposalV1,
        policy: &cairn_migration::OracleAdmissionPolicyV1,
    ) -> Result<cairn_migration::OracleAdmissionMechanismCatalogV1, Self::Error> {
        let plans = self
            .materials
            .oracle_check_plans(task.task_id(), proposal)?;
        self.transition(task.task_id(), TaskPhaseV1::AwaitingOracleControls, None)?;
        self.oracle_controls
            .qualify(task.task_id(), proposal, policy, &plans)
            .await
            .map_err(|error| match error {
                OracleControlRunnerError::SemanticExecutionUnavailable => {
                    MigrationAppApiError::OracleSemanticMechanismUnavailable
                }
                error => MigrationAppApiError::internal(error),
            })
    }

    async fn authorize_candidate_build(
        &mut self,
        _task: &FrozenMigrationTaskV1,
        _intent: &AdmittedIntentV1,
        _oracle: &AdmittedOracleV1,
        _contract: &CandidateOracleContractV1,
        candidate: &CandidateProposalV1,
        _attempt: &CandidateAdmissionAttemptV1,
    ) -> Result<Self::CandidateBuildAuthority, Self::Error> {
        self.candidate_build
            .as_ref()
            .ok_or(MigrationAppApiError::CandidateBuildWorkerUnavailable)?
            .authorize(candidate)
            .map_err(MigrationAppApiError::internal)
    }

    async fn observe_candidate_on_worker(
        &mut self,
        authority: Self::CandidateBuildAuthority,
        _attempt: &CandidateAdmissionAttemptV1,
    ) -> Result<CandidateAdmissionEvidenceV1, Self::Error> {
        let observation = self
            .candidate_build
            .as_ref()
            .ok_or(MigrationAppApiError::CandidateBuildWorkerUnavailable)?
            .observe(authority)
            .await
            .map_err(MigrationAppApiError::internal)?;
        tracing::info!(
            target: "cairn.migration.candidate",
            event = "candidate_build_observed",
            worker_job_id = %observation.job_id(),
            worker_attempt_id = %observation.attempt_id(),
            build_request = %observation.request(),
            receipt_id = %observation.receipt_id(),
            outcome = ?observation.receipt().outcome(),
            exit_code = ?observation.receipt().exit_code(),
            compiled = observation.compiled(),
            "candidate build reached a terminal Worker receipt"
        );
        Err(MigrationAppApiError::CandidateMechanismExecutionUnavailable)
    }

    async fn record_terminal_outcome(
        &mut self,
        _outcome: &MigrationTerminalOutcomeV1,
    ) -> Result<(), Self::Error> {
        Err(MigrationAppApiError::NotImplemented(
            "Terminal outcome recording",
        ))
    }
}

impl MigrationProductServicesV1 {
    fn transition(
        &self,
        task_id: TaskId,
        phase: TaskPhaseV1,
        attention: Option<TaskAttentionV1>,
    ) -> Result<(), MigrationAppApiError> {
        let mut tasks = self.tasks.lock()?;
        let state = tasks
            .get_mut(&task_id)
            .ok_or(MigrationAppApiError::TaskNotFound)?;
        if phase == TaskPhaseV1::Cancelled {
            state.transition(phase, attention)?;
        } else {
            state.transition_if_active(phase, attention)?;
        }
        tracing::info!(
            target: "cairn.migration.lifecycle",
            event = "migration_task_phase_changed",
            task_id = %task_id,
            phase = ?phase,
            attention = ?attention,
            sequence = state.progress.last().map_or(0, |item| item.sequence.get()),
            "CUDA migration task phase changed"
        );
        Ok(())
    }
}

const fn workflow_failure_attention(failure: MigrationWorkflowFailureClassV1) -> TaskAttentionV1 {
    match failure {
        MigrationWorkflowFailureClassV1::AgentLoopExecution
        | MigrationWorkflowFailureClassV1::AgentLoopExhausted => TaskAttentionV1::AgentExecution,
        MigrationWorkflowFailureClassV1::OracleSemanticMechanismUnavailable => {
            TaskAttentionV1::OracleMechanisms
        }
        _ => TaskAttentionV1::WorkflowFailure,
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    sender: mpsc::Sender<SubmittedMigrationTaskV1>,
    tasks: SharedTasksV1,
    authority_subject: TaskIntentAuthoritySubject,
) -> Result<(), MigrationAppApiError> {
    let request = read_frame(&mut stream)
        .await
        .map_err(MigrationAppApiError::internal)?;
    let response = match handle_request(request, &sender, &tasks, &authority_subject).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                target: "cairn.migration.app-api",
                event = "migration_app_api_request_rejected",
                error_class = error.log_class(),
                "CUDA migration App API request rejected"
            );
            CairnResponseV1::Error { code: error.code() }
        }
    };
    write_frame(&mut stream, &response)
        .await
        .map_err(MigrationAppApiError::internal)
}

#[allow(
    clippy::too_many_lines,
    reason = "the closed SDK request variants remain one visible product dispatch table"
)]
async fn handle_request(
    request: CairnRequestV1,
    sender: &mpsc::Sender<SubmittedMigrationTaskV1>,
    tasks: &SharedTasksV1,
    authority_subject: &TaskIntentAuthoritySubject,
) -> Result<CairnResponseV1, MigrationAppApiError> {
    match request {
        CairnRequestV1::SubmitTask {
            command_id,
            task_id,
            submission,
        } => {
            {
                let mut states = tasks.lock()?;
                if states.contains_key(&task_id) {
                    return Err(MigrationAppApiError::Conflict);
                }
                states.insert(task_id, TaskStateV1::new(task_id)?);
            }
            sender
                .send(SubmittedMigrationTaskV1 {
                    task_id,
                    submission,
                })
                .await
                .map_err(|_| MigrationAppApiError::WorkflowUnavailable)?;
            let task = task_resource(tasks, task_id)?;
            tracing::info!(
                target: "cairn.migration.app-api",
                event = "migration_task_submitted",
                task_id = %task_id,
                command_id = %command_id,
                "CUDA migration task accepted at the product boundary"
            );
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
        CairnRequestV1::ListTasks => {
            let states = tasks.lock()?;
            Ok(CairnResponseV1::Tasks {
                tasks: states
                    .iter()
                    .map(|(task_id, state)| state.resource(*task_id))
                    .collect(),
            })
        }
        CairnRequestV1::GetTask { task_id } => Ok(CairnResponseV1::Task {
            task: task_resource(tasks, task_id)?,
        }),
        CairnRequestV1::GetTaskProgress {
            task_id,
            after_sequence,
        } => {
            let states = tasks.lock()?;
            let state = states
                .get(&task_id)
                .ok_or(MigrationAppApiError::TaskNotFound)?;
            Ok(CairnResponseV1::Progress {
                page: TaskProgressPageV1 {
                    task: state.resource(task_id),
                    items: state
                        .progress
                        .iter()
                        .filter(|item| after_sequence.is_none_or(|after| item.sequence > after))
                        .cloned()
                        .collect(),
                },
            })
        }
        CairnRequestV1::CancelTask {
            command_id,
            task_id,
        } => {
            let task = {
                let mut states = tasks.lock()?;
                let state = states
                    .get_mut(&task_id)
                    .ok_or(MigrationAppApiError::TaskNotFound)?;
                state.cancelled = true;
                state.transition(TaskPhaseV1::Cancelled, None)?;
                state.notify.notify_waiters();
                state.resource(task_id)
            };
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
        CairnRequestV1::GetIntentReview { task_id } => {
            let states = tasks.lock()?;
            let review = states
                .get(&task_id)
                .ok_or(MigrationAppApiError::TaskNotFound)?
                .intent_review
                .clone()
                .ok_or(MigrationAppApiError::NotReady)?;
            Ok(CairnResponseV1::IntentReview {
                review: Box::new(review),
            })
        }
        CairnRequestV1::SelectIntentHypothesis {
            command_id,
            task_id,
            request_id,
            hypothesis,
            authority_scope,
        } => {
            record_intent_decision(
                tasks,
                task_id,
                request_id,
                authority_scope,
                authority_subject,
                UserIntentDecisionResponseV1::SelectHypothesis { hypothesis },
            )?;
            Ok(CairnResponseV1::Mutation {
                command_id,
                task: task_resource(tasks, task_id)?,
            })
        }
        CairnRequestV1::KeepIntentUnknown {
            command_id,
            task_id,
            request_id,
            authority_scope,
        } => {
            record_intent_decision(
                tasks,
                task_id,
                request_id,
                authority_scope,
                authority_subject,
                UserIntentDecisionResponseV1::KeepUnknown,
            )?;
            Ok(CairnResponseV1::Mutation {
                command_id,
                task: task_resource(tasks, task_id)?,
            })
        }
        CairnRequestV1::ProvideIntentClaim {
            command_id,
            task_id,
            request_id,
            authority_scope,
            claim,
        } => {
            record_intent_decision(
                tasks,
                task_id,
                request_id,
                authority_scope,
                authority_subject,
                UserIntentDecisionResponseV1::ProvideAuthoritativeClaim { claim },
            )?;
            Ok(CairnResponseV1::Mutation {
                command_id,
                task: task_resource(tasks, task_id)?,
            })
        }
        CairnRequestV1::ReconcileIntentAdmission {
            command_id,
            task_id,
        } => {
            let task = {
                let states = tasks.lock()?;
                let state = states
                    .get(&task_id)
                    .ok_or(MigrationAppApiError::TaskNotFound)?;
                if state.phase != TaskPhaseV1::AwaitingIntentReview
                    && (state.phase != TaskPhaseV1::Blocked
                        || state.attention != Some(TaskAttentionV1::IntentAdmissionReconciliation))
                {
                    return Err(MigrationAppApiError::NotReady);
                }
                let review = state
                    .intent_review
                    .as_ref()
                    .ok_or(MigrationAppApiError::NotReady)?;
                if state.decisions.len() != review.requests.len() {
                    return Err(MigrationAppApiError::NotReady);
                }
                state.notify.notify_waiters();
                state.resource(task_id)
            };
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
    }
}

fn record_intent_decision(
    tasks: &SharedTasksV1,
    task_id: TaskId,
    request_id: cairn_protocol::ContentId<cairn_migration::UserIntentDecisionRequestArtifact>,
    authority_scope: cairn_migration::UserIntentAuthorityScopeV1,
    authority_subject: &TaskIntentAuthoritySubject,
    response: UserIntentDecisionResponseV1,
) -> Result<(), MigrationAppApiError> {
    let mut states = tasks.lock()?;
    let state = states
        .get_mut(&task_id)
        .ok_or(MigrationAppApiError::TaskNotFound)?;
    let revising_unknown = state.phase == TaskPhaseV1::Blocked
        && state.attention == Some(TaskAttentionV1::IntentAdmissionReconciliation);
    if state.phase != TaskPhaseV1::AwaitingIntentReview && !revising_unknown {
        return Err(MigrationAppApiError::NotReady);
    }
    let review = state
        .intent_review
        .as_ref()
        .ok_or(MigrationAppApiError::NotReady)?;
    let request = review
        .requests
        .iter()
        .find(|request| request.request_id == request_id)
        .map(|request| request.request.clone())
        .ok_or(MigrationAppApiError::Conflict)?;
    if let UserIntentDecisionResponseV1::SelectHypothesis { hypothesis } = &response {
        if !request
            .options()
            .iter()
            .any(|option| option.hypothesis() == hypothesis)
        {
            return Err(MigrationAppApiError::Conflict);
        }
    }
    let grant =
        UserIntentAuthorityGrantV1::new(task_id, authority_subject.clone(), authority_scope);
    let decision = UserIntentDecisionV1::new(
        request_id,
        grant.identity().map_err(MigrationAppApiError::internal)?,
        response,
    );
    let authorized = AuthorizedIntentDecisionV1::new(request, grant, decision)
        .map_err(MigrationAppApiError::internal)?;
    if let Some(position) = state.decisions.iter().position(|decision| {
        decision
            .request
            .identity()
            .is_ok_and(|identity| identity == request_id)
    }) {
        if !revising_unknown {
            return Err(MigrationAppApiError::NotReady);
        }
        state.decisions[position] = authorized;
    } else {
        state.decisions.push(authorized);
    }
    state.notify.notify_waiters();
    tracing::info!(
        target: "cairn.migration.app-api",
        event = "intent_authority_decision_recorded",
        task_id = %task_id,
        request_id = %request_id,
        decisions_recorded = state.decisions.len(),
        decisions_required = review.requests.len(),
        "task-authority Intent decision recorded"
    );
    Ok(())
}

fn task_resource(
    tasks: &SharedTasksV1,
    task_id: TaskId,
) -> Result<TaskResourceV1, MigrationAppApiError> {
    tasks
        .lock()?
        .get(&task_id)
        .map(|state| state.resource(task_id))
        .ok_or(MigrationAppApiError::TaskNotFound)
}

fn prepare_socket(path: &Path) -> Result<(), MigrationAppApiError> {
    if !path.is_absolute() {
        return Err(MigrationAppApiError::InvalidConfiguration);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            fs::remove_file(path).map_err(MigrationAppApiError::io)
        }
        Ok(_) => Err(MigrationAppApiError::UnsafeSocketTarget),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MigrationAppApiError::io(error)),
    }
}

fn observed_now() -> Result<ObservedAtUnixMillis, MigrationAppApiError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(MigrationAppApiError::internal)?
        .as_millis();
    Ok(ObservedAtUnixMillis::new(
        i64::try_from(millis).map_err(MigrationAppApiError::internal)?,
    ))
}

/// Stable product-boundary error classes; request responses expose only their public code.
#[derive(Debug, Error)]
pub enum MigrationAppApiError {
    #[error("migration App API configuration is invalid")]
    InvalidConfiguration,
    #[error("migration App API socket target is not a Unix socket")]
    UnsafeSocketTarget,
    #[error("migration task was not found")]
    TaskNotFound,
    #[error("migration request conflicts with current task state")]
    Conflict,
    #[error("migration task is not ready for this request")]
    NotReady,
    #[error("migration task was cancelled")]
    Cancelled,
    #[error("migration workflow inbox is unavailable")]
    WorkflowUnavailable,
    #[error("migration App API state lock is unavailable")]
    StatePoisoned,
    #[error("migration product capability is not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("no candidate-facing executable Oracle mechanism is available")]
    OracleSemanticMechanismUnavailable,
    #[error("no candidate build worker is configured")]
    CandidateBuildWorkerUnavailable,
    #[error("no qualified Candidate mechanism can observe the built artifact")]
    CandidateMechanismExecutionUnavailable,
    #[error("migration App API I/O failed: {0}")]
    Io(String),
    #[error("migration App API internal operation failed: {0}")]
    Internal(String),
}

impl MigrationAppApiError {
    fn io(error: impl std::fmt::Display) -> Self {
        Self::Io(error.to_string())
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }

    pub(crate) const fn log_class(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "invalid-configuration",
            Self::UnsafeSocketTarget => "unsafe-socket-target",
            Self::TaskNotFound => "task-not-found",
            Self::Conflict => "conflict",
            Self::NotReady => "not-ready",
            Self::Cancelled => "cancelled",
            Self::WorkflowUnavailable => "workflow-unavailable",
            Self::StatePoisoned => "state-poisoned",
            Self::NotImplemented(_) => "not-implemented",
            Self::OracleSemanticMechanismUnavailable => "oracle-semantic-mechanism-unavailable",
            Self::CandidateBuildWorkerUnavailable => "candidate-build-worker-unavailable",
            Self::CandidateMechanismExecutionUnavailable => {
                "candidate-mechanism-execution-unavailable"
            }
            Self::Io(_) => "io",
            Self::Internal(_) => "internal",
        }
    }

    const fn code(&self) -> AppApiErrorCodeV1 {
        match self {
            Self::InvalidConfiguration | Self::UnsafeSocketTarget => {
                AppApiErrorCodeV1::InvalidRequest
            }
            Self::TaskNotFound => AppApiErrorCodeV1::TaskNotFound,
            Self::Conflict => AppApiErrorCodeV1::Conflict,
            Self::NotReady | Self::Cancelled => AppApiErrorCodeV1::NotReady,
            Self::WorkflowUnavailable
            | Self::StatePoisoned
            | Self::NotImplemented(_)
            | Self::OracleSemanticMechanismUnavailable
            | Self::CandidateBuildWorkerUnavailable
            | Self::CandidateMechanismExecutionUnavailable
            | Self::Io(_)
            | Self::Internal(_) => AppApiErrorCodeV1::Internal,
        }
    }
}

impl MigrationProductServiceError for MigrationAppApiError {
    fn workflow_failure_class(&self) -> MigrationWorkflowFailureClassV1 {
        match self {
            Self::OracleSemanticMechanismUnavailable => {
                MigrationWorkflowFailureClassV1::OracleSemanticMechanismUnavailable
            }
            _ => MigrationWorkflowFailureClassV1::ProductService,
        }
    }
}

impl From<crate::MigrationAgentRuntimeError> for MigrationAppApiError {
    fn from(error: crate::MigrationAgentRuntimeError) -> Self {
        Self::Internal(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_api_task_progresses_while_the_workflow_runs_blocking_provider_work() {
        let (workflow_started_tx, workflow_started_rx) = tokio::sync::oneshot::channel();
        let (api_progress_tx, api_progress_rx) = tokio::sync::oneshot::channel();
        let product = tokio::spawn(run_product_tasks(
            Box::pin(async move {
                workflow_started_rx.await.expect("workflow started");
                api_progress_tx.send(()).expect("record API progress");
                std::future::pending::<Result<(), MigrationAppApiError>>().await
            }),
            Box::pin(async move {
                workflow_started_tx.send(()).expect("mark workflow started");
                tokio::task::block_in_place(|| {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                });
                std::future::pending::<Result<(), MigrationAppApiError>>().await
            }),
        ));

        tokio::time::timeout(std::time::Duration::from_millis(100), api_progress_rx)
            .await
            .expect("App API task must not wait for blocking provider work")
            .expect("App API progress signal");
        product.abort();
        let _ = product.await;
    }

    #[test]
    fn cancelled_task_cannot_be_advanced_or_reclassified_as_failed() {
        let mut state = TaskStateV1::new(TaskId::new()).expect("task state");
        state.cancelled = true;
        state
            .transition(TaskPhaseV1::Cancelled, None)
            .expect("cancel transition");

        assert!(matches!(
            state.transition_if_active(TaskPhaseV1::ExploringOracle, None),
            Err(MigrationAppApiError::Cancelled)
        ));
        assert_eq!(
            state
                .commit_workflow_failure(MigrationWorkflowFailureClassV1::AgentLoopExecution)
                .expect("cancelled failure is inert"),
            None
        );
        assert_eq!(state.phase, TaskPhaseV1::Cancelled);
        assert_eq!(state.attention, None);
    }

    #[test]
    fn workflow_failure_attention_distinguishes_recoverable_capabilities() {
        assert_eq!(
            workflow_failure_attention(MigrationWorkflowFailureClassV1::AgentLoopExecution),
            TaskAttentionV1::AgentExecution
        );
        assert_eq!(
            workflow_failure_attention(
                MigrationWorkflowFailureClassV1::OracleSemanticMechanismUnavailable
            ),
            TaskAttentionV1::OracleMechanisms
        );
        assert_eq!(
            workflow_failure_attention(MigrationWorkflowFailureClassV1::Domain),
            TaskAttentionV1::WorkflowFailure
        );
    }

    #[test]
    fn semantic_mechanism_failure_survives_the_product_service_boundary() {
        let error = MigrationAppApiError::OracleSemanticMechanismUnavailable;

        assert_eq!(
            error.workflow_failure_class(),
            MigrationWorkflowFailureClassV1::OracleSemanticMechanismUnavailable
        );
        assert_eq!(error.log_class(), "oracle-semantic-mechanism-unavailable");
    }

    #[test]
    fn explicit_unknown_pauses_for_intent_reconciliation_without_repeated_progress() {
        let mut state = TaskStateV1::new(TaskId::new()).expect("task state");
        state
            .transition(
                TaskPhaseV1::AwaitingIntentReview,
                Some(TaskAttentionV1::IntentReview),
            )
            .expect("review transition");

        assert!(pause_for_intent_reconciliation(&mut state, 1).expect("pause for reconciliation"));
        assert_eq!(state.phase, TaskPhaseV1::Blocked);
        assert_eq!(
            state.attention,
            Some(TaskAttentionV1::IntentAdmissionReconciliation)
        );
        let progress_len = state.progress.len();
        assert!(
            !pause_for_intent_reconciliation(&mut state, 1)
                .expect("already paused reconciliation is stable")
        );
        assert_eq!(state.progress.len(), progress_len);
    }
}
