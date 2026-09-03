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
    CandidateOracleContractV1, CandidateProposalArtifact, CandidateProposalV1,
    CandidateSearchLoopV1, IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1,
    IntentRecoveryInputV1, OracleAdmissionAttemptV1, OracleAdmissionEvidenceV1,
    OracleCoveragePolicyV1, OracleDimensionV1, OraclePortfolioProposalV1, OracleStrategyCatalogV1,
    OracleWorkspaceV1, PreparedIntentAdmissionV1, ReasoningDecompositionPolicyV1,
    SirCapabilityManifestV1, SirTaskLimits, SirTaskWorkspace, TaskIntentAuthoritySubject,
    UserIntentAuthorityGrantV1, UserIntentDecisionResponseV1, UserIntentDecisionV1,
    derive_oracle_claims, derive_oracle_dimensions,
};
use cairn_protocol::{
    BlobDigest, CommandId, ContentId, EventId, EventSequence, ObservedAtUnixMillis, TaskId,
};
use cairn_sdk::{
    AppApiErrorCodeV1, CairnRequestV1, CairnResponseV1, IntentReviewRequestResourceV1,
    IntentReviewResourceV1, TaskArchiveManifestV1, TaskAttentionV1, TaskPhaseV1,
    TaskProgressItemV1, TaskProgressPageV1, TaskResourceV1, TaskSubmissionV1, read_optional_frame,
    write_frame,
};
use cairn_server::{ApplicationModule, ApplicationName};
use cairn_store_sqlite::SqliteEventStore;
use thiserror::Error;
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Notify, mpsc},
};

use crate::{
    AdmittedIntentV1, AuthorizedCandidateBuildV1, AuthorizedIntentDecisionV1,
    CandidateBuildRunnerV1, CudaMigrationApplication, CudaMigrationProductServices,
    FrozenMigrationTaskV1, MigrationAgentRuntimeExecutorV1, MigrationProductServiceError,
    MigrationRuntimeMaterialsV1, MigrationTaskRequest, MigrationTerminalOutcomeV1,
    MigrationWorkflowFailureClassV1, OracleControlRunnerError, OracleControlRunnerV1,
};

/// Exact request admitted by the App API and sent to the product workflow inbox.
pub struct SubmittedMigrationTaskV1 {
    task_id: TaskId,
    archive: Vec<u8>,
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

/// Archive bytes staged on one connection while their upload is still in progress.
///
/// The staging lives on the connection because that is the only place from which the client going
/// away is observable. A map keyed by an upload identity would need someone to decide when an
/// abandoned upload dies, and the answer would have to be a clock; this system has already
/// replaced one such clock with an observed fact, and adding a new one here would undo that.
struct PendingArchiveUploadV1 {
    manifest: TaskArchiveManifestV1,
    received: Vec<u8>,
}

impl PendingArchiveUploadV1 {
    /// Stages an upload once its declared length is within the configured archive bound.
    ///
    /// Nothing is preallocated. A declaration costs the server no memory until bytes arrive, so an
    /// archive declared and never sent costs what it delivered, which is nothing.
    fn begin(
        manifest: TaskArchiveManifestV1,
        limits: cairn_migration::TaskArchiveLimits,
    ) -> Result<Self, MigrationAppApiError> {
        if manifest.byte_len() > limits.max_archive_bytes {
            return Err(MigrationAppApiError::ArchiveUploadTooLarge);
        }
        Ok(Self {
            manifest,
            received: Vec::new(),
        })
    }

    /// Appends one chunk at the exact offset the transfer has reached, returning the new total.
    ///
    /// The offset is carried so that a duplicated, reordered or overlapping chunk is a refusal
    /// rather than a silently corrupted archive. It is not a resume cursor: staging dies with the
    /// connection, so there is never a transfer to resume onto.
    fn accept_chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<u64, MigrationAppApiError> {
        let received =
            u64::try_from(self.received.len()).map_err(MigrationAppApiError::internal)?;
        let chunk_len = u64::try_from(bytes.len()).map_err(MigrationAppApiError::internal)?;
        let total = received
            .checked_add(chunk_len)
            .ok_or(MigrationAppApiError::ArchiveUploadRangeInvalid)?;
        if bytes.is_empty() || offset != received || total > self.manifest.byte_len() {
            return Err(MigrationAppApiError::ArchiveUploadRangeInvalid);
        }
        self.received.extend_from_slice(bytes);
        Ok(total)
    }

    /// Returns the uploaded bytes once they are exactly the archive the manifest declared.
    ///
    /// Length alone would accept a transfer whose chunks arrived intact but wrong; the digest is
    /// what makes a completed upload reproduce the client's bytes or fail.
    fn into_archive(self) -> Result<Vec<u8>, MigrationAppApiError> {
        let byte_len =
            u64::try_from(self.received.len()).map_err(MigrationAppApiError::internal)?;
        if byte_len != self.manifest.byte_len()
            || BlobDigest::derive(&self.received) != self.manifest.digest()
        {
            return Err(MigrationAppApiError::ArchiveIdentityMismatch);
        }
        Ok(self.received)
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
    archive_limits: cairn_migration::TaskArchiveLimits,
}

impl MigrationAppApiV1 {
    /// Binds the current-V1 Unix App API and serves canonical requests until a connection ends.
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
            let archive_limits = self.archive_limits;
            tokio::spawn(async move {
                if let Err(error) =
                    handle_connection(stream, sender, tasks, authority_subject, archive_limits)
                        .await
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
    candidate_search: CandidateSearchStoreV1,
    task_workspace: crate::TaskWorkspaceStoreV1,
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
    server: cairn_server::ServerConfig,
    search_policy: cairn_migration::CandidateSearchPolicyV1,
    workspaces: PathBuf,
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
            archive_limits,
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
            candidate_search: CandidateSearchStoreV1::new(server, search_policy),
            task_workspace: crate::TaskWorkspaceStoreV1::new(workspaces),
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
        let sources = cairn_migration::extract_task_sources(&request.archive, self.archive_limits)
            .map_err(MigrationAppApiError::internal)?;
        let workspace = SirTaskWorkspace::from_sources(sources, self.task_limits)
            .map_err(MigrationAppApiError::internal)?;
        // The frozen source is written down before anything is built on it. Registering it in
        // memory first would let a task be admitted, reasoned about and scheduled while the one
        // copy of what it is about lived only in this process.
        self.task_workspace.freeze(request.task_id, &workspace)?;
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

    async fn register_candidate_authority(
        &mut self,
        task: &FrozenMigrationTaskV1,
        workspace: &cairn_migration::CandidateWorkspaceV1,
        contract: &CandidateOracleContractV1,
    ) -> Result<(), Self::Error> {
        self.materials
            .register_candidate(task.task_id(), workspace.clone(), contract.clone())?;
        self.transition(task.task_id(), TaskPhaseV1::ExploringCandidate, None)?;
        Ok(())
    }

    async fn authorize_candidate_build(
        &mut self,
        _task: &FrozenMigrationTaskV1,
        candidate: &CandidateProposalV1,
    ) -> Result<Self::CandidateBuildAuthority, Self::Error> {
        self.candidate_build
            .as_ref()
            .ok_or(MigrationAppApiError::CandidateBuildWorkerUnavailable)?
            .authorize(candidate)
            .map_err(MigrationAppApiError::internal)
    }

    async fn observe_candidate_build(
        &mut self,
        authority: Self::CandidateBuildAuthority,
    ) -> Result<cairn_migration::CandidateBuildOutcomeV1, Self::Error> {
        let proposal = authority.proposal();
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
        // The receipt says whether the exact toolchain accepted the exact artifact, which is a
        // search signal. It is deliberately not admission evidence: nothing here judged meaning.
        Ok(cairn_migration::CandidateBuildOutcomeV1::new(
            proposal,
            observation.receipt_id(),
            observation.compiled(),
        ))
    }

    async fn open_candidate_search(
        &mut self,
        task: &FrozenMigrationTaskV1,
    ) -> Result<cairn_migration::CandidateSearchStateV1, Self::Error> {
        self.candidate_search.open(task.task_id())
    }

    async fn record_candidate_proposal(
        &mut self,
        task: &FrozenMigrationTaskV1,
        proposal: ContentId<CandidateProposalArtifact>,
    ) -> Result<cairn_migration::CandidateSearchStateV1, Self::Error> {
        self.candidate_search
            .record_proposal(task.task_id(), proposal)
    }

    async fn record_missing_candidate_submission(
        &mut self,
        task: &FrozenMigrationTaskV1,
    ) -> Result<cairn_migration::CandidateSearchStateV1, Self::Error> {
        self.candidate_search
            .record_missing_submission(task.task_id())
    }

    async fn record_candidate_build_observation(
        &mut self,
        task: &FrozenMigrationTaskV1,
        outcome: cairn_migration::CandidateBuildOutcomeV1,
    ) -> Result<cairn_migration::CandidateSearchStateV1, Self::Error> {
        self.candidate_search
            .record_build_observation(task.task_id(), outcome)
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

/// Serves one connection until the client stops sending, carrying its archive staging along.
///
/// The connection outlives a single request because an upload is several of them, and the staged
/// bytes are held here rather than in shared state so that they are released exactly when this
/// connection ends, however it ends.
async fn handle_connection(
    mut stream: UnixStream,
    sender: mpsc::Sender<SubmittedMigrationTaskV1>,
    tasks: SharedTasksV1,
    authority_subject: TaskIntentAuthoritySubject,
    archive_limits: cairn_migration::TaskArchiveLimits,
) -> Result<(), MigrationAppApiError> {
    let mut upload: Option<PendingArchiveUploadV1> = None;
    while let Some(request) = read_optional_frame(&mut stream)
        .await
        .map_err(MigrationAppApiError::internal)?
    {
        let response = match handle_request(
            request,
            &sender,
            &tasks,
            &authority_subject,
            &mut upload,
            archive_limits,
        )
        .await
        {
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
            .map_err(MigrationAppApiError::internal)?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "the closed SDK request variants remain one visible product dispatch table"
)]
async fn handle_request(
    request: CairnRequestV1,
    sender: &mpsc::Sender<SubmittedMigrationTaskV1>,
    tasks: &SharedTasksV1,
    authority_subject: &TaskIntentAuthoritySubject,
    upload: &mut Option<PendingArchiveUploadV1>,
    archive_limits: cairn_migration::TaskArchiveLimits,
) -> Result<CairnResponseV1, MigrationAppApiError> {
    match request {
        CairnRequestV1::BeginArchiveUpload { archive } => {
            if upload.is_some() {
                return Err(MigrationAppApiError::ArchiveUploadAlreadyStarted);
            }
            *upload = Some(PendingArchiveUploadV1::begin(archive, archive_limits)?);
            Ok(CairnResponseV1::ArchiveUpload { received: 0 })
        }
        CairnRequestV1::PutArchiveChunk { offset, bytes } => {
            let received = upload
                .as_mut()
                .ok_or(MigrationAppApiError::ArchiveUploadNotStarted)?
                .accept_chunk(offset, &bytes)?;
            Ok(CairnResponseV1::ArchiveUpload { received })
        }
        CairnRequestV1::SubmitTask {
            command_id,
            task_id,
            submission,
        } => {
            // The archive is verified before the task identity is claimed. A submission whose
            // upload did not reproduce the client's bytes never becomes a task at all, so no task
            // state is created that would then have to be unwound.
            let archive = upload
                .take()
                .ok_or(MigrationAppApiError::ArchiveUploadNotStarted)?
                .into_archive()?;
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
                    archive,
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

/// The task-owned aggregate the candidate search loop records into.
fn candidate_search_loop(task_id: TaskId) -> Result<CandidateSearchLoopV1, MigrationAppApiError> {
    CandidateSearchLoopV1::new(task_id).map_err(MigrationAppApiError::internal)
}

/// Where a candidate search loop's transitions are durably recorded.
///
/// This is separated from the rest of the product services because it is the only part of them
/// that can be exercised on its own: it needs a deployment's store and nothing else, so the
/// question "does a transition survive being written and read back" has an answer that does not
/// require an admitted Oracle first.
#[derive(Clone, Debug)]
pub struct CandidateSearchStoreV1 {
    server: cairn_server::ServerConfig,
    policy: cairn_migration::CandidateSearchPolicyV1,
}

impl CandidateSearchStoreV1 {
    /// Binds one deployment's durable store to the policy new loops are frozen under.
    #[must_use]
    pub const fn new(
        server: cairn_server::ServerConfig,
        policy: cairn_migration::CandidateSearchPolicyV1,
    ) -> Self {
        Self { server, policy }
    }

    /// Opens the loop for this task, or returns the position an existing one already reached.
    ///
    /// # Errors
    ///
    /// Returns a store, codec, or invalid-history error.
    pub fn open(
        &self,
        task_id: TaskId,
    ) -> Result<cairn_migration::CandidateSearchStateV1, MigrationAppApiError> {
        let search = candidate_search_loop(task_id)?;
        let policy = self.policy;
        let observed_at = observed_now()?;
        self.on_store(move |events| {
            // Recovering before opening is what makes this idempotent. A loop that already exists
            // keeps its frozen policy and its position instead of being reopened under new ones.
            match cairn_migration::recover_candidate_search(events, &search)
                .map_err(MigrationAppApiError::internal)?
            {
                cairn_migration::CandidateSearchStateV1::NotFound => {
                    cairn_migration::open_candidate_search(
                        events,
                        &search,
                        policy,
                        &CommandId::new(),
                        observed_at,
                    )
                    .map_err(MigrationAppApiError::internal)
                }
                state => Ok(state),
            }
        })
    }

    /// Records one submitted proposal, or the fact that it repeats one already built.
    ///
    /// # Errors
    ///
    /// Returns a store error or an illegal transition.
    pub fn record_proposal(
        &self,
        task_id: TaskId,
        proposal: ContentId<CandidateProposalArtifact>,
    ) -> Result<cairn_migration::CandidateSearchStateV1, MigrationAppApiError> {
        let search = candidate_search_loop(task_id)?;
        let observed_at = observed_now()?;
        self.on_store(move |events| {
            cairn_migration::record_candidate_proposal(
                events,
                &search,
                proposal,
                &CommandId::new(),
                observed_at,
            )
            .map_err(MigrationAppApiError::internal)
        })
    }

    /// Records an episode that ended without any proposal.
    ///
    /// # Errors
    ///
    /// Returns a store error or an illegal transition.
    pub fn record_missing_submission(
        &self,
        task_id: TaskId,
    ) -> Result<cairn_migration::CandidateSearchStateV1, MigrationAppApiError> {
        let search = candidate_search_loop(task_id)?;
        let observed_at = observed_now()?;
        self.on_store(move |events| {
            cairn_migration::record_missing_submission(
                events,
                &search,
                &CommandId::new(),
                observed_at,
            )
            .map_err(MigrationAppApiError::internal)
        })
    }

    /// Folds one build observation back into durable state.
    ///
    /// # Errors
    ///
    /// Returns a store error or an observation that names a proposal this loop is not building.
    pub fn record_build_observation(
        &self,
        task_id: TaskId,
        outcome: cairn_migration::CandidateBuildOutcomeV1,
    ) -> Result<cairn_migration::CandidateSearchStateV1, MigrationAppApiError> {
        let search = candidate_search_loop(task_id)?;
        let observed_at = observed_now()?;
        self.on_store(move |events| {
            cairn_migration::record_candidate_build_observation(
                events,
                &search,
                outcome.proposal(),
                outcome.receipt(),
                outcome.compiled(),
                &CommandId::new(),
                observed_at,
            )
            .map_err(MigrationAppApiError::internal)
        })
    }

    /// Recovers the loop's position without changing it.
    ///
    /// # Errors
    ///
    /// Returns a store, codec, or invalid-history error.
    pub fn recover(
        &self,
        task_id: TaskId,
    ) -> Result<cairn_migration::CandidateSearchStateV1, MigrationAppApiError> {
        let search = candidate_search_loop(task_id)?;
        self.on_store(move |events| {
            cairn_migration::recover_candidate_search(events, &search)
                .map_err(MigrationAppApiError::internal)
        })
    }

    /// Runs one synchronous store interaction off the async runtime's own thread.
    ///
    /// The store is synchronous and every append is an fsync, so doing this inline would park a
    /// runtime worker for the duration. `block_in_place` requires a multi-threaded runtime, which
    /// `scripts/check-product-path.sh` holds this crate to.
    fn on_store<T>(
        &self,
        work: impl FnOnce(&mut SqliteEventStore) -> Result<T, MigrationAppApiError> + Send,
    ) -> Result<T, MigrationAppApiError>
    where
        T: Send,
    {
        let database = self.server.event_database();
        tokio::task::block_in_place(move || {
            let mut events =
                SqliteEventStore::open(&database).map_err(MigrationAppApiError::internal)?;
            work(&mut events)
        })
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
    #[error("no archive upload is in progress on this connection")]
    ArchiveUploadNotStarted,
    #[error("an archive upload is already in progress on this connection")]
    ArchiveUploadAlreadyStarted,
    #[error("archive chunk does not continue the declared transfer")]
    ArchiveUploadRangeInvalid,
    #[error("declared archive is above the configured bound")]
    ArchiveUploadTooLarge,
    #[error("uploaded archive is not the archive that was declared")]
    ArchiveIdentityMismatch,
    #[error("task artifact path leaves the task directory")]
    TaskWorkspacePathEscape,
    #[error("migration App API I/O failed: {0}")]
    Io(String),
    #[error("migration App API internal operation failed: {0}")]
    Internal(String),
}

impl MigrationAppApiError {
    pub(crate) fn io(error: impl std::fmt::Display) -> Self {
        Self::Io(error.to_string())
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
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
            Self::ArchiveUploadNotStarted => "archive-upload-not-started",
            Self::ArchiveUploadAlreadyStarted => "archive-upload-already-started",
            Self::ArchiveUploadRangeInvalid => "archive-upload-range-invalid",
            Self::ArchiveUploadTooLarge => "archive-upload-too-large",
            Self::ArchiveIdentityMismatch => "archive-identity-mismatch",
            Self::TaskWorkspacePathEscape => "task-workspace-path-escape",
            Self::Io(_) => "io",
            Self::Internal(_) => "internal",
        }
    }

    const fn code(&self) -> AppApiErrorCodeV1 {
        match self {
            Self::InvalidConfiguration
            | Self::UnsafeSocketTarget
            | Self::ArchiveUploadNotStarted
            | Self::ArchiveUploadRangeInvalid
            | Self::ArchiveUploadTooLarge
            | Self::ArchiveIdentityMismatch
            | Self::TaskWorkspacePathEscape => AppApiErrorCodeV1::InvalidRequest,
            Self::TaskNotFound => AppApiErrorCodeV1::TaskNotFound,
            Self::Conflict | Self::ArchiveUploadAlreadyStarted => AppApiErrorCodeV1::Conflict,
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
    use cairn_protocol::CommandId;
    use cairn_sdk::{CairnClient, CairnClientConfigV1, MAX_ARCHIVE_CHUNK_BYTES, UnixCairnClient};

    use super::*;

    fn recovery_request() -> cairn_migration::IntentRecoveryRequestV1 {
        serde_json::from_str(include_str!(
            "../../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json"
        ))
        .expect("recovery request")
    }

    fn archive_limits(max_archive_bytes: u64) -> cairn_migration::TaskArchiveLimits {
        cairn_migration::TaskArchiveLimits {
            max_archive_bytes,
            ..cairn_migration::TaskArchiveLimits::default()
        }
    }

    fn staged(archive: &[u8]) -> PendingArchiveUploadV1 {
        PendingArchiveUploadV1::begin(
            TaskArchiveManifestV1::describing(archive).expect("manifest"),
            archive_limits(1 << 20),
        )
        .expect("staged upload")
    }

    #[test]
    fn a_chunked_upload_reassembles_exactly_the_declared_archive() {
        let archive: Vec<u8> = (0..=255_u8).cycle().take(5000).collect();
        let mut pending = staged(&archive);

        let mut offset = 0_u64;
        for chunk in archive.chunks(1024) {
            offset = pending.accept_chunk(offset, chunk).expect("chunk accepted");
        }

        assert_eq!(offset, 5000);
        assert_eq!(pending.into_archive().expect("archive"), archive);
    }

    // The bound is applied to what was declared, so an archive above it costs the server nothing
    // to refuse. Checking it after the bytes arrive would mean accepting them first.
    #[test]
    fn a_declaration_above_the_bound_is_refused_before_any_bytes_arrive() {
        let manifest = TaskArchiveManifestV1::describing(&[7_u8; 64]).expect("manifest");

        assert!(matches!(
            PendingArchiveUploadV1::begin(manifest, archive_limits(63)),
            Err(MigrationAppApiError::ArchiveUploadTooLarge)
        ));
    }

    #[test]
    fn a_chunk_that_does_not_continue_the_transfer_is_refused() {
        let archive = vec![3_u8; 100];
        let mut pending = staged(&archive);

        assert!(matches!(
            pending.accept_chunk(0, &[]),
            Err(MigrationAppApiError::ArchiveUploadRangeInvalid)
        ));
        assert!(matches!(
            pending.accept_chunk(1, &archive[..10]),
            Err(MigrationAppApiError::ArchiveUploadRangeInvalid)
        ));
        assert_eq!(
            pending
                .accept_chunk(0, &archive[..40])
                .expect("first chunk"),
            40
        );
        assert!(matches!(
            pending.accept_chunk(0, &archive[..40]),
            Err(MigrationAppApiError::ArchiveUploadRangeInvalid)
        ));
        assert!(matches!(
            pending.accept_chunk(40, &[3_u8; 61]),
            Err(MigrationAppApiError::ArchiveUploadRangeInvalid)
        ));
        assert_eq!(
            pending
                .accept_chunk(40, &archive[40..])
                .expect("last chunk"),
            100
        );
    }

    // Length alone would accept a transfer whose chunks all arrived but carried the wrong bytes,
    // and a short one would arrive as a smaller archive that still unpacks.
    #[test]
    fn completion_refuses_an_upload_that_is_not_the_declared_archive() {
        let archive = vec![9_u8; 64];

        let mut short = staged(&archive);
        short.accept_chunk(0, &archive[..32]).expect("half");
        assert!(matches!(
            short.into_archive(),
            Err(MigrationAppApiError::ArchiveIdentityMismatch)
        ));

        let mut altered = staged(&archive);
        let mut other = archive.clone();
        other[7] = 0;
        altered.accept_chunk(0, &other).expect("full length");
        assert!(matches!(
            altered.into_archive(),
            Err(MigrationAppApiError::ArchiveIdentityMismatch)
        ));
    }

    #[tokio::test]
    async fn a_submission_without_an_upload_creates_no_task() {
        let (sender, _receiver) = mpsc::channel(1);
        let tasks = SharedTasksV1::default();
        let authority = TaskIntentAuthoritySubject::new("task-authority:test").expect("authority");
        let mut upload = None;

        let error = handle_request(
            CairnRequestV1::SubmitTask {
                command_id: CommandId::new(),
                task_id: TaskId::new(),
                submission: TaskSubmissionV1::new(recovery_request()),
            },
            &sender,
            &tasks,
            &authority,
            &mut upload,
            cairn_migration::TaskArchiveLimits::default(),
        )
        .await
        .expect_err("a submission with no uploaded archive cannot become a task");

        assert!(matches!(
            error,
            MigrationAppApiError::ArchiveUploadNotStarted
        ));
        assert!(tasks.lock().expect("tasks").is_empty());
    }

    // The archive here is deliberately larger than one frame, so it cannot have reached the
    // workflow inbox unless the upload actually spanned several requests on one connection.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_client_uploads_an_archive_larger_than_one_frame_and_submits_it() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let socket = directory.path().join("app.sock");
        let listener = UnixListener::bind(&socket).expect("listener");
        let (sender, mut receiver) = mpsc::channel(1);
        let tasks = SharedTasksV1::default();
        let authority = TaskIntentAuthoritySubject::new("task-authority:test").expect("authority");
        let served = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            handle_connection(
                stream,
                sender,
                tasks,
                authority,
                cairn_migration::TaskArchiveLimits::default(),
            )
            .await
        });

        let archive: Vec<u8> = (0..=255_u8)
            .cycle()
            .take(MAX_ARCHIVE_CHUNK_BYTES * 2 + 7)
            .collect();
        let client = UnixCairnClient::new(CairnClientConfigV1 {
            schema_version: 1,
            unix_socket: socket,
        })
        .expect("client");
        let command_id = CommandId::new();
        let task_id = TaskId::new();

        let response = client
            .submit_task(
                command_id,
                task_id,
                &archive,
                TaskSubmissionV1::new(recovery_request()),
            )
            .await
            .expect("submitted");

        match response {
            CairnResponseV1::Mutation {
                command_id: echoed,
                task,
            } => {
                assert_eq!(echoed, command_id);
                assert_eq!(task.task_id(), task_id);
            }
            other => panic!("expected a mutation response, got {other:?}"),
        }
        let submitted = receiver.recv().await.expect("workflow inbox");
        assert_eq!(submitted.task_id, task_id);
        assert_eq!(submitted.archive, archive);
        // A client that has nothing further to send just goes away, and that is not a failure.
        served
            .await
            .expect("server task")
            .expect("connection ended cleanly when the client went away");
    }

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

    // Freezing a task has to write its source down. Registering it in memory alone would let a
    // task be admitted, reasoned about and scheduled while the only copy of what it is about
    // lived in one process, which is a record the event log cannot make good on after a restart.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn freezing_a_task_persists_its_source_to_its_own_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let workspace = SirTaskWorkspace::from_sources(
            vec![(
                cairn_migration::SirTaskArtifactPath::new("src/kernel.cu").expect("path"),
                "void launch() {}\n".to_owned(),
            )],
            SirTaskLimits::default(),
        )
        .expect("workspace");
        let task_id = TaskId::new();

        crate::TaskWorkspaceStoreV1::new(directory.path().to_path_buf())
            .freeze(task_id, &workspace)
            .expect("freeze");

        // Read it back through a store this test opens itself, so nothing but the task's own
        // directory carries the answer.
        let recovered = crate::TaskWorkspaceStoreV1::new(directory.path().to_path_buf())
            .recover(task_id, SirTaskLimits::default())
            .expect("the frozen source is readable from the directory alone");
        assert_eq!(
            recovered
                .source(&cairn_migration::SirTaskArtifactPath::new("src/kernel.cu").expect("path")),
            Some("void launch() {}\n")
        );
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
