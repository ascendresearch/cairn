//! Strongly typed pointer, capacity, and buffer-aliasing surface obligations.

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::CallerDomainBodyArtifact;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::domain::{
    BufferContractV1, BufferName, DataType, DimensionSpec, DomainContractError,
    InvalidInputBehavior, MigrationDomainContractV1, MigrationDomainExclusionArtifact,
};

/// Caller-declared outcome for one memory-surface condition.
///
/// This intentionally remains distinct from input-value disposition even though the two have
/// similar wire shapes: pointer authority and numerical-value membership are different claims.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MemoryConditionDisposition {
    /// The condition is inside the requested semantic domain.
    Supported,
    /// The condition requires explicit invalid-input behavior.
    Invalid {
        /// Caller-declared behavior; trusted derivation does not invent an error code.
        behavior: InvalidInputBehavior,
    },
    /// No behavior is claimed, with an exact exclusion artifact explaining the boundary.
    ExplicitlyExcluded {
        /// Typed exclusion evidence edge.
        exclusion: ContentId<MigrationDomainExclusionArtifact>,
    },
    /// The caller does not know how the implementation should treat this condition.
    Unknown,
}

impl MemoryConditionDisposition {
    fn exclusion(&self) -> Option<&ContentId<MigrationDomainExclusionArtifact>> {
        match self {
            Self::ExplicitlyExcluded { exclusion } => Some(exclusion),
            Self::Supported | Self::Invalid { .. } | Self::Unknown => None,
        }
    }
}

/// Required alignment in bytes, greater than one and a power of two.
///
/// ```compile_fail
/// use cairn_migration::{CapacityShortfallBytes, RequiredAlignmentBytes};
///
/// fn require_alignment(_: RequiredAlignmentBytes) {}
/// let shortfall = CapacityShortfallBytes::new(1).unwrap();
/// require_alignment(shortfall);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RequiredAlignmentBytes(u32);

impl RequiredAlignmentBytes {
    /// Creates a non-trivial power-of-two alignment.
    ///
    /// # Errors
    ///
    /// Rejects byte alignment, zero, and non-power-of-two values.
    pub fn new(value: u32) -> Result<Self, DomainContractError> {
        if value <= 1 || !value.is_power_of_two() {
            return Err(DomainContractError::InvalidRequiredAlignment);
        }
        Ok(Self(value))
    }

    /// Returns the required byte alignment.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for RequiredAlignmentBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Positive address offset used to construct a known-misaligned pointer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MisalignmentOffsetBytes(u32);

impl MisalignmentOffsetBytes {
    /// Creates a positive byte offset.
    ///
    /// # Errors
    ///
    /// Rejects zero, which would preserve an aligned base address.
    pub fn new(value: u32) -> Result<Self, DomainContractError> {
        if value == 0 {
            return Err(DomainContractError::NonPositiveMemoryQuantity {
                field: "misalignment offset bytes",
            });
        }
        Ok(Self(value))
    }

    /// Returns the address offset in bytes.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for MisalignmentOffsetBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Positive number of bytes deliberately omitted from a required buffer allocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CapacityShortfallBytes(u64);

impl CapacityShortfallBytes {
    /// Creates a positive capacity shortfall.
    ///
    /// # Errors
    ///
    /// Rejects zero, which would not exercise an insufficient-capacity surface.
    pub fn new(value: u64) -> Result<Self, DomainContractError> {
        if value == 0 {
            return Err(DomainContractError::NonPositiveMemoryQuantity {
                field: "capacity shortfall bytes",
            });
        }
        Ok(Self(value))
    }

    /// Returns the omitted byte count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for CapacityShortfallBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Positive offset used to create a partial overlap between two buffer regions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PartialOverlapOffsetBytes(u64);

impl PartialOverlapOffsetBytes {
    /// Creates a positive overlap offset.
    ///
    /// # Errors
    ///
    /// Rejects zero because zero denotes exact aliasing, a separate typed pattern.
    pub fn new(value: u64) -> Result<Self, DomainContractError> {
        if value == 0 {
            return Err(DomainContractError::NonPositiveMemoryQuantity {
                field: "partial-overlap offset bytes",
            });
        }
        Ok(Self(value))
    }

    /// Returns the second region's offset from the first in bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PartialOverlapOffsetBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Alignment requirement coupled to the disposition for violating it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PointerAlignmentContractV1 {
    /// Any byte address is aligned; no misalignment obligation exists.
    ByteAligned,
    /// Addresses must satisfy a non-trivial power-of-two alignment.
    Required {
        /// Required alignment in bytes.
        bytes: RequiredAlignmentBytes,
        /// Required behavior for a deliberately misaligned non-empty buffer.
        misaligned_non_empty: MemoryConditionDisposition,
    },
}

impl PointerAlignmentContractV1 {
    fn misaligned_case(&self) -> Option<(RequiredAlignmentBytes, &MemoryConditionDisposition)> {
        match self {
            Self::ByteAligned => None,
            Self::Required {
                bytes,
                misaligned_non_empty,
            } => Some((*bytes, misaligned_non_empty)),
        }
    }

    fn referenced_exclusions(
        &self,
    ) -> impl Iterator<Item = &ContentId<MigrationDomainExclusionArtifact>> {
        self.misaligned_case()
            .and_then(|(_, disposition)| disposition.exclusion())
            .into_iter()
    }
}

/// Complete pointer and capacity contract for one buffer at a valid non-empty shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferMemoryContractV1 {
    null_non_empty: MemoryConditionDisposition,
    alignment: PointerAlignmentContractV1,
    insufficient_capacity_non_empty: MemoryConditionDisposition,
}

/// Constructor input for a buffer's non-empty memory surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferMemoryContractInput {
    /// Required behavior for a null address at a valid non-empty shape.
    pub null_non_empty: MemoryConditionDisposition,
    /// Address-alignment contract.
    pub alignment: PointerAlignmentContractV1,
    /// Required behavior when the allocation is shorter than the exact required bytes.
    pub insufficient_capacity_non_empty: MemoryConditionDisposition,
}

impl BufferMemoryContractV1 {
    /// Creates an explicit memory contract with no default error behavior.
    #[must_use]
    pub const fn new(input: BufferMemoryContractInput) -> Self {
        Self {
            null_non_empty: input.null_non_empty,
            alignment: input.alignment,
            insufficient_capacity_non_empty: input.insufficient_capacity_non_empty,
        }
    }

    /// Returns the null-pointer disposition for a non-empty buffer.
    #[must_use]
    pub const fn null_non_empty(&self) -> &MemoryConditionDisposition {
        &self.null_non_empty
    }

    /// Returns the pointer-alignment contract.
    #[must_use]
    pub const fn alignment(&self) -> &PointerAlignmentContractV1 {
        &self.alignment
    }

    /// Returns the insufficient-capacity disposition for a non-empty buffer.
    #[must_use]
    pub const fn insufficient_capacity_non_empty(&self) -> &MemoryConditionDisposition {
        &self.insufficient_capacity_non_empty
    }

    pub(crate) fn referenced_exclusions(
        &self,
    ) -> impl Iterator<Item = &ContentId<MigrationDomainExclusionArtifact>> {
        self.null_non_empty
            .exclusion()
            .into_iter()
            .chain(self.alignment.referenced_exclusions())
            .chain(self.insufficient_capacity_non_empty.exclusion())
    }
}

/// Canonically ordered pair of distinct buffer names.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "BufferPairWire")]
pub struct BufferPairV1 {
    first: BufferName,
    second: BufferName,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BufferPairWire {
    first: BufferName,
    second: BufferName,
}

impl BufferPairV1 {
    /// Creates a strictly name-ordered pair.
    ///
    /// # Errors
    ///
    /// Rejects equal or reverse-ordered names instead of silently canonicalizing wire input.
    pub fn new(first: BufferName, second: BufferName) -> Result<Self, DomainContractError> {
        if first >= second {
            return Err(DomainContractError::InvalidBufferPair);
        }
        Ok(Self { first, second })
    }

    /// Returns the lexicographically first buffer.
    #[must_use]
    pub const fn first(&self) -> &BufferName {
        &self.first
    }

    /// Returns the lexicographically second buffer.
    #[must_use]
    pub const fn second(&self) -> &BufferName {
        &self.second
    }
}

impl TryFrom<BufferPairWire> for BufferPairV1 {
    type Error = DomainContractError;

    fn try_from(wire: BufferPairWire) -> Result<Self, Self::Error> {
        Self::new(wire.first, wire.second)
    }
}

/// Constructor input for one pairwise aliasing declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferAliasingContractInput {
    /// Exact pair to which the declaration applies.
    pub pair: BufferPairV1,
    /// Required behavior when both arguments have the same base address.
    pub exact_alias: MemoryConditionDisposition,
    /// Required behavior when the two non-empty regions partially overlap.
    pub partial_overlap: MemoryConditionDisposition,
}

/// Explicit exact-alias and partial-overlap contract for one buffer pair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "BufferAliasingContractWire")]
pub struct BufferAliasingContractV1 {
    pair: BufferPairV1,
    exact_alias: MemoryConditionDisposition,
    partial_overlap: MemoryConditionDisposition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BufferAliasingContractWire {
    pair: BufferPairV1,
    exact_alias: MemoryConditionDisposition,
    partial_overlap: MemoryConditionDisposition,
}

impl BufferAliasingContractV1 {
    /// Creates one explicit pairwise aliasing declaration.
    #[must_use]
    pub fn new(input: BufferAliasingContractInput) -> Self {
        Self {
            pair: input.pair,
            exact_alias: input.exact_alias,
            partial_overlap: input.partial_overlap,
        }
    }

    /// Returns the exact buffer pair.
    #[must_use]
    pub const fn pair(&self) -> &BufferPairV1 {
        &self.pair
    }

    /// Returns the exact-alias disposition.
    #[must_use]
    pub const fn exact_alias(&self) -> &MemoryConditionDisposition {
        &self.exact_alias
    }

    /// Returns the partial-overlap disposition.
    #[must_use]
    pub const fn partial_overlap(&self) -> &MemoryConditionDisposition {
        &self.partial_overlap
    }

    pub(crate) fn referenced_exclusions(
        &self,
    ) -> impl Iterator<Item = &ContentId<MigrationDomainExclusionArtifact>> {
        self.exact_alias
            .exclusion()
            .into_iter()
            .chain(self.partial_overlap.exclusion())
    }
}

impl From<BufferAliasingContractWire> for BufferAliasingContractV1 {
    fn from(wire: BufferAliasingContractWire) -> Self {
        Self::new(BufferAliasingContractInput {
            pair: wire.pair,
            exact_alias: wire.exact_alias,
            partial_overlap: wire.partial_overlap,
        })
    }
}

/// Deterministic pointer/capacity recipe for one non-empty buffer.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BufferMemoryPattern {
    /// Pass a null address for a valid non-empty logical buffer.
    NullPointerNonEmpty,
    /// Offset a correctly aligned base address by a known non-zero amount below the requirement.
    MisalignedPointerNonEmpty {
        /// Alignment being violated.
        required_alignment: RequiredAlignmentBytes,
        /// Offset from the aligned address.
        offset: MisalignmentOffsetBytes,
    },
    /// Allocate exactly this many fewer bytes than the dtype/shape requires.
    InsufficientCapacityNonEmpty {
        /// Positive missing capacity.
        shortfall: CapacityShortfallBytes,
    },
}

/// Deterministic aliasing recipe for one pair of non-empty buffers.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BufferAliasingPattern {
    /// Give both arguments the same base address.
    ExactAlias,
    /// Offset the second region into the first by a positive byte count.
    PartialOverlap {
        /// Positive offset distinguishing this from exact aliasing.
        second_offset: PartialOverlapOffsetBytes,
    },
}

/// Strong target for one mandatory memory-surface obligation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MemorySurfaceCaseTarget {
    /// A condition applying to one buffer.
    Buffer {
        /// Buffer whose pointer/capacity is perturbed.
        buffer: BufferName,
        /// Exact perturbation recipe.
        pattern: BufferMemoryPattern,
    },
    /// A condition applying to a pair of buffers.
    Aliasing {
        /// Exact canonical buffer pair.
        pair: BufferPairV1,
        /// Exact overlap recipe.
        pattern: BufferAliasingPattern,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MemorySurfaceSchemaV1;

impl Serialize for MemorySurfaceSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for MemorySurfaceSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom("memory-surface schema version must be 1")),
        }
    }
}

/// One mandatory pointer/capacity/aliasing obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MandatoryMemorySurfaceCaseWire")]
pub struct MandatoryMemorySurfaceCaseV1 {
    schema_version: MemorySurfaceSchemaV1,
    target: MemorySurfaceCaseTarget,
    disposition: MemoryConditionDisposition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MandatoryMemorySurfaceCaseWire {
    schema_version: MemorySurfaceSchemaV1,
    target: MemorySurfaceCaseTarget,
    disposition: MemoryConditionDisposition,
}

impl MandatoryMemorySurfaceCaseV1 {
    fn new(
        target: MemorySurfaceCaseTarget,
        disposition: MemoryConditionDisposition,
    ) -> Result<Self, DomainContractError> {
        match &target {
            MemorySurfaceCaseTarget::Buffer {
                pattern:
                    BufferMemoryPattern::MisalignedPointerNonEmpty {
                        required_alignment,
                        offset,
                    },
                ..
            } => {
                if offset.get() != 1 || offset.get() >= required_alignment.get() {
                    return Err(DomainContractError::InvalidMisalignmentPattern);
                }
            }
            MemorySurfaceCaseTarget::Buffer {
                pattern: BufferMemoryPattern::InsufficientCapacityNonEmpty { shortfall },
                ..
            } if shortfall.get() != 1 => {
                return Err(DomainContractError::InvalidDerivedCase {
                    reason: "memory-surface capacity shortfall must be one byte",
                });
            }
            MemorySurfaceCaseTarget::Aliasing {
                pattern: BufferAliasingPattern::PartialOverlap { second_offset },
                ..
            } if second_offset.get() != 1 => {
                return Err(DomainContractError::InvalidDerivedCase {
                    reason: "memory-surface partial-overlap offset must be one byte",
                });
            }
            MemorySurfaceCaseTarget::Buffer {
                pattern:
                    BufferMemoryPattern::NullPointerNonEmpty
                    | BufferMemoryPattern::InsufficientCapacityNonEmpty { .. },
                ..
            }
            | MemorySurfaceCaseTarget::Aliasing {
                pattern:
                    BufferAliasingPattern::ExactAlias | BufferAliasingPattern::PartialOverlap { .. },
                ..
            } => {}
        }
        Ok(Self {
            schema_version: MemorySurfaceSchemaV1,
            target,
            disposition,
        })
    }

    /// Returns the exact memory perturbation target.
    #[must_use]
    pub const fn target(&self) -> &MemorySurfaceCaseTarget {
        &self.target
    }

    /// Returns the caller-declared condition disposition.
    #[must_use]
    pub const fn disposition(&self) -> &MemoryConditionDisposition {
        &self.disposition
    }
}

impl TryFrom<MandatoryMemorySurfaceCaseWire> for MandatoryMemorySurfaceCaseV1 {
    type Error = DomainContractError;

    fn try_from(wire: MandatoryMemorySurfaceCaseWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.target, wire.disposition)
    }
}

/// Content identity domain for one mandatory memory-surface obligation.
pub enum MandatoryMemorySurfaceCaseArtifact {}

impl ContentType for MandatoryMemorySurfaceCaseArtifact {
    const DOMAIN: &'static str = "migration.mandatory-memory-surface-case.v1";
}

/// Trusted algorithm used to derive memory-surface obligations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemorySurfaceDerivationPolicy {
    /// Non-empty null, misalignment, short-capacity, exact-alias, and partial-overlap policy.
    PointerAndAliasingV1,
}

/// Canonical memory-surface obligation set for one exact caller domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MandatoryMemorySurfaceCasesWire")]
pub struct MandatoryMemorySurfaceCasesV1 {
    schema_version: MemorySurfaceSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: MemorySurfaceDerivationPolicy,
    cases: Vec<MandatoryMemorySurfaceCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MandatoryMemorySurfaceCasesWire {
    schema_version: MemorySurfaceSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    derivation_policy: MemorySurfaceDerivationPolicy,
    cases: Vec<MandatoryMemorySurfaceCaseV1>,
}

impl MandatoryMemorySurfaceCasesV1 {
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        cases: Vec<MandatoryMemorySurfaceCaseV1>,
    ) -> Result<Self, DomainContractError> {
        if cases
            .windows(2)
            .any(|pair| pair[0].target() >= pair[1].target())
        {
            return Err(DomainContractError::NonCanonicalSet {
                field: "mandatory memory-surface cases",
            });
        }
        Ok(Self {
            schema_version: MemorySurfaceSchemaV1,
            domain,
            derivation_policy: MemorySurfaceDerivationPolicy::PointerAndAliasingV1,
            cases,
        })
    }

    /// Returns the caller-domain identity from which these obligations were derived.
    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    /// Returns the trusted derivation algorithm identity.
    #[must_use]
    pub const fn derivation_policy(&self) -> MemorySurfaceDerivationPolicy {
        self.derivation_policy
    }

    /// Returns obligations in strict typed-target order.
    #[must_use]
    pub fn cases(&self) -> &[MandatoryMemorySurfaceCaseV1] {
        &self.cases
    }
}

impl TryFrom<MandatoryMemorySurfaceCasesWire> for MandatoryMemorySurfaceCasesV1 {
    type Error = DomainContractError;

    fn try_from(wire: MandatoryMemorySurfaceCasesWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        if wire.derivation_policy != MemorySurfaceDerivationPolicy::PointerAndAliasingV1 {
            return Err(DomainContractError::InvalidDerivedCase {
                reason: "unexpected memory-surface derivation policy",
            });
        }
        Self::new(wire.domain, wire.cases)
    }
}

/// Content identity domain for a complete mandatory memory-surface set.
pub enum MandatoryMemorySurfaceCasesArtifact {}

impl ContentType for MandatoryMemorySurfaceCasesArtifact {
    const DOMAIN: &'static str = "migration.mandatory-memory-surface-cases.v1";
}

/// Derives typed non-empty pointer, capacity, and aliasing obligations from trusted code.
///
/// This stage records deterministic perturbation recipes. It does not dereference invalid pointers
/// or allocate overlapping memory; those effects belong to a later isolated case materializer and
/// execution boundary.
///
/// # Errors
///
/// Returns an error if canonical domain identity derivation fails or an internal recipe violates
/// its typed alignment invariant.
pub fn derive_mandatory_memory_surface_cases(
    contract: &MigrationDomainContractV1,
) -> Result<MandatoryMemorySurfaceCasesV1, DomainContractError> {
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
        if buffer_can_be_non_empty(contract, buffer)? {
            add_buffer_cases(buffer, &mut cases)?;
        }
    }
    for aliasing in contract.buffer_aliasing() {
        let first = find_buffer(contract, aliasing.pair().first())?;
        let second = find_buffer(contract, aliasing.pair().second())?;
        if buffer_can_be_non_empty(contract, first)? && buffer_can_be_non_empty(contract, second)? {
            let partial_overlap_applicable = buffer_can_exceed_one_byte(contract, first)?
                && buffer_can_exceed_one_byte(contract, second)?;
            add_aliasing_cases(aliasing, partial_overlap_applicable, &mut cases)?;
        }
    }
    cases.sort_by(|left, right| left.target().cmp(right.target()));
    MandatoryMemorySurfaceCasesV1::new(domain, cases)
}

fn add_buffer_cases(
    buffer: &BufferContractV1,
    cases: &mut Vec<MandatoryMemorySurfaceCaseV1>,
) -> Result<(), DomainContractError> {
    cases.push(MandatoryMemorySurfaceCaseV1::new(
        MemorySurfaceCaseTarget::Buffer {
            buffer: buffer.name().clone(),
            pattern: BufferMemoryPattern::NullPointerNonEmpty,
        },
        buffer.memory().null_non_empty().clone(),
    )?);
    if let Some((required_alignment, disposition)) = buffer.memory().alignment().misaligned_case() {
        cases.push(MandatoryMemorySurfaceCaseV1::new(
            MemorySurfaceCaseTarget::Buffer {
                buffer: buffer.name().clone(),
                pattern: BufferMemoryPattern::MisalignedPointerNonEmpty {
                    required_alignment,
                    offset: MisalignmentOffsetBytes::new(1)?,
                },
            },
            disposition.clone(),
        )?);
    }
    cases.push(MandatoryMemorySurfaceCaseV1::new(
        MemorySurfaceCaseTarget::Buffer {
            buffer: buffer.name().clone(),
            pattern: BufferMemoryPattern::InsufficientCapacityNonEmpty {
                shortfall: CapacityShortfallBytes::new(1)?,
            },
        },
        buffer.memory().insufficient_capacity_non_empty().clone(),
    )?);
    Ok(())
}

fn add_aliasing_cases(
    aliasing: &BufferAliasingContractV1,
    partial_overlap_applicable: bool,
    cases: &mut Vec<MandatoryMemorySurfaceCaseV1>,
) -> Result<(), DomainContractError> {
    cases.push(MandatoryMemorySurfaceCaseV1::new(
        MemorySurfaceCaseTarget::Aliasing {
            pair: aliasing.pair().clone(),
            pattern: BufferAliasingPattern::ExactAlias,
        },
        aliasing.exact_alias().clone(),
    )?);
    if partial_overlap_applicable {
        cases.push(MandatoryMemorySurfaceCaseV1::new(
            MemorySurfaceCaseTarget::Aliasing {
                pair: aliasing.pair().clone(),
                pattern: BufferAliasingPattern::PartialOverlap {
                    second_offset: PartialOverlapOffsetBytes::new(1)?,
                },
            },
            aliasing.partial_overlap().clone(),
        )?);
    }
    Ok(())
}

fn find_buffer<'a>(
    contract: &'a MigrationDomainContractV1,
    name: &BufferName,
) -> Result<&'a BufferContractV1, DomainContractError> {
    contract
        .buffers()
        .iter()
        .find(|buffer| buffer.name() == name)
        .ok_or_else(|| DomainContractError::UnknownAliasingBuffer {
            buffer: name.to_string(),
        })
}

fn buffer_can_be_non_empty(
    contract: &MigrationDomainContractV1,
    buffer: &BufferContractV1,
) -> Result<bool, DomainContractError> {
    for dimension in buffer.shape() {
        match dimension {
            DimensionSpec::Constant { extent } if extent.get() == 0 => return Ok(false),
            DimensionSpec::Constant { .. } => {}
            DimensionSpec::Symbol { symbol } => {
                let declared = contract
                    .shape_symbols()
                    .iter()
                    .find(|candidate| candidate.name() == symbol)
                    .ok_or_else(|| DomainContractError::UnknownShapeSymbol {
                        buffer: buffer.name().to_string(),
                        symbol: symbol.to_string(),
                    })?;
                if declared.valid_range().maximum().get() == 0 {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn buffer_can_exceed_one_byte(
    contract: &MigrationDomainContractV1,
    buffer: &BufferContractV1,
) -> Result<bool, DomainContractError> {
    if !buffer_can_be_non_empty(contract, buffer)? {
        return Ok(false);
    }
    if data_type_exceeds_one_byte(buffer.data_type()) {
        return Ok(true);
    }
    for dimension in buffer.shape() {
        let maximum = match dimension {
            DimensionSpec::Constant { extent } => extent.get(),
            DimensionSpec::Symbol { symbol } => contract
                .shape_symbols()
                .iter()
                .find(|candidate| candidate.name() == symbol)
                .ok_or_else(|| DomainContractError::UnknownShapeSymbol {
                    buffer: buffer.name().to_string(),
                    symbol: symbol.to_string(),
                })?
                .valid_range()
                .maximum()
                .get(),
        };
        if maximum > 1 {
            return Ok(true);
        }
    }
    Ok(false)
}

const fn data_type_exceeds_one_byte(data_type: DataType) -> bool {
    !matches!(data_type, DataType::I8 | DataType::U8 | DataType::Bool)
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, ContentType};
    use cairn_verification::CallerDomainBodyArtifact;

    use super::{
        BufferAliasingContractInput, BufferAliasingContractV1, BufferAliasingPattern,
        BufferMemoryContractInput, BufferMemoryContractV1, BufferMemoryPattern, BufferPairV1,
        CapacityShortfallBytes, MandatoryMemorySurfaceCasesV1, MemoryConditionDisposition,
        MemorySurfaceCaseTarget, MisalignmentOffsetBytes, PartialOverlapOffsetBytes,
        PointerAlignmentContractV1, RequiredAlignmentBytes, derive_mandatory_memory_surface_cases,
    };
    use crate::domain::{
        ArgumentIndex, BufferAccessV1, BufferContractInput, BufferContractV1, BufferName, DataType,
        DimensionAxis, DimensionSpec, DomainContractError, EntryPointName, ExtentValue,
        InclusiveExtentRange, InvalidInputBehavior, MigrationDomainContractInput,
        MigrationDomainContractV1, MigrationDomainExclusionArtifact, RequestedSemanticsArtifact,
        SemanticClaimKind, ShapeSymbolContractInput, ShapeSymbolContractV1, ShapeSymbolName,
        ShapeSymbolSource, StatusCode,
    };
    use crate::input_values::InputValueDomainV1;

    fn id<T: ContentType>(seed: &str) -> ContentId<T> {
        ContentId::derive(seed.as_bytes()).expect("identity")
    }

    fn status(value: i32) -> InvalidInputBehavior {
        InvalidInputBehavior::ReturnStatus {
            status: StatusCode::new(value),
        }
    }

    fn aligned_memory(short_capacity: MemoryConditionDisposition) -> BufferMemoryContractV1 {
        BufferMemoryContractV1::new(BufferMemoryContractInput {
            null_non_empty: MemoryConditionDisposition::Invalid {
                behavior: status(-1),
            },
            alignment: PointerAlignmentContractV1::Required {
                bytes: RequiredAlignmentBytes::new(16).expect("alignment"),
                misaligned_non_empty: MemoryConditionDisposition::Unknown,
            },
            insufficient_capacity_non_empty: short_capacity,
        })
    }

    fn byte_aligned_memory() -> BufferMemoryContractV1 {
        BufferMemoryContractV1::new(BufferMemoryContractInput {
            null_non_empty: MemoryConditionDisposition::Invalid {
                behavior: InvalidInputBehavior::RejectBeforeExecution,
            },
            alignment: PointerAlignmentContractV1::ByteAligned,
            insufficient_capacity_non_empty: MemoryConditionDisposition::Supported,
        })
    }

    fn aliasing(
        first: &str,
        second: &str,
        exact_alias: MemoryConditionDisposition,
        partial_overlap: MemoryConditionDisposition,
    ) -> BufferAliasingContractV1 {
        BufferAliasingContractV1::new(BufferAliasingContractInput {
            pair: BufferPairV1::new(
                BufferName::new(first).expect("first"),
                BufferName::new(second).expect("second"),
            )
            .expect("pair"),
            exact_alias,
            partial_overlap,
        })
    }

    fn memory_domain(
        short_capacity: MemoryConditionDisposition,
        exclusions: Vec<ContentId<MigrationDomainExclusionArtifact>>,
        pair_contracts: Option<Vec<BufferAliasingContractV1>>,
    ) -> Result<MigrationDomainContractV1, DomainContractError> {
        let symbol = ShapeSymbolName::new("n").expect("symbol");
        MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("copy_u32").expect("entry"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: BufferName::new("input").expect("buffer"),
                    access: BufferAccessV1::Input {
                        value_domain: InputValueDomainV1::UnsignedInteger,
                    },
                    data_type: DataType::U32,
                    shape: vec![DimensionSpec::Symbol {
                        symbol: symbol.clone(),
                    }],
                    memory: aligned_memory(short_capacity),
                })?,
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(1),
                    name: BufferName::new("output").expect("buffer"),
                    access: BufferAccessV1::Output,
                    data_type: DataType::U32,
                    shape: Vec::new(),
                    memory: byte_aligned_memory(),
                })?,
            ],
            scalar_parameters: Vec::new(),
            shape_symbols: vec![
                ShapeSymbolContractV1::new(ShapeSymbolContractInput {
                    name: symbol,
                    valid_range: InclusiveExtentRange::new(
                        ExtentValue::new(0),
                        ExtentValue::new(10),
                    )?,
                    source: ShapeSymbolSource::BufferDimension {
                        buffer: BufferName::new("input").expect("buffer"),
                        axis: DimensionAxis::new(0),
                    },
                    boundary_moduli: Vec::new(),
                    invalid_behavior: InvalidInputBehavior::RejectBeforeExecution,
                })
                .expect("buffer"),
            ],
            buffer_aliasing: pair_contracts.unwrap_or_else(|| {
                vec![aliasing(
                    "input",
                    "output",
                    MemoryConditionDisposition::Invalid {
                        behavior: status(-2),
                    },
                    MemoryConditionDisposition::Unknown,
                )]
            }),
            requested_semantics: id::<RequestedSemanticsArtifact>("copy-semantics"),
            semantic_claim: SemanticClaimKind::Exact,
            exclusions,
        })
    }

    #[test]
    fn trusted_derivation_covers_pointer_capacity_and_aliasing_surfaces() {
        let exclusion = id::<MigrationDomainExclusionArtifact>("capacity-exclusion");
        let domain = memory_domain(
            MemoryConditionDisposition::ExplicitlyExcluded { exclusion },
            vec![exclusion],
            None,
        )
        .expect("domain");
        let derived = derive_mandatory_memory_surface_cases(&domain).expect("derive");
        assert_eq!(derived.cases().len(), 7);
        assert!(
            derived
                .cases()
                .windows(2)
                .all(|pair| pair[0].target() < pair[1].target())
        );

        let input_short = derived
            .cases()
            .iter()
            .find(|case| {
                matches!(
                    case.target(),
                    MemorySurfaceCaseTarget::Buffer {
                        buffer,
                        pattern: BufferMemoryPattern::InsufficientCapacityNonEmpty { shortfall },
                    } if buffer.as_str() == "input" && shortfall.get() == 1
                )
            })
            .expect("short capacity");
        assert_eq!(
            input_short.disposition(),
            &MemoryConditionDisposition::ExplicitlyExcluded { exclusion }
        );
        assert!(derived.cases().iter().any(|case| {
            matches!(
                case.target(),
                MemorySurfaceCaseTarget::Buffer {
                    buffer,
                    pattern: BufferMemoryPattern::MisalignedPointerNonEmpty {
                        required_alignment,
                        offset,
                    },
                } if buffer.as_str() == "input"
                    && required_alignment.get() == 16
                    && offset.get() == 1
            ) && case.disposition() == &MemoryConditionDisposition::Unknown
        }));
        assert!(derived.cases().iter().any(|case| {
            matches!(
                case.target(),
                MemorySurfaceCaseTarget::Aliasing {
                    pattern: BufferAliasingPattern::ExactAlias,
                    ..
                }
            ) && case.disposition()
                == &MemoryConditionDisposition::Invalid {
                    behavior: status(-2),
                }
        }));
    }

    #[test]
    fn alignment_quantities_pairs_and_domain_edges_fail_closed() {
        assert!(RequiredAlignmentBytes::new(0).is_err());
        assert!(RequiredAlignmentBytes::new(1).is_err());
        assert!(RequiredAlignmentBytes::new(3).is_err());
        assert!(RequiredAlignmentBytes::new(32).is_ok());
        assert!(MisalignmentOffsetBytes::new(0).is_err());
        assert!(CapacityShortfallBytes::new(0).is_err());
        assert!(PartialOverlapOffsetBytes::new(0).is_err());
        assert!(
            BufferPairV1::new(
                BufferName::new("output").expect("buffer"),
                BufferName::new("input").expect("buffer"),
            )
            .is_err()
        );
        assert!(
            BufferPairV1::new(
                BufferName::new("input").expect("buffer"),
                BufferName::new("input").expect("buffer"),
            )
            .is_err()
        );

        assert!(matches!(
            memory_domain(
                MemoryConditionDisposition::Unknown,
                Vec::new(),
                Some(Vec::new())
            ),
            Err(DomainContractError::IncompleteAliasingContracts)
        ));
        let unknown_pair = aliasing(
            "input",
            "phantom",
            MemoryConditionDisposition::Unknown,
            MemoryConditionDisposition::Unknown,
        );
        assert!(matches!(
            memory_domain(
                MemoryConditionDisposition::Unknown,
                Vec::new(),
                Some(vec![unknown_pair]),
            ),
            Err(DomainContractError::UnknownAliasingBuffer { .. })
        ));

        let exclusion = id::<MigrationDomainExclusionArtifact>("missing-memory-exclusion");
        assert!(matches!(
            memory_domain(
                MemoryConditionDisposition::ExplicitlyExcluded { exclusion },
                Vec::new(),
                None,
            ),
            Err(DomainContractError::UnlistedMemorySurfaceExclusion)
        ));
    }

    #[test]
    fn persisted_memory_contracts_and_obligations_are_strict_v1() {
        let domain =
            memory_domain(MemoryConditionDisposition::Unknown, Vec::new(), None).expect("domain");
        let changed_domain = memory_domain(MemoryConditionDisposition::Supported, Vec::new(), None)
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

        let mut domain_value = serde_json::to_value(&domain).expect("domain json");
        domain_value["buffers"][0]["memory"]["legacy_alignment"] = serde_json::json!(16);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(domain_value).is_err());
        let mut domain_value = serde_json::to_value(&domain).expect("domain json");
        domain_value["buffer_aliasing"][0]["legacy_overlap"] = serde_json::json!(true);
        assert!(serde_json::from_value::<MigrationDomainContractV1>(domain_value).is_err());

        let derived = derive_mandatory_memory_surface_cases(&domain).expect("derive");
        let bytes = cairn_codec::to_vec(&derived).expect("bytes");
        assert_eq!(
            cairn_codec::from_slice::<MandatoryMemorySurfaceCasesV1>(&bytes).expect("round trip"),
            derived
        );

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<MandatoryMemorySurfaceCasesV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["legacy_pointer_policy"] = serde_json::json!("cuda-default");
        assert!(serde_json::from_value::<MandatoryMemorySurfaceCasesV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let case = value["cases"]
            .as_array_mut()
            .expect("cases")
            .iter_mut()
            .find(|case| {
                case["target"]["kind"] == "buffer"
                    && case["target"]["pattern"]["kind"] == "misaligned-pointer-non-empty"
            })
            .expect("misalignment case");
        case["target"]["pattern"]["offset"] = serde_json::json!(16);
        assert!(serde_json::from_value::<MandatoryMemorySurfaceCasesV1>(value).is_err());
    }

    #[test]
    fn zero_only_buffers_produce_no_non_empty_pointer_obligations() {
        let domain = MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("empty_only").expect("entry"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: BufferName::new("in_place").expect("buffer"),
                    access: BufferAccessV1::InputOutput {
                        value_domain: InputValueDomainV1::Boolean,
                    },
                    data_type: DataType::Bool,
                    shape: vec![DimensionSpec::Constant {
                        extent: ExtentValue::new(0),
                    }],
                    memory: byte_aligned_memory(),
                })
                .expect("buffer"),
            ],
            scalar_parameters: Vec::new(),
            shape_symbols: Vec::new(),
            buffer_aliasing: Vec::new(),
            requested_semantics: id::<RequestedSemanticsArtifact>("empty-semantics"),
            semantic_claim: SemanticClaimKind::Implicit,
            exclusions: Vec::new(),
        })
        .expect("domain");
        let cases = derive_mandatory_memory_surface_cases(&domain).expect("derive");
        assert!(cases.cases().is_empty());
    }

    #[test]
    fn one_byte_scalar_pairs_do_not_claim_a_partial_overlap_recipe() {
        let domain = MigrationDomainContractV1::new(MigrationDomainContractInput {
            source_entry_point: EntryPointName::new("bool_scalar_copy").expect("entry"),
            buffers: vec![
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(0),
                    name: BufferName::new("input").expect("buffer"),
                    access: BufferAccessV1::Input {
                        value_domain: InputValueDomainV1::Boolean,
                    },
                    data_type: DataType::Bool,
                    shape: Vec::new(),
                    memory: byte_aligned_memory(),
                })
                .expect("input"),
                BufferContractV1::new(BufferContractInput {
                    argument_index: ArgumentIndex::new(1),
                    name: BufferName::new("output").expect("buffer"),
                    access: BufferAccessV1::Output,
                    data_type: DataType::Bool,
                    shape: Vec::new(),
                    memory: byte_aligned_memory(),
                })
                .expect("output"),
            ],
            scalar_parameters: Vec::new(),
            shape_symbols: Vec::new(),
            buffer_aliasing: vec![aliasing(
                "input",
                "output",
                MemoryConditionDisposition::Unknown,
                MemoryConditionDisposition::Unknown,
            )],
            requested_semantics: id::<RequestedSemanticsArtifact>("bool-copy-semantics"),
            semantic_claim: SemanticClaimKind::Exact,
            exclusions: Vec::new(),
        })
        .expect("domain");
        let cases = derive_mandatory_memory_surface_cases(&domain).expect("derive");
        assert!(cases.cases().iter().any(|case| matches!(
            case.target(),
            MemorySurfaceCaseTarget::Aliasing {
                pattern: BufferAliasingPattern::ExactAlias,
                ..
            }
        )));
        assert!(!cases.cases().iter().any(|case| matches!(
            case.target(),
            MemorySurfaceCaseTarget::Aliasing {
                pattern: BufferAliasingPattern::PartialOverlap { .. },
                ..
            }
        )));
    }
}
