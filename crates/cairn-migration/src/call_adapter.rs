//! Language-neutral isolated-process protocol for operator call adapters.

use std::collections::BTreeSet;

use cairn_execution::{
    CapturePolicy, CapturedOutput, CommandArgument, CommandContract, DeclaredOutputArtifact,
    DiagnosticByteLimit, EvidenceByteLimit, ExecutionEnvironmentArtifact, ExecutionOutcome,
    ExecutionReceipt, ExecutionReceiptArtifact, ExpectedOutput, InputBundleArtifact,
    InputBundleEntry, InputBundleV1, InputFileMode, JobContract, JobContractArtifact,
    NetworkPolicy, OutputByteLimit, OutputName, SandboxPath,
};
use cairn_protocol::{ContentId, ContentType, JobId};
use cairn_record::ContentStore;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    ArgumentIndex, AssembledBoundaryCaseInput, AssembledInputValueCaseInput,
    AssembledMemorySurfaceCaseInput, BufferName, CaseExpectedOutcome, CorpusBufferByteLength,
    InvalidInputBehavior, MaterializedAbiArgumentV1, MaterializedBoundaryCaseArtifact,
    MaterializedInputValueCaseArtifact, MaterializedMemorySurfaceCaseArtifact,
    MigrationExecutionNeed, MigrationValidationTier, StatusCode,
};

const ADAPTER_DIRECTORY: &str = "cairn/bin";
const ADAPTER_PATH: &str = "cairn/bin/call-adapter";
const REQUEST_PATH: &str = "cairn/call-adapter-request.json";
const INVOCATION_PATH: &str = "cairn/invocation.json";
const RESULT_PATH: &str = "cairn/call-adapter-result.json";
const CONTAINER_REQUEST_PATH: &str = "/cairn/input/cairn/call-adapter-request.json";
const CONTAINER_OUTPUT_ROOT: &str = "/cairn/output";
const WORKING_DIRECTORY: &str = "work";

/// Failure to bind a case bundle to one bounded isolated adapter executable.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CallAdapterProtocolError {
    /// Only the current pre-release V1 request is accepted.
    #[error("call-adapter request schema version must be 1")]
    UnsupportedSchemaVersion,
    /// An executable must contain at least one byte.
    #[error("call-adapter executable is empty")]
    EmptyExecutable,
    /// Executable bytes exceed the caller-supplied bound.
    #[error("call-adapter executable exceeds its byte limit")]
    ExecutableLimitExceeded,
    /// The supplied source bundle bytes do not match their typed identity.
    #[error("source case input bundle identity mismatch")]
    SourceBundleMismatch,
    /// Persisted request fields contradict the fixed process protocol.
    #[error("call-adapter request is inconsistent")]
    InconsistentRequest,
    /// The result file or declared ABI outputs are absent, duplicated, extra, or contradictory.
    #[error("call-adapter capture is inconsistent")]
    InconsistentCapture,
    /// A receipt, job contract, and prepared adapter input do not describe the same execution.
    #[error("call-adapter execution receipt binding is inconsistent")]
    InconsistentExecutionReceipt,
    /// Only a successful generic execution can yield an operator observation.
    #[error("call-adapter execution did not succeed: {outcome:?}")]
    ExecutionDidNotSucceed { outcome: ExecutionOutcome },
    /// A typed execution artifact could not be read with verified content identity.
    #[error("call-adapter execution content could not be read: {message}")]
    Content { message: String },
    /// Adapter completion contradicts the caller-declared expected outcome.
    #[error("call-adapter completion contradicts the case expectation")]
    UnexpectedCompletion,
    /// Product execution intent could not become one canonical generic job contract.
    #[error("call-adapter job composition failed: {message}")]
    JobComposition { message: String },
    /// Canonical encoding, path, command, or bundle construction failed.
    #[error("call-adapter protocol codec error: {message}")]
    Codec { message: String },
}

/// Positive maximum size accepted for one adapter executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallAdapterExecutableByteLimit(u64);

impl CallAdapterExecutableByteLimit {
    /// Creates a positive executable bound.
    ///
    /// # Errors
    ///
    /// Rejects zero, which could never admit an executable.
    pub const fn new(value: u64) -> Result<Self, CallAdapterProtocolError> {
        if value == 0 {
            Err(CallAdapterProtocolError::ExecutableLimitExceeded)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the exact byte bound.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Content domain for exact executable bytes of an operator-specific call adapter.
pub enum CallAdapterExecutableArtifact {}

impl ContentType for CallAdapterExecutableArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-executable.v1";
}

/// Strong identity of the exact case manifest the adapter must execute.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CorpusInvocationIdentityV1 {
    /// Quantitative boundary invocation.
    Boundary {
        manifest: ContentId<MaterializedBoundaryCaseArtifact>,
    },
    /// Supported or invalid dtype invocation.
    InputValue {
        manifest: ContentId<MaterializedInputValueCaseArtifact>,
    },
    /// Pointer, capacity, or aliasing invocation.
    MemorySurface {
        manifest: ContentId<MaterializedMemorySurfaceCaseArtifact>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CallAdapterSchemaV1;

impl Serialize for CallAdapterSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for CallAdapterSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                CallAdapterProtocolError::UnsupportedSchemaVersion,
            )),
        }
    }
}

/// Strict V1 request read by a CUDA, Ascend C, or other isolated adapter process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CallAdapterRequestWire")]
pub struct CallAdapterRequestV1 {
    schema_version: CallAdapterSchemaV1,
    source_input_bundle: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    invocation_path: SandboxPath,
    result_path: SandboxPath,
    expected_outputs: Vec<CallAdapterExpectedOutputV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallAdapterRequestWire {
    schema_version: CallAdapterSchemaV1,
    source_input_bundle: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    invocation_path: SandboxPath,
    result_path: SandboxPath,
    expected_outputs: Vec<CallAdapterExpectedOutputV1>,
}

/// One ABI buffer file the adapter must write after an actual invocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallAdapterExpectedOutputV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    byte_length: CorpusBufferByteLength,
    path: SandboxPath,
}

impl CallAdapterExpectedOutputV1 {
    #[must_use]
    pub const fn argument_index(&self) -> ArgumentIndex {
        self.argument_index
    }

    #[must_use]
    pub const fn buffer(&self) -> &BufferName {
        &self.buffer
    }

    #[must_use]
    pub const fn byte_length(&self) -> CorpusBufferByteLength {
        self.byte_length
    }

    #[must_use]
    pub const fn path(&self) -> &SandboxPath {
        &self.path
    }
}

impl CallAdapterRequestV1 {
    fn new(
        source_input_bundle: ContentId<InputBundleArtifact>,
        invocation: CorpusInvocationIdentityV1,
        executable: ContentId<CallAdapterExecutableArtifact>,
        expected_outputs: Vec<CallAdapterExpectedOutputV1>,
    ) -> Result<Self, CallAdapterProtocolError> {
        let mut buffers = BTreeSet::new();
        if expected_outputs.windows(2).any(|pair| {
            pair[0].argument_index >= pair[1].argument_index || pair[0].path >= pair[1].path
        }) || expected_outputs.iter().any(|output| {
            (match output_path(output.argument_index) {
                Ok(expected) => expected != output.path,
                Err(_) => true,
            }) || !buffers.insert(&output.buffer)
        }) {
            return Err(CallAdapterProtocolError::InconsistentRequest);
        }
        Ok(Self {
            schema_version: CallAdapterSchemaV1,
            source_input_bundle,
            invocation,
            executable,
            invocation_path: path(INVOCATION_PATH)?,
            result_path: path(RESULT_PATH)?,
            expected_outputs,
        })
    }

    /// Returns the unmodified case bundle from which this process input was composed.
    #[must_use]
    pub const fn source_input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.source_input_bundle
    }

    /// Returns the exact typed invocation manifest identity.
    #[must_use]
    pub const fn invocation(&self) -> CorpusInvocationIdentityV1 {
        self.invocation
    }

    /// Returns the executable-byte identity selected for this invocation.
    #[must_use]
    pub const fn executable(&self) -> ContentId<CallAdapterExecutableArtifact> {
        self.executable
    }

    /// Returns the input-root-relative invocation manifest path.
    #[must_use]
    pub const fn invocation_path(&self) -> &SandboxPath {
        &self.invocation_path
    }

    /// Returns the output-root-relative result manifest path.
    #[must_use]
    pub const fn result_path(&self) -> &SandboxPath {
        &self.result_path
    }

    /// Returns ABI output files in strict argument order.
    #[must_use]
    pub fn expected_outputs(&self) -> &[CallAdapterExpectedOutputV1] {
        &self.expected_outputs
    }
}

impl TryFrom<CallAdapterRequestWire> for CallAdapterRequestV1 {
    type Error = CallAdapterProtocolError;

    fn try_from(wire: CallAdapterRequestWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        let request = Self::new(
            wire.source_input_bundle,
            wire.invocation,
            wire.executable,
            wire.expected_outputs,
        )?;
        if request.invocation_path != wire.invocation_path
            || request.result_path != wire.result_path
        {
            return Err(CallAdapterProtocolError::InconsistentRequest);
        }
        Ok(request)
    }
}

/// Content identity domain for an exact isolated call-adapter request.
pub enum CallAdapterRequestArtifact {}

impl ContentType for CallAdapterRequestArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-request.v1";
}

/// Process-level completion reported by the exact adapter executable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CallAdapterCompletionV1 {
    /// Trusted input validation rejected the case before calling candidate code.
    RejectedBeforeInvocation,
    /// Candidate code was called and returned through a void ABI.
    InvokedVoid,
    /// Candidate code was called and returned a typed operator status.
    InvokedStatus { status: StatusCode },
}

/// Content domain for exact bytes captured from one output-capable ABI argument.
pub enum CallAdapterOutputBytesArtifact {}

impl ContentType for CallAdapterOutputBytesArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-output-bytes.v1";
}

/// One output identity reported by an adapter result manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallAdapterObservedOutputV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    byte_length: CorpusBufferByteLength,
    bytes: ContentId<CallAdapterOutputBytesArtifact>,
}

impl CallAdapterObservedOutputV1 {
    /// Creates metadata for exact output bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if byte length or content identity cannot be represented.
    pub fn from_bytes(
        argument_index: ArgumentIndex,
        buffer: BufferName,
        bytes: &[u8],
    ) -> Result<Self, CallAdapterProtocolError> {
        Ok(Self {
            argument_index,
            buffer,
            byte_length: CorpusBufferByteLength::new(u64::try_from(bytes.len()).map_err(codec)?),
            bytes: ContentId::<CallAdapterOutputBytesArtifact>::derive(bytes).map_err(codec)?,
        })
    }

    #[must_use]
    pub const fn argument_index(&self) -> ArgumentIndex {
        self.argument_index
    }

    #[must_use]
    pub const fn buffer(&self) -> &BufferName {
        &self.buffer
    }

    #[must_use]
    pub const fn byte_length(&self) -> CorpusBufferByteLength {
        self.byte_length
    }

    #[must_use]
    pub const fn bytes(&self) -> ContentId<CallAdapterOutputBytesArtifact> {
        self.bytes
    }
}

/// Strict V1 adapter-reported result. Validation against captured files and case expectation is
/// separate; decoding this structure alone grants no semantic authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CallAdapterResultWire")]
pub struct CallAdapterResultV1 {
    schema_version: CallAdapterSchemaV1,
    request: ContentId<CallAdapterRequestArtifact>,
    invocation: CorpusInvocationIdentityV1,
    completion: CallAdapterCompletionV1,
    outputs: Vec<CallAdapterObservedOutputV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallAdapterResultWire {
    schema_version: CallAdapterSchemaV1,
    request: ContentId<CallAdapterRequestArtifact>,
    invocation: CorpusInvocationIdentityV1,
    completion: CallAdapterCompletionV1,
    outputs: Vec<CallAdapterObservedOutputV1>,
}

impl CallAdapterResultV1 {
    /// Creates a canonical adapter-reported result.
    ///
    /// # Errors
    ///
    /// Rejects duplicate, unordered, or contradictory output metadata.
    pub fn new(
        request: ContentId<CallAdapterRequestArtifact>,
        invocation: CorpusInvocationIdentityV1,
        completion: CallAdapterCompletionV1,
        outputs: Vec<CallAdapterObservedOutputV1>,
    ) -> Result<Self, CallAdapterProtocolError> {
        if outputs
            .windows(2)
            .any(|pair| pair[0].argument_index >= pair[1].argument_index)
            || (completion == CallAdapterCompletionV1::RejectedBeforeInvocation
                && !outputs.is_empty())
        {
            return Err(CallAdapterProtocolError::InconsistentCapture);
        }
        Ok(Self {
            schema_version: CallAdapterSchemaV1,
            request,
            invocation,
            completion,
            outputs,
        })
    }

    #[must_use]
    pub const fn completion(&self) -> &CallAdapterCompletionV1 {
        &self.completion
    }

    #[must_use]
    pub fn outputs(&self) -> &[CallAdapterObservedOutputV1] {
        &self.outputs
    }
}

impl TryFrom<CallAdapterResultWire> for CallAdapterResultV1 {
    type Error = CallAdapterProtocolError;

    fn try_from(wire: CallAdapterResultWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.request, wire.invocation, wire.completion, wire.outputs)
    }
}

/// Content identity domain for one validated adapter-reported result manifest.
pub enum CallAdapterResultArtifact {}

impl ContentType for CallAdapterResultArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-result.v1";
}

/// Adapter result after exact captured files and caller-declared completion have been checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCallAdapterObservation {
    result: CallAdapterResultV1,
    result_id: ContentId<CallAdapterResultArtifact>,
}

impl ValidatedCallAdapterObservation {
    #[must_use]
    pub const fn result(&self) -> &CallAdapterResultV1 {
        &self.result
    }

    #[must_use]
    pub const fn result_id(&self) -> ContentId<CallAdapterResultArtifact> {
        self.result_id
    }
}

/// Validated operator observation tied to one authoritative generic execution receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCallAdapterExecution {
    receipt: ExecutionReceipt,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    observation: ValidatedCallAdapterObservation,
}

impl ValidatedCallAdapterExecution {
    /// Returns the canonical generic execution receipt.
    #[must_use]
    pub const fn receipt(&self) -> &ExecutionReceipt {
        &self.receipt
    }

    /// Returns the exact receipt artifact identity.
    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt_id
    }

    /// Returns the validated adapter result and ABI-output observation.
    #[must_use]
    pub const fn observation(&self) -> &ValidatedCallAdapterObservation {
        &self.observation
    }
}

/// Independent bounds used when the generic executor captures one adapter process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallAdapterCaptureLimits {
    /// Maximum untrusted stdout bytes.
    pub stdout: OutputByteLimit,
    /// Maximum untrusted stderr bytes.
    pub stderr: OutputByteLimit,
    /// Maximum strict result-manifest bytes.
    pub result: OutputByteLimit,
    /// Maximum durable executor diagnostic bytes.
    pub diagnostic: DiagnosticByteLimit,
    /// Maximum trusted supervisor-evidence bytes.
    pub evidence: EvidenceByteLimit,
}

/// Canonical generic execution contract plus its pre-archival typed identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCallAdapterJob {
    tier: MigrationValidationTier,
    contract: JobContract,
    contract_bytes: Vec<u8>,
    contract_id: ContentId<JobContractArtifact>,
}

impl PreparedCallAdapterJob {
    /// Returns product-owned orchestration position, absent from the generic contract bytes.
    #[must_use]
    pub const fn tier(&self) -> MigrationValidationTier {
        self.tier
    }

    /// Returns the domain-neutral worker job contract.
    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.contract
    }

    /// Returns canonical contract bytes ready for content archival.
    #[must_use]
    pub fn contract_bytes(&self) -> &[u8] {
        &self.contract_bytes
    }

    /// Returns the exact generic execution-contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }
}

/// Complete process input ready for execution-job composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCallAdapterInput {
    request: CallAdapterRequestV1,
    request_id: ContentId<CallAdapterRequestArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    command: CommandContract,
}

impl PreparedCallAdapterInput {
    #[must_use]
    pub const fn request(&self) -> &CallAdapterRequestV1 {
        &self.request
    }

    #[must_use]
    pub const fn request_id(&self) -> ContentId<CallAdapterRequestArtifact> {
        self.request_id
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
    pub const fn command(&self) -> &CommandContract {
        &self.command
    }

    /// Builds the exact executor capture declarations for the result manifest and ABI outputs.
    ///
    /// # Errors
    ///
    /// Rejects an invalid result-manifest limit or an output length not representable by the
    /// generic positive capture bound.
    pub fn declared_outputs(
        &self,
        result_limit: OutputByteLimit,
    ) -> Result<Vec<ExpectedOutput>, CallAdapterProtocolError> {
        let mut outputs = vec![ExpectedOutput {
            name: output_name("call-adapter-result")?,
            path: self.request.result_path.clone(),
            byte_limit: result_limit,
        }];
        for output in &self.request.expected_outputs {
            outputs.push(ExpectedOutput {
                name: output_name_for_argument(output.argument_index)?,
                path: output.path.clone(),
                byte_limit: OutputByteLimit::new(output.byte_length.get().max(1)).map_err(codec)?,
            });
        }
        outputs.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(outputs)
    }
}

/// Composes one prepared adapter process into the existing vendor-neutral worker contract.
///
/// The product validation tier is retained only by the returned migration wrapper. It is not copied
/// into `JobContract`, worker profiles, placement selectors, or capture material.
///
/// # Errors
///
/// Rejects contradictory migration execution intent, invalid output declarations, or canonical
/// contract encoding/identity failure.
pub fn compose_call_adapter_job(
    job_id: JobId,
    input: &PreparedCallAdapterInput,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: CallAdapterCaptureLimits,
) -> Result<PreparedCallAdapterJob, CallAdapterProtocolError> {
    let resources = need.to_resource_request().map_err(job_composition_error)?;
    let expected_outputs = input.declared_outputs(limits.result)?;
    let capture = CapturePolicy::new(
        limits.stdout,
        limits.stderr,
        limits.diagnostic,
        limits.evidence,
        expected_outputs,
    )
    .map_err(job_composition_error)?;
    let contract = JobContract::new(
        job_id,
        input.input_bundle_id,
        environment,
        need.backend().clone(),
        input.command.clone(),
        resources,
        NetworkPolicy::Disabled,
        capture,
    );
    let contract_bytes = cairn_codec::to_vec(&contract).map_err(job_composition_error)?;
    let contract_id =
        ContentId::<JobContractArtifact>::derive(&contract_bytes).map_err(job_composition_error)?;
    Ok(PreparedCallAdapterJob {
        tier: need.tier(),
        contract,
        contract_bytes,
        contract_id,
    })
}

/// Binds a quantitative boundary case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_boundary_call_adapter_input(
    case: &AssembledBoundaryCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::Boundary {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        case.manifest().arguments(),
        executable,
        limit,
    )
}

/// Binds a dtype case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_input_value_call_adapter_input(
    case: &AssembledInputValueCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::InputValue {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        case.manifest().arguments(),
        executable,
        limit,
    )
}

/// Binds a memory-surface case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_memory_surface_call_adapter_input(
    case: &AssembledMemorySurfaceCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::MemorySurface {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        case.manifest().arguments(),
        executable,
        limit,
    )
}

/// Validates a boundary-case adapter capture against its exact request and expected outcome.
///
/// # Errors
///
/// Rejects a request/case mismatch, missing/extra/tampered captured files, or completion that
/// contradicts the case expectation.
pub fn validate_boundary_call_adapter_capture(
    case: &AssembledBoundaryCaseInput,
    prepared: &PreparedCallAdapterInput,
    captured: &[CapturedOutput],
) -> Result<ValidatedCallAdapterObservation, CallAdapterProtocolError> {
    validate_capture(
        prepared,
        CorpusInvocationIdentityV1::Boundary {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        captured,
    )
}

/// Validates a dtype-case adapter capture against its exact request and expected outcome.
///
/// # Errors
///
/// Rejects a request/case mismatch, missing/extra/tampered captured files, or completion that
/// contradicts the case expectation.
pub fn validate_input_value_call_adapter_capture(
    case: &AssembledInputValueCaseInput,
    prepared: &PreparedCallAdapterInput,
    captured: &[CapturedOutput],
) -> Result<ValidatedCallAdapterObservation, CallAdapterProtocolError> {
    validate_capture(
        prepared,
        CorpusInvocationIdentityV1::InputValue {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        captured,
    )
}

/// Validates a memory-surface adapter capture against its exact request and expected outcome.
///
/// # Errors
///
/// Rejects a request/case mismatch, missing/extra/tampered captured files, or completion that
/// contradicts the case expectation.
pub fn validate_memory_surface_call_adapter_capture(
    case: &AssembledMemorySurfaceCaseInput,
    prepared: &PreparedCallAdapterInput,
    captured: &[CapturedOutput],
) -> Result<ValidatedCallAdapterObservation, CallAdapterProtocolError> {
    validate_capture(
        prepared,
        CorpusInvocationIdentityV1::MemorySurface {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
        captured,
    )
}

/// Validates an authoritative successful execution receipt for a boundary case and reads its
/// declared result files through the typed content store.
///
/// The receipt must come from generic execution completion or recovery. This function binds that
/// receipt to the prepared job and case; content storage by itself is not execution authority.
///
/// # Errors
///
/// Rejects a mismatched receipt/job/input, a non-success terminal outcome, unreadable declared
/// output content, or an invalid adapter capture.
pub fn validate_boundary_call_adapter_receipt<C: ContentStore>(
    case: &AssembledBoundaryCaseInput,
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
) -> Result<ValidatedCallAdapterExecution, CallAdapterProtocolError> {
    validate_receipt(
        input,
        job,
        receipt_id,
        receipt,
        content,
        CorpusInvocationIdentityV1::Boundary {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
    )
}

/// Validates an authoritative successful execution receipt for a dtype case and reads its declared
/// result files through the typed content store.
///
/// # Errors
///
/// Rejects a mismatched receipt/job/input, a non-success terminal outcome, unreadable declared
/// output content, or an invalid adapter capture.
pub fn validate_input_value_call_adapter_receipt<C: ContentStore>(
    case: &AssembledInputValueCaseInput,
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
) -> Result<ValidatedCallAdapterExecution, CallAdapterProtocolError> {
    validate_receipt(
        input,
        job,
        receipt_id,
        receipt,
        content,
        CorpusInvocationIdentityV1::InputValue {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
    )
}

/// Validates an authoritative successful execution receipt for a memory-surface case and reads its
/// declared result files through the typed content store.
///
/// # Errors
///
/// Rejects a mismatched receipt/job/input, a non-success terminal outcome, unreadable declared
/// output content, or an invalid adapter capture.
pub fn validate_memory_surface_call_adapter_receipt<C: ContentStore>(
    case: &AssembledMemorySurfaceCaseInput,
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
) -> Result<ValidatedCallAdapterExecution, CallAdapterProtocolError> {
    validate_receipt(
        input,
        job,
        receipt_id,
        receipt,
        content,
        CorpusInvocationIdentityV1::MemorySurface {
            manifest: case.manifest_id(),
        },
        case.manifest().expected_outcome(),
    )
}

fn validate_receipt<C: ContentStore>(
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
    invocation: CorpusInvocationIdentityV1,
    expected_outcome: &CaseExpectedOutcome,
) -> Result<ValidatedCallAdapterExecution, CallAdapterProtocolError> {
    validate_job_binding(input, job)?;
    let receipt_bytes = cairn_codec::to_vec(receipt).map_err(codec)?;
    if ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).map_err(codec)? != receipt_id
        || receipt.job_id() != job.contract.job_id()
        || receipt.contract_id() != job.contract_id
    {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }
    if receipt.outcome() != ExecutionOutcome::Succeeded {
        return Err(CallAdapterProtocolError::ExecutionDidNotSucceed {
            outcome: receipt.outcome(),
        });
    }
    if receipt.exit_code() != Some(0) {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }

    let declarations = job.contract.capture().expected_outputs();
    if receipt.outputs().len() != declarations.len()
        || receipt
            .outputs()
            .iter()
            .zip(declarations)
            .any(|(archived, declared)| archived.name != declared.name)
    {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }
    let mut captured = Vec::with_capacity(receipt.outputs().len());
    for output in receipt.outputs() {
        let mut bytes = Vec::new();
        content
            .write_to::<DeclaredOutputArtifact>(&output.content_id, &mut bytes)
            .map_err(content_error)?;
        captured.push(CapturedOutput {
            name: output.name.clone(),
            bytes,
        });
    }
    let observation = validate_capture(input, invocation, expected_outcome, &captured)?;
    Ok(ValidatedCallAdapterExecution {
        receipt: receipt.clone(),
        receipt_id,
        observation,
    })
}

fn validate_job_binding(
    input: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
) -> Result<(), CallAdapterProtocolError> {
    let canonical_contract = cairn_codec::to_vec(&job.contract).map_err(codec)?;
    if canonical_contract != job.contract_bytes
        || ContentId::<JobContractArtifact>::derive(&canonical_contract).map_err(codec)?
            != job.contract_id
        || job.contract.input_bundle_id() != input.input_bundle_id
        || job.contract.command() != &input.command
    {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }
    let declarations = job.contract.capture().expected_outputs();
    if declarations.len() != input.request.expected_outputs.len().saturating_add(1) {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }
    let result_name = output_name("call-adapter-result")?;
    if !declarations
        .iter()
        .any(|output| output.name == result_name && output.path == input.request.result_path)
    {
        return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
    }
    for expected in &input.request.expected_outputs {
        let name = output_name_for_argument(expected.argument_index)?;
        let byte_limit = OutputByteLimit::new(expected.byte_length.get().max(1)).map_err(codec)?;
        if !declarations.iter().any(|output| {
            output.name == name && output.path == expected.path && output.byte_limit == byte_limit
        }) {
            return Err(CallAdapterProtocolError::InconsistentExecutionReceipt);
        }
    }
    Ok(())
}

fn validate_capture(
    prepared: &PreparedCallAdapterInput,
    invocation: CorpusInvocationIdentityV1,
    expected_outcome: &CaseExpectedOutcome,
    captured: &[CapturedOutput],
) -> Result<ValidatedCallAdapterObservation, CallAdapterProtocolError> {
    if prepared.request.invocation != invocation {
        return Err(CallAdapterProtocolError::InconsistentCapture);
    }
    let result_name = output_name("call-adapter-result")?;
    let mut names = BTreeSet::new();
    if captured.iter().any(|output| !names.insert(&output.name)) {
        return Err(CallAdapterProtocolError::InconsistentCapture);
    }
    let result_bytes = captured
        .iter()
        .find(|output| output.name == result_name)
        .map(|output| output.bytes.as_slice())
        .ok_or(CallAdapterProtocolError::InconsistentCapture)?;
    let result: CallAdapterResultV1 = cairn_codec::from_slice(result_bytes).map_err(codec)?;
    if result.request != prepared.request_id || result.invocation != invocation {
        return Err(CallAdapterProtocolError::InconsistentCapture);
    }
    validate_completion(expected_outcome, &result.completion)?;
    let invoked = result.completion != CallAdapterCompletionV1::RejectedBeforeInvocation;
    let expected_outputs = if invoked {
        prepared.request.expected_outputs.as_slice()
    } else {
        &[]
    };
    if result.outputs.len() != expected_outputs.len()
        || captured.len() != expected_outputs.len().saturating_add(1)
    {
        return Err(CallAdapterProtocolError::InconsistentCapture);
    }
    for (expected, observed) in expected_outputs.iter().zip(&result.outputs) {
        if observed.argument_index != expected.argument_index
            || observed.buffer != expected.buffer
            || observed.byte_length != expected.byte_length
        {
            return Err(CallAdapterProtocolError::InconsistentCapture);
        }
        let name = output_name_for_argument(expected.argument_index)?;
        let bytes = captured
            .iter()
            .find(|output| output.name == name)
            .map(|output| output.bytes.as_slice())
            .ok_or(CallAdapterProtocolError::InconsistentCapture)?;
        if u64::try_from(bytes.len()).ok() != Some(expected.byte_length.get())
            || ContentId::<CallAdapterOutputBytesArtifact>::derive(bytes).map_err(codec)?
                != observed.bytes
        {
            return Err(CallAdapterProtocolError::InconsistentCapture);
        }
    }
    let result_id = ContentId::<CallAdapterResultArtifact>::derive(result_bytes).map_err(codec)?;
    Ok(ValidatedCallAdapterObservation { result, result_id })
}

fn validate_completion(
    expected: &CaseExpectedOutcome,
    observed: &CallAdapterCompletionV1,
) -> Result<(), CallAdapterProtocolError> {
    let matches = match expected {
        CaseExpectedOutcome::Success => matches!(
            observed,
            CallAdapterCompletionV1::InvokedVoid | CallAdapterCompletionV1::InvokedStatus { .. }
        ),
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::RejectBeforeExecution,
        } => observed == &CallAdapterCompletionV1::RejectedBeforeInvocation,
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::ReturnStatus { status },
        } => matches!(
            observed,
            CallAdapterCompletionV1::InvokedStatus { status: actual } if actual == status
        ),
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::ExplicitlyExcluded,
        } => false,
    };
    if matches {
        Ok(())
    } else {
        Err(CallAdapterProtocolError::UnexpectedCompletion)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "source bytes/identity, typed invocation, expectation, arguments, and executable bound are independent trust inputs"
)]
fn prepare(
    source: &InputBundleV1,
    source_bytes: &[u8],
    source_id: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    expected_outcome: &CaseExpectedOutcome,
    arguments: &[MaterializedAbiArgumentV1],
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    if executable.is_empty() {
        return Err(CallAdapterProtocolError::EmptyExecutable);
    }
    if u64::try_from(executable.len()).map_or(true, |length| length > limit.get()) {
        return Err(CallAdapterProtocolError::ExecutableLimitExceeded);
    }
    if source.to_bytes().map_err(codec)? != source_bytes
        || ContentId::<InputBundleArtifact>::derive(source_bytes).map_err(codec)? != source_id
    {
        return Err(CallAdapterProtocolError::SourceBundleMismatch);
    }
    let executable_id =
        ContentId::<CallAdapterExecutableArtifact>::derive(executable).map_err(codec)?;
    let outputs = if expected_outcome == &CaseExpectedOutcome::Success {
        expected_outputs(arguments)?
    } else {
        Vec::new()
    };
    let request = CallAdapterRequestV1::new(source_id, invocation, executable_id, outputs)?;
    let request_bytes = cairn_codec::to_vec(&request).map_err(codec)?;
    let request_id =
        ContentId::<CallAdapterRequestArtifact>::derive(&request_bytes).map_err(codec)?;
    let mut entries = source.entries().to_vec();
    entries.extend([
        InputBundleEntry::Directory {
            path: path(ADAPTER_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: path(ADAPTER_PATH)?,
            mode: InputFileMode::Executable,
            bytes: executable.to_vec(),
        },
        InputBundleEntry::File {
            path: path(REQUEST_PATH)?,
            mode: InputFileMode::Data,
            bytes: request_bytes,
        },
    ]);
    let input_bundle = InputBundleV1::new(entries).map_err(codec)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(codec)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(codec)?;
    let command = CommandContract::new(
        path(ADAPTER_PATH)?,
        vec![
            argument("--request")?,
            argument(CONTAINER_REQUEST_PATH)?,
            argument("--output-root")?,
            argument(CONTAINER_OUTPUT_ROOT)?,
        ],
        path(WORKING_DIRECTORY)?,
    );
    Ok(PreparedCallAdapterInput {
        request,
        request_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        command,
    })
}

fn expected_outputs(
    arguments: &[MaterializedAbiArgumentV1],
) -> Result<Vec<CallAdapterExpectedOutputV1>, CallAdapterProtocolError> {
    arguments
        .iter()
        .filter_map(|argument| match argument {
            MaterializedAbiArgumentV1::OutputBuffer {
                argument_index,
                buffer,
                byte_length,
                ..
            }
            | MaterializedAbiArgumentV1::InputOutputBuffer {
                argument_index,
                buffer,
                byte_length,
                ..
            } => Some((*argument_index, buffer.clone(), *byte_length)),
            MaterializedAbiArgumentV1::InputBuffer { .. }
            | MaterializedAbiArgumentV1::Scalar { .. } => None,
        })
        .map(|(argument_index, buffer, byte_length)| {
            Ok(CallAdapterExpectedOutputV1 {
                argument_index,
                buffer,
                byte_length,
                path: output_path(argument_index)?,
            })
        })
        .collect()
}

fn output_path(index: ArgumentIndex) -> Result<SandboxPath, CallAdapterProtocolError> {
    path(&format!("cairn/abi/arg-{:05}.bin", index.get()))
}

fn output_name_for_argument(index: ArgumentIndex) -> Result<OutputName, CallAdapterProtocolError> {
    output_name(&format!("abi-output-{:05}", index.get()))
}

fn output_name(value: &str) -> Result<OutputName, CallAdapterProtocolError> {
    OutputName::new(value).map_err(codec)
}

fn path(value: &str) -> Result<SandboxPath, CallAdapterProtocolError> {
    SandboxPath::new(value).map_err(codec)
}

fn argument(value: &str) -> Result<CommandArgument, CallAdapterProtocolError> {
    CommandArgument::new(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CallAdapterProtocolError {
    CallAdapterProtocolError::Codec {
        message: error.to_string(),
    }
}

fn job_composition_error(error: impl std::fmt::Display) -> CallAdapterProtocolError {
    CallAdapterProtocolError::JobComposition {
        message: error.to_string(),
    }
}

fn content_error(error: impl std::fmt::Display) -> CallAdapterProtocolError {
    CallAdapterProtocolError::Content {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{Cursor, Read, Write},
    };

    use cairn_execution::{
        ArchivedOutput, CapturedOutput, DeclaredOutputArtifact, DiagnosticByteLimit,
        EvidenceByteLimit, ExecutionBackend, ExecutionElapsedMillis, ExecutionEnvironmentArtifact,
        ExecutionEvidenceArtifact, ExecutionOutcome, ExecutionReceipt, ExecutionReceiptArtifact,
        ExecutionStderrArtifact, ExecutionStdoutArtifact, ExecutionTimeoutMillis, InputBundleEntry,
        InputFileMode, JobContractArtifact, NetworkPolicy, OutputByteLimit,
    };
    use cairn_protocol::{AttemptId, BlobDigest, ContentId, ContentType, JobId};
    use cairn_record::{ContentDescriptor, ContentStore, ContentStoreError};
    use serde::Serialize;
    use serde_json::json;

    use super::{
        ADAPTER_PATH, CallAdapterCaptureLimits, CallAdapterCompletionV1,
        CallAdapterExecutableArtifact, CallAdapterExecutableByteLimit, CallAdapterObservedOutputV1,
        CallAdapterProtocolError, CallAdapterRequestArtifact, CallAdapterRequestV1,
        CallAdapterResultV1, CorpusInvocationIdentityV1, INVOCATION_PATH, PreparedCallAdapterInput,
        PreparedCallAdapterJob, REQUEST_PATH, compose_call_adapter_job, output_name,
        output_name_for_argument, path, prepare, validate_capture, validate_receipt,
    };
    use crate::{
        ArgumentIndex, BufferName, CaseExpectedOutcome, CorpusBufferByteLength, CorpusElementCount,
        DataType, ExtentValue, InvalidInputBehavior, MaterializedAbiArgumentV1,
        MaterializedBoundaryCaseArtifact, MigrationExecutionNeed, MigrationValidationTier,
        StatusCode,
    };

    fn id<T: ContentType>(bytes: &[u8]) -> ContentId<T> {
        ContentId::<T>::derive(bytes).expect("content identity")
    }

    #[derive(Default)]
    struct MemoryContentStore {
        objects: BTreeMap<String, Vec<u8>>,
    }

    impl ContentStore for MemoryContentStore {
        fn put<T: ContentType>(
            &mut self,
            reader: &mut dyn Read,
        ) -> Result<ContentDescriptor<T>, ContentStoreError> {
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|error| ContentStoreError::Io {
                    message: error.to_string(),
                })?;
            let content_id =
                ContentId::<T>::derive(&bytes).map_err(|error| ContentStoreError::Integrity {
                    message: error.to_string(),
                })?;
            self.objects.insert(content_id.to_string(), bytes.clone());
            Ok(ContentDescriptor {
                content_id,
                blob_digest: BlobDigest::derive(&bytes),
                byte_len: u64::try_from(bytes.len()).expect("test content length"),
            })
        }

        fn write_to<T: ContentType>(
            &self,
            content_id: &ContentId<T>,
            writer: &mut dyn Write,
        ) -> Result<ContentDescriptor<T>, ContentStoreError> {
            let bytes = self.objects.get(&content_id.to_string()).ok_or_else(|| {
                ContentStoreError::NotFound {
                    content_id: content_id.to_string(),
                }
            })?;
            let actual =
                ContentId::<T>::derive(bytes).map_err(|error| ContentStoreError::Integrity {
                    message: error.to_string(),
                })?;
            if actual != *content_id {
                return Err(ContentStoreError::Integrity {
                    message: "test object identity changed".to_owned(),
                });
            }
            writer
                .write_all(bytes)
                .map_err(|error| ContentStoreError::Io {
                    message: error.to_string(),
                })?;
            Ok(ContentDescriptor {
                content_id: *content_id,
                blob_digest: BlobDigest::derive(bytes),
                byte_len: u64::try_from(bytes.len()).expect("test content length"),
            })
        }
    }

    impl MemoryContentStore {
        fn archive<T: ContentType>(&mut self, bytes: &[u8]) -> ContentId<T> {
            self.put::<T>(&mut Cursor::new(bytes))
                .expect("archive test content")
                .content_id
        }
    }

    #[derive(Serialize)]
    struct ExecutionReceiptWire {
        schema_version: u16,
        job_id: JobId,
        attempt_id: AttemptId,
        contract_id: ContentId<JobContractArtifact>,
        outcome: ExecutionOutcome,
        exit_code: Option<i32>,
        elapsed_ms: ExecutionElapsedMillis,
        stdout_id: ContentId<ExecutionStdoutArtifact>,
        stderr_id: ContentId<ExecutionStderrArtifact>,
        evidence_id: ContentId<ExecutionEvidenceArtifact>,
        outputs: Vec<ArchivedOutput>,
    }

    fn build_receipt(
        job_id: JobId,
        contract_id: ContentId<JobContractArtifact>,
        outcome: ExecutionOutcome,
        exit_code: Option<i32>,
        outputs: Vec<ArchivedOutput>,
    ) -> (ExecutionReceipt, Vec<u8>) {
        let wire = ExecutionReceiptWire {
            schema_version: 1,
            job_id,
            attempt_id: AttemptId::new(),
            contract_id,
            outcome,
            exit_code,
            elapsed_ms: ExecutionElapsedMillis::new(7),
            stdout_id: id::<ExecutionStdoutArtifact>(b"stdout"),
            stderr_id: id::<ExecutionStderrArtifact>(b"stderr"),
            evidence_id: id::<ExecutionEvidenceArtifact>(b"evidence"),
            outputs,
        };
        let bytes = cairn_codec::to_vec(&wire).expect("receipt bytes");
        let receipt = cairn_codec::from_slice(&bytes).expect("receipt decode");
        (receipt, bytes)
    }

    fn source_bundle() -> (
        cairn_execution::InputBundleV1,
        Vec<u8>,
        ContentId<cairn_execution::InputBundleArtifact>,
    ) {
        let bundle = cairn_execution::InputBundleV1::new(vec![
            InputBundleEntry::Directory {
                path: path("cairn").expect("root"),
            },
            InputBundleEntry::Directory {
                path: path("cairn/abi").expect("abi"),
            },
            InputBundleEntry::File {
                path: path(INVOCATION_PATH).expect("invocation"),
                mode: InputFileMode::Data,
                bytes: b"invocation".to_vec(),
            },
        ])
        .expect("source bundle");
        let bytes = bundle.to_bytes().expect("source bytes");
        let identity = id(&bytes);
        (bundle, bytes, identity)
    }

    struct SuccessfulReceiptFixture {
        input: PreparedCallAdapterInput,
        job: PreparedCallAdapterJob,
        invocation: CorpusInvocationIdentityV1,
        content: MemoryContentStore,
        result: CallAdapterResultV1,
        result_bytes: Vec<u8>,
        result_content: ContentId<DeclaredOutputArtifact>,
        receipt: ExecutionReceipt,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
    }

    fn prepared_output_job() -> (
        PreparedCallAdapterInput,
        PreparedCallAdapterJob,
        CorpusInvocationIdentityV1,
    ) {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let output_argument = MaterializedAbiArgumentV1::OutputBuffer {
            argument_index: ArgumentIndex::new(2),
            buffer: BufferName::new("output").expect("buffer"),
            data_type: DataType::F32,
            extents: vec![ExtentValue::new(2)],
            element_count: CorpusElementCount::new(2),
            byte_length: CorpusBufferByteLength::new(8),
        };
        let input = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Success,
            &[output_argument],
            b"ELF",
            CallAdapterExecutableByteLimit::new(4).expect("executable limit"),
        )
        .expect("adapter input");
        let need = MigrationExecutionNeed::new(
            MigrationValidationTier::V3TargetDevice,
            ExecutionBackend::new("docker-v1").expect("backend"),
            ExecutionTimeoutMillis::new(30_000).expect("timeout"),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        )
        .expect("execution need");
        let limits = CallAdapterCaptureLimits {
            stdout: OutputByteLimit::new(1024).expect("stdout"),
            stderr: OutputByteLimit::new(1024).expect("stderr"),
            result: OutputByteLimit::new(4096).expect("result"),
            diagnostic: DiagnosticByteLimit::new(512).expect("diagnostic"),
            evidence: EvidenceByteLimit::new(1024).expect("evidence"),
        };
        let job = compose_call_adapter_job(
            JobId::new(),
            &input,
            id::<ExecutionEnvironmentArtifact>(b"environment"),
            &need,
            limits,
        )
        .expect("job");
        (input, job, invocation)
    }

    fn successful_receipt_fixture() -> SuccessfulReceiptFixture {
        let (input, job, invocation) = prepared_output_job();
        let output_bytes = [1_u8, 2, 3, 4, 5, 6, 7, 8];
        let result = CallAdapterResultV1::new(
            input.request_id(),
            invocation,
            CallAdapterCompletionV1::InvokedVoid,
            vec![
                CallAdapterObservedOutputV1::from_bytes(
                    ArgumentIndex::new(2),
                    BufferName::new("output").expect("buffer"),
                    &output_bytes,
                )
                .expect("observed output"),
            ],
        )
        .expect("result");
        let result_bytes = cairn_codec::to_vec(&result).expect("result bytes");
        let mut content = MemoryContentStore::default();
        let result_content = content.archive::<DeclaredOutputArtifact>(&result_bytes);
        let output_content = content.archive::<DeclaredOutputArtifact>(&output_bytes);
        let archived = job
            .contract()
            .capture()
            .expected_outputs()
            .iter()
            .map(|expected| ArchivedOutput {
                name: expected.name.clone(),
                content_id: if expected.name.as_str() == "call-adapter-result" {
                    result_content
                } else {
                    output_content
                },
            })
            .collect();
        let (receipt, receipt_bytes) = build_receipt(
            job.contract().job_id(),
            job.contract_id(),
            ExecutionOutcome::Succeeded,
            Some(0),
            archived,
        );
        SuccessfulReceiptFixture {
            input,
            job,
            invocation,
            content,
            result,
            result_bytes,
            result_content,
            receipt,
            receipt_id: id::<ExecutionReceiptArtifact>(&receipt_bytes),
        }
    }

    #[test]
    fn process_input_binds_executable_request_source_and_fixed_command() {
        let (source, source_bytes, source_id) = source_bundle();
        let executable = b"ELF-adapter";
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let prepared = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Success,
            &[],
            executable,
            CallAdapterExecutableByteLimit::new(64).expect("limit"),
        )
        .expect("prepare");

        assert_eq!(prepared.request().source_input_bundle(), source_id);
        assert_eq!(prepared.request().invocation(), invocation);
        assert_eq!(
            prepared.request().executable(),
            id::<CallAdapterExecutableArtifact>(executable)
        );
        assert_eq!(
            prepared.request_id(),
            id::<CallAdapterRequestArtifact>(
                &cairn_codec::to_vec(prepared.request()).expect("request bytes")
            )
        );
        assert_eq!(
            prepared.input_bundle_id(),
            id(prepared.input_bundle_bytes())
        );
        assert_eq!(prepared.command().program().as_str(), ADAPTER_PATH);
        assert_eq!(prepared.command().working_directory().as_str(), "work");
        assert_eq!(
            prepared
                .command()
                .arguments()
                .iter()
                .map(cairn_execution::CommandArgument::as_str)
                .collect::<Vec<_>>(),
            vec![
                "--request",
                "/cairn/input/cairn/call-adapter-request.json",
                "--output-root",
                "/cairn/output"
            ]
        );

        let executable_entry = prepared
            .input_bundle()
            .entries()
            .iter()
            .find(|entry| entry.path().as_str() == ADAPTER_PATH)
            .expect("adapter executable");
        assert!(matches!(
            executable_entry,
            InputBundleEntry::File {
                mode: InputFileMode::Executable,
                bytes,
                ..
            } if bytes == executable
        ));
        let request_bytes = prepared
            .input_bundle()
            .entries()
            .iter()
            .find_map(|entry| match entry {
                InputBundleEntry::File { path, bytes, .. } if path.as_str() == REQUEST_PATH => {
                    Some(bytes)
                }
                _ => None,
            })
            .expect("request file");
        assert_eq!(
            cairn_codec::from_slice::<CallAdapterRequestV1>(request_bytes).expect("request decode"),
            *prepared.request()
        );
    }

    #[test]
    fn executable_bounds_source_identity_and_persisted_request_fail_closed() {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let limit = CallAdapterExecutableByteLimit::new(4).expect("limit");
        assert_eq!(
            prepare(
                &source,
                &source_bytes,
                source_id,
                invocation,
                &CaseExpectedOutcome::Success,
                &[],
                b"",
                limit
            ),
            Err(CallAdapterProtocolError::EmptyExecutable)
        );
        assert_eq!(
            prepare(
                &source,
                &source_bytes,
                source_id,
                invocation,
                &CaseExpectedOutcome::Success,
                &[],
                b"12345",
                limit,
            ),
            Err(CallAdapterProtocolError::ExecutableLimitExceeded)
        );
        assert_eq!(
            prepare(
                &source,
                &source_bytes,
                id(b"wrong bundle"),
                invocation,
                &CaseExpectedOutcome::Success,
                &[],
                b"ELF",
                limit,
            ),
            Err(CallAdapterProtocolError::SourceBundleMismatch)
        );

        let prepared = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Success,
            &[],
            b"ELF",
            limit,
        )
        .expect("prepare");
        let value = serde_json::to_value(prepared.request()).expect("request json");
        let mut wrong_version = value.clone();
        wrong_version["schema_version"] = json!(2);
        assert!(serde_json::from_value::<CallAdapterRequestV1>(wrong_version).is_err());
        let mut wrong_path = value.clone();
        wrong_path["result_path"] = json!("legacy/result.json");
        assert!(serde_json::from_value::<CallAdapterRequestV1>(wrong_path).is_err());
        let mut unknown = value;
        unknown["fallback_python"] = json!(true);
        assert!(serde_json::from_value::<CallAdapterRequestV1>(unknown).is_err());
    }

    #[test]
    fn captured_result_binds_invocation_completion_and_exact_output_bytes() {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let output_argument = MaterializedAbiArgumentV1::OutputBuffer {
            argument_index: ArgumentIndex::new(2),
            buffer: BufferName::new("output").expect("buffer"),
            data_type: DataType::F32,
            extents: vec![ExtentValue::new(2)],
            element_count: CorpusElementCount::new(2),
            byte_length: CorpusBufferByteLength::new(8),
        };
        let prepared = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Success,
            &[output_argument],
            b"ELF",
            CallAdapterExecutableByteLimit::new(4).expect("limit"),
        )
        .expect("prepare");
        let output_bytes = [1_u8, 2, 3, 4, 5, 6, 7, 8];
        let observed = CallAdapterObservedOutputV1::from_bytes(
            ArgumentIndex::new(2),
            BufferName::new("output").expect("buffer"),
            &output_bytes,
        )
        .expect("observed output");
        let result = CallAdapterResultV1::new(
            prepared.request_id(),
            invocation,
            CallAdapterCompletionV1::InvokedVoid,
            vec![observed],
        )
        .expect("result");
        let result_bytes = cairn_codec::to_vec(&result).expect("result bytes");
        let captured = vec![
            CapturedOutput {
                name: output_name("call-adapter-result").expect("result name"),
                bytes: result_bytes.clone(),
            },
            CapturedOutput {
                name: output_name_for_argument(ArgumentIndex::new(2)).expect("output name"),
                bytes: output_bytes.to_vec(),
            },
        ];
        let validated = validate_capture(
            &prepared,
            invocation,
            &CaseExpectedOutcome::Success,
            &captured,
        )
        .expect("validated capture");
        assert_eq!(validated.result(), &result);
        assert_eq!(
            validated.result_id(),
            id::<super::CallAdapterResultArtifact>(&result_bytes)
        );

        let mut tampered = captured.clone();
        tampered[1].bytes[0] ^= 1;
        assert_eq!(
            validate_capture(
                &prepared,
                invocation,
                &CaseExpectedOutcome::Success,
                &tampered,
            ),
            Err(CallAdapterProtocolError::InconsistentCapture)
        );
        assert_eq!(
            validate_capture(
                &prepared,
                invocation,
                &CaseExpectedOutcome::Invalid {
                    behavior: InvalidInputBehavior::RejectBeforeExecution,
                },
                &captured,
            ),
            Err(CallAdapterProtocolError::UnexpectedCompletion)
        );
    }

    #[test]
    fn successful_receipt_loads_exact_declared_outputs_and_binds_execution() {
        let fixture = successful_receipt_fixture();
        let validated = validate_receipt(
            &fixture.input,
            &fixture.job,
            fixture.receipt_id,
            &fixture.receipt,
            &fixture.content,
            fixture.invocation,
            &CaseExpectedOutcome::Success,
        )
        .expect("validated execution");
        assert_eq!(validated.receipt_id(), fixture.receipt_id);
        assert_eq!(validated.receipt(), &fixture.receipt);
        assert_eq!(validated.observation().result(), &fixture.result);
        assert_eq!(
            validated.observation().result_id(),
            id::<super::CallAdapterResultArtifact>(&fixture.result_bytes)
        );
    }

    #[test]
    fn receipt_rejects_failed_outcome_and_wrong_contract() {
        let fixture = successful_receipt_fixture();
        assert_eq!(
            validate_receipt(
                &fixture.input,
                &fixture.job,
                id::<ExecutionReceiptArtifact>(b"wrong receipt"),
                &fixture.receipt,
                &fixture.content,
                fixture.invocation,
                &CaseExpectedOutcome::Success,
            ),
            Err(CallAdapterProtocolError::InconsistentExecutionReceipt)
        );
        let (failed, failed_bytes) = build_receipt(
            fixture.job.contract().job_id(),
            fixture.job.contract_id(),
            ExecutionOutcome::SubjectFailed,
            Some(9),
            Vec::new(),
        );
        assert_eq!(
            validate_receipt(
                &fixture.input,
                &fixture.job,
                id::<ExecutionReceiptArtifact>(&failed_bytes),
                &failed,
                &fixture.content,
                fixture.invocation,
                &CaseExpectedOutcome::Success,
            ),
            Err(CallAdapterProtocolError::ExecutionDidNotSucceed {
                outcome: ExecutionOutcome::SubjectFailed,
            })
        );

        let (wrong_contract, wrong_contract_bytes) = build_receipt(
            fixture.job.contract().job_id(),
            id::<JobContractArtifact>(b"wrong contract"),
            ExecutionOutcome::Succeeded,
            Some(0),
            Vec::new(),
        );
        assert_eq!(
            validate_receipt(
                &fixture.input,
                &fixture.job,
                id::<ExecutionReceiptArtifact>(&wrong_contract_bytes),
                &wrong_contract,
                &fixture.content,
                fixture.invocation,
                &CaseExpectedOutcome::Success,
            ),
            Err(CallAdapterProtocolError::InconsistentExecutionReceipt)
        );
    }

    #[test]
    fn receipt_rejects_missing_and_identity_changed_declared_content() {
        let mut fixture = successful_receipt_fixture();
        let missing_content = MemoryContentStore::default();
        assert!(matches!(
            validate_receipt(
                &fixture.input,
                &fixture.job,
                fixture.receipt_id,
                &fixture.receipt,
                &missing_content,
                fixture.invocation,
                &CaseExpectedOutcome::Success,
            ),
            Err(CallAdapterProtocolError::Content { .. })
        ));
        fixture
            .content
            .objects
            .insert(fixture.result_content.to_string(), b"tampered".to_vec());
        assert!(matches!(
            validate_receipt(
                &fixture.input,
                &fixture.job,
                fixture.receipt_id,
                &fixture.receipt,
                &fixture.content,
                fixture.invocation,
                &CaseExpectedOutcome::Success,
            ),
            Err(CallAdapterProtocolError::Content { .. })
        ));
    }

    #[test]
    fn reject_before_invocation_requires_no_abi_outputs() {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let prepared = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Invalid {
                behavior: InvalidInputBehavior::RejectBeforeExecution,
            },
            &[],
            b"ELF",
            CallAdapterExecutableByteLimit::new(4).expect("limit"),
        )
        .expect("prepare");
        let result = CallAdapterResultV1::new(
            prepared.request_id(),
            invocation,
            CallAdapterCompletionV1::RejectedBeforeInvocation,
            Vec::new(),
        )
        .expect("rejected result");
        let captured = [CapturedOutput {
            name: output_name("call-adapter-result").expect("result name"),
            bytes: cairn_codec::to_vec(&result).expect("result bytes"),
        }];
        validate_capture(
            &prepared,
            invocation,
            &CaseExpectedOutcome::Invalid {
                behavior: InvalidInputBehavior::RejectBeforeExecution,
            },
            &captured,
        )
        .expect("validated rejection");

        let status_result = CallAdapterResultV1::new(
            prepared.request_id(),
            invocation,
            CallAdapterCompletionV1::InvokedStatus {
                status: StatusCode::new(-7),
            },
            Vec::new(),
        )
        .expect("status result");
        let status_capture = [CapturedOutput {
            name: output_name("call-adapter-result").expect("result name"),
            bytes: cairn_codec::to_vec(&status_result).expect("result bytes"),
        }];
        validate_capture(
            &prepared,
            invocation,
            &CaseExpectedOutcome::Invalid {
                behavior: InvalidInputBehavior::ReturnStatus {
                    status: StatusCode::new(-7),
                },
            },
            &status_capture,
        )
        .expect("validated status");
        assert_eq!(
            validate_capture(
                &prepared,
                invocation,
                &CaseExpectedOutcome::Invalid {
                    behavior: InvalidInputBehavior::ReturnStatus {
                        status: StatusCode::new(-8),
                    },
                },
                &status_capture,
            ),
            Err(CallAdapterProtocolError::UnexpectedCompletion)
        );
    }

    #[test]
    fn job_composition_keeps_migration_tier_out_of_generic_worker_contract() {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let input = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            &CaseExpectedOutcome::Success,
            &[],
            b"ELF",
            CallAdapterExecutableByteLimit::new(4).expect("executable limit"),
        )
        .expect("adapter input");
        let execution_need = |tier| {
            MigrationExecutionNeed::new(
                tier,
                ExecutionBackend::new("docker-v1").expect("backend"),
                ExecutionTimeoutMillis::new(30_000).expect("timeout"),
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
            )
            .expect("execution need")
        };
        let limits = CallAdapterCaptureLimits {
            stdout: OutputByteLimit::new(1024).expect("stdout"),
            stderr: OutputByteLimit::new(2048).expect("stderr"),
            result: OutputByteLimit::new(4096).expect("result"),
            diagnostic: DiagnosticByteLimit::new(512).expect("diagnostic"),
            evidence: EvidenceByteLimit::new(1024).expect("evidence"),
        };
        let job_id = JobId::new();
        let environment = id::<ExecutionEnvironmentArtifact>(b"environment");
        let v3 = compose_call_adapter_job(
            job_id,
            &input,
            environment,
            &execution_need(MigrationValidationTier::V3TargetDevice),
            limits,
        )
        .expect("compose V3 job");
        let v1 = compose_call_adapter_job(
            job_id,
            &input,
            environment,
            &execution_need(MigrationValidationTier::V1SourceAccelerator),
            limits,
        )
        .expect("compose V1 job");

        assert_eq!(v3.tier(), MigrationValidationTier::V3TargetDevice);
        assert_eq!(v3.contract(), v1.contract());
        assert_eq!(v3.contract_id(), v1.contract_id());
        assert_eq!(v3.contract().input_bundle_id(), input.input_bundle_id());
        assert_eq!(v3.contract().environment_id(), environment);
        assert_eq!(v3.contract().network(), NetworkPolicy::Disabled);
        assert_eq!(v3.contract().capture().expected_outputs().len(), 1);
        assert_eq!(
            v3.contract().capture().expected_outputs()[0].name.as_str(),
            "call-adapter-result"
        );
        assert_eq!(
            v3.contract_id(),
            id::<JobContractArtifact>(v3.contract_bytes())
        );
        let generic_wire = String::from_utf8(v3.contract_bytes().to_vec()).expect("JSON");
        assert!(!generic_wire.contains("target-device"));
        assert!(!generic_wire.contains("source-accelerator"));
        assert!(!generic_wire.contains("migration"));
    }
}
