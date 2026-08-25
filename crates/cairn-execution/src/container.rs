use cairn_protocol::{AttemptId, ContentId, JobId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    EnvironmentVariable, ExecutionElapsedMillis, ExecutionEnvironmentArtifact,
    ExecutionEnvironmentV1, ExecutionTimeoutMillis, InputBundleArtifact, JobContractArtifact,
    MaterialFormatError, OutputByteLimit, ResolvedProgramIdentity, SandboxPath,
};

/// Exact generic backend claim for the hardened CPU-only OCI adapter.
pub const OCI_CONTAINER_BACKEND: &str = "oci-container-v1";

/// Immutable OCI image reference. Mutable tags are intentionally unrepresentable.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct OciImageDigest(String);

impl OciImageDigest {
    /// Creates one canonical `sha256:<64 lowercase hex>` image digest.
    ///
    /// # Errors
    ///
    /// Rejects image tags, uppercase/non-hex digests, and algorithms other than SHA-256.
    pub fn new(value: impl Into<String>) -> Result<Self, ContainerContractError> {
        let value = value.into();
        let Some(digest) = value.strip_prefix("sha256:") else {
            return Err(ContainerContractError::InvalidImageDigest);
        };
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ContainerContractError::InvalidImageDigest);
        }
        Ok(Self(value))
    }

    /// Returns the canonical digest reference.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for OciImageDigest {
    type Error = ContainerContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<OciImageDigest> for String {
    fn from(value: OciImageDigest) -> Self {
        value.0
    }
}

/// Deterministic runtime name owned by exactly one execution attempt.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContainerName(String);

impl ContainerName {
    const PREFIX: &'static str = "cairn-attempt-";

    /// Derives the only admitted container name for an attempt.
    #[must_use]
    pub fn for_attempt(attempt_id: AttemptId) -> Self {
        Self(format!("{}{}", Self::PREFIX, attempt_id.as_uuid()))
    }

    /// Returns the deterministic runtime name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether the name belongs to the exact attempt.
    #[must_use]
    pub fn belongs_to(&self, attempt_id: AttemptId) -> bool {
        *self == Self::for_attempt(attempt_id)
    }

    fn parse(value: &str) -> Result<Self, ContainerContractError> {
        let Some(uuid) = value.strip_prefix(Self::PREFIX) else {
            return Err(ContainerContractError::InvalidContainerName);
        };
        let attempt = format!("attempt:{uuid}")
            .parse::<AttemptId>()
            .map_err(|_| ContainerContractError::InvalidContainerName)?;
        let canonical = Self::for_attempt(attempt);
        if canonical.0 != value {
            return Err(ContainerContractError::InvalidContainerName);
        }
        Ok(canonical)
    }
}

impl TryFrom<String> for ContainerName {
    type Error = ContainerContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ContainerName> for String {
    fn from(value: ContainerName) -> Self {
        value.0
    }
}

/// Full immutable runtime container identifier returned by a Docker-compatible engine.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuntimeContainerId(String);

impl RuntimeContainerId {
    /// Creates a canonical full 64-character lowercase hexadecimal runtime ID.
    ///
    /// # Errors
    ///
    /// Rejects short IDs, uppercase text, and non-hexadecimal bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ContainerContractError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ContainerContractError::InvalidRuntimeContainerId);
        }
        Ok(Self(value))
    }

    /// Returns the full runtime identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RuntimeContainerId {
    type Error = ContainerContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RuntimeContainerId> for String {
    fn from(value: RuntimeContainerId) -> Self {
        value.0
    }
}

/// Reconciliation phase observed by the trusted runtime adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerPhase {
    Absent,
    Created,
    Running,
    Exited,
}

/// Fixed semantic role of one backend-owned mount.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerMountRole {
    Input,
    Work,
    Output,
    Temporary,
}

/// Whether immutable image metadata would synthesize writable container mounts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerImageVolumeState {
    /// The image declares no volumes; only the code-owned launch mounts can exist.
    None,
    /// The image declares at least one volume and is inadmissible for the CPU sandbox.
    Declared,
}

/// Exact local image resolution plus the security-relevant image mount observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedContainerImage {
    image: OciImageDigest,
    volume_state: ContainerImageVolumeState,
}

impl ResolvedContainerImage {
    #[must_use]
    pub const fn new(image: OciImageDigest, volume_state: ContainerImageVolumeState) -> Self {
        Self {
            image,
            volume_state,
        }
    }

    #[must_use]
    pub const fn image(&self) -> &OciImageDigest {
        &self.image
    }

    #[must_use]
    pub const fn volume_state(&self) -> ContainerImageVolumeState {
        self.volume_state
    }
}

/// Code-owned sandbox policy; operator configuration cannot weaken its individual controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerSandboxPolicy {
    CpuUntrustedV1,
}

impl ContainerSandboxPolicy {
    /// Returns the stable label value recorded on every runtime container.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CpuUntrustedV1 => "cpu-untrusted-v1",
        }
    }
}

/// Immutable labels that must agree before Cairn may reuse or reconcile a named container.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerBinding {
    attempt_id: AttemptId,
    job_id: JobId,
    contract_id: ContentId<JobContractArtifact>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
    sandbox_policy: ContainerSandboxPolicy,
}

impl ContainerBinding {
    /// Creates the exact durable identity binding for one container.
    #[must_use]
    pub const fn new(
        attempt_id: AttemptId,
        job_id: JobId,
        contract_id: ContentId<JobContractArtifact>,
        input_bundle_id: ContentId<InputBundleArtifact>,
        environment_id: ContentId<ExecutionEnvironmentArtifact>,
        sandbox_policy: ContainerSandboxPolicy,
    ) -> Self {
        Self {
            attempt_id,
            job_id,
            contract_id,
            input_bundle_id,
            environment_id,
            sandbox_policy,
        }
    }

    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }

    #[must_use]
    pub const fn environment_id(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment_id
    }

    #[must_use]
    pub const fn sandbox_policy(&self) -> ContainerSandboxPolicy {
        self.sandbox_policy
    }
}

/// Validated runtime inspection. Impossible phase/identity combinations have no representation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "phase")]
pub enum ContainerInspection {
    Absent {
        name: ContainerName,
    },
    Created {
        name: ContainerName,
        runtime_id: RuntimeContainerId,
        binding: ContainerBinding,
    },
    Running {
        name: ContainerName,
        runtime_id: RuntimeContainerId,
        binding: ContainerBinding,
    },
    Exited {
        name: ContainerName,
        runtime_id: RuntimeContainerId,
        binding: ContainerBinding,
        exit_code: ContainerExitCode,
    },
}

impl ContainerInspection {
    #[must_use]
    pub const fn phase(&self) -> ContainerPhase {
        match self {
            Self::Absent { .. } => ContainerPhase::Absent,
            Self::Created { .. } => ContainerPhase::Created,
            Self::Running { .. } => ContainerPhase::Running,
            Self::Exited { .. } => ContainerPhase::Exited,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &ContainerName {
        match self {
            Self::Absent { name }
            | Self::Created { name, .. }
            | Self::Running { name, .. }
            | Self::Exited { name, .. } => name,
        }
    }

    #[must_use]
    pub const fn runtime_id(&self) -> Option<&RuntimeContainerId> {
        match self {
            Self::Absent { .. } => None,
            Self::Created { runtime_id, .. }
            | Self::Running { runtime_id, .. }
            | Self::Exited { runtime_id, .. } => Some(runtime_id),
        }
    }

    #[must_use]
    pub const fn binding(&self) -> Option<&ContainerBinding> {
        match self {
            Self::Absent { .. } => None,
            Self::Created { binding, .. }
            | Self::Running { binding, .. }
            | Self::Exited { binding, .. } => Some(binding),
        }
    }

    #[must_use]
    pub const fn exit_code(&self) -> Option<ContainerExitCode> {
        match self {
            Self::Exited { exit_code, .. } => Some(*exit_code),
            Self::Absent { .. } | Self::Created { .. } | Self::Running { .. } => None,
        }
    }
}

/// Non-negative OCI subject exit code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContainerExitCode(u8);

impl ContainerExitCode {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Complete terminal observation returned only after waiting for one exact container.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerExitObservation {
    name: ContainerName,
    runtime_id: RuntimeContainerId,
    binding: ContainerBinding,
    exit_code: ContainerExitCode,
}

/// Independent runtime-owned stream drained without coupling stdout progress to stderr progress.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerStream {
    Stdout,
    Stderr,
}

/// Bounded bytes returned by a trusted runtime adapter.
///
/// `LimitExceeded` carries at most the configured prefix while proving that additional bytes were
/// observed and discarded. This prevents an adapter API from materializing an unbounded stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "state",
    content = "bytes"
)]
pub enum BoundedContainerBytes {
    Complete(Vec<u8>),
    LimitExceeded(Vec<u8>),
}

impl BoundedContainerBytes {
    /// Creates a complete bounded observation.
    ///
    /// # Errors
    ///
    /// Rejects adapter output larger than the requested bound.
    pub fn complete(
        bytes: Vec<u8>,
        limit: OutputByteLimit,
    ) -> Result<Self, ContainerContractError> {
        Self::validate_len(&bytes, limit)?;
        Ok(Self::Complete(bytes))
    }

    /// Creates an over-limit observation with only a bounded retained prefix.
    ///
    /// # Errors
    ///
    /// Rejects a retained prefix larger than the requested bound.
    pub fn limit_exceeded(
        prefix: Vec<u8>,
        limit: OutputByteLimit,
    ) -> Result<Self, ContainerContractError> {
        Self::validate_len(&prefix, limit)?;
        Ok(Self::LimitExceeded(prefix))
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Complete(bytes) | Self::LimitExceeded(bytes) => bytes,
        }
    }

    #[must_use]
    pub const fn exceeded_limit(&self) -> bool {
        matches!(self, Self::LimitExceeded(_))
    }

    fn validate_len(bytes: &[u8], limit: OutputByteLimit) -> Result<(), ContainerContractError> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit.get() {
            return Err(ContainerContractError::AdapterCaptureExceedsBound);
        }
        Ok(())
    }
}

/// Runtime classification for one declared candidate output path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "state")]
pub enum ContainerOutputObservation {
    Missing,
    NonRegular,
    Regular { capture: BoundedContainerBytes },
}

/// Total execution bound and independent stream bounds passed to one runtime wait operation.
///
/// The execution timeout is total time since the runtime-observed container start, not time since
/// this call. Consequently recovery cannot reset the contract deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerWaitPolicy {
    execution_timeout: ExecutionTimeoutMillis,
    stdout_limit: OutputByteLimit,
    stderr_limit: OutputByteLimit,
}

impl ContainerWaitPolicy {
    #[must_use]
    pub const fn new(
        execution_timeout: ExecutionTimeoutMillis,
        stdout_limit: OutputByteLimit,
        stderr_limit: OutputByteLimit,
    ) -> Self {
        Self {
            execution_timeout,
            stdout_limit,
            stderr_limit,
        }
    }

    #[must_use]
    pub const fn execution_timeout(self) -> ExecutionTimeoutMillis {
        self.execution_timeout
    }

    #[must_use]
    pub const fn stdout_limit(self) -> OutputByteLimit {
        self.stdout_limit
    }

    #[must_use]
    pub const fn stderr_limit(self) -> OutputByteLimit {
        self.stderr_limit
    }
}

/// Trusted reason for forcing a still-running subject to stop.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerStopReason {
    ExecutionTimeout,
    StdoutLimitExceeded,
    StderrLimitExceeded,
}

impl ContainerStopReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionTimeout => "execution-timeout",
            Self::StdoutLimitExceeded => "stdout-limit-exceeded",
            Self::StderrLimitExceeded => "stderr-limit-exceeded",
        }
    }
}

/// Result of a bounded wait while the runtime drains both output streams independently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerWaitOutcome {
    Exited(ContainerExitObservation),
    StopRequired(ContainerStopReason),
}

/// Runtime-owned terminal facts stored outside all candidate-writable mounts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerTerminalEvidence {
    exit: ContainerExitObservation,
    observed_image: OciImageDigest,
    resolved_program: ResolvedProgramIdentity,
    elapsed_ms: ExecutionElapsedMillis,
    forced_stop: Option<ContainerStopReason>,
}

impl ContainerTerminalEvidence {
    #[must_use]
    pub const fn new(
        exit: ContainerExitObservation,
        observed_image: OciImageDigest,
        resolved_program: ResolvedProgramIdentity,
        elapsed_ms: ExecutionElapsedMillis,
        forced_stop: Option<ContainerStopReason>,
    ) -> Self {
        Self {
            exit,
            observed_image,
            resolved_program,
            elapsed_ms,
            forced_stop,
        }
    }

    #[must_use]
    pub const fn exit(&self) -> &ContainerExitObservation {
        &self.exit
    }
    #[must_use]
    pub const fn observed_image(&self) -> &OciImageDigest {
        &self.observed_image
    }
    #[must_use]
    pub const fn resolved_program(&self) -> &ResolvedProgramIdentity {
        &self.resolved_program
    }
    #[must_use]
    pub const fn elapsed_ms(&self) -> ExecutionElapsedMillis {
        self.elapsed_ms
    }
    #[must_use]
    pub const fn forced_stop(&self) -> Option<ContainerStopReason> {
        self.forced_stop
    }
}

impl ContainerExitObservation {
    #[must_use]
    pub const fn new(
        name: ContainerName,
        runtime_id: RuntimeContainerId,
        binding: ContainerBinding,
        exit_code: ContainerExitCode,
    ) -> Self {
        Self {
            name,
            runtime_id,
            binding,
            exit_code,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &ContainerName {
        &self.name
    }

    #[must_use]
    pub const fn runtime_id(&self) -> &RuntimeContainerId {
        &self.runtime_id
    }

    #[must_use]
    pub const fn binding(&self) -> &ContainerBinding {
        &self.binding
    }

    #[must_use]
    pub const fn exit_code(&self) -> ContainerExitCode {
        self.exit_code
    }
}

/// Strict OCI-specific execution environment stored in `ExecutionEnvironmentArtifact`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OciExecutionEnvironmentV1 {
    schema_version: u16,
    image: OciImageDigest,
    variables: Vec<EnvironmentVariable>,
}

impl OciExecutionEnvironmentV1 {
    /// Creates canonical OCI environment bytes with one immutable image digest.
    ///
    /// # Errors
    ///
    /// Rejects duplicate environment names and NUL-containing values.
    pub fn new(
        image: OciImageDigest,
        variables: Vec<EnvironmentVariable>,
    ) -> Result<Self, ContainerContractError> {
        let canonical = ExecutionEnvironmentV1::new(variables)?;
        Ok(Self {
            schema_version: 1,
            image,
            variables: canonical.variables().to_vec(),
        })
    }

    #[must_use]
    pub const fn image(&self) -> &OciImageDigest {
        &self.image
    }

    #[must_use]
    pub fn variables(&self) -> &[EnvironmentVariable] {
        &self.variables
    }

    /// Decodes strict canonical JSON and validates the current V1 format.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical JSON, non-V1 schema, mutable image references, and invalid variables.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ContainerContractError> {
        let environment: Self = cairn_codec::from_slice(bytes)
            .map_err(|error| ContainerContractError::Codec(error.to_string()))?;
        environment.validate()?;
        Ok(environment)
    }

    /// Encodes canonical bytes suitable for typed content identity derivation.
    ///
    /// # Errors
    ///
    /// Rejects an invalid in-memory value or codec failure.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ContainerContractError> {
        self.validate()?;
        cairn_codec::to_vec(self).map_err(|error| ContainerContractError::Codec(error.to_string()))
    }

    fn validate(&self) -> Result<(), ContainerContractError> {
        if self.schema_version != 1 {
            return Err(ContainerContractError::UnsupportedEnvironmentSchema);
        }
        let canonical = ExecutionEnvironmentV1::new(self.variables.clone())?;
        if canonical.variables() != self.variables {
            return Err(ContainerContractError::NonCanonicalEnvironment);
        }
        Ok(())
    }
}

/// Read-only portion of the replaceable container-runtime capability.
///
/// F2d-b has frozen the non-weakenable launch plan; lifecycle mutation remains a later capability.
/// Product code never parses Docker/Podman output directly; adapters must return these typed
/// observations.
pub trait ContainerRuntime {
    /// Resolves the requested registry digest to the exact local immutable image identity.
    ///
    /// # Errors
    ///
    /// Returns a typed runtime error when reachability, resolution, or observation fails.
    fn resolve_image(
        &mut self,
        requested: &OciImageDigest,
    ) -> Result<ResolvedContainerImage, ContainerRuntimeError>;

    /// Inspects only the deterministic name and returns a provider-neutral lifecycle observation.
    ///
    /// # Errors
    ///
    /// Returns a typed runtime error when the name cannot be observed unambiguously.
    fn inspect(
        &mut self,
        name: &ContainerName,
    ) -> Result<ContainerInspection, ContainerRuntimeError>;
}

/// Minimal create/start capability parameterized by a backend-owned launch-plan type.
///
/// A definite `Unavailable` or `Rejected` mutation error proves that mutation was not applied.
/// `Ambiguous` means it may have been applied and must be reconciled through `inspect`. Successful
/// create/start calls are also inspected before the next transition. Terminal wait/capture belongs
/// to the stricter `ContainerCaptureRuntime` capability below.
pub trait ContainerLifecycleRuntime: ContainerRuntime {
    type LaunchPlan;

    /// Creates but does not start the exact planned container.
    ///
    /// # Errors
    ///
    /// Returns a classified definitive or ambiguous runtime failure.
    fn create(&mut self, plan: &Self::LaunchPlan) -> Result<(), ContainerRuntimeError>;

    /// Starts only the exact full runtime identity already observed in `Created`.
    ///
    /// # Errors
    ///
    /// Returns a classified definitive or ambiguous runtime failure.
    fn start(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
    ) -> Result<(), ContainerRuntimeError>;
}

/// Terminal supervision and bounded capture capability for one exact runtime identity.
///
/// This capability intentionally has no remove/cleanup operation. The worker cannot acquire
/// cleanup authority through this port before its terminal result has been durably published.
pub trait ContainerCaptureRuntime: ContainerLifecycleRuntime {
    /// Waits under the original total execution deadline while draining both streams independently.
    ///
    /// # Errors
    ///
    /// Returns a classified runtime observation failure for this exact container.
    fn wait_bounded(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
        policy: ContainerWaitPolicy,
    ) -> Result<ContainerWaitOutcome, ContainerRuntimeError>;

    /// Forces the exact container to stop and records the reason outside candidate-writable mounts.
    ///
    /// # Errors
    ///
    /// Returns a classified definite or ambiguous stop failure.
    fn stop(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
        reason: ContainerStopReason,
    ) -> Result<(), ContainerRuntimeError>;

    /// Returns one independently drained terminal stream under the supplied contract bound.
    ///
    /// # Errors
    ///
    /// Returns a classified failure when the exact bounded stream cannot be observed.
    fn capture_stream(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
        stream: ContainerStream,
        limit: OutputByteLimit,
    ) -> Result<BoundedContainerBytes, ContainerRuntimeError>;

    /// Reads only one declared sandbox path and classifies its file type before returning bytes.
    ///
    /// # Errors
    ///
    /// Returns a classified failure when the declared path cannot be observed safely.
    fn capture_output(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
        path: &SandboxPath,
        limit: OutputByteLimit,
    ) -> Result<ContainerOutputObservation, ContainerRuntimeError>;

    /// Returns runtime-owned terminal identity, timing, image, program, and forced-stop facts.
    ///
    /// # Errors
    ///
    /// Returns a classified failure when exact trusted terminal facts cannot be obtained.
    fn terminal_evidence(
        &mut self,
        name: &ContainerName,
        runtime_id: &RuntimeContainerId,
    ) -> Result<ContainerTerminalEvidence, ContainerRuntimeError>;
}

/// Classified failure of the trusted runtime capability.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContainerRuntimeError {
    /// The request did not cross the mutation boundary.
    #[error("container runtime is unavailable: {0}")]
    Unavailable(String),
    /// The runtime definitively rejected the request without applying a mutation.
    #[error("container runtime rejected the request: {0}")]
    Rejected(String),
    /// The request may have taken effect and only exact inspection may reconcile it.
    #[error("container runtime observation is ambiguous: {0}")]
    Ambiguous(String),
}

/// Invalid OCI/container contract value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContainerContractError {
    #[error("OCI image must be one canonical sha256 digest")]
    InvalidImageDigest,
    #[error("container name must be derived from one AttemptId")]
    InvalidContainerName,
    #[error("runtime container ID must be 64 lowercase hexadecimal characters")]
    InvalidRuntimeContainerId,
    #[error("OCI execution environment schema version is unsupported")]
    UnsupportedEnvironmentSchema,
    #[error("OCI environment variables are not in canonical order")]
    NonCanonicalEnvironment,
    #[error("OCI execution environment JSON failed: {0}")]
    Codec(String),
    #[error("container runtime adapter returned bytes above the requested capture bound")]
    AdapterCaptureExceedsBound,
    #[error(transparent)]
    Material(#[from] MaterialFormatError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EnvironmentVariableName;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn image_and_runtime_id_reject_mutable_or_ambiguous_values() {
        assert!(OciImageDigest::new(DIGEST).is_ok());
        assert!(OciImageDigest::new("ubuntu:latest").is_err());
        assert!(OciImageDigest::new(DIGEST.to_ascii_uppercase()).is_err());
        assert!(RuntimeContainerId::new(&DIGEST[7..]).is_ok());
        assert!(RuntimeContainerId::new(&DIGEST[7..19]).is_err());
    }

    #[test]
    fn container_name_is_exactly_attempt_derived() {
        let attempt_id = AttemptId::new();
        let name = ContainerName::for_attempt(attempt_id);
        assert!(name.belongs_to(attempt_id));
        assert!(!name.belongs_to(AttemptId::new()));
        let bytes = cairn_codec::to_vec(&name).expect("encode name");
        assert_eq!(
            cairn_codec::from_slice::<ContainerName>(&bytes).expect("decode name"),
            name
        );
        assert!(cairn_codec::from_slice::<ContainerName>(br#""candidate-chosen""#).is_err());
    }

    #[test]
    fn oci_environment_is_canonical_and_digest_bound() {
        let environment = OciExecutionEnvironmentV1::new(
            OciImageDigest::new(DIGEST).expect("digest"),
            vec![
                EnvironmentVariable::new(
                    EnvironmentVariableName::new("ZED").expect("name"),
                    "2".into(),
                ),
                EnvironmentVariable::new(
                    EnvironmentVariableName::new("ALPHA").expect("name"),
                    "1".into(),
                ),
            ],
        )
        .expect("environment");
        let bytes = environment.to_bytes().expect("encode");
        let first_identity = ContentId::<ExecutionEnvironmentArtifact>::derive(&bytes)
            .expect("first environment identity");
        assert_eq!(
            OciExecutionEnvironmentV1::from_bytes(&bytes).expect("decode"),
            environment
        );
        assert_eq!(environment.variables()[0].name().as_str(), "ALPHA");
        assert!(
            OciExecutionEnvironmentV1::from_bytes(
                br#"{"image":"ubuntu:latest","schema_version":1,"variables":[]}"#
            )
            .is_err()
        );
        let other_digest =
            "sha256:1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let changed = OciExecutionEnvironmentV1::new(
            OciImageDigest::new(other_digest).expect("other digest"),
            environment.variables().to_vec(),
        )
        .expect("changed environment")
        .to_bytes()
        .expect("changed bytes");
        assert_ne!(
            first_identity,
            ContentId::<ExecutionEnvironmentArtifact>::derive(&changed)
                .expect("changed environment identity")
        );
        let noncanonical_variables = format!(
            "{{\"image\":\"{DIGEST}\",\"schema_version\":1,\"variables\":[{{\"name\":\"ZED\",\"value\":\"2\"}},{{\"name\":\"ALPHA\",\"value\":\"1\"}}]}}"
        );
        assert_eq!(
            OciExecutionEnvironmentV1::from_bytes(noncanonical_variables.as_bytes()),
            Err(ContainerContractError::NonCanonicalEnvironment)
        );
        assert!(
            OciExecutionEnvironmentV1::from_bytes(
                format!("{{\"image\":\"{DIGEST}\",\"schema_version\":2,\"variables\":[]}}")
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn inspection_phase_cannot_exist_without_required_identity() {
        let name = ContainerName::for_attempt(AttemptId::new());
        let absent = ContainerInspection::Absent { name };
        assert_eq!(absent.phase(), ContainerPhase::Absent);
        assert!(absent.runtime_id().is_none());
        assert!(absent.binding().is_none());

        let name_json = String::from_utf8(cairn_codec::to_vec(absent.name()).expect("name JSON"))
            .expect("JSON is UTF-8");
        let invalid = format!("{{\"name\":{name_json},\"phase\":\"running\"}}");
        assert!(cairn_codec::from_slice::<ContainerInspection>(invalid.as_bytes()).is_err());
    }

    #[test]
    fn runtime_port_returns_only_typed_resolution_and_binding() {
        struct FakeRuntime {
            resolved: OciImageDigest,
            inspection: ContainerInspection,
        }

        impl ContainerRuntime for FakeRuntime {
            fn resolve_image(
                &mut self,
                _requested: &OciImageDigest,
            ) -> Result<ResolvedContainerImage, ContainerRuntimeError> {
                Ok(ResolvedContainerImage::new(
                    self.resolved.clone(),
                    ContainerImageVolumeState::None,
                ))
            }

            fn inspect(
                &mut self,
                _name: &ContainerName,
            ) -> Result<ContainerInspection, ContainerRuntimeError> {
                Ok(self.inspection.clone())
            }
        }

        let attempt_id = AttemptId::new();
        let name = ContainerName::for_attempt(attempt_id);
        let runtime_id = RuntimeContainerId::new(&DIGEST[7..]).expect("runtime ID");
        let binding = ContainerBinding::new(
            attempt_id,
            JobId::new(),
            ContentId::derive(b"contract").expect("contract ID"),
            ContentId::derive(b"input").expect("input ID"),
            ContentId::derive(b"environment").expect("environment ID"),
            ContainerSandboxPolicy::CpuUntrustedV1,
        );
        let running = ContainerInspection::Running {
            name: name.clone(),
            runtime_id: runtime_id.clone(),
            binding: binding.clone(),
        };
        let bytes = cairn_codec::to_vec(&running).expect("inspection bytes");
        assert_eq!(
            cairn_codec::from_slice::<ContainerInspection>(&bytes).expect("inspection decode"),
            running
        );

        let requested = OciImageDigest::new(DIGEST).expect("digest");
        let mut runtime = FakeRuntime {
            resolved: requested.clone(),
            inspection: running,
        };
        assert_eq!(
            runtime.resolve_image(&requested).expect("resolve").image(),
            &requested
        );
        let inspected = runtime.inspect(&name).expect("inspect");
        assert_eq!(inspected.phase(), ContainerPhase::Running);
        assert_eq!(inspected.runtime_id(), Some(&runtime_id));
        assert_eq!(inspected.binding(), Some(&binding));

        let exited = ContainerInspection::Exited {
            name,
            runtime_id,
            binding,
            exit_code: ContainerExitCode::new(137),
        };
        let exited_bytes = cairn_codec::to_vec(&exited).expect("exited bytes");
        let decoded = cairn_codec::from_slice::<ContainerInspection>(&exited_bytes)
            .expect("exited inspection");
        assert_eq!(decoded.exit_code().map(ContainerExitCode::get), Some(137));

        let terminal = ContainerExitObservation::new(
            exited.name().clone(),
            exited.runtime_id().expect("exited runtime ID").clone(),
            exited.binding().expect("exited binding").clone(),
            exited.exit_code().expect("exited code"),
        );
        let terminal_bytes = cairn_codec::to_vec(&terminal).expect("terminal bytes");
        assert_eq!(
            cairn_codec::from_slice::<ContainerExitObservation>(&terminal_bytes)
                .expect("terminal observation"),
            terminal
        );
    }
}
