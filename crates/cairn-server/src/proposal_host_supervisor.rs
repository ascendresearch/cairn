//! Controller-owned supervision of one exact generic Proposal Host operation.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use cairn_agent::{
    AdapterVersion, EpisodeBudget, ModelOutputTokenLimit, ModelSelection, ResolvedRuntimeModel,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, ProposalHostBinaryIdentity, ProposalHostExperimentRequestV1,
    ProposalHostExperimentWorker, ProposalHostOutcomeV1, ProposalHostRequestV1,
    ProposalHostRuntimeV1, SirTaskLimits, execute_proposal_host_experiments,
};
use cairn_protocol::{ContentId, EpisodeId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use crate::ServerError;

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
            /// Rejects zero and values outside the current-V1 bound.
            pub fn new(value: u64) -> Result<Self, ServerError> {
                if value == 0 || value > $maximum {
                    Err(ServerError::Configuration(concat!(stringify!($name), " is outside its positive current-V1 bound").into()))
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 { self.0 }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: Deserializer<'de>,
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

/// Strict current-V1 process policy shared by every generic Proposal Host profile.
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

impl ProposalHostProcessConfigV1 {
    pub(crate) fn validate(&self) -> Result<(), ServerError> {
        if !self.executable.is_file() {
            return Err(ServerError::Configuration(
                "Proposal Host executable must name an existing regular file".into(),
            ));
        }
        let _ = self.binary_identity()?;
        let _ = self.resolved_model()?;
        Ok(())
    }

    pub(crate) fn resolve_paths(&mut self, base: &Path) {
        resolve(&mut self.executable, base);
        resolve(&mut self.state_root, base);
        resolve(&mut self.resolved_runtime_model, base);
    }

    fn binary_identity(&self) -> Result<ProposalHostBinaryIdentity, ServerError> {
        binary_identity(&self.executable)
    }

    fn resolved_model(&self) -> Result<ResolvedRuntimeModel, ServerError> {
        let bytes = fs::read(&self.resolved_runtime_model)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let model: ResolvedRuntimeModel = cairn_codec::from_slice(&bytes)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        if model.canonical_bytes().map_err(configuration_error)? != bytes
            || model.provider() != &self.selection.provider
            || model.wire_model() != &self.selection.model
            || model.deployment() != &self.selection.deployment
            || self.selection.adapter_version
                != AdapterVersion::new("native-protocol-v1").map_err(configuration_error)?
            || self.max_output_tokens > model.capabilities().max_output_tokens()
        {
            return Err(ServerError::Configuration(
                "resolved runtime model changed the configured Proposal Host policy".into(),
            ));
        }
        Ok(model)
    }

    pub(crate) fn runtime(
        &self,
        episode_id: EpisodeId,
    ) -> Result<ProposalHostRuntimeV1, ServerError> {
        let model = self.resolved_model()?;
        let bytes = model.canonical_bytes().map_err(configuration_error)?;
        Ok(ProposalHostRuntimeV1::new(
            episode_id,
            self.binary_identity()?,
            ContentId::<AgentResolvedRuntimeModelArtifact>::derive(&bytes)
                .map_err(supervisor_error)?,
            self.selection.clone(),
            self.budget.clone(),
            self.max_output_tokens,
            self.task_limits,
        ))
    }
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

pub(crate) struct HostProcessFailure {
    pub(crate) reason: ProposalHostProcessBlockedV1,
    pub(crate) diagnostic: String,
}

#[allow(
    clippy::too_many_lines,
    reason = "the bounded child lifecycle keeps spawn, drain, timeout, and terminal validation visible"
)]
pub(crate) async fn run_proposal_host_process(
    config: &ProposalHostProcessConfigV1,
    request: &ProposalHostRequestV1,
) -> Result<ProposalHostOutcomeV1, HostProcessFailure> {
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
    let outcome: ProposalHostOutcomeV1 =
        cairn_codec::from_slice(&stdout).map_err(|error| HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::InvalidTerminal,
            diagnostic: error.to_string(),
        })?;
    if cairn_codec::to_vec(&outcome).ok().as_deref() != Some(stdout.as_slice())
        || outcome.validate_against(request).is_err()
    {
        return Err(HostProcessFailure {
            reason: ProposalHostProcessBlockedV1::InvalidTerminal,
            diagnostic: "Proposal Host returned a noncanonical or cross-bound terminal".into(),
        });
    }
    Ok(outcome)
}

pub(crate) fn initialize_proposal_host_operation(
    config: &ProposalHostProcessConfigV1,
    runtime: &ProposalHostRuntimeV1,
) -> Result<(), ServerError> {
    fs::create_dir_all(&config.state_root)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let state = config.state_root.join(runtime.episode_id().to_string());
    fs::create_dir(&state).map_err(|error| ServerError::MigrationWorkflow(error.to_string()))?;
    let bytes = cairn_codec::to_vec(runtime).map_err(supervisor_error)?;
    let marker = state.join("invocation.v1.json");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(marker)
        .map_err(|error| ServerError::MigrationWorkflow(error.to_string()))?;
    file.write_all(&bytes).map_err(supervisor_error)?;
    file.sync_all().map_err(supervisor_error)
}

/// Executes Controller-authorized Worker experiments against one exact yielded Host episode.
///
/// The Host-owned `SQLite` journals remain the authority source: this function reopens them,
/// validates the stdout yield against their bound operations, and commits each operation start
/// before invoking the selected Worker adapter.
///
/// # Errors
///
/// Rejects invocation drift, missing/corrupt Host state, yield/binding/receipt drift, Worker
/// failure, or any durable operation publication failure.
pub fn execute_proposal_host_controller_experiments<W: ProposalHostExperimentWorker>(
    config: &ProposalHostProcessConfigV1,
    request: &ProposalHostRequestV1,
    experiment: &ProposalHostExperimentRequestV1,
    worker: &mut W,
) -> Result<(), ServerError> {
    validate_proposal_host_operation(config, request)
        .map_err(|failure| ServerError::MigrationWorkflow(failure.diagnostic))?;
    let state = config
        .state_root
        .join(request.runtime().episode_id().to_string());
    let mut content = SqliteContentStore::open(state.join("content.db"), state.join("cas"))
        .map_err(supervisor_error)?;
    let mut events = SqliteEventStore::open(state.join("events.db")).map_err(supervisor_error)?;
    execute_proposal_host_experiments(&mut events, &mut content, request, experiment, worker)
        .map_err(supervisor_error)
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
        .map_err(supervisor_error)
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

fn configuration_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::Configuration(error.to_string())
}

fn supervisor_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
