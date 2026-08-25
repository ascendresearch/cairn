use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    num::NonZeroU64,
    os::unix::{
        fs::{DirBuilderExt as _, OpenOptionsExt as _},
        process::CommandExt as _,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use cairn_execution::{
    CapturedOutput, ExecutionBackend, ExecutionCapture, ExecutionElapsedMillis,
    ExecutionEnvironmentArtifact, ExecutionEnvironmentV1, ExecutionInput, ExecutionObservation,
    ExecutionOutcome, Executor, ExecutorError, InputBundleArtifact, InputBundleEntry,
    InputBundleV1, InputFileMode, NetworkPolicy, ResolvedProgramIdentity, TrustedExecutionEvidence,
};
use cairn_protocol::AttemptId;
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use rustix::process::{Pid, Signal, kill_process_group};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LOCAL_PROCESS_BACKEND: &str = "local-process-v1";

/// Worker execution capability. Bootstrap defaults to `disabled`; activation is an explicit edit.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "mode")]
pub enum WorkerExecutionConfig {
    /// No workload can start.
    #[default]
    Disabled,
    /// Controlled host process with an isolated Linux user/network namespace.
    LocalProcess {
        sandbox_directory: PathBuf,
        namespace: LinuxNamespaceConfig,
        supervisor_poll_interval_ms: NonZeroU64,
        /// Aggregate decoded regular-file bytes; `null` disables this separate budget.
        materialized_file_byte_limit: Option<NonZeroU64>,
    },
}

/// Explicit command used to create a Linux user and network namespace.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxNamespaceConfig {
    /// Absolute path to a compatible util-linux `unshare` binary.
    pub command: PathBuf,
    /// Namespace preflight bound; `null` disables the timeout.
    pub preflight_timeout_ms: Option<NonZeroU64>,
}

impl WorkerExecutionConfig {
    pub(crate) fn backend(&self) -> Result<Option<ExecutionBackend>, ExecutorError> {
        match self {
            Self::Disabled => Ok(None),
            Self::LocalProcess { .. } => ExecutionBackend::new(LOCAL_PROCESS_BACKEND)
                .map(Some)
                .map_err(|error| ExecutorError::NotStarted(error.to_string())),
        }
    }

    pub(crate) fn resolve_paths(&mut self, base: &Path) {
        if let Self::LocalProcess {
            sandbox_directory,
            namespace,
            ..
        } = self
        {
            super::resolve(sandbox_directory, base);
            super::resolve(&mut namespace.command, base);
        }
    }
}

pub(crate) struct LocalProcessExecutor<'a> {
    content: &'a SqliteContentStore,
    sandbox_directory: &'a Path,
    namespace: &'a LinuxNamespaceConfig,
    supervisor_poll_interval: Duration,
    materialized_file_byte_limit: Option<u64>,
}

impl<'a> LocalProcessExecutor<'a> {
    pub(crate) fn from_config(
        content: &'a SqliteContentStore,
        config: &'a WorkerExecutionConfig,
    ) -> Result<Self, ExecutorError> {
        let WorkerExecutionConfig::LocalProcess {
            sandbox_directory,
            namespace,
            supervisor_poll_interval_ms,
            materialized_file_byte_limit,
        } = config
        else {
            return Err(ExecutorError::NotStarted(
                "worker execution mode is disabled".into(),
            ));
        };
        Ok(Self {
            content,
            sandbox_directory,
            namespace,
            supervisor_poll_interval: Duration::from_millis(supervisor_poll_interval_ms.get()),
            materialized_file_byte_limit: materialized_file_byte_limit.map(NonZeroU64::get),
        })
    }

    pub(crate) fn preflight(&self) -> Result<(), ExecutorError> {
        self.preflight_namespace()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "material verification, create-only expansion, namespace launch, supervision, and evidence capture remain visibly ordered across the workload authority boundary"
    )]
    fn execute_inner(
        &self,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
    ) -> Result<ExecutionCapture, ExecutorError> {
        if contract.network() != NetworkPolicy::Disabled {
            return Err(ExecutorError::NotStarted(
                "local-process-v1 admits only network=disabled; dependency-fetch requires a separate constrained adapter"
                    .into(),
            ));
        }
        self.preflight_namespace()?;

        let bundle_bytes =
            read_content::<InputBundleArtifact>(self.content, &contract.input_bundle_id())?;
        let environment_bytes =
            read_content::<ExecutionEnvironmentArtifact>(self.content, &contract.environment_id())?;
        let bundle = InputBundleV1::from_bytes(&bundle_bytes)
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        let environment = ExecutionEnvironmentV1::from_bytes(&environment_bytes)
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        let attempt_directory = self
            .sandbox_directory
            .join(attempt_id.as_uuid().to_string());
        let workspace = attempt_directory.join("workspace");
        let supervisor = attempt_directory.join("supervisor");
        create_private_directory(&attempt_directory)?;
        create_private_directory(&workspace)?;
        create_private_directory(&supervisor)?;
        materialize_bundle(&workspace, &bundle, self.materialized_file_byte_limit)?;

        let program = workspace.join(contract.command().program().as_str());
        require_regular_file(&program, "declared program")?;
        let working_directory = workspace.join(contract.command().working_directory().as_str());
        require_directory(&working_directory, "declared working directory")?;
        let resolved_program = digest_program(&program)?;

        let stdout_path = supervisor.join("stdout");
        let stderr_path = supervisor.join("stderr");
        let stdout_file = create_private_file(&stdout_path, false)?;
        let stderr_file = create_private_file(&stderr_path, false)?;
        let mut command = Command::new(&self.namespace.command);
        command
            .args(["--user", "--map-root-user", "--net", "--"])
            .arg(&program)
            .args(
                contract
                    .command()
                    .arguments()
                    .iter()
                    .map(cairn_execution::CommandArgument::as_str),
            )
            .current_dir(&working_directory)
            .env_clear()
            .envs(
                environment
                    .variables()
                    .iter()
                    .map(|variable| (variable.name().as_str(), variable.value())),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .process_group(0);

        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            ExecutorError::NotStarted(format!("workload spawn failed: {error}"))
        })?;
        let process_group = Pid::from_child(&child);
        let timeout = Duration::from_millis(contract.resources().timeout().get());
        let stdout_limit = contract.capture().stdout_limit().get();
        let stderr_limit = contract.capture().stderr_limit().get();
        let mut forced_outcome = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    terminate_group(process_group, &mut child);
                    return Err(ExecutorError::Ambiguous(format!(
                        "workload wait failed: {error}"
                    )));
                }
            }
            if started.elapsed() >= timeout {
                forced_outcome = Some(ExecutionOutcome::TimedOut);
                terminate_group(process_group, &mut child);
                break child.wait().map_err(|error| {
                    ExecutorError::Ambiguous(format!("timed-out workload reap failed: {error}"))
                })?;
            }
            if file_len(&stdout_path)? > stdout_limit || file_len(&stderr_path)? > stderr_limit {
                forced_outcome = Some(ExecutionOutcome::IntegrityViolation);
                terminate_group(process_group, &mut child);
                break child.wait().map_err(|error| {
                    ExecutorError::Ambiguous(format!("over-limit workload reap failed: {error}"))
                })?;
            }
            thread::sleep(self.supervisor_poll_interval);
        };
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if file_len(&stdout_path)? > stdout_limit || file_len(&stderr_path)? > stderr_limit {
            forced_outcome = Some(ExecutionOutcome::IntegrityViolation);
        }
        let stdout = read_bounded_file(&stdout_path, stdout_limit)?;
        let stderr = read_bounded_file(&stderr_path, stderr_limit)?;
        let (outputs, output_integrity_violation) =
            capture_outputs(&workspace, contract.capture().expected_outputs())?;
        if output_integrity_violation {
            forced_outcome = Some(ExecutionOutcome::IntegrityViolation);
        }
        if forced_outcome.is_none()
            && status.success()
            && outputs.len() != contract.capture().expected_outputs().len()
        {
            forced_outcome = Some(ExecutionOutcome::IntegrityViolation);
        }
        let outcome = forced_outcome.unwrap_or_else(|| {
            if status.success() {
                ExecutionOutcome::Succeeded
            } else {
                ExecutionOutcome::SubjectFailed
            }
        });
        let backend = ExecutionBackend::new(LOCAL_PROCESS_BACKEND)
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        let evidence = TrustedExecutionEvidence::new(
            backend,
            contract.environment_id(),
            resolved_program,
            vec![
                ExecutionObservation::new("filesystem:create-only-workspace")
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
                ExecutionObservation::new("network:linux-user-net-namespace")
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
                ExecutionObservation::new("process:new-process-group")
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
            ],
        )
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        Ok(ExecutionCapture::new(
            outcome,
            status.code(),
            ExecutionElapsedMillis::new(elapsed),
            stdout,
            stderr,
            outputs,
            evidence,
        ))
    }

    fn preflight_namespace(&self) -> Result<(), ExecutorError> {
        let mut child = Command::new(&self.namespace.command)
            .args(["--user", "--map-root-user", "--net", "--"])
            .arg("/bin/true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                ExecutorError::NotStarted(format!("namespace preflight failed: {error}"))
            })?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(ExecutorError::NotStarted(format!(
                        "namespace preflight exited with {status}"
                    )));
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(ExecutorError::NotStarted(format!(
                        "namespace preflight wait failed: {error}"
                    )));
                }
            }
            if self
                .namespace
                .preflight_timeout_ms
                .is_some_and(|limit| started.elapsed() >= Duration::from_millis(limit.get()))
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ExecutorError::NotStarted(
                    "namespace preflight timed out".into(),
                ));
            }
            thread::sleep(self.supervisor_poll_interval);
        }
    }
}

impl Executor for LocalProcessExecutor<'_> {
    fn execute(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        self.execute_inner(input.attempt_id(), input.contract())
    }
}

fn read_content<T: cairn_protocol::ContentType>(
    content: &SqliteContentStore,
    content_id: &cairn_protocol::ContentId<T>,
) -> Result<Vec<u8>, ExecutorError> {
    let mut bytes = Vec::new();
    content
        .write_to(content_id, &mut bytes)
        .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
    Ok(bytes)
}

fn materialize_bundle(
    workspace: &Path,
    bundle: &InputBundleV1,
    byte_limit: Option<u64>,
) -> Result<(), ExecutorError> {
    let mut observed = 0_u64;
    for entry in bundle.entries() {
        let path = workspace.join(entry.path().as_str());
        match entry {
            InputBundleEntry::Directory { .. } => create_private_directory(&path)?,
            InputBundleEntry::File { mode, bytes, .. } => {
                observed = observed
                    .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                    .ok_or_else(|| {
                        ExecutorError::NotStarted("materialized size overflow".into())
                    })?;
                if byte_limit.is_some_and(|limit| observed > limit) {
                    return Err(ExecutorError::NotStarted(format!(
                        "materialized file bytes {observed} exceed configured limit"
                    )));
                }
                let mut file = create_private_file(&path, *mode == InputFileMode::Executable)?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| {
                        ExecutorError::NotStarted(format!("input write failed: {error}"))
                    })?;
            }
        }
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), ExecutorError> {
    fs::DirBuilder::new()
        .recursive(false)
        .mode(0o700)
        .create(path)
        .map_err(|error| {
            ExecutorError::NotStarted(format!(
                "create-only directory {} failed: {error}",
                path.display()
            ))
        })
}

fn create_private_file(path: &Path, executable: bool) -> Result<File, ExecutorError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(if executable { 0o700 } else { 0o600 })
        .open(path)
        .map_err(|error| {
            ExecutorError::NotStarted(format!(
                "create-only file {} failed: {error}",
                path.display()
            ))
        })
}

fn require_regular_file(path: &Path, purpose: &str) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ExecutorError::NotStarted(format!("{purpose} is unavailable: {error}")))?;
    if !metadata.file_type().is_file() {
        return Err(ExecutorError::NotStarted(format!(
            "{purpose} is not a regular file"
        )));
    }
    Ok(())
}

fn require_directory(path: &Path, purpose: &str) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ExecutorError::NotStarted(format!("{purpose} is unavailable: {error}")))?;
    if !metadata.file_type().is_dir() {
        return Err(ExecutorError::NotStarted(format!(
            "{purpose} is not a directory"
        )));
    }
    Ok(())
}

fn digest_program(path: &Path) -> Result<ResolvedProgramIdentity, ExecutorError> {
    let mut file = File::open(path)
        .map_err(|error| ExecutorError::NotStarted(format!("program open failed: {error}")))?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)
        .map_err(|error| ExecutorError::NotStarted(format!("program hash failed: {error}")))?;
    ResolvedProgramIdentity::new(format!("sha256:{:x}", hash.finalize()))
        .map_err(|error| ExecutorError::NotStarted(error.to_string()))
}

fn file_len(path: &Path) -> Result<u64, ExecutorError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| ExecutorError::Ambiguous(format!("capture metadata failed: {error}")))
}

fn read_bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>, ExecutorError> {
    let file = File::open(path)
        .map_err(|error| ExecutorError::Ambiguous(format!("capture open failed: {error}")))?;
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| ExecutorError::Ambiguous(format!("capture read failed: {error}")))?;
    Ok(bytes)
}

fn capture_outputs(
    workspace: &Path,
    outputs: &[cairn_execution::ExpectedOutput],
) -> Result<(Vec<CapturedOutput>, bool), ExecutorError> {
    let mut captured = Vec::new();
    let mut integrity_violation = false;
    for output in outputs {
        let path = workspace.join(output.path.as_str());
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            integrity_violation = true;
            continue;
        }
        if file_len(&path)? > output.byte_limit.get() {
            integrity_violation = true;
            continue;
        }
        captured.push(CapturedOutput {
            name: output.name.clone(),
            bytes: read_bounded_file(&path, output.byte_limit.get())?,
        });
    }
    Ok((captured, integrity_violation))
}

fn terminate_group(process_group: Pid, child: &mut std::process::Child) {
    let _ = kill_process_group(process_group, Signal::KILL);
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, os::unix::fs::PermissionsExt as _};

    use cairn_execution::{
        CapturePolicy, CommandContract, DiagnosticByteLimit, EnvironmentVariable,
        EnvironmentVariableName, EvidenceByteLimit, ExecutionCompletion, ExecutionEnvironmentV1,
        ExecutionJob, ExecutionJobState, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
        ExpectedOutput, InputBundleEntry, InputBundleV1, InputFileMode, JobContract, NetworkPolicy,
        OutputByteLimit, OutputName, PlacementRequest, ResourceRequest, SandboxPath,
        authorize_execution_attempt, begin_execution_attempt, execute_execution_attempt,
        prepare_execution_job, recover_execution_job,
    };
    use cairn_protocol::{CommandId, JobId, ObservedAtUnixMillis};
    use cairn_store_sqlite::SqliteEventStore;
    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        directory: TempDir,
        content: SqliteContentStore,
        executor_config: WorkerExecutionConfig,
        contract: JobContract,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("temporary directory");
            let launcher = directory.path().join("namespace-launcher");
            fs::write(
                &launcher,
                b"#!/bin/sh\n[ \"$1 $2 $3 $4\" = \"--user --map-root-user --net --\" ] || exit 91\nshift 4\nexec \"$@\"\n",
            )
            .expect("launcher");
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700))
                .expect("launcher permissions");
            let bundle = InputBundleV1::new(vec![
                InputBundleEntry::Directory {
                    path: SandboxPath::new("bin").expect("bin path"),
                },
                InputBundleEntry::File {
                    path: SandboxPath::new("bin/run").expect("program path"),
                    mode: InputFileMode::Executable,
                    bytes: b"#!/bin/sh\nif [ \"${1-}\" = sleep ]; then sleep 10; fi\nprintf '%s' \"$F2C_VALUE\"\nprintf 'result' > result.txt\n"
                        .to_vec(),
                },
                InputBundleEntry::Directory {
                    path: SandboxPath::new("work").expect("work path"),
                },
            ])
            .expect("bundle");
            let environment = ExecutionEnvironmentV1::new(vec![EnvironmentVariable::new(
                EnvironmentVariableName::new("F2C_VALUE").expect("environment name"),
                "stable-output".into(),
            )])
            .expect("environment");
            let mut content = SqliteContentStore::open(
                directory.path().join("content.sqlite3"),
                directory.path().join("content"),
            )
            .expect("content store");
            let input_bundle_id = content
                .put::<InputBundleArtifact>(&mut Cursor::new(
                    bundle.to_bytes().expect("bundle bytes"),
                ))
                .expect("put bundle")
                .content_id;
            let environment_id = content
                .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(
                    environment.to_bytes().expect("environment bytes"),
                ))
                .expect("put environment")
                .content_id;
            let contract = JobContract::new(
                JobId::new(),
                input_bundle_id,
                environment_id,
                ExecutionBackend::new(LOCAL_PROCESS_BACKEND).expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/run").expect("program"),
                    Vec::new(),
                    SandboxPath::new("work").expect("working directory"),
                ),
                ResourceRequest::new(
                    ExecutionTimeoutMillis::new(5_000).expect("timeout"),
                    PlacementRequest::new(
                        ExecutionPlatformRequirement::default(),
                        Vec::new(),
                        Vec::new(),
                    )
                    .expect("placement"),
                )
                .expect("resources"),
                NetworkPolicy::Disabled,
                CapturePolicy::new(
                    OutputByteLimit::new(1024).expect("stdout limit"),
                    OutputByteLimit::new(1024).expect("stderr limit"),
                    DiagnosticByteLimit::new(1024).expect("diagnostic limit"),
                    EvidenceByteLimit::new(4096).expect("evidence limit"),
                    vec![ExpectedOutput {
                        name: OutputName::new("result").expect("output name"),
                        path: SandboxPath::new("work/result.txt").expect("output path"),
                        byte_limit: OutputByteLimit::new(1024).expect("output limit"),
                    }],
                )
                .expect("capture"),
            );
            let executor_config = WorkerExecutionConfig::LocalProcess {
                sandbox_directory: directory.path().join("sandboxes"),
                namespace: LinuxNamespaceConfig {
                    command: launcher,
                    preflight_timeout_ms: NonZeroU64::new(1_000),
                },
                supervisor_poll_interval_ms: NonZeroU64::new(5).expect("poll interval"),
                materialized_file_byte_limit: NonZeroU64::new(1024 * 1024),
            };
            fs::create_dir(directory.path().join("sandboxes")).expect("sandbox root");
            Self {
                directory,
                content,
                executor_config,
                contract,
            }
        }
    }

    #[test]
    fn real_process_materializes_once_and_returns_bounded_capture() {
        let fixture = Fixture::new();
        let executor =
            LocalProcessExecutor::from_config(&fixture.content, &fixture.executor_config)
                .expect("executor");
        let attempt_id = AttemptId::new();
        let capture = executor
            .execute_inner(attempt_id, &fixture.contract)
            .expect("execution capture");
        let capture = serde_json::to_value(capture).expect("capture JSON");
        assert_eq!(capture["outcome"], "succeeded");
        assert_eq!(
            capture["stdout"],
            serde_json::json!([115, 116, 97, 98, 108, 101, 45, 111, 117, 116, 112, 117, 116])
        );
        assert_eq!(
            capture["outputs"][0]["bytes"],
            serde_json::json!([114, 101, 115, 117, 108, 116])
        );

        let replay = executor.execute_inner(attempt_id, &fixture.contract);
        assert!(matches!(replay, Err(ExecutorError::NotStarted(_))));
    }

    #[test]
    fn dependency_fetch_fails_before_namespace_or_materialization() {
        let mut fixture = Fixture::new();
        let value = serde_json::to_value(&fixture.contract).expect("contract JSON");
        let mut value = value;
        value["network"] = serde_json::json!("dependency-fetch");
        fixture.contract = serde_json::from_value(value).expect("contract");
        let executor =
            LocalProcessExecutor::from_config(&fixture.content, &fixture.executor_config)
                .expect("executor");
        assert!(matches!(
            executor.execute_inner(AttemptId::new(), &fixture.contract),
            Err(ExecutorError::NotStarted(_))
        ));
    }

    #[test]
    fn timeout_terminates_the_process_group_and_is_terminal() {
        let mut fixture = Fixture::new();
        let mut value = serde_json::to_value(&fixture.contract).expect("contract JSON");
        value["command"]["arguments"] = serde_json::json!(["sleep"]);
        value["resources"]["timeout"] = serde_json::json!(20);
        fixture.contract = serde_json::from_value(value).expect("contract");
        let executor =
            LocalProcessExecutor::from_config(&fixture.content, &fixture.executor_config)
                .expect("executor");
        let capture = executor
            .execute_inner(AttemptId::new(), &fixture.contract)
            .expect("timed-out capture");
        assert_eq!(
            serde_json::to_value(capture).expect("capture JSON")["outcome"],
            "timed-out"
        );
    }

    #[test]
    fn durable_executor_seam_publishes_and_recovers_the_real_capture() {
        let fixture = Fixture::new();
        let mut publishing_content = SqliteContentStore::open(
            fixture.directory.path().join("content.sqlite3"),
            fixture.directory.path().join("content"),
        )
        .expect("second content handle");
        let mut events = SqliteEventStore::open(fixture.directory.path().join("events.sqlite3"))
            .expect("event store");
        let attempt_id = AttemptId::new();
        let prepared =
            prepare_execution_job(&mut publishing_content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut events,
            prepared,
            attempt_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        let started = begin_execution_attempt(
            &mut events,
            authority,
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("start");
        let mut executor =
            LocalProcessExecutor::from_config(&fixture.content, &fixture.executor_config)
                .expect("executor");
        assert!(matches!(
            execute_execution_attempt(
                &mut events,
                &mut publishing_content,
                &mut executor,
                started,
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            )
            .expect("execute"),
            ExecutionCompletion::Completed { .. }
        ));
        assert!(matches!(
            recover_execution_job(
                &events,
                &publishing_content,
                &ExecutionJob::new(fixture.contract.job_id()).expect("job")
            )
            .expect("recover"),
            ExecutionJobState::Completed { .. }
        ));
    }
}
