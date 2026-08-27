//! Materialization of a model-authored Oracle proposal into execution-ready zero-K matmul input.

use cairn_execution::{
    InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode, SandboxPath,
};
use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ArgumentIndex, BufferName, CallAdapterOutputBytesArtifact, CorpusBufferByteLength,
    CorpusElementCount, DataType, ExtentValue,
};

const MATERIAL_ROOT: &str = "cairn";
const ABI_DIRECTORY: &str = "cairn/abi";
const INVOCATION_PATH: &str = "cairn/invocation.json";
const REQUIRED_CASE_NAME: &str = "matmul-zero-k";

/// Exact IEEE-754 binary32 representation supplied by Blue without JSON floating-point ambiguity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleF32Bits(u32);

impl OracleF32Bits {
    /// Creates one exact binary32 bit pattern.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the exact binary32 bit pattern.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Rank-two shape used by the first executable Oracle slice.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleMatrixShapeV1 {
    rows: ExtentValue,
    columns: ExtentValue,
}

impl OracleMatrixShapeV1 {
    /// Creates one explicit matrix shape, including zero extents.
    #[must_use]
    pub const fn new(rows: ExtentValue, columns: ExtentValue) -> Self {
        Self { rows, columns }
    }

    #[must_use]
    pub const fn rows(self) -> ExtentValue {
        self.rows
    }

    #[must_use]
    pub const fn columns(self) -> ExtentValue {
        self.columns
    }
}

/// Comparison strength supported by the first executable Oracle slice.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutableOracleComparisonV1 {
    /// Every output byte must match the proposed IEEE-754 bit patterns.
    ExactBits,
    /// Every decoded f32 value must compare numerically equal; signed zero is normalized.
    F32NumericExact,
}

/// Model-authored proposal for one exact f32 zero-K matrix multiplication.
///
/// Decoding this value grants no authority. [`assemble_zero_k_matmul_f32_oracle`] revalidates all
/// cross-field arithmetic and creates the byte material and content identities used downstream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZeroKMatmulF32OracleCaseV1 {
    schema_version: u16,
    case_name: String,
    lhs_argument: ArgumentIndex,
    rhs_argument: ArgumentIndex,
    output_argument: ArgumentIndex,
    lhs_shape: OracleMatrixShapeV1,
    rhs_shape: OracleMatrixShapeV1,
    output_shape: OracleMatrixShapeV1,
    lhs_bits: Vec<OracleF32Bits>,
    rhs_bits: Vec<OracleF32Bits>,
    expected_output_bits: Vec<OracleF32Bits>,
    comparison: ExecutableOracleComparisonV1,
}

impl ZeroKMatmulF32OracleCaseV1 {
    /// Returns the authored comparator that downstream observation must use.
    #[must_use]
    pub const fn comparison(&self) -> ExecutableOracleComparisonV1 {
        self.comparison
    }

    /// Validates the exact dogfood sample contract without materializing any files.
    ///
    /// # Errors
    ///
    /// Rejects any non-V1, non-zero-K, shape-inconsistent, non-canonical ABI, or non-identity
    /// proposal. The sample is intentionally closed to `[2, 0] x [0, 3] -> [2, 3]`.
    pub fn validate_matmul_zero_k_sample(&self) -> Result<(), ExecutableOracleError> {
        self.validate_common()?;
        if self.lhs_shape != shape(2, 0)
            || self.rhs_shape != shape(0, 3)
            || self.output_shape != shape(2, 3)
        {
            return Err(ExecutableOracleError::SampleContractMismatch);
        }
        if self.comparison != ExecutableOracleComparisonV1::F32NumericExact {
            return Err(ExecutableOracleError::SampleComparatorOverconstrained);
        }
        Ok(())
    }

    fn validate_common(&self) -> Result<(), ExecutableOracleError> {
        if self.schema_version != 1 {
            return Err(ExecutableOracleError::UnsupportedSchemaVersion);
        }
        if self.case_name != REQUIRED_CASE_NAME {
            return Err(ExecutableOracleError::InvalidCaseName);
        }
        if [
            self.lhs_argument.get(),
            self.rhs_argument.get(),
            self.output_argument.get(),
        ] != [0, 1, 2]
        {
            return Err(ExecutableOracleError::NonCanonicalAbi);
        }
        let lhs_k = self.lhs_shape.columns.get();
        let rhs_k = self.rhs_shape.rows.get();
        if lhs_k != 0 || rhs_k != 0 || lhs_k != rhs_k {
            return Err(ExecutableOracleError::NotZeroK);
        }
        if self.output_shape.rows != self.lhs_shape.rows
            || self.output_shape.columns != self.rhs_shape.columns
        {
            return Err(ExecutableOracleError::ShapeMismatch);
        }
        require_bits(&self.lhs_bits, element_count(self.lhs_shape)?)?;
        require_bits(&self.rhs_bits, element_count(self.rhs_shape)?)?;
        require_bits(
            &self.expected_output_bits,
            element_count(self.output_shape)?,
        )?;
        if self
            .expected_output_bits
            .iter()
            .any(|bits| f32::from_bits(bits.get()) != 0.0)
        {
            return Err(ExecutableOracleError::ExpectedNumericZeroMismatch);
        }
        Ok(())
    }
}

/// Raw input bytes proposed for execution. Their identity is separate from observed output bytes.
pub enum ExecutableOracleInputBytesArtifact {}

impl ContentType for ExecutableOracleInputBytesArtifact {
    const DOMAIN: &'static str = "migration.executable-oracle-input-bytes.v1";
}

/// Typed identity of the invocation manifest exposed to a call adapter.
pub enum ExecutableOracleInvocationArtifact {}

impl ContentType for ExecutableOracleInvocationArtifact {
    const DOMAIN: &'static str = "migration.executable-oracle-invocation.v1";
}

/// Content domain for one trusted-code-produced Oracle output comparison body.
pub enum ExecutableOracleOutputComparisonArtifact {}

impl ContentType for ExecutableOracleOutputComparisonArtifact {
    const DOMAIN: &'static str = "migration.executable-oracle-output-comparison.v1";
}

/// One materialized read-only f32 input buffer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableOracleInputBufferV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    data_type: DataType,
    shape: OracleMatrixShapeV1,
    element_count: CorpusElementCount,
    byte_length: CorpusBufferByteLength,
    path: SandboxPath,
    bytes: ContentId<ExecutableOracleInputBytesArtifact>,
}

/// One materialized write-only f32 output allocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableOracleOutputBufferV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    data_type: DataType,
    shape: OracleMatrixShapeV1,
    element_count: CorpusElementCount,
    byte_length: CorpusBufferByteLength,
}

impl ExecutableOracleOutputBufferV1 {
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
}

/// Strict adapter-visible invocation. Expected values are intentionally absent to avoid leakage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ExecutableOracleInvocationWire")]
pub struct ExecutableOracleInvocationV1 {
    schema_version: u16,
    case_name: String,
    operator: ExecutableOracleOperatorV1,
    lhs: ExecutableOracleInputBufferV1,
    rhs: ExecutableOracleInputBufferV1,
    output: ExecutableOracleOutputBufferV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutableOracleInvocationWire {
    schema_version: u16,
    case_name: String,
    operator: ExecutableOracleOperatorV1,
    lhs: ExecutableOracleInputBufferV1,
    rhs: ExecutableOracleInputBufferV1,
    output: ExecutableOracleOutputBufferV1,
}

impl ExecutableOracleInvocationV1 {
    #[must_use]
    pub const fn output(&self) -> &ExecutableOracleOutputBufferV1 {
        &self.output
    }
}

impl TryFrom<ExecutableOracleInvocationWire> for ExecutableOracleInvocationV1 {
    type Error = ExecutableOracleError;

    fn try_from(wire: ExecutableOracleInvocationWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(ExecutableOracleError::UnsupportedSchemaVersion);
        }
        if wire.case_name != REQUIRED_CASE_NAME {
            return Err(ExecutableOracleError::InvalidCaseName);
        }
        let lhs_name = BufferName::new("lhs").map_err(codec)?;
        let rhs_name = BufferName::new("rhs").map_err(codec)?;
        let output_name = BufferName::new("output").map_err(codec)?;
        if wire.operator != ExecutableOracleOperatorV1::MatmulF32
            || wire.lhs.argument_index != ArgumentIndex::new(0)
            || wire.rhs.argument_index != ArgumentIndex::new(1)
            || wire.output.argument_index != ArgumentIndex::new(2)
            || wire.lhs.buffer != lhs_name
            || wire.rhs.buffer != rhs_name
            || wire.output.buffer != output_name
            || wire.lhs.data_type != DataType::F32
            || wire.rhs.data_type != DataType::F32
            || wire.output.data_type != DataType::F32
            || wire.lhs.shape.columns.get() != 0
            || wire.rhs.shape.rows.get() != 0
            || wire.output.shape.rows != wire.lhs.shape.rows
            || wire.output.shape.columns != wire.rhs.shape.columns
            || wire.lhs.element_count != count(wire.lhs.shape)?
            || wire.rhs.element_count != count(wire.rhs.shape)?
            || wire.output.element_count != count(wire.output.shape)?
            || wire.lhs.byte_length != byte_length(wire.lhs.shape)?
            || wire.rhs.byte_length != byte_length(wire.rhs.shape)?
            || wire.output.byte_length != byte_length(wire.output.shape)?
            || wire.lhs.path != argument_path(wire.lhs.argument_index)?
            || wire.rhs.path != argument_path(wire.rhs.argument_index)?
        {
            return Err(ExecutableOracleError::InconsistentInvocation);
        }
        Ok(Self {
            schema_version: wire.schema_version,
            case_name: wire.case_name,
            operator: wire.operator,
            lhs: wire.lhs,
            rhs: wire.rhs,
            output: wire.output,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ExecutableOracleOperatorV1 {
    MatmulF32,
}

/// Complete transient product ready for CAS archival and call-adapter preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssembledExecutableOracleCaseInput {
    invocation: ExecutableOracleInvocationV1,
    invocation_id: ContentId<ExecutableOracleInvocationArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    expected_output_bytes: Vec<u8>,
    expected_output_id: ContentId<CallAdapterOutputBytesArtifact>,
    comparison: ExecutableOracleComparisonV1,
}

impl AssembledExecutableOracleCaseInput {
    #[must_use]
    pub const fn invocation(&self) -> &ExecutableOracleInvocationV1 {
        &self.invocation
    }

    #[must_use]
    pub const fn invocation_id(&self) -> ContentId<ExecutableOracleInvocationArtifact> {
        self.invocation_id
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
    pub fn expected_output_bytes(&self) -> &[u8] {
        &self.expected_output_bytes
    }

    #[must_use]
    pub const fn expected_output_id(&self) -> ContentId<CallAdapterOutputBytesArtifact> {
        self.expected_output_id
    }

    #[must_use]
    pub const fn comparison(&self) -> ExecutableOracleComparisonV1 {
        self.comparison
    }
}

/// Proposed-reference/observed-output comparison facts; no admission verdict is stored.
///
/// This transient value is intentionally serialize-only: validating a persisted numeric comparison
/// requires loading the cited raw contents and rerunning [`compare_executable_oracle_output`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExecutableOracleOutputComparisonV1 {
    schema_version: u16,
    invocation: ContentId<ExecutableOracleInvocationArtifact>,
    argument_index: ArgumentIndex,
    buffer: BufferName,
    byte_length: CorpusBufferByteLength,
    comparison: ExecutableOracleComparisonV1,
    expected: ContentId<CallAdapterOutputBytesArtifact>,
    observed: ContentId<CallAdapterOutputBytesArtifact>,
    normalized_expected: ContentId<CallAdapterOutputBytesArtifact>,
    normalized_observed: ContentId<CallAdapterOutputBytesArtifact>,
}

impl ExecutableOracleOutputComparisonV1 {
    /// Recomputes exact equality from immutable content identities.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.normalized_expected == self.normalized_observed
    }
}

/// Canonical comparison bytes and identity ready for archival after candidate output exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedExecutableOracleOutputComparison {
    comparison: ExecutableOracleOutputComparisonV1,
    bytes: Vec<u8>,
    id: ContentId<ExecutableOracleOutputComparisonArtifact>,
}

impl PreparedExecutableOracleOutputComparison {
    #[must_use]
    pub const fn comparison(&self) -> &ExecutableOracleOutputComparisonV1 {
        &self.comparison
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<ExecutableOracleOutputComparisonArtifact> {
        self.id
    }

    /// Recomputes equality from the normalized identities in the trusted prepared body.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.comparison.matches()
    }
}

/// Failure to validate or materialize the executable Oracle slice.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ExecutableOracleError {
    #[error("executable Oracle schema version must be 1")]
    UnsupportedSchemaVersion,
    #[error("executable Oracle case name must be matmul-zero-k")]
    InvalidCaseName,
    #[error("zero-K matmul ABI must be lhs=0, rhs=1, output=2")]
    NonCanonicalAbi,
    #[error("executable Oracle matmul must have a zero shared K extent")]
    NotZeroK,
    #[error("matmul output shape does not follow [M,K] x [K,N] -> [M,N]")]
    ShapeMismatch,
    #[error("executable Oracle vector length contradicts its typed shape")]
    ValueCountMismatch,
    #[error("zero-K expected output must contain only numerical f32 zero values")]
    ExpectedNumericZeroMismatch,
    #[error("executable Oracle proposal does not match the fixed matmul-zero-k dogfood sample")]
    SampleContractMismatch,
    #[error("matmul-zero-k caller contract does not authorize signed-zero bit exactness")]
    SampleComparatorOverconstrained,
    #[error("executable Oracle size arithmetic overflow")]
    SizeOverflow,
    #[error("observed output byte length contradicts the Oracle output allocation")]
    ObservedOutputLengthMismatch,
    #[error("materialized executable Oracle invocation is inconsistent")]
    InconsistentInvocation,
    #[error("executable Oracle codec or path error: {message}")]
    Codec { message: String },
}

/// Revalidates and materializes one model-authored zero-K proposal.
///
/// Expected output bytes remain outside the adapter input bundle. This prevents the candidate
/// process from reading the answer it will later be compared against.
///
/// # Errors
///
/// Rejects invalid proposal semantics, overflowing sizes, paths, or canonical encoding failures.
pub fn assemble_zero_k_matmul_f32_oracle(
    case: &ZeroKMatmulF32OracleCaseV1,
) -> Result<AssembledExecutableOracleCaseInput, ExecutableOracleError> {
    case.validate_common()?;
    let lhs_bytes = encode_bits(&case.lhs_bits);
    let rhs_bytes = encode_bits(&case.rhs_bits);
    let expected_output_bytes = encode_bits(&case.expected_output_bits);
    let lhs = input_buffer(case.lhs_argument, "lhs", case.lhs_shape, &lhs_bytes)?;
    let rhs = input_buffer(case.rhs_argument, "rhs", case.rhs_shape, &rhs_bytes)?;
    let output = ExecutableOracleOutputBufferV1 {
        argument_index: case.output_argument,
        buffer: BufferName::new("output").map_err(codec)?,
        data_type: DataType::F32,
        shape: case.output_shape,
        element_count: count(case.output_shape)?,
        byte_length: byte_length(case.output_shape)?,
    };
    let invocation = ExecutableOracleInvocationV1 {
        schema_version: 1,
        case_name: case.case_name.clone(),
        operator: ExecutableOracleOperatorV1::MatmulF32,
        lhs,
        rhs,
        output,
    };
    let invocation_bytes = cairn_codec::to_vec(&invocation).map_err(codec)?;
    let invocation_id = ContentId::<ExecutableOracleInvocationArtifact>::derive(&invocation_bytes)
        .map_err(codec)?;
    let entries = vec![
        InputBundleEntry::Directory {
            path: sandbox_path(MATERIAL_ROOT)?,
        },
        InputBundleEntry::Directory {
            path: sandbox_path(ABI_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: argument_path(case.lhs_argument)?,
            mode: InputFileMode::Data,
            bytes: lhs_bytes,
        },
        InputBundleEntry::File {
            path: argument_path(case.rhs_argument)?,
            mode: InputFileMode::Data,
            bytes: rhs_bytes,
        },
        InputBundleEntry::File {
            path: sandbox_path(INVOCATION_PATH)?,
            mode: InputFileMode::Data,
            bytes: invocation_bytes,
        },
    ];
    let input_bundle = InputBundleV1::new(entries).map_err(codec)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(codec)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(codec)?;
    let expected_output_id =
        ContentId::<CallAdapterOutputBytesArtifact>::derive(&expected_output_bytes)
            .map_err(codec)?;
    Ok(AssembledExecutableOracleCaseInput {
        invocation,
        invocation_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        expected_output_bytes,
        expected_output_id,
        comparison: case.comparison,
    })
}

/// Compares actual candidate bytes with the separately held model-proposed exact output.
///
/// # Errors
///
/// Rejects an observed byte slice whose length contradicts the typed output allocation or whose
/// content identity cannot be derived.
pub fn compare_executable_oracle_output(
    case: &AssembledExecutableOracleCaseInput,
    observed: &[u8],
) -> Result<PreparedExecutableOracleOutputComparison, ExecutableOracleError> {
    let output = case.invocation.output();
    if u64::try_from(observed.len()).ok() != Some(output.byte_length.get()) {
        return Err(ExecutableOracleError::ObservedOutputLengthMismatch);
    }
    let normalized_expected =
        normalize_for_comparison(case.comparison, case.expected_output_bytes())?;
    let normalized_observed = normalize_for_comparison(case.comparison, observed)?;
    let comparison = ExecutableOracleOutputComparisonV1 {
        schema_version: 1,
        invocation: case.invocation_id,
        argument_index: output.argument_index,
        buffer: output.buffer.clone(),
        byte_length: output.byte_length,
        comparison: case.comparison,
        expected: case.expected_output_id,
        observed: ContentId::<CallAdapterOutputBytesArtifact>::derive(observed).map_err(codec)?,
        normalized_expected: ContentId::<CallAdapterOutputBytesArtifact>::derive(
            &normalized_expected,
        )
        .map_err(codec)?,
        normalized_observed: ContentId::<CallAdapterOutputBytesArtifact>::derive(
            &normalized_observed,
        )
        .map_err(codec)?,
    };
    let bytes = cairn_codec::to_vec(&comparison).map_err(codec)?;
    let id =
        ContentId::<ExecutableOracleOutputComparisonArtifact>::derive(&bytes).map_err(codec)?;
    Ok(PreparedExecutableOracleOutputComparison {
        comparison,
        bytes,
        id,
    })
}

fn normalize_for_comparison(
    comparison: ExecutableOracleComparisonV1,
    bytes: &[u8],
) -> Result<Vec<u8>, ExecutableOracleError> {
    match comparison {
        ExecutableOracleComparisonV1::ExactBits => Ok(bytes.to_vec()),
        ExecutableOracleComparisonV1::F32NumericExact => {
            let chunks = bytes.chunks_exact(4);
            if !chunks.remainder().is_empty() {
                return Err(ExecutableOracleError::ObservedOutputLengthMismatch);
            }
            Ok(chunks
                .flat_map(|chunk| {
                    let bits = u32::from_le_bytes(
                        <[u8; 4]>::try_from(chunk).expect("chunks_exact fixes f32 width"),
                    );
                    let normalized = if f32::from_bits(bits) == 0.0 { 0 } else { bits };
                    normalized.to_le_bytes()
                })
                .collect())
        }
    }
}

fn input_buffer(
    argument_index: ArgumentIndex,
    name: &str,
    shape: OracleMatrixShapeV1,
    bytes: &[u8],
) -> Result<ExecutableOracleInputBufferV1, ExecutableOracleError> {
    Ok(ExecutableOracleInputBufferV1 {
        argument_index,
        buffer: BufferName::new(name).map_err(codec)?,
        data_type: DataType::F32,
        shape,
        element_count: count(shape)?,
        byte_length: byte_length(shape)?,
        path: argument_path(argument_index)?,
        bytes: ContentId::<ExecutableOracleInputBytesArtifact>::derive(bytes).map_err(codec)?,
    })
}

fn shape(rows: u64, columns: u64) -> OracleMatrixShapeV1 {
    OracleMatrixShapeV1::new(ExtentValue::new(rows), ExtentValue::new(columns))
}

fn element_count(shape: OracleMatrixShapeV1) -> Result<usize, ExecutableOracleError> {
    let count = shape
        .rows
        .get()
        .checked_mul(shape.columns.get())
        .ok_or(ExecutableOracleError::SizeOverflow)?;
    usize::try_from(count).map_err(|_| ExecutableOracleError::SizeOverflow)
}

fn count(shape: OracleMatrixShapeV1) -> Result<CorpusElementCount, ExecutableOracleError> {
    u64::try_from(element_count(shape)?)
        .map(CorpusElementCount::new)
        .map_err(|_| ExecutableOracleError::SizeOverflow)
}

fn byte_length(
    shape: OracleMatrixShapeV1,
) -> Result<CorpusBufferByteLength, ExecutableOracleError> {
    let bytes = u64::try_from(element_count(shape)?)
        .map_err(|_| ExecutableOracleError::SizeOverflow)?
        .checked_mul(4)
        .ok_or(ExecutableOracleError::SizeOverflow)?;
    Ok(CorpusBufferByteLength::new(bytes))
}

fn require_bits(bits: &[OracleF32Bits], expected: usize) -> Result<(), ExecutableOracleError> {
    if bits.len() == expected {
        Ok(())
    } else {
        Err(ExecutableOracleError::ValueCountMismatch)
    }
}

fn encode_bits(bits: &[OracleF32Bits]) -> Vec<u8> {
    bits.iter()
        .flat_map(|bits| bits.get().to_le_bytes())
        .collect()
}

fn argument_path(index: ArgumentIndex) -> Result<SandboxPath, ExecutableOracleError> {
    sandbox_path(&format!("{ABI_DIRECTORY}/arg-{:05}.bin", index.get()))
}

fn sandbox_path(value: &str) -> Result<SandboxPath, ExecutableOracleError> {
    SandboxPath::new(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> ExecutableOracleError {
    ExecutableOracleError::Codec {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> ZeroKMatmulF32OracleCaseV1 {
        ZeroKMatmulF32OracleCaseV1 {
            schema_version: 1,
            case_name: REQUIRED_CASE_NAME.to_owned(),
            lhs_argument: ArgumentIndex::new(0),
            rhs_argument: ArgumentIndex::new(1),
            output_argument: ArgumentIndex::new(2),
            lhs_shape: shape(2, 0),
            rhs_shape: shape(0, 3),
            output_shape: shape(2, 3),
            lhs_bits: Vec::new(),
            rhs_bits: Vec::new(),
            expected_output_bits: vec![OracleF32Bits::new(0); 6],
            comparison: ExecutableOracleComparisonV1::F32NumericExact,
        }
    }

    #[test]
    fn materializes_typed_inputs_without_leaking_expected_bytes() {
        let assembled = assemble_zero_k_matmul_f32_oracle(&proposal()).expect("assemble");
        assert_eq!(assembled.expected_output_bytes(), &[0; 24]);
        let entries = assembled.input_bundle().entries();
        assert!(
            entries
                .iter()
                .any(|entry| entry.path().as_str() == INVOCATION_PATH)
        );
        assert!(!entries.iter().any(|entry| match entry {
            InputBundleEntry::File { bytes, .. } => bytes == assembled.expected_output_bytes(),
            InputBundleEntry::Directory { .. } => false,
        }));
        let exact = compare_executable_oracle_output(&assembled, &[0; 24]).expect("comparison");
        assert!(exact.matches());
        assert_eq!(
            ContentId::<ExecutableOracleOutputComparisonArtifact>::derive(exact.bytes())
                .expect("comparison identity"),
            exact.id()
        );
        let negative_zero = [0x8000_0000_u32; 6]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let signed_zero =
            compare_executable_oracle_output(&assembled, &negative_zero).expect("comparison");
        assert!(signed_zero.matches());
        let ones = [1.0_f32.to_bits(); 6]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let mismatch = compare_executable_oracle_output(&assembled, &ones).expect("comparison");
        assert!(!mismatch.matches());

        let mut bit_exact = proposal();
        bit_exact.comparison = ExecutableOracleComparisonV1::ExactBits;
        let bit_exact = assemble_zero_k_matmul_f32_oracle(&bit_exact).expect("bit exact");
        assert!(
            !compare_executable_oracle_output(&bit_exact, &negative_zero)
                .expect("bit comparison")
                .matches()
        );
    }

    #[test]
    fn rejects_semantically_wrong_model_proposals() {
        let mut overconstrained = proposal();
        overconstrained.comparison = ExecutableOracleComparisonV1::ExactBits;
        assert_eq!(
            overconstrained
                .validate_matmul_zero_k_sample()
                .expect_err("signed-zero overconstraint"),
            ExecutableOracleError::SampleComparatorOverconstrained
        );
        let mut wrong = proposal();
        wrong.expected_output_bits[5] = OracleF32Bits::new(1.0_f32.to_bits());
        assert_eq!(
            assemble_zero_k_matmul_f32_oracle(&wrong).expect_err("nonzero"),
            ExecutableOracleError::ExpectedNumericZeroMismatch
        );
        let mut wrong = proposal();
        wrong.output_shape = shape(2, 4);
        assert_eq!(
            assemble_zero_k_matmul_f32_oracle(&wrong).expect_err("shape"),
            ExecutableOracleError::ShapeMismatch
        );
    }
}
