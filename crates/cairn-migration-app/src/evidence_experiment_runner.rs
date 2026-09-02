use std::{
    collections::BTreeSet,
    io::Cursor,
    thread,
    time::{Duration, Instant},
};

use cairn_agent::{
    CanonicalToolResult, PreparedToolOperation, ToolEffectClass, ToolGateway, ToolGatewayError,
};
use cairn_execution::{
    CapabilityRequirement, CapturePolicy, CommandContract, DOCKER_BACKEND, DiagnosticByteLimit,
    DockerExecutionEnvironmentV1, DockerImageId, EvidenceByteLimit, ExecutionBackend,
    ExecutionEnvironmentArtifact, ExecutionJob, ExecutionJobState, ExecutionTimeoutMillis,
    InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode, JobContract,
    NetworkPolicy, OutputByteLimit, PlacementRequest, ReservationReleaseReason, ResourceRequest,
    SandboxPath, WorkerPoolName, recover_execution_job,
};
use cairn_migration::SirTaskWorkspace;
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
use serde::{Deserialize, Serialize};
use serde_json::json;

const TOOL_NAME: &str = "migration-run-evidence-experiment";
const TOOL_VERSION: &str = "migration-role-tools-v1";
const RUNNER: &[u8] = br"#!/bin/sh
set -eu
cd /cairn/input/task
exec /bin/sh /cairn/input/experiment/program.sh
";
const MIN_SCHEDULING_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// Exact ordinary-Worker placement and bounded capture policy for proposal-visible experiments.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceExperimentWorkerConfigV1 {
    schema_version: u16,
    image: DockerImageId,
    worker_pool: WorkerPoolName,
    capabilities: Vec<CapabilityRequirement>,
    execution_timeout: ExecutionTimeoutMillis,
    poll_interval_ms: u64,
    completion_timeout_ms: u64,
}

impl EvidenceExperimentWorkerConfigV1 {
    pub(crate) fn validate(&self) -> Result<(), ToolGatewayError> {
        if self.schema_version != 1 || self.poll_interval_ms == 0 || self.completion_timeout_ms == 0
        {
            return Err(ToolGatewayError::Rejected(
                "invalid evidence experiment Worker configuration".to_owned(),
            ));
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(ToolGatewayError::Rejected(
                "evidence experiment capabilities are not canonical".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExperimentRequestV1 {
    schema_version: u16,
    language: ExperimentLanguageV1,
    purpose: String,
    program: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ExperimentLanguageV1 {
    PosixShell,
}

/// Trusted adapter from one yielded Agent operation to an ordinary scheduled Worker job.
pub(crate) struct EvidenceExperimentRunnerV1 {
    server: ServerConfig,
    config: EvidenceExperimentWorkerConfigV1,
    workspace: SirTaskWorkspace,
}

impl EvidenceExperimentRunnerV1 {
    pub(crate) fn new(
        server: ServerConfig,
        config: EvidenceExperimentWorkerConfigV1,
        workspace: SirTaskWorkspace,
    ) -> Result<Self, ToolGatewayError> {
        config.validate()?;
        if server.scheduler.is_none() {
            return Err(ToolGatewayError::Rejected(
                "evidence experiment Worker scheduling is disabled".to_owned(),
            ));
        }
        Ok(Self {
            server,
            config,
            workspace,
        })
    }

    fn content(&self) -> Result<SqliteContentStore, ToolGatewayError> {
        SqliteContentStore::open(
            self.server.content_database(),
            self.server.content_directory(),
        )
        .map_err(rejected)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one linear Worker transaction preserves bundle, scheduling, and receipt lineage"
    )]
    fn execute(
        &self,
        request: &ExperimentRequestV1,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        if request.schema_version != 1
            || request.language != ExperimentLanguageV1::PosixShell
            || request.purpose.is_empty()
            || request.purpose.len() > 1024
            || request.program.is_empty()
            || request.program.len() > 32 * 1024
        {
            return Err(ToolGatewayError::Rejected(
                "evidence experiment request violates its current-V1 bounds".to_owned(),
            ));
        }
        let bundle = build_bundle(&self.workspace, request)?;
        let bundle_bytes = bundle.to_bytes().map_err(rejected)?;
        let environment = DockerExecutionEnvironmentV1::new(self.config.image.clone(), Vec::new())
            .map_err(rejected)?;
        let environment_bytes = environment.to_bytes().map_err(rejected)?;
        let input_id = ContentId::<InputBundleArtifact>::derive(&bundle_bytes).map_err(rejected)?;
        let environment_id = ContentId::<ExecutionEnvironmentArtifact>::derive(&environment_bytes)
            .map_err(rejected)?;
        let job_id = JobId::new();
        let contract = JobContract::new(
            job_id,
            input_id,
            environment_id,
            ExecutionBackend::new(DOCKER_BACKEND).map_err(rejected)?,
            CommandContract::new(
                SandboxPath::new("experiment/run").map_err(rejected)?,
                Vec::new(),
                SandboxPath::new("work").map_err(rejected)?,
            ),
            ResourceRequest::new(
                self.config.execution_timeout,
                PlacementRequest::new(
                    cairn_execution::ExecutionPlatformRequirement::default(),
                    vec![self.config.worker_pool.clone()],
                    self.config.capabilities.clone(),
                )
                .map_err(rejected)?,
            )
            .map_err(rejected)?,
            NetworkPolicy::Disabled,
            CapturePolicy::new(
                OutputByteLimit::new(16 * 1024).map_err(rejected)?,
                OutputByteLimit::new(16 * 1024).map_err(rejected)?,
                DiagnosticByteLimit::new(4 * 1024).map_err(rejected)?,
                EvidenceByteLimit::new(64 * 1024).map_err(rejected)?,
                Vec::new(),
            )
            .map_err(rejected)?,
        );
        {
            let mut content = self.content()?;
            archive(&mut content, input_id, &bundle_bytes)?;
            archive(&mut content, environment_id, &environment_bytes)?;
        }
        let deadline = Instant::now() + Duration::from_millis(self.config.completion_timeout_ms);
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
            match schedule_execution_contract(&self.server, &contract, ids).map_err(rejected)? {
                ControllerSchedulingOutcome::Scheduled { .. } => {
                    break (attempt_id, reservation_id);
                }
                ControllerSchedulingOutcome::NoCandidate { .. } => {
                    if Instant::now() >= deadline {
                        return Err(ToolGatewayError::NotStarted(
                            "no ordinary Worker satisfied the evidence experiment placement before the completion deadline"
                                .to_owned(),
                        ));
                    }
                    thread::sleep(scheduling_retry_interval);
                }
            }
        };
        tracing::info!(
            target: "cairn.migration.evidence-experiment",
            event = "evidence_experiment_worker_scheduled",
            job_id = %job_id,
            attempt_id = %attempt_id,
            input_id = %input_id,
            "proposal evidence experiment scheduled on an ordinary Worker"
        );
        let job = ExecutionJob::new(job_id).map_err(rejected)?;
        loop {
            let events = SqliteEventStore::open(self.server.event_database()).map_err(rejected)?;
            let content = self.content()?;
            match recover_execution_job(&events, &content, &job).map_err(rejected)? {
                ExecutionJobState::Completed {
                    receipt_id,
                    receipt,
                } => {
                    let release_reason = release_execution_reservation(
                        &self.server,
                        reservation_id,
                        &CommandId::new(),
                    )
                    .map_err(rejected)?;
                    if release_reason != ReservationReleaseReason::ExecutionTerminal {
                        return Err(ToolGatewayError::Rejected(
                            "evidence experiment released capacity without terminal execution"
                                .to_owned(),
                        ));
                    }
                    let stdout = read(&content, receipt.stdout_id())?;
                    let stderr = read(&content, receipt.stderr_id())?;
                    tracing::info!(
                        target: "cairn.migration.evidence-experiment",
                        event = "evidence_experiment_worker_completed",
                        job_id = %job_id,
                        attempt_id = %attempt_id,
                        receipt_id = %receipt_id,
                        outcome = ?receipt.outcome(),
                        exit_code = receipt.exit_code(),
                        stdout_bytes = stdout.len(),
                        stderr_bytes = stderr.len(),
                        release_reason = ?release_reason,
                        "proposal evidence experiment completed on an ordinary Worker"
                    );
                    return CanonicalToolResult::from_value(&json!({
                        "schema_version": 1,
                        "job_id": job_id,
                        "attempt_id": attempt_id,
                        "receipt_id": receipt_id,
                        "outcome": receipt.outcome(),
                        "exit_code": receipt.exit_code(),
                        "elapsed_ms": receipt.elapsed(),
                        "stdout": String::from_utf8_lossy(&stdout),
                        "stderr": String::from_utf8_lossy(&stderr),
                    }))
                    .map_err(rejected);
                }
                ExecutionJobState::NotStarted { .. } => {
                    return Err(ToolGatewayError::NotStarted(
                        "evidence experiment Worker definitively did not start".to_owned(),
                    ));
                }
                ExecutionJobState::Ambiguous { .. } => {
                    return Err(ToolGatewayError::Ambiguous(
                        "evidence experiment Worker completion is ambiguous".to_owned(),
                    ));
                }
                ExecutionJobState::NotFound
                | ExecutionJobState::ReadyToStart(_)
                | ExecutionJobState::InDoubt { .. } => {}
            }
            if Instant::now() >= deadline {
                return Err(ToolGatewayError::Ambiguous(
                    "evidence experiment Worker completion timed out".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(self.config.poll_interval_ms));
        }
    }
}

impl ToolGateway for EvidenceExperimentRunnerV1 {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        if operation.tool().as_str() != TOOL_NAME
            || operation.implementation_version().as_str() != TOOL_VERSION
            || operation.effect() != ToolEffectClass::Idempotent
        {
            return Err(ToolGatewayError::NotStarted(
                "operation does not match the evidence experiment registration".to_owned(),
            ));
        }
        let request = cairn_codec::from_slice(operation.argument_bytes()).map_err(rejected)?;
        self.execute(&request)
    }
}

fn build_bundle(
    workspace: &SirTaskWorkspace,
    request: &ExperimentRequestV1,
) -> Result<InputBundleV1, ToolGatewayError> {
    let mut directories = BTreeSet::from([
        "experiment".to_owned(),
        "task".to_owned(),
        "work".to_owned(),
    ]);
    for (path, _) in workspace.materialized_sources() {
        let mut current = String::from("task");
        let components = path.as_str().split('/').collect::<Vec<_>>();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            current.push('/');
            current.push_str(component);
            directories.insert(current.clone());
        }
    }
    let mut entries = directories
        .into_iter()
        .map(|path| {
            SandboxPath::new(path)
                .map(|path| InputBundleEntry::Directory { path })
                .map_err(rejected)
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.push(file(
        "experiment/run",
        InputFileMode::Executable,
        RUNNER.to_vec(),
    )?);
    entries.push(file(
        "experiment/program.sh",
        InputFileMode::Data,
        request.program.as_bytes().to_vec(),
    )?);
    entries.push(file(
        "experiment/purpose.txt",
        InputFileMode::Data,
        request.purpose.as_bytes().to_vec(),
    )?);
    for (path, source) in workspace.materialized_sources() {
        entries.push(file(
            &format!("task/{}", path.as_str()),
            InputFileMode::Data,
            source.into_bytes(),
        )?);
    }
    InputBundleV1::new(entries).map_err(rejected)
}

fn file(
    path: &str,
    mode: InputFileMode,
    bytes: Vec<u8>,
) -> Result<InputBundleEntry, ToolGatewayError> {
    Ok(InputBundleEntry::File {
        path: SandboxPath::new(path).map_err(rejected)?,
        mode,
        bytes,
    })
}

fn archive<T: cairn_protocol::ContentType>(
    content: &mut SqliteContentStore,
    expected: ContentId<T>,
    bytes: &[u8],
) -> Result<(), ToolGatewayError> {
    let actual = content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(rejected)?
        .content_id;
    if actual == expected {
        Ok(())
    } else {
        Err(ToolGatewayError::Rejected(
            "evidence experiment content identity changed".to_owned(),
        ))
    }
}

fn read<T: cairn_protocol::ContentType>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<Vec<u8>, ToolGatewayError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(rejected)?;
    Ok(bytes)
}

fn rejected(error: impl std::fmt::Display) -> ToolGatewayError {
    ToolGatewayError::Rejected(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ExperimentLanguageV1, ExperimentRequestV1, MIN_SCHEDULING_RETRY_INTERVAL, RUNNER};
    use std::time::Duration;

    #[test]
    fn runner_uses_the_read_only_input_mount_for_task_and_program() {
        let runner = std::str::from_utf8(RUNNER).expect("runner is UTF-8");
        assert!(runner.contains("cd /cairn/input/task"));
        assert!(runner.contains("/cairn/input/experiment/program.sh"));
        assert!(!runner.contains("cd ../task"));
    }

    #[test]
    fn scheduling_retries_are_rate_limited() {
        assert_eq!(MIN_SCHEDULING_RETRY_INTERVAL, Duration::from_secs(1));
    }

    #[test]
    fn experiment_request_requires_the_exact_execution_language() {
        let request: ExperimentRequestV1 = cairn_codec::from_slice(
            br#"{"language":"posix-shell","program":"true","purpose":"probe","schema_version":1}"#,
        )
        .expect("current request");
        assert_eq!(request.language, ExperimentLanguageV1::PosixShell);
        assert!(
            cairn_codec::from_slice::<ExperimentRequestV1>(
                br#"{"program":"true","purpose":"probe","schema_version":1}"#,
            )
            .is_err()
        );
        assert!(
            cairn_codec::from_slice::<ExperimentRequestV1>(
                br#"{"language":"python","program":"pass","purpose":"probe","schema_version":1}"#,
            )
            .is_err()
        );
    }
}
