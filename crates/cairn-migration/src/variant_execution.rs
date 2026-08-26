//! Admission-variant build evidence and exact execution/comparison composition.

use cairn_execution::{
    CapturePolicy, CommandArgument, CommandContract, DeclaredOutputArtifact, DiagnosticByteLimit,
    EvidenceByteLimit, ExecutionEnvironmentArtifact, ExecutionOutcome, ExecutionReceipt,
    ExecutionReceiptArtifact, ExpectedOutput, InputBundleArtifact, InputBundleEntry, InputBundleV1,
    InputFileMode, JobContract, JobContractArtifact, NetworkPolicy, OutputByteLimit, OutputName,
    SandboxPath,
};
use cairn_protocol::{ContentId, ContentType, JobId};
use cairn_record::ContentStore;
use cairn_verification::{
    ImplementationBundleArtifact, ImplementationVariantArtifact, ImplementationVariantV1,
    VariantExpectation,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CallAdapterExecutableArtifact, CorpusExecutionSubjectV1, ExactCorpusComparisonArtifact,
    MigrationDomainContractV1, MigrationExecutionNeed, MigrationValidationTier,
    PreparedCorpusExecutionPlan, PreparedExactCorpusComparison, ValidatedCorpusObservationSet,
    compare_exact_corpus_observations,
};

const BUILD_DIRECTORY: &str = "cairn";
const BUILD_DRIVER_PATH: &str = "cairn/build-driver";
const VARIANT_PATH: &str = "cairn/variant.json";
const IMPLEMENTATION_PATH: &str = "cairn/implementation.bundle";
const BUILD_OUTPUT_PATH: &str = "cairn/call-adapter";
const BUILD_OUTPUT_NAME: &str = "call-adapter-executable";
const WORKING_DIRECTORY: &str = "work";
const CONTAINER_VARIANT_PATH: &str = "/cairn/input/cairn/variant.json";
const CONTAINER_IMPLEMENTATION_PATH: &str = "/cairn/input/cairn/implementation.bundle";
const CONTAINER_BUILD_OUTPUT_PATH: &str = "/cairn/output/cairn/call-adapter";

macro_rules! byte_limit {
    ($(#[$meta:meta])* $name:ident, $field:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive byte limit.
            ///
            /// # Errors
            ///
            /// Rejects zero rather than assigning it disabled meaning.
            pub fn new(value: u64) -> Result<Self, VariantExecutionError> {
                if value == 0 {
                    return Err(VariantExecutionError::InvalidByteLimit { field: $field });
                }
                Ok(Self(value))
            }

            /// Returns the bounded byte count.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = VariantExecutionError;

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

byte_limit!(
    /// Maximum trusted build-driver byte count.
    VariantBuildDriverByteLimit,
    "variant build driver"
);
byte_limit!(
    /// Maximum implementation-bundle byte count admitted to one build.
    VariantImplementationByteLimit,
    "variant implementation bundle"
);

/// Independent build streams, artifact, diagnostics, and trusted-evidence bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VariantBuildCaptureLimits {
    /// Maximum stdout bytes.
    pub stdout: OutputByteLimit,
    /// Maximum stderr bytes.
    pub stderr: OutputByteLimit,
    /// Maximum produced call-adapter executable bytes.
    pub executable: OutputByteLimit,
    /// Maximum durable executor diagnostic bytes.
    pub diagnostic: DiagnosticByteLimit,
    /// Maximum trusted execution evidence bytes.
    pub evidence: EvidenceByteLimit,
}

/// Failure to bind admission variants through build, execute, observe, and compare evidence.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum VariantExecutionError {
    /// Only strict V1 build and trial artifacts are accepted.
    #[error("variant execution schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A caller-provided byte limit was zero.
    #[error("{field} byte limit must be greater than zero")]
    InvalidByteLimit { field: &'static str },
    /// Build driver or implementation bytes were empty or exceeded their caller bound.
    #[error("{field} bytes are empty or exceed their limit")]
    InvalidBuildInput { field: &'static str },
    /// Implementation bytes do not match the bundle selected by the variant.
    #[error("variant implementation bytes do not match the implementation identity")]
    ImplementationIdentityMismatch,
    /// Prepared build material, contract, or persisted plan was contradictory.
    #[error("variant build plan is inconsistent")]
    InconsistentBuildPlan,
    /// Generic execution did not produce an authoritative successful build receipt.
    #[error("variant build receipt is inconsistent")]
    InconsistentBuildReceipt,
    /// The corpus plan is not bound to the exact supplied admission variant and executable.
    #[error("variant corpus execution plan is inconsistent")]
    InconsistentVariantPlan,
    /// The supplied exact comparison is not the recomputation of the variant observations.
    #[error("variant exact comparison is inconsistent")]
    InconsistentComparison,
    /// Persisted variant-control trial facts differ from trusted recomputation.
    #[error("exact admission-variant trial is inconsistent")]
    InconsistentTrial,
    /// Content storage failed while loading a declared build output.
    #[error("variant build content error: {message}")]
    Content { message: String },
    /// A nested execution-contract, comparison, or codec operation failed.
    #[error("variant execution composition error: {message}")]
    Composition { message: String },
}

/// Content domain for an exact trusted build-driver implementation.
pub enum VariantBuildDriverArtifact {}

impl ContentType for VariantBuildDriverArtifact {
    const DOMAIN: &'static str = "migration.variant-build-driver.v1";
}

/// Strict V1 product wrapper around one generic variant build job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "VariantBuildPlanWire")]
pub struct VariantBuildPlanV1 {
    schema_version: u16,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    driver: ContentId<VariantBuildDriverArtifact>,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    tier: MigrationValidationTier,
    job_id: JobId,
    contract: ContentId<JobContractArtifact>,
    executable_output_name: OutputName,
    executable_output_path: SandboxPath,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantBuildPlanWire {
    schema_version: u16,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    driver: ContentId<VariantBuildDriverArtifact>,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    tier: MigrationValidationTier,
    job_id: JobId,
    contract: ContentId<JobContractArtifact>,
    executable_output_name: OutputName,
    executable_output_path: SandboxPath,
}

impl VariantBuildPlanV1 {
    #[expect(
        clippy::too_many_arguments,
        reason = "variant, implementation, driver, generic job, and declared output are independent immutable bindings"
    )]
    fn new(
        variant: ContentId<ImplementationVariantArtifact>,
        implementation: ContentId<ImplementationBundleArtifact>,
        driver: ContentId<VariantBuildDriverArtifact>,
        input_bundle: ContentId<InputBundleArtifact>,
        environment: ContentId<ExecutionEnvironmentArtifact>,
        tier: MigrationValidationTier,
        job_id: JobId,
        contract: ContentId<JobContractArtifact>,
        executable_output_name: OutputName,
        executable_output_path: SandboxPath,
    ) -> Result<Self, VariantExecutionError> {
        if executable_output_name.as_str() != BUILD_OUTPUT_NAME
            || executable_output_path.as_str() != BUILD_OUTPUT_PATH
        {
            return Err(VariantExecutionError::InconsistentBuildPlan);
        }
        Ok(Self {
            schema_version: 1,
            variant,
            implementation,
            driver,
            input_bundle,
            environment,
            tier,
            job_id,
            contract,
            executable_output_name,
            executable_output_path,
        })
    }

    /// Returns the exact variant artifact being built.
    #[must_use]
    pub const fn variant(&self) -> ContentId<ImplementationVariantArtifact> {
        self.variant
    }

    /// Returns the variant's exact implementation bundle.
    #[must_use]
    pub const fn implementation(&self) -> ContentId<ImplementationBundleArtifact> {
        self.implementation
    }

    /// Returns the exact generic job contract.
    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }

    /// Returns the product validation tier, which is absent from generic worker bytes.
    #[must_use]
    pub const fn tier(&self) -> MigrationValidationTier {
        self.tier
    }
}

impl TryFrom<VariantBuildPlanWire> for VariantBuildPlanV1 {
    type Error = VariantExecutionError;

    fn try_from(wire: VariantBuildPlanWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(VariantExecutionError::UnsupportedSchemaVersion);
        }
        Self::new(
            wire.variant,
            wire.implementation,
            wire.driver,
            wire.input_bundle,
            wire.environment,
            wire.tier,
            wire.job_id,
            wire.contract,
            wire.executable_output_name,
            wire.executable_output_path,
        )
    }
}

/// Content domain for one immutable admission-variant build plan.
pub enum VariantBuildPlanArtifact {}

impl ContentType for VariantBuildPlanArtifact {
    const DOMAIN: &'static str = "migration.variant-build-plan.v1";
}

/// Exact build input, generic contract, and product plan ready for normal execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedVariantBuildJob {
    variant: ImplementationVariantV1,
    variant_bytes: Vec<u8>,
    variant_id: ContentId<ImplementationVariantArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    contract: JobContract,
    contract_bytes: Vec<u8>,
    contract_id: ContentId<JobContractArtifact>,
    plan: VariantBuildPlanV1,
    plan_bytes: Vec<u8>,
    plan_id: ContentId<VariantBuildPlanArtifact>,
}

impl PreparedVariantBuildJob {
    #[must_use]
    pub const fn variant(&self) -> &ImplementationVariantV1 {
        &self.variant
    }

    #[must_use]
    pub const fn variant_id(&self) -> ContentId<ImplementationVariantArtifact> {
        self.variant_id
    }

    #[must_use]
    pub const fn input_bundle(&self) -> &InputBundleV1 {
        &self.input_bundle
    }

    #[must_use]
    pub fn input_bundle_bytes(&self) -> &[u8] {
        &self.input_bundle_bytes
    }

    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }

    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.contract
    }

    #[must_use]
    pub fn contract_bytes(&self) -> &[u8] {
        &self.contract_bytes
    }

    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    #[must_use]
    pub const fn plan(&self) -> &VariantBuildPlanV1 {
        &self.plan
    }

    #[must_use]
    pub fn plan_bytes(&self) -> &[u8] {
        &self.plan_bytes
    }

    #[must_use]
    pub const fn plan_id(&self) -> ContentId<VariantBuildPlanArtifact> {
        self.plan_id
    }
}

/// Prepares a bounded, non-shell generic build job for one exact implementation variant.
///
/// # Errors
///
/// Rejects empty or oversized inputs, implementation identity mismatch, invalid execution intent,
/// or non-canonical material.
#[expect(
    clippy::too_many_arguments,
    reason = "variant bytes, driver bytes, their independent limits, environment, execution need, and capture bounds are separate trust inputs"
)]
pub fn prepare_variant_build_job(
    job_id: JobId,
    variant: &ImplementationVariantV1,
    implementation_bytes: &[u8],
    implementation_limit: VariantImplementationByteLimit,
    build_driver: &[u8],
    driver_limit: VariantBuildDriverByteLimit,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: VariantBuildCaptureLimits,
) -> Result<PreparedVariantBuildJob, VariantExecutionError> {
    validate_bytes(
        implementation_bytes,
        implementation_limit.get(),
        "variant implementation bundle",
    )?;
    validate_bytes(build_driver, driver_limit.get(), "variant build driver")?;
    let variant_bytes = cairn_codec::to_vec(variant).map_err(composition)?;
    let variant_id =
        ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes).map_err(composition)?;
    let implementation_id = ContentId::<ImplementationBundleArtifact>::derive(implementation_bytes)
        .map_err(composition)?;
    if implementation_id != variant.implementation() {
        return Err(VariantExecutionError::ImplementationIdentityMismatch);
    }
    let driver_id =
        ContentId::<VariantBuildDriverArtifact>::derive(build_driver).map_err(composition)?;
    let (input_bundle, input_bundle_bytes, input_bundle_id, command) =
        prepare_build_input(&variant_bytes, implementation_bytes, build_driver)?;
    let executable_output_name = output_name(BUILD_OUTPUT_NAME)?;
    let executable_output_path = path(BUILD_OUTPUT_PATH)?;
    let capture = CapturePolicy::new(
        limits.stdout,
        limits.stderr,
        limits.diagnostic,
        limits.evidence,
        vec![ExpectedOutput {
            name: executable_output_name.clone(),
            path: executable_output_path.clone(),
            byte_limit: limits.executable,
        }],
    )
    .map_err(composition)?;
    let contract = JobContract::new(
        job_id,
        input_bundle_id,
        environment,
        need.backend().clone(),
        command,
        need.to_resource_request().map_err(composition)?,
        NetworkPolicy::Disabled,
        capture,
    );
    let contract_bytes = cairn_codec::to_vec(&contract).map_err(composition)?;
    let contract_id =
        ContentId::<JobContractArtifact>::derive(&contract_bytes).map_err(composition)?;
    let plan = VariantBuildPlanV1::new(
        variant_id,
        implementation_id,
        driver_id,
        input_bundle_id,
        environment,
        need.tier(),
        job_id,
        contract_id,
        executable_output_name,
        executable_output_path,
    )?;
    let plan_bytes = cairn_codec::to_vec(&plan).map_err(composition)?;
    let plan_id =
        ContentId::<VariantBuildPlanArtifact>::derive(&plan_bytes).map_err(composition)?;
    Ok(PreparedVariantBuildJob {
        variant: variant.clone(),
        variant_bytes,
        variant_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        contract,
        contract_bytes,
        contract_id,
        plan,
        plan_bytes,
        plan_id,
    })
}

fn prepare_build_input(
    variant_bytes: &[u8],
    implementation_bytes: &[u8],
    build_driver: &[u8],
) -> Result<
    (
        InputBundleV1,
        Vec<u8>,
        ContentId<InputBundleArtifact>,
        CommandContract,
    ),
    VariantExecutionError,
> {
    let input_bundle = InputBundleV1::new(vec![
        InputBundleEntry::Directory {
            path: path(BUILD_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: path(BUILD_DRIVER_PATH)?,
            mode: InputFileMode::Executable,
            bytes: build_driver.to_vec(),
        },
        InputBundleEntry::File {
            path: path(IMPLEMENTATION_PATH)?,
            mode: InputFileMode::Data,
            bytes: implementation_bytes.to_vec(),
        },
        InputBundleEntry::File {
            path: path(VARIANT_PATH)?,
            mode: InputFileMode::Data,
            bytes: variant_bytes.to_vec(),
        },
    ])
    .map_err(composition)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(composition)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(composition)?;
    let command = CommandContract::new(
        path(BUILD_DRIVER_PATH)?,
        vec![
            argument("--variant")?,
            argument(CONTAINER_VARIANT_PATH)?,
            argument("--implementation")?,
            argument(CONTAINER_IMPLEMENTATION_PATH)?,
            argument("--output")?,
            argument(CONTAINER_BUILD_OUTPUT_PATH)?,
        ],
        path(WORKING_DIRECTORY)?,
    );
    Ok((input_bundle, input_bundle_bytes, input_bundle_id, command))
}

/// Strict V1 facts binding an authoritative generic build receipt to exact executable bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "VariantBuildReceiptWire")]
pub struct VariantBuildReceiptV1 {
    schema_version: u16,
    plan: ContentId<VariantBuildPlanArtifact>,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    execution_evidence: ContentId<cairn_execution::ExecutionEvidenceArtifact>,
    declared_output: ContentId<DeclaredOutputArtifact>,
    executable: ContentId<CallAdapterExecutableArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantBuildReceiptWire {
    schema_version: u16,
    plan: ContentId<VariantBuildPlanArtifact>,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    execution_evidence: ContentId<cairn_execution::ExecutionEvidenceArtifact>,
    declared_output: ContentId<DeclaredOutputArtifact>,
    executable: ContentId<CallAdapterExecutableArtifact>,
}

impl VariantBuildReceiptV1 {
    fn new(
        plan: ContentId<VariantBuildPlanArtifact>,
        variant: ContentId<ImplementationVariantArtifact>,
        implementation: ContentId<ImplementationBundleArtifact>,
        receipt: ContentId<ExecutionReceiptArtifact>,
        execution_evidence: ContentId<cairn_execution::ExecutionEvidenceArtifact>,
        declared_output: ContentId<DeclaredOutputArtifact>,
        executable: ContentId<CallAdapterExecutableArtifact>,
    ) -> Self {
        Self {
            schema_version: 1,
            plan,
            variant,
            implementation,
            receipt,
            execution_evidence,
            declared_output,
            executable,
        }
    }

    /// Returns the exact variant build plan.
    #[must_use]
    pub const fn plan(&self) -> ContentId<VariantBuildPlanArtifact> {
        self.plan
    }

    /// Returns the exact proposal-authored variant artifact.
    #[must_use]
    pub const fn variant(&self) -> ContentId<ImplementationVariantArtifact> {
        self.variant
    }

    /// Returns the exact implementation bundle built for that variant.
    #[must_use]
    pub const fn implementation(&self) -> ContentId<ImplementationBundleArtifact> {
        self.implementation
    }

    /// Returns the authoritative generic execution receipt.
    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }

    /// Returns the call-adapter executable identity derived from captured bytes.
    #[must_use]
    pub const fn executable(&self) -> ContentId<CallAdapterExecutableArtifact> {
        self.executable
    }

    /// Revalidates this persisted build fact against its authoritative generic receipt and output.
    ///
    /// # Errors
    ///
    /// Rejects changed prepared material, receipt authority, or declared executable bytes.
    pub fn validate_inputs<C: ContentStore>(
        &self,
        build: &PreparedVariantBuildJob,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: &ExecutionReceipt,
        content: &C,
    ) -> Result<(), VariantExecutionError> {
        let recomputed = validate_variant_build_receipt(build, receipt_id, receipt, content)?;
        if recomputed.build_receipt != *self {
            return Err(VariantExecutionError::InconsistentBuildReceipt);
        }
        Ok(())
    }
}

impl TryFrom<VariantBuildReceiptWire> for VariantBuildReceiptV1 {
    type Error = VariantExecutionError;

    fn try_from(wire: VariantBuildReceiptWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(VariantExecutionError::UnsupportedSchemaVersion);
        }
        Ok(Self::new(
            wire.plan,
            wire.variant,
            wire.implementation,
            wire.receipt,
            wire.execution_evidence,
            wire.declared_output,
            wire.executable,
        ))
    }
}

/// Content domain for exact validated admission-variant build facts.
pub enum VariantBuildReceiptArtifact {}

impl ContentType for VariantBuildReceiptArtifact {
    const DOMAIN: &'static str = "migration.variant-build-receipt.v1";
}

/// Validated executable output from an authoritative generic variant build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedVariantBuild {
    build_receipt: VariantBuildReceiptV1,
    build_receipt_bytes: Vec<u8>,
    build_receipt_id: ContentId<VariantBuildReceiptArtifact>,
    executable_bytes: Vec<u8>,
}

impl ValidatedVariantBuild {
    #[must_use]
    pub const fn build_receipt(&self) -> &VariantBuildReceiptV1 {
        &self.build_receipt
    }

    #[must_use]
    pub fn build_receipt_bytes(&self) -> &[u8] {
        &self.build_receipt_bytes
    }

    #[must_use]
    pub const fn build_receipt_id(&self) -> ContentId<VariantBuildReceiptArtifact> {
        self.build_receipt_id
    }

    #[must_use]
    pub fn executable_bytes(&self) -> &[u8] {
        &self.executable_bytes
    }

    #[must_use]
    pub const fn executable_id(&self) -> ContentId<CallAdapterExecutableArtifact> {
        self.build_receipt.executable
    }
}

/// Validates one authoritative generic build receipt and loads the exact declared executable.
///
/// # Errors
///
/// Rejects changed prepared material, failed execution, wrong output declaration, unavailable
/// output bytes, or any receipt/content identity mismatch.
pub fn validate_variant_build_receipt<C: ContentStore>(
    build: &PreparedVariantBuildJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
) -> Result<ValidatedVariantBuild, VariantExecutionError> {
    validate_prepared_build(build)?;
    let receipt_bytes = cairn_codec::to_vec(receipt).map_err(composition)?;
    if ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).map_err(composition)?
        != receipt_id
        || receipt.job_id() != build.contract.job_id()
        || receipt.contract_id() != build.contract_id
        || receipt.outcome() != ExecutionOutcome::Succeeded
        || receipt.exit_code() != Some(0)
        || receipt.outputs().len() != 1
        || receipt.outputs()[0].name.as_str() != BUILD_OUTPUT_NAME
    {
        return Err(VariantExecutionError::InconsistentBuildReceipt);
    }
    let declared_output = receipt.outputs()[0].content_id;
    let mut executable_bytes = Vec::new();
    content
        .write_to::<DeclaredOutputArtifact>(&declared_output, &mut executable_bytes)
        .map_err(|error| VariantExecutionError::Content {
            message: error.to_string(),
        })?;
    let executable_limit = build.contract.capture().expected_outputs()[0]
        .byte_limit
        .get();
    if executable_bytes.is_empty()
        || u64::try_from(executable_bytes.len()).map_err(composition)? > executable_limit
    {
        return Err(VariantExecutionError::InconsistentBuildReceipt);
    }
    let executable = ContentId::<CallAdapterExecutableArtifact>::derive(&executable_bytes)
        .map_err(composition)?;
    let build_receipt = VariantBuildReceiptV1::new(
        build.plan_id,
        build.variant_id,
        build.variant.implementation(),
        receipt_id,
        receipt.evidence_id(),
        declared_output,
        executable,
    );
    let build_receipt_bytes = cairn_codec::to_vec(&build_receipt).map_err(composition)?;
    let build_receipt_id = ContentId::<VariantBuildReceiptArtifact>::derive(&build_receipt_bytes)
        .map_err(composition)?;
    Ok(ValidatedVariantBuild {
        build_receipt,
        build_receipt_bytes,
        build_receipt_id,
        executable_bytes,
    })
}

fn validate_prepared_build(build: &PreparedVariantBuildJob) -> Result<(), VariantExecutionError> {
    let variant_bytes = cairn_codec::to_vec(&build.variant).map_err(composition)?;
    let input_bytes = build.input_bundle.to_bytes().map_err(composition)?;
    let contract_bytes = cairn_codec::to_vec(&build.contract).map_err(composition)?;
    let plan_bytes = cairn_codec::to_vec(&build.plan).map_err(composition)?;
    if variant_bytes != build.variant_bytes
        || ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes)
            .map_err(composition)?
            != build.variant_id
        || input_bytes != build.input_bundle_bytes
        || ContentId::<InputBundleArtifact>::derive(&input_bytes).map_err(composition)?
            != build.input_bundle_id
        || contract_bytes != build.contract_bytes
        || ContentId::<JobContractArtifact>::derive(&contract_bytes).map_err(composition)?
            != build.contract_id
        || plan_bytes != build.plan_bytes
        || ContentId::<VariantBuildPlanArtifact>::derive(&plan_bytes).map_err(composition)?
            != build.plan_id
        || build.plan.variant != build.variant_id
        || build.plan.implementation != build.variant.implementation()
        || build.plan.input_bundle != build.input_bundle_id
        || build.plan.contract != build.contract_id
        || build.plan.job_id != build.contract.job_id()
        || build.plan.environment != build.contract.environment_id()
        || build.contract.input_bundle_id() != build.input_bundle_id
        || build.contract.network() != NetworkPolicy::Disabled
        || build.contract.capture().expected_outputs().len() != 1
        || build.contract.capture().expected_outputs()[0].name.as_str() != BUILD_OUTPUT_NAME
        || build.contract.capture().expected_outputs()[0].path.as_str() != BUILD_OUTPUT_PATH
    {
        return Err(VariantExecutionError::InconsistentBuildPlan);
    }
    Ok(())
}

/// Strict V1 exact control trial for one correct or deliberately wrong admission variant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ExactVariantTrialWire")]
pub struct ExactVariantTrialV1 {
    schema_version: u16,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    expectation: VariantExpectation,
    build: ContentId<VariantBuildReceiptArtifact>,
    variant_plan: ContentId<crate::CorpusExecutionPlanArtifact>,
    variant_observations: ContentId<crate::CorpusObservationSetArtifact>,
    comparison: ContentId<ExactCorpusComparisonArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactVariantTrialWire {
    schema_version: u16,
    variant: ContentId<ImplementationVariantArtifact>,
    implementation: ContentId<ImplementationBundleArtifact>,
    expectation: VariantExpectation,
    build: ContentId<VariantBuildReceiptArtifact>,
    variant_plan: ContentId<crate::CorpusExecutionPlanArtifact>,
    variant_observations: ContentId<crate::CorpusObservationSetArtifact>,
    comparison: ContentId<ExactCorpusComparisonArtifact>,
}

impl ExactVariantTrialV1 {
    fn new(
        variant: ContentId<ImplementationVariantArtifact>,
        implementation: ContentId<ImplementationBundleArtifact>,
        expectation: VariantExpectation,
        build: ContentId<VariantBuildReceiptArtifact>,
        variant_plan: ContentId<crate::CorpusExecutionPlanArtifact>,
        variant_observations: ContentId<crate::CorpusObservationSetArtifact>,
        comparison: ContentId<ExactCorpusComparisonArtifact>,
    ) -> Self {
        Self {
            schema_version: 1,
            variant,
            implementation,
            expectation,
            build,
            variant_plan,
            variant_observations,
            comparison,
        }
    }

    /// Returns the exact variant artifact under control.
    #[must_use]
    pub const fn variant(&self) -> ContentId<ImplementationVariantArtifact> {
        self.variant
    }

    /// Returns the proposal-authored expectation retained without a stored result bit.
    #[must_use]
    pub const fn expectation(&self) -> &VariantExpectation {
        &self.expectation
    }

    /// Recomputes whether the exact comparison produced the response required by the variant.
    #[must_use]
    pub fn expectation_satisfied(&self, comparison: &PreparedExactCorpusComparison) -> bool {
        match self.expectation {
            VariantExpectation::MustAccept { .. } => comparison.comparison().all_match(),
            VariantExpectation::MustReject { .. } => !comparison.comparison().all_match(),
        }
    }

    /// Fully recomputes this persisted trial from build and corpus execution evidence.
    ///
    /// # Errors
    ///
    /// Rejects any changed identity, expectation, plan, observation, executable, or comparison.
    #[expect(
        clippy::too_many_arguments,
        reason = "variant, build, reference evidence, subject evidence, and comparison are independent trust inputs"
    )]
    pub fn validate_inputs(
        &self,
        domain: &MigrationDomainContractV1,
        variant: &ImplementationVariantV1,
        build: &ValidatedVariantBuild,
        reference_plan: &PreparedCorpusExecutionPlan,
        reference_observations: &ValidatedCorpusObservationSet,
        variant_plan: &PreparedCorpusExecutionPlan,
        variant_observations: &ValidatedCorpusObservationSet,
        comparison: &PreparedExactCorpusComparison,
    ) -> Result<(), VariantExecutionError> {
        let recomputed = compose_exact_variant_trial(
            domain,
            variant,
            build,
            reference_plan,
            reference_observations,
            variant_plan,
            variant_observations,
            comparison,
        )?;
        if recomputed.trial != *self {
            return Err(VariantExecutionError::InconsistentTrial);
        }
        Ok(())
    }
}

impl TryFrom<ExactVariantTrialWire> for ExactVariantTrialV1 {
    type Error = VariantExecutionError;

    fn try_from(wire: ExactVariantTrialWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(VariantExecutionError::UnsupportedSchemaVersion);
        }
        Ok(Self::new(
            wire.variant,
            wire.implementation,
            wire.expectation,
            wire.build,
            wire.variant_plan,
            wire.variant_observations,
            wire.comparison,
        ))
    }
}

/// Content domain for an exact admission-variant control trial.
pub enum ExactVariantTrialArtifact {}

impl ContentType for ExactVariantTrialArtifact {
    const DOMAIN: &'static str = "migration.exact-variant-trial.v1";
}

/// Canonical exact variant-control trial ready for admission composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedExactVariantTrial {
    trial: ExactVariantTrialV1,
    trial_bytes: Vec<u8>,
    trial_id: ContentId<ExactVariantTrialArtifact>,
}

impl PreparedExactVariantTrial {
    #[must_use]
    pub const fn trial(&self) -> &ExactVariantTrialV1 {
        &self.trial
    }

    #[must_use]
    pub fn trial_bytes(&self) -> &[u8] {
        &self.trial_bytes
    }

    #[must_use]
    pub const fn trial_id(&self) -> ContentId<ExactVariantTrialArtifact> {
        self.trial_id
    }
}

/// Composes an exact admission variant through validated build, corpus execution, observation, and
/// trusted comparison evidence.
///
/// The output remains a control trial, not an oracle-admission decision. Whether `MustAccept` or
/// `MustReject` was satisfied is recomputed from the supplied comparison.
///
/// # Errors
///
/// Rejects a relabeled variant plan, executable/build mismatch, inconsistent observations, or a
/// comparison not fully recomputed from those exact inputs.
#[expect(
    clippy::too_many_arguments,
    reason = "variant, build, reference evidence, subject evidence, and comparison are independent trust inputs"
)]
pub fn compose_exact_variant_trial(
    domain: &MigrationDomainContractV1,
    variant: &ImplementationVariantV1,
    build: &ValidatedVariantBuild,
    reference_plan: &PreparedCorpusExecutionPlan,
    reference_observations: &ValidatedCorpusObservationSet,
    variant_plan: &PreparedCorpusExecutionPlan,
    variant_observations: &ValidatedCorpusObservationSet,
    comparison: &PreparedExactCorpusComparison,
) -> Result<PreparedExactVariantTrial, VariantExecutionError> {
    let variant_bytes = cairn_codec::to_vec(variant).map_err(composition)?;
    let variant_id =
        ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes).map_err(composition)?;
    if build.build_receipt.variant != variant_id
        || build.build_receipt.implementation != variant.implementation()
        || build.build_receipt.executable != variant_plan.plan().executable()
        || !matches!(
            variant_plan.plan().subject(),
            CorpusExecutionSubjectV1::AdmissionVariant { variant } if variant == variant_id
        )
    {
        return Err(VariantExecutionError::InconsistentVariantPlan);
    }
    let recomputed = compare_exact_corpus_observations(
        domain,
        reference_plan,
        reference_observations,
        variant_plan,
        variant_observations,
    )
    .map_err(composition)?;
    if recomputed != *comparison {
        return Err(VariantExecutionError::InconsistentComparison);
    }
    let trial = ExactVariantTrialV1::new(
        variant_id,
        variant.implementation(),
        variant.expectation().clone(),
        build.build_receipt_id,
        variant_plan.plan_id(),
        variant_observations.observation_set_id(),
        comparison.comparison_id(),
    );
    let trial_bytes = cairn_codec::to_vec(&trial).map_err(composition)?;
    let trial_id =
        ContentId::<ExactVariantTrialArtifact>::derive(&trial_bytes).map_err(composition)?;
    Ok(PreparedExactVariantTrial {
        trial,
        trial_bytes,
        trial_id,
    })
}

fn validate_bytes(
    bytes: &[u8],
    limit: u64,
    field: &'static str,
) -> Result<(), VariantExecutionError> {
    if bytes.is_empty() || u64::try_from(bytes.len()).map_err(composition)? > limit {
        return Err(VariantExecutionError::InvalidBuildInput { field });
    }
    Ok(())
}

fn path(value: &str) -> Result<SandboxPath, VariantExecutionError> {
    SandboxPath::new(value).map_err(composition)
}

fn argument(value: &str) -> Result<CommandArgument, VariantExecutionError> {
    CommandArgument::new(value).map_err(composition)
}

fn output_name(value: &str) -> Result<OutputName, VariantExecutionError> {
    OutputName::new(value).map_err(composition)
}

fn composition(error: impl std::fmt::Display) -> VariantExecutionError {
    VariantExecutionError::Composition {
        message: error.to_string(),
    }
}
