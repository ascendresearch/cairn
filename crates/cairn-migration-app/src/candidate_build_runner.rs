//! Product-owned Candidate build execution against an ordinary managed Worker.
//!
//! The build recipe is Controller configuration, never Candidate output: the Candidate contributes
//! only the files under `source/`, while `bin/run` is supplied by this deployment. A Candidate
//! therefore cannot select its own build path, which is the control that separates a native build
//! from a host fallback wearing its name.

use std::{io::Cursor, path::PathBuf, time::Duration};

use cairn_execution::{
    CapabilityRequirement, CapturePolicy, DiagnosticByteLimit, DockerImageId, EvidenceByteLimit,
    ExecutionJob, ExecutionJobState, ExecutionOutcome, ExecutionReceipt, ExecutionTimeoutMillis,
    NetworkPolicy, OutputByteLimit, ReservationReleaseReason, WorkerPoolName,
    recover_execution_job,
};
use cairn_migration::{
    CandidateBuildPlanV1, CandidateBuildRequestArtifact, CandidateProposalArtifact,
    CandidateProposalV1, prepare_generic_candidate_build_job,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentId, ControlMessageId, JobId, LeaseId, PlacementId,
    ReservationId,
};
use cairn_record::ContentStore;
use cairn_server::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome, ServerConfig,
    release_execution_reservation, schedule_execution_contract,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Deserialize;
use thiserror::Error;

const MIN_SCHEDULING_RETRY_INTERVAL: Duration = Duration::from_millis(25);

/// Deployment-owned Candidate build configuration.
///
/// `runner_path` names a file on the Controller host. Its bytes become `bin/run` inside the
/// sandbox, so the build recipe travels with the job contract and is covered by its identity.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBuildWorkerConfigV1 {
    schema_version: u16,
    image: DockerImageId,
    runner_path: PathBuf,
    worker_pools: Vec<WorkerPoolName>,
    #[serde(default)]
    capabilities: Vec<CapabilityRequirement>,
    execution_timeout: ExecutionTimeoutMillis,
    poll_interval_ms: u64,
    completion_timeout_ms: u64,
}

impl CandidateBuildWorkerConfigV1 {
    /// Returns the deployment-side path to the build recipe the Controller supplies.
    #[must_use]
    pub fn runner_path(&self) -> &std::path::Path {
        &self.runner_path
    }

    fn validate(&self) -> Result<(), CandidateBuildRunnerError> {
        if self.schema_version != 1
            || self.poll_interval_ms == 0
            || self.completion_timeout_ms == 0
            || self.worker_pools.is_empty()
        {
            return Err(CandidateBuildRunnerError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Exact typed observation of one Candidate build attempt on a managed Worker.
///
/// This is an observation, never a verdict. A successful build says the exact artifact compiled in
/// the exact environment; it says nothing about the Candidate's semantics.
#[derive(Clone, Debug)]
pub struct CandidateBuildObservationV1 {
    job_id: JobId,
    attempt_id: AttemptId,
    request: ContentId<CandidateBuildRequestArtifact>,
    receipt_id: ContentId<cairn_execution::ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
}

impl CandidateBuildObservationV1 {
    /// Returns the exact Worker job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the exact Worker attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the frozen build request identity.
    #[must_use]
    pub const fn request(&self) -> ContentId<CandidateBuildRequestArtifact> {
        self.request
    }

    /// Returns the trusted receipt identity.
    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<cairn_execution::ExecutionReceiptArtifact> {
        self.receipt_id
    }

    /// Returns the trusted execution receipt.
    #[must_use]
    pub const fn receipt(&self) -> &ExecutionReceipt {
        &self.receipt
    }

    /// Reports whether the exact artifact compiled under the product-owned build plan.
    #[must_use]
    pub fn compiled(&self) -> bool {
        self.receipt.outcome() == ExecutionOutcome::Succeeded && self.receipt.exit_code() == Some(0)
    }
}

/// Worker-backed Candidate build runner.
pub struct CandidateBuildRunnerV1 {
    server: ServerConfig,
    config: CandidateBuildWorkerConfigV1,
    plan: CandidateBuildPlanV1,
}

impl CandidateBuildRunnerV1 {
    /// Builds a Worker-backed Candidate build runner from Controller configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is invalid, scheduling is disabled, the runner file
    /// cannot be read, or the resulting build plan fails its own construction invariants.
    pub fn new(
        server: ServerConfig,
        config: CandidateBuildWorkerConfigV1,
    ) -> Result<Self, CandidateBuildRunnerError> {
        config.validate()?;
        if server.scheduler.is_none() {
            return Err(CandidateBuildRunnerError::InvalidConfiguration);
        }
        let runner = std::fs::read(&config.runner_path)
            .map_err(|error| CandidateBuildRunnerError::Runner(error.to_string()))?;
        let plan = CandidateBuildPlanV1::new(
            config.image.clone(),
            runner,
            config.worker_pools.clone(),
            config.capabilities.clone(),
            config.execution_timeout,
            CapturePolicy::new(
                OutputByteLimit::new(64 * 1024).map_err(domain)?,
                OutputByteLimit::new(64 * 1024).map_err(domain)?,
                DiagnosticByteLimit::new(16 * 1024).map_err(domain)?,
                EvidenceByteLimit::new(64 * 1024).map_err(domain)?,
                Vec::new(),
            )
            .map_err(domain)?,
            NetworkPolicy::Disabled,
        )
        .map_err(domain)?;
        Ok(Self {
            server,
            config,
            plan,
        })
    }

    /// Materializes the exact Candidate build job and archives every bound artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when the proposal cannot be encoded, the job cannot be composed under the
    /// product-owned plan, or the content store rejects an artifact.
    pub fn authorize(
        &self,
        proposal: &CandidateProposalV1,
    ) -> Result<AuthorizedCandidateBuildV1, CandidateBuildRunnerError> {
        let proposal_bytes = cairn_codec::to_vec(proposal).map_err(domain)?;
        let proposal_id =
            ContentId::<CandidateProposalArtifact>::derive(&proposal_bytes).map_err(domain)?;
        let job_id = JobId::new();
        let prepared = prepare_generic_candidate_build_job(
            job_id,
            &proposal_bytes,
            proposal_id,
            self.plan.clone(),
        )
        .map_err(domain)?;
        let contract = prepared.contract().clone();
        let request_bytes = cairn_codec::to_vec(prepared.request()).map_err(domain)?;
        let mut content = self.content()?;
        prepared.archive(&mut content).map_err(domain)?;
        let request = content
            .put::<CandidateBuildRequestArtifact>(&mut Cursor::new(request_bytes))
            .map_err(domain)?
            .content_id;
        Ok(AuthorizedCandidateBuildV1 {
            proposal: proposal_id,
            job_id,
            request,
            contract,
        })
    }

    /// Schedules the authorized build onto an ordinary Worker and folds back its trusted receipt.
    ///
    /// # Errors
    ///
    /// Returns an error when no Worker accepts the contract within the configured window, the
    /// Worker rejects the job, or the receipt cannot be recovered before the deadline.
    pub async fn observe(
        &self,
        authorized: AuthorizedCandidateBuildV1,
    ) -> Result<CandidateBuildObservationV1, CandidateBuildRunnerError> {
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.config.completion_timeout_ms);
        let scheduling_retry_interval =
            Duration::from_millis(self.config.poll_interval_ms).max(MIN_SCHEDULING_RETRY_INTERVAL);
        let (attempt_id, reservation_id) = loop {
            let attempt_id = AttemptId::new();
            let reservation_id = ReservationId::new();
            let ids = ControllerScheduleIds {
                attempt_id,
                placement_id: PlacementId::new(),
                reservation_id,
                assignment_id: AssignmentId::new(),
                lease_id: LeaseId::new(),
                offer_message_id: ControlMessageId::new(),
                start_message_id: ControlMessageId::new(),
                commands: ControllerScheduleCommandIds {
                    authorize_attempt: CommandId::new(),
                    reserve_placement: CommandId::new(),
                    grant_assignment: CommandId::new(),
                    enqueue_offer: CommandId::new(),
                },
            };
            match tokio::task::block_in_place(|| {
                schedule_execution_contract(&self.server, &authorized.contract, ids)
            })
            .map_err(domain)?
            {
                ControllerSchedulingOutcome::Scheduled { .. } => {
                    break (attempt_id, reservation_id);
                }
                ControllerSchedulingOutcome::NoCandidate { .. } => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(CandidateBuildRunnerError::NoWorker);
                    }
                    tokio::time::sleep(scheduling_retry_interval).await;
                }
            }
        };
        let job = ExecutionJob::new(authorized.job_id).map_err(domain)?;
        loop {
            let events = SqliteEventStore::open(self.server.event_database()).map_err(domain)?;
            let content = self.content()?;
            match recover_execution_job(&events, &content, &job).map_err(domain)? {
                ExecutionJobState::Completed {
                    receipt_id,
                    receipt,
                } => {
                    let release_reason = release_execution_reservation(
                        &self.server,
                        reservation_id,
                        &CommandId::new(),
                    )
                    .map_err(domain)?;
                    if release_reason != ReservationReleaseReason::ExecutionTerminal {
                        return Err(CandidateBuildRunnerError::Binding);
                    }
                    return Ok(CandidateBuildObservationV1 {
                        job_id: authorized.job_id,
                        attempt_id,
                        request: authorized.request,
                        receipt_id,
                        receipt,
                    });
                }
                ExecutionJobState::NotStarted { .. } | ExecutionJobState::Ambiguous { .. } => {
                    return Err(CandidateBuildRunnerError::WorkerRejected);
                }
                ExecutionJobState::NotFound
                | ExecutionJobState::ReadyToStart(_)
                | ExecutionJobState::InDoubt { .. } => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(CandidateBuildRunnerError::WorkerTimeout);
            }
            tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }
    }

    fn content(&self) -> Result<SqliteContentStore, CandidateBuildRunnerError> {
        SqliteContentStore::open(
            self.server.content_database(),
            self.server.content_directory(),
        )
        .map_err(domain)
    }
}

/// Frozen Candidate build authority: the exact contract a Worker will be asked to execute.
#[derive(Clone, Debug)]
pub struct AuthorizedCandidateBuildV1 {
    proposal: ContentId<CandidateProposalArtifact>,
    job_id: JobId,
    request: ContentId<CandidateBuildRequestArtifact>,
    contract: cairn_execution::JobContract,
}

impl AuthorizedCandidateBuildV1 {
    /// Returns the exact proposal this build was authorized for.
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }

    /// Returns the frozen build request identity.
    #[must_use]
    pub const fn request(&self) -> ContentId<CandidateBuildRequestArtifact> {
        self.request
    }
}

/// Failure of a Worker-backed Candidate build.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CandidateBuildRunnerError {
    /// Controller or Worker configuration is invalid or scheduling is disabled.
    #[error("candidate build worker configuration is invalid")]
    InvalidConfiguration,
    /// The product-owned runner file could not be read.
    #[error("candidate build runner is unavailable: {0}")]
    Runner(String),
    /// No enrolled Worker accepted the contract inside the configured window.
    #[error("no worker accepted the candidate build contract")]
    NoWorker,
    /// A Worker refused or lost the authorized job.
    #[error("worker rejected the candidate build job")]
    WorkerRejected,
    /// No terminal receipt arrived before the deadline.
    #[error("candidate build did not reach a terminal receipt")]
    WorkerTimeout,
    /// Identity or reservation lineage drifted from the frozen contract.
    #[error("candidate build binding drifted from the frozen contract")]
    Binding,
    /// A domain, codec, or store operation failed.
    #[error("candidate build operation failed: {0}")]
    Domain(String),
}

fn domain(error: impl std::fmt::Display) -> CandidateBuildRunnerError {
    CandidateBuildRunnerError::Domain(error.to_string())
}
