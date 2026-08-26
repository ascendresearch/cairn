//! Deterministic materialization of typed input-value recipes into exact bytes.

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;
use thiserror::Error;

use crate::input_values::{
    BooleanInputPattern, FloatingDataType, FloatingInputPattern, InputValueCaseTarget,
    InputValueDisposition, MandatoryInputValueCaseArtifact, MandatoryInputValueCaseV1,
    SignedIntegerDataType, SignedIntegerInputPattern, UnsignedIntegerDataType,
    UnsignedIntegerInputPattern,
};

/// Failure to turn a typed value recipe into bounded exact bytes.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CorpusMaterializationError {
    /// Only the current pre-release V1 schema is accepted.
    #[error("corpus materialization schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A configured materialization limit used zero.
    #[error("corpus buffer byte limit must be greater than zero")]
    NonPositiveByteLimit,
    /// Element-count multiplication overflowed the typed byte-length boundary.
    #[error("corpus buffer byte length overflow")]
    ByteLengthOverflow,
    /// The host could not reserve the exact checked buffer capacity.
    #[error("corpus buffer allocation failed")]
    AllocationFailed,
    /// Exact bytes would exceed the caller-supplied per-buffer limit.
    #[error("corpus buffer exceeds the caller-supplied per-buffer byte limit")]
    BufferLimitExceeded {
        /// Exact required bytes.
        required: CorpusBufferByteLength,
        /// Configured maximum bytes.
        limit: CorpusBufferByteLimit,
    },
    /// Excluded or unresolved values cannot become executable verdict inputs.
    #[error("input-value disposition is not executable")]
    NonExecutableDisposition,
    /// A persisted manifest contradicts its typed target or byte length.
    #[error("materialized buffer manifest is inconsistent")]
    InconsistentManifest,
    /// Canonical encoding or identity derivation failed.
    #[error("corpus materialization codec error: {message}")]
    Codec {
        /// Adapter-neutral diagnostic.
        message: String,
    },
}

/// Number of logical elements to materialize.
///
/// ```compile_fail
/// use cairn_migration::{CorpusBufferByteLength, CorpusElementCount};
///
/// fn require_elements(_: CorpusElementCount) {}
/// require_elements(CorpusBufferByteLength::new(4));
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CorpusElementCount(u64);

impl CorpusElementCount {
    /// Creates an exact element count; zero represents a valid empty buffer.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the element count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Exact materialized buffer length in bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CorpusBufferByteLength(u64);

impl CorpusBufferByteLength {
    /// Creates an exact byte length.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the byte length.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for CorpusBufferByteLength {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Caller-supplied maximum raw bytes for one materialized buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CorpusBufferByteLimit(u64);

impl CorpusBufferByteLimit {
    /// Creates a positive byte limit.
    ///
    /// # Errors
    ///
    /// Rejects zero instead of giving it disabled or unbounded meaning.
    pub fn new(value: u64) -> Result<Self, CorpusMaterializationError> {
        if value == 0 {
            return Err(CorpusMaterializationError::NonPositiveByteLimit);
        }
        Ok(Self(value))
    }

    /// Returns the configured maximum bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for CorpusBufferByteLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for CorpusBufferByteLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Explicit byte order used by all current corpus scalar encodings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusByteOrder {
    /// Least-significant byte first.
    LittleEndian,
}

/// Content identity domain for raw bytes of one materialized corpus buffer.
pub enum MaterializedCorpusBufferBytesArtifact {}

impl ContentType for MaterializedCorpusBufferBytesArtifact {
    const DOMAIN: &'static str = "migration.materialized-corpus-buffer-bytes.v1";
}

/// Content identity domain for one immutable materialized-buffer manifest.
pub enum MaterializedCorpusBufferArtifact {}

impl ContentType for MaterializedCorpusBufferArtifact {
    const DOMAIN: &'static str = "migration.materialized-corpus-buffer.v1";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ElementWidthBytes(u64);

impl ElementWidthBytes {
    const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MaterializationSchemaV1;

impl Serialize for MaterializationSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for MaterializationSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                CorpusMaterializationError::UnsupportedSchemaVersion,
            )),
        }
    }
}

/// Immutable manifest for exact bytes produced from one typed value obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MaterializedCorpusBufferWire")]
pub struct MaterializedCorpusBufferV1 {
    schema_version: MaterializationSchemaV1,
    source_case: ContentId<MandatoryInputValueCaseArtifact>,
    target: InputValueCaseTarget,
    disposition: InputValueDisposition,
    element_count: CorpusElementCount,
    byte_order: CorpusByteOrder,
    byte_length: CorpusBufferByteLength,
    bytes: ContentId<MaterializedCorpusBufferBytesArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializedCorpusBufferWire {
    schema_version: MaterializationSchemaV1,
    source_case: ContentId<MandatoryInputValueCaseArtifact>,
    target: InputValueCaseTarget,
    disposition: InputValueDisposition,
    element_count: CorpusElementCount,
    byte_order: CorpusByteOrder,
    byte_length: CorpusBufferByteLength,
    bytes: ContentId<MaterializedCorpusBufferBytesArtifact>,
}

impl MaterializedCorpusBufferV1 {
    fn new(
        source_case: ContentId<MandatoryInputValueCaseArtifact>,
        target: InputValueCaseTarget,
        disposition: InputValueDisposition,
        element_count: CorpusElementCount,
        byte_length: CorpusBufferByteLength,
        bytes: ContentId<MaterializedCorpusBufferBytesArtifact>,
    ) -> Result<Self, CorpusMaterializationError> {
        let expected = element_count
            .get()
            .checked_mul(target_width_bytes(&target).get())
            .ok_or(CorpusMaterializationError::ByteLengthOverflow)?;
        if expected != byte_length.get() || !is_executable(&disposition) {
            return Err(CorpusMaterializationError::InconsistentManifest);
        }
        Ok(Self {
            schema_version: MaterializationSchemaV1,
            source_case,
            target,
            disposition,
            element_count,
            byte_order: CorpusByteOrder::LittleEndian,
            byte_length,
            bytes,
        })
    }

    /// Returns the typed source obligation identity.
    #[must_use]
    pub const fn source_case(&self) -> ContentId<MandatoryInputValueCaseArtifact> {
        self.source_case
    }

    /// Returns the exact target and construction recipe.
    #[must_use]
    pub const fn target(&self) -> &InputValueCaseTarget {
        &self.target
    }

    /// Returns whether the source recipe is supported or intentionally invalid.
    #[must_use]
    pub const fn disposition(&self) -> &InputValueDisposition {
        &self.disposition
    }

    /// Returns the logical element count.
    #[must_use]
    pub const fn element_count(&self) -> CorpusElementCount {
        self.element_count
    }

    /// Returns the exact scalar byte order.
    #[must_use]
    pub const fn byte_order(&self) -> CorpusByteOrder {
        self.byte_order
    }

    /// Returns the exact raw byte length.
    #[must_use]
    pub const fn byte_length(&self) -> CorpusBufferByteLength {
        self.byte_length
    }

    /// Returns the raw byte content identity.
    #[must_use]
    pub const fn bytes(&self) -> ContentId<MaterializedCorpusBufferBytesArtifact> {
        self.bytes
    }

    /// Verifies that a source obligation matches the identity and metadata committed here.
    ///
    /// # Errors
    ///
    /// Rejects a different canonical source case or contradictory copied metadata.
    pub fn validate_source_case(
        &self,
        case: &MandatoryInputValueCaseV1,
    ) -> Result<(), CorpusMaterializationError> {
        let case_bytes =
            cairn_codec::to_vec(case).map_err(|error| CorpusMaterializationError::Codec {
                message: error.to_string(),
            })?;
        let observed =
            ContentId::<MandatoryInputValueCaseArtifact>::derive(&case_bytes).map_err(|error| {
                CorpusMaterializationError::Codec {
                    message: error.to_string(),
                }
            })?;
        if observed != self.source_case
            || case.target() != &self.target
            || case.disposition() != &self.disposition
        {
            return Err(CorpusMaterializationError::InconsistentManifest);
        }
        Ok(())
    }

    /// Verifies supplied raw bytes against length and content identity.
    ///
    /// # Errors
    ///
    /// Rejects bytes different from those committed by this manifest.
    pub fn validate_bytes(&self, bytes: &[u8]) -> Result<(), CorpusMaterializationError> {
        if u64::try_from(bytes.len()).ok() != Some(self.byte_length.get()) {
            return Err(CorpusMaterializationError::InconsistentManifest);
        }
        let observed =
            ContentId::<MaterializedCorpusBufferBytesArtifact>::derive(bytes).map_err(|error| {
                CorpusMaterializationError::Codec {
                    message: error.to_string(),
                }
            })?;
        if observed != self.bytes {
            return Err(CorpusMaterializationError::InconsistentManifest);
        }
        Ok(())
    }
}

impl TryFrom<MaterializedCorpusBufferWire> for MaterializedCorpusBufferV1 {
    type Error = CorpusMaterializationError;

    fn try_from(wire: MaterializedCorpusBufferWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        if wire.byte_order != CorpusByteOrder::LittleEndian {
            return Err(CorpusMaterializationError::InconsistentManifest);
        }
        Self::new(
            wire.source_case,
            wire.target,
            wire.disposition,
            wire.element_count,
            wire.byte_length,
            wire.bytes,
        )
    }
}

/// Transient result containing both the immutable manifest and its exact raw bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedCorpusBuffer {
    manifest: MaterializedCorpusBufferV1,
    bytes: Vec<u8>,
}

impl MaterializedCorpusBuffer {
    /// Returns the immutable manifest.
    #[must_use]
    pub const fn manifest(&self) -> &MaterializedCorpusBufferV1 {
        &self.manifest
    }

    /// Returns exact bytes ready for CAS archival.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Materializes one supported or explicitly-invalid typed value recipe.
///
/// # Errors
///
/// Rejects unknown/excluded dispositions, configured limit overflow, canonical codec failures, or
/// byte lengths that cannot fit the host address space after the explicit `u64` checks.
pub fn materialize_input_value_case(
    case: &MandatoryInputValueCaseV1,
    element_count: CorpusElementCount,
    limit: CorpusBufferByteLimit,
) -> Result<MaterializedCorpusBuffer, CorpusMaterializationError> {
    if !is_executable(case.disposition()) {
        return Err(CorpusMaterializationError::NonExecutableDisposition);
    }
    let required = element_count
        .get()
        .checked_mul(target_width_bytes(case.target()).get())
        .ok_or(CorpusMaterializationError::ByteLengthOverflow)?;
    if required > limit.get() {
        return Err(CorpusMaterializationError::BufferLimitExceeded {
            required: CorpusBufferByteLength::new(required),
            limit,
        });
    }
    let capacity =
        usize::try_from(required).map_err(|_| CorpusMaterializationError::ByteLengthOverflow)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| CorpusMaterializationError::AllocationFailed)?;
    encode_target(case.target(), element_count, &mut bytes);
    if bytes.len() != capacity {
        return Err(CorpusMaterializationError::InconsistentManifest);
    }
    let case_bytes =
        cairn_codec::to_vec(case).map_err(|error| CorpusMaterializationError::Codec {
            message: error.to_string(),
        })?;
    let source_case =
        ContentId::<MandatoryInputValueCaseArtifact>::derive(&case_bytes).map_err(|error| {
            CorpusMaterializationError::Codec {
                message: error.to_string(),
            }
        })?;
    let bytes_id =
        ContentId::<MaterializedCorpusBufferBytesArtifact>::derive(&bytes).map_err(|error| {
            CorpusMaterializationError::Codec {
                message: error.to_string(),
            }
        })?;
    let manifest = MaterializedCorpusBufferV1::new(
        source_case,
        case.target().clone(),
        case.disposition().clone(),
        element_count,
        CorpusBufferByteLength::new(required),
        bytes_id,
    )?;
    Ok(MaterializedCorpusBuffer { manifest, bytes })
}

const fn is_executable(disposition: &InputValueDisposition) -> bool {
    matches!(
        disposition,
        InputValueDisposition::Supported | InputValueDisposition::Invalid { .. }
    )
}

const fn target_width_bytes(target: &InputValueCaseTarget) -> ElementWidthBytes {
    match target {
        InputValueCaseTarget::Floating { data_type, .. } => match data_type {
            FloatingDataType::F16 => ElementWidthBytes(2),
            FloatingDataType::F32 => ElementWidthBytes(4),
            FloatingDataType::F64 => ElementWidthBytes(8),
        },
        InputValueCaseTarget::SignedInteger { data_type, .. } => match data_type {
            SignedIntegerDataType::I8 => ElementWidthBytes(1),
            SignedIntegerDataType::I16 => ElementWidthBytes(2),
            SignedIntegerDataType::I32 => ElementWidthBytes(4),
            SignedIntegerDataType::I64 => ElementWidthBytes(8),
        },
        InputValueCaseTarget::UnsignedInteger { data_type, .. } => match data_type {
            UnsignedIntegerDataType::U8 => ElementWidthBytes(1),
            UnsignedIntegerDataType::U16 => ElementWidthBytes(2),
            UnsignedIntegerDataType::U32 => ElementWidthBytes(4),
            UnsignedIntegerDataType::U64 => ElementWidthBytes(8),
        },
        InputValueCaseTarget::Boolean { .. } => ElementWidthBytes(1),
    }
}

fn encode_target(target: &InputValueCaseTarget, count: CorpusElementCount, output: &mut Vec<u8>) {
    for index in 0..count.get() {
        match target {
            InputValueCaseTarget::Floating {
                data_type, pattern, ..
            } => encode_float(*data_type, *pattern, index, output),
            InputValueCaseTarget::SignedInteger {
                data_type, pattern, ..
            } => encode_signed(*data_type, *pattern, index, output),
            InputValueCaseTarget::UnsignedInteger {
                data_type, pattern, ..
            } => encode_unsigned(*data_type, *pattern, output),
            InputValueCaseTarget::Boolean { pattern, .. } => output.push(match pattern {
                BooleanInputPattern::False => 0,
                BooleanInputPattern::True => 1,
                BooleanInputPattern::Alternating => u8::from(index % 2 == 1),
            }),
        }
    }
}

fn encode_float(
    data_type: FloatingDataType,
    pattern: FloatingInputPattern,
    index: u64,
    output: &mut Vec<u8>,
) {
    match data_type {
        FloatingDataType::F16 => {
            output.extend_from_slice(&float16_bits(pattern, index).to_le_bytes());
        }
        FloatingDataType::F32 => {
            output.extend_from_slice(&float32_bits(pattern, index).to_le_bytes());
        }
        FloatingDataType::F64 => {
            output.extend_from_slice(&float64_bits(pattern, index).to_le_bytes());
        }
    }
}

macro_rules! float_bits {
    ($name:ident, $ty:ty, $positive_zero:expr, $negative_zero:expr, $positive_one:expr,
     $negative_one:expr, $lowest:expr, $highest:expr, $normal:expr, $positive_sub:expr,
     $negative_sub:expr, $positive_inf:expr, $negative_inf:expr, $quiet_nan:expr,
     $signaling_nan:expr, $mixed_positive:expr, $mixed_negative:expr) => {
        fn $name(pattern: FloatingInputPattern, index: u64) -> $ty {
            match pattern {
                FloatingInputPattern::PositiveZero => $positive_zero,
                FloatingInputPattern::NegativeZero => $negative_zero,
                FloatingInputPattern::PositiveOne => $positive_one,
                FloatingInputPattern::NegativeOne => $negative_one,
                FloatingInputPattern::LowestFinite => $lowest,
                FloatingInputPattern::HighestFinite => $highest,
                FloatingInputPattern::SmallestPositiveNormal => $normal,
                FloatingInputPattern::SmallestPositiveSubnormal => $positive_sub,
                FloatingInputPattern::SmallestNegativeSubnormal => $negative_sub,
                FloatingInputPattern::PositiveInfinity => $positive_inf,
                FloatingInputPattern::NegativeInfinity => $negative_inf,
                FloatingInputPattern::QuietNan => $quiet_nan,
                FloatingInputPattern::SignalingNan => $signaling_nan,
                FloatingInputPattern::AlternatingUnitCancellation => {
                    if index % 2 == 0 {
                        $positive_one
                    } else {
                        $negative_one
                    }
                }
                FloatingInputPattern::MixedFiniteScaleCancellation => match index % 4 {
                    0 => $mixed_positive,
                    1 | 3 => $positive_one,
                    _ => $mixed_negative,
                },
            }
        }
    };
}

float_bits!(
    float16_bits,
    u16,
    0,
    0x8000,
    0x3c00,
    0xbc00,
    0xfbff,
    0x7bff,
    0x0400,
    1,
    0x8001,
    0x7c00,
    0xfc00,
    0x7e00,
    0x7d00,
    0x6c00,
    0xec00
);
float_bits!(
    float32_bits,
    u32,
    0,
    0x8000_0000,
    0x3f80_0000,
    0xbf80_0000,
    0xff7f_ffff,
    0x7f7f_ffff,
    0x0080_0000,
    1,
    0x8000_0001,
    0x7f80_0000,
    0xff80_0000,
    0x7fc0_0000,
    0x7fa0_0000,
    0x4c00_0000,
    0xcc00_0000
);
float_bits!(
    float64_bits,
    u64,
    0,
    0x8000_0000_0000_0000,
    0x3ff0_0000_0000_0000,
    0xbff0_0000_0000_0000,
    0xffef_ffff_ffff_ffff,
    0x7fef_ffff_ffff_ffff,
    0x0010_0000_0000_0000,
    1,
    0x8000_0000_0000_0001,
    0x7ff0_0000_0000_0000,
    0xfff0_0000_0000_0000,
    0x7ff8_0000_0000_0000,
    0x7ff4_0000_0000_0000,
    0x4350_0000_0000_0000,
    0xc350_0000_0000_0000
);

fn encode_signed(
    data_type: SignedIntegerDataType,
    pattern: SignedIntegerInputPattern,
    index: u64,
    output: &mut Vec<u8>,
) {
    let alternating = if index % 2 == 0 { 1 } else { -1 };
    match data_type {
        SignedIntegerDataType::I8 => output.extend_from_slice(
            &(match pattern {
                SignedIntegerInputPattern::Minimum => i8::MIN,
                SignedIntegerInputPattern::Maximum => i8::MAX,
                SignedIntegerInputPattern::Zero => 0,
                SignedIntegerInputPattern::One => 1,
                SignedIntegerInputPattern::NegativeOne => -1,
                SignedIntegerInputPattern::AlternatingUnitCancellation => alternating,
            })
            .to_le_bytes(),
        ),
        SignedIntegerDataType::I16 => output.extend_from_slice(
            &(match pattern {
                SignedIntegerInputPattern::Minimum => i16::MIN,
                SignedIntegerInputPattern::Maximum => i16::MAX,
                SignedIntegerInputPattern::Zero => 0,
                SignedIntegerInputPattern::One => 1,
                SignedIntegerInputPattern::NegativeOne => -1,
                SignedIntegerInputPattern::AlternatingUnitCancellation => i16::from(alternating),
            })
            .to_le_bytes(),
        ),
        SignedIntegerDataType::I32 => output.extend_from_slice(
            &(match pattern {
                SignedIntegerInputPattern::Minimum => i32::MIN,
                SignedIntegerInputPattern::Maximum => i32::MAX,
                SignedIntegerInputPattern::Zero => 0,
                SignedIntegerInputPattern::One => 1,
                SignedIntegerInputPattern::NegativeOne => -1,
                SignedIntegerInputPattern::AlternatingUnitCancellation => i32::from(alternating),
            })
            .to_le_bytes(),
        ),
        SignedIntegerDataType::I64 => output.extend_from_slice(
            &(match pattern {
                SignedIntegerInputPattern::Minimum => i64::MIN,
                SignedIntegerInputPattern::Maximum => i64::MAX,
                SignedIntegerInputPattern::Zero => 0,
                SignedIntegerInputPattern::One => 1,
                SignedIntegerInputPattern::NegativeOne => -1,
                SignedIntegerInputPattern::AlternatingUnitCancellation => i64::from(alternating),
            })
            .to_le_bytes(),
        ),
    }
}

fn encode_unsigned(
    data_type: UnsignedIntegerDataType,
    pattern: UnsignedIntegerInputPattern,
    output: &mut Vec<u8>,
) {
    macro_rules! value {
        ($ty:ty) => {
            match pattern {
                UnsignedIntegerInputPattern::Minimum => <$ty>::MIN,
                UnsignedIntegerInputPattern::Maximum => <$ty>::MAX,
                UnsignedIntegerInputPattern::One => 1,
            }
        };
    }
    match data_type {
        UnsignedIntegerDataType::U8 => output.push(value!(u8)),
        UnsignedIntegerDataType::U16 => output.extend_from_slice(&value!(u16).to_le_bytes()),
        UnsignedIntegerDataType::U32 => output.extend_from_slice(&value!(u32).to_le_bytes()),
        UnsignedIntegerDataType::U64 => output.extend_from_slice(&value!(u64).to_le_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use cairn_protocol::ContentId;
    use serde_json::json;

    use super::{
        CorpusBufferByteLength, CorpusBufferByteLimit, CorpusByteOrder, CorpusElementCount,
        CorpusMaterializationError, MaterializedCorpusBufferArtifact, materialize_input_value_case,
    };
    use crate::{
        BooleanInputPattern, BufferName, FloatingDataType, FloatingInputPattern,
        InputValueCaseTarget, InputValueDisposition, InvalidInputBehavior,
        MandatoryInputValueCaseV1, MigrationDomainExclusionArtifact, SignedIntegerDataType,
        SignedIntegerInputPattern, UnsignedIntegerDataType, UnsignedIntegerInputPattern,
    };

    fn case(
        target: InputValueCaseTarget,
        disposition: InputValueDisposition,
    ) -> MandatoryInputValueCaseV1 {
        MandatoryInputValueCaseV1::new(target, disposition)
    }

    fn buffer() -> BufferName {
        BufferName::new("input").expect("buffer name")
    }

    fn supported_float(
        data_type: FloatingDataType,
        pattern: FloatingInputPattern,
    ) -> MandatoryInputValueCaseV1 {
        case(
            InputValueCaseTarget::Floating {
                buffer: buffer(),
                data_type,
                pattern,
            },
            InputValueDisposition::Supported,
        )
    }

    fn materialize(
        case: &MandatoryInputValueCaseV1,
        count: u64,
    ) -> super::MaterializedCorpusBuffer {
        materialize_input_value_case(
            case,
            CorpusElementCount::new(count),
            CorpusBufferByteLimit::new(1_024).expect("limit"),
        )
        .expect("materialize")
    }

    #[test]
    fn floating_patterns_have_exact_little_endian_golden_bytes() {
        let negative_zero = materialize(
            &supported_float(FloatingDataType::F16, FloatingInputPattern::NegativeZero),
            2,
        );
        assert_eq!(negative_zero.bytes(), &[0x00, 0x80, 0x00, 0x80]);

        let quiet_nan = materialize(
            &supported_float(FloatingDataType::F32, FloatingInputPattern::QuietNan),
            1,
        );
        assert_eq!(quiet_nan.bytes(), &[0x00, 0x00, 0xc0, 0x7f]);

        let negative_subnormal = materialize(
            &supported_float(
                FloatingDataType::F64,
                FloatingInputPattern::SmallestNegativeSubnormal,
            ),
            1,
        );
        assert_eq!(
            negative_subnormal.bytes(),
            &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]
        );

        let mixed = materialize(
            &supported_float(
                FloatingDataType::F32,
                FloatingInputPattern::MixedFiniteScaleCancellation,
            ),
            4,
        );
        assert_eq!(
            mixed.bytes(),
            &[
                0x00, 0x00, 0x00, 0x4c, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0xcc, 0x00, 0x00,
                0x80, 0x3f,
            ]
        );
    }

    #[test]
    fn integer_and_boolean_patterns_have_exact_golden_bytes() {
        let signed = case(
            InputValueCaseTarget::SignedInteger {
                buffer: buffer(),
                data_type: SignedIntegerDataType::I16,
                pattern: SignedIntegerInputPattern::AlternatingUnitCancellation,
            },
            InputValueDisposition::Supported,
        );
        assert_eq!(
            materialize(&signed, 4).bytes(),
            &[0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff]
        );

        let unsigned = case(
            InputValueCaseTarget::UnsignedInteger {
                buffer: buffer(),
                data_type: UnsignedIntegerDataType::U64,
                pattern: UnsignedIntegerInputPattern::Maximum,
            },
            InputValueDisposition::Supported,
        );
        assert_eq!(materialize(&unsigned, 1).bytes(), &[0xff; 8]);

        let boolean = case(
            InputValueCaseTarget::Boolean {
                buffer: buffer(),
                pattern: BooleanInputPattern::Alternating,
            },
            InputValueDisposition::Supported,
        );
        assert_eq!(materialize(&boolean, 3).bytes(), &[0, 1, 0]);
    }

    #[test]
    fn manifest_binds_source_metadata_and_exact_bytes() {
        let source = supported_float(FloatingDataType::F32, FloatingInputPattern::PositiveOne);
        let materialized = materialize(&source, 3);
        let repeated = materialize(&source, 3);
        assert_eq!(materialized, repeated);
        assert_eq!(
            materialized.manifest().byte_length(),
            CorpusBufferByteLength::new(12)
        );
        assert_eq!(
            materialized.manifest().byte_order(),
            CorpusByteOrder::LittleEndian
        );
        assert_eq!(
            materialized.manifest().disposition(),
            &InputValueDisposition::Supported
        );
        materialized
            .manifest()
            .validate_source_case(&source)
            .expect("source case identity");
        materialized
            .manifest()
            .validate_bytes(materialized.bytes())
            .expect("bytes identity");

        let manifest_bytes =
            cairn_codec::to_vec(materialized.manifest()).expect("canonical manifest");
        let manifest_id = ContentId::<MaterializedCorpusBufferArtifact>::derive(&manifest_bytes)
            .expect("manifest identity");
        let repeated_bytes =
            cairn_codec::to_vec(repeated.manifest()).expect("canonical repeated manifest");
        assert_eq!(
            manifest_id,
            ContentId::<MaterializedCorpusBufferArtifact>::derive(&repeated_bytes)
                .expect("repeated manifest identity")
        );

        let changed_count = materialize(&source, 2);
        let changed_manifest_bytes =
            cairn_codec::to_vec(changed_count.manifest()).expect("changed manifest");
        assert_ne!(
            manifest_id,
            ContentId::<MaterializedCorpusBufferArtifact>::derive(&changed_manifest_bytes)
                .expect("changed manifest identity")
        );

        let different_source =
            supported_float(FloatingDataType::F32, FloatingInputPattern::NegativeOne);
        assert!(
            materialized
                .manifest()
                .validate_source_case(&different_source)
                .is_err()
        );
        let mut corrupted = materialized.bytes().to_vec();
        corrupted[0] ^= 1;
        assert!(materialized.manifest().validate_bytes(&corrupted).is_err());
    }

    #[test]
    fn limits_overflow_empty_buffers_and_dispositions_fail_closed() {
        assert_eq!(
            CorpusBufferByteLimit::new(0),
            Err(CorpusMaterializationError::NonPositiveByteLimit)
        );
        let source = supported_float(FloatingDataType::F64, FloatingInputPattern::PositiveOne);
        assert_eq!(
            materialize_input_value_case(
                &source,
                CorpusElementCount::new(u64::MAX),
                CorpusBufferByteLimit::new(u64::MAX).expect("limit"),
            ),
            Err(CorpusMaterializationError::ByteLengthOverflow)
        );
        assert_eq!(
            materialize_input_value_case(
                &source,
                CorpusElementCount::new(2),
                CorpusBufferByteLimit::new(15).expect("limit"),
            ),
            Err(CorpusMaterializationError::BufferLimitExceeded {
                required: CorpusBufferByteLength::new(16),
                limit: CorpusBufferByteLimit::new(15).expect("limit"),
            })
        );

        let empty = materialize_input_value_case(
            &source,
            CorpusElementCount::new(0),
            CorpusBufferByteLimit::new(1).expect("limit"),
        )
        .expect("empty buffer");
        assert!(empty.bytes().is_empty());
        empty
            .manifest()
            .validate_bytes(&[])
            .expect("empty bytes identity");

        let unknown = case(source.target().clone(), InputValueDisposition::Unknown);
        assert_eq!(
            materialize_input_value_case(
                &unknown,
                CorpusElementCount::new(1),
                CorpusBufferByteLimit::new(8).expect("limit"),
            ),
            Err(CorpusMaterializationError::NonExecutableDisposition)
        );
        let exclusion = ContentId::<MigrationDomainExclusionArtifact>::derive(b"excluded-family")
            .expect("exclusion identity");
        let excluded = case(
            source.target().clone(),
            InputValueDisposition::ExplicitlyExcluded { exclusion },
        );
        assert_eq!(
            materialize_input_value_case(
                &excluded,
                CorpusElementCount::new(1),
                CorpusBufferByteLimit::new(8).expect("limit"),
            ),
            Err(CorpusMaterializationError::NonExecutableDisposition)
        );
        let invalid = case(
            source.target().clone(),
            InputValueDisposition::Invalid {
                behavior: InvalidInputBehavior::RejectBeforeExecution,
            },
        );
        assert!(
            materialize_input_value_case(
                &invalid,
                CorpusElementCount::new(1),
                CorpusBufferByteLimit::new(8).expect("limit"),
            )
            .is_ok()
        );
    }

    #[test]
    fn persisted_manifest_accepts_only_consistent_v1() {
        let source = supported_float(FloatingDataType::F16, FloatingInputPattern::PositiveOne);
        let materialized = materialize(&source, 2);
        let value = serde_json::to_value(materialized.manifest()).expect("manifest json");
        let decoded: super::MaterializedCorpusBufferV1 =
            serde_json::from_value(value.clone()).expect("valid manifest");
        assert_eq!(&decoded, materialized.manifest());

        let mut wrong_version = value.clone();
        wrong_version["schema_version"] = json!(2);
        assert!(
            serde_json::from_value::<super::MaterializedCorpusBufferV1>(wrong_version).is_err()
        );

        let mut unknown_field = value.clone();
        unknown_field["legacy_bytes"] = json!("unused");
        assert!(
            serde_json::from_value::<super::MaterializedCorpusBufferV1>(unknown_field).is_err()
        );

        let mut wrong_length = value.clone();
        wrong_length["byte_length"] = json!(3);
        assert!(serde_json::from_value::<super::MaterializedCorpusBufferV1>(wrong_length).is_err());

        let mut unknown_order = value;
        unknown_order["byte_order"] = json!("native-endian");
        assert!(
            serde_json::from_value::<super::MaterializedCorpusBufferV1>(unknown_order).is_err()
        );
    }
}
