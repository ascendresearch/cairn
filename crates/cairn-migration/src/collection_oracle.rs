//! Contract-bound collection-output policy, ABI materialization, and observation semantics.

use cairn_execution::{
    DeclaredOutputArtifact, ExecutionReceiptArtifact, InputBundleArtifact, InputBundleEntry,
    InputBundleV1, InputFileMode, OutputName, SandboxPath,
};
use cairn_protocol::{ContentId, ContentType, SchemaVersion};
use cairn_record::ContentStore;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    ArgumentIndex, BufferName, CorpusBufferByteLength, CorpusElementCount, SirCallerClaimId,
};

const MAX_COLLECTION_ELEMENTS: usize = 1_048_576;
const ABI_DIRECTORY: &str = "cairn/abi";
const INVOCATION_PATH: &str = "cairn/invocation.json";
const INPUT_ARGUMENT: ArgumentIndex = ArgumentIndex::new(0);
const THRESHOLD_ARGUMENT: ArgumentIndex = ArgumentIndex::new(1);
const VALUES_OUTPUT_ARGUMENT: ArgumentIndex = ArgumentIndex::new(2);
const COUNT_OUTPUT_ARGUMENT: ArgumentIndex = ArgumentIndex::new(3);
const COLLECTION_ORACLE_MATERIALIZER_V1: &[u8] = include_bytes!("collection_oracle.rs");
const CALL_ADAPTER_PROTOCOL_V1: &[u8] = include_bytes!("call_adapter.rs");

/// First immutable admitted migration-intent contract identity.
pub enum MigrationIntentContractArtifact {}

impl ContentType for MigrationIntentContractArtifact {
    const DOMAIN: &'static str = "migration.intent-contract.v1";
}

/// Exact semantic identity of one collection element.
pub enum CollectionOracleElementArtifact {}

impl ContentType for CollectionOracleElementArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-element.v1";
}

/// Trusted expected collection identity.
pub enum ExpectedCollectionOracleOutputArtifact {}

impl ContentType for ExpectedCollectionOracleOutputArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-expected.v1";
}

/// Candidate observation identity.
pub enum ObservedCollectionOracleOutputArtifact {}

impl ContentType for ObservedCollectionOracleOutputArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-observed.v1";
}

/// Contract-bound comparator-decision identity.
pub enum CollectionOutputOracleDecisionArtifact {}

impl ContentType for CollectionOutputOracleDecisionArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-decision.v1";
}

/// Count reported by a collection-producing implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CollectionReportedCount(u32);

impl CollectionReportedCount {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Trusted expected collection produced by the selected Oracle reference.
///
/// A candidate observation cannot substitute for trusted expected values.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionReportedCount, ExpectedCollectionOracleOutputV1,
///     ObservedCollectionOracleOutputV1,
/// };
/// fn require_expected(_: ExpectedCollectionOracleOutputV1) {}
/// let observed = ObservedCollectionOracleOutputV1::new(
///     Vec::new(),
///     CollectionReportedCount::new(0),
/// ).unwrap();
/// require_expected(observed);
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExpectedCollectionOracleOutputV1 {
    schema_version: SchemaVersion,
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCollectionOracleOutputWire {
    schema_version: SchemaVersion,
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
}

impl ExpectedCollectionOracleOutputV1 {
    /// Creates a bounded trusted expected collection.
    ///
    /// # Errors
    ///
    /// Rejects expected collections exceeding the current-V1 element bound.
    pub fn new(
        elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    ) -> Result<Self, CollectionOutputOracleError> {
        validate_collection_bound(&elements)?;
        Ok(Self {
            schema_version: schema_v1(),
            elements,
        })
    }

    #[must_use]
    pub fn elements(&self) -> &[ContentId<CollectionOracleElementArtifact>] {
        &self.elements
    }

    /// Derives the exact expected-output identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<ExpectedCollectionOracleOutputArtifact>, CollectionOutputOracleError>
    {
        derive_id(self)
    }
}

impl TryFrom<ExpectedCollectionOracleOutputWire> for ExpectedCollectionOracleOutputV1 {
    type Error = CollectionOutputOracleError;

    fn try_from(wire: ExpectedCollectionOracleOutputWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1() {
            return Err(CollectionOutputOracleError::InvalidStructure(
                "expected collection schema",
            ));
        }
        Self::new(wire.elements)
    }
}

impl<'de> Deserialize<'de> for ExpectedCollectionOracleOutputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ExpectedCollectionOracleOutputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Candidate observation whose independently reported count may be wrong.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObservedCollectionOracleOutputV1 {
    schema_version: SchemaVersion,
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    reported_count: CollectionReportedCount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedCollectionOracleOutputWire {
    schema_version: SchemaVersion,
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    reported_count: CollectionReportedCount,
}

impl ObservedCollectionOracleOutputV1 {
    /// Creates a bounded candidate observation without trusting its reported count.
    ///
    /// # Errors
    ///
    /// Rejects observations exceeding the current-V1 element bound.
    pub fn new(
        elements: Vec<ContentId<CollectionOracleElementArtifact>>,
        reported_count: CollectionReportedCount,
    ) -> Result<Self, CollectionOutputOracleError> {
        validate_collection_bound(&elements)?;
        Ok(Self {
            schema_version: schema_v1(),
            elements,
            reported_count,
        })
    }

    #[must_use]
    pub fn elements(&self) -> &[ContentId<CollectionOracleElementArtifact>] {
        &self.elements
    }

    #[must_use]
    pub const fn reported_count(&self) -> CollectionReportedCount {
        self.reported_count
    }

    /// Derives the exact observation identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<ObservedCollectionOracleOutputArtifact>, CollectionOutputOracleError>
    {
        derive_id(self)
    }
}

impl TryFrom<ObservedCollectionOracleOutputWire> for ObservedCollectionOracleOutputV1 {
    type Error = CollectionOutputOracleError;

    fn try_from(wire: ObservedCollectionOracleOutputWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1() {
            return Err(CollectionOutputOracleError::InvalidStructure(
                "observed collection schema",
            ));
        }
        Self::new(wire.elements, wire.reported_count)
    }
}

impl<'de> Deserialize<'de> for ObservedCollectionOracleOutputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ObservedCollectionOracleOutputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Concrete comparator decision selected from an admitted contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOutputOraclePolicyV1 {
    ExactMultisetAndCount,
    ExactSequenceAndCount,
}

/// Explicit comparison result; no stored pass boolean erases the failure class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOutputComparisonV1 {
    Equivalent,
    ReportedCountMismatch,
    ElementMultisetMismatch,
    ElementSequenceMismatch,
}

/// Contract-bound Oracle decision. Its constructor does not grant admission authority; only the
/// Admission process may publish it from an admitted contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionOutputOracleDecisionV1 {
    schema_version: SchemaVersion,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    policy: CollectionOutputOraclePolicyV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionOutputOracleDecisionWire {
    schema_version: SchemaVersion,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    policy: CollectionOutputOraclePolicyV1,
}

impl CollectionOutputOracleDecisionV1 {
    #[must_use]
    pub fn new(
        contract: ContentId<MigrationIntentContractArtifact>,
        selection_claim: SirCallerClaimId,
        policy: CollectionOutputOraclePolicyV1,
    ) -> Self {
        Self {
            schema_version: schema_v1(),
            contract,
            selection_claim,
            policy,
        }
    }

    #[must_use]
    pub const fn policy(&self) -> CollectionOutputOraclePolicyV1 {
        self.policy
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    /// Derives the exact policy-decision identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CollectionOutputOracleDecisionArtifact>, CollectionOutputOracleError>
    {
        derive_id(self)
    }

    #[must_use]
    pub fn compare(
        &self,
        expected: &ExpectedCollectionOracleOutputV1,
        actual: &ObservedCollectionOracleOutputV1,
    ) -> CollectionOutputComparisonV1 {
        if u32::try_from(expected.elements.len()).unwrap_or(u32::MAX) != actual.reported_count.get()
        {
            return CollectionOutputComparisonV1::ReportedCountMismatch;
        }
        match self.policy {
            CollectionOutputOraclePolicyV1::ExactSequenceAndCount => {
                if expected.elements == actual.elements {
                    CollectionOutputComparisonV1::Equivalent
                } else {
                    CollectionOutputComparisonV1::ElementSequenceMismatch
                }
            }
            CollectionOutputOraclePolicyV1::ExactMultisetAndCount => {
                let mut expected_elements = expected.elements.clone();
                let mut actual_elements = actual.elements.clone();
                expected_elements.sort_by_key(ContentId::to_wire);
                actual_elements.sort_by_key(ContentId::to_wire);
                if expected_elements == actual_elements {
                    CollectionOutputComparisonV1::Equivalent
                } else {
                    CollectionOutputComparisonV1::ElementMultisetMismatch
                }
            }
        }
    }
}

impl TryFrom<CollectionOutputOracleDecisionWire> for CollectionOutputOracleDecisionV1 {
    type Error = CollectionOutputOracleError;

    fn try_from(wire: CollectionOutputOracleDecisionWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1() {
            return Err(CollectionOutputOracleError::InvalidStructure(
                "collection Oracle decision schema",
            ));
        }
        Ok(Self::new(wire.contract, wire.selection_claim, wire.policy))
    }
}

impl<'de> Deserialize<'de> for CollectionOutputOracleDecisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionOutputOracleDecisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact input bytes for the collection reference mechanism.
pub enum CollectionF32InputBytesArtifact {}

impl ContentType for CollectionF32InputBytesArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-f32-input-bytes.v1";
}

/// Exact threshold bytes for the collection reference mechanism.
pub enum CollectionF32ThresholdBytesArtifact {}

impl ContentType for CollectionF32ThresholdBytesArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-f32-threshold-bytes.v1";
}

/// Adapter-visible invocation for one contract-bound collection case.
pub enum CollectionF32InvocationArtifact {}

impl ContentType for CollectionF32InvocationArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-f32-invocation.v1";
}

/// Exact trusted implementation bytes that materialize and compare this first collection case.
pub enum CollectionOracleMechanismArtifact {}

impl ContentType for CollectionOracleMechanismArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-mechanism.v1";
}

/// Receipt-bound comparison evidence produced by the trusted collection mechanism.
pub enum CollectionOutputComparisonEvidenceArtifact {}

impl ContentType for CollectionOutputComparisonEvidenceArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-comparison-evidence.v1";
}

/// One finite, normal, nonzero IEEE-754 binary32 value accepted by this narrow mechanism.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionF32Bits(u32);

impl CollectionF32Bits {
    /// Creates one value in the mechanism's deliberately narrow numeric domain.
    ///
    /// # Errors
    ///
    /// Rejects zero, subnormal, infinite, and NaN representations.
    pub fn new(value: u32) -> Result<Self, CollectionOutputOracleError> {
        if f32::from_bits(value).is_normal() {
            Ok(Self(value))
        } else {
            Err(CollectionOutputOracleError::InvalidF32Domain)
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
    }
}

impl Serialize for CollectionF32Bits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for CollectionF32Bits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Read-only f32 input visible to the adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionF32InputBufferV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    element_count: CorpusElementCount,
    byte_length: CorpusBufferByteLength,
    path: SandboxPath,
    bytes: ContentId<CollectionF32InputBytesArtifact>,
}

impl CollectionF32InputBufferV1 {
    #[must_use]
    pub const fn element_count(&self) -> CorpusElementCount {
        self.element_count
    }

    #[must_use]
    pub const fn byte_length(&self) -> CorpusBufferByteLength {
        self.byte_length
    }

    #[must_use]
    pub const fn path(&self) -> &SandboxPath {
        &self.path
    }

    #[must_use]
    pub const fn bytes(&self) -> ContentId<CollectionF32InputBytesArtifact> {
        self.bytes
    }
}

/// Read-only scalar threshold visible to the adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionF32ThresholdV1 {
    argument_index: ArgumentIndex,
    bits: CollectionF32Bits,
    byte_length: CorpusBufferByteLength,
    path: SandboxPath,
    bytes: ContentId<CollectionF32ThresholdBytesArtifact>,
}

impl CollectionF32ThresholdV1 {
    #[must_use]
    pub const fn bits(&self) -> CollectionF32Bits {
        self.bits
    }

    #[must_use]
    pub const fn path(&self) -> &SandboxPath {
        &self.path
    }

    #[must_use]
    pub const fn bytes(&self) -> ContentId<CollectionF32ThresholdBytesArtifact> {
        self.bytes
    }
}

/// One adapter-owned output allocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionF32OutputBufferV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    byte_length: CorpusBufferByteLength,
}

impl CollectionF32OutputBufferV1 {
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

/// Strict adapter-visible invocation. Trusted expected elements are intentionally absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionF32InvocationV1 {
    schema_version: SchemaVersion,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    input: CollectionF32InputBufferV1,
    threshold: CollectionF32ThresholdV1,
    values_output: CollectionF32OutputBufferV1,
    count_output: CollectionF32OutputBufferV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionF32InvocationWire {
    schema_version: SchemaVersion,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    input: CollectionF32InputBufferV1,
    threshold: CollectionF32ThresholdV1,
    values_output: CollectionF32OutputBufferV1,
    count_output: CollectionF32OutputBufferV1,
}

impl CollectionF32InvocationV1 {
    #[must_use]
    pub const fn decision(&self) -> ContentId<CollectionOutputOracleDecisionArtifact> {
        self.decision
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub const fn input(&self) -> &CollectionF32InputBufferV1 {
        &self.input
    }

    #[must_use]
    pub const fn threshold(&self) -> &CollectionF32ThresholdV1 {
        &self.threshold
    }

    #[must_use]
    pub const fn values_output(&self) -> &CollectionF32OutputBufferV1 {
        &self.values_output
    }

    #[must_use]
    pub const fn count_output(&self) -> &CollectionF32OutputBufferV1 {
        &self.count_output
    }

    fn validate(&self) -> Result<(), CollectionOutputOracleError> {
        let input_count = self.input.element_count.get();
        let input_bytes = input_count
            .checked_mul(4)
            .ok_or(CollectionOutputOracleError::SizeOverflow)?;
        if self.schema_version != schema_v1()
            || usize::try_from(input_count).map_or(true, |count| count > MAX_COLLECTION_ELEMENTS)
            || self.input.argument_index != INPUT_ARGUMENT
            || self.threshold.argument_index != THRESHOLD_ARGUMENT
            || self.values_output.argument_index != VALUES_OUTPUT_ARGUMENT
            || self.count_output.argument_index != COUNT_OUTPUT_ARGUMENT
            || self.input.buffer != buffer_name("input")?
            || self.values_output.buffer != buffer_name("values")?
            || self.count_output.buffer != buffer_name("reported-count")?
            || self.input.byte_length != CorpusBufferByteLength::new(input_bytes)
            || self.threshold.byte_length != CorpusBufferByteLength::new(4)
            || self.values_output.byte_length != CorpusBufferByteLength::new(input_bytes)
            || self.count_output.byte_length != CorpusBufferByteLength::new(4)
            || self.input.path != argument_path(INPUT_ARGUMENT)?
            || self.threshold.path != argument_path(THRESHOLD_ARGUMENT)?
        {
            return Err(CollectionOutputOracleError::InvalidStructure(
                "collection f32 invocation",
            ));
        }
        Ok(())
    }
}

impl TryFrom<CollectionF32InvocationWire> for CollectionF32InvocationV1 {
    type Error = CollectionOutputOracleError;

    fn try_from(wire: CollectionF32InvocationWire) -> Result<Self, Self::Error> {
        let invocation = Self {
            schema_version: wire.schema_version,
            decision: wire.decision,
            contract: wire.contract,
            selection_claim: wire.selection_claim,
            input: wire.input,
            threshold: wire.threshold,
            values_output: wire.values_output,
            count_output: wire.count_output,
        };
        invocation.validate()?;
        Ok(invocation)
    }
}

impl<'de> Deserialize<'de> for CollectionF32InvocationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionF32InvocationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Complete transient case: candidate-visible material and separately held expected output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssembledCollectionF32OracleCaseInput {
    invocation: CollectionF32InvocationV1,
    invocation_id: ContentId<CollectionF32InvocationArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    expected: ExpectedCollectionOracleOutputV1,
}

impl AssembledCollectionF32OracleCaseInput {
    #[must_use]
    pub const fn invocation(&self) -> &CollectionF32InvocationV1 {
        &self.invocation
    }

    #[must_use]
    pub const fn invocation_id(&self) -> ContentId<CollectionF32InvocationArtifact> {
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
    pub const fn expected(&self) -> &ExpectedCollectionOracleOutputV1 {
        &self.expected
    }
}

/// Immutable facts from one receipt-bound collection comparison. This value is serialize-only;
/// persisted evidence must be checked by loading its cited contents and rerunning materialization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionOutputComparisonEvidenceV1 {
    schema_version: SchemaVersion,
    mechanism: ContentId<CollectionOracleMechanismArtifact>,
    decision: ContentId<CollectionOutputOracleDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    invocation: ContentId<CollectionF32InvocationArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    expected: ContentId<ExpectedCollectionOracleOutputArtifact>,
    observed: ContentId<ObservedCollectionOracleOutputArtifact>,
    comparison: CollectionOutputComparisonV1,
}

impl CollectionOutputComparisonEvidenceV1 {
    #[must_use]
    pub const fn comparison(&self) -> CollectionOutputComparisonV1 {
        self.comparison
    }
}

/// Canonical comparison evidence ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCollectionOutputComparisonEvidence {
    evidence: CollectionOutputComparisonEvidenceV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionOutputComparisonEvidenceArtifact>,
}

impl PreparedCollectionOutputComparisonEvidence {
    #[must_use]
    pub const fn evidence(&self) -> &CollectionOutputComparisonEvidenceV1 {
        &self.evidence
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionOutputComparisonEvidenceArtifact> {
        self.id
    }

    #[must_use]
    pub fn matches(&self) -> bool {
        self.evidence.comparison == CollectionOutputComparisonV1::Equivalent
    }
}

/// Materializes a generic finite-normal f32 threshold case from an admitted comparator decision.
///
/// Expected elements remain outside the candidate-visible bundle.
///
/// # Errors
///
/// Rejects an oversized case or canonical encoding, identity, and path failures.
pub fn assemble_collection_f32_oracle_case(
    decision: &CollectionOutputOracleDecisionV1,
    input: &[CollectionF32Bits],
    threshold: CollectionF32Bits,
) -> Result<AssembledCollectionF32OracleCaseInput, CollectionOutputOracleError> {
    validate_collection_bound(input)?;
    let input_count =
        u64::try_from(input.len()).map_err(|_| CollectionOutputOracleError::SizeOverflow)?;
    let input_bytes = encode_f32_bits(input);
    let threshold_bytes = threshold.get().to_le_bytes();
    let byte_length = input_count
        .checked_mul(4)
        .ok_or(CollectionOutputOracleError::SizeOverflow)?;
    let expected = ExpectedCollectionOracleOutputV1::new(
        input
            .iter()
            .copied()
            .filter(|value| value.value() > threshold.value())
            .map(collection_element_id)
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let invocation = CollectionF32InvocationV1 {
        schema_version: schema_v1(),
        decision: decision.identity()?,
        contract: decision.contract(),
        selection_claim: decision.selection_claim().clone(),
        input: CollectionF32InputBufferV1 {
            argument_index: INPUT_ARGUMENT,
            buffer: buffer_name("input")?,
            element_count: CorpusElementCount::new(input_count),
            byte_length: CorpusBufferByteLength::new(byte_length),
            path: argument_path(INPUT_ARGUMENT)?,
            bytes: ContentId::<CollectionF32InputBytesArtifact>::derive(&input_bytes)
                .map_err(codec)?,
        },
        threshold: CollectionF32ThresholdV1 {
            argument_index: THRESHOLD_ARGUMENT,
            bits: threshold,
            byte_length: CorpusBufferByteLength::new(4),
            path: argument_path(THRESHOLD_ARGUMENT)?,
            bytes: ContentId::<CollectionF32ThresholdBytesArtifact>::derive(&threshold_bytes)
                .map_err(codec)?,
        },
        values_output: CollectionF32OutputBufferV1 {
            argument_index: VALUES_OUTPUT_ARGUMENT,
            buffer: buffer_name("values")?,
            byte_length: CorpusBufferByteLength::new(byte_length),
        },
        count_output: CollectionF32OutputBufferV1 {
            argument_index: COUNT_OUTPUT_ARGUMENT,
            buffer: buffer_name("reported-count")?,
            byte_length: CorpusBufferByteLength::new(4),
        },
    };
    invocation.validate()?;
    let invocation_bytes = cairn_codec::to_vec(&invocation).map_err(codec)?;
    let invocation_id =
        ContentId::<CollectionF32InvocationArtifact>::derive(&invocation_bytes).map_err(codec)?;
    let input_bundle = InputBundleV1::new(vec![
        InputBundleEntry::Directory {
            path: sandbox_path("cairn")?,
        },
        InputBundleEntry::Directory {
            path: sandbox_path(ABI_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: argument_path(INPUT_ARGUMENT)?,
            mode: InputFileMode::Data,
            bytes: input_bytes,
        },
        InputBundleEntry::File {
            path: argument_path(THRESHOLD_ARGUMENT)?,
            mode: InputFileMode::Data,
            bytes: threshold_bytes.to_vec(),
        },
        InputBundleEntry::File {
            path: sandbox_path(INVOCATION_PATH)?,
            mode: InputFileMode::Data,
            bytes: invocation_bytes,
        },
    ])
    .map_err(codec)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(codec)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(codec)?;
    Ok(AssembledCollectionF32OracleCaseInput {
        invocation,
        invocation_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        expected,
    })
}

/// Returns the exact source identity of the materializer, adapter binding, and comparator.
///
/// # Errors
///
/// Returns an error only if the typed content identity cannot be derived.
pub fn collection_oracle_mechanism_id()
-> Result<ContentId<CollectionOracleMechanismArtifact>, CollectionOutputOracleError> {
    let mut bytes = Vec::with_capacity(
        COLLECTION_ORACLE_MATERIALIZER_V1.len() + CALL_ADAPTER_PROTOCOL_V1.len(),
    );
    bytes.extend_from_slice(COLLECTION_ORACLE_MATERIALIZER_V1);
    bytes.extend_from_slice(CALL_ADAPTER_PROTOCOL_V1);
    ContentId::derive(&bytes).map_err(codec)
}

/// Loads the two exact ABI outputs cited by a validated execution receipt, reconstructs the
/// reported collection prefix, and applies the admitted decision.
///
/// A raw adapter-reported result cannot substitute for receipt-bound execution authority.
///
/// ```compile_fail
/// use cairn_migration::{
///     AssembledCollectionF32OracleCaseInput, CallAdapterResultV1,
///     CollectionOutputOracleDecisionV1, materialize_collection_output_comparison,
/// };
/// use cairn_record::ContentStore;
/// fn invalid<C: ContentStore>(
///     case: &AssembledCollectionF32OracleCaseInput,
///     decision: &CollectionOutputOracleDecisionV1,
///     raw: &CallAdapterResultV1,
///     content: &C,
/// ) {
///     let _ = materialize_collection_output_comparison(case, decision, raw, content);
/// }
/// ```
///
/// # Errors
///
/// Rejects decision/case mismatch, missing or malformed receipt output, count over capacity, or
/// content/identity failure.
pub fn materialize_collection_output_comparison<C: ContentStore>(
    case: &AssembledCollectionF32OracleCaseInput,
    decision: &CollectionOutputOracleDecisionV1,
    execution: &crate::ValidatedCallAdapterExecution,
    content: &C,
) -> Result<PreparedCollectionOutputComparisonEvidence, CollectionOutputOracleError> {
    if decision.identity()? != case.invocation.decision
        || decision.contract() != case.invocation.contract
        || decision.selection_claim() != &case.invocation.selection_claim
    {
        return Err(CollectionOutputOracleError::BindingMismatch);
    }
    let values = read_argument_output(
        execution,
        content,
        case.invocation.values_output.argument_index,
    )?;
    let count_bytes = read_argument_output(
        execution,
        content,
        case.invocation.count_output.argument_index,
    )?;
    if values.len()
        != usize::try_from(case.invocation.values_output.byte_length.get())
            .map_err(|_| CollectionOutputOracleError::SizeOverflow)?
        || count_bytes.len() != 4
    {
        return Err(CollectionOutputOracleError::ObservedOutputLengthMismatch);
    }
    let observed =
        observe_collection_output(&values, &count_bytes, case.invocation.input.element_count)?;
    let evidence = CollectionOutputComparisonEvidenceV1 {
        schema_version: schema_v1(),
        mechanism: collection_oracle_mechanism_id()?,
        decision: case.invocation.decision,
        contract: case.invocation.contract,
        selection_claim: case.invocation.selection_claim.clone(),
        invocation: case.invocation_id,
        receipt: execution.receipt_id(),
        expected: case.expected.identity()?,
        observed: observed.identity()?,
        comparison: decision.compare(&case.expected, &observed),
    };
    let bytes = cairn_codec::to_vec(&evidence).map_err(codec)?;
    let id =
        ContentId::<CollectionOutputComparisonEvidenceArtifact>::derive(&bytes).map_err(codec)?;
    Ok(PreparedCollectionOutputComparisonEvidence {
        evidence,
        bytes,
        id,
    })
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CollectionOutputOracleError {
    #[error("invalid collection-output Oracle structure: {0}")]
    InvalidStructure(&'static str),
    #[error("collection f32 case accepts only finite, normal, nonzero values")]
    InvalidF32Domain,
    #[error("collection-output Oracle size arithmetic overflow")]
    SizeOverflow,
    #[error("collection-output Oracle decision and invocation binding mismatch")]
    BindingMismatch,
    #[error("reported collection count exceeds the output capacity")]
    ReportedCountExceedsCapacity,
    #[error("observed collection ABI output length is inconsistent")]
    ObservedOutputLengthMismatch,
    #[error("collection-output receipt content could not be loaded: {0}")]
    Content(String),
    #[error("collection-output Oracle codec error: {0}")]
    Codec(String),
}

fn read_argument_output<C: ContentStore>(
    execution: &crate::ValidatedCallAdapterExecution,
    content: &C,
    argument: ArgumentIndex,
) -> Result<Vec<u8>, CollectionOutputOracleError> {
    let name = OutputName::new(format!("abi-output-{:05}", argument.get())).map_err(codec)?;
    let archived = execution
        .receipt()
        .outputs()
        .iter()
        .find(|output| output.name == name)
        .ok_or(CollectionOutputOracleError::ObservedOutputLengthMismatch)?;
    let mut bytes = Vec::new();
    content
        .write_to::<DeclaredOutputArtifact>(&archived.content_id, &mut bytes)
        .map_err(|error| CollectionOutputOracleError::Content(error.to_string()))?;
    Ok(bytes)
}

fn observe_collection_output(
    values: &[u8],
    count_bytes: &[u8],
    capacity: CorpusElementCount,
) -> Result<ObservedCollectionOracleOutputV1, CollectionOutputOracleError> {
    if count_bytes.len() != 4 || u64::try_from(values.len()).ok() != capacity.get().checked_mul(4) {
        return Err(CollectionOutputOracleError::ObservedOutputLengthMismatch);
    }
    let reported_count = u32::from_le_bytes(
        <[u8; 4]>::try_from(count_bytes)
            .map_err(|_| CollectionOutputOracleError::ObservedOutputLengthMismatch)?,
    );
    if u64::from(reported_count) > capacity.get() {
        return Err(CollectionOutputOracleError::ReportedCountExceedsCapacity);
    }
    let prefix_length = usize::try_from(
        u64::from(reported_count)
            .checked_mul(4)
            .ok_or(CollectionOutputOracleError::SizeOverflow)?,
    )
    .map_err(|_| CollectionOutputOracleError::SizeOverflow)?;
    ObservedCollectionOracleOutputV1::new(
        values[..prefix_length]
            .chunks_exact(4)
            .map(collection_element_id_from_bytes)
            .collect::<Result<Vec<_>, _>>()?,
        CollectionReportedCount::new(reported_count),
    )
}

fn encode_f32_bits(values: &[CollectionF32Bits]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.get().to_le_bytes())
        .collect()
}

fn collection_element_id(
    value: CollectionF32Bits,
) -> Result<ContentId<CollectionOracleElementArtifact>, CollectionOutputOracleError> {
    ContentId::derive(&value.get().to_le_bytes()).map_err(codec)
}

fn collection_element_id_from_bytes(
    bytes: &[u8],
) -> Result<ContentId<CollectionOracleElementArtifact>, CollectionOutputOracleError> {
    if bytes.len() != 4 {
        return Err(CollectionOutputOracleError::ObservedOutputLengthMismatch);
    }
    ContentId::derive(bytes).map_err(codec)
}

fn argument_path(index: ArgumentIndex) -> Result<SandboxPath, CollectionOutputOracleError> {
    sandbox_path(&format!("{ABI_DIRECTORY}/arg-{:05}.bin", index.get()))
}

fn sandbox_path(value: &str) -> Result<SandboxPath, CollectionOutputOracleError> {
    SandboxPath::new(value).map_err(codec)
}

fn buffer_name(value: &str) -> Result<BufferName, CollectionOutputOracleError> {
    BufferName::new(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CollectionOutputOracleError {
    CollectionOutputOracleError::Codec(error.to_string())
}

fn validate_collection_bound<T>(values: &[T]) -> Result<(), CollectionOutputOracleError> {
    if values.len() > MAX_COLLECTION_ELEMENTS {
        return Err(CollectionOutputOracleError::InvalidStructure(
            "collection element count",
        ));
    }
    Ok(())
}

fn derive_id<T: ContentType>(
    value: &impl Serialize,
) -> Result<ContentId<T>, CollectionOutputOracleError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| CollectionOutputOracleError::Codec(error.to_string()))?;
    ContentId::derive(&bytes).map_err(|error| CollectionOutputOracleError::Codec(error.to_string()))
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid non-zero schema version")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(value: f32) -> CollectionF32Bits {
        CollectionF32Bits::new(value.to_bits()).expect("finite normal test value")
    }

    fn decision(policy: CollectionOutputOraclePolicyV1) -> CollectionOutputOracleDecisionV1 {
        CollectionOutputOracleDecisionV1::new(
            ContentId::<MigrationIntentContractArtifact>::derive(b"generic contract")
                .expect("contract"),
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            policy,
        )
    }

    fn element(value: f32) -> ContentId<CollectionOracleElementArtifact> {
        ContentId::derive(&value.to_bits().to_le_bytes()).expect("element")
    }

    #[test]
    fn assembles_without_expected_leakage_and_rejects_ambiguous_f32_values() {
        let case = assemble_collection_f32_oracle_case(
            &decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount),
            &[bits(1.0), bits(4.0), bits(3.0), bits(2.0)],
            bits(2.0),
        )
        .expect("assembled collection case");
        assert_eq!(case.expected.elements(), &[element(4.0), element(3.0)]);
        let invocation = serde_json::to_string(case.invocation()).expect("invocation JSON");
        assert!(!invocation.contains("expected"));
        assert!(!invocation.contains(&case.expected.identity().expect("expected id").to_wire()));
        assert_eq!(
            CollectionF32Bits::new(0.0_f32.to_bits()),
            Err(CollectionOutputOracleError::InvalidF32Domain)
        );
        assert_eq!(
            CollectionF32Bits::new(f32::NAN.to_bits()),
            Err(CollectionOutputOracleError::InvalidF32Domain)
        );
        assert_eq!(
            CollectionF32Bits::new(1_u32),
            Err(CollectionOutputOracleError::InvalidF32Domain)
        );
    }

    #[test]
    fn comparator_controls_preserve_multiplicity_count_and_order_policy() {
        let expected =
            ExpectedCollectionOracleOutputV1::new(vec![element(4.0), element(3.0), element(3.0)])
                .expect("expected");
        let reordered = ObservedCollectionOracleOutputV1::new(
            vec![element(3.0), element(4.0), element(3.0)],
            CollectionReportedCount::new(3),
        )
        .expect("reordered");
        assert_eq!(
            decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount)
                .compare(&expected, &reordered),
            CollectionOutputComparisonV1::Equivalent
        );
        assert_eq!(
            decision(CollectionOutputOraclePolicyV1::ExactSequenceAndCount)
                .compare(&expected, &reordered),
            CollectionOutputComparisonV1::ElementSequenceMismatch
        );

        let missing = ObservedCollectionOracleOutputV1::new(
            vec![element(4.0), element(3.0)],
            CollectionReportedCount::new(2),
        )
        .expect("missing");
        assert_eq!(
            decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount)
                .compare(&expected, &missing),
            CollectionOutputComparisonV1::ReportedCountMismatch
        );
        let duplicate = ObservedCollectionOracleOutputV1::new(
            vec![element(4.0), element(4.0), element(3.0)],
            CollectionReportedCount::new(3),
        )
        .expect("duplicate");
        assert_eq!(
            decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount)
                .compare(&expected, &duplicate),
            CollectionOutputComparisonV1::ElementMultisetMismatch
        );
        let wrong = ObservedCollectionOracleOutputV1::new(
            vec![element(4.0), element(3.0), element(5.0)],
            CollectionReportedCount::new(3),
        )
        .expect("wrong");
        assert_eq!(
            decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount)
                .compare(&expected, &wrong),
            CollectionOutputComparisonV1::ElementMultisetMismatch
        );
    }

    #[test]
    fn observation_uses_reported_prefix_and_rejects_count_over_capacity() {
        let values = [3.0_f32, 4.0, 99.0]
            .into_iter()
            .flat_map(|value| value.to_bits().to_le_bytes())
            .collect::<Vec<_>>();
        let observed =
            observe_collection_output(&values, &2_u32.to_le_bytes(), CorpusElementCount::new(3))
                .expect("reported prefix");
        assert_eq!(observed.elements(), &[element(3.0), element(4.0)]);
        assert_eq!(
            observe_collection_output(&values, &4_u32.to_le_bytes(), CorpusElementCount::new(3),),
            Err(CollectionOutputOracleError::ReportedCountExceedsCapacity)
        );
    }

    #[test]
    fn persisted_v1_revalidates_schema_numeric_and_cross_field_invariants() {
        let case = assemble_collection_f32_oracle_case(
            &decision(CollectionOutputOraclePolicyV1::ExactMultisetAndCount),
            &[bits(1.0), bits(3.0)],
            bits(2.0),
        )
        .expect("case");
        let mut value = serde_json::to_value(case.invocation()).expect("invocation JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<CollectionF32InvocationV1>(value).is_err());

        let mut value = serde_json::to_value(case.invocation()).expect("invocation JSON");
        value["threshold"]["bits"] = serde_json::json!(0);
        assert!(serde_json::from_value::<CollectionF32InvocationV1>(value).is_err());

        let mut value = serde_json::to_value(case.invocation()).expect("invocation JSON");
        value["values_output"]["byte_length"] = serde_json::json!(4);
        assert!(serde_json::from_value::<CollectionF32InvocationV1>(value).is_err());

        let mut value = serde_json::to_value(case.invocation()).expect("invocation JSON");
        value["legacy_expected"] = serde_json::json!([]);
        assert!(serde_json::from_value::<CollectionF32InvocationV1>(value).is_err());
    }
}
