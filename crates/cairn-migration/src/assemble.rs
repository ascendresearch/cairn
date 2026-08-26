//! ABI-ordered assembly of one trusted boundary case into canonical execution input material.

use std::collections::BTreeSet;

use cairn_execution::{
    InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode, SandboxPath,
};
use cairn_protocol::{ContentId, ContentType};
use cairn_verification::CallerDomainBodyArtifact;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    ArgumentIndex, BufferContractV1, BufferName, BufferRole, CaseExpectedOutcome, DataType,
    DimensionSpec, ExtentValue, InputValueCaseTarget, InputValueDisposition, IntegerValue,
    MaterializedCorpusBuffer, MaterializedCorpusBufferArtifact,
    MaterializedCorpusBufferBytesArtifact, MigrationDomainCaseArtifact, MigrationDomainCaseV1,
    MigrationDomainContractV1, ScalarParameterName, derive_mandatory_base_cases,
};
use crate::{CorpusBufferByteLength, CorpusBufferByteLimit, CorpusByteOrder, CorpusElementCount};

const MATERIAL_ROOT: &str = "cairn";
const ABI_DIRECTORY: &str = "cairn/abi";
const INVOCATION_PATH: &str = "cairn/invocation.json";

/// Failure to bind a trusted boundary case to complete ABI-ordered execution material.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BoundaryCaseAssemblyError {
    /// Only the current pre-release V1 schema is accepted.
    #[error("materialized boundary-case schema version must be 1")]
    UnsupportedSchemaVersion,
    /// The supplied case is not one of the exact cases rederived from the caller domain.
    #[error("boundary case is not a trusted derivation of the caller domain")]
    UntrustedBoundaryCase,
    /// An explicitly excluded boundary must not become execution authority.
    #[error("explicitly excluded boundary case is not executable")]
    ExcludedBoundaryCase,
    /// Input-capable domain buffers and supplied materialized values do not match exactly.
    #[error("materialized input buffers do not exactly cover the input ABI")]
    InputCoverageMismatch,
    /// A materialized value contradicts its domain buffer, shape, dtype, disposition, or bytes.
    #[error("materialized input buffer contradicts the caller domain")]
    InputBufferMismatch,
    /// A shape or scalar assignment required by the ABI is absent.
    #[error("boundary case does not carry complete ABI assignments")]
    IncompleteAssignments,
    /// Shape/product or dtype-width multiplication overflowed a byte boundary.
    #[error("ABI buffer byte length overflow")]
    ByteLengthOverflow,
    /// One ABI buffer exceeds the caller-supplied per-buffer limit.
    #[error("ABI buffer exceeds the caller-supplied per-buffer byte limit")]
    BufferLimitExceeded {
        /// Exact bytes required by the resolved shape and dtype.
        required: CorpusBufferByteLength,
        /// Caller-supplied maximum for each buffer.
        limit: CorpusBufferByteLimit,
    },
    /// A scalar assignment cannot be represented by its declared ABI dtype.
    #[error("scalar assignment cannot be encoded by its declared ABI dtype")]
    ScalarEncodingMismatch,
    /// Persisted manifest arguments are contradictory or non-canonical.
    #[error("materialized boundary-case manifest is inconsistent")]
    InconsistentManifest,
    /// The canonical input bundle does not contain exactly the files committed by the manifest.
    #[error("input bundle contradicts the materialized boundary-case manifest")]
    InconsistentInputBundle,
    /// Canonical encoding, identity, or execution-material construction failed.
    #[error("boundary-case assembly codec error: {message}")]
    Codec {
        /// Adapter-neutral diagnostic.
        message: String,
    },
}

/// Content identity domain for exact little-endian bytes of one scalar ABI argument.
pub enum MaterializedScalarArgumentBytesArtifact {}

impl ContentType for MaterializedScalarArgumentBytesArtifact {
    const DOMAIN: &'static str = "migration.materialized-scalar-argument-bytes.v1";
}

/// Content identity domain for one complete materialized boundary-case invocation manifest.
///
/// ```compile_fail
/// use cairn_execution::InputBundleArtifact;
/// use cairn_migration::MaterializedBoundaryCaseArtifact;
/// use cairn_protocol::ContentId;
///
/// fn require_manifest(_: Option<ContentId<MaterializedBoundaryCaseArtifact>>) {}
/// let bundle: Option<ContentId<InputBundleArtifact>> = None;
/// require_manifest(bundle);
/// ```
pub enum MaterializedBoundaryCaseArtifact {}

impl ContentType for MaterializedBoundaryCaseArtifact {
    const DOMAIN: &'static str = "migration.materialized-boundary-case.v1";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BoundaryCaseSchemaV1;

impl Serialize for BoundaryCaseSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for BoundaryCaseSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                BoundaryCaseAssemblyError::UnsupportedSchemaVersion,
            )),
        }
    }
}

/// One exact ABI argument in an assembled boundary-case invocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MaterializedAbiArgumentV1 {
    /// Read-only buffer initialized from a typed value recipe.
    InputBuffer {
        /// Exact ABI position.
        argument_index: ArgumentIndex,
        /// Exact domain buffer.
        buffer: BufferName,
        /// Element dtype.
        data_type: DataType,
        /// Fully resolved ordered shape.
        extents: Vec<ExtentValue>,
        /// Product of the resolved extents; rank zero has one element.
        element_count: CorpusElementCount,
        /// Exact raw byte length.
        byte_length: CorpusBufferByteLength,
        /// Canonical sandbox data path.
        path: SandboxPath,
        /// Typed materialization-manifest identity.
        materialization: ContentId<MaterializedCorpusBufferArtifact>,
        /// Exact raw-byte identity.
        bytes: ContentId<MaterializedCorpusBufferBytesArtifact>,
        /// Supported baseline value behavior.
        disposition: InputValueDisposition,
    },
    /// Write-only buffer allocated by the trusted call adapter.
    OutputBuffer {
        /// Exact ABI position.
        argument_index: ArgumentIndex,
        /// Exact domain buffer.
        buffer: BufferName,
        /// Element dtype.
        data_type: DataType,
        /// Fully resolved ordered shape.
        extents: Vec<ExtentValue>,
        /// Product of the resolved extents; rank zero has one element.
        element_count: CorpusElementCount,
        /// Exact allocation byte length.
        byte_length: CorpusBufferByteLength,
    },
    /// Read/write buffer initialized from a typed value recipe.
    InputOutputBuffer {
        /// Exact ABI position.
        argument_index: ArgumentIndex,
        /// Exact domain buffer.
        buffer: BufferName,
        /// Element dtype.
        data_type: DataType,
        /// Fully resolved ordered shape.
        extents: Vec<ExtentValue>,
        /// Product of the resolved extents; rank zero has one element.
        element_count: CorpusElementCount,
        /// Exact raw/allocation byte length.
        byte_length: CorpusBufferByteLength,
        /// Canonical sandbox data path.
        path: SandboxPath,
        /// Typed materialization-manifest identity.
        materialization: ContentId<MaterializedCorpusBufferArtifact>,
        /// Exact raw-byte identity.
        bytes: ContentId<MaterializedCorpusBufferBytesArtifact>,
        /// Supported baseline value behavior.
        disposition: InputValueDisposition,
    },
    /// Integer or boolean scalar encoded into an exact ABI-width file.
    Scalar {
        /// Exact ABI position.
        argument_index: ArgumentIndex,
        /// Exact scalar parameter.
        parameter: ScalarParameterName,
        /// Integer/bool ABI dtype.
        data_type: DataType,
        /// Typed logical scalar value.
        value: IntegerValue,
        /// Explicit scalar byte order.
        byte_order: CorpusByteOrder,
        /// Exact scalar byte length.
        byte_length: CorpusBufferByteLength,
        /// Canonical sandbox data path.
        path: SandboxPath,
        /// Exact scalar-byte identity.
        bytes: ContentId<MaterializedScalarArgumentBytesArtifact>,
    },
}

impl MaterializedAbiArgumentV1 {
    /// Returns the exact ABI position.
    #[must_use]
    pub const fn argument_index(&self) -> ArgumentIndex {
        match self {
            Self::InputBuffer { argument_index, .. }
            | Self::OutputBuffer { argument_index, .. }
            | Self::InputOutputBuffer { argument_index, .. }
            | Self::Scalar { argument_index, .. } => *argument_index,
        }
    }

    /// Returns the sandbox file path when the argument has input bytes.
    #[must_use]
    pub const fn path(&self) -> Option<&SandboxPath> {
        match self {
            Self::InputBuffer { path, .. }
            | Self::InputOutputBuffer { path, .. }
            | Self::Scalar { path, .. } => Some(path),
            Self::OutputBuffer { .. } => None,
        }
    }
}

/// Strict V1 invocation manifest for one trusted quantitative boundary case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MaterializedBoundaryCaseWire")]
pub struct MaterializedBoundaryCaseV1 {
    schema_version: BoundaryCaseSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    boundary_case: ContentId<MigrationDomainCaseArtifact>,
    expected_outcome: CaseExpectedOutcome,
    arguments: Vec<MaterializedAbiArgumentV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializedBoundaryCaseWire {
    schema_version: BoundaryCaseSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    boundary_case: ContentId<MigrationDomainCaseArtifact>,
    expected_outcome: CaseExpectedOutcome,
    arguments: Vec<MaterializedAbiArgumentV1>,
}

impl MaterializedBoundaryCaseV1 {
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        boundary_case: ContentId<MigrationDomainCaseArtifact>,
        expected_outcome: CaseExpectedOutcome,
        arguments: Vec<MaterializedAbiArgumentV1>,
    ) -> Result<Self, BoundaryCaseAssemblyError> {
        if arguments.is_empty()
            || arguments
                .windows(2)
                .any(|pair| pair[0].argument_index() >= pair[1].argument_index())
            || arguments
                .iter()
                .any(|argument| !argument_is_consistent(argument))
        {
            return Err(BoundaryCaseAssemblyError::InconsistentManifest);
        }
        let mut paths = BTreeSet::new();
        if arguments
            .iter()
            .filter_map(MaterializedAbiArgumentV1::path)
            .any(|path| !paths.insert(path.as_str()))
        {
            return Err(BoundaryCaseAssemblyError::InconsistentManifest);
        }
        Ok(Self {
            schema_version: BoundaryCaseSchemaV1,
            domain,
            boundary_case,
            expected_outcome,
            arguments,
        })
    }

    /// Returns the exact caller-domain identity.
    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    /// Returns the exact trusted boundary-case identity.
    #[must_use]
    pub const fn boundary_case(&self) -> ContentId<MigrationDomainCaseArtifact> {
        self.boundary_case
    }

    /// Returns the caller-declared boundary outcome.
    #[must_use]
    pub const fn expected_outcome(&self) -> &CaseExpectedOutcome {
        &self.expected_outcome
    }

    /// Returns all buffer and scalar arguments in strict ABI order.
    #[must_use]
    pub fn arguments(&self) -> &[MaterializedAbiArgumentV1] {
        &self.arguments
    }

    /// Recomputes domain/case identities and trusted derivation membership.
    ///
    /// # Errors
    ///
    /// Rejects a different domain, an underived case, or contradictory copied outcome metadata.
    pub fn validate_sources(
        &self,
        domain: &MigrationDomainContractV1,
        case: &MigrationDomainCaseV1,
    ) -> Result<(), BoundaryCaseAssemblyError> {
        let domain_id = canonical_id::<CallerDomainBodyArtifact, _>(domain)?;
        let case_id = canonical_id::<MigrationDomainCaseArtifact, _>(case)?;
        let derived = derive_mandatory_base_cases(domain).map_err(codec_error)?;
        if domain_id != self.domain
            || case_id != self.boundary_case
            || case.expected_outcome() != &self.expected_outcome
            || !derived.cases().contains(case)
            || !self.arguments_match_sources(domain, case)
        {
            return Err(BoundaryCaseAssemblyError::UntrustedBoundaryCase);
        }
        Ok(())
    }

    fn arguments_match_sources(
        &self,
        domain: &MigrationDomainContractV1,
        case: &MigrationDomainCaseV1,
    ) -> bool {
        if self.arguments.len()
            != domain
                .buffers()
                .len()
                .saturating_add(domain.scalar_parameters().len())
        {
            return false;
        }
        let buffers_match = domain.buffers().iter().all(|buffer| {
            let Ok(extents) = resolve_extents(buffer.shape(), case) else {
                return false;
            };
            let Some(argument) = self
                .arguments
                .iter()
                .find(|argument| argument.argument_index() == buffer.argument_index())
            else {
                return false;
            };
            match (buffer.role(), argument) {
                (
                    BufferRole::Input,
                    MaterializedAbiArgumentV1::InputBuffer {
                        buffer: name,
                        data_type,
                        extents: observed,
                        ..
                    },
                )
                | (
                    BufferRole::InputOutput,
                    MaterializedAbiArgumentV1::InputOutputBuffer {
                        buffer: name,
                        data_type,
                        extents: observed,
                        ..
                    },
                )
                | (
                    BufferRole::Output,
                    MaterializedAbiArgumentV1::OutputBuffer {
                        buffer: name,
                        data_type,
                        extents: observed,
                        ..
                    },
                ) => {
                    name == buffer.name()
                        && *data_type == buffer.data_type()
                        && observed == &extents
                }
                _ => false,
            }
        });
        buffers_match
            && domain.scalar_parameters().iter().all(|parameter| {
                let Some(assignment) = case
                    .scalar_assignments()
                    .iter()
                    .find(|assignment| assignment.parameter() == parameter.name())
                else {
                    return false;
                };
                matches!(
                    self.arguments.iter().find(|argument| {
                        argument.argument_index() == parameter.argument_index()
                    }),
                    Some(MaterializedAbiArgumentV1::Scalar {
                        parameter: name,
                        data_type,
                        value,
                        ..
                    }) if name == parameter.name()
                        && *data_type == parameter.data_type()
                        && *value == assignment.value()
                )
            })
    }

    /// Verifies that a canonical input bundle contains exactly this manifest and its input files.
    ///
    /// # Errors
    ///
    /// Rejects missing, extra, executable, length-mismatched, or identity-mismatched files.
    pub fn validate_input_bundle(
        &self,
        bundle: &InputBundleV1,
    ) -> Result<(), BoundaryCaseAssemblyError> {
        let manifest_bytes = cairn_codec::to_vec(self).map_err(codec_error)?;
        let expected_file_count = 1 + self
            .arguments
            .iter()
            .filter_map(Self::argument_path)
            .count();
        if bundle.entries().len() != expected_file_count + 2 {
            return Err(BoundaryCaseAssemblyError::InconsistentInputBundle);
        }
        let mut saw_root = false;
        let mut saw_abi = false;
        let mut saw_manifest = false;
        let mut seen_argument_paths = BTreeSet::new();
        for entry in bundle.entries() {
            match entry {
                InputBundleEntry::Directory { path } if path.as_str() == MATERIAL_ROOT => {
                    saw_root = true;
                }
                InputBundleEntry::Directory { path } if path.as_str() == ABI_DIRECTORY => {
                    saw_abi = true;
                }
                InputBundleEntry::File { path, mode, bytes }
                    if path.as_str() == INVOCATION_PATH
                        && *mode == InputFileMode::Data
                        && bytes == &manifest_bytes =>
                {
                    saw_manifest = true;
                }
                InputBundleEntry::File { path, mode, bytes } if *mode == InputFileMode::Data => {
                    let Some(argument) = self
                        .arguments
                        .iter()
                        .find(|argument| argument.path() == Some(path))
                    else {
                        return Err(BoundaryCaseAssemblyError::InconsistentInputBundle);
                    };
                    if !seen_argument_paths.insert(path.as_str())
                        || !Self::bytes_match_argument(argument, bytes)?
                    {
                        return Err(BoundaryCaseAssemblyError::InconsistentInputBundle);
                    }
                }
                _ => return Err(BoundaryCaseAssemblyError::InconsistentInputBundle),
            }
        }
        if !saw_root
            || !saw_abi
            || !saw_manifest
            || seen_argument_paths.len() + 1 != expected_file_count
        {
            return Err(BoundaryCaseAssemblyError::InconsistentInputBundle);
        }
        Ok(())
    }

    fn argument_path(argument: &MaterializedAbiArgumentV1) -> Option<&SandboxPath> {
        argument.path()
    }

    fn bytes_match_argument(
        argument: &MaterializedAbiArgumentV1,
        bytes: &[u8],
    ) -> Result<bool, BoundaryCaseAssemblyError> {
        let length = u64::try_from(bytes.len()).ok();
        match argument {
            MaterializedAbiArgumentV1::InputBuffer {
                byte_length,
                bytes: expected,
                ..
            }
            | MaterializedAbiArgumentV1::InputOutputBuffer {
                byte_length,
                bytes: expected,
                ..
            } => Ok(length == Some(byte_length.get())
                && ContentId::<MaterializedCorpusBufferBytesArtifact>::derive(bytes)
                    .map_err(codec_error)?
                    == *expected),
            MaterializedAbiArgumentV1::Scalar {
                byte_length,
                bytes: expected,
                ..
            } => Ok(length == Some(byte_length.get())
                && ContentId::<MaterializedScalarArgumentBytesArtifact>::derive(bytes)
                    .map_err(codec_error)?
                    == *expected),
            MaterializedAbiArgumentV1::OutputBuffer { .. } => Ok(false),
        }
    }
}

impl TryFrom<MaterializedBoundaryCaseWire> for MaterializedBoundaryCaseV1 {
    type Error = BoundaryCaseAssemblyError;

    fn try_from(wire: MaterializedBoundaryCaseWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.domain,
            wire.boundary_case,
            wire.expected_outcome,
            wire.arguments,
        )
    }
}

/// Complete transient product ready to archive in the execution input-bundle content domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssembledBoundaryCaseInput {
    manifest: MaterializedBoundaryCaseV1,
    manifest_id: ContentId<MaterializedBoundaryCaseArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
}

impl AssembledBoundaryCaseInput {
    /// Returns the immutable invocation manifest embedded in the bundle.
    #[must_use]
    pub const fn manifest(&self) -> &MaterializedBoundaryCaseV1 {
        &self.manifest
    }

    /// Returns the typed identity of the invocation manifest embedded in the bundle.
    #[must_use]
    pub const fn manifest_id(&self) -> ContentId<MaterializedBoundaryCaseArtifact> {
        self.manifest_id
    }

    /// Returns the canonical execution input bundle.
    #[must_use]
    pub const fn input_bundle(&self) -> &InputBundleV1 {
        &self.input_bundle
    }

    /// Returns canonical bytes ready for CAS archival.
    #[must_use]
    pub fn input_bundle_bytes(&self) -> &[u8] {
        &self.input_bundle_bytes
    }

    /// Returns the exact execution input-bundle identity.
    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }
}

/// Assembles one exact trusted boundary case and complete supported input-buffer baselines.
///
/// The supplied supported materialized buffers must cover every input/input-output ABI buffer
/// exactly once. Explicitly-invalid dtype recipes are reserved for a separate single-variable
/// dtype-case composition so they cannot silently contradict this boundary case's expected outcome.
/// Output-only buffers carry allocation shapes and lengths but no fabricated input bytes. Unsafe
/// pointer, misalignment, short-capacity, and aliasing surfaces are intentionally outside this
/// ordinary-file bundle.
///
/// # Errors
///
/// Rejects underived/excluded boundary cases, incomplete or contradictory assignments and inputs,
/// checked size/limit failures, and any canonical material or identity failure.
pub fn assemble_boundary_case_input(
    domain: &MigrationDomainContractV1,
    case: &MigrationDomainCaseV1,
    materialized_inputs: &[MaterializedCorpusBuffer],
    per_buffer_limit: CorpusBufferByteLimit,
) -> Result<AssembledBoundaryCaseInput, BoundaryCaseAssemblyError> {
    let derived = derive_mandatory_base_cases(domain).map_err(codec_error)?;
    if !derived.cases().contains(case) {
        return Err(BoundaryCaseAssemblyError::UntrustedBoundaryCase);
    }
    if matches!(
        case.expected_outcome(),
        CaseExpectedOutcome::Invalid {
            behavior: crate::InvalidInputBehavior::ExplicitlyExcluded
        }
    ) {
        return Err(BoundaryCaseAssemblyError::ExcludedBoundaryCase);
    }

    let domain_id = canonical_id::<CallerDomainBodyArtifact, _>(domain)?;
    let case_id = canonical_id::<MigrationDomainCaseArtifact, _>(case)?;
    let (mut arguments, mut argument_files) =
        assemble_buffer_arguments(domain, case, materialized_inputs, per_buffer_limit)?;
    let (scalar_arguments, scalar_files) = assemble_scalar_arguments(domain, case)?;
    arguments.extend(scalar_arguments);
    argument_files.extend(scalar_files);
    arguments.sort_by_key(MaterializedAbiArgumentV1::argument_index);

    let manifest = MaterializedBoundaryCaseV1::new(
        domain_id,
        case_id,
        case.expected_outcome().clone(),
        arguments,
    )?;
    let manifest_bytes = cairn_codec::to_vec(&manifest).map_err(codec_error)?;
    let manifest_id = ContentId::<MaterializedBoundaryCaseArtifact>::derive(&manifest_bytes)
        .map_err(codec_error)?;
    let mut files = vec![
        InputBundleEntry::Directory {
            path: sandbox_path(MATERIAL_ROOT)?,
        },
        InputBundleEntry::Directory {
            path: sandbox_path(ABI_DIRECTORY)?,
        },
    ];
    files.append(&mut argument_files);
    files.push(InputBundleEntry::File {
        path: sandbox_path(INVOCATION_PATH)?,
        mode: InputFileMode::Data,
        bytes: manifest_bytes,
    });
    let input_bundle = InputBundleV1::new(files).map_err(codec_error)?;
    manifest.validate_input_bundle(&input_bundle)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(codec_error)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(codec_error)?;
    Ok(AssembledBoundaryCaseInput {
        manifest,
        manifest_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
    })
}

fn assemble_buffer_arguments(
    domain: &MigrationDomainContractV1,
    case: &MigrationDomainCaseV1,
    materialized_inputs: &[MaterializedCorpusBuffer],
    limit: CorpusBufferByteLimit,
) -> Result<(Vec<MaterializedAbiArgumentV1>, Vec<InputBundleEntry>), BoundaryCaseAssemblyError> {
    let mut arguments = Vec::with_capacity(domain.buffers().len());
    let mut files = Vec::new();
    let mut consumed_inputs = BTreeSet::new();
    for buffer in domain.buffers() {
        let (argument, file) = assemble_buffer_argument(
            buffer,
            case,
            materialized_inputs,
            limit,
            &mut consumed_inputs,
        )?;
        arguments.push(argument);
        files.extend(file);
    }
    if consumed_inputs.len() != materialized_inputs.len() {
        return Err(BoundaryCaseAssemblyError::InputCoverageMismatch);
    }
    Ok((arguments, files))
}

fn assemble_buffer_argument<'a>(
    buffer: &BufferContractV1,
    case: &MigrationDomainCaseV1,
    materialized_inputs: &'a [MaterializedCorpusBuffer],
    limit: CorpusBufferByteLimit,
    consumed_inputs: &mut BTreeSet<&'a BufferName>,
) -> Result<(MaterializedAbiArgumentV1, Option<InputBundleEntry>), BoundaryCaseAssemblyError> {
    let extents = resolve_extents(buffer.shape(), case)?;
    let element_count = element_count(&extents)?;
    let byte_length = buffer_byte_length(element_count, buffer.data_type(), limit)?;
    if buffer.role() == BufferRole::Output {
        return Ok((
            MaterializedAbiArgumentV1::OutputBuffer {
                argument_index: buffer.argument_index(),
                buffer: buffer.name().clone(),
                data_type: buffer.data_type(),
                extents,
                element_count,
                byte_length,
            },
            None,
        ));
    }
    let materialized = materialized_inputs
        .iter()
        .find(|candidate| candidate.manifest().target().buffer() == buffer.name())
        .ok_or(BoundaryCaseAssemblyError::InputCoverageMismatch)?;
    if !consumed_inputs.insert(materialized.manifest().target().buffer()) {
        return Err(BoundaryCaseAssemblyError::InputCoverageMismatch);
    }
    validate_materialized_input(
        materialized,
        buffer.name(),
        buffer.data_type(),
        element_count,
        byte_length,
    )?;
    let path = argument_path(buffer.argument_index())?;
    let file = InputBundleEntry::File {
        path: path.clone(),
        mode: InputFileMode::Data,
        bytes: materialized.bytes().to_vec(),
    };
    let materialization =
        canonical_id::<MaterializedCorpusBufferArtifact, _>(materialized.manifest())?;
    let argument = match buffer.role() {
        BufferRole::Input => MaterializedAbiArgumentV1::InputBuffer {
            argument_index: buffer.argument_index(),
            buffer: buffer.name().clone(),
            data_type: buffer.data_type(),
            extents,
            element_count,
            byte_length,
            path,
            materialization,
            bytes: materialized.manifest().bytes(),
            disposition: materialized.manifest().disposition().clone(),
        },
        BufferRole::InputOutput => MaterializedAbiArgumentV1::InputOutputBuffer {
            argument_index: buffer.argument_index(),
            buffer: buffer.name().clone(),
            data_type: buffer.data_type(),
            extents,
            element_count,
            byte_length,
            path,
            materialization,
            bytes: materialized.manifest().bytes(),
            disposition: materialized.manifest().disposition().clone(),
        },
        BufferRole::Output => return Err(BoundaryCaseAssemblyError::InconsistentManifest),
    };
    Ok((argument, Some(file)))
}

fn assemble_scalar_arguments(
    domain: &MigrationDomainContractV1,
    case: &MigrationDomainCaseV1,
) -> Result<(Vec<MaterializedAbiArgumentV1>, Vec<InputBundleEntry>), BoundaryCaseAssemblyError> {
    let mut arguments = Vec::with_capacity(domain.scalar_parameters().len());
    let mut files = Vec::with_capacity(domain.scalar_parameters().len());
    for parameter in domain.scalar_parameters() {
        let assignment = case
            .scalar_assignments()
            .iter()
            .find(|assignment| assignment.parameter() == parameter.name())
            .ok_or(BoundaryCaseAssemblyError::IncompleteAssignments)?;
        let bytes = encode_scalar(parameter.data_type(), assignment.value())?;
        let byte_length = CorpusBufferByteLength::new(
            u64::try_from(bytes.len())
                .map_err(|_| BoundaryCaseAssemblyError::ByteLengthOverflow)?,
        );
        let path = argument_path(parameter.argument_index())?;
        let bytes_id = ContentId::<MaterializedScalarArgumentBytesArtifact>::derive(&bytes)
            .map_err(codec_error)?;
        files.push(InputBundleEntry::File {
            path: path.clone(),
            mode: InputFileMode::Data,
            bytes,
        });
        arguments.push(MaterializedAbiArgumentV1::Scalar {
            argument_index: parameter.argument_index(),
            parameter: parameter.name().clone(),
            data_type: parameter.data_type(),
            value: assignment.value(),
            byte_order: CorpusByteOrder::LittleEndian,
            byte_length,
            path,
            bytes: bytes_id,
        });
    }
    Ok((arguments, files))
}

fn resolve_extents(
    shape: &[DimensionSpec],
    case: &MigrationDomainCaseV1,
) -> Result<Vec<ExtentValue>, BoundaryCaseAssemblyError> {
    shape
        .iter()
        .map(|dimension| match dimension {
            DimensionSpec::Constant { extent } => Ok(*extent),
            DimensionSpec::Symbol { symbol } => case
                .shape_assignments()
                .iter()
                .find(|assignment| assignment.symbol() == symbol)
                .map(crate::ShapeAssignment::value)
                .ok_or(BoundaryCaseAssemblyError::IncompleteAssignments),
        })
        .collect()
}

fn element_count(extents: &[ExtentValue]) -> Result<CorpusElementCount, BoundaryCaseAssemblyError> {
    extents
        .iter()
        .try_fold(1_u64, |product, extent| product.checked_mul(extent.get()))
        .map(CorpusElementCount::new)
        .ok_or(BoundaryCaseAssemblyError::ByteLengthOverflow)
}

fn buffer_byte_length(
    element_count: CorpusElementCount,
    data_type: DataType,
    limit: CorpusBufferByteLimit,
) -> Result<CorpusBufferByteLength, BoundaryCaseAssemblyError> {
    let required = element_count
        .get()
        .checked_mul(data_type.byte_width().get())
        .ok_or(BoundaryCaseAssemblyError::ByteLengthOverflow)?;
    let required = CorpusBufferByteLength::new(required);
    if required.get() > limit.get() {
        return Err(BoundaryCaseAssemblyError::BufferLimitExceeded { required, limit });
    }
    Ok(required)
}

fn validate_materialized_input(
    materialized: &MaterializedCorpusBuffer,
    buffer: &BufferName,
    data_type: DataType,
    element_count: CorpusElementCount,
    byte_length: CorpusBufferByteLength,
) -> Result<(), BoundaryCaseAssemblyError> {
    let manifest = materialized.manifest();
    if manifest.target().buffer() != buffer
        || target_data_type(manifest.target()) != data_type
        || manifest.element_count() != element_count
        || manifest.byte_length() != byte_length
        || manifest.disposition() != &InputValueDisposition::Supported
        || manifest.validate_bytes(materialized.bytes()).is_err()
    {
        return Err(BoundaryCaseAssemblyError::InputBufferMismatch);
    }
    Ok(())
}

const fn target_data_type(target: &InputValueCaseTarget) -> DataType {
    match target {
        InputValueCaseTarget::Floating { data_type, .. } => match data_type {
            crate::FloatingDataType::F16 => DataType::F16,
            crate::FloatingDataType::F32 => DataType::F32,
            crate::FloatingDataType::F64 => DataType::F64,
        },
        InputValueCaseTarget::SignedInteger { data_type, .. } => match data_type {
            crate::SignedIntegerDataType::I8 => DataType::I8,
            crate::SignedIntegerDataType::I16 => DataType::I16,
            crate::SignedIntegerDataType::I32 => DataType::I32,
            crate::SignedIntegerDataType::I64 => DataType::I64,
        },
        InputValueCaseTarget::UnsignedInteger { data_type, .. } => match data_type {
            crate::UnsignedIntegerDataType::U8 => DataType::U8,
            crate::UnsignedIntegerDataType::U16 => DataType::U16,
            crate::UnsignedIntegerDataType::U32 => DataType::U32,
            crate::UnsignedIntegerDataType::U64 => DataType::U64,
        },
        InputValueCaseTarget::Boolean { .. } => DataType::Bool,
    }
}

fn encode_scalar(
    data_type: DataType,
    value: IntegerValue,
) -> Result<Vec<u8>, BoundaryCaseAssemblyError> {
    let value = value.get();
    macro_rules! integer_bytes {
        ($ty:ty) => {
            <$ty>::try_from(value)
                .map(|encoded| encoded.to_le_bytes().to_vec())
                .map_err(|_| BoundaryCaseAssemblyError::ScalarEncodingMismatch)
        };
    }
    match data_type {
        DataType::I8 => integer_bytes!(i8),
        DataType::I16 => integer_bytes!(i16),
        DataType::I32 => integer_bytes!(i32),
        DataType::I64 => Ok(value.to_le_bytes().to_vec()),
        DataType::U8 => integer_bytes!(u8),
        DataType::U16 => integer_bytes!(u16),
        DataType::U32 => integer_bytes!(u32),
        DataType::U64 => integer_bytes!(u64),
        DataType::Bool if matches!(value, 0 | 1) => integer_bytes!(u8),
        DataType::Bool | DataType::F16 | DataType::F32 | DataType::F64 => {
            Err(BoundaryCaseAssemblyError::ScalarEncodingMismatch)
        }
    }
}

fn argument_is_consistent(argument: &MaterializedAbiArgumentV1) -> bool {
    match argument {
        MaterializedAbiArgumentV1::InputBuffer {
            argument_index,
            data_type,
            extents,
            element_count,
            byte_length,
            path,
            disposition,
            ..
        }
        | MaterializedAbiArgumentV1::InputOutputBuffer {
            argument_index,
            data_type,
            extents,
            element_count,
            byte_length,
            path,
            disposition,
            ..
        } => {
            disposition == &InputValueDisposition::Supported
                && path_matches(*argument_index, path)
                && shape_lengths_match(*data_type, extents, *element_count, *byte_length)
        }
        MaterializedAbiArgumentV1::OutputBuffer {
            data_type,
            extents,
            element_count,
            byte_length,
            ..
        } => shape_lengths_match(*data_type, extents, *element_count, *byte_length),
        MaterializedAbiArgumentV1::Scalar {
            argument_index,
            data_type,
            value,
            byte_order,
            byte_length,
            path,
            ..
        } => {
            *byte_order == CorpusByteOrder::LittleEndian
                && path_matches(*argument_index, path)
                && data_type.supports_integer_domain()
                && byte_length.get() == data_type.byte_width().get()
                && encode_scalar(*data_type, *value).is_ok()
        }
    }
}

fn shape_lengths_match(
    data_type: DataType,
    extents: &[ExtentValue],
    element_count: CorpusElementCount,
    byte_length: CorpusBufferByteLength,
) -> bool {
    extents
        .iter()
        .try_fold(1_u64, |product, extent| product.checked_mul(extent.get()))
        == Some(element_count.get())
        && element_count
            .get()
            .checked_mul(data_type.byte_width().get())
            == Some(byte_length.get())
}

fn argument_path(index: ArgumentIndex) -> Result<SandboxPath, BoundaryCaseAssemblyError> {
    sandbox_path(&format!("{ABI_DIRECTORY}/arg-{:05}.bin", index.get()))
}

fn path_matches(index: ArgumentIndex, path: &SandboxPath) -> bool {
    path.as_str() == format!("{ABI_DIRECTORY}/arg-{:05}.bin", index.get())
}

fn sandbox_path(value: &str) -> Result<SandboxPath, BoundaryCaseAssemblyError> {
    SandboxPath::new(value).map_err(codec_error)
}

fn canonical_id<T: ContentType, V: Serialize>(
    value: &V,
) -> Result<ContentId<T>, BoundaryCaseAssemblyError> {
    let bytes = cairn_codec::to_vec(value).map_err(codec_error)?;
    ContentId::<T>::derive(&bytes).map_err(codec_error)
}

fn codec_error(error: impl std::fmt::Display) -> BoundaryCaseAssemblyError {
    BoundaryCaseAssemblyError::Codec {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use cairn_execution::{InputBundleArtifact, InputBundleEntry, InputBundleV1};
    use cairn_protocol::{ContentId, ContentType};
    use cairn_verification::CallerDomainBodyArtifact;
    use serde_json::json;

    use super::{
        BoundaryCaseAssemblyError, MaterializedAbiArgumentV1, MaterializedBoundaryCaseV1,
        assemble_boundary_case_input,
    };
    use crate::{
        ArgumentIndex, BufferAccessV1, BufferAliasingContractInput, BufferAliasingContractV1,
        BufferContractInput, BufferContractV1, BufferMemoryContractInput, BufferMemoryContractV1,
        BufferName, BufferPairV1, CaseTarget, CorpusBufferByteLimit, CorpusElementCount, DataType,
        DimensionSpec, EntryPointName, ExtentValue, FloatingDataType, FloatingInputPattern,
        FloatingInputValueDomainInput, FloatingInputValueDomainV1, InclusiveExtentRange,
        InclusiveIntegerRange, InputValueCaseTarget, InputValueDisposition, InputValueDomainV1,
        IntegerValue, InvalidInputBehavior, MandatoryInputValueCaseV1, MemoryConditionDisposition,
        MigrationDomainContractInput, MigrationDomainContractV1, PointerAlignmentContractV1,
        RequestedSemanticsArtifact, ScalarParameterContractInput, ScalarParameterContractV1,
        ScalarParameterName, ScalarParameterRole, SemanticClaimKind, ShapeSymbolContractInput,
        ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource, derive_mandatory_base_cases,
        materialize_input_value_case,
    };

    fn id<T: ContentType>(bytes: &[u8]) -> ContentId<T> {
        ContentId::<T>::derive(bytes).expect("content identity")
    }

    fn memory_contract() -> BufferMemoryContractV1 {
        BufferMemoryContractV1::new(BufferMemoryContractInput {
            null_non_empty: MemoryConditionDisposition::Unknown,
            alignment: PointerAlignmentContractV1::ByteAligned,
            insufficient_capacity_non_empty: MemoryConditionDisposition::Unknown,
        })
    }

    fn float_domain() -> InputValueDomainV1 {
        InputValueDomainV1::Floating {
            special_values: FloatingInputValueDomainV1::new(FloatingInputValueDomainInput {
                negative_zero: InputValueDisposition::Supported,
                subnormal: InputValueDisposition::Supported,
                infinity: InputValueDisposition::Supported,
                nan: InputValueDisposition::Supported,
            }),
        }
    }

    fn domain(invalid_behavior: InvalidInputBehavior) -> MigrationDomainContractV1 {
        let input = BufferName::new("input").expect("input");
        let output = BufferName::new("output").expect("output");
        let symbol = ShapeSymbolName::new("n").expect("symbol");
        let parameter = ScalarParameterName::new("n_arg").expect("parameter");
        let extent_range =
            InclusiveExtentRange::new(ExtentValue::new(1), ExtentValue::new(3)).expect("range");
        let integer_range =
            InclusiveIntegerRange::new(IntegerValue::new(1), IntegerValue::new(3)).expect("range");
        MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("scale").expect("entry point"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: input.clone(),
                    access: BufferAccessV1::Input {
                        value_domain: float_domain(),
                    },
                    data_type: DataType::F32,
                    shape: vec![DimensionSpec::Symbol {
                        symbol: symbol.clone(),
                    }],
                    memory: memory_contract(),
                })
                .expect("input contract"),
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(2),
                    name: output.clone(),
                    access: BufferAccessV1::Output,
                    data_type: DataType::F32,
                    shape: vec![DimensionSpec::Symbol {
                        symbol: symbol.clone(),
                    }],
                    memory: memory_contract(),
                })
                .expect("output contract"),
            ],
            scalar_parameters: vec![
                ScalarParameterContractV1::new(ScalarParameterContractInput {
                    argument_index: ArgumentIndex::new(1),
                    name: parameter.clone(),
                    role: ScalarParameterRole::ShapeExtent,
                    data_type: DataType::I32,
                    valid_range: integer_range,
                    invalid_behavior: invalid_behavior.clone(),
                })
                .expect("scalar contract"),
            ],
            shape_symbols: vec![
                ShapeSymbolContractV1::new(ShapeSymbolContractInput {
                    name: symbol,
                    valid_range: extent_range,
                    source: ShapeSymbolSource::ScalarParameter { parameter },
                    boundary_moduli: Vec::new(),
                    invalid_behavior,
                })
                .expect("shape contract"),
            ],
            buffer_aliasing: vec![BufferAliasingContractV1::new(BufferAliasingContractInput {
                pair: BufferPairV1::new(input, output).expect("buffer pair"),
                exact_alias: MemoryConditionDisposition::Unknown,
                partial_overlap: MemoryConditionDisposition::Invalid {
                    behavior: InvalidInputBehavior::RejectBeforeExecution,
                },
            })],
            requested_semantics: id::<RequestedSemanticsArtifact>(b"scale-semantics"),
            semantic_claim: SemanticClaimKind::Numerical,
            exclusions: Vec::new(),
        })
        .expect("domain")
    }

    fn boundary_case(
        domain: &MigrationDomainContractV1,
        extent: u64,
    ) -> crate::MigrationDomainCaseV1 {
        derive_mandatory_base_cases(domain)
            .expect("derive")
            .cases()
            .iter()
            .find(|case| {
                matches!(
                    case.target(),
                    CaseTarget::ShapeSymbol { value, .. } if value.get() == extent
                )
            })
            .cloned()
            .expect("boundary case")
    }

    fn input_case() -> MandatoryInputValueCaseV1 {
        MandatoryInputValueCaseV1::new(
            InputValueCaseTarget::Floating {
                buffer: BufferName::new("input").expect("input"),
                data_type: FloatingDataType::F32,
                pattern: FloatingInputPattern::PositiveOne,
            },
            InputValueDisposition::Supported,
        )
    }

    fn assembled(
        domain: &MigrationDomainContractV1,
        extent: u64,
    ) -> super::AssembledBoundaryCaseInput {
        let case = boundary_case(domain, extent);
        let input = materialize_input_value_case(
            &input_case(),
            CorpusElementCount::new(extent),
            CorpusBufferByteLimit::new(128).expect("limit"),
        )
        .expect("materialize input");
        assemble_boundary_case_input(
            domain,
            &case,
            &[input],
            CorpusBufferByteLimit::new(128).expect("limit"),
        )
        .expect("assemble")
    }

    #[test]
    fn assembly_preserves_interleaved_abi_order_and_exact_bytes() {
        let domain = domain(InvalidInputBehavior::RejectBeforeExecution);
        let case = boundary_case(&domain, 2);
        let assembled = assembled(&domain, 2);
        assert_eq!(
            assembled
                .manifest()
                .arguments()
                .iter()
                .map(MaterializedAbiArgumentV1::argument_index)
                .collect::<Vec<_>>(),
            vec![
                ArgumentIndex::new(0),
                ArgumentIndex::new(1),
                ArgumentIndex::new(2)
            ]
        );
        assert!(matches!(
            &assembled.manifest().arguments()[2],
            MaterializedAbiArgumentV1::OutputBuffer {
                element_count,
                byte_length,
                ..
            } if element_count.get() == 2 && byte_length.get() == 8
        ));

        let file = |path: &str| {
            assembled
                .input_bundle()
                .entries()
                .iter()
                .find_map(|entry| match entry {
                    InputBundleEntry::File {
                        path: candidate,
                        bytes,
                        ..
                    } if candidate.as_str() == path => Some(bytes.as_slice()),
                    _ => None,
                })
                .expect("file")
        };
        assert_eq!(
            file("cairn/abi/arg-00000.bin"),
            &[0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x80, 0x3f]
        );
        assert_eq!(file("cairn/abi/arg-00001.bin"), &[2, 0, 0, 0]);
        assert!(
            assembled
                .input_bundle()
                .entries()
                .iter()
                .all(|entry| entry.path().as_str() != "cairn/abi/arg-00002.bin")
        );
        assembled
            .manifest()
            .validate_sources(&domain, &case)
            .expect("source binding");
        assembled
            .manifest()
            .validate_input_bundle(assembled.input_bundle())
            .expect("bundle binding");
        assert_eq!(
            assembled.input_bundle_id(),
            ContentId::<InputBundleArtifact>::derive(assembled.input_bundle_bytes())
                .expect("bundle identity")
        );
        assert_eq!(
            assembled.manifest_id(),
            ContentId::<super::MaterializedBoundaryCaseArtifact>::derive(
                &cairn_codec::to_vec(assembled.manifest()).expect("manifest bytes")
            )
            .expect("manifest identity")
        );
    }

    #[test]
    fn assembly_is_deterministic_and_bundle_tampering_fails() {
        let domain = domain(InvalidInputBehavior::RejectBeforeExecution);
        let first = assembled(&domain, 3);
        let second = assembled(&domain, 3);
        assert_eq!(first, second);

        let mut entries = first.input_bundle().entries().to_vec();
        let InputBundleEntry::File { bytes, .. } = entries
            .iter_mut()
            .find(|entry| entry.path().as_str() == "cairn/abi/arg-00000.bin")
            .expect("input file")
        else {
            panic!("expected input file");
        };
        bytes[0] ^= 1;
        let tampered = InputBundleV1::new(entries).expect("structurally valid bundle");
        assert_eq!(
            first.manifest().validate_input_bundle(&tampered),
            Err(BoundaryCaseAssemblyError::InconsistentInputBundle)
        );
    }

    #[test]
    fn coverage_shape_and_limit_errors_fail_closed() {
        let domain = domain(InvalidInputBehavior::RejectBeforeExecution);
        let case = boundary_case(&domain, 2);
        let limit = CorpusBufferByteLimit::new(128).expect("limit");
        assert_eq!(
            assemble_boundary_case_input(&domain, &case, &[], limit),
            Err(BoundaryCaseAssemblyError::InputCoverageMismatch)
        );

        let wrong_count =
            materialize_input_value_case(&input_case(), CorpusElementCount::new(1), limit)
                .expect("wrong count input");
        assert_eq!(
            assemble_boundary_case_input(&domain, &case, &[wrong_count], limit),
            Err(BoundaryCaseAssemblyError::InputBufferMismatch)
        );

        let invalid_value_case = MandatoryInputValueCaseV1::new(
            input_case().target().clone(),
            InputValueDisposition::Invalid {
                behavior: InvalidInputBehavior::RejectBeforeExecution,
            },
        );
        let invalid_value =
            materialize_input_value_case(&invalid_value_case, CorpusElementCount::new(2), limit)
                .expect("invalid dtype recipe can be materialized separately");
        assert_eq!(
            assemble_boundary_case_input(&domain, &case, &[invalid_value], limit),
            Err(BoundaryCaseAssemblyError::InputBufferMismatch)
        );

        let correct =
            materialize_input_value_case(&input_case(), CorpusElementCount::new(2), limit)
                .expect("input");
        assert_eq!(
            assemble_boundary_case_input(
                &domain,
                &case,
                &[correct],
                CorpusBufferByteLimit::new(7).expect("small limit"),
            ),
            Err(BoundaryCaseAssemblyError::BufferLimitExceeded {
                required: crate::CorpusBufferByteLength::new(8),
                limit: CorpusBufferByteLimit::new(7).expect("small limit"),
            })
        );
    }

    #[test]
    fn excluded_boundary_and_persisted_manifest_attacks_are_rejected() {
        let excluded_domain = domain(InvalidInputBehavior::ExplicitlyExcluded);
        let excluded_case = boundary_case(&excluded_domain, 0);
        assert_eq!(
            assemble_boundary_case_input(
                &excluded_domain,
                &excluded_case,
                &[],
                CorpusBufferByteLimit::new(128).expect("limit"),
            ),
            Err(BoundaryCaseAssemblyError::ExcludedBoundaryCase)
        );

        let main_domain = domain(InvalidInputBehavior::RejectBeforeExecution);
        let assembled = assembled(&main_domain, 2);
        let value = serde_json::to_value(assembled.manifest()).expect("manifest json");
        assert!(serde_json::from_value::<MaterializedBoundaryCaseV1>(value.clone()).is_ok());

        let mut wrong_version = value.clone();
        wrong_version["schema_version"] = json!(2);
        assert!(serde_json::from_value::<MaterializedBoundaryCaseV1>(wrong_version).is_err());

        let mut unknown_field = value.clone();
        unknown_field["legacy_bundle"] = json!(true);
        assert!(serde_json::from_value::<MaterializedBoundaryCaseV1>(unknown_field).is_err());

        let mut wrong_path = value.clone();
        wrong_path["arguments"][0]["path"] = json!("cairn/abi/arg-00009.bin");
        assert!(serde_json::from_value::<MaterializedBoundaryCaseV1>(wrong_path).is_err());

        let mut wrong_dtype = value;
        wrong_dtype["arguments"][0]["data_type"] = json!("f64");
        assert!(serde_json::from_value::<MaterializedBoundaryCaseV1>(wrong_dtype).is_err());

        let different_domain = domain(InvalidInputBehavior::ReturnStatus {
            status: crate::StatusCode::new(-9),
        });
        let different_case = boundary_case(&different_domain, 2);
        assert_eq!(
            assembled
                .manifest()
                .validate_sources(&different_domain, &different_case),
            Err(BoundaryCaseAssemblyError::UntrustedBoundaryCase)
        );
        assert_ne!(
            assembled.manifest().domain(),
            id::<CallerDomainBodyArtifact>(
                &cairn_codec::to_vec(&different_domain).expect("domain bytes")
            )
        );
    }
}
