//! Hardware-free historical reduction execution and numerical-comparison control.

use std::{fmt, str::FromStr};

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
    AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyArtifact, AdmissionPolicyV1,
    AllowanceClaimClass, AllowanceProvenance, CallerDomainBodyArtifact, ConstructionClaimArtifact,
    ConstructionClaimV1, ConstructionClassName, CorpusProposalArtifact, CorpusProposalV1,
    DeclaredDomainArtifact, DeclaredDomainV1, FaultClassName, FaultInjectionEvidenceArtifact,
    ImplementationBundleArtifact, ImplementationVariantArtifact, ImplementationVariantV1,
    MutationGridCellV1, MutationGridProofArtifact, MutationGridProofV1, NumericalAllowanceArtifact,
    NumericalAllowanceV1, OracleProposalArtifact, OracleProposalV1, OracleStrength,
    PreparedGenericMutantSet, PreparedMutationGrid, ReferenceArtifact, VariantExpectation,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CallAdapterExecutableArtifact, HistoricalDetectionRequirement, HistoricalFailureCoverageV1,
    HistoricalFailureObligationArtifact, HistoricalFailureObligationV1, HistoricalFailureRecordV1,
    MigrationDomainContractV1, MigrationExecutionNeed, MigrationValidationTier,
    ValidatedVariantBuild,
};

const REDUCTION_DIRECTORY: &str = "cairn";
const REDUCTION_EXECUTABLE_PATH: &str = "cairn/reduction-adapter";
const REDUCTION_CORPUS_PATH: &str = "cairn/reduction-corpus.json";
const REDUCTION_OUTPUT_PATH: &str = "cairn/reduction-observation.json";
const REDUCTION_OUTPUT_NAME: &str = "reduction-observation";
const WORKING_DIRECTORY: &str = "work";
const CONTAINER_CORPUS_PATH: &str = "/cairn/input/cairn/reduction-corpus.json";
const CONTAINER_OUTPUT_PATH: &str = "/cairn/output/cairn/reduction-observation.json";

/// Failure to execute or validate the hardware-free historical reduction control.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum HistoricalReductionControlError {
    /// Only the current pre-release V1 schema is accepted.
    #[error("historical reduction schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A persisted floating-point word was non-finite.
    #[error("historical reduction values must be finite f32 bit patterns")]
    NonFiniteValue,
    /// The reduction corpus was empty, duplicated, or non-canonical.
    #[error("historical reduction corpus is inconsistent")]
    InconsistentCorpus,
    /// An executable or output byte limit was zero or exceeded.
    #[error("historical reduction {field} bytes are empty or exceed their limit")]
    InvalidBytes { field: &'static str },
    /// A variant build did not match the exact variant or executable under execution.
    #[error("historical reduction variant build is inconsistent")]
    InconsistentVariantBuild,
    /// Prepared job material or a persisted execution plan was contradictory.
    #[error("historical reduction execution plan is inconsistent")]
    InconsistentExecutionPlan,
    /// Generic execution did not produce the exact authoritative observation receipt.
    #[error("historical reduction execution receipt is inconsistent")]
    InconsistentExecutionReceipt,
    /// Fixture output was not the trusted recomputation for its corpus and algorithm.
    #[error("historical reduction observation is inconsistent")]
    InconsistentObservation,
    /// Proposal, caller domain, corpus, reference, or historical obligation did not form one graph.
    #[error("historical reduction proposal graph is inconsistent")]
    InconsistentProposalGraph,
    /// Admission policy could not authorize the supplied family or execution path.
    #[error("historical reduction admission policy is unsatisfied")]
    UnsatisfiedPolicy,
    /// The numerical allowance was unmeasured, mismatched, or insufficient for admission.
    #[error("historical reduction numerical allowance is inadmissible")]
    InadmissibleAllowance,
    /// Correct/wrong variant evidence was incomplete, relabeled, or contradicted its observations.
    #[error("historical reduction variant controls are inconsistent")]
    InconsistentVariantControls,
    /// The old single-sample rule did not reproduce the historical false rejection.
    #[error("historical reduction false reject was not reproduced")]
    FalseRejectNotReproduced,
    /// Mutation proof failed or did not retain a case-dependent accumulation blind spot.
    #[error("historical reduction mutation control is insufficient")]
    InsufficientMutationControl,
    /// Persisted control facts were not the exact trusted recomputation.
    #[error("historical reduction control artifact is inconsistent")]
    InconsistentControl,
    /// Candidate execution did not remain inside the admitted oracle graph and environment.
    #[error("historical reduction candidate is outside the admitted oracle scope")]
    CandidateOutsideAdmission,
    /// Candidate comparison facts differed from exact reference/candidate output bits.
    #[error("historical reduction candidate comparison is inconsistent")]
    InconsistentCandidateComparison,
    /// Content storage failed while loading a declared observation.
    #[error("historical reduction content error: {message}")]
    Content { message: String },
    /// Canonical encoding, identity derivation, or generic contract construction failed.
    #[error("historical reduction composition error: {message}")]
    Composition { message: String },
}

/// Exact finite IEEE-754 binary32 bits; raw floats never enter canonical JSON.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct FiniteF32Bits(u32);

impl FiniteF32Bits {
    /// Creates an exact finite binary32 value from its wire bits.
    ///
    /// # Errors
    ///
    /// Rejects infinities and NaNs. Signed zero and subnormals remain distinct exact values.
    pub fn new(bits: u32) -> Result<Self, HistoricalReductionControlError> {
        if !f32::from_bits(bits).is_finite() {
            return Err(HistoricalReductionControlError::NonFiniteValue);
        }
        Ok(Self(bits))
    }

    /// Captures one finite host binary32 result as exact bits.
    ///
    /// # Errors
    ///
    /// Rejects a non-finite result.
    pub fn from_f32(value: f32) -> Result<Self, HistoricalReductionControlError> {
        Self::new(value.to_bits())
    }

    /// Returns the exact IEEE-754 bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    fn value(self) -> f32 {
        f32::from_bits(self.0)
    }
}

impl TryFrom<u32> for FiniteF32Bits {
    type Error = HistoricalReductionControlError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FiniteF32Bits> for u32 {
    fn from(value: FiniteF32Bits) -> Self {
        value.0
    }
}

/// Exact ULP distance between two finite binary32 observations.
///
/// ```compile_fail
/// use cairn_migration::{FiniteF32Bits, ReductionUlpDistance};
///
/// fn require_distance(_: ReductionUlpDistance) {}
/// let bits = FiniteF32Bits::new(0).unwrap();
/// require_distance(bits);
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReductionUlpDistance(u32);

impl ReductionUlpDistance {
    /// Creates an exact unsigned ULP count.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Computes monotonic IEEE-754 representation distance without floating-point subtraction.
    #[must_use]
    pub fn between(left: FiniteF32Bits, right: FiniteF32Bits) -> Self {
        let left = ordered_f32_bits(left.bits());
        let right = ordered_f32_bits(right.bits());
        Self(left.abs_diff(right))
    }

    /// Returns the exact ULP count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

fn ordered_f32_bits(bits: u32) -> u32 {
    if bits & 0x8000_0000 == 0 {
        bits | 0x8000_0000
    } else {
        !bits
    }
}

/// Executable reduction implementation selected by the offline control.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalReductionAlgorithm {
    /// Accumulate in binary64 and round once to binary32.
    HighPrecisionReference,
    /// Accumulate in ABI order using binary32 rounding after every addition.
    Sequential,
    /// Accumulate pairwise as a balanced binary32 tree.
    BalancedTree,
    /// Deliberately return zero.
    ZeroOutput,
    /// Deliberately omit the final input element.
    DropLast,
    /// Deliberately add one to the sequential result.
    UnitOffset,
}

impl HistoricalReductionAlgorithm {
    /// Returns the stable CLI/wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HighPrecisionReference => "high-precision-reference",
            Self::Sequential => "sequential",
            Self::BalancedTree => "balanced-tree",
            Self::ZeroOutput => "zero-output",
            Self::DropLast => "drop-last",
            Self::UnitOffset => "unit-offset",
        }
    }
}

impl fmt::Display for HistoricalReductionAlgorithm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for HistoricalReductionAlgorithm {
    type Err = HistoricalReductionControlError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "high-precision-reference" => Ok(Self::HighPrecisionReference),
            "sequential" => Ok(Self::Sequential),
            "balanced-tree" => Ok(Self::BalancedTree),
            "zero-output" => Ok(Self::ZeroOutput),
            "drop-last" => Ok(Self::DropLast),
            "unit-offset" => Ok(Self::UnitOffset),
            _ => Err(HistoricalReductionControlError::InconsistentExecutionPlan),
        }
    }
}

/// Content domain for one exact historical reduction input case.
pub enum HistoricalReductionCaseArtifact {}

impl ContentType for HistoricalReductionCaseArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-case.v1";
}

/// One non-empty finite binary32 reduction input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionCaseWire")]
pub struct HistoricalReductionCaseV1 {
    schema_version: u16,
    inputs: Vec<FiniteF32Bits>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionCaseWire {
    schema_version: u16,
    inputs: Vec<FiniteF32Bits>,
}

impl HistoricalReductionCaseV1 {
    /// Creates one non-empty finite reduction case.
    ///
    /// # Errors
    ///
    /// Rejects an empty input sequence.
    pub fn new(inputs: Vec<FiniteF32Bits>) -> Result<Self, HistoricalReductionControlError> {
        if inputs.is_empty() {
            return Err(HistoricalReductionControlError::InconsistentCorpus);
        }
        Ok(Self {
            schema_version: 1,
            inputs,
        })
    }

    /// Returns inputs in their semantically significant accumulation order.
    #[must_use]
    pub fn inputs(&self) -> &[FiniteF32Bits] {
        &self.inputs
    }
}

impl TryFrom<HistoricalReductionCaseWire> for HistoricalReductionCaseV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionCaseWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        Self::new(wire.inputs)
    }
}

/// One canonical case entry with its independently derived case identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReductionCaseEntryV1 {
    case: ContentId<HistoricalReductionCaseArtifact>,
    body: HistoricalReductionCaseV1,
}

impl HistoricalReductionCaseEntryV1 {
    fn prepare(body: HistoricalReductionCaseV1) -> Result<Self, HistoricalReductionControlError> {
        let bytes = cairn_codec::to_vec(&body).map_err(composition)?;
        let case =
            ContentId::<HistoricalReductionCaseArtifact>::derive(&bytes).map_err(composition)?;
        Ok(Self { case, body })
    }

    fn validate(&self) -> Result<(), HistoricalReductionControlError> {
        if Self::prepare(self.body.clone())? != *self {
            return Err(HistoricalReductionControlError::InconsistentCorpus);
        }
        Ok(())
    }

    /// Returns the exact input-case identity.
    #[must_use]
    pub const fn case(&self) -> ContentId<HistoricalReductionCaseArtifact> {
        self.case
    }

    /// Returns the exact input sequence.
    #[must_use]
    pub const fn body(&self) -> &HistoricalReductionCaseV1 {
        &self.body
    }
}

/// Content domain for the ordinary proposal-bound historical reduction corpus.
pub enum HistoricalReductionCorpusArtifact {}

impl ContentType for HistoricalReductionCorpusArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-corpus.v1";
}

/// Strict V1 hardware-free reduction corpus.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionCorpusWire")]
pub struct HistoricalReductionCorpusV1 {
    schema_version: u16,
    proposal: ContentId<cairn_verification::CorpusProposalArtifact>,
    cases: Vec<HistoricalReductionCaseEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionCorpusWire {
    schema_version: u16,
    proposal: ContentId<cairn_verification::CorpusProposalArtifact>,
    cases: Vec<HistoricalReductionCaseEntryV1>,
}

impl HistoricalReductionCorpusV1 {
    fn new(
        proposal: ContentId<cairn_verification::CorpusProposalArtifact>,
        cases: Vec<HistoricalReductionCaseEntryV1>,
    ) -> Result<Self, HistoricalReductionControlError> {
        if cases.is_empty()
            || cases
                .windows(2)
                .any(|pair| pair[0].case.to_wire() >= pair[1].case.to_wire())
            || cases.iter().any(|case| case.validate().is_err())
        {
            return Err(HistoricalReductionControlError::InconsistentCorpus);
        }
        Ok(Self {
            schema_version: 1,
            proposal,
            cases,
        })
    }

    /// Returns the ordinary proposal corpus identity this executable corpus realizes.
    #[must_use]
    pub const fn proposal(&self) -> ContentId<cairn_verification::CorpusProposalArtifact> {
        self.proposal
    }

    /// Returns exact cases in canonical identity order.
    #[must_use]
    pub fn cases(&self) -> &[HistoricalReductionCaseEntryV1] {
        &self.cases
    }
}

impl TryFrom<HistoricalReductionCorpusWire> for HistoricalReductionCorpusV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionCorpusWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        Self::new(wire.proposal, wire.cases)
    }
}

/// Canonical executable reduction corpus ready for archival and jobs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionCorpus {
    corpus: HistoricalReductionCorpusV1,
    corpus_bytes: Vec<u8>,
    corpus_id: ContentId<HistoricalReductionCorpusArtifact>,
}

impl PreparedHistoricalReductionCorpus {
    #[must_use]
    pub const fn corpus(&self) -> &HistoricalReductionCorpusV1 {
        &self.corpus
    }

    #[must_use]
    pub fn corpus_bytes(&self) -> &[u8] {
        &self.corpus_bytes
    }

    #[must_use]
    pub const fn corpus_id(&self) -> ContentId<HistoricalReductionCorpusArtifact> {
        self.corpus_id
    }
}

/// Prepares an input-order-independent corpus manifest while preserving order inside each case.
///
/// # Errors
///
/// Rejects an empty corpus, duplicated cases, or canonical encoding failure.
pub fn prepare_historical_reduction_corpus(
    proposal: ContentId<cairn_verification::CorpusProposalArtifact>,
    cases: Vec<HistoricalReductionCaseV1>,
) -> Result<PreparedHistoricalReductionCorpus, HistoricalReductionControlError> {
    let mut cases = cases
        .into_iter()
        .map(HistoricalReductionCaseEntryV1::prepare)
        .collect::<Result<Vec<_>, _>>()?;
    cases.sort_by_key(|case| case.case.to_wire());
    let corpus = HistoricalReductionCorpusV1::new(proposal, cases)?;
    let corpus_bytes = cairn_codec::to_vec(&corpus).map_err(composition)?;
    let corpus_id = ContentId::<HistoricalReductionCorpusArtifact>::derive(&corpus_bytes)
        .map_err(composition)?;
    Ok(PreparedHistoricalReductionCorpus {
        corpus,
        corpus_bytes,
        corpus_id,
    })
}

/// One exact case result emitted by the isolated reduction fixture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReductionCaseOutputV1 {
    case: ContentId<HistoricalReductionCaseArtifact>,
    value: FiniteF32Bits,
}

impl HistoricalReductionCaseOutputV1 {
    #[must_use]
    pub const fn case(self) -> ContentId<HistoricalReductionCaseArtifact> {
        self.case
    }

    #[must_use]
    pub const fn value(self) -> FiniteF32Bits {
        self.value
    }
}

/// Strict output of one isolated reduction implementation process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionFixtureOutputWire")]
pub struct HistoricalReductionFixtureOutputV1 {
    schema_version: u16,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    algorithm: HistoricalReductionAlgorithm,
    outputs: Vec<HistoricalReductionCaseOutputV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionFixtureOutputWire {
    schema_version: u16,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    algorithm: HistoricalReductionAlgorithm,
    outputs: Vec<HistoricalReductionCaseOutputV1>,
}

impl HistoricalReductionFixtureOutputV1 {
    fn new(
        corpus: ContentId<HistoricalReductionCorpusArtifact>,
        algorithm: HistoricalReductionAlgorithm,
        outputs: Vec<HistoricalReductionCaseOutputV1>,
    ) -> Result<Self, HistoricalReductionControlError> {
        if outputs.is_empty()
            || outputs
                .windows(2)
                .any(|pair| pair[0].case.to_wire() >= pair[1].case.to_wire())
        {
            return Err(HistoricalReductionControlError::InconsistentObservation);
        }
        Ok(Self {
            schema_version: 1,
            corpus,
            algorithm,
            outputs,
        })
    }

    #[must_use]
    pub const fn corpus(&self) -> ContentId<HistoricalReductionCorpusArtifact> {
        self.corpus
    }

    #[must_use]
    pub const fn algorithm(&self) -> HistoricalReductionAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub fn outputs(&self) -> &[HistoricalReductionCaseOutputV1] {
        &self.outputs
    }
}

impl TryFrom<HistoricalReductionFixtureOutputWire> for HistoricalReductionFixtureOutputV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionFixtureOutputWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        Self::new(wire.corpus, wire.algorithm, wire.outputs)
    }
}

/// Content domain for exact isolated reduction-process output bytes.
pub enum HistoricalReductionFixtureOutputArtifact {}

impl ContentType for HistoricalReductionFixtureOutputArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-fixture-output.v1";
}

/// Trusted deterministic implementation shared by the host fixture and receipt validator.
///
/// # Errors
///
/// Rejects overflow to a non-finite binary32 result.
pub fn compute_historical_reduction_output(
    corpus: &PreparedHistoricalReductionCorpus,
    algorithm: HistoricalReductionAlgorithm,
) -> Result<HistoricalReductionFixtureOutputV1, HistoricalReductionControlError> {
    validate_prepared_corpus(corpus)?;
    compute_historical_reduction_fixture_output(&corpus.corpus, algorithm)
}

/// Computes fixture output directly from a strict decoded corpus.
///
/// This is the shared deterministic implementation used inside the isolated host fixture. The
/// trusted receipt validator independently calls the prepared-corpus entry point above.
///
/// # Errors
///
/// Rejects a non-canonical corpus or non-finite result.
pub fn compute_historical_reduction_fixture_output(
    corpus: &HistoricalReductionCorpusV1,
    algorithm: HistoricalReductionAlgorithm,
) -> Result<HistoricalReductionFixtureOutputV1, HistoricalReductionControlError> {
    let corpus_bytes = cairn_codec::to_vec(corpus).map_err(composition)?;
    let corpus_id = ContentId::<HistoricalReductionCorpusArtifact>::derive(&corpus_bytes)
        .map_err(composition)?;
    let outputs = corpus
        .cases
        .iter()
        .map(|case| {
            Ok(HistoricalReductionCaseOutputV1 {
                case: case.case,
                value: reduce(case.body.inputs(), algorithm)?,
            })
        })
        .collect::<Result<Vec<_>, HistoricalReductionControlError>>()?;
    HistoricalReductionFixtureOutputV1::new(corpus_id, algorithm, outputs)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the reference deliberately accumulates in binary64 and rounds once to the binary32 ABI"
)]
fn reduce(
    inputs: &[FiniteF32Bits],
    algorithm: HistoricalReductionAlgorithm,
) -> Result<FiniteF32Bits, HistoricalReductionControlError> {
    let value = match algorithm {
        HistoricalReductionAlgorithm::HighPrecisionReference => inputs
            .iter()
            .map(|value| f64::from(value.value()))
            .sum::<f64>() as f32,
        HistoricalReductionAlgorithm::Sequential => sequential(inputs),
        HistoricalReductionAlgorithm::BalancedTree => balanced_tree(inputs),
        HistoricalReductionAlgorithm::ZeroOutput => 0.0,
        HistoricalReductionAlgorithm::DropLast => {
            sequential(&inputs[..inputs.len().saturating_sub(1)])
        }
        HistoricalReductionAlgorithm::UnitOffset => sequential(inputs) + 1.0,
    };
    FiniteF32Bits::from_f32(value)
}

fn sequential(inputs: &[FiniteF32Bits]) -> f32 {
    inputs
        .iter()
        .fold(0.0_f32, |sum, value| sum + value.value())
}

fn balanced_tree(inputs: &[FiniteF32Bits]) -> f32 {
    let mut level = inputs.iter().map(|value| value.value()).collect::<Vec<_>>();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            next.push(if pair.len() == 2 {
                pair[0] + pair[1]
            } else {
                pair[0]
            });
        }
        level = next;
    }
    level.first().copied().unwrap_or(0.0)
}

fn validate_prepared_corpus(
    corpus: &PreparedHistoricalReductionCorpus,
) -> Result<(), HistoricalReductionControlError> {
    let bytes = cairn_codec::to_vec(&corpus.corpus).map_err(composition)?;
    if bytes != corpus.corpus_bytes
        || ContentId::<HistoricalReductionCorpusArtifact>::derive(&bytes).map_err(composition)?
            != corpus.corpus_id
    {
        return Err(HistoricalReductionControlError::InconsistentCorpus);
    }
    Ok(())
}

/// Closed subject role for one reference or admission-variant reduction execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HistoricalReductionExecutionSubjectV1 {
    /// Proposed semantic reference execution.
    Reference {
        /// Exact proposed reference identity.
        reference: ContentId<ReferenceArtifact>,
    },
    /// Correct or deliberately wrong variant built through the shared execution port.
    AdmissionVariant {
        /// Exact proposal-authored variant identity.
        variant: ContentId<ImplementationVariantArtifact>,
        /// Exact implementation bundle selected by that variant.
        implementation: ContentId<ImplementationBundleArtifact>,
        /// Authoritative validated build fact producing this executable.
        build: ContentId<crate::VariantBuildReceiptArtifact>,
    },
    /// Candidate implementation judged only after an oracle has been admitted.
    Candidate {
        /// Exact candidate implementation bundle.
        implementation: ContentId<ImplementationBundleArtifact>,
        /// Authoritative build fact that produced the executed bytes.
        build: ContentId<crate::VariantBuildReceiptArtifact>,
    },
}

/// Independent capture bounds for a historical reduction process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoricalReductionCaptureLimits {
    pub stdout: OutputByteLimit,
    pub stderr: OutputByteLimit,
    pub observation: OutputByteLimit,
    pub diagnostic: DiagnosticByteLimit,
    pub evidence: EvidenceByteLimit,
}

/// Strict product wrapper around one generic historical reduction job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionExecutionPlanWire")]
pub struct HistoricalReductionExecutionPlanV1 {
    schema_version: u16,
    subject: HistoricalReductionExecutionSubjectV1,
    algorithm: HistoricalReductionAlgorithm,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    executable: ContentId<CallAdapterExecutableArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    tier: MigrationValidationTier,
    job_id: JobId,
    contract: ContentId<JobContractArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionExecutionPlanWire {
    schema_version: u16,
    subject: HistoricalReductionExecutionSubjectV1,
    algorithm: HistoricalReductionAlgorithm,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    executable: ContentId<CallAdapterExecutableArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    tier: MigrationValidationTier,
    job_id: JobId,
    contract: ContentId<JobContractArtifact>,
}

impl HistoricalReductionExecutionPlanV1 {
    fn from_wire(
        wire: HistoricalReductionExecutionPlanWire,
    ) -> Result<Self, HistoricalReductionControlError> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        if matches!(
            (&wire.subject, wire.algorithm),
            (
                HistoricalReductionExecutionSubjectV1::Reference { .. },
                algorithm
            ) if algorithm != HistoricalReductionAlgorithm::HighPrecisionReference
        ) || matches!(
            (&wire.subject, wire.algorithm),
            (
                HistoricalReductionExecutionSubjectV1::AdmissionVariant { .. }
                    | HistoricalReductionExecutionSubjectV1::Candidate { .. },
                HistoricalReductionAlgorithm::HighPrecisionReference
            )
        ) {
            return Err(HistoricalReductionControlError::InconsistentExecutionPlan);
        }
        Ok(Self {
            schema_version: 1,
            subject: wire.subject,
            algorithm: wire.algorithm,
            corpus: wire.corpus,
            executable: wire.executable,
            environment: wire.environment,
            tier: wire.tier,
            job_id: wire.job_id,
            contract: wire.contract,
        })
    }

    #[must_use]
    pub const fn subject(&self) -> &HistoricalReductionExecutionSubjectV1 {
        &self.subject
    }

    #[must_use]
    pub const fn algorithm(&self) -> HistoricalReductionAlgorithm {
        self.algorithm
    }

    /// Returns the exact execution environment exercised by this plan.
    #[must_use]
    pub const fn environment(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment
    }

    /// Returns the frozen reduction corpus exercised by this plan.
    #[must_use]
    pub const fn corpus(&self) -> ContentId<HistoricalReductionCorpusArtifact> {
        self.corpus
    }
}

impl TryFrom<HistoricalReductionExecutionPlanWire> for HistoricalReductionExecutionPlanV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionExecutionPlanWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire)
    }
}

/// Content domain for one exact historical reduction execution plan.
pub enum HistoricalReductionExecutionPlanArtifact {}

impl ContentType for HistoricalReductionExecutionPlanArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-execution-plan.v1";
}

/// Exact generic job and product plan ready for execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionJob {
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    contract: JobContract,
    contract_bytes: Vec<u8>,
    contract_id: ContentId<JobContractArtifact>,
    plan: HistoricalReductionExecutionPlanV1,
    plan_bytes: Vec<u8>,
    plan_id: ContentId<HistoricalReductionExecutionPlanArtifact>,
}

impl PreparedHistoricalReductionJob {
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
    pub const fn plan(&self) -> &HistoricalReductionExecutionPlanV1 {
        &self.plan
    }

    #[must_use]
    pub fn plan_bytes(&self) -> &[u8] {
        &self.plan_bytes
    }

    #[must_use]
    pub const fn plan_id(&self) -> ContentId<HistoricalReductionExecutionPlanArtifact> {
        self.plan_id
    }
}

/// Prepares a reference reduction job from exact executable bytes.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes or invalid generic execution material.
#[expect(
    clippy::too_many_arguments,
    reason = "reference identity, executable, environment, execution need, and capture bounds are independent trust inputs"
)]
pub fn prepare_historical_reduction_reference_job(
    job_id: JobId,
    reference: ContentId<ReferenceArtifact>,
    algorithm: HistoricalReductionAlgorithm,
    corpus: &PreparedHistoricalReductionCorpus,
    executable_bytes: &[u8],
    executable_limit: u64,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: HistoricalReductionCaptureLimits,
) -> Result<PreparedHistoricalReductionJob, HistoricalReductionControlError> {
    if !matches!(
        algorithm,
        HistoricalReductionAlgorithm::HighPrecisionReference
    ) {
        return Err(HistoricalReductionControlError::InconsistentExecutionPlan);
    }
    validate_bytes(executable_bytes, executable_limit, "executable")?;
    let executable = ContentId::<CallAdapterExecutableArtifact>::derive(executable_bytes)
        .map_err(composition)?;
    prepare_reduction_job(
        job_id,
        HistoricalReductionExecutionSubjectV1::Reference { reference },
        algorithm,
        corpus,
        executable_bytes,
        executable,
        environment,
        need,
        limits,
    )
}

/// Prepares an admission-variant reduction job from an authoritative validated build.
///
/// # Errors
///
/// Rejects a relabeled variant/build, wrong reference algorithm, or invalid execution material.
#[expect(
    clippy::too_many_arguments,
    reason = "variant, validated build, corpus, environment, execution need, and capture bounds are independent trust inputs"
)]
pub fn prepare_historical_reduction_variant_job(
    job_id: JobId,
    variant: &ImplementationVariantV1,
    algorithm: HistoricalReductionAlgorithm,
    corpus: &PreparedHistoricalReductionCorpus,
    build: &ValidatedVariantBuild,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: HistoricalReductionCaptureLimits,
) -> Result<PreparedHistoricalReductionJob, HistoricalReductionControlError> {
    if matches!(
        algorithm,
        HistoricalReductionAlgorithm::HighPrecisionReference
    ) {
        return Err(HistoricalReductionControlError::InconsistentExecutionPlan);
    }
    let variant_bytes = cairn_codec::to_vec(variant).map_err(composition)?;
    let variant_id =
        ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes).map_err(composition)?;
    if build.build_receipt().variant() != variant_id
        || build.build_receipt().implementation() != variant.implementation()
        || build.executable_bytes().is_empty()
    {
        return Err(HistoricalReductionControlError::InconsistentVariantBuild);
    }
    prepare_reduction_job(
        job_id,
        HistoricalReductionExecutionSubjectV1::AdmissionVariant {
            variant: variant_id,
            implementation: variant.implementation(),
            build: build.build_receipt_id(),
        },
        algorithm,
        corpus,
        build.executable_bytes(),
        build.executable_id(),
        environment,
        need,
        limits,
    )
}

/// Prepares a candidate-role execution from exact authoritative build evidence.
///
/// # Errors
///
/// Rejects a changed candidate/build identity, reference-only algorithm, or invalid execution
/// material. The build may retain its admission-variant provenance, but the run is independently
/// bound to the candidate role and implementation identity.
#[expect(
    clippy::too_many_arguments,
    reason = "candidate, build, corpus, environment, execution need, and capture limits are independent trust inputs"
)]
pub fn prepare_historical_reduction_candidate_job(
    job_id: JobId,
    candidate: ContentId<ImplementationBundleArtifact>,
    algorithm: HistoricalReductionAlgorithm,
    corpus: &PreparedHistoricalReductionCorpus,
    build: &ValidatedVariantBuild,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: HistoricalReductionCaptureLimits,
) -> Result<PreparedHistoricalReductionJob, HistoricalReductionControlError> {
    if algorithm == HistoricalReductionAlgorithm::HighPrecisionReference
        || build.build_receipt().implementation() != candidate
        || build.executable_bytes().is_empty()
    {
        return Err(HistoricalReductionControlError::InconsistentVariantBuild);
    }
    prepare_reduction_job(
        job_id,
        HistoricalReductionExecutionSubjectV1::Candidate {
            implementation: candidate,
            build: build.build_receipt_id(),
        },
        algorithm,
        corpus,
        build.executable_bytes(),
        build.executable_id(),
        environment,
        need,
        limits,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "subject, corpus, executable, environment, execution need, and capture bounds remain separate immutable bindings"
)]
fn prepare_reduction_job(
    job_id: JobId,
    subject: HistoricalReductionExecutionSubjectV1,
    algorithm: HistoricalReductionAlgorithm,
    corpus: &PreparedHistoricalReductionCorpus,
    executable_bytes: &[u8],
    executable: ContentId<CallAdapterExecutableArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    need: &MigrationExecutionNeed,
    limits: HistoricalReductionCaptureLimits,
) -> Result<PreparedHistoricalReductionJob, HistoricalReductionControlError> {
    validate_prepared_corpus(corpus)?;
    let input_bundle = InputBundleV1::new(vec![
        InputBundleEntry::Directory {
            path: path(REDUCTION_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: path(REDUCTION_EXECUTABLE_PATH)?,
            mode: InputFileMode::Executable,
            bytes: executable_bytes.to_vec(),
        },
        InputBundleEntry::File {
            path: path(REDUCTION_CORPUS_PATH)?,
            mode: InputFileMode::Data,
            bytes: corpus.corpus_bytes.clone(),
        },
    ])
    .map_err(composition)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(composition)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(composition)?;
    let command = CommandContract::new(
        path(REDUCTION_EXECUTABLE_PATH)?,
        vec![
            argument("--corpus")?,
            argument(CONTAINER_CORPUS_PATH)?,
            argument("--algorithm")?,
            argument(algorithm.as_str())?,
            argument("--output")?,
            argument(CONTAINER_OUTPUT_PATH)?,
        ],
        path(WORKING_DIRECTORY)?,
    );
    let capture = CapturePolicy::new(
        limits.stdout,
        limits.stderr,
        limits.diagnostic,
        limits.evidence,
        vec![ExpectedOutput {
            name: output_name(REDUCTION_OUTPUT_NAME)?,
            path: path(REDUCTION_OUTPUT_PATH)?,
            byte_limit: limits.observation,
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
    let plan = HistoricalReductionExecutionPlanV1 {
        schema_version: 1,
        subject,
        algorithm,
        corpus: corpus.corpus_id,
        executable,
        environment,
        tier: need.tier(),
        job_id,
        contract: contract_id,
    };
    let plan_bytes = cairn_codec::to_vec(&plan).map_err(composition)?;
    let plan_id = ContentId::<HistoricalReductionExecutionPlanArtifact>::derive(&plan_bytes)
        .map_err(composition)?;
    Ok(PreparedHistoricalReductionJob {
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

/// Strict V1 fact binding an authoritative receipt to one validated reduction observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionExecutionReceiptWire")]
pub struct HistoricalReductionExecutionReceiptV1 {
    schema_version: u16,
    plan: ContentId<HistoricalReductionExecutionPlanArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    declared_output: ContentId<DeclaredOutputArtifact>,
    observation: ContentId<HistoricalReductionFixtureOutputArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionExecutionReceiptWire {
    schema_version: u16,
    plan: ContentId<HistoricalReductionExecutionPlanArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    declared_output: ContentId<DeclaredOutputArtifact>,
    observation: ContentId<HistoricalReductionFixtureOutputArtifact>,
}

impl TryFrom<HistoricalReductionExecutionReceiptWire> for HistoricalReductionExecutionReceiptV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionExecutionReceiptWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        Ok(Self {
            schema_version: 1,
            plan: wire.plan,
            receipt: wire.receipt,
            declared_output: wire.declared_output,
            observation: wire.observation,
        })
    }
}

impl HistoricalReductionExecutionReceiptV1 {
    /// Fully revalidates this persisted fact against authoritative generic execution and content.
    ///
    /// # Errors
    ///
    /// Rejects changed prepared material, receipt authority, declared output, or observation bytes.
    pub fn validate_inputs<C: ContentStore>(
        &self,
        corpus: &PreparedHistoricalReductionCorpus,
        job: &PreparedHistoricalReductionJob,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: &ExecutionReceipt,
        content: &C,
    ) -> Result<(), HistoricalReductionControlError> {
        let recomputed =
            validate_historical_reduction_receipt(corpus, job, receipt_id, receipt, content)?;
        if recomputed.execution_receipt != *self {
            return Err(HistoricalReductionControlError::InconsistentExecutionReceipt);
        }
        Ok(())
    }
}

/// Content domain for exact validated historical reduction execution facts.
pub enum HistoricalReductionExecutionReceiptArtifact {}

impl ContentType for HistoricalReductionExecutionReceiptArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-execution-receipt.v1";
}

/// Validated process output backed by a generic authoritative receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedHistoricalReductionRun {
    execution_receipt: HistoricalReductionExecutionReceiptV1,
    execution_receipt_bytes: Vec<u8>,
    execution_receipt_id: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    observation: HistoricalReductionFixtureOutputV1,
}

impl ValidatedHistoricalReductionRun {
    #[must_use]
    pub const fn execution_receipt(&self) -> &HistoricalReductionExecutionReceiptV1 {
        &self.execution_receipt
    }

    #[must_use]
    pub const fn execution_receipt_id(
        &self,
    ) -> ContentId<HistoricalReductionExecutionReceiptArtifact> {
        self.execution_receipt_id
    }

    #[must_use]
    pub fn execution_receipt_bytes(&self) -> &[u8] {
        &self.execution_receipt_bytes
    }

    #[must_use]
    pub const fn observation(&self) -> &HistoricalReductionFixtureOutputV1 {
        &self.observation
    }
}

/// Correct-by-construction variant evidence supplied to the historical control.
pub struct HistoricalReductionCorrectVariantEvidence<'a> {
    pub variant: &'a ImplementationVariantV1,
    pub construction_claim: &'a ConstructionClaimV1,
    pub job: &'a PreparedHistoricalReductionJob,
    pub run: &'a ValidatedHistoricalReductionRun,
}

/// Deliberately wrong variant evidence supplied to the historical control.
pub struct HistoricalReductionWrongVariantEvidence<'a> {
    pub variant: &'a ImplementationVariantV1,
    pub job: &'a PreparedHistoricalReductionJob,
    pub run: &'a ValidatedHistoricalReductionRun,
}

/// Role facts copied from a proposal-authored correct or deliberately wrong variant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "expectation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HistoricalReductionTrialExpectationV1 {
    MustAccept {
        construction_claim: ContentId<ConstructionClaimArtifact>,
        construction_class: ConstructionClassName,
    },
    MustReject {
        fault_class: FaultClassName,
        fault_evidence: ContentId<FaultInjectionEvidenceArtifact>,
    },
}

/// Underlying numerical facts for one variant/case comparison.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReductionCaseComparisonV1 {
    case: ContentId<HistoricalReductionCaseArtifact>,
    reference: FiniteF32Bits,
    subject: FiniteF32Bits,
    ulp_distance: ReductionUlpDistance,
}

impl HistoricalReductionCaseComparisonV1 {
    #[must_use]
    pub const fn case(self) -> ContentId<HistoricalReductionCaseArtifact> {
        self.case
    }

    #[must_use]
    pub const fn ulp_distance(self) -> ReductionUlpDistance {
        self.ulp_distance
    }
}

/// One exact variant execution and every recomputed case distance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReductionVariantTrialV1 {
    variant: ContentId<ImplementationVariantArtifact>,
    algorithm: HistoricalReductionAlgorithm,
    execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    expectation: HistoricalReductionTrialExpectationV1,
    comparisons: Vec<HistoricalReductionCaseComparisonV1>,
}

impl HistoricalReductionVariantTrialV1 {
    #[must_use]
    pub const fn variant(&self) -> ContentId<ImplementationVariantArtifact> {
        self.variant
    }

    #[must_use]
    pub const fn algorithm(&self) -> HistoricalReductionAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub const fn expectation(&self) -> &HistoricalReductionTrialExpectationV1 {
        &self.expectation
    }

    #[must_use]
    pub fn comparisons(&self) -> &[HistoricalReductionCaseComparisonV1] {
        &self.comparisons
    }

    #[must_use]
    pub fn maximum_ulp_distance(&self) -> ReductionUlpDistance {
        self.comparisons
            .iter()
            .map(|comparison| comparison.ulp_distance)
            .max()
            .unwrap_or(ReductionUlpDistance(0))
    }

    #[must_use]
    pub fn within(&self, allowance: ReductionUlpDistance) -> bool {
        self.comparisons
            .iter()
            .all(|comparison| comparison.ulp_distance <= allowance)
    }
}

/// Strict V1 facts for the hardware-free historical reduction acceptance control.
///
/// There is deliberately no stored `passed` field. Construction succeeds only after every input
/// is validated, and loading requires full recomputation through [`Self::validate_inputs`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionControlWire")]
pub struct HistoricalReductionControlV1 {
    schema_version: u16,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    admission_corpus: ContentId<AdmissionCorpusArtifact>,
    historical_obligation: ContentId<HistoricalFailureObligationArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    mutation_proof: ContentId<MutationGridProofArtifact>,
    reference_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
    old_baseline_variant: ContentId<ImplementationVariantArtifact>,
    old_single_sample_allowance: ReductionUlpDistance,
    correct_trials: Vec<HistoricalReductionVariantTrialV1>,
    wrong_trials: Vec<HistoricalReductionVariantTrialV1>,
    blind_spots: Vec<MutationGridCellV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionControlWire {
    schema_version: u16,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    admission_corpus: ContentId<AdmissionCorpusArtifact>,
    historical_obligation: ContentId<HistoricalFailureObligationArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    mutation_proof: ContentId<MutationGridProofArtifact>,
    reference_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
    old_baseline_variant: ContentId<ImplementationVariantArtifact>,
    old_single_sample_allowance: ReductionUlpDistance,
    correct_trials: Vec<HistoricalReductionVariantTrialV1>,
    wrong_trials: Vec<HistoricalReductionVariantTrialV1>,
    blind_spots: Vec<MutationGridCellV1>,
}

impl HistoricalReductionControlV1 {
    fn from_wire(
        wire: HistoricalReductionControlWire,
    ) -> Result<Self, HistoricalReductionControlError> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        if !trials_are_canonical(&wire.correct_trials)
            || !trials_are_canonical(&wire.wrong_trials)
            || wire.correct_trials.iter().any(|trial| {
                !matches!(
                    trial.expectation,
                    HistoricalReductionTrialExpectationV1::MustAccept { .. }
                )
            })
            || wire.wrong_trials.iter().any(|trial| {
                !matches!(
                    trial.expectation,
                    HistoricalReductionTrialExpectationV1::MustReject { .. }
                )
            })
            || wire
                .blind_spots
                .windows(2)
                .any(|pair| mutation_cell_key(pair[0]) >= mutation_cell_key(pair[1]))
        {
            return Err(HistoricalReductionControlError::InconsistentControl);
        }
        Ok(Self {
            schema_version: 1,
            proposal: wire.proposal,
            policy: wire.policy,
            corpus: wire.corpus,
            admission_corpus: wire.admission_corpus,
            historical_obligation: wire.historical_obligation,
            allowance: wire.allowance,
            mutation_proof: wire.mutation_proof,
            reference_execution: wire.reference_execution,
            old_sample_case: wire.old_sample_case,
            old_baseline_variant: wire.old_baseline_variant,
            old_single_sample_allowance: wire.old_single_sample_allowance,
            correct_trials: wire.correct_trials,
            wrong_trials: wire.wrong_trials,
            blind_spots: wire.blind_spots,
        })
    }

    #[must_use]
    pub fn correct_trials(&self) -> &[HistoricalReductionVariantTrialV1] {
        &self.correct_trials
    }

    #[must_use]
    pub fn wrong_trials(&self) -> &[HistoricalReductionVariantTrialV1] {
        &self.wrong_trials
    }

    #[must_use]
    pub const fn old_single_sample_allowance(&self) -> ReductionUlpDistance {
        self.old_single_sample_allowance
    }

    #[must_use]
    pub fn blind_spots(&self) -> &[MutationGridCellV1] {
        &self.blind_spots
    }

    /// Returns the exact reference execution frozen by the control.
    #[must_use]
    pub const fn reference_execution(
        &self,
    ) -> ContentId<HistoricalReductionExecutionReceiptArtifact> {
        self.reference_execution
    }

    /// Recomputes every graph, execution, numerical, variant, and mutation fact.
    ///
    /// # Errors
    ///
    /// Rejects any input or persisted fact that differs from trusted recomputation.
    #[expect(
        clippy::too_many_arguments,
        reason = "proposal graph, historical record, policy, allowance, executions, and mutation proof are independent trust inputs"
    )]
    pub fn validate_inputs(
        &self,
        domain: &MigrationDomainContractV1,
        declared_domain: &DeclaredDomainV1,
        corpus_proposal: &CorpusProposalV1,
        proposal: &OracleProposalV1,
        historical_record: &HistoricalFailureRecordV1,
        historical_obligation: &HistoricalFailureObligationV1,
        historical_coverage: &HistoricalFailureCoverageV1,
        policy: &AdmissionPolicyV1,
        allowance: &NumericalAllowanceV1,
        corpus: &PreparedHistoricalReductionCorpus,
        old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
        old_baseline_variant: ContentId<ImplementationVariantArtifact>,
        reference_job: &PreparedHistoricalReductionJob,
        reference_run: &ValidatedHistoricalReductionRun,
        correct: &[HistoricalReductionCorrectVariantEvidence<'_>],
        wrong: &[HistoricalReductionWrongVariantEvidence<'_>],
        mutant_set: &PreparedGenericMutantSet,
        mutation_grid: &PreparedMutationGrid,
        mutation_proof: &MutationGridProofV1,
    ) -> Result<(), HistoricalReductionControlError> {
        let recomputed = compose_historical_reduction_control(
            domain,
            declared_domain,
            corpus_proposal,
            proposal,
            historical_record,
            historical_obligation,
            historical_coverage,
            policy,
            allowance,
            corpus,
            old_sample_case,
            old_baseline_variant,
            reference_job,
            reference_run,
            correct,
            wrong,
            mutant_set,
            mutation_grid,
            mutation_proof,
        )?;
        if recomputed.control != *self {
            return Err(HistoricalReductionControlError::InconsistentControl);
        }
        Ok(())
    }
}

impl TryFrom<HistoricalReductionControlWire> for HistoricalReductionControlV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionControlWire) -> Result<Self, Self::Error> {
        Self::from_wire(wire)
    }
}

/// Content domain for one complete hardware-free historical reduction control.
pub enum HistoricalReductionControlArtifact {}

impl ContentType for HistoricalReductionControlArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-control.v1";
}

/// Canonical historical reduction control ready for later admission-receipt composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionControl {
    control: HistoricalReductionControlV1,
    control_bytes: Vec<u8>,
    control_id: ContentId<HistoricalReductionControlArtifact>,
}

impl PreparedHistoricalReductionControl {
    #[must_use]
    pub const fn control(&self) -> &HistoricalReductionControlV1 {
        &self.control
    }

    #[must_use]
    pub fn control_bytes(&self) -> &[u8] {
        &self.control_bytes
    }

    #[must_use]
    pub const fn control_id(&self) -> ContentId<HistoricalReductionControlArtifact> {
        self.control_id
    }
}

/// Composes the first hardware-free historical reduction acceptance control.
///
/// This emits recomputable control evidence, not an admitted-oracle receipt.
///
/// # Errors
///
/// Rejects graph mismatches, insufficient policy family/scope coverage, unmeasured allowance,
/// missing historical false rejection, accepted wrong variants, rejected correct variants, an
/// empty/failing mutation grid, or absence of the known case-dependent blind spot.
#[expect(
    clippy::too_many_arguments,
    reason = "proposal graph, historical record, policy, allowance, executions, and mutation proof are independent trust inputs"
)]
#[expect(
    clippy::too_many_lines,
    reason = "the admission control keeps graph, family, allowance, false-reject, and mutation checks visibly sequenced in one trust boundary"
)]
pub fn compose_historical_reduction_control(
    domain: &MigrationDomainContractV1,
    declared_domain: &DeclaredDomainV1,
    corpus_proposal: &CorpusProposalV1,
    proposal: &OracleProposalV1,
    historical_record: &HistoricalFailureRecordV1,
    historical_obligation: &HistoricalFailureObligationV1,
    historical_coverage: &HistoricalFailureCoverageV1,
    policy: &AdmissionPolicyV1,
    allowance: &NumericalAllowanceV1,
    corpus: &PreparedHistoricalReductionCorpus,
    old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
    old_baseline_variant: ContentId<ImplementationVariantArtifact>,
    reference_job: &PreparedHistoricalReductionJob,
    reference_run: &ValidatedHistoricalReductionRun,
    correct: &[HistoricalReductionCorrectVariantEvidence<'_>],
    wrong: &[HistoricalReductionWrongVariantEvidence<'_>],
    mutant_set: &PreparedGenericMutantSet,
    mutation_grid: &PreparedMutationGrid,
    mutation_proof: &MutationGridProofV1,
) -> Result<PreparedHistoricalReductionControl, HistoricalReductionControlError> {
    validate_control_proposal_graph(
        domain,
        declared_domain,
        corpus_proposal,
        proposal,
        historical_record,
        historical_obligation,
        historical_coverage,
        corpus,
        reference_job,
    )?;
    validate_control_run(corpus, reference_job, reference_run)?;
    if !matches!(
        reference_job.plan.subject,
        HistoricalReductionExecutionSubjectV1::Reference { .. }
    ) || reference_job.plan.algorithm != HistoricalReductionAlgorithm::HighPrecisionReference
    {
        return Err(HistoricalReductionControlError::InconsistentProposalGraph);
    }
    let reference = &reference_run.observation;
    let mut correct_trials = correct
        .iter()
        .map(|evidence| prepare_correct_trial(corpus, reference, evidence))
        .collect::<Result<Vec<_>, _>>()?;
    let mut wrong_trials = wrong
        .iter()
        .map(|evidence| prepare_wrong_trial(corpus, reference, evidence))
        .collect::<Result<Vec<_>, _>>()?;
    correct_trials.sort_by_key(|trial| trial.variant.to_wire());
    wrong_trials.sort_by_key(|trial| trial.variant.to_wire());
    if !trials_are_canonical(&correct_trials)
        || !trials_are_canonical(&wrong_trials)
        || correct_trials.iter().any(|correct| {
            wrong_trials.iter().any(|wrong| {
                wrong.variant == correct.variant || wrong.algorithm == correct.algorithm
            })
        })
    {
        return Err(HistoricalReductionControlError::InconsistentVariantControls);
    }
    validate_policy(policy, &correct_trials, &wrong_trials)?;
    let admission_corpus =
        ContentId::<AdmissionCorpusArtifact>::derive(corpus.corpus_bytes()).map_err(composition)?;
    let allowance_value = validate_allowance(allowance, admission_corpus, &correct_trials)?;
    if correct_trials
        .iter()
        .any(|trial| !trial.within(allowance_value))
        || wrong_trials
            .iter()
            .any(|trial| trial.within(allowance_value))
    {
        return Err(HistoricalReductionControlError::InconsistentVariantControls);
    }
    let old_baseline = correct_trials
        .iter()
        .find(|trial| trial.variant == old_baseline_variant)
        .ok_or(HistoricalReductionControlError::FalseRejectNotReproduced)?;
    let old_single_sample_allowance = old_baseline
        .comparisons
        .iter()
        .find(|comparison| comparison.case == old_sample_case)
        .map(|comparison| comparison.ulp_distance)
        .ok_or(HistoricalReductionControlError::FalseRejectNotReproduced)?;
    if !correct_trials
        .iter()
        .any(|trial| !trial.within(old_single_sample_allowance))
    {
        return Err(HistoricalReductionControlError::FalseRejectNotReproduced);
    }
    mutation_proof
        .validate_against(policy, mutant_set, mutation_grid)
        .map_err(composition)?;
    if mutation_grid.grid().corpus() != admission_corpus
        || !correct
            .iter()
            .any(|evidence| evidence.variant.implementation() == mutation_grid.grid().subject())
        || !mutation_proof.obligations_satisfied()
        || mutation_proof.blind_spots().is_empty()
    {
        return Err(HistoricalReductionControlError::InsufficientMutationControl);
    }
    let proposal_bytes = cairn_codec::to_vec(proposal).map_err(composition)?;
    let policy_bytes = cairn_codec::to_vec(policy).map_err(composition)?;
    let allowance_bytes = cairn_codec::to_vec(allowance).map_err(composition)?;
    let obligation_bytes = cairn_codec::to_vec(historical_obligation).map_err(composition)?;
    let proof_bytes = cairn_codec::to_vec(mutation_proof).map_err(composition)?;
    let control = HistoricalReductionControlV1::from_wire(HistoricalReductionControlWire {
        schema_version: 1,
        proposal: ContentId::<OracleProposalArtifact>::derive(&proposal_bytes)
            .map_err(composition)?,
        policy: ContentId::<AdmissionPolicyArtifact>::derive(&policy_bytes).map_err(composition)?,
        corpus: corpus.corpus_id,
        admission_corpus,
        historical_obligation: ContentId::<HistoricalFailureObligationArtifact>::derive(
            &obligation_bytes,
        )
        .map_err(composition)?,
        allowance: ContentId::<NumericalAllowanceArtifact>::derive(&allowance_bytes)
            .map_err(composition)?,
        mutation_proof: ContentId::<MutationGridProofArtifact>::derive(&proof_bytes)
            .map_err(composition)?,
        reference_execution: reference_run.execution_receipt_id,
        old_sample_case,
        old_baseline_variant,
        old_single_sample_allowance,
        correct_trials,
        wrong_trials,
        blind_spots: mutation_proof.blind_spots().to_vec(),
    })?;
    let control_bytes = cairn_codec::to_vec(&control).map_err(composition)?;
    let control_id = ContentId::<HistoricalReductionControlArtifact>::derive(&control_bytes)
        .map_err(composition)?;
    Ok(PreparedHistoricalReductionControl {
        control,
        control_bytes,
        control_id,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "each ordinary proposal and historical artifact is an independent identity edge"
)]
fn validate_control_proposal_graph(
    domain: &MigrationDomainContractV1,
    declared_domain: &DeclaredDomainV1,
    corpus_proposal: &CorpusProposalV1,
    proposal: &OracleProposalV1,
    historical_record: &HistoricalFailureRecordV1,
    historical_obligation: &HistoricalFailureObligationV1,
    historical_coverage: &HistoricalFailureCoverageV1,
    corpus: &PreparedHistoricalReductionCorpus,
    reference_job: &PreparedHistoricalReductionJob,
) -> Result<(), HistoricalReductionControlError> {
    validate_prepared_corpus(corpus)?;
    historical_obligation
        .validate_record(historical_record)
        .map_err(composition)?;
    let domain_bytes = cairn_codec::to_vec(domain).map_err(composition)?;
    let domain_id =
        ContentId::<CallerDomainBodyArtifact>::derive(&domain_bytes).map_err(composition)?;
    let declared_bytes = cairn_codec::to_vec(declared_domain).map_err(composition)?;
    let declared_id =
        ContentId::<DeclaredDomainArtifact>::derive(&declared_bytes).map_err(composition)?;
    let corpus_proposal_bytes = cairn_codec::to_vec(corpus_proposal).map_err(composition)?;
    let corpus_proposal_id =
        ContentId::<CorpusProposalArtifact>::derive(&corpus_proposal_bytes).map_err(composition)?;
    let reference = match reference_job.plan.subject {
        HistoricalReductionExecutionSubjectV1::Reference { reference } => reference,
        HistoricalReductionExecutionSubjectV1::AdmissionVariant { .. }
        | HistoricalReductionExecutionSubjectV1::Candidate { .. } => {
            return Err(HistoricalReductionControlError::InconsistentProposalGraph);
        }
    };
    if declared_domain.body() != domain_id
        || proposal.task_id() != declared_domain.task_id()
        || corpus_proposal.declared_domain() != declared_id
        || corpus.corpus().proposal() != corpus_proposal_id
        || proposal.declared_domain() != declared_id
        || proposal.corpus_proposal() != corpus_proposal_id
        || proposal.requested_strength() != OracleStrength::Reference
        || !proposal.references().contains(&reference)
        || historical_coverage.domain() != domain_id
        || !historical_coverage
            .obligations()
            .contains(historical_obligation)
        || !historical_coverage
            .obligations()
            .iter()
            .any(|obligation| obligation.record() == historical_obligation.record())
        || historical_obligation.required_detection()
            != &HistoricalDetectionRequirement::OracleVerdictDivergence
    {
        return Err(HistoricalReductionControlError::InconsistentProposalGraph);
    }
    Ok(())
}

fn prepare_correct_trial(
    corpus: &PreparedHistoricalReductionCorpus,
    reference: &HistoricalReductionFixtureOutputV1,
    evidence: &HistoricalReductionCorrectVariantEvidence<'_>,
) -> Result<HistoricalReductionVariantTrialV1, HistoricalReductionControlError> {
    validate_control_run(corpus, evidence.job, evidence.run)?;
    let variant_bytes = cairn_codec::to_vec(evidence.variant).map_err(composition)?;
    let variant =
        ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes).map_err(composition)?;
    let claim_bytes = cairn_codec::to_vec(evidence.construction_claim).map_err(composition)?;
    let construction_claim =
        ContentId::<ConstructionClaimArtifact>::derive(&claim_bytes).map_err(composition)?;
    let expected_claim = match evidence.variant.expectation() {
        VariantExpectation::MustAccept { construction_claim } => *construction_claim,
        VariantExpectation::MustReject { .. } => {
            return Err(HistoricalReductionControlError::InconsistentVariantControls);
        }
    };
    if expected_claim != construction_claim
        || !matches!(
            evidence.job.plan.subject,
            HistoricalReductionExecutionSubjectV1::AdmissionVariant {
                variant: planned,
                implementation,
                ..
            } if planned == variant && implementation == evidence.variant.implementation()
        )
    {
        return Err(HistoricalReductionControlError::InconsistentVariantControls);
    }
    Ok(HistoricalReductionVariantTrialV1 {
        variant,
        algorithm: evidence.job.plan.algorithm,
        execution: evidence.run.execution_receipt_id,
        expectation: HistoricalReductionTrialExpectationV1::MustAccept {
            construction_claim,
            construction_class: evidence.construction_claim.construction_class().clone(),
        },
        comparisons: compare_reduction_outputs(reference, &evidence.run.observation)?,
    })
}

fn prepare_wrong_trial(
    corpus: &PreparedHistoricalReductionCorpus,
    reference: &HistoricalReductionFixtureOutputV1,
    evidence: &HistoricalReductionWrongVariantEvidence<'_>,
) -> Result<HistoricalReductionVariantTrialV1, HistoricalReductionControlError> {
    validate_control_run(corpus, evidence.job, evidence.run)?;
    let variant_bytes = cairn_codec::to_vec(evidence.variant).map_err(composition)?;
    let variant =
        ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes).map_err(composition)?;
    let (fault_class, fault_evidence) = match evidence.variant.expectation() {
        VariantExpectation::MustReject {
            fault_class,
            fault_evidence,
        } => (fault_class.clone(), *fault_evidence),
        VariantExpectation::MustAccept { .. } => {
            return Err(HistoricalReductionControlError::InconsistentVariantControls);
        }
    };
    if !matches!(
        evidence.job.plan.subject,
        HistoricalReductionExecutionSubjectV1::AdmissionVariant {
            variant: planned,
            implementation,
            ..
        } if planned == variant && implementation == evidence.variant.implementation()
    ) {
        return Err(HistoricalReductionControlError::InconsistentVariantControls);
    }
    Ok(HistoricalReductionVariantTrialV1 {
        variant,
        algorithm: evidence.job.plan.algorithm,
        execution: evidence.run.execution_receipt_id,
        expectation: HistoricalReductionTrialExpectationV1::MustReject {
            fault_class,
            fault_evidence,
        },
        comparisons: compare_reduction_outputs(reference, &evidence.run.observation)?,
    })
}

pub(crate) fn validate_control_run(
    corpus: &PreparedHistoricalReductionCorpus,
    job: &PreparedHistoricalReductionJob,
    run: &ValidatedHistoricalReductionRun,
) -> Result<(), HistoricalReductionControlError> {
    validate_prepared_reduction_job(corpus, job)?;
    let expected = compute_historical_reduction_output(corpus, job.plan.algorithm)?;
    if run.execution_receipt.plan != job.plan_id
        || run.observation != expected
        || run.execution_receipt.observation
            != ContentId::<HistoricalReductionFixtureOutputArtifact>::derive(
                &cairn_codec::to_vec(&run.observation).map_err(composition)?,
            )
            .map_err(composition)?
    {
        return Err(HistoricalReductionControlError::InconsistentObservation);
    }
    Ok(())
}

fn compare_reduction_outputs(
    reference: &HistoricalReductionFixtureOutputV1,
    subject: &HistoricalReductionFixtureOutputV1,
) -> Result<Vec<HistoricalReductionCaseComparisonV1>, HistoricalReductionControlError> {
    if reference.corpus != subject.corpus || reference.outputs.len() != subject.outputs.len() {
        return Err(HistoricalReductionControlError::InconsistentObservation);
    }
    reference
        .outputs
        .iter()
        .zip(&subject.outputs)
        .map(|(reference, subject)| {
            if reference.case != subject.case {
                return Err(HistoricalReductionControlError::InconsistentObservation);
            }
            Ok(HistoricalReductionCaseComparisonV1 {
                case: reference.case,
                reference: reference.value,
                subject: subject.value,
                ulp_distance: ReductionUlpDistance::between(reference.value, subject.value),
            })
        })
        .collect()
}

fn validate_policy(
    policy: &AdmissionPolicyV1,
    correct: &[HistoricalReductionVariantTrialV1],
    wrong: &[HistoricalReductionVariantTrialV1],
) -> Result<(), HistoricalReductionControlError> {
    let correct_count = u32::try_from(correct.len()).map_err(composition)?;
    let wrong_count = u32::try_from(wrong.len()).map_err(composition)?;
    let mut construction_claims = correct
        .iter()
        .filter_map(|trial| match &trial.expectation {
            HistoricalReductionTrialExpectationV1::MustAccept {
                construction_claim, ..
            } => Some(construction_claim.to_wire()),
            HistoricalReductionTrialExpectationV1::MustReject { .. } => None,
        })
        .collect::<Vec<_>>();
    construction_claims.sort();
    construction_claims.dedup();
    let construction_classes = correct
        .iter()
        .filter_map(|trial| match &trial.expectation {
            HistoricalReductionTrialExpectationV1::MustAccept {
                construction_class, ..
            } => Some(construction_class),
            HistoricalReductionTrialExpectationV1::MustReject { .. } => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let fault_classes = wrong
        .iter()
        .filter_map(|trial| match &trial.expectation {
            HistoricalReductionTrialExpectationV1::MustReject { fault_class, .. } => {
                Some(fault_class)
            }
            HistoricalReductionTrialExpectationV1::MustAccept { .. } => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let distinct_required = matches!(
        policy.structural_independence(),
        cairn_verification::StructuralIndependenceRequirement::DistinctConstructionClaims
    );
    if correct_count < policy.minimum_correct_variants().get()
        || wrong_count < policy.minimum_incorrect_variants().get()
        || !policy
            .required_construction_classes()
            .iter()
            .all(|required| construction_classes.contains(required))
        || !policy
            .required_fault_classes()
            .iter()
            .all(|required| fault_classes.contains(required))
        || (distinct_required && construction_claims.len() != correct.len())
        || !policy
            .accepted_strengths()
            .contains(&OracleStrength::Reference)
        || !policy
            .required_execution_scopes()
            .contains(&AdmissionExecutionScope::Implementation)
        || !policy
            .required_execution_scopes()
            .contains(&AdmissionExecutionScope::ObservationPipeline)
    {
        return Err(HistoricalReductionControlError::UnsatisfiedPolicy);
    }
    Ok(())
}

fn validate_allowance(
    allowance: &NumericalAllowanceV1,
    admission_corpus: ContentId<AdmissionCorpusArtifact>,
    correct: &[HistoricalReductionVariantTrialV1],
) -> Result<ReductionUlpDistance, HistoricalReductionControlError> {
    let Some(absolute) = allowance.absolute() else {
        return Err(HistoricalReductionControlError::InadmissibleAllowance);
    };
    let parsed = absolute
        .as_str()
        .parse::<u32>()
        .map_err(|_| HistoricalReductionControlError::InadmissibleAllowance)?;
    let measured = correct
        .iter()
        .map(HistoricalReductionVariantTrialV1::maximum_ulp_distance)
        .max()
        .unwrap_or(ReductionUlpDistance(0));
    if allowance.relative().is_some()
        || allowance.provenance() != AllowanceProvenance::MeasuredFamily
        || allowance.maximum_claim_class() == AllowanceClaimClass::InsufficientEvidence
        || !allowance.derivation_corpora().contains(&admission_corpus)
        || !allowance
            .domain_regions()
            .iter()
            .any(|region| region.as_str() == "finite-f32-reductions")
        || parsed != measured.get()
    {
        return Err(HistoricalReductionControlError::InadmissibleAllowance);
    }
    Ok(measured)
}

fn trials_are_canonical(trials: &[HistoricalReductionVariantTrialV1]) -> bool {
    !trials.is_empty()
        && !trials
            .windows(2)
            .any(|pair| pair[0].variant.to_wire() >= pair[1].variant.to_wire())
        && trials.iter().all(|trial| {
            !trial.comparisons.is_empty()
                && !trial
                    .comparisons
                    .windows(2)
                    .any(|pair| pair[0].case.to_wire() >= pair[1].case.to_wire())
                && trial.comparisons.iter().all(|comparison| {
                    comparison.ulp_distance
                        == ReductionUlpDistance::between(comparison.reference, comparison.subject)
                })
        })
}

fn mutation_cell_key(cell: MutationGridCellV1) -> (String, String) {
    (cell.mutant().to_wire(), cell.case().to_wire())
}

/// Validates an authoritative generic receipt, loads the exact fixture output, and recomputes it.
///
/// # Errors
///
/// Rejects changed prepared material, failed execution, unavailable output, or any output differing
/// from trusted reduction recomputation.
pub fn validate_historical_reduction_receipt<C: ContentStore>(
    corpus: &PreparedHistoricalReductionCorpus,
    job: &PreparedHistoricalReductionJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    content: &C,
) -> Result<ValidatedHistoricalReductionRun, HistoricalReductionControlError> {
    validate_prepared_reduction_job(corpus, job)?;
    let receipt_bytes = cairn_codec::to_vec(receipt).map_err(composition)?;
    if ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).map_err(composition)?
        != receipt_id
        || receipt.job_id() != job.contract.job_id()
        || receipt.contract_id() != job.contract_id
        || receipt.outcome() != ExecutionOutcome::Succeeded
        || receipt.exit_code() != Some(0)
        || receipt.outputs().len() != 1
        || receipt.outputs()[0].name.as_str() != REDUCTION_OUTPUT_NAME
    {
        return Err(HistoricalReductionControlError::InconsistentExecutionReceipt);
    }
    let declared_output = receipt.outputs()[0].content_id;
    let mut output_bytes = Vec::new();
    content
        .write_to::<DeclaredOutputArtifact>(&declared_output, &mut output_bytes)
        .map_err(|error| HistoricalReductionControlError::Content {
            message: error.to_string(),
        })?;
    let observation: HistoricalReductionFixtureOutputV1 =
        cairn_codec::from_slice(&output_bytes).map_err(composition)?;
    let expected = compute_historical_reduction_output(corpus, job.plan.algorithm)?;
    if observation != expected {
        return Err(HistoricalReductionControlError::InconsistentObservation);
    }
    let observation_id =
        ContentId::<HistoricalReductionFixtureOutputArtifact>::derive(&output_bytes)
            .map_err(composition)?;
    let execution_receipt = HistoricalReductionExecutionReceiptV1 {
        schema_version: 1,
        plan: job.plan_id,
        receipt: receipt_id,
        declared_output,
        observation: observation_id,
    };
    let execution_receipt_bytes = cairn_codec::to_vec(&execution_receipt).map_err(composition)?;
    let execution_receipt_id =
        ContentId::<HistoricalReductionExecutionReceiptArtifact>::derive(&execution_receipt_bytes)
            .map_err(composition)?;
    Ok(ValidatedHistoricalReductionRun {
        execution_receipt,
        execution_receipt_bytes,
        execution_receipt_id,
        observation,
    })
}

fn validate_prepared_reduction_job(
    corpus: &PreparedHistoricalReductionCorpus,
    job: &PreparedHistoricalReductionJob,
) -> Result<(), HistoricalReductionControlError> {
    validate_prepared_corpus(corpus)?;
    let input_bytes = job.input_bundle.to_bytes().map_err(composition)?;
    let contract_bytes = cairn_codec::to_vec(&job.contract).map_err(composition)?;
    let plan_bytes = cairn_codec::to_vec(&job.plan).map_err(composition)?;
    if input_bytes != job.input_bundle_bytes
        || ContentId::<InputBundleArtifact>::derive(&input_bytes).map_err(composition)?
            != job.input_bundle_id
        || contract_bytes != job.contract_bytes
        || ContentId::<JobContractArtifact>::derive(&contract_bytes).map_err(composition)?
            != job.contract_id
        || plan_bytes != job.plan_bytes
        || ContentId::<HistoricalReductionExecutionPlanArtifact>::derive(&plan_bytes)
            .map_err(composition)?
            != job.plan_id
        || job.plan.corpus != corpus.corpus_id
        || job.plan.job_id != job.contract.job_id()
        || job.plan.contract != job.contract_id
        || job.plan.environment != job.contract.environment_id()
        || job.contract.input_bundle_id() != job.input_bundle_id
        || job.contract.network() != NetworkPolicy::Disabled
        || job.contract.capture().expected_outputs().len() != 1
        || job.contract.capture().expected_outputs()[0].name.as_str() != REDUCTION_OUTPUT_NAME
        || job.contract.capture().expected_outputs()[0].path.as_str() != REDUCTION_OUTPUT_PATH
    {
        return Err(HistoricalReductionControlError::InconsistentExecutionPlan);
    }
    Ok(())
}

fn validate_bytes(
    bytes: &[u8],
    limit: u64,
    field: &'static str,
) -> Result<(), HistoricalReductionControlError> {
    if limit == 0 || bytes.is_empty() || u64::try_from(bytes.len()).map_err(composition)? > limit {
        return Err(HistoricalReductionControlError::InvalidBytes { field });
    }
    Ok(())
}

fn path(value: &str) -> Result<SandboxPath, HistoricalReductionControlError> {
    SandboxPath::new(value).map_err(composition)
}

fn argument(value: &str) -> Result<CommandArgument, HistoricalReductionControlError> {
    CommandArgument::new(value).map_err(composition)
}

fn output_name(value: &str) -> Result<OutputName, HistoricalReductionControlError> {
    OutputName::new(value).map_err(composition)
}

fn composition(error: impl std::fmt::Display) -> HistoricalReductionControlError {
    HistoricalReductionControlError::Composition {
        message: error.to_string(),
    }
}
