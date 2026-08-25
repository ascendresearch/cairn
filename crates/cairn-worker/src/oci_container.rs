use std::{
    ffi::OsString,
    num::{NonZeroU16, NonZeroU64},
    path::{Component, Path, PathBuf},
};

use cairn_execution::{
    ContainerBinding, ContainerExitObservation, ContainerImageVolumeState, ContainerInspection,
    ContainerLifecycleRuntime, ContainerName, ContainerRuntimeError, ContainerSandboxPolicy,
    EnvironmentVariable, ExecutionEnvironmentArtifact, InputBundleArtifact, InputBundleEntry,
    InputBundleV1, JobContract, JobContractArtifact, NetworkPolicy, OCI_CONTAINER_BACKEND,
    OciExecutionEnvironmentV1, OciImageDigest, RuntimeContainerId, VerifiedAssignmentMaterials,
};
use cairn_protocol::{AttemptId, ContentId};
use thiserror::Error;

const CONTAINER_INPUT: &str = "/cairn/input";
const CONTAINER_WORK: &str = "/cairn/work";
const CONTAINER_OUTPUT: &str = "/cairn/output";
const CONTAINER_TEMPORARY: &str = "/tmp";
const CONTAINER_USER: &str = "65532:65532";
const MINIMUM_DOCKER_MEMORY_BYTES: u64 = 6 * 1024 * 1024;

/// Positive integer CPU ceiling applied by the container runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerLogicalCpuLimit(NonZeroU16);

impl ContainerLogicalCpuLimit {
    /// Creates an integer CPU ceiling.
    ///
    /// # Errors
    ///
    /// Rejects zero.
    pub fn new(value: u16) -> Result<Self, ContainerLaunchPlanError> {
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(ContainerLaunchPlanError::ZeroCpuLimit)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Docker-compatible hard memory and combined memory/swap ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerMemoryByteLimit(NonZeroU64);

impl ContainerMemoryByteLimit {
    /// Creates a memory ceiling accepted by the initial Docker-compatible adapter.
    ///
    /// # Errors
    ///
    /// Rejects values below Docker's six-MiB minimum or above its signed runtime range.
    pub fn new(value: u64) -> Result<Self, ContainerLaunchPlanError> {
        let value =
            NonZeroU64::new(value).ok_or(ContainerLaunchPlanError::MemoryLimitBelowMinimum)?;
        if value.get() < MINIMUM_DOCKER_MEMORY_BYTES {
            return Err(ContainerLaunchPlanError::MemoryLimitBelowMinimum);
        }
        if value.get() > i64::MAX as u64 {
            return Err(ContainerLaunchPlanError::MemoryLimitAboveMaximum);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Positive process-count ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerPidsLimit(NonZeroU64);

impl ContainerPidsLimit {
    /// Creates a bounded PID limit.
    ///
    /// # Errors
    ///
    /// Rejects zero, the runtime's `-1` unlimited sentinel, and values above its signed range.
    pub fn new(value: u64) -> Result<Self, ContainerLaunchPlanError> {
        let value = NonZeroU64::new(value).ok_or(ContainerLaunchPlanError::ZeroPidsLimit)?;
        if value.get() > i64::MAX as u64 {
            return Err(ContainerLaunchPlanError::PidsLimitAboveMaximum);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Positive byte ceiling for one writable tmpfs role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerWritableByteLimit(NonZeroU64);

impl ContainerWritableByteLimit {
    /// Creates a positive writable-space bound.
    ///
    /// # Errors
    ///
    /// Rejects zero and values above the runtime's signed byte range.
    pub fn new(value: u64) -> Result<Self, ContainerLaunchPlanError> {
        let value = NonZeroU64::new(value).ok_or(ContainerLaunchPlanError::ZeroWritableLimit)?;
        if value.get() > i64::MAX as u64 {
            return Err(ContainerLaunchPlanError::WritableLimitAboveMaximum);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Mandatory resource ceilings for the code-owned CPU sandbox policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContainerLaunchLimits {
    logical_cpus: ContainerLogicalCpuLimit,
    memory_bytes: ContainerMemoryByteLimit,
    pids: ContainerPidsLimit,
    work_bytes: ContainerWritableByteLimit,
    output_bytes: ContainerWritableByteLimit,
    temporary_bytes: ContainerWritableByteLimit,
}

impl ContainerLaunchLimits {
    #[must_use]
    pub const fn new(
        logical_cpus: ContainerLogicalCpuLimit,
        memory_bytes: ContainerMemoryByteLimit,
        pids: ContainerPidsLimit,
        work_bytes: ContainerWritableByteLimit,
        output_bytes: ContainerWritableByteLimit,
        temporary_bytes: ContainerWritableByteLimit,
    ) -> Self {
        Self {
            logical_cpus,
            memory_bytes,
            pids,
            work_bytes,
            output_bytes,
            temporary_bytes,
        }
    }

    #[must_use]
    pub const fn logical_cpus(self) -> ContainerLogicalCpuLimit {
        self.logical_cpus
    }

    #[must_use]
    pub const fn memory_bytes(self) -> ContainerMemoryByteLimit {
        self.memory_bytes
    }

    #[must_use]
    pub const fn pids(self) -> ContainerPidsLimit {
        self.pids
    }

    #[must_use]
    pub const fn work_bytes(self) -> ContainerWritableByteLimit {
        self.work_bytes
    }

    #[must_use]
    pub const fn output_bytes(self) -> ContainerWritableByteLimit {
        self.output_bytes
    }

    #[must_use]
    pub const fn temporary_bytes(self) -> ContainerWritableByteLimit {
        self.temporary_bytes
    }
}

/// Absolute backend-owned root. It is never itself mounted into a subject container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerStateRoot(PathBuf);

impl ContainerStateRoot {
    /// Creates one lexical absolute root suitable for Docker `--mount` rendering.
    ///
    /// # Errors
    ///
    /// Rejects root itself, relative/dot/parent components, non-UTF-8, commas, and controls.
    pub fn new(path: PathBuf) -> Result<Self, ContainerLaunchPlanError> {
        let text = path
            .to_str()
            .ok_or(ContainerLaunchPlanError::InvalidStateRoot)?;
        if path == Path::new("/")
            || !path.is_absolute()
            || text.contains(',')
            || text.chars().any(char::is_control)
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::CurDir | Component::ParentDir | Component::Prefix(_)
                )
            })
        {
            return Err(ContainerLaunchPlanError::InvalidStateRoot);
        }
        Ok(Self(path))
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    fn input_directory(&self, attempt_id: AttemptId) -> PathBuf {
        self.0.join(attempt_id.as_uuid().to_string()).join("input")
    }
}

/// Complete immutable create plan. It contains no runtime executable or mutation authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerLaunchPlan {
    name: ContainerName,
    binding: ContainerBinding,
    image: OciImageDigest,
    input_directory: PathBuf,
    variables: Vec<EnvironmentVariable>,
    program: String,
    arguments: Vec<String>,
    limits: ContainerLaunchLimits,
}

impl ContainerLaunchPlan {
    #[must_use]
    pub const fn name(&self) -> &ContainerName {
        &self.name
    }

    #[must_use]
    pub const fn binding(&self) -> &ContainerBinding {
        &self.binding
    }

    #[must_use]
    pub const fn image(&self) -> &OciImageDigest {
        &self.image
    }

    #[must_use]
    pub fn input_directory(&self) -> &Path {
        &self.input_directory
    }

    #[must_use]
    pub const fn limits(&self) -> ContainerLaunchLimits {
        self.limits
    }

    /// Renders deterministic arguments following a trusted Docker-compatible executable.
    ///
    /// Each vector element is one argv entry; no shell string is constructed.
    #[must_use]
    pub fn docker_create_arguments(&self) -> Vec<OsString> {
        let binding = &self.binding;
        let input_mount = format!(
            "type=bind,src={},dst={CONTAINER_INPUT},readonly,bind-propagation=private",
            self.input_directory.display()
        );
        let labels = [
            format!("io.cairn.attempt-id={}", binding.attempt_id()),
            format!("io.cairn.job-id={}", binding.job_id()),
            format!("io.cairn.contract-id={}", binding.contract_id()),
            format!("io.cairn.input-id={}", binding.input_bundle_id()),
            format!("io.cairn.environment-id={}", binding.environment_id()),
            format!(
                "io.cairn.sandbox-policy={}",
                binding.sandbox_policy().as_str()
            ),
        ];
        let mut arguments = vec![
            "container".into(),
            "create".into(),
            "--name".into(),
            self.name.as_str().into(),
        ];
        for label in labels {
            arguments.push("--label".into());
            arguments.push(label.into());
        }
        arguments.extend([
            "--read-only".into(),
            "--pull".into(),
            "never".into(),
            "--restart".into(),
            "no".into(),
            "--no-healthcheck".into(),
            "--network".into(),
            "none".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--user".into(),
            CONTAINER_USER.into(),
            "--cgroupns".into(),
            "private".into(),
            "--ipc".into(),
            "private".into(),
            "--pid".into(),
            "private".into(),
            "--pids-limit".into(),
            self.limits.pids().get().to_string().into(),
            "--cpus".into(),
            self.limits.logical_cpus().get().to_string().into(),
            "--memory".into(),
            self.limits.memory_bytes().get().to_string().into(),
            "--memory-swap".into(),
            self.limits.memory_bytes().get().to_string().into(),
            "--memory-swappiness".into(),
            "0".into(),
            "--mount".into(),
            input_mount.into(),
            "--tmpfs".into(),
            tmpfs_argument(CONTAINER_WORK, self.limits.work_bytes(), true).into(),
            "--tmpfs".into(),
            tmpfs_argument(CONTAINER_OUTPUT, self.limits.output_bytes(), false).into(),
            "--tmpfs".into(),
            tmpfs_argument(CONTAINER_TEMPORARY, self.limits.temporary_bytes(), false).into(),
            "--workdir".into(),
            CONTAINER_WORK.into(),
        ]);
        for variable in &self.variables {
            arguments.push("--env".into());
            arguments.push(format!("{}={}", variable.name().as_str(), variable.value()).into());
        }
        arguments.push("--entrypoint".into());
        arguments.push(self.program.as_str().into());
        arguments.push(self.image.as_str().into());
        arguments.extend(self.arguments.iter().map(OsString::from));
        arguments
    }
}

fn tmpfs_argument(target: &str, limit: ContainerWritableByteLimit, executable: bool) -> String {
    let noexec = if executable { "" } else { ",noexec" };
    format!(
        "{target}:rw,nosuid,nodev{noexec},size={},mode=0700,uid=65532,gid=65532",
        limit.get()
    )
}

/// Durable external-effect classification for an unfinished container-supervisor call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerSupervisorFailureClass {
    /// Inspection and preflight prove that no container was created by the initial call.
    NotStarted,
    /// A matching or conflicting container may exist or the subject may have executed.
    Ambiguous,
}

/// Recoverable CPU-container lifecycle failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContainerSupervisorError {
    #[error("container subject was not started: {0}")]
    NotStarted(String),
    #[error("container lifecycle requires reconciliation: {0}")]
    Ambiguous(String),
}

impl ContainerSupervisorError {
    #[must_use]
    pub const fn failure_class(&self) -> ContainerSupervisorFailureClass {
        match self {
            Self::NotStarted(_) => ContainerSupervisorFailureClass::NotStarted,
            Self::Ambiguous(_) => ContainerSupervisorFailureClass::Ambiguous,
        }
    }
}

#[derive(Clone, Copy)]
enum SupervisorEntry {
    Initial,
    Recovery,
}

/// Performs the only initial inspect/create/start/wait lifecycle for one immutable plan.
///
/// This entry point is for the first invocation only. An ambiguous result must be resumed through
/// [`recover_container_supervision`], never by reconstructing job or attempt authority.
///
/// # Errors
///
/// Returns `NotStarted` only while the deterministic name is absent and no mutation may have been
/// applied. Once a container exists or a mutation response is uncertain, returns `Ambiguous`.
pub fn start_container_supervision<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    supervise_container(runtime, plan, SupervisorEntry::Initial)
}

/// Reconciles one previously entered lifecycle from the deterministic runtime state.
///
/// Recovery may create an absent container or start the exact matching `Created` container. Name
/// uniqueness and full identity checks ensure those retries converge on one runtime subject. It
/// never starts an `Exited` container and never mutates a conflicting container.
///
/// # Errors
///
/// Any inability to reconcile a recovery call is `Ambiguous`; recovery never grants a fresh
/// attempt or claims that the prior invocation had no effect.
pub fn recover_container_supervision<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    supervise_container(runtime, plan, SupervisorEntry::Recovery)
}

fn supervise_container<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
    entry: SupervisorEntry,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    let inspection = runtime
        .inspect(plan.name())
        .map_err(|error| entry_error(entry, "initial inspect", &error))?;
    validate_name(&inspection, plan)?;
    match inspection {
        ContainerInspection::Absent { .. } => create_then_advance(runtime, plan, entry),
        present => advance_present(runtime, plan, present),
    }
}

fn create_then_advance<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
    entry: SupervisorEntry,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    let image = runtime
        .resolve_image(plan.image())
        .map_err(|error| entry_error(entry, "image preflight", &error))?;
    if image.image() != plan.image() {
        return Err(entry_message(
            entry,
            "resolved image identity differs from the immutable launch plan",
        ));
    }
    if image.volume_state() != ContainerImageVolumeState::None {
        return Err(entry_message(
            entry,
            "image declares volumes that would synthesize writable mounts",
        ));
    }
    match runtime.create(plan) {
        Ok(()) => {}
        Err(ContainerRuntimeError::Ambiguous(diagnostic)) => {
            return Err(ContainerSupervisorError::Ambiguous(format!(
                "create response is unknown: {diagnostic}"
            )));
        }
        Err(error) => {
            let inspection = runtime.inspect(plan.name()).map_err(|inspect_error| {
                ContainerSupervisorError::Ambiguous(format!(
                    "create failed definitively but reconciliation inspect failed: {inspect_error}"
                ))
            })?;
            validate_name(&inspection, plan)?;
            return match inspection {
                ContainerInspection::Absent { .. } => Err(entry_error(entry, "create", &error)),
                present => advance_present(runtime, plan, present),
            };
        }
    }
    let inspection = runtime.inspect(plan.name()).map_err(|error| {
        ContainerSupervisorError::Ambiguous(format!("post-create inspection failed: {error}"))
    })?;
    validate_name(&inspection, plan)?;
    match inspection {
        ContainerInspection::Absent { .. } => Err(ContainerSupervisorError::Ambiguous(
            "runtime reported success but the created container is absent".into(),
        )),
        present => advance_present(runtime, plan, present),
    }
}

fn advance_present<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
    inspection: ContainerInspection,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    validate_binding(&inspection, plan, None)?;
    match inspection {
        ContainerInspection::Created { runtime_id, .. } => {
            runtime
                .start(plan.name(), &runtime_id)
                .map_err(|error| mutation_error("start", &error))?;
            let inspection = runtime.inspect(plan.name()).map_err(|error| {
                ContainerSupervisorError::Ambiguous(format!(
                    "post-start inspection failed: {error}"
                ))
            })?;
            validate_binding(&inspection, plan, Some(&runtime_id))?;
            match inspection {
                ContainerInspection::Running { .. } => wait_for_exit(runtime, plan, &runtime_id),
                ContainerInspection::Exited { .. } => exited_container(inspection),
                ContainerInspection::Absent { .. } | ContainerInspection::Created { .. } => {
                    Err(ContainerSupervisorError::Ambiguous(
                        "runtime accepted start without observing running or exited state".into(),
                    ))
                }
            }
        }
        ContainerInspection::Running { runtime_id, .. } => {
            wait_for_exit(runtime, plan, &runtime_id)
        }
        ContainerInspection::Exited { .. } => exited_container(inspection),
        ContainerInspection::Absent { .. } => Err(ContainerSupervisorError::Ambiguous(
            "present-container transition received absent state".into(),
        )),
    }
}

fn wait_for_exit<R>(
    runtime: &mut R,
    plan: &ContainerLaunchPlan,
    runtime_id: &RuntimeContainerId,
) -> Result<ContainerExitObservation, ContainerSupervisorError>
where
    R: ContainerLifecycleRuntime<LaunchPlan = ContainerLaunchPlan>,
{
    let observation = runtime
        .wait(plan.name(), runtime_id)
        .map_err(|error| mutation_error("wait", &error))?;
    validate_exit_observation(&observation, plan, runtime_id)?;
    Ok(observation)
}

fn validate_name(
    inspection: &ContainerInspection,
    plan: &ContainerLaunchPlan,
) -> Result<(), ContainerSupervisorError> {
    if inspection.name() != plan.name() {
        return Err(ContainerSupervisorError::Ambiguous(
            "runtime inspection returned another deterministic name".into(),
        ));
    }
    Ok(())
}

fn validate_binding(
    inspection: &ContainerInspection,
    plan: &ContainerLaunchPlan,
    expected_runtime_id: Option<&RuntimeContainerId>,
) -> Result<(), ContainerSupervisorError> {
    validate_name(inspection, plan)?;
    if inspection.binding() != Some(plan.binding()) {
        return Err(ContainerSupervisorError::Ambiguous(
            "container name is occupied by a conflicting immutable binding".into(),
        ));
    }
    let Some(runtime_id) = inspection.runtime_id() else {
        return Err(ContainerSupervisorError::Ambiguous(
            "present container omitted its full runtime identity".into(),
        ));
    };
    if expected_runtime_id.is_some_and(|expected| runtime_id != expected) {
        return Err(ContainerSupervisorError::Ambiguous(
            "container runtime identity changed during reconciliation".into(),
        ));
    }
    Ok(())
}

fn exited_container(
    inspection: ContainerInspection,
) -> Result<ContainerExitObservation, ContainerSupervisorError> {
    let ContainerInspection::Exited {
        name,
        runtime_id,
        binding,
        exit_code,
    } = inspection
    else {
        return Err(ContainerSupervisorError::Ambiguous(
            "terminal conversion received a non-exited phase".into(),
        ));
    };
    Ok(ContainerExitObservation::new(
        name, runtime_id, binding, exit_code,
    ))
}

fn validate_exit_observation(
    observation: &ContainerExitObservation,
    plan: &ContainerLaunchPlan,
    expected_runtime_id: &RuntimeContainerId,
) -> Result<(), ContainerSupervisorError> {
    if observation.name() != plan.name()
        || observation.binding() != plan.binding()
        || observation.runtime_id() != expected_runtime_id
    {
        return Err(ContainerSupervisorError::Ambiguous(
            "wait returned a conflicting terminal container identity".into(),
        ));
    }
    Ok(())
}

fn entry_error(
    entry: SupervisorEntry,
    operation: &str,
    error: &ContainerRuntimeError,
) -> ContainerSupervisorError {
    if matches!(error, ContainerRuntimeError::Ambiguous(_)) {
        return ContainerSupervisorError::Ambiguous(format!("{operation} is unknown: {error}"));
    }
    entry_message(entry, format!("{operation} failed: {error}"))
}

fn entry_message(
    entry: SupervisorEntry,
    diagnostic: impl Into<String>,
) -> ContainerSupervisorError {
    match entry {
        SupervisorEntry::Initial => ContainerSupervisorError::NotStarted(diagnostic.into()),
        SupervisorEntry::Recovery => ContainerSupervisorError::Ambiguous(diagnostic.into()),
    }
}

fn mutation_error(operation: &str, error: &ContainerRuntimeError) -> ContainerSupervisorError {
    ContainerSupervisorError::Ambiguous(format!("{operation} failed after creation: {error}"))
}

/// Builds the sole admitted CPU-only create plan from worker-verified exact material.
///
/// # Errors
///
/// Rejects identity mismatches, mutable/invalid OCI environment bytes, non-OCI backend or network,
/// device requests, reserved input paths, unsafe command/output layout, and insufficient ceilings.
#[expect(
    clippy::too_many_arguments,
    reason = "attempt authority, exact identities, verified bytes, state ownership, and limits remain explicit"
)]
pub fn build_container_launch_plan(
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    contract: &JobContract,
    materials: &VerifiedAssignmentMaterials,
    input_bundle_bytes: &[u8],
    environment_bytes: &[u8],
    state_root: &ContainerStateRoot,
    limits: ContainerLaunchLimits,
) -> Result<ContainerLaunchPlan, ContainerLaunchPlanError> {
    validate_identities(
        contract_id,
        contract,
        materials,
        input_bundle_bytes,
        environment_bytes,
    )?;
    if contract.backend().as_str() != OCI_CONTAINER_BACKEND {
        return Err(ContainerLaunchPlanError::BackendMismatch);
    }
    if contract.network() != NetworkPolicy::Disabled {
        return Err(ContainerLaunchPlanError::NetworkNotDisabled);
    }
    let quantitative = contract.resources().quantitative();
    if quantitative.accelerator().is_some() || quantitative.require_complete_accelerator_discovery()
    {
        return Err(ContainerLaunchPlanError::DeviceRequestRejected);
    }
    if quantitative
        .minimum_logical_cpus()
        .is_some_and(|minimum| u64::from(limits.logical_cpus().get()) < minimum.get())
    {
        return Err(ContainerLaunchPlanError::CpuLimitBelowRequest);
    }
    if quantitative
        .minimum_memory_bytes()
        .is_some_and(|minimum| limits.memory_bytes().get() < minimum.get())
    {
        return Err(ContainerLaunchPlanError::MemoryLimitBelowRequest);
    }
    if quantitative
        .minimum_scratch_bytes()
        .is_some_and(|minimum| limits.work_bytes().get() < minimum.get())
    {
        return Err(ContainerLaunchPlanError::WorkLimitBelowRequest);
    }

    let bundle = InputBundleV1::from_bytes(input_bundle_bytes)
        .map_err(|error| ContainerLaunchPlanError::InputFormat(error.to_string()))?;
    validate_input_layout(&bundle, contract)?;
    validate_output_layout(contract, limits.output_bytes())?;
    let environment = OciExecutionEnvironmentV1::from_bytes(environment_bytes)
        .map_err(|error| ContainerLaunchPlanError::EnvironmentFormat(error.to_string()))?;
    let binding = ContainerBinding::new(
        attempt_id,
        contract.job_id(),
        contract_id,
        contract.input_bundle_id(),
        contract.environment_id(),
        ContainerSandboxPolicy::CpuUntrustedV1,
    );
    Ok(ContainerLaunchPlan {
        name: ContainerName::for_attempt(attempt_id),
        binding,
        image: environment.image().clone(),
        input_directory: state_root.input_directory(attempt_id),
        variables: environment.variables().to_vec(),
        program: format!(
            "{CONTAINER_INPUT}/{}",
            contract.command().program().as_str()
        ),
        arguments: contract
            .command()
            .arguments()
            .iter()
            .map(|argument| argument.as_str().to_owned())
            .collect(),
        limits,
    })
}

fn validate_identities(
    contract_id: ContentId<JobContractArtifact>,
    contract: &JobContract,
    materials: &VerifiedAssignmentMaterials,
    input_bundle_bytes: &[u8],
    environment_bytes: &[u8],
) -> Result<(), ContainerLaunchPlanError> {
    let contract_bytes = cairn_codec::to_vec(contract)
        .map_err(|error| ContainerLaunchPlanError::Codec(error.to_string()))?;
    if ContentId::derive(&contract_bytes) != Ok(contract_id) {
        return Err(ContainerLaunchPlanError::ContractIdentityMismatch);
    }
    let input_id = ContentId::<InputBundleArtifact>::derive(input_bundle_bytes)
        .map_err(|error| ContainerLaunchPlanError::Codec(error.to_string()))?;
    let environment_id = ContentId::<ExecutionEnvironmentArtifact>::derive(environment_bytes)
        .map_err(|error| ContainerLaunchPlanError::Codec(error.to_string()))?;
    if materials.input_bundle_id() != contract.input_bundle_id()
        || materials.environment_id() != contract.environment_id()
        || input_id != materials.input_bundle_id()
        || environment_id != materials.environment_id()
    {
        return Err(ContainerLaunchPlanError::MaterialIdentityMismatch);
    }
    Ok(())
}

fn validate_input_layout(
    bundle: &InputBundleV1,
    contract: &JobContract,
) -> Result<(), ContainerLaunchPlanError> {
    if contract.command().working_directory().as_str() != "work" {
        return Err(ContainerLaunchPlanError::InvalidWorkingDirectory);
    }
    let program = contract.command().program();
    let mut executable_program = false;
    for entry in bundle.entries() {
        let first = entry.path().as_str().split('/').next().unwrap_or_default();
        if matches!(first, "work" | "output" | "tmp") {
            return Err(ContainerLaunchPlanError::ReservedInputPath(
                entry.path().as_str().to_owned(),
            ));
        }
        if entry.path() == program {
            executable_program = matches!(
                entry,
                InputBundleEntry::File {
                    mode: cairn_execution::InputFileMode::Executable,
                    ..
                }
            );
        }
    }
    if !executable_program {
        return Err(ContainerLaunchPlanError::ProgramNotExecutableInput);
    }
    Ok(())
}

fn validate_output_layout(
    contract: &JobContract,
    output_limit: ContainerWritableByteLimit,
) -> Result<(), ContainerLaunchPlanError> {
    let mut total = 0_u64;
    for output in contract.capture().expected_outputs() {
        if !output.path.as_str().starts_with("output/") {
            return Err(ContainerLaunchPlanError::OutputOutsideOutputMount(
                output.path.as_str().to_owned(),
            ));
        }
        total = total
            .checked_add(output.byte_limit.get())
            .ok_or(ContainerLaunchPlanError::OutputBoundsOverflow)?;
    }
    if total > output_limit.get() {
        return Err(ContainerLaunchPlanError::OutputLimitBelowContract);
    }
    Ok(())
}

/// Invalid fixed CPU-only launch plan.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContainerLaunchPlanError {
    #[error("container CPU limit must be positive")]
    ZeroCpuLimit,
    #[error("container memory limit is below the Docker-compatible minimum")]
    MemoryLimitBelowMinimum,
    #[error("container memory limit exceeds the Docker-compatible maximum")]
    MemoryLimitAboveMaximum,
    #[error("container PID limit must be positive")]
    ZeroPidsLimit,
    #[error("container PID limit exceeds the Docker-compatible maximum")]
    PidsLimitAboveMaximum,
    #[error("container writable tmpfs limit must be positive")]
    ZeroWritableLimit,
    #[error("container writable tmpfs limit exceeds the Docker-compatible maximum")]
    WritableLimitAboveMaximum,
    #[error("container state root must be a safe absolute UTF-8 path other than root")]
    InvalidStateRoot,
    #[error("container contract identity does not match its canonical bytes")]
    ContractIdentityMismatch,
    #[error("container material bytes do not match worker-verified identities")]
    MaterialIdentityMismatch,
    #[error("container plan requires backend oci-container-v1")]
    BackendMismatch,
    #[error("container plan requires network=disabled")]
    NetworkNotDisabled,
    #[error("CPU-only container policy rejects all device requests")]
    DeviceRequestRejected,
    #[error("container CPU ceiling is below the contract minimum")]
    CpuLimitBelowRequest,
    #[error("container memory ceiling is below the contract minimum")]
    MemoryLimitBelowRequest,
    #[error("container work tmpfs ceiling is below the contract scratch minimum")]
    WorkLimitBelowRequest,
    #[error("input bundle path is reserved for a writable container mount: {0}")]
    ReservedInputPath(String),
    #[error("OCI container working directory must be exactly work")]
    InvalidWorkingDirectory,
    #[error("declared program is not one executable input-bundle file")]
    ProgramNotExecutableInput,
    #[error("declared output is outside the output mount: {0}")]
    OutputOutsideOutputMount(String),
    #[error("declared output byte bounds overflow")]
    OutputBoundsOverflow,
    #[error("output tmpfs ceiling is below summed declared output bounds")]
    OutputLimitBelowContract,
    #[error("input bundle format failed: {0}")]
    InputFormat(String),
    #[error("OCI environment format failed: {0}")]
    EnvironmentFormat(String),
    #[error("container plan canonical encoding failed: {0}")]
    Codec(String),
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_execution::{
        AcceleratorDeviceCount, AcceleratorResourceRequest, AssignmentMaterialChunkSize,
        CapturePolicy, CommandArgument, CommandContract, ContainerExitCode, ContainerPhase,
        ContainerRuntime, DiagnosticByteLimit, EnvironmentVariableName, EvidenceByteLimit,
        ExecutionBackend, ExecutionPlatformRequirement, ExecutionTimeoutMillis, ExpectedOutput,
        InputFileMode, LogicalCpuCount, MemoryByteCount, OutputByteLimit, OutputName,
        PlacementRequest, QuantitativeResourceRequest, ResolvedContainerImage, ResourceRequest,
        SandboxPath, ScratchByteCount, load_assignment_material_manifest,
        verify_persisted_assignment_materials,
    };
    use cairn_protocol::JobId;
    use cairn_record::ContentStore;
    use cairn_store_sqlite::SqliteContentStore;
    use tempfile::TempDir;

    use super::*;

    const IMAGE: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    struct Fixture {
        _directory: TempDir,
        contract: JobContract,
        contract_id: ContentId<JobContractArtifact>,
        materials: VerifiedAssignmentMaterials,
        input_bytes: Vec<u8>,
        environment_bytes: Vec<u8>,
        state_root: ContainerStateRoot,
        limits: ContainerLaunchLimits,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("temporary directory");
            let bundle = normal_bundle();
            let input_bytes = bundle.to_bytes().expect("input bytes");
            let environment = OciExecutionEnvironmentV1::new(
                OciImageDigest::new(IMAGE).expect("image"),
                vec![
                    EnvironmentVariable::new(
                        EnvironmentVariableName::new("ZED").expect("name"),
                        "two words".into(),
                    ),
                    EnvironmentVariable::new(
                        EnvironmentVariableName::new("ALPHA").expect("name"),
                        "one".into(),
                    ),
                ],
            )
            .expect("environment");
            let environment_bytes = environment.to_bytes().expect("environment bytes");
            let mut content = SqliteContentStore::open(
                directory.path().join("content.sqlite3"),
                directory.path().join("content"),
            )
            .expect("content store");
            let input_id = content
                .put::<InputBundleArtifact>(&mut Cursor::new(&input_bytes))
                .expect("put input")
                .content_id;
            let environment_id = content
                .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(&environment_bytes))
                .expect("put environment")
                .content_id;
            let contract = make_contract(
                input_id,
                environment_id,
                OCI_CONTAINER_BACKEND,
                NetworkPolicy::Disabled,
                quantitative(None),
                "work",
                "output/result.bin",
            );
            let contract_bytes = cairn_codec::to_vec(&contract).expect("contract bytes");
            let contract_id = ContentId::derive(&contract_bytes).expect("contract ID");
            let manifest = load_assignment_material_manifest(
                &content,
                &contract,
                AssignmentMaterialChunkSize::new(4096).expect("chunk size"),
                None,
            )
            .expect("manifest");
            let materials =
                verify_persisted_assignment_materials(&content, &contract, &manifest, None)
                    .expect("verified materials");
            let state_root =
                ContainerStateRoot::new(directory.path().join("oci-state")).expect("state root");
            Self {
                _directory: directory,
                contract,
                contract_id,
                materials,
                input_bytes,
                environment_bytes,
                state_root,
                limits: limits(2, 32 * 1024 * 1024, 64, 2 * 1024 * 1024),
            }
        }

        fn build(
            &self,
            attempt_id: AttemptId,
        ) -> Result<ContainerLaunchPlan, ContainerLaunchPlanError> {
            build_container_launch_plan(
                attempt_id,
                self.contract_id,
                &self.contract,
                &self.materials,
                &self.input_bytes,
                &self.environment_bytes,
                &self.state_root,
                self.limits,
            )
        }
    }

    fn normal_bundle() -> InputBundleV1 {
        InputBundleV1::new(vec![
            InputBundleEntry::Directory {
                path: SandboxPath::new("bin").expect("bin"),
            },
            InputBundleEntry::File {
                path: SandboxPath::new("bin/run").expect("program"),
                mode: InputFileMode::Executable,
                bytes: b"fixture".to_vec(),
            },
        ])
        .expect("bundle")
    }

    fn quantitative(
        accelerator: Option<AcceleratorResourceRequest>,
    ) -> QuantitativeResourceRequest {
        QuantitativeResourceRequest::new(
            Some(LogicalCpuCount::new(1).expect("CPU")),
            Some(MemoryByteCount::new(16 * 1024 * 1024).expect("memory")),
            Some(ScratchByteCount::new(1024 * 1024).expect("scratch")),
            accelerator,
            false,
        )
    }

    fn make_contract(
        input_id: ContentId<InputBundleArtifact>,
        environment_id: ContentId<ExecutionEnvironmentArtifact>,
        backend: &str,
        network: NetworkPolicy,
        quantitative: QuantitativeResourceRequest,
        working_directory: &str,
        output_path: &str,
    ) -> JobContract {
        JobContract::new(
            JobId::new(),
            input_id,
            environment_id,
            ExecutionBackend::new(backend).expect("backend"),
            CommandContract::new(
                SandboxPath::new("bin/run").expect("program"),
                vec![
                    CommandArgument::new("--fixture").expect("argument"),
                    CommandArgument::new("value with spaces").expect("argument"),
                ],
                SandboxPath::new(working_directory).expect("working directory"),
            ),
            ResourceRequest::new_with_quantitative(
                ExecutionTimeoutMillis::new(30_000).expect("timeout"),
                PlacementRequest::new(
                    ExecutionPlatformRequirement::default(),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("placement"),
                quantitative,
            )
            .expect("resources"),
            network,
            CapturePolicy::new(
                OutputByteLimit::new(4096).expect("stdout"),
                OutputByteLimit::new(4096).expect("stderr"),
                DiagnosticByteLimit::new(4096).expect("diagnostic"),
                EvidenceByteLimit::new(8192).expect("evidence"),
                vec![ExpectedOutput {
                    name: OutputName::new("result").expect("output name"),
                    path: SandboxPath::new(output_path).expect("output path"),
                    byte_limit: OutputByteLimit::new(1024).expect("output bound"),
                }],
            )
            .expect("capture"),
        )
    }

    fn limits(cpus: u16, memory: u64, pids: u64, work: u64) -> ContainerLaunchLimits {
        ContainerLaunchLimits::new(
            ContainerLogicalCpuLimit::new(cpus).expect("CPU limit"),
            ContainerMemoryByteLimit::new(memory).expect("memory limit"),
            ContainerPidsLimit::new(pids).expect("PID limit"),
            ContainerWritableByteLimit::new(work).expect("work limit"),
            ContainerWritableByteLimit::new(2 * 1024 * 1024).expect("output limit"),
            ContainerWritableByteLimit::new(1024 * 1024).expect("temporary limit"),
        )
    }

    fn strings(arguments: Vec<OsString>) -> Vec<String> {
        arguments
            .into_iter()
            .map(|argument| argument.into_string().expect("UTF-8 argv"))
            .collect()
    }

    #[derive(Clone, Copy)]
    enum MutationFault {
        AmbiguousBeforeEffect,
        AmbiguousAfterEffect,
        Rejected,
        RejectedAfterConcurrentCreate,
    }

    struct FakeRuntime {
        image: ResolvedContainerImage,
        inspection: ContainerInspection,
        runtime_id: RuntimeContainerId,
        resolve_error: Option<ContainerRuntimeError>,
        create_fault: Option<MutationFault>,
        start_fault: Option<MutationFault>,
        wait_fault: Option<MutationFault>,
        replace_runtime_id_after_start: bool,
        create_calls: u64,
        successful_creates: u64,
        start_calls: u64,
        successful_starts: u64,
        wait_calls: u64,
    }

    impl FakeRuntime {
        fn absent(plan: &ContainerLaunchPlan) -> Self {
            Self {
                image: ResolvedContainerImage::new(
                    plan.image().clone(),
                    ContainerImageVolumeState::None,
                ),
                inspection: ContainerInspection::Absent {
                    name: plan.name().clone(),
                },
                runtime_id: RuntimeContainerId::new(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .expect("runtime ID"),
                resolve_error: None,
                create_fault: None,
                start_fault: None,
                wait_fault: None,
                replace_runtime_id_after_start: false,
                create_calls: 0,
                successful_creates: 0,
                start_calls: 0,
                successful_starts: 0,
                wait_calls: 0,
            }
        }

        fn apply_create(&mut self, plan: &ContainerLaunchPlan) {
            assert_eq!(self.inspection.phase(), ContainerPhase::Absent);
            self.successful_creates += 1;
            self.inspection = ContainerInspection::Created {
                name: plan.name().clone(),
                runtime_id: self.runtime_id.clone(),
                binding: plan.binding().clone(),
            };
        }

        fn apply_start(&mut self) {
            let ContainerInspection::Created {
                name,
                runtime_id,
                binding,
            } = &self.inspection
            else {
                panic!("start requires created state");
            };
            self.successful_starts += 1;
            let runtime_id = if self.replace_runtime_id_after_start {
                RuntimeContainerId::new(
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                )
                .expect("replacement runtime ID")
            } else {
                runtime_id.clone()
            };
            self.inspection = ContainerInspection::Running {
                name: name.clone(),
                runtime_id,
                binding: binding.clone(),
            };
        }

        fn apply_exit(&mut self) {
            let (name, runtime_id, binding) = match &self.inspection {
                ContainerInspection::Running {
                    name,
                    runtime_id,
                    binding,
                }
                | ContainerInspection::Exited {
                    name,
                    runtime_id,
                    binding,
                    ..
                } => (name.clone(), runtime_id.clone(), binding.clone()),
                _ => panic!("exit requires running state"),
            };
            self.inspection = ContainerInspection::Exited {
                name,
                runtime_id,
                binding,
                exit_code: ContainerExitCode::new(0),
            };
        }

        fn exit_observation(&self) -> ContainerExitObservation {
            let ContainerInspection::Exited {
                name,
                runtime_id,
                binding,
                exit_code,
            } = &self.inspection
            else {
                panic!("terminal observation requires exited state");
            };
            ContainerExitObservation::new(
                name.clone(),
                runtime_id.clone(),
                binding.clone(),
                *exit_code,
            )
        }
    }

    impl ContainerRuntime for FakeRuntime {
        fn resolve_image(
            &mut self,
            _requested: &OciImageDigest,
        ) -> Result<ResolvedContainerImage, ContainerRuntimeError> {
            if let Some(error) = self.resolve_error.take() {
                return Err(error);
            }
            Ok(self.image.clone())
        }

        fn inspect(
            &mut self,
            _name: &ContainerName,
        ) -> Result<ContainerInspection, ContainerRuntimeError> {
            Ok(self.inspection.clone())
        }
    }

    impl ContainerLifecycleRuntime for FakeRuntime {
        type LaunchPlan = ContainerLaunchPlan;

        fn create(&mut self, plan: &Self::LaunchPlan) -> Result<(), ContainerRuntimeError> {
            self.create_calls += 1;
            match self.create_fault.take() {
                Some(MutationFault::AmbiguousBeforeEffect) => Err(
                    ContainerRuntimeError::Ambiguous("create response lost before effect".into()),
                ),
                Some(MutationFault::AmbiguousAfterEffect) => {
                    self.apply_create(plan);
                    Err(ContainerRuntimeError::Ambiguous(
                        "create response lost after effect".into(),
                    ))
                }
                Some(MutationFault::Rejected) => {
                    Err(ContainerRuntimeError::Rejected("create rejected".into()))
                }
                Some(MutationFault::RejectedAfterConcurrentCreate) => {
                    self.apply_create(plan);
                    Err(ContainerRuntimeError::Rejected(
                        "another reconciler won the create race".into(),
                    ))
                }
                None => {
                    self.apply_create(plan);
                    Ok(())
                }
            }
        }

        fn start(
            &mut self,
            name: &ContainerName,
            runtime_id: &RuntimeContainerId,
        ) -> Result<(), ContainerRuntimeError> {
            self.start_calls += 1;
            assert_eq!(self.inspection.name(), name);
            assert_eq!(self.inspection.runtime_id(), Some(runtime_id));
            match self.start_fault.take() {
                Some(MutationFault::AmbiguousBeforeEffect) => Err(
                    ContainerRuntimeError::Ambiguous("start response lost before effect".into()),
                ),
                Some(MutationFault::AmbiguousAfterEffect) => {
                    self.apply_start();
                    Err(ContainerRuntimeError::Ambiguous(
                        "start response lost after effect".into(),
                    ))
                }
                Some(MutationFault::Rejected) => {
                    Err(ContainerRuntimeError::Rejected("start rejected".into()))
                }
                Some(MutationFault::RejectedAfterConcurrentCreate) => {
                    unreachable!("concurrent-create fault is create-only")
                }
                None => {
                    self.apply_start();
                    Ok(())
                }
            }
        }

        fn wait(
            &mut self,
            name: &ContainerName,
            runtime_id: &RuntimeContainerId,
        ) -> Result<ContainerExitObservation, ContainerRuntimeError> {
            self.wait_calls += 1;
            assert_eq!(self.inspection.name(), name);
            assert_eq!(self.inspection.runtime_id(), Some(runtime_id));
            match self.wait_fault.take() {
                Some(MutationFault::AmbiguousBeforeEffect) => {
                    Err(ContainerRuntimeError::Ambiguous(
                        "wait response unavailable before exit".into(),
                    ))
                }
                Some(MutationFault::AmbiguousAfterEffect) => {
                    self.apply_exit();
                    Err(ContainerRuntimeError::Ambiguous(
                        "wait response lost after exit".into(),
                    ))
                }
                Some(MutationFault::Rejected) => {
                    Err(ContainerRuntimeError::Rejected("wait rejected".into()))
                }
                Some(MutationFault::RejectedAfterConcurrentCreate) => {
                    unreachable!("concurrent-create fault is create-only")
                }
                None => {
                    self.apply_exit();
                    Ok(self.exit_observation())
                }
            }
        }
    }

    #[test]
    fn supervisor_runs_one_subject_and_exited_recovery_is_observational() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");
        let mut runtime = FakeRuntime::absent(&plan);

        let exited = start_container_supervision(&mut runtime, &plan).expect("terminal exit");
        assert_eq!(exited.name(), plan.name());
        assert_eq!(exited.binding(), plan.binding());
        assert_eq!(exited.runtime_id(), &runtime.runtime_id);
        assert_eq!(exited.exit_code().get(), 0);
        assert_eq!(runtime.create_calls, 1);
        assert_eq!(runtime.successful_creates, 1);
        assert_eq!(runtime.start_calls, 1);
        assert_eq!(runtime.successful_starts, 1);
        assert_eq!(runtime.wait_calls, 1);

        assert_eq!(
            recover_container_supervision(&mut runtime, &plan).expect("recover exit"),
            exited
        );
        assert_eq!(runtime.create_calls, 1);
        assert_eq!(runtime.start_calls, 1);
        assert_eq!(runtime.wait_calls, 1);
    }

    #[test]
    fn ambiguous_create_recovery_converges_on_one_created_subject() {
        for fault in [
            MutationFault::AmbiguousBeforeEffect,
            MutationFault::AmbiguousAfterEffect,
        ] {
            let fixture = Fixture::new();
            let plan = fixture.build(AttemptId::new()).expect("launch plan");
            let mut runtime = FakeRuntime::absent(&plan);
            runtime.create_fault = Some(fault);

            let error = start_container_supervision(&mut runtime, &plan).expect_err("ambiguous");
            assert_eq!(
                error.failure_class(),
                ContainerSupervisorFailureClass::Ambiguous
            );
            let exited =
                recover_container_supervision(&mut runtime, &plan).expect("reconciled exit");
            assert_eq!(exited.binding(), plan.binding());
            assert_eq!(runtime.successful_creates, 1);
            assert_eq!(runtime.successful_starts, 1);
            assert_eq!(runtime.start_calls, 1);
            assert!(runtime.create_calls <= 2);
        }
    }

    #[test]
    fn definitive_create_race_reinspects_and_uses_the_single_matching_container() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");
        let mut runtime = FakeRuntime::absent(&plan);
        runtime.create_fault = Some(MutationFault::RejectedAfterConcurrentCreate);

        start_container_supervision(&mut runtime, &plan).expect("race reconciliation");
        assert_eq!(runtime.create_calls, 1);
        assert_eq!(runtime.successful_creates, 1);
        assert_eq!(runtime.start_calls, 1);
        assert_eq!(runtime.successful_starts, 1);
    }

    #[test]
    fn ambiguous_start_recovery_never_starts_the_subject_twice() {
        for fault in [
            MutationFault::AmbiguousBeforeEffect,
            MutationFault::AmbiguousAfterEffect,
        ] {
            let fixture = Fixture::new();
            let plan = fixture.build(AttemptId::new()).expect("launch plan");
            let mut runtime = FakeRuntime::absent(&plan);
            runtime.start_fault = Some(fault);

            assert_eq!(
                start_container_supervision(&mut runtime, &plan)
                    .expect_err("ambiguous")
                    .failure_class(),
                ContainerSupervisorFailureClass::Ambiguous
            );
            recover_container_supervision(&mut runtime, &plan).expect("reconciled exit");
            assert_eq!(runtime.successful_creates, 1);
            assert_eq!(runtime.successful_starts, 1);
            assert!(runtime.start_calls <= 2);
        }
    }

    #[test]
    fn completed_while_wait_response_is_lost_recovers_without_restart() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");
        let mut runtime = FakeRuntime::absent(&plan);
        runtime.wait_fault = Some(MutationFault::AmbiguousAfterEffect);

        assert_eq!(
            start_container_supervision(&mut runtime, &plan)
                .expect_err("ambiguous")
                .failure_class(),
            ContainerSupervisorFailureClass::Ambiguous
        );
        assert_eq!(runtime.inspection.phase(), ContainerPhase::Exited);
        recover_container_supervision(&mut runtime, &plan).expect("recover completed result");
        assert_eq!(runtime.successful_starts, 1);
        assert_eq!(runtime.start_calls, 1);
        assert_eq!(runtime.wait_calls, 1);
    }

    #[test]
    fn running_subject_completed_during_disconnect_recovers_terminal_state() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");
        let mut runtime = FakeRuntime::absent(&plan);
        runtime.wait_fault = Some(MutationFault::AmbiguousBeforeEffect);

        start_container_supervision(&mut runtime, &plan).expect_err("wait unavailable");
        assert_eq!(runtime.inspection.phase(), ContainerPhase::Running);
        runtime.apply_exit();
        recover_container_supervision(&mut runtime, &plan).expect("recover completed result");
        assert_eq!(runtime.successful_starts, 1);
        assert_eq!(runtime.start_calls, 1);
        assert_eq!(runtime.wait_calls, 1);
    }

    #[test]
    fn conflicting_binding_fails_closed_without_mutation() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");
        let mut runtime = FakeRuntime::absent(&plan);
        let conflict = ContainerBinding::new(
            plan.binding().attempt_id(),
            JobId::new(),
            plan.binding().contract_id(),
            plan.binding().input_bundle_id(),
            plan.binding().environment_id(),
            ContainerSandboxPolicy::CpuUntrustedV1,
        );
        runtime.inspection = ContainerInspection::Created {
            name: plan.name().clone(),
            runtime_id: runtime.runtime_id.clone(),
            binding: conflict,
        };

        assert_eq!(
            recover_container_supervision(&mut runtime, &plan)
                .expect_err("binding conflict")
                .failure_class(),
            ContainerSupervisorFailureClass::Ambiguous
        );
        assert_eq!(runtime.create_calls, 0);
        assert_eq!(runtime.start_calls, 0);
        assert_eq!(runtime.wait_calls, 0);
        assert_eq!(runtime.inspection.phase(), ContainerPhase::Created);
    }

    #[test]
    fn changed_name_or_runtime_identity_fails_closed() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");

        let mut wrong_name = FakeRuntime::absent(&plan);
        wrong_name.inspection = ContainerInspection::Created {
            name: ContainerName::for_attempt(AttemptId::new()),
            runtime_id: wrong_name.runtime_id.clone(),
            binding: plan.binding().clone(),
        };
        assert_eq!(
            recover_container_supervision(&mut wrong_name, &plan)
                .expect_err("wrong name")
                .failure_class(),
            ContainerSupervisorFailureClass::Ambiguous
        );
        assert_eq!(wrong_name.start_calls, 0);

        let mut changed_runtime = FakeRuntime::absent(&plan);
        changed_runtime.replace_runtime_id_after_start = true;
        assert_eq!(
            start_container_supervision(&mut changed_runtime, &plan)
                .expect_err("changed runtime ID")
                .failure_class(),
            ContainerSupervisorFailureClass::Ambiguous
        );
        assert_eq!(changed_runtime.successful_starts, 1);
        assert_eq!(changed_runtime.wait_calls, 0);
    }

    #[test]
    fn image_preflight_and_definitive_create_failure_have_no_start_effect() {
        let fixture = Fixture::new();
        let plan = fixture.build(AttemptId::new()).expect("launch plan");

        let mut volumes = FakeRuntime::absent(&plan);
        volumes.image =
            ResolvedContainerImage::new(plan.image().clone(), ContainerImageVolumeState::Declared);
        assert_eq!(
            start_container_supervision(&mut volumes, &plan)
                .expect_err("declared volumes")
                .failure_class(),
            ContainerSupervisorFailureClass::NotStarted
        );
        assert_eq!(volumes.create_calls, 0);

        let mut wrong_image = FakeRuntime::absent(&plan);
        wrong_image.image = ResolvedContainerImage::new(
            OciImageDigest::new(
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .expect("other image"),
            ContainerImageVolumeState::None,
        );
        assert_eq!(
            start_container_supervision(&mut wrong_image, &plan)
                .expect_err("image identity")
                .failure_class(),
            ContainerSupervisorFailureClass::NotStarted
        );
        assert_eq!(wrong_image.create_calls, 0);

        let mut rejected = FakeRuntime::absent(&plan);
        rejected.create_fault = Some(MutationFault::Rejected);
        assert_eq!(
            start_container_supervision(&mut rejected, &plan)
                .expect_err("create rejected")
                .failure_class(),
            ContainerSupervisorFailureClass::NotStarted
        );
        assert_eq!(rejected.successful_creates, 0);
        assert_eq!(rejected.start_calls, 0);

        rejected.create_fault = Some(MutationFault::Rejected);
        assert_eq!(
            recover_container_supervision(&mut rejected, &plan)
                .expect_err("recovery remains conservative")
                .failure_class(),
            ContainerSupervisorFailureClass::Ambiguous
        );
    }

    #[test]
    fn docker_create_argv_is_fixed_complete_and_shell_free() {
        let fixture = Fixture::new();
        let attempt_id = AttemptId::new();
        let plan = fixture.build(attempt_id).expect("launch plan");
        let input = fixture
            .state_root
            .input_directory(attempt_id)
            .display()
            .to_string();
        let binding = plan.binding();
        let expected = vec![
            "container".into(),
            "create".into(),
            "--name".into(),
            plan.name().as_str().into(),
            "--label".into(),
            format!("io.cairn.attempt-id={attempt_id}"),
            "--label".into(),
            format!("io.cairn.job-id={}", binding.job_id()),
            "--label".into(),
            format!("io.cairn.contract-id={}", binding.contract_id()),
            "--label".into(),
            format!("io.cairn.input-id={}", binding.input_bundle_id()),
            "--label".into(),
            format!("io.cairn.environment-id={}", binding.environment_id()),
            "--label".into(),
            "io.cairn.sandbox-policy=cpu-untrusted-v1".into(),
            "--read-only".into(),
            "--pull".into(),
            "never".into(),
            "--restart".into(),
            "no".into(),
            "--no-healthcheck".into(),
            "--network".into(),
            "none".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--user".into(),
            "65532:65532".into(),
            "--cgroupns".into(),
            "private".into(),
            "--ipc".into(),
            "private".into(),
            "--pid".into(),
            "private".into(),
            "--pids-limit".into(),
            "64".into(),
            "--cpus".into(),
            "2".into(),
            "--memory".into(),
            "33554432".into(),
            "--memory-swap".into(),
            "33554432".into(),
            "--memory-swappiness".into(),
            "0".into(),
            "--mount".into(),
            format!("type=bind,src={input},dst=/cairn/input,readonly,bind-propagation=private"),
            "--tmpfs".into(),
            "/cairn/work:rw,nosuid,nodev,size=2097152,mode=0700,uid=65532,gid=65532".into(),
            "--tmpfs".into(),
            "/cairn/output:rw,nosuid,nodev,noexec,size=2097152,mode=0700,uid=65532,gid=65532"
                .into(),
            "--tmpfs".into(),
            "/tmp:rw,nosuid,nodev,noexec,size=1048576,mode=0700,uid=65532,gid=65532".into(),
            "--workdir".into(),
            "/cairn/work".into(),
            "--env".into(),
            "ALPHA=one".into(),
            "--env".into(),
            "ZED=two words".into(),
            "--entrypoint".into(),
            "/cairn/input/bin/run".into(),
            IMAGE.into(),
            "--fixture".into(),
            "value with spaces".into(),
        ];
        let actual = strings(plan.docker_create_arguments());
        assert_eq!(actual, expected);
        assert_eq!(actual, strings(plan.docker_create_arguments()));
    }

    #[test]
    fn fixed_policy_has_one_read_only_host_mount_and_no_privilege_downgrade() {
        let fixture = Fixture::new();
        let arguments = strings(
            fixture
                .build(AttemptId::new())
                .expect("plan")
                .docker_create_arguments(),
        );
        assert_eq!(
            arguments.iter().filter(|value| *value == "--mount").count(),
            1
        );
        for forbidden in [
            "--privileged",
            "--device",
            "--cap-add",
            "--publish",
            "--userns",
            "host",
            "/var/run/docker.sock",
        ] {
            assert!(!arguments.iter().any(|argument| argument == forbidden));
        }
        assert!(arguments.iter().any(|argument| argument == "--read-only"));
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "no-new-privileges")
        );
        assert!(arguments.iter().any(|argument| argument == "none"));
    }

    #[test]
    fn policy_rejects_network_devices_and_insufficient_limits() {
        let fixture = Fixture::new();
        let network_contract = make_contract(
            fixture.contract.input_bundle_id(),
            fixture.contract.environment_id(),
            OCI_CONTAINER_BACKEND,
            NetworkPolicy::DependencyFetch,
            quantitative(None),
            "work",
            "output/result.bin",
        );
        assert_plan_error(
            &fixture,
            &network_contract,
            fixture.limits,
            ContainerLaunchPlanError::NetworkNotDisabled,
        );

        let accelerator = AcceleratorResourceRequest::new(
            AcceleratorDeviceCount::new(1).expect("device count"),
            Vec::new(),
        )
        .expect("accelerator");
        let device_contract = make_contract(
            fixture.contract.input_bundle_id(),
            fixture.contract.environment_id(),
            OCI_CONTAINER_BACKEND,
            NetworkPolicy::Disabled,
            quantitative(Some(accelerator)),
            "work",
            "output/result.bin",
        );
        assert_plan_error(
            &fixture,
            &device_contract,
            fixture.limits,
            ContainerLaunchPlanError::DeviceRequestRejected,
        );

        assert_plan_error(
            &fixture,
            &fixture.contract,
            limits(1, 8 * 1024 * 1024, 64, 2 * 1024 * 1024),
            ContainerLaunchPlanError::MemoryLimitBelowRequest,
        );
        assert_plan_error(
            &fixture,
            &fixture.contract,
            limits(1, 32 * 1024 * 1024, 64, 512 * 1024),
            ContainerLaunchPlanError::WorkLimitBelowRequest,
        );
    }

    fn assert_plan_error(
        fixture: &Fixture,
        contract: &JobContract,
        limits: ContainerLaunchLimits,
        expected: ContainerLaunchPlanError,
    ) {
        let contract_id =
            ContentId::derive(&cairn_codec::to_vec(contract).expect("contract bytes"))
                .expect("contract ID");
        assert_eq!(
            build_container_launch_plan(
                AttemptId::new(),
                contract_id,
                contract,
                &fixture.materials,
                &fixture.input_bytes,
                &fixture.environment_bytes,
                &fixture.state_root,
                limits,
            ),
            Err(expected)
        );
    }

    #[test]
    fn layout_and_identity_controls_fail_before_runtime_authority() {
        let fixture = Fixture::new();
        let local_contract = make_contract(
            fixture.contract.input_bundle_id(),
            fixture.contract.environment_id(),
            "local-process-v1",
            NetworkPolicy::Disabled,
            quantitative(None),
            "work",
            "output/result.bin",
        );
        assert_plan_error(
            &fixture,
            &local_contract,
            fixture.limits,
            ContainerLaunchPlanError::BackendMismatch,
        );

        let wrong_work = make_contract(
            fixture.contract.input_bundle_id(),
            fixture.contract.environment_id(),
            OCI_CONTAINER_BACKEND,
            NetworkPolicy::Disabled,
            quantitative(None),
            "bin",
            "output/result.bin",
        );
        assert_plan_error(
            &fixture,
            &wrong_work,
            fixture.limits,
            ContainerLaunchPlanError::InvalidWorkingDirectory,
        );

        let wrong_output = make_contract(
            fixture.contract.input_bundle_id(),
            fixture.contract.environment_id(),
            OCI_CONTAINER_BACKEND,
            NetworkPolicy::Disabled,
            quantitative(None),
            "work",
            "result.bin",
        );
        assert_plan_error(
            &fixture,
            &wrong_output,
            fixture.limits,
            ContainerLaunchPlanError::OutputOutsideOutputMount("result.bin".into()),
        );

        assert_eq!(
            build_container_launch_plan(
                AttemptId::new(),
                ContentId::derive(b"wrong contract").expect("wrong ID"),
                &fixture.contract,
                &fixture.materials,
                &fixture.input_bytes,
                &fixture.environment_bytes,
                &fixture.state_root,
                fixture.limits,
            ),
            Err(ContainerLaunchPlanError::ContractIdentityMismatch)
        );
        let mut changed_input = fixture.input_bytes.clone();
        changed_input.push(b' ');
        assert_eq!(
            build_container_launch_plan(
                AttemptId::new(),
                fixture.contract_id,
                &fixture.contract,
                &fixture.materials,
                &changed_input,
                &fixture.environment_bytes,
                &fixture.state_root,
                fixture.limits,
            ),
            Err(ContainerLaunchPlanError::MaterialIdentityMismatch)
        );
    }

    #[test]
    fn reserved_input_paths_and_unsafe_state_roots_are_unrepresentable() {
        let fixture = Fixture::new();
        let reserved = InputBundleV1::new(vec![InputBundleEntry::Directory {
            path: SandboxPath::new("work").expect("reserved path"),
        }])
        .expect("bundle");
        assert_eq!(
            validate_input_layout(&reserved, &fixture.contract),
            Err(ContainerLaunchPlanError::ReservedInputPath("work".into()))
        );
        assert_eq!(
            ContainerStateRoot::new(PathBuf::from("relative/oci")),
            Err(ContainerLaunchPlanError::InvalidStateRoot)
        );
        assert_eq!(
            ContainerStateRoot::new(PathBuf::from("/")),
            Err(ContainerLaunchPlanError::InvalidStateRoot)
        );
        assert_eq!(
            ContainerStateRoot::new(PathBuf::from("/var/lib/cairn,escape")),
            Err(ContainerLaunchPlanError::InvalidStateRoot)
        );
        assert_eq!(
            ContainerMemoryByteLimit::new(u64::MAX),
            Err(ContainerLaunchPlanError::MemoryLimitAboveMaximum)
        );
        assert_eq!(
            ContainerPidsLimit::new(u64::MAX),
            Err(ContainerLaunchPlanError::PidsLimitAboveMaximum)
        );
        assert_eq!(
            ContainerWritableByteLimit::new(u64::MAX),
            Err(ContainerLaunchPlanError::WritableLimitAboveMaximum)
        );
    }
}
