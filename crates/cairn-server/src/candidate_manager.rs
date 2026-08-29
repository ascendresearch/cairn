//! Controller-owned process manager for one existing Candidate workflow.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use cairn_agent::{
    AdapterVersion, EpisodeBudget, ModelOutputTokenLimit, ModelSelection, ResolvedRuntimeModel,
};
use cairn_execution::{
    AssignmentExecutionTerminal, ExecutionAssignmentState, ExecutionOutcome, ExecutionReceipt,
    ExecutionReceiptArtifact, recover_execution_assignment,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, CandidateNativeBuildDispatchV1,
    CandidateNativeBuildScheduleV1, CandidateNativeDiagnosticV1, CandidateNativePublicationV1,
    CandidateWorkflowNextActionV1, CandidateWorkflowStateV1, CandidateWorkflowTerminalV1,
    CollectionCandidateNativeFollowupRevisionArtifact,
    CollectionCandidateNativeRepairRevisionArtifact, CollectionCandidateRevisionArtifact,
    MigrationWorkflowV1, PreparedCandidateNativeFollowupBuildJob,
    PreparedCandidateNativeRepairBuildJob, PreparedCandidateNativeRevisionBuildJob,
    ProposalHostBinaryIdentity, ProposalHostRequestV1, ProposalHostRuntimeV1,
    ProposalHostTerminalV1, SirTaskLimits, prepare_candidate_native_build_diagnostic,
    prepare_candidate_native_followup_build_job, prepare_candidate_native_repair_build_diagnostic,
    prepare_candidate_native_repair_build_job,
    prepare_candidate_native_repair_round_build_diagnostic,
    prepare_candidate_native_revision_build_job, record_candidate_native_subject_failure,
    record_candidate_native_terminal, record_candidate_proposal_host_terminal,
    recover_candidate_workflow, request_candidate_episode, request_candidate_native_build,
    require_candidate_native_build_reconciliation,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentId, ContentType, ControlMessageId, EpisodeId, JobId,
    LeaseId, PlacementId, ReservationId, SchemaVersion, TaskId,
};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::{sleep, timeout},
};

use crate::{
    ControllerSchedulingOutcome, ScheduledAssignmentPhase, ServerConfig, ServerError,
    archive_proposal_host_runtime, observed_now, prepare_candidate_native_build_dispatch,
    prepare_candidate_proposal_host_request, schedule_candidate_native_build,
};

const HOST_REQUEST_BYTE_LIMIT: usize = 2 * 1024 * 1024;

macro_rules! positive_process_quantity {
    ($(#[$meta:meta])* $name:ident, $maximum:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive process-management quantity.
            ///
            /// # Errors
            ///
            /// Rejects zero.
            pub fn new(value: u64) -> Result<Self, ServerError> {
                if value == 0 || value > $maximum {
                    Err(ServerError::Configuration(concat!(stringify!($name), " is outside its positive current-V1 bound").into()))
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

positive_process_quantity!(
    /// Maximum wall-clock duration of one exact Proposal Host child operation.
    ProposalHostProcessTimeoutMillis,
    86_400_000
);
positive_process_quantity!(
    /// Maximum canonical terminal bytes accepted from Proposal Host stdout.
    ProposalHostStdoutByteLimit,
    2 * 1024 * 1024
);
positive_process_quantity!(
    /// Maximum observational diagnostic bytes retained from Proposal Host stderr.
    ProposalHostStderrByteLimit,
    2 * 1024 * 1024
);
positive_process_quantity!(
    /// Delay between recovery checks while one managed Worker attempt is active.
    CandidateWorkflowPollIntervalMillis,
    60_000
);

/// Strict current-V1 process policy for the generic Proposal Host used by this workflow.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostProcessConfigV1 {
    pub executable: PathBuf,
    pub state_root: PathBuf,
    pub resolved_runtime_model: PathBuf,
    pub selection: ModelSelection,
    pub budget: EpisodeBudget,
    pub max_output_tokens: ModelOutputTokenLimit,
    pub task_limits: SirTaskLimits,
    pub process_timeout_ms: ProposalHostProcessTimeoutMillis,
    pub stdout_byte_limit: ProposalHostStdoutByteLimit,
    pub stderr_byte_limit: ProposalHostStderrByteLimit,
}

/// One explicitly configured, already-opened Candidate workflow supervised by the Controller.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateWorkflowManagerConfigV1 {
    pub schema_version: SchemaVersion,
    pub task_id: TaskId,
    pub proposal_host: ProposalHostProcessConfigV1,
    pub poll_interval_ms: CandidateWorkflowPollIntervalMillis,
}

impl CandidateWorkflowManagerConfigV1 {
    pub(crate) fn validate(&self) -> Result<(), ServerError> {
        if self.schema_version.get() != 1 {
            return Err(ServerError::Configuration(
                "only Candidate workflow manager schema_version 1 is supported".into(),
            ));
        }
        if !self.proposal_host.executable.is_file() {
            return Err(ServerError::Configuration(
                "Proposal Host executable must name an existing regular file".into(),
            ));
        }
        let _ = self.proposal_host.binary_identity()?;
        let _ = self.proposal_host.resolved_model()?;
        Ok(())
    }

    pub(crate) fn resolve_paths(&mut self, base: &Path) {
        resolve(&mut self.proposal_host.executable, base);
        resolve(&mut self.proposal_host.state_root, base);
        resolve(&mut self.proposal_host.resolved_runtime_model, base);
    }
}

impl ProposalHostProcessConfigV1 {
    fn binary_identity(&self) -> Result<ProposalHostBinaryIdentity, ServerError> {
        binary_identity(&self.executable)
    }

    fn resolved_model(&self) -> Result<ResolvedRuntimeModel, ServerError> {
        let bytes = fs::read(&self.resolved_runtime_model)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let model: ResolvedRuntimeModel = cairn_codec::from_slice(&bytes)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        if model
            .canonical_bytes()
            .map_err(|error| ServerError::Configuration(error.to_string()))?
            != bytes
            || model.provider() != &self.selection.provider
            || model.wire_model() != &self.selection.model
            || model.deployment() != &self.selection.deployment
            || self.selection.adapter_version
                != AdapterVersion::new("native-protocol-v1")
                    .map_err(|error| ServerError::Configuration(error.to_string()))?
            || self.max_output_tokens > model.capabilities().max_output_tokens()
        {
            return Err(ServerError::Configuration(
                "resolved runtime model changed the configured Proposal Host policy".into(),
            ));
        }
        Ok(model)
    }

    fn runtime(&self, episode_id: EpisodeId) -> Result<ProposalHostRuntimeV1, ServerError> {
        let model = self.resolved_model()?;
        let bytes = model
            .canonical_bytes()
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        Ok(ProposalHostRuntimeV1::new(
            episode_id,
            self.binary_identity()?,
            ContentId::<AgentResolvedRuntimeModelArtifact>::derive(&bytes)
                .map_err(manager_error)?,
            self.selection.clone(),
            self.budget.clone(),
            self.max_output_tokens,
            self.task_limits,
        ))
    }
}

/// Expected state in which the manager has no new authority and should poll exact durable state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateWorkflowWaitingV1 {
    Worker {
        attempt_id: AttemptId,
        assignment_id: AssignmentId,
        phase: ScheduledAssignmentPhase,
    },
}

/// Proposal Host failure categories that forbid an implicit replacement episode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalHostProcessBlockedV1 {
    InvocationDrift,
    TimedOut,
    ExitFailure,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    InvalidTerminal,
}

/// Recoverable but authority-blocked state requiring policy or operator reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateWorkflowBlockedV1 {
    NoCandidate {
        placement_id: PlacementId,
    },
    ExpiredBeforeStart {
        attempt_id: AttemptId,
    },
    NativeBuildReconciliationRequired {
        attempt_id: AttemptId,
    },
    ExecutionNotStarted {
        attempt_id: AttemptId,
    },
    ExecutionAmbiguous {
        attempt_id: AttemptId,
    },
    ProposalHost {
        episode_id: EpisodeId,
        reason: ProposalHostProcessBlockedV1,
    },
}

/// Result of consuming at most one action selected by the durable Candidate workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateWorkflowManagerStatusV1 {
    Advanced,
    Waiting(CandidateWorkflowWaitingV1),
    Blocked(CandidateWorkflowBlockedV1),
    Terminal(CandidateWorkflowTerminalV1),
}

/// Runs the configured task until terminal or an explicit blocked state is reached.
pub(crate) async fn run_candidate_workflow_manager(
    server: ServerConfig,
    manager: CandidateWorkflowManagerConfigV1,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    fs::create_dir_all(&manager.proposal_host.state_root)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    loop {
        let status = drive_candidate_workflow_once(&server, &manager).await?;
        match status {
            CandidateWorkflowManagerStatusV1::Advanced => {}
            CandidateWorkflowManagerStatusV1::Waiting(_) => {
                sleep(Duration::from_millis(manager.poll_interval_ms.get())).await;
            }
            terminal => return Ok(terminal),
        }
    }
}

pub(crate) fn validate_candidate_workflow_exists(
    server: &ServerConfig,
    manager: &CandidateWorkflowManagerConfigV1,
) -> Result<(), ServerError> {
    let events = SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)?;
    let workflow = MigrationWorkflowV1::new(manager.task_id).map_err(manager_error)?;
    if recover_candidate_workflow(&events, &workflow).map_err(manager_error)?
        == CandidateWorkflowStateV1::NotFound
    {
        return Err(ServerError::MigrationWorkflow(
            "configured Candidate workflow does not exist".into(),
        ));
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the closed next-action match keeps every durable effect boundary visible"
)]
pub(crate) async fn drive_candidate_workflow_once(
    server: &ServerConfig,
    manager: &CandidateWorkflowManagerConfigV1,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    let mut events =
        SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)?;
    let workflow = MigrationWorkflowV1::new(manager.task_id).map_err(manager_error)?;
    let state = recover_candidate_workflow(&events, &workflow).map_err(manager_error)?;
    let action = state.next_action().map_err(manager_error)?;
    match action {
        CandidateWorkflowNextActionV1::None => Err(ServerError::MigrationWorkflow(
            "configured Candidate workflow does not exist".into(),
        )),
        CandidateWorkflowNextActionV1::PrepareNativeBuild {
            publication,
            image,
            profile,
        } => {
            let dispatch = prepare_candidate_native_build_dispatch(
                server,
                publication,
                JobId::new(),
                image,
                profile,
                schedule_ids(),
            )?;
            transition_or_recover(
                request_candidate_native_build(
                    &mut events,
                    &workflow,
                    dispatch,
                    &CommandId::new(),
                    observed_now()?,
                ),
                &mut events,
                &workflow,
                &state,
            )
        }
        CandidateWorkflowNextActionV1::ScheduleNativeBuild(dispatch) => {
            schedule_build(server, &workflow, &state, &dispatch, &mut events)
        }
        CandidateWorkflowNextActionV1::ReconcileNativeBuild(dispatch) => {
            reconcile_build(server, &workflow, &state, &dispatch)
        }
        CandidateWorkflowNextActionV1::PrepareCandidateEpisode { .. } => {
            let episode_id = EpisodeId::new();
            let runtime = manager.proposal_host.runtime(episode_id)?;
            let invocation = archive_proposal_host_runtime(server, &runtime)?;
            initialize_proposal_host_operation(&manager.proposal_host, &runtime)?;
            transition_or_recover(
                request_candidate_episode(
                    &mut events,
                    &workflow,
                    episode_id,
                    invocation,
                    &CommandId::new(),
                    observed_now()?,
                ),
                &mut events,
                &workflow,
                &state,
            )
        }
        CandidateWorkflowNextActionV1::RequestCandidateEpisode(workflow_request) => {
            let request = prepare_candidate_proposal_host_request(server, workflow_request)?;
            match run_proposal_host_process(&manager.proposal_host, &request).await {
                Ok(terminal) => transition_or_recover(
                    record_candidate_proposal_host_terminal(
                        &mut events,
                        &workflow,
                        &request,
                        &terminal,
                        &CommandId::new(),
                        observed_now()?,
                    ),
                    &mut events,
                    &workflow,
                    &state,
                ),
                Err(failure) => {
                    tracing::warn!(
                        target: "cairn.server.candidate-workflow",
                        event = "proposal_host_blocked",
                        task_id = %manager.task_id,
                        episode_id = %request.runtime().episode_id(),
                        reason = ?failure.reason,
                        diagnostic = %failure.diagnostic,
                        "Proposal Host operation requires reconciliation"
                    );
                    Ok(CandidateWorkflowManagerStatusV1::Blocked(
                        CandidateWorkflowBlockedV1::ProposalHost {
                            episode_id: request.runtime().episode_id(),
                            reason: failure.reason,
                        },
                    ))
                }
            }
        }
        CandidateWorkflowNextActionV1::Terminal(terminal) => {
            Ok(CandidateWorkflowManagerStatusV1::Terminal(terminal))
        }
    }
}

fn schedule_build(
    server: &ServerConfig,
    workflow: &MigrationWorkflowV1,
    state: &CandidateWorkflowStateV1,
    dispatch: &CandidateNativeBuildDispatchV1,
    events: &mut SqliteEventStore,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    match schedule_candidate_native_build(server, dispatch)? {
        ControllerSchedulingOutcome::NoCandidate { placement } => Ok(
            CandidateWorkflowManagerStatusV1::Blocked(CandidateWorkflowBlockedV1::NoCandidate {
                placement_id: placement.placement_id(),
            }),
        ),
        ControllerSchedulingOutcome::Scheduled { phase, .. } => match phase {
            ScheduledAssignmentPhase::OfferPending
            | ScheduledAssignmentPhase::Accepted
            | ScheduledAssignmentPhase::Running => Ok(CandidateWorkflowManagerStatusV1::Waiting(
                CandidateWorkflowWaitingV1::Worker {
                    attempt_id: dispatch.schedule().attempt_id,
                    assignment_id: dispatch.schedule().assignment_id,
                    phase,
                },
            )),
            ScheduledAssignmentPhase::ExpiredBeforeStart => {
                Ok(CandidateWorkflowManagerStatusV1::Blocked(
                    CandidateWorkflowBlockedV1::ExpiredBeforeStart {
                        attempt_id: dispatch.schedule().attempt_id,
                    },
                ))
            }
            ScheduledAssignmentPhase::ReconciliationRequired => transition_or_recover(
                require_candidate_native_build_reconciliation(
                    events,
                    workflow,
                    dispatch.clone(),
                    &CommandId::new(),
                    observed_now()?,
                ),
                events,
                workflow,
                state,
            ),
            ScheduledAssignmentPhase::Terminal => {
                reconcile_build(server, workflow, state, dispatch)
            }
        },
    }
}

fn reconcile_build(
    server: &ServerConfig,
    workflow: &MigrationWorkflowV1,
    state: &CandidateWorkflowStateV1,
    dispatch: &CandidateNativeBuildDispatchV1,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    let events = SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)?;
    let content = SqliteContentStore::open(
        &server.storage.content_database,
        &server.storage.content_directory,
    )
    .map_err(manager_error)?;
    let attempt_id = dispatch.schedule().attempt_id;
    match recover_execution_assignment(&events, &content, attempt_id, observed_now()?)
        .map_err(manager_error)?
    {
        ExecutionAssignmentState::Leased(_) => Ok(CandidateWorkflowManagerStatusV1::Waiting(
            CandidateWorkflowWaitingV1::Worker {
                attempt_id,
                assignment_id: dispatch.schedule().assignment_id,
                phase: ScheduledAssignmentPhase::OfferPending,
            },
        )),
        ExecutionAssignmentState::Accepted(_) => Ok(CandidateWorkflowManagerStatusV1::Waiting(
            CandidateWorkflowWaitingV1::Worker {
                attempt_id,
                assignment_id: dispatch.schedule().assignment_id,
                phase: ScheduledAssignmentPhase::Accepted,
            },
        )),
        ExecutionAssignmentState::Running { .. } => Ok(CandidateWorkflowManagerStatusV1::Waiting(
            CandidateWorkflowWaitingV1::Worker {
                attempt_id,
                assignment_id: dispatch.schedule().assignment_id,
                phase: ScheduledAssignmentPhase::Running,
            },
        )),
        ExecutionAssignmentState::ExpiredBeforeStart { .. } => {
            Ok(CandidateWorkflowManagerStatusV1::Blocked(
                CandidateWorkflowBlockedV1::ExpiredBeforeStart { attempt_id },
            ))
        }
        ExecutionAssignmentState::ReconciliationRequired { .. } => {
            Ok(CandidateWorkflowManagerStatusV1::Blocked(
                CandidateWorkflowBlockedV1::NativeBuildReconciliationRequired { attempt_id },
            ))
        }
        ExecutionAssignmentState::ExecutionTerminal { terminal, .. } => match terminal {
            AssignmentExecutionTerminal::Completed { receipt_id } => {
                fold_execution_receipt(server, workflow, state, dispatch, receipt_id)
            }
            AssignmentExecutionTerminal::NotStarted => {
                Ok(CandidateWorkflowManagerStatusV1::Blocked(
                    CandidateWorkflowBlockedV1::ExecutionNotStarted { attempt_id },
                ))
            }
            AssignmentExecutionTerminal::Ambiguous => {
                Ok(CandidateWorkflowManagerStatusV1::Blocked(
                    CandidateWorkflowBlockedV1::ExecutionAmbiguous { attempt_id },
                ))
            }
        },
        ExecutionAssignmentState::NotFound => Err(ServerError::MigrationWorkflow(
            "Candidate workflow dispatch names no durable execution assignment".into(),
        )),
    }
}

fn fold_execution_receipt(
    server: &ServerConfig,
    workflow: &MigrationWorkflowV1,
    state: &CandidateWorkflowStateV1,
    dispatch: &CandidateNativeBuildDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    let mut events =
        SqliteEventStore::open(&server.storage.event_database).map_err(manager_error)?;
    let mut content = SqliteContentStore::open(
        &server.storage.content_database,
        &server.storage.content_directory,
    )
    .map_err(manager_error)?;
    let receipt: ExecutionReceipt = load_canonical(&content, receipt_id)?;
    if receipt.outcome() == ExecutionOutcome::SubjectFailed {
        let stderr = load_content(&content, receipt.stderr_id())?;
        let evidence = load_content(&content, receipt.evidence_id())?;
        let (image, profile) = native_build_environment(state)?;
        let prepared = rematerialize_build(&content, dispatch, image, profile)?;
        let diagnostic = match prepared {
            PreparedNativeBuild::Revision(build) => {
                let prepared = prepare_candidate_native_build_diagnostic(
                    &build, receipt_id, &receipt, &stderr, &evidence,
                )
                .map_err(manager_error)?;
                prepared.archive(&mut content).map_err(manager_error)?;
                CandidateNativeDiagnosticV1::NativeFollowup(prepared.id())
            }
            PreparedNativeBuild::Followup(build) => {
                let prepared = prepare_candidate_native_repair_build_diagnostic(
                    &build, receipt_id, &receipt, &stderr, &evidence,
                )
                .map_err(manager_error)?;
                prepared.archive(&mut content).map_err(manager_error)?;
                CandidateNativeDiagnosticV1::NativeRepair(prepared.id())
            }
            PreparedNativeBuild::Repair(build) => {
                let prepared = prepare_candidate_native_repair_round_build_diagnostic(
                    &build, receipt_id, &receipt, &stderr, &evidence,
                )
                .map_err(manager_error)?;
                prepared.archive(&mut content).map_err(manager_error)?;
                CandidateNativeDiagnosticV1::NativeRepair(prepared.id())
            }
        };
        transition_or_recover(
            record_candidate_native_subject_failure(
                &mut events,
                workflow,
                receipt_id,
                &receipt,
                diagnostic,
                &CommandId::new(),
                observed_now()?,
            ),
            &mut events,
            workflow,
            state,
        )
    } else {
        transition_or_recover(
            record_candidate_native_terminal(
                &mut events,
                workflow,
                receipt_id,
                &receipt,
                &CommandId::new(),
                observed_now()?,
            ),
            &mut events,
            workflow,
            state,
        )
    }
}

fn native_build_environment(
    state: &CandidateWorkflowStateV1,
) -> Result<
    (
        cairn_execution::DockerImageId,
        cairn_migration::CandidateBuildEnvironmentProfileV1,
    ),
    ServerError,
> {
    match state {
        CandidateWorkflowStateV1::NativeBuildRequested { image, profile, .. }
        | CandidateWorkflowStateV1::NativeBuildReconciliationRequired { image, profile, .. } => {
            Ok((image.clone(), *profile))
        }
        _ => Err(ServerError::MigrationWorkflow(
            "native-build receipt is not bound to an active workflow dispatch".into(),
        )),
    }
}

enum PreparedNativeBuild {
    Revision(PreparedCandidateNativeRevisionBuildJob),
    Followup(PreparedCandidateNativeFollowupBuildJob),
    Repair(PreparedCandidateNativeRepairBuildJob),
}

fn rematerialize_build(
    content: &SqliteContentStore,
    dispatch: &CandidateNativeBuildDispatchV1,
    image: cairn_execution::DockerImageId,
    profile: cairn_migration::CandidateBuildEnvironmentProfileV1,
) -> Result<PreparedNativeBuild, ServerError> {
    let prepared = match dispatch.publication() {
        CandidateNativePublicationV1::Revision(id) => PreparedNativeBuild::Revision(
            prepare_candidate_native_revision_build_job(
                dispatch.job_id(),
                &load_content::<CollectionCandidateRevisionArtifact>(content, id)?,
                id,
                image,
                profile,
            )
            .map_err(manager_error)?,
        ),
        CandidateNativePublicationV1::NativeFollowup(id) => PreparedNativeBuild::Followup(
            prepare_candidate_native_followup_build_job(
                dispatch.job_id(),
                &load_content::<CollectionCandidateNativeFollowupRevisionArtifact>(content, id)?,
                id,
                image,
                profile,
            )
            .map_err(manager_error)?,
        ),
        CandidateNativePublicationV1::NativeRepair(id) => PreparedNativeBuild::Repair(
            prepare_candidate_native_repair_build_job(
                dispatch.job_id(),
                &load_content::<CollectionCandidateNativeRepairRevisionArtifact>(content, id)?,
                id,
                image,
                profile,
            )
            .map_err(manager_error)?,
        ),
    };
    let binding = match &prepared {
        PreparedNativeBuild::Revision(build) => (
            build.input_bundle_id(),
            build.environment_id(),
            build.contract_id(),
        ),
        PreparedNativeBuild::Followup(build) => (
            build.input_bundle_id(),
            build.environment_id(),
            build.contract_id(),
        ),
        PreparedNativeBuild::Repair(build) => (
            build.input_bundle_id(),
            build.environment_id(),
            build.contract_id(),
        ),
    };
    if binding
        != (
            dispatch.input_bundle(),
            dispatch.environment(),
            dispatch.contract(),
        )
    {
        return Err(ServerError::MigrationWorkflow(
            "native-build receipt material changed the durable workflow dispatch".into(),
        ));
    }
    Ok(prepared)
}

struct HostProcessFailure {
    reason: ProposalHostProcessBlockedV1,
    diagnostic: String,
}

#[allow(
    clippy::too_many_lines,
    reason = "the bounded child lifecycle keeps spawn, drain, timeout, and terminal validation visible"
)]
async fn run_proposal_host_process(
    config: &ProposalHostProcessConfigV1,
    request: &ProposalHostRequestV1,
) -> Result<ProposalHostTerminalV1, HostProcessFailure> {
    validate_proposal_host_operation(config, request)?;
    let request_bytes = cairn_codec::to_vec(request).map_err(|error| HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::InvalidTerminal,
        diagnostic: error.to_string(),
    })?;
    if request_bytes.len() > HOST_REQUEST_BYTE_LIMIT {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::InvalidTerminal,
            diagnostic: "Proposal Host request exceeds the current-V1 ingress limit".into(),
        });
    }
    let mut child = Command::new(&config.executable)
        .arg(&config.state_root)
        .arg(&config.resolved_runtime_model)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::ExitFailure,
            diagnostic: error.to_string(),
        })?;
    let mut stdin = child.stdin.take().ok_or_else(|| HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::ExitFailure,
        diagnostic: "Proposal Host stdin pipe is absent".into(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::ExitFailure,
        diagnostic: "Proposal Host stdout pipe is absent".into(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::ExitFailure,
        diagnostic: "Proposal Host stderr pipe is absent".into(),
    })?;
    let stdout_limit = config.stdout_byte_limit.get();
    let stderr_limit = config.stderr_byte_limit.get();
    let writer = tokio::spawn(async move {
        stdin.write_all(&request_bytes).await?;
        stdin.shutdown().await
    });
    let stdout_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take(stdout_limit + 1)
            .read_to_end(&mut bytes)
            .await?;
        Ok::<_, std::io::Error>(bytes)
    });
    let stderr_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take(stderr_limit + 1)
            .read_to_end(&mut bytes)
            .await?;
        Ok::<_, std::io::Error>(bytes)
    });
    let status = if let Ok(result) = timeout(
        Duration::from_millis(config.process_timeout_ms.get()),
        child.wait(),
    )
    .await
    {
        result.map_err(|error| HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::ExitFailure,
            diagnostic: error.to_string(),
        })?
    } else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::TimedOut,
            diagnostic: "Proposal Host process exceeded its exact wall-clock limit".into(),
        });
    };
    writer
        .await
        .map_err(|error| join_failure(&error))?
        .map_err(|error| io_failure(&error))?;
    let stdout = stdout_reader
        .await
        .map_err(|error| join_failure(&error))?
        .map_err(|error| io_failure(&error))?;
    let stderr = stderr_reader
        .await
        .map_err(|error| join_failure(&error))?
        .map_err(|error| io_failure(&error))?;
    if stdout.len() > usize::try_from(stdout_limit).unwrap_or(usize::MAX) {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::StdoutLimitExceeded,
            diagnostic: "Proposal Host stdout exceeded its configured byte limit".into(),
        });
    }
    if stderr.len() > usize::try_from(stderr_limit).unwrap_or(usize::MAX) {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::StderrLimitExceeded,
            diagnostic: "Proposal Host stderr exceeded its configured byte limit".into(),
        });
    }
    if !status.success() {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::ExitFailure,
            diagnostic: String::from_utf8_lossy(&stderr).into_owned(),
        });
    }
    let terminal: ProposalHostTerminalV1 =
        cairn_codec::from_slice(&stdout).map_err(|error| HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::InvalidTerminal,
            diagnostic: error.to_string(),
        })?;
    if cairn_codec::to_vec(&terminal).ok().as_deref() != Some(stdout.as_slice())
        || terminal.validate_against(request).is_err()
    {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::InvalidTerminal,
            diagnostic: "Proposal Host returned a noncanonical or cross-bound terminal".into(),
        });
    }
    Ok(terminal)
}

fn initialize_proposal_host_operation(
    config: &ProposalHostProcessConfigV1,
    runtime: &ProposalHostRuntimeV1,
) -> Result<(), ServerError> {
    fs::create_dir_all(&config.state_root)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let state = config.state_root.join(runtime.episode_id().to_string());
    fs::create_dir(&state).map_err(|error| ServerError::MigrationWorkflow(error.to_string()))?;
    let bytes = cairn_codec::to_vec(runtime).map_err(manager_error)?;
    let marker = state.join("invocation.v1.json");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(marker)
        .map_err(|error| ServerError::MigrationWorkflow(error.to_string()))?;
    file.write_all(&bytes).map_err(manager_error)?;
    file.sync_all().map_err(manager_error)
}

fn validate_proposal_host_operation(
    config: &ProposalHostProcessConfigV1,
    request: &ProposalHostRequestV1,
) -> Result<(), HostProcessFailure> {
    let drift = |diagnostic: String| HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::InvocationDrift,
        diagnostic,
    };
    let runtime_bytes =
        cairn_codec::to_vec(request.runtime()).map_err(|error| drift(error.to_string()))?;
    let marker = config
        .state_root
        .join(request.runtime().episode_id().to_string())
        .join("invocation.v1.json");
    if fs::read(marker).map_err(|error| drift(error.to_string()))? != runtime_bytes
        || config
            .binary_identity()
            .map_err(|error| drift(error.to_string()))?
            != *request.runtime().binary_identity()
    {
        return Err(drift(
            "Proposal Host process state or binary changed the durable invocation".into(),
        ));
    }
    let model = config
        .resolved_model()
        .map_err(|error| drift(error.to_string()))?;
    let model_bytes = model
        .canonical_bytes()
        .map_err(|error| drift(error.to_string()))?;
    if ContentId::<AgentResolvedRuntimeModelArtifact>::derive(&model_bytes)
        .map_err(|error| drift(error.to_string()))?
        != request.runtime().model_configuration()
    {
        return Err(drift(
            "resolved runtime model changed the durable Host invocation".into(),
        ));
    }
    Ok(())
}

fn schedule_ids() -> CandidateNativeBuildScheduleV1 {
    CandidateNativeBuildScheduleV1 {
        attempt_id: AttemptId::new(),
        placement_id: PlacementId::new(),
        reservation_id: ReservationId::new(),
        assignment_id: AssignmentId::new(),
        lease_id: LeaseId::new(),
        offer_message_id: ControlMessageId::new(),
        start_message_id: ControlMessageId::new(),
        authorize_attempt_command: CommandId::new(),
        reserve_placement_command: CommandId::new(),
        grant_assignment_command: CommandId::new(),
        enqueue_offer_command: CommandId::new(),
    }
}

fn load_content<T: ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<Vec<u8>, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(manager_error)?;
    Ok(bytes)
}

fn load_canonical<T, V>(content: &SqliteContentStore, id: ContentId<T>) -> Result<V, ServerError>
where
    T: ContentType,
    V: serde::de::DeserializeOwned + Serialize,
{
    let bytes = load_content(content, id)?;
    let value: V = cairn_codec::from_slice(&bytes).map_err(manager_error)?;
    if cairn_codec::to_vec(&value).map_err(manager_error)? != bytes
        || ContentId::<T>::derive(&bytes).map_err(manager_error)? != id
    {
        return Err(ServerError::MigrationWorkflow(
            "workflow material changed its canonical typed identity".into(),
        ));
    }
    Ok(value)
}

fn binary_identity(path: &Path) -> Result<ProposalHostBinaryIdentity, ServerError> {
    let mut file =
        fs::File::open(path).map_err(|error| ServerError::Configuration(error.to_string()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    ProposalHostBinaryIdentity::new(format!("sha256:{:x}", digest.finalize()))
        .map_err(manager_error)
}

fn resolve(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
}

fn join_failure(error: &tokio::task::JoinError) -> HostProcessFailure {
    HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::ExitFailure,
        diagnostic: error.to_string(),
    }
}

fn io_failure(error: &std::io::Error) -> HostProcessFailure {
    HostProcessFailure {
        reason: ProposalHostProcessBlockedV1::ExitFailure,
        diagnostic: error.to_string(),
    }
}

fn manager_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}

fn transition_or_recover<T, E: std::fmt::Display>(
    result: Result<T, E>,
    events: &mut SqliteEventStore,
    workflow: &MigrationWorkflowV1,
    prior: &CandidateWorkflowStateV1,
) -> Result<CandidateWorkflowManagerStatusV1, ServerError> {
    match result {
        Ok(_) => Ok(CandidateWorkflowManagerStatusV1::Advanced),
        Err(error) => {
            let recovered = recover_candidate_workflow(events, workflow).map_err(manager_error)?;
            if &recovered == prior {
                Err(manager_error(error))
            } else {
                Ok(CandidateWorkflowManagerStatusV1::Advanced)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        sync::{Arc, Barrier},
    };

    use cairn_agent::{
        DeploymentName, EpisodeStepLimit, EpisodeToolOperationLimit, ModelName, ProviderName,
    };
    use cairn_execution::{
        DOCKER_BACKEND, DockerImageId, ExecutionBackend, ExecutionEvidenceArtifact,
        ExecutionObservation, ExecutionReceiptArtifact, ExecutionStderrArtifact,
        ExecutionStdoutArtifact, ResolvedProgramIdentity, TrustedExecutionEvidence,
    };
    use cairn_migration::{
        AdmittedCollectionOracleClaimArtifact, CandidateBuildEnvironmentProfileV1,
        CandidateRevisionRoundLimit, CandidateWorkflowAuthorityV1,
        CollectionCandidateBuildDiagnosticArtifact, CollectionCandidateProposalArtifact,
        CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
        CollectionCandidateSearchAuthorityInput, CollectionOracleAdmissionPublicOutcomeArtifact,
        CollectionOracleClaimDomainV1, CollectionOracleClaimStrengthV1,
        IntentRecoveryInputArtifact, MigrationIntentContractArtifact, SirCallerClaimId,
        open_candidate_workflow, prepare_collection_candidate_search_input,
    };
    use cairn_protocol::{ContentType, ObservedAtUnixMillis};
    use serde_json::{Value, json};

    use super::*;

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn server(root: &Path) -> ServerConfig {
        let mut value: Value =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("controller example");
        value["storage"]["event_database"] =
            json!(root.join("events.db").to_string_lossy().into_owned());
        value["storage"]["content_database"] =
            json!(root.join("content.db").to_string_lossy().into_owned());
        value["storage"]["content_directory"] =
            json!(root.join("cas").to_string_lossy().into_owned());
        serde_json::from_value(value).expect("server configuration")
    }

    fn manager(task_id: TaskId, root: &Path) -> CandidateWorkflowManagerConfigV1 {
        CandidateWorkflowManagerConfigV1 {
            schema_version: SchemaVersion::new(1).expect("schema"),
            task_id,
            proposal_host: ProposalHostProcessConfigV1 {
                executable: PathBuf::from("/bin/false"),
                state_root: root.join("host"),
                resolved_runtime_model: root.join("unused-model.json"),
                selection: ModelSelection {
                    provider: ProviderName::new("recorded").expect("provider"),
                    model: ModelName::new("recorded-model").expect("model"),
                    deployment: DeploymentName::new("isolated").expect("deployment"),
                    adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
                },
                budget: EpisodeBudget {
                    step_limit: Some(EpisodeStepLimit::new(4).expect("steps")),
                    tool_operation_limit: Some(EpisodeToolOperationLimit::new(8)),
                    provider_token_limit: None,
                    deadline_unix_ms: None,
                    external_meter_limits: None,
                },
                max_output_tokens: ModelOutputTokenLimit::new(4_096).expect("output limit"),
                task_limits: SirTaskLimits::default(),
                process_timeout_ms: ProposalHostProcessTimeoutMillis::new(1_000).expect("timeout"),
                stdout_byte_limit: ProposalHostStdoutByteLimit::new(1024 * 1024).expect("stdout"),
                stderr_byte_limit: ProposalHostStderrByteLimit::new(64 * 1024).expect("stderr"),
            },
            poll_interval_ms: CandidateWorkflowPollIntervalMillis::new(10).expect("poll"),
        }
    }

    fn open_workflow(
        server: &ServerConfig,
        task_id: TaskId,
        source_path: &str,
        source: &str,
        label: &[u8],
    ) -> MigrationWorkflowV1 {
        let search = prepare_collection_candidate_search_input(
            &CollectionCandidateSearchAuthorityInput::new(
                task_id,
                id::<IntentRecoveryInputArtifact>(label),
                id::<MigrationIntentContractArtifact>(b"intent"),
                id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"oracle outcome"),
                id::<AdmittedCollectionOracleClaimArtifact>(b"oracle claim"),
                SirCallerClaimId::new("selected-contract").expect("claim"),
                CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
                CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
            ),
        )
        .expect("search input");
        let revision: CollectionCandidateRevisionV1 = cairn_codec::from_slice(
            &cairn_codec::to_vec(&json!({
                "schema_version":1,
                "search_input":search.id(),
                "parent_proposal":id::<CollectionCandidateProposalArtifact>(label),
                "build_diagnostic":id::<CollectionCandidateBuildDiagnosticArtifact>(label),
                "episode_id":EpisodeId::new(),
                "model_configuration":id::<AgentResolvedRuntimeModelArtifact>(label),
                "submission":{
                    "schema_version":1,
                    "files":[{"path":source_path,"source":source}],
                    "primary_source":source_path,
                    "explanation":"Materially distinct recorded Candidate revision."
                }
            }))
            .expect("revision encoding"),
        )
        .expect("revision");
        let revision_bytes = cairn_codec::to_vec(&revision).expect("revision bytes");
        let revision_id = revision.identity().expect("revision identity");
        let mut content = SqliteContentStore::open(
            &server.storage.content_database,
            &server.storage.content_directory,
        )
        .expect("content");
        assert_eq!(
            content
                .put::<CollectionCandidateRevisionArtifact>(&mut Cursor::new(revision_bytes))
                .expect("archive revision")
                .content_id,
            revision_id
        );
        let mut events =
            SqliteEventStore::open(&server.storage.event_database).expect("event store");
        let workflow = MigrationWorkflowV1::new(task_id).expect("workflow");
        open_candidate_workflow(
            &mut events,
            &workflow,
            CandidateWorkflowAuthorityV1::from_search_input(search.id(), search.input())
                .expect("authority"),
            search.input(),
            &revision,
            revision_id,
            DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            CandidateRevisionRoundLimit::new(2).expect("revision limit"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(1),
        )
        .expect("open workflow");
        workflow
    }

    #[test]
    fn process_quantities_reject_zero_and_values_outside_current_v1_bounds() {
        assert!(serde_json::from_str::<ProposalHostProcessTimeoutMillis>("0").is_err());
        assert!(serde_json::from_str::<ProposalHostProcessTimeoutMillis>("86400001").is_err());
        assert!(serde_json::from_str::<ProposalHostStdoutByteLimit>("2097153").is_err());
        assert!(serde_json::from_str::<ProposalHostStderrByteLimit>("0").is_err());
        assert!(serde_json::from_str::<CandidateWorkflowPollIntervalMillis>("60001").is_err());
    }

    #[test]
    fn manager_config_is_strict_current_v1_before_process_or_model_access() {
        let temporary = tempfile::tempdir().expect("temporary root");
        let mut non_v1 = manager(TaskId::new(), temporary.path());
        non_v1.schema_version = SchemaVersion::new(99).expect("schema");
        assert!(matches!(
            non_v1.validate(),
            Err(ServerError::Configuration(message))
                if message == "only Candidate workflow manager schema_version 1 is supported"
        ));

        let mut unknown = serde_json::to_value(manager(TaskId::new(), temporary.path()))
            .expect("manager config value");
        unknown["unexpected_hint"] = json!("must not be accepted");
        assert!(serde_json::from_value::<CandidateWorkflowManagerConfigV1>(unknown).is_err());
    }

    #[test]
    fn concurrent_managers_accept_one_durable_dispatch_and_never_replace_its_ids() {
        let temporary = tempfile::tempdir().expect("temporary root");
        let server = server(temporary.path());
        let task_id = TaskId::new();
        let workflow = open_workflow(
            &server,
            task_id,
            "src/concurrent.asc",
            "#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void concurrent() {}\n",
            b"concurrent-manager",
        );
        let manager = manager(task_id, temporary.path());
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let server = server.clone();
                let manager = manager.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    tokio::runtime::Runtime::new()
                        .expect("runtime")
                        .block_on(drive_candidate_workflow_once(&server, &manager))
                })
            })
            .collect();
        for handle in handles {
            let status = handle
                .join()
                .expect("manager thread")
                .expect("manager result");
            assert!(matches!(
                status,
                CandidateWorkflowManagerStatusV1::Advanced
                    | CandidateWorkflowManagerStatusV1::Blocked(
                        CandidateWorkflowBlockedV1::NoCandidate { .. }
                    )
            ));
        }
        let events = SqliteEventStore::open(&server.storage.event_database).expect("event store");
        let CandidateWorkflowStateV1::NativeBuildRequested { dispatch, .. } =
            recover_candidate_workflow(&events, &workflow).expect("recover winner")
        else {
            panic!("concurrent managers did not retain one durable dispatch");
        };
        let expected_placement = dispatch.schedule().placement_id;
        let status = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(drive_candidate_workflow_once(&server, &manager))
            .expect("recover scheduling outcome");
        assert_eq!(
            status,
            CandidateWorkflowManagerStatusV1::Blocked(CandidateWorkflowBlockedV1::NoCandidate {
                placement_id: expected_placement,
            })
        );
        let events = SqliteEventStore::open(&server.storage.event_database).expect("event store");
        let CandidateWorkflowStateV1::NativeBuildRequested {
            dispatch: recovered,
            ..
        } = recover_candidate_workflow(&events, &workflow).expect("recover exact dispatch")
        else {
            panic!("NoCandidate changed the durable dispatch");
        };
        assert_eq!(recovered, dispatch);
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "the integration control keeps two tasks, exact scheduling replay, and receipt folding together"
    )]
    async fn two_materially_different_tasks_share_one_manager_path_and_freeze_no_candidate_ids() {
        let temporary = tempfile::tempdir().expect("temporary root");
        let server = server(temporary.path());
        let tasks = [
            (
                "src/select.asc",
                "#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void select() {}\n",
                b"select-layout".as_slice(),
            ),
            (
                "src/window.asc",
                "#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void window() {}\n",
                b"stream-window".as_slice(),
            ),
        ];
        for (source_path, source, label) in tasks {
            let task_id = TaskId::new();
            let workflow = open_workflow(&server, task_id, source_path, source, label);
            let manager = manager(task_id, temporary.path());
            assert_eq!(
                drive_candidate_workflow_once(&server, &manager)
                    .await
                    .expect("prepare native build"),
                CandidateWorkflowManagerStatusV1::Advanced
            );
            let events =
                SqliteEventStore::open(&server.storage.event_database).expect("event store");
            let CandidateWorkflowStateV1::NativeBuildRequested { dispatch, .. } =
                recover_candidate_workflow(&events, &workflow).expect("recover requested build")
            else {
                panic!("manager did not durably request the native build");
            };
            let expected_placement = dispatch.schedule().placement_id;
            assert_eq!(
                drive_candidate_workflow_once(&server, &manager)
                    .await
                    .expect("schedule without worker"),
                CandidateWorkflowManagerStatusV1::Blocked(
                    CandidateWorkflowBlockedV1::NoCandidate {
                        placement_id: expected_placement,
                    }
                )
            );
            assert_eq!(
                drive_candidate_workflow_once(&server, &manager)
                    .await
                    .expect("exact no-candidate recovery"),
                CandidateWorkflowManagerStatusV1::Blocked(
                    CandidateWorkflowBlockedV1::NoCandidate {
                        placement_id: expected_placement,
                    }
                )
            );
            let events =
                SqliteEventStore::open(&server.storage.event_database).expect("event store");
            let CandidateWorkflowStateV1::NativeBuildRequested {
                dispatch: recovered,
                ..
            } = recover_candidate_workflow(&events, &workflow).expect("recover frozen dispatch")
            else {
                panic!("NoCandidate changed the workflow state");
            };
            assert_eq!(recovered, dispatch);

            let stderr = b"candidate_primary.asc:4: error: recorded native diagnostic\n".to_vec();
            let evidence = cairn_codec::to_vec(
                &TrustedExecutionEvidence::new(
                    ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
                    dispatch.environment(),
                    ResolvedProgramIdentity::new("sha256:recorded-native-gate")
                        .expect("program identity"),
                    vec![
                        ExecutionObservation::new("docker:accelerator:none").expect("observation"),
                    ],
                )
                .expect("evidence"),
            )
            .expect("evidence bytes");
            let stderr_id =
                ContentId::<ExecutionStderrArtifact>::derive(&stderr).expect("stderr identity");
            let evidence_id = ContentId::<ExecutionEvidenceArtifact>::derive(&evidence)
                .expect("evidence identity");
            let receipt_bytes = cairn_codec::to_vec(&json!({
                "schema_version":1,
                "job_id":dispatch.job_id(),
                "attempt_id":dispatch.schedule().attempt_id,
                "contract_id":dispatch.contract(),
                "outcome":"subject-failed",
                "exit_code":1,
                "elapsed_ms":12,
                "stdout_id":id::<ExecutionStdoutArtifact>(b"stdout"),
                "stderr_id":stderr_id,
                "evidence_id":evidence_id,
                "outputs":[]
            }))
            .expect("receipt bytes");
            let receipt_id = ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes)
                .expect("receipt identity");
            let mut content = SqliteContentStore::open(
                &server.storage.content_database,
                &server.storage.content_directory,
            )
            .expect("content");
            assert_eq!(
                content
                    .put::<ExecutionStderrArtifact>(&mut Cursor::new(&stderr))
                    .expect("archive stderr")
                    .content_id,
                stderr_id
            );
            assert_eq!(
                content
                    .put::<ExecutionEvidenceArtifact>(&mut Cursor::new(&evidence))
                    .expect("archive evidence")
                    .content_id,
                evidence_id
            );
            assert_eq!(
                content
                    .put::<ExecutionReceiptArtifact>(&mut Cursor::new(&receipt_bytes))
                    .expect("archive receipt")
                    .content_id,
                receipt_id
            );
            drop(content);
            let state = CandidateWorkflowStateV1::NativeBuildRequested {
                authority: match recover_candidate_workflow(
                    &SqliteEventStore::open(&server.storage.event_database).expect("event store"),
                    &workflow,
                )
                .expect("recover active build")
                {
                    CandidateWorkflowStateV1::NativeBuildRequested { authority, .. } => authority,
                    _ => panic!("workflow lost active build authority"),
                },
                dispatch: dispatch.clone(),
                image: DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
                profile: CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
                revision_limit: CandidateRevisionRoundLimit::new(2).expect("revision limit"),
                revisions_used: cairn_migration::CandidateRevisionRoundCount::zero(),
            };
            assert_eq!(
                fold_execution_receipt(&server, &workflow, &state, &dispatch, receipt_id)
                    .expect("fold subject failure"),
                CandidateWorkflowManagerStatusV1::Advanced
            );
            let events =
                SqliteEventStore::open(&server.storage.event_database).expect("event store");
            assert!(matches!(
                recover_candidate_workflow(&events, &workflow).expect("recover diagnostic"),
                CandidateWorkflowStateV1::NativeBuildSubjectFailed {
                    diagnostic: CandidateNativeDiagnosticV1::NativeFollowup(_),
                    ..
                }
            ));
        }
    }
}
