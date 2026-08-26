//! Strongly typed input-value domains and trusted dtype-pattern obligations.

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::CallerDomainBodyArtifact;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::domain::{
    BufferName, DataType, DomainContractError, InvalidInputBehavior, MigrationDomainContractV1,
    MigrationDomainExclusionArtifact,
};

/// Caller-declared treatment of one special input-value family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InputValueDisposition {
    /// Values in this family are inside the requested semantic domain.
    Supported,
    /// Values in this family require an explicit invalid-input response.
    Invalid {
        /// Caller-declared behavior; trusted derivation does not invent a status.
        behavior: InvalidInputBehavior,
    },
    /// No behavior is claimed, with an exact exclusion artifact explaining the boundary.
    ExplicitlyExcluded {
        /// Typed exclusion evidence edge.
        exclusion: ContentId<MigrationDomainExclusionArtifact>,
    },
    /// The caller does not know whether this family is in the requested domain.
    Unknown,
}

impl InputValueDisposition {
    fn exclusion(&self) -> Option<&ContentId<MigrationDomainExclusionArtifact>> {
        match self {
            Self::ExplicitlyExcluded { exclusion } => Some(exclusion),
            Self::Supported | Self::Invalid { .. } | Self::Unknown => None,
        }
    }
}

/// Constructor input for all floating special-value boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloatingInputValueDomainInput {
    /// Treatment of negative zero as distinct from positive zero.
    pub negative_zero: InputValueDisposition,
    /// Treatment shared by positive and negative subnormal values.
    pub subnormal: InputValueDisposition,
    /// Treatment shared by positive and negative infinity.
    pub infinity: InputValueDisposition,
    /// Treatment shared by quiet and signaling NaN construction patterns.
    pub nan: InputValueDisposition,
}

/// Complete caller declaration for floating special-value families.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "FloatingInputValueDomainWire")]
pub struct FloatingInputValueDomainV1 {
    negative_zero: InputValueDisposition,
    subnormal: InputValueDisposition,
    infinity: InputValueDisposition,
    nan: InputValueDisposition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatingInputValueDomainWire {
    negative_zero: InputValueDisposition,
    subnormal: InputValueDisposition,
    infinity: InputValueDisposition,
    nan: InputValueDisposition,
}

impl FloatingInputValueDomainV1 {
    /// Creates an explicit declaration with no implicit special-value defaults.
    #[must_use]
    pub const fn new(input: FloatingInputValueDomainInput) -> Self {
        Self {
            negative_zero: input.negative_zero,
            subnormal: input.subnormal,
            infinity: input.infinity,
            nan: input.nan,
        }
    }

    /// Returns the negative-zero disposition.
    #[must_use]
    pub const fn negative_zero(&self) -> &InputValueDisposition {
        &self.negative_zero
    }

    /// Returns the subnormal disposition.
    #[must_use]
    pub const fn subnormal(&self) -> &InputValueDisposition {
        &self.subnormal
    }

    /// Returns the infinity disposition.
    #[must_use]
    pub const fn infinity(&self) -> &InputValueDisposition {
        &self.infinity
    }

    /// Returns the NaN disposition.
    #[must_use]
    pub const fn nan(&self) -> &InputValueDisposition {
        &self.nan
    }

    fn referenced_exclusions(
        &self,
    ) -> impl Iterator<Item = &ContentId<MigrationDomainExclusionArtifact>> {
        [
            self.negative_zero.exclusion(),
            self.subnormal.exclusion(),
            self.infinity.exclusion(),
            self.nan.exclusion(),
        ]
        .into_iter()
        .flatten()
    }
}

impl From<FloatingInputValueDomainWire> for FloatingInputValueDomainV1 {
    fn from(wire: FloatingInputValueDomainWire) -> Self {
        Self::new(FloatingInputValueDomainInput {
            negative_zero: wire.negative_zero,
            subnormal: wire.subnormal,
            infinity: wire.infinity,
            nan: wire.nan,
        })
    }
}

/// Value-domain category attached to an input-capable buffer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InputValueDomainV1 {
    /// IEEE floating values with every special family explicitly classified.
    Floating {
        /// Special-value declaration.
        special_values: FloatingInputValueDomainV1,
    },
    /// All values representable by the declared signed integer dtype.
    SignedInteger,
    /// All values representable by the declared unsigned integer dtype.
    UnsignedInteger,
    /// Boolean false and true.
    Boolean,
}

impl InputValueDomainV1 {
    pub(crate) const fn is_compatible_with(&self, data_type: DataType) -> bool {
        matches!(
            (self, data_type),
            (
                Self::Floating { .. },
                DataType::F16 | DataType::F32 | DataType::F64
            ) | (
                Self::SignedInteger,
                DataType::I8 | DataType::I16 | DataType::I32 | DataType::I64
            ) | (
                Self::UnsignedInteger,
                DataType::U8 | DataType::U16 | DataType::U32 | DataType::U64
            ) | (Self::Boolean, DataType::Bool)
        )
    }

    pub(crate) fn referenced_exclusions(
        &self,
    ) -> impl Iterator<Item = &ContentId<MigrationDomainExclusionArtifact>> {
        match self {
            Self::Floating { special_values } => Some(special_values),
            Self::SignedInteger | Self::UnsignedInteger | Self::Boolean => None,
        }
        .into_iter()
        .flat_map(FloatingInputValueDomainV1::referenced_exclusions)
    }
}

/// IEEE floating dtype carried only by floating pattern targets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FloatingDataType {
    /// IEEE binary16.
    F16,
    /// IEEE binary32.
    F32,
    /// IEEE binary64.
    F64,
}

/// Signed integer dtype carried only by signed-integer pattern targets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignedIntegerDataType {
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 32-bit integer.
    I32,
    /// Signed 64-bit integer.
    I64,
}

/// Unsigned integer dtype carried only by unsigned-integer pattern targets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnsignedIntegerDataType {
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer.
    U64,
}

/// Floating input construction recipe.
///
/// These recipes are semantic instructions to a later typed materializer, not binary `f64`
/// literals. The target dtype determines the exact bit pattern.
///
/// ```compile_fail
/// use cairn_migration::{FloatingInputPattern, SignedIntegerInputPattern};
///
/// fn require_float(_: FloatingInputPattern) {}
/// require_float(SignedIntegerInputPattern::Minimum);
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FloatingInputPattern {
    /// Fill with positive zero.
    PositiveZero,
    /// Fill with negative zero, preserving its sign bit.
    NegativeZero,
    /// Fill with positive one.
    PositiveOne,
    /// Fill with negative one.
    NegativeOne,
    /// Fill with the most negative finite value of the exact dtype.
    LowestFinite,
    /// Fill with the greatest finite value of the exact dtype.
    HighestFinite,
    /// Fill with the smallest positive normal value.
    SmallestPositiveNormal,
    /// Fill with the smallest positive subnormal value.
    SmallestPositiveSubnormal,
    /// Fill with the greatest negative subnormal value by magnitude nearest zero.
    SmallestNegativeSubnormal,
    /// Fill with positive infinity.
    PositiveInfinity,
    /// Fill with negative infinity.
    NegativeInfinity,
    /// Fill with a canonical quiet NaN payload.
    QuietNan,
    /// Fill with a canonical signaling NaN payload where the dtype supports it.
    SignalingNan,
    /// Fill `[+1, -1, +1, -1, ...]`, retaining the positive tail when length is odd.
    AlternatingUnitCancellation,
    /// Repeat `[2^(p+1), 1, -2^(p+1), 1]`, truncated to length, where `p` is the exact dtype's
    /// significand precision including its implicit leading bit.
    MixedFiniteScaleCancellation,
}

/// Signed-integer input construction recipe.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignedIntegerInputPattern {
    /// Exact dtype minimum.
    Minimum,
    /// Exact dtype maximum.
    Maximum,
    /// Zero.
    Zero,
    /// One.
    One,
    /// Negative one.
    NegativeOne,
    /// Fill `[1, -1, 1, -1, ...]`, retaining the positive tail when length is odd.
    AlternatingUnitCancellation,
}

/// Unsigned-integer input construction recipe.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnsignedIntegerInputPattern {
    /// Exact dtype minimum (zero).
    Minimum,
    /// Exact dtype maximum.
    Maximum,
    /// One.
    One,
}

/// Boolean input construction recipe.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BooleanInputPattern {
    /// All false.
    False,
    /// All true.
    True,
    /// Alternating false and true.
    Alternating,
}

/// Strongly typed target of one mandatory input-value obligation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InputValueCaseTarget {
    /// Floating input pattern.
    Floating {
        /// Input buffer.
        buffer: BufferName,
        /// Exact floating dtype.
        data_type: FloatingDataType,
        /// Typed construction recipe.
        pattern: FloatingInputPattern,
    },
    /// Signed-integer input pattern.
    SignedInteger {
        /// Input buffer.
        buffer: BufferName,
        /// Exact signed dtype.
        data_type: SignedIntegerDataType,
        /// Typed construction recipe.
        pattern: SignedIntegerInputPattern,
    },
    /// Unsigned-integer input pattern.
    UnsignedInteger {
        /// Input buffer.
        buffer: BufferName,
        /// Exact unsigned dtype.
        data_type: UnsignedIntegerDataType,
        /// Typed construction recipe.
        pattern: UnsignedIntegerInputPattern,
    },
    /// Boolean input pattern.
    Boolean {
        /// Input buffer.
        buffer: BufferName,
        /// Typed construction recipe.
        pattern: BooleanInputPattern,
    },
}

impl InputValueCaseTarget {
    /// Returns the input buffer whose bytes will be materialized.
    #[must_use]
    pub const fn buffer(&self) -> &BufferName {
        match self {
            Self::Floating { buffer, .. }
            | Self::SignedInteger { buffer, .. }
            | Self::UnsignedInteger { buffer, .. }
            | Self::Boolean { buffer, .. } => buffer,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InputValueSchemaV1;

impl Serialize for InputValueSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for InputValueSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom("input-value schema version must be 1")),
        }
    }
}

/// One typed mandatory input-value obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MandatoryInputValueCaseWire")]
pub struct MandatoryInputValueCaseV1 {
    schema_version: InputValueSchemaV1,
    target: InputValueCaseTarget,
    disposition: InputValueDisposition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MandatoryInputValueCaseWire {
    schema_version: InputValueSchemaV1,
    target: InputValueCaseTarget,
    disposition: InputValueDisposition,
}

impl MandatoryInputValueCaseV1 {
    const fn new(target: InputValueCaseTarget, disposition: InputValueDisposition) -> Self {
        Self {
            schema_version: InputValueSchemaV1,
            target,
            disposition,
        }
    }

    /// Returns the exact dtype-specific construction target.
    #[must_use]
    pub const fn target(&self) -> &InputValueCaseTarget {
        &self.target
    }

    /// Returns whether the caller supports, rejects, excludes, or does not know this family.
    #[must_use]
    pub const fn disposition(&self) -> &InputValueDisposition {
        &self.disposition
    }
}

impl From<MandatoryInputValueCaseWire> for MandatoryInputValueCaseV1 {
    fn from(wire: MandatoryInputValueCaseWire) -> Self {
        let _ = wire.schema_version;
        Self::new(wire.target, wire.disposition)
    }
}

/// Content identity domain for one mandatory dtype-pattern obligation.
pub enum MandatoryInputValueCaseArtifact {}

impl ContentType for MandatoryInputValueCaseArtifact {
    const DOMAIN: &'static str = "migration.mandatory-input-value-case.v1";
}

/// Trusted algorithm used to derive dtype-pattern obligations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputValueDerivationPolicy {
    /// Dtype extrema, signed zero, non-finite, subnormal, cancellation, and scale recipes.
    DtypePatternsV1,
}

/// Canonical set of mandatory dtype-pattern obligations for one exact caller domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MandatoryInputValueCasesWire")]
pub struct MandatoryInputValueCasesV1 {
    schema_version: InputValueSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: InputValueDerivationPolicy,
    cases: Vec<MandatoryInputValueCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MandatoryInputValueCasesWire {
    schema_version: InputValueSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: InputValueDerivationPolicy,
    cases: Vec<MandatoryInputValueCaseV1>,
}

impl MandatoryInputValueCasesV1 {
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        cases: Vec<MandatoryInputValueCaseV1>,
    ) -> Result<Self, DomainContractError> {
        if cases.is_empty() {
            return Err(DomainContractError::EmptySet {
                field: "mandatory input-value cases",
            });
        }
        if cases
            .windows(2)
            .any(|pair| pair[0].target() >= pair[1].target())
        {
            return Err(DomainContractError::NonCanonicalSet {
                field: "mandatory input-value cases",
            });
        }
        Ok(Self {
            schema_version: InputValueSchemaV1,
            domain,
            derivation_policy: InputValueDerivationPolicy::DtypePatternsV1,
            cases,
        })
    }

    /// Returns the exact caller-domain body from which these obligations were derived.
    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    /// Returns the trusted derivation algorithm identity.
    #[must_use]
    pub const fn derivation_policy(&self) -> InputValueDerivationPolicy {
        self.derivation_policy
    }

    /// Returns obligations in strict typed-target order.
    #[must_use]
    pub fn cases(&self) -> &[MandatoryInputValueCaseV1] {
        &self.cases
    }
}

impl TryFrom<MandatoryInputValueCasesWire> for MandatoryInputValueCasesV1 {
    type Error = DomainContractError;

    fn try_from(wire: MandatoryInputValueCasesWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        if wire.derivation_policy != InputValueDerivationPolicy::DtypePatternsV1 {
            return Err(DomainContractError::InvalidDerivedCase {
                reason: "unexpected input-value derivation policy",
            });
        }
        Self::new(wire.domain, wire.cases)
    }
}

/// Content identity domain for a complete mandatory dtype-pattern set.
pub enum MandatoryInputValueCasesArtifact {}

impl ContentType for MandatoryInputValueCasesArtifact {
    const DOMAIN: &'static str = "migration.mandatory-input-value-cases.v1";
}

/// Derives mandatory dtype extrema and floating special/cancellation/scale recipes.
///
/// This stage creates typed construction obligations. It deliberately does not fabricate concrete
/// input bytes; a later operator adapter must materialize each recipe for the exact shape and dtype.
///
/// # Errors
///
/// Returns an error if the caller-domain bytes cannot be canonicalized or the resulting set is not
/// a non-empty canonical target set.
pub fn derive_mandatory_input_value_cases(
    contract: &MigrationDomainContractV1,
) -> Result<MandatoryInputValueCasesV1, DomainContractError> {
    let domain_bytes =
        cairn_codec::to_vec(contract).map_err(|error| DomainContractError::Codec {
            message: error.to_string(),
        })?;
    let domain = ContentId::<CallerDomainBodyArtifact>::derive(&domain_bytes).map_err(|error| {
        DomainContractError::Codec {
            message: error.to_string(),
        }
    })?;
    let mut cases = Vec::new();
    for buffer in contract.buffers() {
        let Some(value_domain) = buffer.input_value_domain() else {
            continue;
        };
        derive_buffer_cases(buffer.name(), buffer.data_type(), value_domain, &mut cases)?;
    }
    cases.sort_by(|left, right| left.target().cmp(right.target()));
    MandatoryInputValueCasesV1::new(domain, cases)
}

fn derive_buffer_cases(
    buffer: &BufferName,
    data_type: DataType,
    domain: &InputValueDomainV1,
    cases: &mut Vec<MandatoryInputValueCaseV1>,
) -> Result<(), DomainContractError> {
    match (data_type, domain) {
        (DataType::F16, InputValueDomainV1::Floating { special_values }) => {
            add_floating_cases(buffer, FloatingDataType::F16, special_values, cases);
        }
        (DataType::F32, InputValueDomainV1::Floating { special_values }) => {
            add_floating_cases(buffer, FloatingDataType::F32, special_values, cases);
        }
        (DataType::F64, InputValueDomainV1::Floating { special_values }) => {
            add_floating_cases(buffer, FloatingDataType::F64, special_values, cases);
        }
        (DataType::I8, InputValueDomainV1::SignedInteger) => {
            add_signed_cases(buffer, SignedIntegerDataType::I8, cases);
        }
        (DataType::I16, InputValueDomainV1::SignedInteger) => {
            add_signed_cases(buffer, SignedIntegerDataType::I16, cases);
        }
        (DataType::I32, InputValueDomainV1::SignedInteger) => {
            add_signed_cases(buffer, SignedIntegerDataType::I32, cases);
        }
        (DataType::I64, InputValueDomainV1::SignedInteger) => {
            add_signed_cases(buffer, SignedIntegerDataType::I64, cases);
        }
        (DataType::U8, InputValueDomainV1::UnsignedInteger) => {
            add_unsigned_cases(buffer, UnsignedIntegerDataType::U8, cases);
        }
        (DataType::U16, InputValueDomainV1::UnsignedInteger) => {
            add_unsigned_cases(buffer, UnsignedIntegerDataType::U16, cases);
        }
        (DataType::U32, InputValueDomainV1::UnsignedInteger) => {
            add_unsigned_cases(buffer, UnsignedIntegerDataType::U32, cases);
        }
        (DataType::U64, InputValueDomainV1::UnsignedInteger) => {
            add_unsigned_cases(buffer, UnsignedIntegerDataType::U64, cases);
        }
        (DataType::Bool, InputValueDomainV1::Boolean) => add_boolean_cases(buffer, cases),
        _ => {
            return Err(DomainContractError::InputValueDomainTypeMismatch {
                buffer: buffer.to_string(),
            });
        }
    }
    Ok(())
}

fn add_floating_cases(
    buffer: &BufferName,
    data_type: FloatingDataType,
    special_values: &FloatingInputValueDomainV1,
    cases: &mut Vec<MandatoryInputValueCaseV1>,
) {
    const PATTERNS: [FloatingInputPattern; 15] = [
        FloatingInputPattern::PositiveZero,
        FloatingInputPattern::NegativeZero,
        FloatingInputPattern::PositiveOne,
        FloatingInputPattern::NegativeOne,
        FloatingInputPattern::LowestFinite,
        FloatingInputPattern::HighestFinite,
        FloatingInputPattern::SmallestPositiveNormal,
        FloatingInputPattern::SmallestPositiveSubnormal,
        FloatingInputPattern::SmallestNegativeSubnormal,
        FloatingInputPattern::PositiveInfinity,
        FloatingInputPattern::NegativeInfinity,
        FloatingInputPattern::QuietNan,
        FloatingInputPattern::SignalingNan,
        FloatingInputPattern::AlternatingUnitCancellation,
        FloatingInputPattern::MixedFiniteScaleCancellation,
    ];
    for pattern in PATTERNS {
        let disposition = match pattern {
            FloatingInputPattern::NegativeZero => special_values.negative_zero().clone(),
            FloatingInputPattern::SmallestPositiveSubnormal
            | FloatingInputPattern::SmallestNegativeSubnormal => special_values.subnormal().clone(),
            FloatingInputPattern::PositiveInfinity | FloatingInputPattern::NegativeInfinity => {
                special_values.infinity().clone()
            }
            FloatingInputPattern::QuietNan | FloatingInputPattern::SignalingNan => {
                special_values.nan().clone()
            }
            FloatingInputPattern::PositiveZero
            | FloatingInputPattern::PositiveOne
            | FloatingInputPattern::NegativeOne
            | FloatingInputPattern::LowestFinite
            | FloatingInputPattern::HighestFinite
            | FloatingInputPattern::SmallestPositiveNormal
            | FloatingInputPattern::AlternatingUnitCancellation
            | FloatingInputPattern::MixedFiniteScaleCancellation => {
                InputValueDisposition::Supported
            }
        };
        cases.push(MandatoryInputValueCaseV1::new(
            InputValueCaseTarget::Floating {
                buffer: buffer.clone(),
                data_type,
                pattern,
            },
            disposition,
        ));
    }
}

fn add_signed_cases(
    buffer: &BufferName,
    data_type: SignedIntegerDataType,
    cases: &mut Vec<MandatoryInputValueCaseV1>,
) {
    for pattern in [
        SignedIntegerInputPattern::Minimum,
        SignedIntegerInputPattern::Maximum,
        SignedIntegerInputPattern::Zero,
        SignedIntegerInputPattern::One,
        SignedIntegerInputPattern::NegativeOne,
        SignedIntegerInputPattern::AlternatingUnitCancellation,
    ] {
        cases.push(MandatoryInputValueCaseV1::new(
            InputValueCaseTarget::SignedInteger {
                buffer: buffer.clone(),
                data_type,
                pattern,
            },
            InputValueDisposition::Supported,
        ));
    }
}

fn add_unsigned_cases(
    buffer: &BufferName,
    data_type: UnsignedIntegerDataType,
    cases: &mut Vec<MandatoryInputValueCaseV1>,
) {
    for pattern in [
        UnsignedIntegerInputPattern::Minimum,
        UnsignedIntegerInputPattern::Maximum,
        UnsignedIntegerInputPattern::One,
    ] {
        cases.push(MandatoryInputValueCaseV1::new(
            InputValueCaseTarget::UnsignedInteger {
                buffer: buffer.clone(),
                data_type,
                pattern,
            },
            InputValueDisposition::Supported,
        ));
    }
}

fn add_boolean_cases(buffer: &BufferName, cases: &mut Vec<MandatoryInputValueCaseV1>) {
    for pattern in [
        BooleanInputPattern::False,
        BooleanInputPattern::True,
        BooleanInputPattern::Alternating,
    ] {
        cases.push(MandatoryInputValueCaseV1::new(
            InputValueCaseTarget::Boolean {
                buffer: buffer.clone(),
                pattern,
            },
            InputValueDisposition::Supported,
        ));
    }
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, ContentType};
    use cairn_verification::CallerDomainBodyArtifact;

    use super::{
        FloatingInputPattern, FloatingInputValueDomainInput, FloatingInputValueDomainV1,
        InputValueCaseTarget, InputValueDisposition, InputValueDomainV1,
        MandatoryInputValueCasesV1, derive_mandatory_input_value_cases,
    };
    use crate::domain::{
        ArgumentIndex, BufferAccessV1, BufferContractInput, BufferContractV1, BufferName, DataType,
        DimensionAxis, DimensionSpec, DomainContractError, EntryPointName, ExtentModulus,
        ExtentValue, InclusiveExtentRange, InvalidInputBehavior, MigrationDomainContractInput,
        MigrationDomainContractV1, MigrationDomainExclusionArtifact, RequestedSemanticsArtifact,
        SemanticClaimKind, ShapeSymbolContractInput, ShapeSymbolContractV1, ShapeSymbolName,
        ShapeSymbolSource, StatusCode,
    };
    use crate::memory_surface::{
        BufferAliasingContractInput, BufferAliasingContractV1, BufferMemoryContractInput,
        BufferMemoryContractV1, BufferPairV1, MemoryConditionDisposition,
        PointerAlignmentContractV1,
    };

    fn id<T: ContentType>(seed: &str) -> ContentId<T> {
        ContentId::derive(seed.as_bytes()).expect("identity")
    }

    fn special_values(
        infinity: InputValueDisposition,
        nan: InputValueDisposition,
    ) -> FloatingInputValueDomainV1 {
        FloatingInputValueDomainV1::new(FloatingInputValueDomainInput {
            negative_zero: InputValueDisposition::Supported,
            subnormal: InputValueDisposition::Invalid {
                behavior: InvalidInputBehavior::ReturnStatus {
                    status: StatusCode::new(-7),
                },
            },
            infinity,
            nan,
        })
    }

    fn memory_contract() -> BufferMemoryContractV1 {
        BufferMemoryContractV1::new(BufferMemoryContractInput {
            null_non_empty: MemoryConditionDisposition::Unknown,
            alignment: PointerAlignmentContractV1::ByteAligned,
            insufficient_capacity_non_empty: MemoryConditionDisposition::Unknown,
        })
    }

    fn aliasing_contracts(names: &[&str]) -> Vec<BufferAliasingContractV1> {
        let mut contracts = Vec::new();
        for (index, left) in names.iter().enumerate() {
            for right in &names[index + 1..] {
                let mut pair = [*left, *right];
                pair.sort_unstable();
                contracts.push(BufferAliasingContractV1::new(BufferAliasingContractInput {
                    pair: BufferPairV1::new(
                        BufferName::new(pair[0]).expect("buffer"),
                        BufferName::new(pair[1]).expect("buffer"),
                    )
                    .expect("pair"),
                    exact_alias: MemoryConditionDisposition::Unknown,
                    partial_overlap: MemoryConditionDisposition::Unknown,
                }));
            }
        }
        contracts.sort_by(|left, right| left.pair().cmp(right.pair()));
        contracts
    }

    fn reduction_domain(
        values: FloatingInputValueDomainV1,
        exclusions: Vec<ContentId<MigrationDomainExclusionArtifact>>,
    ) -> Result<MigrationDomainContractV1, DomainContractError> {
        let symbol = ShapeSymbolName::new("n").expect("symbol");
        MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("reduce_sum").expect("entry"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: BufferName::new("input").expect("buffer"),
                    access: BufferAccessV1::Input {
                        value_domain: InputValueDomainV1::Floating {
                            special_values: values,
                        },
                    },
                    data_type: DataType::F32,
                    shape: vec![DimensionSpec::Symbol {
                        symbol: symbol.clone(),
                    }],
                    memory: memory_contract(),
                })?,
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(1),
                    name: BufferName::new("output").expect("buffer"),
                    access: BufferAccessV1::Output,
                    data_type: DataType::F32,
                    shape: Vec::new(),
                    memory: memory_contract(),
                })?,
            ],
            scalar_parameters: Vec::new(),
            shape_symbols: vec![ShapeSymbolContractV1::new(ShapeSymbolContractInput {
                name: symbol,
                valid_range: InclusiveExtentRange::new(
                    ExtentValue::new(1),
                    ExtentValue::new(1_025),
                )?,
                source: ShapeSymbolSource::BufferDimension {
                    buffer: BufferName::new("input").expect("buffer"),
                    axis: DimensionAxis::new(0),
                },
                boundary_moduli: vec![ExtentModulus::new(256)?],
                invalid_behavior: InvalidInputBehavior::RejectBeforeExecution,
            })?],
            buffer_aliasing: aliasing_contracts(&["input", "output"]),
            requested_semantics: id::<RequestedSemanticsArtifact>("sum-semantics"),
            semantic_claim: SemanticClaimKind::Numerical,
            exclusions,
        })
    }

    #[test]
    fn dtype_patterns_preserve_special_dispositions_and_are_canonical() {
        let nan_exclusion = id::<MigrationDomainExclusionArtifact>("nan-exclusion");
        let domain = reduction_domain(
            special_values(
                InputValueDisposition::Unknown,
                InputValueDisposition::ExplicitlyExcluded {
                    exclusion: nan_exclusion,
                },
            ),
            vec![nan_exclusion],
        )
        .expect("domain");
        let derived = derive_mandatory_input_value_cases(&domain).expect("derive");
        assert_eq!(derived.cases().len(), 15);
        assert!(
            derived
                .cases()
                .windows(2)
                .all(|pair| pair[0].target() < pair[1].target())
        );

        let disposition = |pattern| {
            derived
                .cases()
                .iter()
                .find_map(|case| match case.target() {
                    InputValueCaseTarget::Floating {
                        pattern: candidate, ..
                    } if *candidate == pattern => Some(case.disposition()),
                    _ => None,
                })
                .expect("pattern")
        };
        assert_eq!(
            disposition(FloatingInputPattern::PositiveInfinity),
            &InputValueDisposition::Unknown
        );
        assert_eq!(
            disposition(FloatingInputPattern::SmallestPositiveSubnormal),
            &InputValueDisposition::Invalid {
                behavior: InvalidInputBehavior::ReturnStatus {
                    status: StatusCode::new(-7),
                },
            }
        );
        assert_eq!(
            disposition(FloatingInputPattern::QuietNan),
            &InputValueDisposition::ExplicitlyExcluded {
                exclusion: nan_exclusion,
            }
        );
        assert_eq!(
            disposition(FloatingInputPattern::MixedFiniteScaleCancellation),
            &InputValueDisposition::Supported
        );
    }

    #[test]
    fn value_domain_type_and_exclusion_edges_fail_closed() {
        let type_error = BufferContractV1::new(BufferContractInput {
            argument_index: ArgumentIndex::new(0),
            name: BufferName::new("input").expect("buffer"),
            access: BufferAccessV1::Input {
                value_domain: InputValueDomainV1::SignedInteger,
            },
            data_type: DataType::F32,
            shape: Vec::new(),
            memory: memory_contract(),
        });
        assert!(matches!(
            type_error,
            Err(DomainContractError::InputValueDomainTypeMismatch { .. })
        ));

        let exclusion = id::<MigrationDomainExclusionArtifact>("missing-exclusion");
        let domain = reduction_domain(
            special_values(
                InputValueDisposition::Unknown,
                InputValueDisposition::ExplicitlyExcluded { exclusion },
            ),
            Vec::new(),
        );
        assert!(matches!(
            domain,
            Err(DomainContractError::UnlistedInputValueExclusion { .. })
        ));
    }

    #[test]
    fn each_numeric_category_derives_only_its_typed_patterns() {
        let float_domain = InputValueDomainV1::Floating {
            special_values: special_values(
                InputValueDisposition::Unknown,
                InputValueDisposition::Unknown,
            ),
        };
        let input = |index, name, data_type, value_domain| {
            BufferContractV1::new(BufferContractInput {
                argument_index: ArgumentIndex::new(index),
                name: BufferName::new(name).expect("buffer"),
                access: BufferAccessV1::Input { value_domain },
                data_type,
                shape: Vec::new(),
                memory: memory_contract(),
            })
            .expect("input")
        };
        let domain = MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("typed_inputs").expect("entry"),
            buffers: vec![
                input(0, "float_input", DataType::F16, float_domain),
                input(
                    1,
                    "signed_input",
                    DataType::I8,
                    InputValueDomainV1::SignedInteger,
                ),
                input(
                    2,
                    "unsigned_input",
                    DataType::U64,
                    InputValueDomainV1::UnsignedInteger,
                ),
                input(3, "bool_input", DataType::Bool, InputValueDomainV1::Boolean),
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(4),
                    name: BufferName::new("output").expect("buffer"),
                    access: BufferAccessV1::Output,
                    data_type: DataType::F16,
                    shape: Vec::new(),
                    memory: memory_contract(),
                })
                .expect("output"),
            ],
            scalar_parameters: Vec::new(),
            shape_symbols: Vec::new(),
            buffer_aliasing: aliasing_contracts(&[
                "float_input",
                "signed_input",
                "unsigned_input",
                "bool_input",
                "output",
            ]),
            requested_semantics: id::<RequestedSemanticsArtifact>("typed-input-semantics"),
            semantic_claim: SemanticClaimKind::Numerical,
            exclusions: Vec::new(),
        })
        .expect("domain");
        let cases = derive_mandatory_input_value_cases(&domain).expect("derive");
        assert_eq!(cases.cases().len(), 27);
        assert_eq!(
            cases
                .cases()
                .iter()
                .filter(|case| matches!(case.target(), InputValueCaseTarget::Floating { .. }))
                .count(),
            15
        );
        assert_eq!(
            cases
                .cases()
                .iter()
                .filter(|case| matches!(case.target(), InputValueCaseTarget::SignedInteger { .. }))
                .count(),
            6
        );
        assert_eq!(
            cases
                .cases()
                .iter()
                .filter(|case| matches!(
                    case.target(),
                    InputValueCaseTarget::UnsignedInteger { .. }
                ))
                .count(),
            3
        );
        assert_eq!(
            cases
                .cases()
                .iter()
                .filter(|case| matches!(case.target(), InputValueCaseTarget::Boolean { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn persisted_value_domains_and_case_sets_are_strict_v1() {
        let domain = reduction_domain(
            special_values(
                InputValueDisposition::Unknown,
                InputValueDisposition::Unknown,
            ),
            Vec::new(),
        )
        .expect("domain");
        let changed_domain = reduction_domain(
            special_values(
                InputValueDisposition::Supported,
                InputValueDisposition::Unknown,
            ),
            Vec::new(),
        )
        .expect("changed domain");
        let domain_id = ContentId::<CallerDomainBodyArtifact>::derive(
            &cairn_codec::to_vec(&domain).expect("domain bytes"),
        )
        .expect("domain id");
        let changed_domain_id = ContentId::<CallerDomainBodyArtifact>::derive(
            &cairn_codec::to_vec(&changed_domain).expect("changed domain bytes"),
        )
        .expect("changed domain id");
        assert_ne!(domain_id, changed_domain_id);

        let derived = derive_mandatory_input_value_cases(&domain).expect("derive");
        let bytes = cairn_codec::to_vec(&derived).expect("bytes");
        assert_eq!(
            cairn_codec::from_slice::<MandatoryInputValueCasesV1>(&bytes).expect("round trip"),
            derived
        );

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<MandatoryInputValueCasesV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["legacy_default"] = serde_json::json!("all-finite");
        assert!(serde_json::from_value::<MandatoryInputValueCasesV1>(value).is_err());

        let mut domain_value = serde_json::to_value(domain).expect("domain json");
        domain_value["buffers"][0]["access"]["value_domain"]["special_values"]["legacy_nan"] =
            serde_json::json!(true);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(domain_value).is_err());
    }
}
