use std::collections::BTreeSet;

use cairn_protocol::{AttemptId, ContentId, ContentType, JobId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! label_type {
    ($(#[$meta:meta])* $name:ident, $description:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Creates a validated label.
            ///
            /// # Errors
            ///
            #[doc = $description]
            pub fn new(value: impl Into<String>) -> Result<Self, ContractValueError> {
                let value = value.into();
                if value.is_empty()
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(ContractValueError::InvalidLabel(stringify!($name)));
                }
                Ok(Self(value))
            }

            /// Returns the validated string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ContractValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

macro_rules! selector_label_type {
    ($(#[$meta:meta])* $name:ident, $description:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Creates a bounded lowercase ASCII selector.
            ///
            /// # Errors
            ///
            #[doc = $description]
            pub fn new(value: impl Into<String>) -> Result<Self, ContractValueError> {
                let value = value.into();
                let bounded_edge = |byte: Option<&u8>| {
                    byte.is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                };
                if value.is_empty()
                    || value.len() > 64
                    || !bounded_edge(value.as_bytes().first())
                    || !bounded_edge(value.as_bytes().last())
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_' | b'.')
                    })
                {
                    return Err(ContractValueError::InvalidLabel(stringify!($name)));
                }
                Ok(Self(value))
            }

            /// Returns the validated selector.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ContractValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

selector_label_type!(
    /// Domain-neutral executor/backend capability name.
    ExecutionBackend,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector backend names."
);
selector_label_type!(
    /// Required worker capability key.
    CapabilityName,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector capability names."
);
label_type!(
    /// Required worker capability value.
    CapabilityValue,
    "Rejects empty, untrimmed, or control-containing capability values."
);
selector_label_type!(
    /// Canonical execution-platform architecture such as `x86_64` or `aarch64`.
    ArchitectureName,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector architecture names."
);
selector_label_type!(
    /// Canonical execution-platform operating system such as `linux`.
    OperatingSystemName,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector operating-system names."
);
selector_label_type!(
    /// Canonical target environment/ABI such as `gnu` or `musl`.
    TargetEnvironmentName,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector target-environment names."
);
selector_label_type!(
    /// Operator-owned worker-pool name used as a scheduling and policy boundary.
    WorkerPoolName,
    "Rejects empty, overlong, or non-lowercase-ASCII-selector worker-pool names."
);
label_type!(
    /// Logical declared-output name.
    OutputName,
    "Rejects empty, untrimmed, or control-containing output names."
);
label_type!(
    /// Executor-observed immutable program identity.
    ResolvedProgramIdentity,
    "Rejects empty, untrimmed, or control-containing program identities."
);
label_type!(
    /// One executor-observed runtime fact.
    ExecutionObservation,
    "Rejects empty, untrimmed, or control-containing runtime observations."
);

/// Sandbox-relative path that cannot address a host or parent path.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SandboxPath(String);

impl SandboxPath {
    /// Creates a portable sandbox-relative path.
    ///
    /// # Errors
    ///
    /// Rejects absolute, empty, parent/current-directory, backslash, and control-containing paths.
    pub fn new(value: impl Into<String>) -> Result<Self, ContractValueError> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || value
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ContractValueError::InvalidSandboxPath);
        }
        Ok(Self(value))
    }

    /// Returns the portable slash-separated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SandboxPath {
    type Error = ContractValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SandboxPath> for String {
    fn from(value: SandboxPath) -> Self {
        value.0
    }
}

/// One argv entry passed without shell interpretation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandArgument(String);

impl CommandArgument {
    /// Creates an argument that cannot contain a NUL byte.
    ///
    /// # Errors
    ///
    /// Rejects NUL-containing values because no process API can preserve them.
    pub fn new(value: impl Into<String>) -> Result<Self, ContractValueError> {
        let value = value.into();
        if value.contains('\0') {
            return Err(ContractValueError::InvalidArgument);
        }
        Ok(Self(value))
    }

    /// Returns the exact argument text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CommandArgument {
    type Error = ContractValueError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CommandArgument> for String {
    fn from(value: CommandArgument) -> Self {
        value.0
    }
}

macro_rules! positive_quantity {
    ($(#[$meta:meta])* $name:ident, $error:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive bounded quantity.
            ///
            /// # Errors
            ///
            /// Rejects zero.
            pub fn new(value: u64) -> Result<Self, ContractValueError> {
                if value == 0 {
                    Err(ContractValueError::$error)
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the wire value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = ContractValueError;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

positive_quantity!(/// Maximum execution duration.
ExecutionTimeoutMillis, ZeroTimeout);
positive_quantity!(/// Maximum captured byte count.
OutputByteLimit, ZeroOutputLimit);
positive_quantity!(/// Maximum durable executor-diagnostic byte count.
DiagnosticByteLimit, ZeroDiagnosticLimit);
positive_quantity!(/// Maximum canonical trusted-evidence byte count.
EvidenceByteLimit, ZeroEvidenceLimit);

/// Non-negative elapsed execution time observed by the trusted executor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ExecutionElapsedMillis(u64);

impl ExecutionElapsedMillis {
    /// Creates an elapsed duration. Zero is valid for sub-millisecond work.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Invalid execution-contract value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContractValueError {
    /// A label did not satisfy its type boundary.
    #[error("invalid execution label: {0}")]
    InvalidLabel(&'static str),
    /// A sandbox path could escape or be interpreted inconsistently.
    #[error("sandbox path must be a non-empty portable relative path without dot components")]
    InvalidSandboxPath,
    /// Process arguments cannot contain NUL.
    #[error("command argument contains NUL")]
    InvalidArgument,
    /// Timeout zero would disable a required safety bound ambiguously.
    #[error("execution timeout must be greater than zero")]
    ZeroTimeout,
    /// Output zero would disable capture ambiguously.
    #[error("output byte limit must be greater than zero")]
    ZeroOutputLimit,
    /// Diagnostic zero would disable durable failure context ambiguously.
    #[error("diagnostic byte limit must be greater than zero")]
    ZeroDiagnosticLimit,
    /// Evidence zero would disable trusted observation capture ambiguously.
    #[error("evidence byte limit must be greater than zero")]
    ZeroEvidenceLimit,
    /// A command must name one program.
    #[error("execution command must name one sandbox-relative program")]
    MissingProgram,
    /// Capability keys must be unique.
    #[error("execution capability requirement is duplicated: {0}")]
    DuplicateCapability(String),
    /// Persisted capability requirements must retain constructor ordering.
    #[error("execution capability requirements are not in canonical name order")]
    NonCanonicalCapabilities,
    /// Allowed worker pools must form a canonical set.
    #[error("worker pool selector is duplicated: {0}")]
    DuplicateWorkerPool(String),
    /// Persisted worker-pool selectors must retain constructor ordering.
    #[error("worker pool selectors are not in canonical order")]
    NonCanonicalWorkerPools,
    /// Expected output names and paths must both be unique.
    #[error("expected output name or path is duplicated: {0}")]
    DuplicateExpectedOutput(String),
    /// Persisted expected outputs must retain constructor ordering.
    #[error("expected outputs are not in canonical name order")]
    NonCanonicalExpectedOutputs,
    /// Trusted observations must form a canonical set.
    #[error("trusted execution observation is duplicated: {0}")]
    DuplicateObservation(String),
    /// Persisted observations must retain constructor ordering.
    #[error("trusted execution observations are not in canonical order")]
    NonCanonicalObservations,
    /// V2 contracts use a fixed schema version.
    #[error("job contract schema version is unsupported")]
    UnsupportedSchema,
}

/// Network access admitted for one sandbox.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkPolicy {
    /// No network namespace access.
    Disabled,
    /// Only a separately constrained dependency-fetch path is admitted.
    DependencyFetch,
}

/// Terminal classification of one observed execution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionOutcome {
    /// Declared command and required outputs completed successfully.
    Succeeded,
    /// The subject exited unsuccessfully under a healthy executor.
    SubjectFailed,
    /// The trusted supervisor enforced the timeout.
    TimedOut,
    /// An authorized cancellation was enforced.
    Cancelled,
    /// Infrastructure failed with a captured terminal observation.
    InfrastructureFailed,
    /// Captured bytes or environment violated the immutable contract.
    IntegrityViolation,
}

/// Exact argv contract; no shell string is accepted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandContract {
    program: SandboxPath,
    arguments: Vec<CommandArgument>,
    working_directory: SandboxPath,
}

impl CommandContract {
    /// Creates an exact non-shell command.
    #[must_use]
    pub const fn new(
        program: SandboxPath,
        arguments: Vec<CommandArgument>,
        working_directory: SandboxPath,
    ) -> Self {
        Self {
            program,
            arguments,
            working_directory,
        }
    }

    /// Returns the sandbox-relative executable.
    #[must_use]
    pub const fn program(&self) -> &SandboxPath {
        &self.program
    }

    /// Returns exact argv entries after argv[0].
    #[must_use]
    pub fn arguments(&self) -> &[CommandArgument] {
        &self.arguments
    }

    /// Returns the sandbox-relative working directory.
    #[must_use]
    pub const fn working_directory(&self) -> &SandboxPath {
        &self.working_directory
    }
}

/// One exact capability equality required from a future worker lease.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    /// Capability key.
    pub name: CapabilityName,
    /// Required value.
    pub value: CapabilityValue,
}

/// Concrete platform on which a native worker binary is executing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlatform {
    architecture: ArchitectureName,
    operating_system: OperatingSystemName,
    target_environment: TargetEnvironmentName,
}

impl ExecutionPlatform {
    /// Creates one exact execution platform.
    #[must_use]
    pub const fn new(
        architecture: ArchitectureName,
        operating_system: OperatingSystemName,
        target_environment: TargetEnvironmentName,
    ) -> Self {
        Self {
            architecture,
            operating_system,
            target_environment,
        }
    }

    /// Detects the platform compiled into the current worker binary.
    ///
    /// # Errors
    ///
    /// Returns an error only if Rust exposes a non-canonical target label.
    pub fn detect_host() -> Result<Self, ContractValueError> {
        Ok(Self::new(
            ArchitectureName::new(std::env::consts::ARCH)?,
            OperatingSystemName::new(std::env::consts::OS)?,
            TargetEnvironmentName::new(compiled_target_environment())?,
        ))
    }

    /// Returns the CPU architecture.
    #[must_use]
    pub const fn architecture(&self) -> &ArchitectureName {
        &self.architecture
    }

    /// Returns the operating system.
    #[must_use]
    pub const fn operating_system(&self) -> &OperatingSystemName {
        &self.operating_system
    }

    /// Returns the compiler target environment/ABI.
    #[must_use]
    pub const fn target_environment(&self) -> &TargetEnvironmentName {
        &self.target_environment
    }
}

fn compiled_target_environment() -> &'static str {
    if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "musl") {
        "musl"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else if cfg!(target_env = "sgx") {
        "sgx"
    } else {
        "unknown"
    }
}

/// Optional exact platform dimensions requested by a domain-neutral job.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlatformRequirement {
    architecture: Option<ArchitectureName>,
    operating_system: Option<OperatingSystemName>,
    target_environment: Option<TargetEnvironmentName>,
}

impl ExecutionPlatformRequirement {
    /// Creates independently optional platform constraints.
    #[must_use]
    pub const fn new(
        architecture: Option<ArchitectureName>,
        operating_system: Option<OperatingSystemName>,
        target_environment: Option<TargetEnvironmentName>,
    ) -> Self {
        Self {
            architecture,
            operating_system,
            target_environment,
        }
    }

    /// Requires all dimensions of one exact platform.
    #[must_use]
    pub fn exact(platform: &ExecutionPlatform) -> Self {
        Self::new(
            Some(platform.architecture.clone()),
            Some(platform.operating_system.clone()),
            Some(platform.target_environment.clone()),
        )
    }

    /// Returns the optional architecture constraint.
    #[must_use]
    pub const fn architecture(&self) -> Option<&ArchitectureName> {
        self.architecture.as_ref()
    }

    /// Returns the optional operating-system constraint.
    #[must_use]
    pub const fn operating_system(&self) -> Option<&OperatingSystemName> {
        self.operating_system.as_ref()
    }

    /// Returns the optional target-environment constraint.
    #[must_use]
    pub const fn target_environment(&self) -> Option<&TargetEnvironmentName> {
        self.target_environment.as_ref()
    }
}

/// Domain-neutral placement selectors produced above the execution scheduler.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementRequest {
    platform: ExecutionPlatformRequirement,
    allowed_worker_pools: Vec<WorkerPoolName>,
    capabilities: Vec<CapabilityRequirement>,
}

impl PlacementRequest {
    /// Creates canonical platform, pool, and capability selectors.
    ///
    /// An empty pool list admits every authenticated pool. It does not mean that the worker may
    /// self-select a pool.
    ///
    /// # Errors
    ///
    /// Rejects duplicate pool names or capability keys.
    pub fn new(
        platform: ExecutionPlatformRequirement,
        mut allowed_worker_pools: Vec<WorkerPoolName>,
        mut capabilities: Vec<CapabilityRequirement>,
    ) -> Result<Self, ContractValueError> {
        allowed_worker_pools.sort();
        if let Some(pair) = allowed_worker_pools
            .windows(2)
            .find(|pair| pair[0] == pair[1])
        {
            return Err(ContractValueError::DuplicateWorkerPool(
                pair[0].as_str().to_owned(),
            ));
        }
        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        if let Some(pair) = capabilities
            .windows(2)
            .find(|pair| pair[0].name == pair[1].name)
        {
            return Err(ContractValueError::DuplicateCapability(
                pair[0].name.as_str().to_owned(),
            ));
        }
        Ok(Self {
            platform,
            allowed_worker_pools,
            capabilities,
        })
    }

    /// Returns platform selectors.
    #[must_use]
    pub const fn platform(&self) -> &ExecutionPlatformRequirement {
        &self.platform
    }

    /// Returns the canonical allowed-pool set; empty means any authenticated pool.
    #[must_use]
    pub fn allowed_worker_pools(&self) -> &[WorkerPoolName] {
        &self.allowed_worker_pools
    }

    /// Returns sorted capability requirements.
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityRequirement] {
        &self.capabilities
    }

    fn validate(&self) -> Result<(), ContractValueError> {
        if self
            .allowed_worker_pools
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(ContractValueError::NonCanonicalWorkerPools);
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(ContractValueError::NonCanonicalCapabilities);
        }
        Ok(())
    }
}

/// Domain-neutral resources and capability selectors.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequest {
    timeout: ExecutionTimeoutMillis,
    placement: PlacementRequest,
}

impl ResourceRequest {
    /// Creates a resource request with unique capability keys.
    ///
    /// # Errors
    ///
    /// Rejects duplicate capability names.
    pub fn new(
        timeout: ExecutionTimeoutMillis,
        placement: PlacementRequest,
    ) -> Result<Self, ContractValueError> {
        placement.validate()?;
        Ok(Self { timeout, placement })
    }

    /// Returns the execution timeout.
    #[must_use]
    pub const fn timeout(&self) -> ExecutionTimeoutMillis {
        self.timeout
    }

    /// Returns the immutable placement request.
    #[must_use]
    pub const fn placement(&self) -> &PlacementRequest {
        &self.placement
    }

    fn validate(&self) -> Result<(), ContractValueError> {
        self.placement.validate()
    }
}

/// One required output collected by the trusted executor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedOutput {
    /// Stable logical output name.
    pub name: OutputName,
    /// Candidate-visible sandbox path to ingest after execution.
    pub path: SandboxPath,
    /// Independent ingestion bound.
    pub byte_limit: OutputByteLimit,
}

/// Independent stream and declared-output capture bounds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePolicy {
    stdout_limit: OutputByteLimit,
    stderr_limit: OutputByteLimit,
    diagnostic_limit: DiagnosticByteLimit,
    evidence_limit: EvidenceByteLimit,
    expected_outputs: Vec<ExpectedOutput>,
}

impl CapturePolicy {
    /// Creates a deterministic capture policy.
    ///
    /// # Errors
    ///
    /// Rejects duplicate output names or paths.
    pub fn new(
        stdout_limit: OutputByteLimit,
        stderr_limit: OutputByteLimit,
        diagnostic_limit: DiagnosticByteLimit,
        evidence_limit: EvidenceByteLimit,
        mut expected_outputs: Vec<ExpectedOutput>,
    ) -> Result<Self, ContractValueError> {
        expected_outputs.sort_by(|left, right| left.name.cmp(&right.name));
        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for output in &expected_outputs {
            if !names.insert(output.name.as_str()) {
                return Err(ContractValueError::DuplicateExpectedOutput(
                    output.name.as_str().to_owned(),
                ));
            }
            if !paths.insert(output.path.as_str()) {
                return Err(ContractValueError::DuplicateExpectedOutput(
                    output.path.as_str().to_owned(),
                ));
            }
        }
        Ok(Self {
            stdout_limit,
            stderr_limit,
            diagnostic_limit,
            evidence_limit,
            expected_outputs,
        })
    }

    /// Returns the stdout bound.
    #[must_use]
    pub const fn stdout_limit(&self) -> OutputByteLimit {
        self.stdout_limit
    }

    /// Returns the stderr bound.
    #[must_use]
    pub const fn stderr_limit(&self) -> OutputByteLimit {
        self.stderr_limit
    }

    /// Returns the durable executor-failure diagnostic bound.
    #[must_use]
    pub const fn diagnostic_limit(&self) -> DiagnosticByteLimit {
        self.diagnostic_limit
    }

    /// Returns the canonical trusted-evidence bound.
    #[must_use]
    pub const fn evidence_limit(&self) -> EvidenceByteLimit {
        self.evidence_limit
    }

    /// Returns expected outputs in canonical name order.
    #[must_use]
    pub fn expected_outputs(&self) -> &[ExpectedOutput] {
        &self.expected_outputs
    }

    fn validate(&self) -> Result<(), ContractValueError> {
        if self
            .expected_outputs
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(ContractValueError::NonCanonicalExpectedOutputs);
        }
        let mut paths = BTreeSet::new();
        if self
            .expected_outputs
            .iter()
            .any(|output| !paths.insert(&output.path))
        {
            return Err(ContractValueError::NonCanonicalExpectedOutputs);
        }
        Ok(())
    }
}

/// Immutable opaque execution job contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobContract {
    schema_version: u16,
    job_id: JobId,
    input_bundle_id: ContentId<InputBundleArtifact>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
    backend: ExecutionBackend,
    command: CommandContract,
    resources: ResourceRequest,
    network: NetworkPolicy,
    capture: CapturePolicy,
}

impl JobContract {
    /// Creates a V2 opaque job contract with explicit placement selectors.
    #[expect(
        clippy::too_many_arguments,
        reason = "every immutable execution dimension remains explicit at construction"
    )]
    #[must_use]
    pub const fn new(
        job_id: JobId,
        input_bundle_id: ContentId<InputBundleArtifact>,
        environment_id: ContentId<ExecutionEnvironmentArtifact>,
        backend: ExecutionBackend,
        command: CommandContract,
        resources: ResourceRequest,
        network: NetworkPolicy,
        capture: CapturePolicy,
    ) -> Self {
        Self {
            schema_version: 2,
            job_id,
            input_bundle_id,
            environment_id,
            backend,
            command,
            resources,
            network,
            capture,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), ContractValueError> {
        if self.schema_version != 2 {
            return Err(ContractValueError::UnsupportedSchema);
        }
        self.resources.validate()?;
        self.capture.validate()?;
        Ok(())
    }

    /// Returns the stable logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the immutable input bundle root.
    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }

    /// Returns the declared environment/image identity.
    #[must_use]
    pub const fn environment_id(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment_id
    }

    /// Returns the required execution backend.
    #[must_use]
    pub const fn backend(&self) -> &ExecutionBackend {
        &self.backend
    }

    /// Returns the exact command contract.
    #[must_use]
    pub const fn command(&self) -> &CommandContract {
        &self.command
    }

    /// Returns resource requirements and capability selectors.
    #[must_use]
    pub const fn resources(&self) -> &ResourceRequest {
        &self.resources
    }

    /// Returns the sandbox network policy.
    #[must_use]
    pub const fn network(&self) -> NetworkPolicy {
        self.network
    }

    /// Returns stream and declared-output capture bounds.
    #[must_use]
    pub const fn capture(&self) -> &CapturePolicy {
        &self.capture
    }
}

/// Trusted worker/supervisor observation inaccessible to candidate writes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedExecutionEvidence {
    backend: ExecutionBackend,
    observed_environment_id: ContentId<ExecutionEnvironmentArtifact>,
    resolved_program: ResolvedProgramIdentity,
    observations: Vec<ExecutionObservation>,
}

impl TrustedExecutionEvidence {
    /// Creates canonical trusted evidence outside the candidate write boundary.
    ///
    /// # Errors
    ///
    /// Rejects duplicate runtime observations.
    pub fn new(
        backend: ExecutionBackend,
        observed_environment_id: ContentId<ExecutionEnvironmentArtifact>,
        resolved_program: ResolvedProgramIdentity,
        mut observations: Vec<ExecutionObservation>,
    ) -> Result<Self, ContractValueError> {
        observations.sort();
        for pair in observations.windows(2) {
            if pair[0] == pair[1] {
                return Err(ContractValueError::DuplicateObservation(
                    pair[0].as_str().to_owned(),
                ));
            }
        }
        Ok(Self {
            backend,
            observed_environment_id,
            resolved_program,
            observations,
        })
    }

    /// Returns the backend actually selected by the executor.
    #[must_use]
    pub const fn backend(&self) -> &ExecutionBackend {
        &self.backend
    }

    /// Returns the exact observed environment/image manifest identity.
    #[must_use]
    pub const fn observed_environment_id(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.observed_environment_id
    }

    /// Returns the resolved program identity observed by the executor.
    #[must_use]
    pub const fn resolved_program(&self) -> &ResolvedProgramIdentity {
        &self.resolved_program
    }

    /// Returns canonical device/runtime observations.
    #[must_use]
    pub fn observations(&self) -> &[ExecutionObservation] {
        &self.observations
    }

    pub(crate) fn validate(&self) -> Result<(), ContractValueError> {
        if self.observations.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractValueError::NonCanonicalObservations);
        }
        Ok(())
    }
}

/// One archived declared output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedOutput {
    /// Logical expected-output name.
    pub name: OutputName,
    /// Exact untrusted output bytes ingested by the trusted executor.
    pub content_id: ContentId<DeclaredOutputArtifact>,
}

/// Canonical receipt tying terminal classification to exact captured artifacts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub(crate) schema_version: u16,
    pub(crate) job_id: JobId,
    pub(crate) attempt_id: AttemptId,
    pub(crate) contract_id: ContentId<JobContractArtifact>,
    pub(crate) outcome: ExecutionOutcome,
    pub(crate) exit_code: Option<i32>,
    pub(crate) elapsed_ms: ExecutionElapsedMillis,
    pub(crate) stdout_id: ContentId<ExecutionStdoutArtifact>,
    pub(crate) stderr_id: ContentId<ExecutionStderrArtifact>,
    pub(crate) evidence_id: ContentId<ExecutionEvidenceArtifact>,
    pub(crate) outputs: Vec<ArchivedOutput>,
}

impl ExecutionReceipt {
    /// Returns the logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the concrete attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the terminal classification.
    #[must_use]
    pub const fn outcome(&self) -> ExecutionOutcome {
        self.outcome
    }

    /// Returns the exact immutable contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Returns the trusted process exit code when one was observed.
    #[must_use]
    pub const fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Returns trusted elapsed execution time.
    #[must_use]
    pub const fn elapsed(&self) -> ExecutionElapsedMillis {
        self.elapsed_ms
    }

    /// Returns the exact stdout artifact.
    #[must_use]
    pub const fn stdout_id(&self) -> ContentId<ExecutionStdoutArtifact> {
        self.stdout_id
    }

    /// Returns the exact stderr artifact.
    #[must_use]
    pub const fn stderr_id(&self) -> ContentId<ExecutionStderrArtifact> {
        self.stderr_id
    }

    /// Returns trusted executor evidence.
    #[must_use]
    pub const fn evidence_id(&self) -> ContentId<ExecutionEvidenceArtifact> {
        self.evidence_id
    }

    /// Returns captured declared outputs in canonical name order.
    #[must_use]
    pub fn outputs(&self) -> &[ArchivedOutput] {
        &self.outputs
    }
}

macro_rules! content_type {
    ($name:ident, $domain:literal) => {
        /// Marker for an immutable execution content domain.
        pub struct $name;
        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

content_type!(InputBundleArtifact, "execution.input-bundle.v1");
content_type!(ExecutionEnvironmentArtifact, "execution.environment.v1");
content_type!(JobContractArtifact, "execution.job-contract.v2");
content_type!(ExecutionStdoutArtifact, "execution.stdout-untrusted.v1");
content_type!(ExecutionStderrArtifact, "execution.stderr-untrusted.v1");
content_type!(
    DeclaredOutputArtifact,
    "execution.declared-output-untrusted.v1"
);
content_type!(
    ExecutionEvidenceArtifact,
    "execution.worker-evidence-trusted.v1"
);
content_type!(ExecutionReceiptArtifact, "execution.receipt.v1");

#[cfg(test)]
mod tests {
    use cairn_protocol::ContentId;

    use super::*;

    #[test]
    fn persisted_contract_cannot_bypass_canonical_collection_invariants() {
        let contract = JobContract {
            schema_version: 2,
            job_id: JobId::new(),
            input_bundle_id: ContentId::derive(b"input").expect("input identity"),
            environment_id: ContentId::derive(b"environment").expect("environment identity"),
            backend: ExecutionBackend::new("recorded").expect("backend"),
            command: CommandContract::new(
                SandboxPath::new("bin/run").expect("program"),
                Vec::new(),
                SandboxPath::new("work").expect("working directory"),
            ),
            resources: ResourceRequest {
                timeout: ExecutionTimeoutMillis::new(1).expect("timeout"),
                placement: PlacementRequest {
                    platform: ExecutionPlatformRequirement::default(),
                    allowed_worker_pools: Vec::new(),
                    capabilities: vec![
                        CapabilityRequirement {
                            name: CapabilityName::new("z").expect("name"),
                            value: CapabilityValue::new("1").expect("value"),
                        },
                        CapabilityRequirement {
                            name: CapabilityName::new("a").expect("name"),
                            value: CapabilityValue::new("1").expect("value"),
                        },
                    ],
                },
            },
            network: NetworkPolicy::Disabled,
            capture: CapturePolicy::new(
                OutputByteLimit::new(1).expect("stdout"),
                OutputByteLimit::new(1).expect("stderr"),
                DiagnosticByteLimit::new(1).expect("diagnostic"),
                EvidenceByteLimit::new(1).expect("evidence"),
                Vec::new(),
            )
            .expect("capture"),
        };
        let bytes = cairn_codec::to_vec(&contract).expect("encode");
        let decoded: JobContract = cairn_codec::from_slice(&bytes).expect("decode");
        assert_eq!(
            decoded.validate(),
            Err(ContractValueError::NonCanonicalCapabilities)
        );
    }

    #[test]
    fn placement_has_strong_platform_and_canonical_pool_boundaries() {
        assert!(ArchitectureName::new("X86_64").is_err());
        assert!(WorkerPoolName::new("-target-lab").is_err());
        let pool = WorkerPoolName::new("target-lab").expect("pool");
        assert_eq!(
            PlacementRequest::new(
                ExecutionPlatformRequirement::default(),
                vec![pool.clone(), pool],
                Vec::new(),
            ),
            Err(ContractValueError::DuplicateWorkerPool("target-lab".into()))
        );
        let host = ExecutionPlatform::detect_host().expect("host platform");
        let exact = ExecutionPlatformRequirement::exact(&host);
        assert_eq!(exact.architecture(), Some(host.architecture()));
        assert_eq!(exact.operating_system(), Some(host.operating_system()));
        assert_eq!(exact.target_environment(), Some(host.target_environment()));
    }

    #[test]
    fn persisted_evidence_cannot_bypass_canonical_observation_invariants() {
        let evidence = TrustedExecutionEvidence {
            backend: ExecutionBackend::new("recorded").expect("backend"),
            observed_environment_id: ContentId::derive(b"environment")
                .expect("environment identity"),
            resolved_program: ResolvedProgramIdentity::new("sha256:program")
                .expect("program identity"),
            observations: vec![
                ExecutionObservation::new("z").expect("observation"),
                ExecutionObservation::new("a").expect("observation"),
            ],
        };
        let bytes = cairn_codec::to_vec(&evidence).expect("encode");
        let decoded: TrustedExecutionEvidence = cairn_codec::from_slice(&bytes).expect("decode");
        assert_eq!(
            decoded.validate(),
            Err(ContractValueError::NonCanonicalObservations)
        );
    }
}
