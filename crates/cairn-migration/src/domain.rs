//! Strongly typed operator-domain contract and trusted mandatory boundary derivation.

use std::collections::{BTreeMap, BTreeSet};
use std::{fmt, str::FromStr};

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::CallerDomainBodyArtifact;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::input_values::InputValueDomainV1;
use crate::memory_surface::{BufferAliasingContractV1, BufferMemoryContractV1, BufferPairV1};

const MAX_DOMAIN_LABEL_LEN: usize = 128;

/// Failure to construct or derive a migration-domain contract.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DomainContractError {
    /// Only schema V1 is accepted during pre-release development.
    #[error("migration domain schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A strong domain label is invalid.
    #[error("{kind} is not a valid domain label")]
    InvalidLabel {
        /// Semantic label kind that failed.
        kind: &'static str,
    },
    /// An inclusive range has its bounds reversed.
    #[error("{kind} range minimum exceeds maximum")]
    ReversedRange {
        /// Unit of the invalid range.
        kind: &'static str,
    },
    /// A modulus cannot produce meaningful below/at/above boundaries.
    #[error("boundary modulus must be greater than one")]
    InvalidModulus,
    /// A collection that represents a canonical set is empty.
    #[error("{field} must contain at least one value")]
    EmptySet {
        /// Collection that failed.
        field: &'static str,
    },
    /// A canonical collection is duplicated or out of order.
    #[error("{field} must be in strict canonical order without duplicates")]
    NonCanonicalSet {
        /// Collection that failed.
        field: &'static str,
    },
    /// Two ABI arguments reuse one position.
    #[error("operator ABI argument index {index} is duplicated")]
    DuplicateArgumentIndex {
        /// Reused zero-based position.
        index: u16,
    },
    /// Buffer and scalar namespaces collide at the ABI boundary.
    #[error("operator ABI argument name {name} is duplicated")]
    DuplicateArgumentName {
        /// Reused wire name.
        name: String,
    },
    /// A buffer shape cites a shape symbol absent from the contract.
    #[error("buffer {buffer} cites unknown shape symbol {symbol}")]
    UnknownShapeSymbol {
        /// Buffer containing the reference.
        buffer: String,
        /// Missing symbol.
        symbol: String,
    },
    /// A shape-symbol source contradicts the ABI declarations.
    #[error("shape symbol {symbol} has invalid source: {reason}")]
    InvalidShapeSource {
        /// Symbol whose source failed validation.
        symbol: String,
        /// Stable diagnostic.
        reason: &'static str,
    },
    /// A scalar parameter uses a non-integer type with an integer domain.
    #[error("scalar parameter {parameter} requires an integer or bool data type")]
    InvalidScalarDataType {
        /// Parameter that failed.
        parameter: String,
    },
    /// A bool parameter declares values outside zero and one.
    #[error("bool parameter {parameter} must have a range within 0..=1")]
    InvalidBooleanRange {
        /// Parameter that failed.
        parameter: String,
    },
    /// An integer range cannot be represented by its declared ABI data type.
    #[error("scalar parameter {parameter} range is outside its declared data type")]
    ScalarRangeOutsideDataType {
        /// Parameter that failed.
        parameter: String,
    },
    /// A buffer value-domain category disagrees with its element dtype.
    #[error("buffer {buffer} has a value-domain category incompatible with its data type")]
    InputValueDomainTypeMismatch {
        /// Buffer whose typed value domain disagreed with its dtype.
        buffer: String,
    },
    /// A special-value exclusion was not present in the domain's canonical exclusion set.
    #[error("buffer {buffer} cites an input-value exclusion absent from the domain exclusions")]
    UnlistedInputValueExclusion {
        /// Buffer containing the dangling exclusion edge.
        buffer: String,
    },
    /// A required alignment was not a non-trivial power of two.
    #[error("required buffer alignment must be a power of two greater than one")]
    InvalidRequiredAlignment,
    /// A memory quantity that represents a perturbation used zero.
    #[error("{field} must be greater than zero")]
    NonPositiveMemoryQuantity {
        /// Typed quantity that failed validation.
        field: &'static str,
    },
    /// A deliberate misalignment offset did not lie below its required alignment.
    #[error("misalignment offset must be the policy-sized one byte below the required alignment")]
    InvalidMisalignmentPattern,
    /// A buffer pair reused one name or was not in strict lexical order.
    #[error("buffer pair must contain two distinct names in strict lexical order")]
    InvalidBufferPair,
    /// An aliasing declaration cites a buffer absent from the ABI.
    #[error("aliasing contract cites unknown buffer {buffer}")]
    UnknownAliasingBuffer {
        /// Missing buffer name.
        buffer: String,
    },
    /// Pairwise aliasing declarations did not cover the exact ABI buffer-pair set.
    #[error("buffer aliasing contracts must cover every distinct ABI buffer pair exactly once")]
    IncompleteAliasingContracts,
    /// A memory-surface exclusion was not present in the domain's canonical exclusion set.
    #[error("memory-surface contract cites an exclusion absent from the domain exclusions")]
    UnlistedMemorySurfaceExclusion,
    /// The contract has no input-capable buffer.
    #[error("migration domain requires at least one input-capable buffer")]
    MissingInputBuffer,
    /// The contract has no output-capable buffer.
    #[error("migration domain requires at least one output-capable buffer")]
    MissingOutputBuffer,
    /// A shape rank cannot be represented by the V1 typed rank.
    #[error("buffer rank exceeds the V1 u16 boundary")]
    RankOverflow,
    /// Canonical encoding or identity derivation failed.
    #[error("migration domain codec failure: {message}")]
    Codec {
        /// Adapter-neutral diagnostic.
        message: String,
    },
    /// A derived case contradicts the domain or its own target.
    #[error("mandatory domain case is invalid: {reason}")]
    InvalidDerivedCase {
        /// Stable diagnostic.
        reason: &'static str,
    },
}

/// The single current migration-domain schema version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct MigrationDomainSchemaV1;

impl Serialize for MigrationDomainSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for MigrationDomainSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                DomainContractError::UnsupportedSchemaVersion,
            )),
        }
    }
}

fn validate_label(value: &str, kind: &'static str) -> Result<(), DomainContractError> {
    if value.is_empty()
        || value.len() > MAX_DOMAIN_LABEL_LEN
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
    {
        return Err(DomainContractError::InvalidLabel { kind });
    }
    Ok(())
}

macro_rules! domain_label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated domain label.
            ///
            /// # Errors
            ///
            /// Rejects an empty, oversized, or non-canonical value.
            pub fn new(value: impl Into<String>) -> Result<Self, DomainContractError> {
                let value = value.into();
                validate_label(&value, $kind)?;
                Ok(Self(value))
            }

            /// Returns the exact wire label.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainContractError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

domain_label!(
    /// Source operator function/ABI entry point.
    EntryPointName,
    "operator entry point"
);
domain_label!(
    /// Buffer argument identity. It is intentionally not a scalar parameter name.
    ///
    /// ```compile_fail
    /// use cairn_migration::{BufferName, ScalarParameterName};
    /// let buffer = BufferName::new("input").unwrap();
    /// let _parameter: ScalarParameterName = buffer;
    /// ```
    BufferName,
    "buffer name"
);
domain_label!(
    /// Scalar ABI parameter identity.
    ScalarParameterName,
    "scalar parameter name"
);
domain_label!(
    /// Logical shape variable independent from its ABI binding.
    ShapeSymbolName,
    "shape symbol"
);

/// Zero-based ABI argument position.
///
/// ```compile_fail
/// use cairn_migration::{ArgumentIndex, DimensionAxis};
/// let argument = ArgumentIndex::new(0);
/// let _axis: DimensionAxis = argument;
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ArgumentIndex(u16);

impl ArgumentIndex {
    /// Creates a zero-based ABI position.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the zero-based position.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Zero-based dimension axis. It cannot be used as an ABI argument index.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DimensionAxis(u16);

impl DimensionAxis {
    /// Creates a zero-based dimension axis.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the zero-based axis.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Typed buffer rank, distinct from argument positions and axes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ShapeRank(u16);

impl ShapeRank {
    fn from_len(value: usize) -> Result<Self, DomainContractError> {
        u16::try_from(value)
            .map(Self)
            .map_err(|_| DomainContractError::RankOverflow)
    }

    /// Returns the number of dimensions.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Unsigned shape extent.
///
/// ```compile_fail
/// use cairn_migration::{ExtentValue, IntegerValue};
/// let extent = ExtentValue::new(4);
/// let _scalar: IntegerValue = extent;
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ExtentValue(u64);

impl ExtentValue {
    /// Creates an extent; zero remains meaningful for empty/degenerate shapes.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the unsigned extent.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Signed scalar integer value, distinct from a shape extent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntegerValue(i64);

impl IntegerValue {
    /// Creates a signed scalar value.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the signed value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Required status result for an invalid input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StatusCode(i32);

impl StatusCode {
    /// Creates a typed status code.
    #[must_use]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    /// Returns the ABI status value.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

/// Positive shape boundary modulus used for tile/alignment tail cases.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ExtentModulus(u64);

impl ExtentModulus {
    /// Creates a modulus capable of distinct below/at/above cases.
    ///
    /// # Errors
    ///
    /// Rejects zero and one.
    pub fn new(value: u64) -> Result<Self, DomainContractError> {
        if value <= 1 {
            return Err(DomainContractError::InvalidModulus);
        }
        Ok(Self(value))
    }

    /// Returns the unsigned modulus.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ExtentModulus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Inclusive valid range for a shape extent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ExtentRangeWire")]
pub struct InclusiveExtentRange {
    minimum: ExtentValue,
    maximum: ExtentValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtentRangeWire {
    minimum: ExtentValue,
    maximum: ExtentValue,
}

impl InclusiveExtentRange {
    /// Creates an ordered inclusive extent range.
    ///
    /// # Errors
    ///
    /// Rejects a minimum above the maximum.
    pub fn new(minimum: ExtentValue, maximum: ExtentValue) -> Result<Self, DomainContractError> {
        if minimum > maximum {
            return Err(DomainContractError::ReversedRange { kind: "extent" });
        }
        Ok(Self { minimum, maximum })
    }

    /// Returns the inclusive minimum.
    #[must_use]
    pub const fn minimum(self) -> ExtentValue {
        self.minimum
    }

    /// Returns the inclusive maximum.
    #[must_use]
    pub const fn maximum(self) -> ExtentValue {
        self.maximum
    }

    /// Tests membership in the valid range.
    #[must_use]
    pub const fn contains(self, value: ExtentValue) -> bool {
        self.minimum.0 <= value.0 && value.0 <= self.maximum.0
    }
}

impl TryFrom<ExtentRangeWire> for InclusiveExtentRange {
    type Error = DomainContractError;

    fn try_from(wire: ExtentRangeWire) -> Result<Self, Self::Error> {
        Self::new(wire.minimum, wire.maximum)
    }
}

/// Inclusive valid range for an integer scalar parameter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "IntegerRangeWire")]
pub struct InclusiveIntegerRange {
    minimum: IntegerValue,
    maximum: IntegerValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegerRangeWire {
    minimum: IntegerValue,
    maximum: IntegerValue,
}

impl InclusiveIntegerRange {
    /// Creates an ordered inclusive scalar range.
    ///
    /// # Errors
    ///
    /// Rejects a minimum above the maximum.
    pub fn new(minimum: IntegerValue, maximum: IntegerValue) -> Result<Self, DomainContractError> {
        if minimum > maximum {
            return Err(DomainContractError::ReversedRange { kind: "integer" });
        }
        Ok(Self { minimum, maximum })
    }

    /// Returns the inclusive minimum.
    #[must_use]
    pub const fn minimum(self) -> IntegerValue {
        self.minimum
    }

    /// Returns the inclusive maximum.
    #[must_use]
    pub const fn maximum(self) -> IntegerValue {
        self.maximum
    }

    /// Tests membership in the valid range.
    #[must_use]
    pub const fn contains(self, value: IntegerValue) -> bool {
        self.minimum.0 <= value.0 && value.0 <= self.maximum.0
    }
}

impl TryFrom<IntegerRangeWire> for InclusiveIntegerRange {
    type Error = DomainContractError;

    fn try_from(wire: IntegerRangeWire) -> Result<Self, Self::Error> {
        Self::new(wire.minimum, wire.maximum)
    }
}

/// Closed V1 element/scalar data types used by the first operator slice.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataType {
    /// IEEE binary16.
    F16,
    /// IEEE binary32.
    F32,
    /// IEEE binary64.
    F64,
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 32-bit integer.
    I32,
    /// Signed 64-bit integer.
    I64,
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
    /// Boolean represented by zero or one.
    Bool,
}

impl DataType {
    const fn supports_integer_domain(self) -> bool {
        !matches!(self, Self::F16 | Self::F32 | Self::F64)
    }

    const fn integer_bounds(self) -> Option<(i64, i64)> {
        match self {
            Self::I8 => Some((i8::MIN as i64, i8::MAX as i64)),
            Self::I16 => Some((i16::MIN as i64, i16::MAX as i64)),
            Self::I32 => Some((i32::MIN as i64, i32::MAX as i64)),
            Self::I64 => Some((i64::MIN, i64::MAX)),
            Self::U8 => Some((0, u8::MAX as i64)),
            Self::U16 => Some((0, u16::MAX as i64)),
            Self::U32 => Some((0, u32::MAX as i64)),
            Self::U64 => Some((0, i64::MAX)),
            Self::Bool => Some((0, 1)),
            Self::F16 | Self::F32 | Self::F64 => None,
        }
    }
}

/// Buffer direction at the operator ABI.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BufferRole {
    /// Read-only input.
    Input,
    /// Write-only output.
    Output,
    /// Read/write input and output.
    InputOutput,
}

/// Buffer access and its structurally required input-value domain.
///
/// An output-only value cannot carry an input domain, while both input-capable variants require
/// one. The impossible role/domain combinations therefore have no Rust representation.
///
/// ```compile_fail
/// use cairn_migration::{BufferAccessV1, InputValueDomainV1};
///
/// let domain = InputValueDomainV1::Boolean;
/// let access = BufferAccessV1::Output { value_domain: domain };
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BufferAccessV1 {
    /// Read-only input with an explicit value domain.
    Input {
        /// Caller-declared input values.
        value_domain: InputValueDomainV1,
    },
    /// Write-only output, which has no input-value declaration.
    Output,
    /// Read/write buffer with an explicit input-value domain.
    InputOutput {
        /// Caller-declared input values.
        value_domain: InputValueDomainV1,
    },
}

impl BufferAccessV1 {
    const fn role(&self) -> BufferRole {
        match self {
            Self::Input { .. } => BufferRole::Input,
            Self::Output => BufferRole::Output,
            Self::InputOutput { .. } => BufferRole::InputOutput,
        }
    }

    const fn input_value_domain(&self) -> Option<&InputValueDomainV1> {
        match self {
            Self::Input { value_domain } | Self::InputOutput { value_domain } => Some(value_domain),
            Self::Output => None,
        }
    }
}

/// One buffer dimension, either fixed or bound to a declared symbol.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DimensionSpec {
    /// Fixed dimension.
    Constant {
        /// Exact fixed extent.
        extent: ExtentValue,
    },
    /// Dimension resolved from a logical symbol.
    Symbol {
        /// Declared shape symbol.
        symbol: ShapeSymbolName,
    },
}

/// Constructor input for one buffer ABI argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferContractInput {
    /// Zero-based ABI position.
    pub argument_index: ArgumentIndex,
    /// Strong buffer name.
    pub name: BufferName,
    /// Access role coupled to its required input-value declaration.
    pub access: BufferAccessV1,
    /// Element type.
    pub data_type: DataType,
    /// Ordered shape dimensions; empty means a scalar buffer value.
    pub shape: Vec<DimensionSpec>,
    /// Pointer, alignment, and capacity behavior at valid non-empty shapes.
    pub memory: BufferMemoryContractV1,
}

/// Strict V1 buffer argument contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "BufferContractWire")]
pub struct BufferContractV1 {
    schema_version: MigrationDomainSchemaV1,
    argument_index: ArgumentIndex,
    name: BufferName,
    access: BufferAccessV1,
    data_type: DataType,
    shape: Vec<DimensionSpec>,
    memory: BufferMemoryContractV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BufferContractWire {
    schema_version: MigrationDomainSchemaV1,
    argument_index: ArgumentIndex,
    name: BufferName,
    access: BufferAccessV1,
    data_type: DataType,
    shape: Vec<DimensionSpec>,
    memory: BufferMemoryContractV1,
}

impl BufferContractV1 {
    /// Creates a typed buffer argument.
    ///
    /// # Errors
    ///
    /// Rejects a rank outside the typed V1 range.
    pub fn new(input: BufferContractInput) -> Result<Self, DomainContractError> {
        let _ = ShapeRank::from_len(input.shape.len())?;
        if input
            .access
            .input_value_domain()
            .is_some_and(|domain| !domain.is_compatible_with(input.data_type))
        {
            return Err(DomainContractError::InputValueDomainTypeMismatch {
                buffer: input.name.to_string(),
            });
        }
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            argument_index: input.argument_index,
            name: input.name,
            access: input.access,
            data_type: input.data_type,
            shape: input.shape,
            memory: input.memory,
        })
    }

    /// Returns the ABI position.
    #[must_use]
    pub const fn argument_index(&self) -> ArgumentIndex {
        self.argument_index
    }

    /// Returns the buffer name.
    #[must_use]
    pub const fn name(&self) -> &BufferName {
        &self.name
    }

    /// Returns its input/output role.
    #[must_use]
    pub const fn role(&self) -> BufferRole {
        self.access.role()
    }

    /// Returns the element type.
    #[must_use]
    pub const fn data_type(&self) -> DataType {
        self.data_type
    }

    /// Returns the typed rank.
    #[must_use]
    pub fn rank(&self) -> ShapeRank {
        ShapeRank(u16::try_from(self.shape.len()).unwrap_or(u16::MAX))
    }

    /// Returns ordered dimension expressions.
    #[must_use]
    pub fn shape(&self) -> &[DimensionSpec] {
        &self.shape
    }

    /// Returns pointer, alignment, and capacity behavior for non-empty shapes.
    #[must_use]
    pub const fn memory(&self) -> &BufferMemoryContractV1 {
        &self.memory
    }

    /// Returns the declared value domain for input-capable buffers.
    #[must_use]
    pub const fn input_value_domain(&self) -> Option<&InputValueDomainV1> {
        self.access.input_value_domain()
    }
}

impl TryFrom<BufferContractWire> for BufferContractV1 {
    type Error = DomainContractError;

    fn try_from(wire: BufferContractWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(BufferContractInput {
            argument_index: wire.argument_index,
            name: wire.name,
            access: wire.access,
            data_type: wire.data_type,
            shape: wire.shape,
            memory: wire.memory,
        })
    }
}

/// Product role of one integer scalar parameter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScalarParameterRole {
    /// Supplies one logical shape extent.
    ShapeExtent,
    /// Controls non-shape operator behavior.
    Control,
}

/// Required observable behavior outside a declared valid range.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InvalidInputBehavior {
    /// Operator must return this status code.
    ReturnStatus {
        /// Required typed status.
        status: StatusCode,
    },
    /// Trusted input validation must reject before execution.
    RejectBeforeExecution,
    /// The value is explicitly excluded; no behavior claim is requested.
    ExplicitlyExcluded,
}

/// Constructor input for one scalar ABI parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarParameterContractInput {
    /// Zero-based ABI position.
    pub argument_index: ArgumentIndex,
    /// Strong scalar name.
    pub name: ScalarParameterName,
    /// Shape or control role.
    pub role: ScalarParameterRole,
    /// Integer/bool ABI type.
    pub data_type: DataType,
    /// Inclusive valid values.
    pub valid_range: InclusiveIntegerRange,
    /// Required invalid behavior.
    pub invalid_behavior: InvalidInputBehavior,
}

/// Strict V1 scalar ABI parameter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ScalarParameterContractWire")]
pub struct ScalarParameterContractV1 {
    schema_version: MigrationDomainSchemaV1,
    argument_index: ArgumentIndex,
    name: ScalarParameterName,
    role: ScalarParameterRole,
    data_type: DataType,
    valid_range: InclusiveIntegerRange,
    invalid_behavior: InvalidInputBehavior,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScalarParameterContractWire {
    schema_version: MigrationDomainSchemaV1,
    argument_index: ArgumentIndex,
    name: ScalarParameterName,
    role: ScalarParameterRole,
    data_type: DataType,
    valid_range: InclusiveIntegerRange,
    invalid_behavior: InvalidInputBehavior,
}

impl ScalarParameterContractV1 {
    /// Creates an integer/bool scalar parameter.
    ///
    /// # Errors
    ///
    /// Rejects floating data types and bool ranges outside zero/one.
    pub fn new(input: ScalarParameterContractInput) -> Result<Self, DomainContractError> {
        if !input.data_type.supports_integer_domain() {
            return Err(DomainContractError::InvalidScalarDataType {
                parameter: input.name.to_string(),
            });
        }
        if input.data_type == DataType::Bool
            && (input.valid_range.minimum().get() < 0 || input.valid_range.maximum().get() > 1)
        {
            return Err(DomainContractError::InvalidBooleanRange {
                parameter: input.name.to_string(),
            });
        }
        let (type_minimum, type_maximum) = input.data_type.integer_bounds().ok_or_else(|| {
            DomainContractError::InvalidScalarDataType {
                parameter: input.name.to_string(),
            }
        })?;
        if input.valid_range.minimum().get() < type_minimum
            || input.valid_range.maximum().get() > type_maximum
        {
            return Err(DomainContractError::ScalarRangeOutsideDataType {
                parameter: input.name.to_string(),
            });
        }
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            argument_index: input.argument_index,
            name: input.name,
            role: input.role,
            data_type: input.data_type,
            valid_range: input.valid_range,
            invalid_behavior: input.invalid_behavior,
        })
    }

    /// Returns the ABI position.
    #[must_use]
    pub const fn argument_index(&self) -> ArgumentIndex {
        self.argument_index
    }

    /// Returns the scalar name.
    #[must_use]
    pub const fn name(&self) -> &ScalarParameterName {
        &self.name
    }

    /// Returns the product role.
    #[must_use]
    pub const fn role(&self) -> ScalarParameterRole {
        self.role
    }

    /// Returns the valid integer range.
    #[must_use]
    pub const fn valid_range(&self) -> InclusiveIntegerRange {
        self.valid_range
    }

    /// Returns required invalid behavior.
    #[must_use]
    pub const fn invalid_behavior(&self) -> &InvalidInputBehavior {
        &self.invalid_behavior
    }
}

impl TryFrom<ScalarParameterContractWire> for ScalarParameterContractV1 {
    type Error = DomainContractError;

    fn try_from(wire: ScalarParameterContractWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(ScalarParameterContractInput {
            argument_index: wire.argument_index,
            name: wire.name,
            role: wire.role,
            data_type: wire.data_type,
            valid_range: wire.valid_range,
            invalid_behavior: wire.invalid_behavior,
        })
    }
}

/// Exact ABI source from which a logical shape symbol is observed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ShapeSymbolSource {
    /// Integer scalar parameter supplies the extent.
    ScalarParameter {
        /// Exact scalar argument.
        parameter: ScalarParameterName,
    },
    /// A buffer dimension supplies the extent.
    BufferDimension {
        /// Exact buffer argument.
        buffer: BufferName,
        /// Typed dimension axis.
        axis: DimensionAxis,
    },
}

/// Constructor input for one logical shape symbol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShapeSymbolContractInput {
    /// Logical symbol name.
    pub name: ShapeSymbolName,
    /// Inclusive valid extents.
    pub valid_range: InclusiveExtentRange,
    /// ABI observation source.
    pub source: ShapeSymbolSource,
    /// Tile/alignment moduli in strict numeric order.
    pub boundary_moduli: Vec<ExtentModulus>,
    /// Required behavior outside the range.
    pub invalid_behavior: InvalidInputBehavior,
}

/// Strict logical shape variable contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ShapeSymbolContractWire")]
pub struct ShapeSymbolContractV1 {
    schema_version: MigrationDomainSchemaV1,
    name: ShapeSymbolName,
    valid_range: InclusiveExtentRange,
    source: ShapeSymbolSource,
    boundary_moduli: Vec<ExtentModulus>,
    invalid_behavior: InvalidInputBehavior,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapeSymbolContractWire {
    schema_version: MigrationDomainSchemaV1,
    name: ShapeSymbolName,
    valid_range: InclusiveExtentRange,
    source: ShapeSymbolSource,
    boundary_moduli: Vec<ExtentModulus>,
    invalid_behavior: InvalidInputBehavior,
}

impl ShapeSymbolContractV1 {
    /// Creates a logical shape symbol and its boundary obligations.
    ///
    /// # Errors
    ///
    /// Rejects duplicate or non-canonical moduli.
    pub fn new(input: ShapeSymbolContractInput) -> Result<Self, DomainContractError> {
        if input
            .boundary_moduli
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(DomainContractError::NonCanonicalSet {
                field: "shape boundary moduli",
            });
        }
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            name: input.name,
            valid_range: input.valid_range,
            source: input.source,
            boundary_moduli: input.boundary_moduli,
            invalid_behavior: input.invalid_behavior,
        })
    }

    /// Returns the logical symbol name.
    #[must_use]
    pub const fn name(&self) -> &ShapeSymbolName {
        &self.name
    }

    /// Returns the valid extent range.
    #[must_use]
    pub const fn valid_range(&self) -> InclusiveExtentRange {
        self.valid_range
    }

    /// Returns the exact ABI source.
    #[must_use]
    pub const fn source(&self) -> &ShapeSymbolSource {
        &self.source
    }

    /// Returns tile/alignment boundary moduli.
    #[must_use]
    pub fn boundary_moduli(&self) -> &[ExtentModulus] {
        &self.boundary_moduli
    }

    /// Returns required behavior outside the range.
    #[must_use]
    pub const fn invalid_behavior(&self) -> &InvalidInputBehavior {
        &self.invalid_behavior
    }
}

impl TryFrom<ShapeSymbolContractWire> for ShapeSymbolContractV1 {
    type Error = DomainContractError;

    fn try_from(wire: ShapeSymbolContractWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(ShapeSymbolContractInput {
            name: wire.name,
            valid_range: wire.valid_range,
            source: wire.source,
            boundary_moduli: wire.boundary_moduli,
            invalid_behavior: wire.invalid_behavior,
        })
    }
}

/// Requested semantic strength of the product-domain body.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticClaimKind {
    /// Exact result equality.
    Exact,
    /// Numerical comparison requiring an admitted allowance.
    Numerical,
    /// Membership in a declared allowed-result set.
    AllowedResultSet,
    /// Non-semantic invocation/status/shape observations only.
    Implicit,
}

/// Content domain for requested operator semantics.
pub enum RequestedSemanticsArtifact {}

impl ContentType for RequestedSemanticsArtifact {
    const DOMAIN: &'static str = "migration.requested-semantics.v1";
}

/// Content domain for one explicit migration-domain exclusion.
pub enum MigrationDomainExclusionArtifact {}

impl ContentType for MigrationDomainExclusionArtifact {
    const DOMAIN: &'static str = "migration.domain-exclusion.v1";
}

/// Constructor input for the complete operator-domain body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationDomainContractInput {
    /// Source ABI entry point.
    pub source_entry_point: EntryPointName,
    /// Buffer arguments in strict ABI-index order.
    pub buffers: Vec<BufferContractV1>,
    /// Scalar arguments in strict ABI-index order.
    pub scalar_parameters: Vec<ScalarParameterContractV1>,
    /// Logical symbols in strict name order.
    pub shape_symbols: Vec<ShapeSymbolContractV1>,
    /// Complete pairwise aliasing declarations in strict pair order.
    pub buffer_aliasing: Vec<BufferAliasingContractV1>,
    /// Requested semantics artifact.
    pub requested_semantics: ContentId<RequestedSemanticsArtifact>,
    /// Strength requested by the caller's domain body.
    pub semantic_claim: SemanticClaimKind,
    /// Explicit exclusions in strict identity order.
    pub exclusions: Vec<ContentId<MigrationDomainExclusionArtifact>>,
}

/// Strongly typed caller-domain body for operator migration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MigrationDomainContractWire")]
pub struct MigrationDomainContractV1 {
    schema_version: MigrationDomainSchemaV1,
    source_entry_point: EntryPointName,
    buffers: Vec<BufferContractV1>,
    scalar_parameters: Vec<ScalarParameterContractV1>,
    shape_symbols: Vec<ShapeSymbolContractV1>,
    buffer_aliasing: Vec<BufferAliasingContractV1>,
    requested_semantics: ContentId<RequestedSemanticsArtifact>,
    semantic_claim: SemanticClaimKind,
    exclusions: Vec<ContentId<MigrationDomainExclusionArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationDomainContractWire {
    schema_version: MigrationDomainSchemaV1,
    source_entry_point: EntryPointName,
    buffers: Vec<BufferContractV1>,
    scalar_parameters: Vec<ScalarParameterContractV1>,
    shape_symbols: Vec<ShapeSymbolContractV1>,
    buffer_aliasing: Vec<BufferAliasingContractV1>,
    requested_semantics: ContentId<RequestedSemanticsArtifact>,
    semantic_claim: SemanticClaimKind,
    exclusions: Vec<ContentId<MigrationDomainExclusionArtifact>>,
}

impl MigrationDomainContractV1 {
    /// Validates a complete caller-domain body.
    ///
    /// # Errors
    ///
    /// Rejects ambiguous ABI order/names, incomplete pairwise aliasing, dangling exclusion edges,
    /// non-canonical collections, unknown symbols, source/range disagreement, missing input/output
    /// roles, and implicit merging of shape authorities.
    pub fn new(input: MigrationDomainContractInput) -> Result<Self, DomainContractError> {
        validate_argument_order(&input.buffers, &input.scalar_parameters)?;
        if input.buffers.is_empty() {
            return Err(DomainContractError::EmptySet { field: "buffers" });
        }
        if !input
            .buffers
            .iter()
            .any(|buffer| matches!(buffer.role(), BufferRole::Input | BufferRole::InputOutput))
        {
            return Err(DomainContractError::MissingInputBuffer);
        }
        if !input
            .buffers
            .iter()
            .any(|buffer| matches!(buffer.role(), BufferRole::Output | BufferRole::InputOutput))
        {
            return Err(DomainContractError::MissingOutputBuffer);
        }
        if input
            .shape_symbols
            .windows(2)
            .any(|pair| pair[0].name() >= pair[1].name())
        {
            return Err(DomainContractError::NonCanonicalSet {
                field: "shape symbols",
            });
        }
        validate_content_ids(&input.exclusions, "domain exclusions")?;
        validate_input_value_exclusions(&input.buffers, &input.exclusions)?;
        validate_buffer_aliasing(&input.buffers, &input.buffer_aliasing)?;
        validate_memory_surface_exclusions(
            &input.buffers,
            &input.buffer_aliasing,
            &input.exclusions,
        )?;
        validate_shape_graph(
            &input.buffers,
            &input.scalar_parameters,
            &input.shape_symbols,
        )?;
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            source_entry_point: input.source_entry_point,
            buffers: input.buffers,
            scalar_parameters: input.scalar_parameters,
            shape_symbols: input.shape_symbols,
            buffer_aliasing: input.buffer_aliasing,
            requested_semantics: input.requested_semantics,
            semantic_claim: input.semantic_claim,
            exclusions: input.exclusions,
        })
    }

    /// Returns the source ABI entry point.
    #[must_use]
    pub const fn source_entry_point(&self) -> &EntryPointName {
        &self.source_entry_point
    }

    /// Returns buffers in ABI order.
    #[must_use]
    pub fn buffers(&self) -> &[BufferContractV1] {
        &self.buffers
    }

    /// Returns scalar parameters in ABI order.
    #[must_use]
    pub fn scalar_parameters(&self) -> &[ScalarParameterContractV1] {
        &self.scalar_parameters
    }

    /// Returns logical shape symbols in canonical name order.
    #[must_use]
    pub fn shape_symbols(&self) -> &[ShapeSymbolContractV1] {
        &self.shape_symbols
    }

    /// Returns complete pairwise aliasing declarations in strict pair order.
    #[must_use]
    pub fn buffer_aliasing(&self) -> &[BufferAliasingContractV1] {
        &self.buffer_aliasing
    }

    /// Returns requested semantics.
    #[must_use]
    pub const fn requested_semantics(&self) -> ContentId<RequestedSemanticsArtifact> {
        self.requested_semantics
    }

    /// Returns the requested semantic claim kind.
    #[must_use]
    pub const fn semantic_claim(&self) -> SemanticClaimKind {
        self.semantic_claim
    }

    /// Returns explicit domain exclusions.
    #[must_use]
    pub fn exclusions(&self) -> &[ContentId<MigrationDomainExclusionArtifact>] {
        &self.exclusions
    }
}

impl TryFrom<MigrationDomainContractWire> for MigrationDomainContractV1 {
    type Error = DomainContractError;

    fn try_from(wire: MigrationDomainContractWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(MigrationDomainContractInput {
            source_entry_point: wire.source_entry_point,
            buffers: wire.buffers,
            scalar_parameters: wire.scalar_parameters,
            shape_symbols: wire.shape_symbols,
            buffer_aliasing: wire.buffer_aliasing,
            requested_semantics: wire.requested_semantics,
            semantic_claim: wire.semantic_claim,
            exclusions: wire.exclusions,
        })
    }
}

trait AbiArgument {
    fn argument_index(&self) -> ArgumentIndex;
    fn name(&self) -> &str;
}

impl AbiArgument for BufferContractV1 {
    fn argument_index(&self) -> ArgumentIndex {
        self.argument_index()
    }

    fn name(&self) -> &str {
        self.name().as_str()
    }
}

impl AbiArgument for ScalarParameterContractV1 {
    fn argument_index(&self) -> ArgumentIndex {
        self.argument_index()
    }

    fn name(&self) -> &str {
        self.name().as_str()
    }
}

fn validate_argument_order(
    buffers: &[BufferContractV1],
    parameters: &[ScalarParameterContractV1],
) -> Result<(), DomainContractError> {
    validate_abi_slice(buffers, "buffers")?;
    validate_abi_slice(parameters, "scalar parameters")?;
    let mut indices = BTreeSet::new();
    let mut names = BTreeSet::new();
    for argument in buffers
        .iter()
        .map(|value| value as &dyn AbiArgument)
        .chain(parameters.iter().map(|value| value as &dyn AbiArgument))
    {
        if !indices.insert(argument.argument_index()) {
            return Err(DomainContractError::DuplicateArgumentIndex {
                index: argument.argument_index().get(),
            });
        }
        if !names.insert(argument.name()) {
            return Err(DomainContractError::DuplicateArgumentName {
                name: argument.name().to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_abi_slice<T: AbiArgument>(
    values: &[T],
    field: &'static str,
) -> Result<(), DomainContractError> {
    if values
        .windows(2)
        .any(|pair| pair[0].argument_index() >= pair[1].argument_index())
    {
        return Err(DomainContractError::NonCanonicalSet { field });
    }
    Ok(())
}

fn validate_shape_graph(
    buffers: &[BufferContractV1],
    parameters: &[ScalarParameterContractV1],
    symbols: &[ShapeSymbolContractV1],
) -> Result<(), DomainContractError> {
    for buffer in buffers {
        for dimension in buffer.shape() {
            if let DimensionSpec::Symbol { symbol } = dimension {
                if !symbols.iter().any(|candidate| candidate.name() == symbol) {
                    return Err(DomainContractError::UnknownShapeSymbol {
                        buffer: buffer.name().to_string(),
                        symbol: symbol.to_string(),
                    });
                }
            }
        }
    }
    for symbol in symbols {
        match symbol.source() {
            ShapeSymbolSource::ScalarParameter { parameter } => {
                let Some(source) = parameters
                    .iter()
                    .find(|candidate| candidate.name() == parameter)
                else {
                    return invalid_shape_source(symbol, "scalar parameter is not declared");
                };
                if source.role() != ScalarParameterRole::ShapeExtent {
                    return invalid_shape_source(symbol, "scalar source is not shape-extent role");
                }
                let range = source.valid_range();
                let Ok(minimum) = u64::try_from(range.minimum().get()) else {
                    return invalid_shape_source(symbol, "scalar range contains a negative extent");
                };
                let Ok(maximum) = u64::try_from(range.maximum().get()) else {
                    return invalid_shape_source(symbol, "scalar range cannot represent an extent");
                };
                if symbol.valid_range().minimum().get() != minimum
                    || symbol.valid_range().maximum().get() != maximum
                {
                    return invalid_shape_source(symbol, "scalar and logical extent ranges differ");
                }
            }
            ShapeSymbolSource::BufferDimension { buffer, axis } => {
                let Some(source) = buffers.iter().find(|candidate| candidate.name() == buffer)
                else {
                    return invalid_shape_source(symbol, "buffer source is not declared");
                };
                let Some(dimension) = source.shape().get(usize::from(axis.get())) else {
                    return invalid_shape_source(symbol, "buffer dimension axis is out of range");
                };
                if !matches!(dimension, DimensionSpec::Symbol { symbol: bound } if bound == symbol.name())
                {
                    return invalid_shape_source(
                        symbol,
                        "buffer dimension is not bound to this symbol",
                    );
                }
            }
        }
    }
    for parameter in parameters
        .iter()
        .filter(|value| value.role() == ScalarParameterRole::ShapeExtent)
    {
        let binding_count = symbols
            .iter()
            .filter(|symbol| {
                matches!(
                    symbol.source(),
                    ShapeSymbolSource::ScalarParameter { parameter: source }
                        if source == parameter.name()
                )
            })
            .count();
        if binding_count != 1 {
            return Err(DomainContractError::InvalidShapeSource {
                symbol: parameter.name().to_string(),
                reason: "shape-extent parameter must bind exactly one symbol",
            });
        }
    }
    Ok(())
}

fn invalid_shape_source<T>(
    symbol: &ShapeSymbolContractV1,
    reason: &'static str,
) -> Result<T, DomainContractError> {
    Err(DomainContractError::InvalidShapeSource {
        symbol: symbol.name().to_string(),
        reason,
    })
}

fn validate_content_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), DomainContractError> {
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(DomainContractError::NonCanonicalSet { field });
    }
    Ok(())
}

fn validate_input_value_exclusions(
    buffers: &[BufferContractV1],
    exclusions: &[ContentId<MigrationDomainExclusionArtifact>],
) -> Result<(), DomainContractError> {
    for buffer in buffers {
        let Some(value_domain) = buffer.input_value_domain() else {
            continue;
        };
        if value_domain
            .referenced_exclusions()
            .any(|exclusion| !exclusions.contains(exclusion))
        {
            return Err(DomainContractError::UnlistedInputValueExclusion {
                buffer: buffer.name().to_string(),
            });
        }
    }
    Ok(())
}

fn validate_buffer_aliasing(
    buffers: &[BufferContractV1],
    aliasing: &[BufferAliasingContractV1],
) -> Result<(), DomainContractError> {
    for contract in aliasing {
        for name in [contract.pair().first(), contract.pair().second()] {
            if !buffers.iter().any(|buffer| buffer.name() == name) {
                return Err(DomainContractError::UnknownAliasingBuffer {
                    buffer: name.to_string(),
                });
            }
        }
    }
    if aliasing
        .windows(2)
        .any(|pair| pair[0].pair() >= pair[1].pair())
    {
        return Err(DomainContractError::NonCanonicalSet {
            field: "buffer aliasing contracts",
        });
    }

    let mut expected = Vec::new();
    for (index, left) in buffers.iter().enumerate() {
        for right in &buffers[index + 1..] {
            let (first, second) = if left.name() < right.name() {
                (left.name().clone(), right.name().clone())
            } else {
                (right.name().clone(), left.name().clone())
            };
            expected.push(BufferPairV1::new(first, second)?);
        }
    }
    expected.sort();
    let actual: Vec<_> = aliasing
        .iter()
        .map(|contract| contract.pair().clone())
        .collect();
    if actual != expected {
        return Err(DomainContractError::IncompleteAliasingContracts);
    }
    Ok(())
}

fn validate_memory_surface_exclusions(
    buffers: &[BufferContractV1],
    aliasing: &[BufferAliasingContractV1],
    exclusions: &[ContentId<MigrationDomainExclusionArtifact>],
) -> Result<(), DomainContractError> {
    let missing_buffer_exclusion = buffers.iter().any(|buffer| {
        buffer
            .memory()
            .referenced_exclusions()
            .any(|exclusion| !exclusions.contains(exclusion))
    });
    let missing_aliasing_exclusion = aliasing.iter().any(|contract| {
        contract
            .referenced_exclusions()
            .any(|exclusion| !exclusions.contains(exclusion))
    });
    if missing_buffer_exclusion || missing_aliasing_exclusion {
        return Err(DomainContractError::UnlistedMemorySurfaceExclusion);
    }
    Ok(())
}

/// Trusted derivation policy frozen into the mandatory case set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MandatoryCaseDerivationPolicy {
    /// V1 boundary/min/max/zero/one/interior/tile-tail policy.
    BoundaryV1,
}

/// Why one shape extent is mandatory.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "modulus", rename_all = "kebab-case")]
pub enum ShapeBoundaryObligation {
    /// Inclusive valid minimum.
    ValidMinimum,
    /// Inclusive valid maximum.
    ValidMaximum,
    /// Empty/zero extent where valid.
    Zero,
    /// Singleton extent where valid.
    One,
    /// First valid value after the minimum where distinct.
    LowerInterior,
    /// Last valid value before the maximum where distinct.
    UpperInterior,
    /// Valid tail immediately below a modulus boundary.
    ModulusBelow(ExtentModulus),
    /// Valid value exactly at a modulus boundary.
    ModulusAt(ExtentModulus),
    /// Valid tail immediately above a modulus boundary.
    ModulusAbove(ExtentModulus),
    /// Representable value immediately below the valid range.
    InvalidBelowMinimum,
    /// Representable value immediately above the valid range.
    InvalidAboveMaximum,
}

/// Why one non-shape scalar value is mandatory.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScalarBoundaryObligation {
    /// Inclusive valid minimum.
    ValidMinimum,
    /// Inclusive valid maximum.
    ValidMaximum,
    /// Zero where valid.
    Zero,
    /// One where valid.
    One,
    /// First valid value after the minimum where distinct.
    LowerInterior,
    /// Last valid value before the maximum where distinct.
    UpperInterior,
    /// Representable value immediately below the valid range.
    InvalidBelowMinimum,
    /// Representable value immediately above the valid range.
    InvalidAboveMaximum,
}

/// Complete logical shape assignment for one case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeAssignment {
    symbol: ShapeSymbolName,
    value: ExtentValue,
}

impl ShapeAssignment {
    const fn new(symbol: ShapeSymbolName, value: ExtentValue) -> Self {
        Self { symbol, value }
    }

    /// Returns the logical symbol.
    #[must_use]
    pub const fn symbol(&self) -> &ShapeSymbolName {
        &self.symbol
    }

    /// Returns the assigned extent.
    #[must_use]
    pub const fn value(&self) -> ExtentValue {
        self.value
    }
}

/// Complete scalar ABI assignment for one case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarAssignment {
    parameter: ScalarParameterName,
    value: IntegerValue,
}

impl ScalarAssignment {
    const fn new(parameter: ScalarParameterName, value: IntegerValue) -> Self {
        Self { parameter, value }
    }

    /// Returns the scalar parameter.
    #[must_use]
    pub const fn parameter(&self) -> &ScalarParameterName {
        &self.parameter
    }

    /// Returns the assigned integer.
    #[must_use]
    pub const fn value(&self) -> IntegerValue {
        self.value
    }
}

/// Primary independently varied dimension of one mandatory case.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CaseTarget {
    /// Shape extent target.
    ShapeSymbol {
        /// Logical symbol.
        symbol: ShapeSymbolName,
        /// Varied extent.
        value: ExtentValue,
    },
    /// Non-shape scalar target.
    ScalarParameter {
        /// Scalar argument.
        parameter: ScalarParameterName,
        /// Varied value.
        value: IntegerValue,
    },
}

/// Expected operator behavior for one derived case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CaseExpectedOutcome {
    /// Input is within the declared domain.
    Success,
    /// Input is outside the range and carries explicit required behavior.
    Invalid {
        /// Caller-declared behavior, not inferred by the verifier.
        behavior: InvalidInputBehavior,
    },
}

/// One complete mandatory boundary case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MigrationDomainCaseWire")]
pub struct MigrationDomainCaseV1 {
    schema_version: MigrationDomainSchemaV1,
    target: CaseTarget,
    shape_assignments: Vec<ShapeAssignment>,
    scalar_assignments: Vec<ScalarAssignment>,
    shape_obligations: Vec<ShapeBoundaryObligation>,
    scalar_obligations: Vec<ScalarBoundaryObligation>,
    expected_outcome: CaseExpectedOutcome,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationDomainCaseWire {
    schema_version: MigrationDomainSchemaV1,
    target: CaseTarget,
    shape_assignments: Vec<ShapeAssignment>,
    scalar_assignments: Vec<ScalarAssignment>,
    shape_obligations: Vec<ShapeBoundaryObligation>,
    scalar_obligations: Vec<ScalarBoundaryObligation>,
    expected_outcome: CaseExpectedOutcome,
}

impl MigrationDomainCaseV1 {
    fn new(
        target: CaseTarget,
        shape_assignments: Vec<ShapeAssignment>,
        scalar_assignments: Vec<ScalarAssignment>,
        shape_obligations: Vec<ShapeBoundaryObligation>,
        scalar_obligations: Vec<ScalarBoundaryObligation>,
        expected_outcome: CaseExpectedOutcome,
    ) -> Result<Self, DomainContractError> {
        validate_assignments(&shape_assignments, &scalar_assignments)?;
        let invalid = match &target {
            CaseTarget::ShapeSymbol { symbol, value } => {
                if shape_obligations.is_empty() || !scalar_obligations.is_empty() {
                    return invalid_case("shape target requires only shape obligations");
                }
                validate_obligation_order(&shape_obligations)?;
                if !shape_assignments
                    .iter()
                    .any(|assignment| assignment.symbol() == symbol && assignment.value() == *value)
                {
                    return invalid_case("shape target differs from complete assignments");
                }
                shape_obligations.iter().any(|obligation| {
                    matches!(
                        obligation,
                        ShapeBoundaryObligation::InvalidBelowMinimum
                            | ShapeBoundaryObligation::InvalidAboveMaximum
                    )
                })
            }
            CaseTarget::ScalarParameter { parameter, value } => {
                if scalar_obligations.is_empty() || !shape_obligations.is_empty() {
                    return invalid_case("scalar target requires only scalar obligations");
                }
                validate_obligation_order(&scalar_obligations)?;
                if !scalar_assignments.iter().any(|assignment| {
                    assignment.parameter() == parameter && assignment.value() == *value
                }) {
                    return invalid_case("scalar target differs from complete assignments");
                }
                scalar_obligations.iter().any(|obligation| {
                    matches!(
                        obligation,
                        ScalarBoundaryObligation::InvalidBelowMinimum
                            | ScalarBoundaryObligation::InvalidAboveMaximum
                    )
                })
            }
        };
        if invalid != matches!(expected_outcome, CaseExpectedOutcome::Invalid { .. }) {
            return invalid_case("boundary obligation and expected outcome disagree");
        }
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            target,
            shape_assignments,
            scalar_assignments,
            shape_obligations,
            scalar_obligations,
            expected_outcome,
        })
    }

    /// Returns the independently varied target.
    #[must_use]
    pub const fn target(&self) -> &CaseTarget {
        &self.target
    }

    /// Returns complete logical shape assignments.
    #[must_use]
    pub fn shape_assignments(&self) -> &[ShapeAssignment] {
        &self.shape_assignments
    }

    /// Returns complete scalar ABI assignments.
    #[must_use]
    pub fn scalar_assignments(&self) -> &[ScalarAssignment] {
        &self.scalar_assignments
    }

    /// Returns shape obligations when the target is a shape symbol.
    #[must_use]
    pub fn shape_obligations(&self) -> &[ShapeBoundaryObligation] {
        &self.shape_obligations
    }

    /// Returns scalar obligations when the target is a scalar control.
    #[must_use]
    pub fn scalar_obligations(&self) -> &[ScalarBoundaryObligation] {
        &self.scalar_obligations
    }

    /// Returns caller-declared expected behavior.
    #[must_use]
    pub const fn expected_outcome(&self) -> &CaseExpectedOutcome {
        &self.expected_outcome
    }
}

impl TryFrom<MigrationDomainCaseWire> for MigrationDomainCaseV1 {
    type Error = DomainContractError;

    fn try_from(wire: MigrationDomainCaseWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.target,
            wire.shape_assignments,
            wire.scalar_assignments,
            wire.shape_obligations,
            wire.scalar_obligations,
            wire.expected_outcome,
        )
    }
}

fn validate_assignments(
    shapes: &[ShapeAssignment],
    scalars: &[ScalarAssignment],
) -> Result<(), DomainContractError> {
    if shapes
        .windows(2)
        .any(|pair| pair[0].symbol() >= pair[1].symbol())
    {
        return invalid_case("shape assignments are not in strict symbol order");
    }
    if scalars
        .windows(2)
        .any(|pair| pair[0].parameter() >= pair[1].parameter())
    {
        return invalid_case("scalar assignments are not in strict parameter order");
    }
    Ok(())
}

fn validate_obligation_order<T: Ord>(values: &[T]) -> Result<(), DomainContractError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return invalid_case("case obligations are not in strict canonical order");
    }
    Ok(())
}

fn invalid_case<T>(reason: &'static str) -> Result<T, DomainContractError> {
    Err(DomainContractError::InvalidDerivedCase { reason })
}

/// Content domain for one canonical mandatory migration-domain case.
pub enum MigrationDomainCaseArtifact {}

impl ContentType for MigrationDomainCaseArtifact {
    const DOMAIN: &'static str = "migration.mandatory-domain-case.v1";
}

/// Complete deterministic mandatory case set tied to one exact caller-domain body.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MigrationMandatoryCasesWire")]
pub struct MigrationMandatoryCasesV1 {
    schema_version: MigrationDomainSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: MandatoryCaseDerivationPolicy,
    cases: Vec<MigrationDomainCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationMandatoryCasesWire {
    schema_version: MigrationDomainSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: MandatoryCaseDerivationPolicy,
    cases: Vec<MigrationDomainCaseV1>,
}

impl MigrationMandatoryCasesV1 {
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        derivation_policy: MandatoryCaseDerivationPolicy,
        cases: Vec<MigrationDomainCaseV1>,
    ) -> Result<Self, DomainContractError> {
        if cases.is_empty() {
            return Err(DomainContractError::EmptySet {
                field: "mandatory domain cases",
            });
        }
        if cases
            .windows(2)
            .any(|pair| pair[0].target() >= pair[1].target())
        {
            return Err(DomainContractError::NonCanonicalSet {
                field: "mandatory domain cases",
            });
        }
        Ok(Self {
            schema_version: MigrationDomainSchemaV1,
            domain,
            derivation_policy,
            cases,
        })
    }

    /// Returns the exact domain body from which cases were derived.
    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    /// Returns the trusted derivation policy.
    #[must_use]
    pub const fn derivation_policy(&self) -> MandatoryCaseDerivationPolicy {
        self.derivation_policy
    }

    /// Returns complete cases in strict target order.
    #[must_use]
    pub fn cases(&self) -> &[MigrationDomainCaseV1] {
        &self.cases
    }
}

impl TryFrom<MigrationMandatoryCasesWire> for MigrationMandatoryCasesV1 {
    type Error = DomainContractError;

    fn try_from(wire: MigrationMandatoryCasesWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.domain, wire.derivation_policy, wire.cases)
    }
}

/// Content domain for the complete canonical mandatory case set.
pub enum MigrationMandatoryCasesArtifact {}

impl ContentType for MigrationMandatoryCasesArtifact {
    const DOMAIN: &'static str = "migration.mandatory-domain-cases.v1";
}

/// Derives mandatory min/max/zero/one/interior/invalid and tile-tail cases from trusted code.
///
/// Every case varies exactly one logical dimension while carrying complete nominal assignments for
/// the remaining dimensions. Shape-bound scalar ABI parameters are updated with their symbols.
/// No model-authored case list can replace this derivation.
///
/// # Errors
///
/// Returns [`DomainContractError`] if canonical domain encoding/identity derivation fails or an
/// internal derived case contradicts the validated contract.
pub fn derive_mandatory_base_cases(
    contract: &MigrationDomainContractV1,
) -> Result<MigrationMandatoryCasesV1, DomainContractError> {
    let domain_bytes =
        cairn_codec::to_vec(contract).map_err(|error| DomainContractError::Codec {
            message: error.to_string(),
        })?;
    let domain = ContentId::<CallerDomainBodyArtifact>::derive(&domain_bytes).map_err(|error| {
        DomainContractError::Codec {
            message: error.to_string(),
        }
    })?;
    let (base_shapes, base_scalars) = base_assignments(contract)?;
    let mut cases = derive_shape_cases(contract, &base_shapes, &base_scalars)?;
    cases.extend(derive_scalar_cases(contract, &base_shapes, &base_scalars)?);
    cases.sort_by(|left, right| left.target().cmp(right.target()));
    MigrationMandatoryCasesV1::new(domain, MandatoryCaseDerivationPolicy::BoundaryV1, cases)
}

fn base_assignments(
    contract: &MigrationDomainContractV1,
) -> Result<(Vec<ShapeAssignment>, Vec<ScalarAssignment>), DomainContractError> {
    let shapes: Vec<_> = contract
        .shape_symbols()
        .iter()
        .map(|symbol| {
            ShapeAssignment::new(symbol.name().clone(), nominal_extent(symbol.valid_range()))
        })
        .collect();
    let mut scalars = Vec::with_capacity(contract.scalar_parameters().len());
    for parameter in contract.scalar_parameters() {
        let value = if parameter.role() == ScalarParameterRole::Control {
            nominal_integer(parameter.valid_range())
        } else {
            let symbol = contract
                .shape_symbols()
                .iter()
                .find(|candidate| {
                    matches!(
                        candidate.source(),
                        ShapeSymbolSource::ScalarParameter { parameter: source }
                            if source == parameter.name()
                    )
                })
                .ok_or_else(|| {
                    derived_error("shape parameter lost its validated symbol binding")
                })?;
            IntegerValue::new(
                i64::try_from(nominal_extent(symbol.valid_range()).get())
                    .map_err(|_| derived_error("scalar-backed extent exceeds signed ABI range"))?,
            )
        };
        scalars.push(ScalarAssignment::new(parameter.name().clone(), value));
    }
    scalars.sort_by(|left, right| left.parameter().cmp(right.parameter()));
    Ok((shapes, scalars))
}

fn derive_shape_cases(
    contract: &MigrationDomainContractV1,
    base_shapes: &[ShapeAssignment],
    base_scalars: &[ScalarAssignment],
) -> Result<Vec<MigrationDomainCaseV1>, DomainContractError> {
    let mut cases = Vec::new();
    for symbol in contract.shape_symbols() {
        for (value, obligations) in shape_candidates(symbol) {
            let mut shapes = base_shapes.to_vec();
            shapes
                .iter_mut()
                .find(|assignment| assignment.symbol() == symbol.name())
                .ok_or_else(|| derived_error("shape baseline omitted a declared symbol"))?
                .value = value;
            let mut scalars = base_scalars.to_vec();
            update_shape_scalar(symbol, value, &mut scalars)?;
            cases.push(MigrationDomainCaseV1::new(
                CaseTarget::ShapeSymbol {
                    symbol: symbol.name().clone(),
                    value,
                },
                shapes,
                scalars,
                obligations.into_iter().collect(),
                Vec::new(),
                expected_shape_outcome(symbol, value),
            )?);
        }
    }
    Ok(cases)
}

fn update_shape_scalar(
    symbol: &ShapeSymbolContractV1,
    value: ExtentValue,
    scalars: &mut [ScalarAssignment],
) -> Result<(), DomainContractError> {
    if let ShapeSymbolSource::ScalarParameter { parameter } = symbol.source() {
        scalars
            .iter_mut()
            .find(|assignment| assignment.parameter() == parameter)
            .ok_or_else(|| derived_error("scalar baseline omitted a shape parameter"))?
            .value = IntegerValue::new(
            i64::try_from(value.get())
                .map_err(|_| derived_error("derived extent exceeds signed ABI range"))?,
        );
    }
    Ok(())
}

fn expected_shape_outcome(
    symbol: &ShapeSymbolContractV1,
    value: ExtentValue,
) -> CaseExpectedOutcome {
    if symbol.valid_range().contains(value) {
        CaseExpectedOutcome::Success
    } else {
        CaseExpectedOutcome::Invalid {
            behavior: symbol.invalid_behavior().clone(),
        }
    }
}

fn derive_scalar_cases(
    contract: &MigrationDomainContractV1,
    base_shapes: &[ShapeAssignment],
    base_scalars: &[ScalarAssignment],
) -> Result<Vec<MigrationDomainCaseV1>, DomainContractError> {
    let mut cases = Vec::new();
    for parameter in contract
        .scalar_parameters()
        .iter()
        .filter(|value| value.role() == ScalarParameterRole::Control)
    {
        for (value, obligations) in scalar_candidates(parameter.valid_range()) {
            let mut scalars = base_scalars.to_vec();
            scalars
                .iter_mut()
                .find(|assignment| assignment.parameter() == parameter.name())
                .ok_or_else(|| derived_error("scalar baseline omitted a control parameter"))?
                .value = value;
            let expected_outcome = if parameter.valid_range().contains(value) {
                CaseExpectedOutcome::Success
            } else {
                CaseExpectedOutcome::Invalid {
                    behavior: parameter.invalid_behavior().clone(),
                }
            };
            cases.push(MigrationDomainCaseV1::new(
                CaseTarget::ScalarParameter {
                    parameter: parameter.name().clone(),
                    value,
                },
                base_shapes.to_vec(),
                scalars,
                Vec::new(),
                obligations.into_iter().collect(),
                expected_outcome,
            )?);
        }
    }
    Ok(cases)
}

const fn derived_error(reason: &'static str) -> DomainContractError {
    DomainContractError::InvalidDerivedCase { reason }
}

fn nominal_extent(range: InclusiveExtentRange) -> ExtentValue {
    let one = ExtentValue::new(1);
    if range.contains(one) {
        one
    } else {
        range.minimum()
    }
}

fn nominal_integer(range: InclusiveIntegerRange) -> IntegerValue {
    for candidate in [IntegerValue::new(0), IntegerValue::new(1)] {
        if range.contains(candidate) {
            return candidate;
        }
    }
    range.minimum()
}

fn shape_candidates(
    symbol: &ShapeSymbolContractV1,
) -> BTreeMap<ExtentValue, BTreeSet<ShapeBoundaryObligation>> {
    let range = symbol.valid_range();
    let mut candidates = BTreeMap::<_, BTreeSet<_>>::new();
    add_shape(
        &mut candidates,
        range.minimum(),
        ShapeBoundaryObligation::ValidMinimum,
    );
    add_shape(
        &mut candidates,
        range.maximum(),
        ShapeBoundaryObligation::ValidMaximum,
    );
    for (value, obligation) in [
        (ExtentValue::new(0), ShapeBoundaryObligation::Zero),
        (ExtentValue::new(1), ShapeBoundaryObligation::One),
    ] {
        if range.contains(value) {
            add_shape(&mut candidates, value, obligation);
        }
    }
    if let Some(value) = range.minimum().get().checked_add(1).map(ExtentValue::new) {
        if range.contains(value) {
            add_shape(
                &mut candidates,
                value,
                ShapeBoundaryObligation::LowerInterior,
            );
        }
    }
    if let Some(value) = range.maximum().get().checked_sub(1).map(ExtentValue::new) {
        if range.contains(value) {
            add_shape(
                &mut candidates,
                value,
                ShapeBoundaryObligation::UpperInterior,
            );
        }
    }
    for modulus in symbol.boundary_moduli() {
        let first = first_positive_multiple(range, *modulus);
        let last = last_multiple(range, *modulus);
        for at in [first, last].into_iter().flatten().collect::<BTreeSet<_>>() {
            add_modulus_neighbors(&mut candidates, range, at, *modulus);
        }
    }
    if let Some(value) = range.minimum().get().checked_sub(1).map(ExtentValue::new) {
        add_shape(
            &mut candidates,
            value,
            ShapeBoundaryObligation::InvalidBelowMinimum,
        );
    }
    if let Some(value) = range.maximum().get().checked_add(1).map(ExtentValue::new) {
        add_shape(
            &mut candidates,
            value,
            ShapeBoundaryObligation::InvalidAboveMaximum,
        );
    }
    candidates
}

fn first_positive_multiple(
    range: InclusiveExtentRange,
    modulus: ExtentModulus,
) -> Option<ExtentValue> {
    let start = range.minimum().get().max(1);
    let quotient = start.div_ceil(modulus.get());
    quotient
        .checked_mul(modulus.get())
        .map(ExtentValue::new)
        .filter(|value| range.contains(*value))
}

fn last_multiple(range: InclusiveExtentRange, modulus: ExtentModulus) -> Option<ExtentValue> {
    let value = (range.maximum().get() / modulus.get()).checked_mul(modulus.get())?;
    let value = ExtentValue::new(value);
    (value.get() > 0 && range.contains(value)).then_some(value)
}

fn add_modulus_neighbors(
    candidates: &mut BTreeMap<ExtentValue, BTreeSet<ShapeBoundaryObligation>>,
    range: InclusiveExtentRange,
    at: ExtentValue,
    modulus: ExtentModulus,
) {
    if let Some(below) = at.get().checked_sub(1).map(ExtentValue::new) {
        if range.contains(below) {
            add_shape(
                candidates,
                below,
                ShapeBoundaryObligation::ModulusBelow(modulus),
            );
        }
    }
    add_shape(candidates, at, ShapeBoundaryObligation::ModulusAt(modulus));
    if let Some(above) = at.get().checked_add(1).map(ExtentValue::new) {
        if range.contains(above) {
            add_shape(
                candidates,
                above,
                ShapeBoundaryObligation::ModulusAbove(modulus),
            );
        }
    }
}

fn add_shape(
    candidates: &mut BTreeMap<ExtentValue, BTreeSet<ShapeBoundaryObligation>>,
    value: ExtentValue,
    obligation: ShapeBoundaryObligation,
) {
    candidates.entry(value).or_default().insert(obligation);
}

fn scalar_candidates(
    range: InclusiveIntegerRange,
) -> BTreeMap<IntegerValue, BTreeSet<ScalarBoundaryObligation>> {
    let mut candidates = BTreeMap::<_, BTreeSet<_>>::new();
    add_scalar(
        &mut candidates,
        range.minimum(),
        ScalarBoundaryObligation::ValidMinimum,
    );
    add_scalar(
        &mut candidates,
        range.maximum(),
        ScalarBoundaryObligation::ValidMaximum,
    );
    for (value, obligation) in [
        (IntegerValue::new(0), ScalarBoundaryObligation::Zero),
        (IntegerValue::new(1), ScalarBoundaryObligation::One),
    ] {
        if range.contains(value) {
            add_scalar(&mut candidates, value, obligation);
        }
    }
    if let Some(value) = range.minimum().get().checked_add(1).map(IntegerValue::new) {
        if range.contains(value) {
            add_scalar(
                &mut candidates,
                value,
                ScalarBoundaryObligation::LowerInterior,
            );
        }
    }
    if let Some(value) = range.maximum().get().checked_sub(1).map(IntegerValue::new) {
        if range.contains(value) {
            add_scalar(
                &mut candidates,
                value,
                ScalarBoundaryObligation::UpperInterior,
            );
        }
    }
    if let Some(value) = range.minimum().get().checked_sub(1).map(IntegerValue::new) {
        add_scalar(
            &mut candidates,
            value,
            ScalarBoundaryObligation::InvalidBelowMinimum,
        );
    }
    if let Some(value) = range.maximum().get().checked_add(1).map(IntegerValue::new) {
        add_scalar(
            &mut candidates,
            value,
            ScalarBoundaryObligation::InvalidAboveMaximum,
        );
    }
    candidates
}

fn add_scalar(
    candidates: &mut BTreeMap<IntegerValue, BTreeSet<ScalarBoundaryObligation>>,
    value: IntegerValue,
    obligation: ScalarBoundaryObligation,
) {
    candidates.entry(value).or_default().insert(obligation);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cairn_protocol::{ContentId, ContentType};
    use cairn_verification::CallerDomainBodyArtifact;

    use super::{
        ArgumentIndex, BufferAccessV1, BufferContractInput, BufferContractV1, BufferName,
        CaseExpectedOutcome, CaseTarget, DataType, DimensionSpec, DomainContractError,
        EntryPointName, ExtentModulus, ExtentValue, InclusiveExtentRange, InclusiveIntegerRange,
        IntegerValue, InvalidInputBehavior, MigrationDomainContractInput,
        MigrationDomainContractV1, MigrationDomainExclusionArtifact,
        MigrationMandatoryCasesArtifact, MigrationMandatoryCasesV1, RequestedSemanticsArtifact,
        ScalarParameterContractInput, ScalarParameterContractV1, ScalarParameterName,
        ScalarParameterRole, SemanticClaimKind, ShapeBoundaryObligation, ShapeSymbolContractInput,
        ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource, StatusCode,
        derive_mandatory_base_cases,
    };
    use crate::input_values::{
        FloatingInputValueDomainInput, FloatingInputValueDomainV1, InputValueDisposition,
        InputValueDomainV1,
    };
    use crate::memory_surface::{
        BufferAliasingContractInput, BufferAliasingContractV1, BufferMemoryContractInput,
        BufferMemoryContractV1, BufferPairV1, MemoryConditionDisposition,
        PointerAlignmentContractV1, RequiredAlignmentBytes,
    };

    fn id<T: ContentType>(seed: &str) -> ContentId<T> {
        ContentId::derive(seed.as_bytes()).expect("identity")
    }

    fn range(minimum: u64, maximum: u64) -> InclusiveExtentRange {
        InclusiveExtentRange::new(ExtentValue::new(minimum), ExtentValue::new(maximum))
            .expect("range")
    }

    fn integer_range(minimum: i64, maximum: i64) -> InclusiveIntegerRange {
        InclusiveIntegerRange::new(IntegerValue::new(minimum), IntegerValue::new(maximum))
            .expect("range")
    }

    fn status_error() -> InvalidInputBehavior {
        InvalidInputBehavior::ReturnStatus {
            status: StatusCode::new(-1),
        }
    }

    fn floating_value_domain() -> InputValueDomainV1 {
        InputValueDomainV1::Floating {
            special_values: FloatingInputValueDomainV1::new(FloatingInputValueDomainInput {
                negative_zero: InputValueDisposition::Supported,
                subnormal: InputValueDisposition::Supported,
                infinity: InputValueDisposition::Unknown,
                nan: InputValueDisposition::Unknown,
            }),
        }
    }

    fn memory_contract() -> BufferMemoryContractV1 {
        BufferMemoryContractV1::new(BufferMemoryContractInput {
            null_non_empty: MemoryConditionDisposition::Invalid {
                behavior: status_error(),
            },
            alignment: PointerAlignmentContractV1::Required {
                bytes: RequiredAlignmentBytes::new(16).expect("alignment"),
                misaligned_non_empty: MemoryConditionDisposition::Invalid {
                    behavior: status_error(),
                },
            },
            insufficient_capacity_non_empty: MemoryConditionDisposition::Invalid {
                behavior: status_error(),
            },
        })
    }

    fn reduction_domain(maximum: u64) -> MigrationDomainContractV1 {
        let n = ShapeSymbolName::new("n").expect("symbol");
        MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("reduce_sum").expect("entry"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: BufferName::new("input").expect("buffer"),
                    access: BufferAccessV1::Input {
                        value_domain: floating_value_domain(),
                    },
                    data_type: DataType::F32,
                    shape: vec![DimensionSpec::Symbol { symbol: n.clone() }],
                    memory: memory_contract(),
                })
                .expect("input"),
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(1),
                    name: BufferName::new("output").expect("buffer"),
                    access: BufferAccessV1::Output,
                    data_type: DataType::F32,
                    shape: Vec::new(),
                    memory: memory_contract(),
                })
                .expect("output"),
            ],
            scalar_parameters: vec![
                ScalarParameterContractV1::new(ScalarParameterContractInput {
                    argument_index: ArgumentIndex::new(2),
                    name: ScalarParameterName::new("element_count").expect("parameter"),
                    role: ScalarParameterRole::ShapeExtent,
                    data_type: DataType::U64,
                    valid_range: integer_range(0, i64::try_from(maximum).expect("test maximum")),
                    invalid_behavior: status_error(),
                })
                .expect("shape parameter"),
                ScalarParameterContractV1::new(ScalarParameterContractInput {
                    argument_index: ArgumentIndex::new(3),
                    name: ScalarParameterName::new("mode").expect("parameter"),
                    role: ScalarParameterRole::Control,
                    data_type: DataType::I32,
                    valid_range: integer_range(-1, 2),
                    invalid_behavior: InvalidInputBehavior::RejectBeforeExecution,
                })
                .expect("control parameter"),
            ],
            shape_symbols: vec![
                ShapeSymbolContractV1::new(ShapeSymbolContractInput {
                    name: n,
                    valid_range: range(0, maximum),
                    source: ShapeSymbolSource::ScalarParameter {
                        parameter: ScalarParameterName::new("element_count").expect("parameter"),
                    },
                    boundary_moduli: vec![ExtentModulus::new(256).expect("modulus")],
                    invalid_behavior: status_error(),
                })
                .expect("symbol"),
            ],
            buffer_aliasing: vec![BufferAliasingContractV1::new(BufferAliasingContractInput {
                pair: BufferPairV1::new(
                    BufferName::new("input").expect("buffer"),
                    BufferName::new("output").expect("buffer"),
                )
                .expect("pair"),
                exact_alias: MemoryConditionDisposition::Invalid {
                    behavior: status_error(),
                },
                partial_overlap: MemoryConditionDisposition::Invalid {
                    behavior: status_error(),
                },
            })],
            requested_semantics: id::<RequestedSemanticsArtifact>("sum-semantics"),
            semantic_claim: SemanticClaimKind::Numerical,
            exclusions: vec![id::<MigrationDomainExclusionArtifact>("nan-payload")],
        })
        .expect("domain")
    }

    #[test]
    fn domain_round_trips_strict_v1_and_commits_every_semantic_edge() {
        let domain = reduction_domain(1_025);
        let bytes = cairn_codec::to_vec(&domain).expect("domain bytes");
        assert_eq!(
            cairn_codec::from_slice::<MigrationDomainContractV1>(&bytes).expect("strict domain"),
            domain
        );
        let base = ContentId::<CallerDomainBodyArtifact>::derive(&bytes).expect("domain id");
        let changed = reduction_domain(2_049);
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed bytes");
        assert_ne!(
            base,
            ContentId::<CallerDomainBodyArtifact>::derive(&changed_bytes).expect("changed id")
        );

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["legacy_shape"] = serde_json::json!(true);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(value).is_err());
    }

    #[test]
    fn abi_names_indices_types_and_shape_sources_fail_closed() {
        assert!(BufferName::new("-input").is_err());
        assert!(
            ScalarParameterContractV1::new(ScalarParameterContractInput {
                argument_index: ArgumentIndex::new(0),
                name: ScalarParameterName::new("scale").expect("name"),
                role: ScalarParameterRole::Control,
                data_type: DataType::F32,
                valid_range: integer_range(0, 1),
                invalid_behavior: status_error(),
            })
            .is_err()
        );
        assert!(
            ScalarParameterContractV1::new(ScalarParameterContractInput {
                argument_index: ArgumentIndex::new(0),
                name: ScalarParameterName::new("unsigned_count").expect("name"),
                role: ScalarParameterRole::Control,
                data_type: DataType::U64,
                valid_range: integer_range(-1, 8),
                invalid_behavior: status_error(),
            })
            .is_err()
        );
        assert!(
            ScalarParameterContractV1::new(ScalarParameterContractInput {
                argument_index: ArgumentIndex::new(0),
                name: ScalarParameterName::new("small_signed").expect("name"),
                role: ScalarParameterRole::Control,
                data_type: DataType::I8,
                valid_range: integer_range(-128, 128),
                invalid_behavior: status_error(),
            })
            .is_err()
        );
        assert!(
            ScalarParameterContractV1::new(ScalarParameterContractInput {
                argument_index: ArgumentIndex::new(0),
                name: ScalarParameterName::new("flag").expect("name"),
                role: ScalarParameterRole::Control,
                data_type: DataType::Bool,
                valid_range: integer_range(0, 2),
                invalid_behavior: status_error(),
            })
            .is_err()
        );

        let mut value = serde_json::to_value(reduction_domain(1_025)).expect("json");
        value["scalar_parameters"][0]["argument_index"] = serde_json::json!(1);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(value).is_err());

        let mut value = serde_json::to_value(reduction_domain(1_025)).expect("json");
        value["shape_symbols"][0]["valid_range"]["maximum"] = serde_json::json!(1024);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(value).is_err());
    }

    #[test]
    fn trusted_derivation_covers_boundaries_tails_invalids_and_complete_assignments() {
        let derived = derive_mandatory_base_cases(&reduction_domain(1_025)).expect("derive");
        let shape_values: BTreeSet<_> = derived
            .cases()
            .iter()
            .filter_map(|case| match case.target() {
                CaseTarget::ShapeSymbol { value, .. } => Some(value.get()),
                CaseTarget::ScalarParameter { .. } => None,
            })
            .collect();
        for required in [0, 1, 255, 256, 257, 1_023, 1_024, 1_025, 1_026] {
            assert!(
                shape_values.contains(&required),
                "missing extent {required}"
            );
        }
        let tail = derived
            .cases()
            .iter()
            .find(|case| {
                matches!(
                    case.target(),
                    CaseTarget::ShapeSymbol { value, .. } if value.get() == 257
                )
            })
            .expect("tail case");
        assert!(
            tail.shape_obligations()
                .contains(&ShapeBoundaryObligation::ModulusAbove(
                    ExtentModulus::new(256).expect("modulus")
                ))
        );
        assert_eq!(tail.shape_assignments().len(), 1);
        assert_eq!(tail.scalar_assignments().len(), 2);
        assert!(tail.scalar_assignments().iter().any(|assignment| {
            assignment.parameter().as_str() == "element_count" && assignment.value().get() == 257
        }));

        let invalid = derived
            .cases()
            .iter()
            .find(|case| {
                matches!(
                    case.target(),
                    CaseTarget::ShapeSymbol { value, .. } if value.get() == 1_026
                )
            })
            .expect("invalid case");
        assert!(matches!(
            invalid.expected_outcome(),
            CaseExpectedOutcome::Invalid {
                behavior: InvalidInputBehavior::ReturnStatus { status }
            } if status.get() == -1
        ));
    }

    #[test]
    fn derived_case_set_is_canonical_strict_and_identity_sensitive() {
        let derived = derive_mandatory_base_cases(&reduction_domain(1_025)).expect("derive");
        let bytes = cairn_codec::to_vec(&derived).expect("case bytes");
        assert_eq!(
            cairn_codec::from_slice::<MigrationMandatoryCasesV1>(&bytes).expect("strict cases"),
            derived
        );
        let changed =
            derive_mandatory_base_cases(&reduction_domain(2_049)).expect("changed derive");
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed case bytes");
        assert_ne!(
            ContentId::<MigrationMandatoryCasesArtifact>::derive(&bytes).expect("case identity"),
            ContentId::<MigrationMandatoryCasesArtifact>::derive(&changed_bytes)
                .expect("changed case identity")
        );
    }

    #[test]
    fn reversed_ranges_moduli_and_noncanonical_collections_are_rejected() {
        assert!(InclusiveExtentRange::new(ExtentValue::new(2), ExtentValue::new(1)).is_err());
        assert!(InclusiveIntegerRange::new(IntegerValue::new(2), IntegerValue::new(1)).is_err());
        assert_eq!(
            ExtentModulus::new(1),
            Err(DomainContractError::InvalidModulus)
        );

        let mut value = serde_json::to_value(reduction_domain(1_025)).expect("json");
        value["shape_symbols"][0]["boundary_moduli"] = serde_json::json!([256, 128]);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(value).is_err());
    }
}
