use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    num::{NonZeroU16, NonZeroU64},
    os::unix::fs::{
        DirBuilderExt as _, FileTypeExt as _, OpenOptionsExt as _, PermissionsExt as _,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use cairn_execution::{
    CapturedOutput, DOCKER_BACKEND, DockerExecutionEnvironmentV1, ExecutionBackend,
    ExecutionCapture, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionInput,
    ExecutionObservation, ExecutionOutcome, Executor, ExecutorError, InputBundleArtifact,
    InputBundleEntry, InputBundleV1, InputFileMode, NetworkPolicy, RecoverableExecutor,
    ResolvedProgramIdentity, TrustedExecutionEvidence,
};
use cairn_protocol::AttemptId;
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const INPUT: &str = "/cairn/input";
const WORK: &str = "/cairn/work";
const OUTPUT: &str = "/cairn/output";
const ASCEND_DRIVER: &str = "/usr/local/Ascend/driver";
const ASCEND_MANAGER: &str = "/dev/davinci_manager";
const ASCEND_HDC: &str = "/dev/hisi_hdc";

macro_rules! docker_device_index {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(u16);

        impl $name {
            /// Creates a bounded host accelerator index.
            ///
            /// # Errors
            ///
            /// Rejects indices outside the worker policy's bounded host-device namespace.
            pub fn new(value: u16) -> Result<Self, &'static str> {
                if value > 1023 {
                    Err("Docker accelerator device index must be between 0 and 1023")
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u16 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = u16::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

docker_device_index!(
    /// Host NVIDIA device selected by the local Docker worker policy.
    NvidiaDeviceIndex
);
docker_device_index!(
    /// Host Ascend device selected by the local Docker worker policy.
    AscendDeviceIndex
);

/// Closed worker-local accelerator exposure policy for `docker-v1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum DockerAcceleratorConfig {
    None,
    Nvidia { device_index: NvidiaDeviceIndex },
    Ascend { device_index: AscendDeviceIndex },
}

/// Worker execution mode. Join defaults to disabled; Docker activation is one explicit edit.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "mode")]
pub enum WorkerExecutionConfig {
    #[default]
    Disabled,
    Docker {
        command: PathBuf,
        state_directory: PathBuf,
        accelerator: DockerAcceleratorConfig,
        poll_interval_ms: NonZeroU64,
        logical_cpu_limit: Option<NonZeroU16>,
        memory_byte_limit: Option<NonZeroU64>,
        pids_limit: Option<NonZeroU64>,
        writable_byte_limit: Option<NonZeroU64>,
    },
}

impl WorkerExecutionConfig {
    pub(crate) fn backend(&self) -> Result<Option<ExecutionBackend>, ExecutorError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Docker { .. } => ExecutionBackend::new(DOCKER_BACKEND)
                .map(Some)
                .map_err(|error| ExecutorError::NotStarted(error.to_string())),
        }
    }

    pub(crate) fn resolve_paths(&mut self, base: &Path) {
        if let Self::Docker {
            command,
            state_directory,
            ..
        } = self
        {
            super::resolve(command, base);
            super::resolve(state_directory, base);
        }
    }
}

pub(crate) struct DockerExecutor<'a> {
    content: &'a SqliteContentStore,
    command: &'a Path,
    state_directory: &'a Path,
    poll_interval: Duration,
    logical_cpu_limit: Option<NonZeroU16>,
    memory_byte_limit: Option<NonZeroU64>,
    pids_limit: Option<NonZeroU64>,
    writable_byte_limit: Option<NonZeroU64>,
    accelerator: &'a DockerAcceleratorConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerState {
    status: String,
    exit_code: i32,
    started_at: String,
    finished_at: String,
}

enum ContainerState {
    Absent,
    Created,
    Running(DockerState),
    Exited(DockerState),
}

impl<'a> DockerExecutor<'a> {
    pub(crate) fn from_config(
        content: &'a SqliteContentStore,
        config: &'a WorkerExecutionConfig,
    ) -> Result<Self, ExecutorError> {
        let WorkerExecutionConfig::Docker {
            command,
            state_directory,
            accelerator,
            poll_interval_ms,
            logical_cpu_limit,
            memory_byte_limit,
            pids_limit,
            writable_byte_limit,
        } = config
        else {
            return Err(ExecutorError::NotStarted(
                "worker execution mode is disabled".into(),
            ));
        };
        Ok(Self {
            content,
            command,
            state_directory,
            poll_interval: Duration::from_millis(poll_interval_ms.get()),
            logical_cpu_limit: *logical_cpu_limit,
            memory_byte_limit: *memory_byte_limit,
            pids_limit: *pids_limit,
            writable_byte_limit: *writable_byte_limit,
            accelerator,
        })
    }

    pub(crate) fn preflight(&self) -> Result<(), ExecutorError> {
        let output = self.run(["version", "--format", "{{.Server.Version}}"])?;
        require_success(output, "Docker daemon preflight")?;
        validate_accelerator_paths(self.accelerator)?;
        Ok(())
    }

    fn execute_inner(
        &self,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
    ) -> Result<ExecutionCapture, ExecutorError> {
        if contract.backend().as_str() != DOCKER_BACKEND {
            return Err(ExecutorError::NotStarted(
                "job backend is not docker-v1".into(),
            ));
        }
        if contract.network() != NetworkPolicy::Disabled {
            return Err(ExecutorError::NotStarted(
                "docker-v1 currently supports network=disabled only".into(),
            ));
        }
        if contract.command().working_directory().as_str() != "work" {
            return Err(ExecutorError::NotStarted(
                "docker-v1 working directory must be work".into(),
            ));
        }
        let name = container_name(attempt_id);
        let environment = self.environment(contract)?;
        let state = self.inspect(&name)?;
        match state {
            ContainerState::Absent => {
                self.prepare_input(attempt_id, contract)?;
                self.resolve_image(environment.image().as_str())?;
                self.create(&name, attempt_id, contract, &environment)?;
                self.start(&name)?;
            }
            ContainerState::Created => self.start(&name)?,
            ContainerState::Running(_) | ContainerState::Exited(_) => {}
        }
        let state = self.wait_for_exit(&name, attempt_id, contract)?;
        self.capture(&name, attempt_id, contract, &environment, &state)
    }

    fn environment(
        &self,
        contract: &cairn_execution::JobContract,
    ) -> Result<DockerExecutionEnvironmentV1, ExecutorError> {
        let bytes =
            read_content::<ExecutionEnvironmentArtifact>(self.content, &contract.environment_id())?;
        DockerExecutionEnvironmentV1::from_bytes(&bytes)
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))
    }

    fn prepare_input(
        &self,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
    ) -> Result<(), ExecutorError> {
        let bytes = read_content::<InputBundleArtifact>(self.content, &contract.input_bundle_id())?;
        let bundle = InputBundleV1::from_bytes(&bytes)
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        let attempt = self.attempt_directory(attempt_id);
        if attempt.exists() {
            fs::remove_dir_all(&attempt)
                .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        }
        create_directory(&attempt, 0o700)?;
        let input = attempt.join("input");
        create_directory(&input, 0o755)?;
        materialize_input(&input, &bundle)?;
        create_directory(&attempt.join("output"), 0o777)?;
        Ok(())
    }

    fn resolve_image(&self, image: &str) -> Result<(), ExecutorError> {
        let output = self.run(["image", "inspect", "--format", "{{.Id}}", image])?;
        let stdout = require_success(output, "Docker image inspect")?;
        if stdout.trim() != image {
            return Err(ExecutorError::NotStarted(
                "configured Docker image ID did not resolve exactly".into(),
            ));
        }
        Ok(())
    }

    fn create(
        &self,
        name: &str,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
        environment: &DockerExecutionEnvironmentV1,
    ) -> Result<(), ExecutorError> {
        let input = self.attempt_directory(attempt_id).join("input");
        let input = input
            .canonicalize()
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        let output = self
            .attempt_directory(attempt_id)
            .join("output")
            .canonicalize()
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        let mut arguments = vec![
            "container".to_owned(),
            "create".to_owned(),
            "--name".to_owned(),
            name.to_owned(),
            "--label".to_owned(),
            format!("io.cairn.attempt-id={attempt_id}"),
            "--read-only".to_owned(),
            "--restart".to_owned(),
            "no".to_owned(),
            "--network".to_owned(),
            "none".to_owned(),
            "--cap-drop".to_owned(),
            "ALL".to_owned(),
            "--security-opt".to_owned(),
            "no-new-privileges".to_owned(),
            "--user".to_owned(),
            "65532:65532".to_owned(),
        ];
        optional_limit(&mut arguments, "--cpus", self.logical_cpu_limit);
        optional_limit(&mut arguments, "--memory", self.memory_byte_limit);
        optional_limit(&mut arguments, "--pids-limit", self.pids_limit);
        append_accelerator_arguments(&mut arguments, self.accelerator);
        arguments.extend([
            "--mount".to_owned(),
            format!("type=bind,src={},dst={INPUT},readonly", input.display()),
            "--mount".to_owned(),
            format!("type=bind,src={},dst={OUTPUT}", output.display()),
            "--tmpfs".to_owned(),
            tmpfs(WORK, self.writable_byte_limit),
            "--tmpfs".to_owned(),
            tmpfs("/tmp", self.writable_byte_limit),
            "--workdir".to_owned(),
            WORK.to_owned(),
        ]);
        for variable in environment.variables() {
            arguments.extend([
                "--env".to_owned(),
                format!("{}={}", variable.name().as_str(), variable.value()),
            ]);
        }
        if matches!(self.accelerator, DockerAcceleratorConfig::Ascend { .. }) {
            arguments.extend(["--env".to_owned(), "ASCEND_RT_VISIBLE_DEVICES=0".to_owned()]);
        }
        arguments.extend([
            "--entrypoint".to_owned(),
            format!("{INPUT}/{}", contract.command().program().as_str()),
            environment.image().as_str().to_owned(),
        ]);
        arguments.extend(
            contract
                .command()
                .arguments()
                .iter()
                .map(|argument| argument.as_str().to_owned()),
        );
        let output = Command::new(self.command)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
        require_success(output, "Docker container create")?;
        Ok(())
    }

    fn start(&self, name: &str) -> Result<(), ExecutorError> {
        let output = self.run(["container", "start", name])?;
        require_success(output, "Docker container start")?;
        Ok(())
    }

    fn wait_for_exit(
        &self,
        name: &str,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
    ) -> Result<DockerState, ExecutorError> {
        loop {
            match self.inspect(name)? {
                ContainerState::Running(state) => {
                    if elapsed_since(&state.started_at)? >= contract.resources().timeout().get() {
                        persist_timeout_marker(&self.attempt_directory(attempt_id))?;
                        let output = self.run(["container", "stop", "--time", "1", name])?;
                        require_success(output, "Docker container stop")?;
                    } else {
                        thread::sleep(self.poll_interval);
                    }
                }
                ContainerState::Exited(state) => return Ok(state),
                ContainerState::Absent | ContainerState::Created => {
                    return Err(ExecutorError::Ambiguous(
                        "Docker container disappeared or returned to created state".into(),
                    ));
                }
            }
        }
    }

    fn capture(
        &self,
        name: &str,
        attempt_id: AttemptId,
        contract: &cairn_execution::JobContract,
        environment: &DockerExecutionEnvironmentV1,
        state: &DockerState,
    ) -> Result<ExecutionCapture, ExecutorError> {
        let capture_directory = self.attempt_directory(attempt_id).join("capture");
        if capture_directory.exists() {
            fs::remove_dir_all(&capture_directory)
                .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        }
        create_directory(&capture_directory, 0o700)?;
        let stdout_path = capture_directory.join("stdout");
        let stderr_path = capture_directory.join("stderr");
        let stdout_file = create_file(&stdout_path, 0o600)?;
        let stderr_file = create_file(&stderr_path, 0o600)?;
        let status = Command::new(self.command)
            .args(["container", "logs", name])
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .status()
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        if !status.success() {
            return Err(ExecutorError::Ambiguous(
                "Docker logs failed for exited container".into(),
            ));
        }
        let stdout_limit = contract.capture().stdout_limit().get();
        let stderr_limit = contract.capture().stderr_limit().get();
        let mut integrity_violation =
            file_len(&stdout_path)? > stdout_limit || file_len(&stderr_path)? > stderr_limit;
        let stdout = read_bounded(&stdout_path, stdout_limit)?;
        let stderr = read_bounded(&stderr_path, stderr_limit)?;
        let mut outputs = Vec::new();
        for expected in contract.capture().expected_outputs() {
            let Some(relative) = expected.path.as_str().strip_prefix("output/") else {
                integrity_violation = true;
                continue;
            };
            let target = self
                .attempt_directory(attempt_id)
                .join("output")
                .join(relative);
            let Ok(metadata) = fs::symlink_metadata(&target) else {
                integrity_violation = true;
                continue;
            };
            if !metadata.file_type().is_file() || metadata.len() > expected.byte_limit.get() {
                integrity_violation = true;
                continue;
            }
            outputs.push(CapturedOutput {
                name: expected.name.clone(),
                bytes: read_bounded(&target, expected.byte_limit.get())?,
            });
        }
        if state.exit_code == 0 && outputs.len() != contract.capture().expected_outputs().len() {
            integrity_violation = true;
        }
        let timed_out = self
            .attempt_directory(attempt_id)
            .join("timed-out")
            .exists();
        let outcome = if timed_out {
            ExecutionOutcome::TimedOut
        } else if integrity_violation {
            ExecutionOutcome::IntegrityViolation
        } else if state.exit_code == 0 {
            ExecutionOutcome::Succeeded
        } else {
            ExecutionOutcome::SubjectFailed
        };
        let elapsed = elapsed_between(&state.started_at, &state.finished_at)?;
        let backend = ExecutionBackend::new(DOCKER_BACKEND)
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        let resolved_program = ResolvedProgramIdentity::new(format!(
            "{}:{}",
            environment.image().as_str(),
            contract.command().program().as_str()
        ))
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        let evidence = TrustedExecutionEvidence::new(
            backend,
            contract.environment_id(),
            resolved_program,
            vec![
                ExecutionObservation::new(format!("docker:image:{}", environment.image().as_str()))
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
                ExecutionObservation::new(format!("docker:container:{name}"))
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
                ExecutionObservation::new(accelerator_observation(self.accelerator))
                    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
            ],
        )
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        Ok(ExecutionCapture::new(
            outcome,
            Some(state.exit_code),
            ExecutionElapsedMillis::new(elapsed),
            stdout,
            stderr,
            outputs,
            evidence,
        ))
    }

    fn inspect(&self, name: &str) -> Result<ContainerState, ExecutorError> {
        let output = self.run(["container", "inspect", "--format", "{{json .State}}", name])?;
        if !output.status.success() {
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            if diagnostic.contains("No such object") || diagnostic.contains("No such container") {
                return Ok(ContainerState::Absent);
            }
            return Err(ExecutorError::Ambiguous(format!(
                "Docker inspect failed: {}",
                diagnostic.trim()
            )));
        }
        let state: DockerState = serde_json::from_slice(trim_ascii(&output.stdout))
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
        match state.status.as_str() {
            "created" => Ok(ContainerState::Created),
            "running" => Ok(ContainerState::Running(state)),
            "exited" | "dead" => Ok(ContainerState::Exited(state)),
            other => Err(ExecutorError::Ambiguous(format!(
                "unsupported Docker container state {other}"
            ))),
        }
    }

    fn attempt_directory(&self, attempt_id: AttemptId) -> PathBuf {
        self.state_directory.join(attempt_id.as_uuid().to_string())
    }

    fn run<const N: usize>(&self, arguments: [&str; N]) -> Result<Output, ExecutorError> {
        Command::new(self.command)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| ExecutorError::NotStarted(error.to_string()))
    }
}

impl Executor for DockerExecutor<'_> {
    fn execute(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        self.execute_inner(input.attempt_id(), input.contract())
    }
}

impl RecoverableExecutor for DockerExecutor<'_> {
    fn recover(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        self.execute_inner(input.attempt_id(), input.contract())
    }
}

fn container_name(attempt_id: AttemptId) -> String {
    format!("cairn-{}", attempt_id.as_uuid())
}

pub(crate) fn cleanup_published_attempt(config: &WorkerExecutionConfig, attempt_id: AttemptId) {
    if let WorkerExecutionConfig::Docker {
        command,
        state_directory,
        ..
    } = config
    {
        cleanup_attempt(command, state_directory, attempt_id);
    }
}

fn cleanup_attempt(command: &Path, state_directory: &Path, attempt_id: AttemptId) {
    let name = container_name(attempt_id);
    let _ = Command::new(command)
        .args(["container", "rm", "--force", "--volumes", &name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let directory = state_directory.join(attempt_id.as_uuid().to_string());
    if directory.parent() == Some(state_directory) {
        let _ = fs::remove_dir_all(directory);
    }
}

fn append_accelerator_arguments(
    arguments: &mut Vec<String>,
    accelerator: &DockerAcceleratorConfig,
) {
    match accelerator {
        DockerAcceleratorConfig::None => {}
        DockerAcceleratorConfig::Nvidia { device_index } => arguments.extend([
            "--gpus".to_owned(),
            format!("device={}", device_index.get()),
        ]),
        DockerAcceleratorConfig::Ascend { device_index } => {
            arguments.extend([
                "--cap-add".to_owned(),
                "DAC_OVERRIDE".to_owned(),
                "--mount".to_owned(),
                format!("type=bind,src={ASCEND_DRIVER},dst={ASCEND_DRIVER},readonly"),
            ]);
            for path in [
                format!("/dev/davinci{}", device_index.get()),
                ASCEND_MANAGER.to_owned(),
                ASCEND_HDC.to_owned(),
            ] {
                arguments.extend(["--device".to_owned(), format!("{path}:{path}:rwm")]);
            }
        }
    }
}

fn validate_accelerator_paths(accelerator: &DockerAcceleratorConfig) -> Result<(), ExecutorError> {
    let DockerAcceleratorConfig::Ascend { device_index } = accelerator else {
        return Ok(());
    };
    let driver = fs::metadata(ASCEND_DRIVER)
        .map_err(|error| ExecutorError::NotStarted(format!("{ASCEND_DRIVER}: {error}")))?;
    if !driver.is_dir() {
        return Err(ExecutorError::NotStarted(
            "Ascend driver policy path is not a directory".into(),
        ));
    }
    for path in [
        format!("/dev/davinci{}", device_index.get()),
        ASCEND_MANAGER.to_owned(),
        ASCEND_HDC.to_owned(),
    ] {
        let metadata = fs::metadata(&path)
            .map_err(|error| ExecutorError::NotStarted(format!("{path}: {error}")))?;
        if !metadata.file_type().is_char_device() {
            return Err(ExecutorError::NotStarted(format!(
                "Ascend policy path is not a character device: {path}"
            )));
        }
    }
    Ok(())
}

fn accelerator_observation(accelerator: &DockerAcceleratorConfig) -> String {
    match accelerator {
        DockerAcceleratorConfig::None => "docker:accelerator:none".to_owned(),
        DockerAcceleratorConfig::Nvidia { device_index } => {
            format!("docker:accelerator:nvidia:{}", device_index.get())
        }
        DockerAcceleratorConfig::Ascend { device_index } => {
            format!("docker:accelerator:ascend:{}", device_index.get())
        }
    }
}

fn optional_limit<T: std::fmt::Display>(arguments: &mut Vec<String>, flag: &str, value: Option<T>) {
    if let Some(value) = value {
        arguments.extend([flag.to_owned(), value.to_string()]);
    }
}

fn tmpfs(path: &str, limit: Option<NonZeroU64>) -> String {
    let size = limit.map_or_else(String::new, |value| format!(",size={}", value.get()));
    format!("{path}:rw,nosuid,nodev,mode=0770,uid=65532,gid=65532{size}")
}

fn require_success(output: Output, operation: &str) -> Result<String, ExecutorError> {
    if !output.status.success() {
        return Err(ExecutorError::NotStarted(format!(
            "{operation} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| ExecutorError::NotStarted(error.to_string()))
}

fn read_content<T: cairn_protocol::ContentType>(
    content: &SqliteContentStore,
    id: &cairn_protocol::ContentId<T>,
) -> Result<Vec<u8>, ExecutorError> {
    let mut bytes = Vec::new();
    content
        .write_to(id, &mut bytes)
        .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
    Ok(bytes)
}

fn materialize_input(root: &Path, bundle: &InputBundleV1) -> Result<(), ExecutorError> {
    for entry in bundle.entries() {
        let path = root.join(entry.path().as_str());
        match entry {
            InputBundleEntry::Directory { .. } => create_directory(&path, 0o755)?,
            InputBundleEntry::File { mode, bytes, .. } => {
                let permissions = match mode {
                    InputFileMode::Executable => 0o555,
                    InputFileMode::Data => 0o444,
                };
                let mut file = create_file(&path, permissions)?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn create_directory(path: &Path, mode: u32) -> Result<(), ExecutorError> {
    fs::DirBuilder::new()
        .recursive(false)
        .mode(mode)
        .create(path)
        .map_err(|error| ExecutorError::NotStarted(format!("{}: {error}", path.display())))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| ExecutorError::NotStarted(error.to_string()))
}

fn create_file(path: &Path, mode: u32) -> Result<File, ExecutorError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| ExecutorError::NotStarted(format!("{}: {error}", path.display())))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| ExecutorError::NotStarted(error.to_string()))?;
    Ok(file)
}

fn file_len(path: &Path) -> Result<u64, ExecutorError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ExecutorError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
    Ok(bytes)
}

fn persist_timeout_marker(attempt: &Path) -> Result<(), ExecutorError> {
    let marker = attempt.join("timed-out");
    if marker.exists() {
        return Ok(());
    }
    create_file(&marker, 0o600).map(|_| ())
}

fn elapsed_since(started_at: &str) -> Result<u64, ExecutorError> {
    let started = parse_time(started_at)?;
    let now = OffsetDateTime::now_utc();
    u64::try_from((now - started).whole_milliseconds())
        .map_err(|_| ExecutorError::Ambiguous("Docker start time is in the future".into()))
}

fn elapsed_between(started_at: &str, finished_at: &str) -> Result<u64, ExecutorError> {
    let elapsed = parse_time(finished_at)? - parse_time(started_at)?;
    u64::try_from(elapsed.whole_milliseconds())
        .map_err(|_| ExecutorError::Ambiguous("invalid Docker execution timing".into()))
}

fn parse_time(value: &str) -> Result<OffsetDateTime, ExecutorError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| ExecutorError::Ambiguous(error.to_string()))
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_execution::{
        CapturePolicy, CommandContract, DiagnosticByteLimit, DockerImageId, EnvironmentVariable,
        EnvironmentVariableName, EvidenceByteLimit, ExecutionPlatformRequirement,
        ExecutionTimeoutMillis, ExpectedOutput, InputBundleEntry, JobContract, OutputByteLimit,
        OutputName, PlacementRequest, ResourceRequest, SandboxPath,
    };
    use cairn_protocol::JobId;
    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        _directory: TempDir,
        content: SqliteContentStore,
        config: WorkerExecutionConfig,
        contract: JobContract,
    }

    impl Fixture {
        fn new(image: &str) -> Self {
            let directory = tempfile::tempdir().expect("temporary directory");
            let bundle = InputBundleV1::new(vec![
                InputBundleEntry::Directory {
                    path: SandboxPath::new("bin").expect("bin"),
                },
                InputBundleEntry::File {
                    path: SandboxPath::new("bin/hello").expect("program"),
                    mode: InputFileMode::Executable,
                    bytes: b"#!/bin/sh\nprintf '%s\\n' \"$MESSAGE\"\nprintf 'artifact' > /cairn/output/result.txt\n"
                        .to_vec(),
                },
            ])
            .expect("bundle");
            let environment = DockerExecutionEnvironmentV1::new(
                DockerImageId::new(image).expect("image"),
                vec![EnvironmentVariable::new(
                    EnvironmentVariableName::new("MESSAGE").expect("name"),
                    "hello world".into(),
                )],
            )
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
                ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/hello").expect("program"),
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
                    OutputByteLimit::new(1024).expect("stdout"),
                    OutputByteLimit::new(1024).expect("stderr"),
                    DiagnosticByteLimit::new(1024).expect("diagnostic"),
                    EvidenceByteLimit::new(4096).expect("evidence"),
                    vec![ExpectedOutput {
                        name: OutputName::new("result").expect("name"),
                        path: SandboxPath::new("output/result.txt").expect("path"),
                        byte_limit: OutputByteLimit::new(1024).expect("output"),
                    }],
                )
                .expect("capture"),
            );
            let state_directory = directory.path().join("docker");
            fs::create_dir(&state_directory).expect("state directory");
            let config = WorkerExecutionConfig::Docker {
                command: PathBuf::from("/usr/bin/docker"),
                state_directory,
                accelerator: DockerAcceleratorConfig::None,
                poll_interval_ms: NonZeroU64::new(10).expect("poll"),
                logical_cpu_limit: None,
                memory_byte_limit: None,
                pids_limit: None,
                writable_byte_limit: NonZeroU64::new(16 * 1024 * 1024),
            };
            Self {
                _directory: directory,
                content,
                config,
                contract,
            }
        }
    }

    #[test]
    fn accelerator_policy_is_closed_typed_and_derives_fixed_docker_arguments() {
        assert!(serde_json::from_str::<DockerAcceleratorConfig>(r#"{"kind":"other"}"#).is_err());
        assert!(
            serde_json::from_str::<DockerAcceleratorConfig>(
                r#"{"kind":"nvidia","device_index":1024}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<DockerAcceleratorConfig>(
                r#"{"kind":"nvidia","device_index":0,"extra":true}"#,
            )
            .is_err()
        );

        let mut nvidia = Vec::new();
        append_accelerator_arguments(
            &mut nvidia,
            &DockerAcceleratorConfig::Nvidia {
                device_index: NvidiaDeviceIndex::new(0).expect("NVIDIA index"),
            },
        );
        assert_eq!(nvidia, ["--gpus", "device=0"]);

        let mut ascend = Vec::new();
        append_accelerator_arguments(
            &mut ascend,
            &DockerAcceleratorConfig::Ascend {
                device_index: AscendDeviceIndex::new(3).expect("Ascend index"),
            },
        );
        assert!(
            ascend
                .windows(2)
                .any(|pair| pair == ["--cap-add", "DAC_OVERRIDE"])
        );
        assert!(
            ascend
                .windows(2)
                .any(|pair| { pair == ["--device", "/dev/davinci3:/dev/davinci3:rwm"] })
        );
        assert!(
            ascend.windows(2).any(|pair| {
                pair == ["--device", "/dev/davinci_manager:/dev/davinci_manager:rwm"]
            })
        );
        assert!(
            ascend
                .windows(2)
                .any(|pair| { pair == ["--device", "/dev/hisi_hdc:/dev/hisi_hdc:rwm"] })
        );
        assert_eq!(
            accelerator_observation(&DockerAcceleratorConfig::Ascend {
                device_index: AscendDeviceIndex::new(3).expect("Ascend index"),
            }),
            "docker:accelerator:ascend:3"
        );
    }

    #[test]
    fn docker_state_decoder_accepts_the_states_used_for_replay() {
        let running: DockerState = serde_json::from_str(
            r#"{"Status":"running","ExitCode":0,"StartedAt":"2026-01-01T00:00:00Z","FinishedAt":"0001-01-01T00:00:00Z"}"#,
        )
        .expect("running state");
        assert_eq!(running.status, "running");
    }

    #[test]
    #[ignore = "requires a reachable local Docker daemon and CAIRN_DOCKER_IMAGE_ID"]
    fn real_docker_hello_world_is_replayable() {
        let image = std::env::var("CAIRN_DOCKER_IMAGE_ID").expect("Docker image ID");
        let fixture = Fixture::new(&image);
        let executor = DockerExecutor::from_config(&fixture.content, &fixture.config)
            .expect("Docker executor");
        executor.preflight().expect("Docker preflight");
        let attempt_id = AttemptId::new();
        let first = executor
            .execute_inner(attempt_id, &fixture.contract)
            .expect("first capture");
        let replay = executor
            .execute_inner(attempt_id, &fixture.contract)
            .expect("replayed capture");
        let first = serde_json::to_value(first).expect("first JSON");
        let replay = serde_json::to_value(replay).expect("replay JSON");
        assert_eq!(first, replay);
        assert_eq!(first["outcome"], "succeeded");
        assert_eq!(
            first["stdout"],
            serde_json::json!([104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 10])
        );
        assert_eq!(
            first["outputs"][0]["bytes"],
            serde_json::json!([97, 114, 116, 105, 102, 97, 99, 116])
        );
        cleanup_published_attempt(&fixture.config, attempt_id);
    }
}
