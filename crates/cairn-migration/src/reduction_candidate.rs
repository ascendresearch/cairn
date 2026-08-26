//! Candidate judgment against the frozen historical reduction oracle.

use cairn_execution::ExecutionEnvironmentArtifact;
use cairn_protocol::{ContentId, ContentType};
use cairn_verification::{
    AdmissionCorpusArtifact, AdmissionEnvironmentArtifact, CandidateArtifact,
    CandidateBuildArtifact, CandidateComparisonArtifact, CandidateFailedCaseArtifact,
    CandidateRunArtifact, CandidateSourceArtifact, CandidateVerdictInput, CandidateVerdictV1,
    ImplementationBundleArtifact, NumericalAllowanceArtifact, PreparedCandidateVerdict,
    prepare_candidate_verdict,
};
use serde::{Deserialize, Serialize};

use crate::{
    FiniteF32Bits, HistoricalReductionAdmissionInputs, HistoricalReductionCaseArtifact,
    HistoricalReductionControlError, HistoricalReductionCorpusArtifact,
    HistoricalReductionExecutionReceiptArtifact, HistoricalReductionExecutionSubjectV1,
    PreparedHistoricalReductionAdmission, PreparedHistoricalReductionJob, ReductionUlpDistance,
    ValidatedHistoricalReductionRun, ValidatedVariantBuild, VariantBuildReceiptArtifact,
    compose_historical_reduction_admission,
};

/// One recomputable reference/candidate numerical comparison.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReductionCandidateCaseV1 {
    case: ContentId<HistoricalReductionCaseArtifact>,
    reference: FiniteF32Bits,
    candidate: FiniteF32Bits,
    ulp_distance: ReductionUlpDistance,
}

impl HistoricalReductionCandidateCaseV1 {
    #[must_use]
    pub const fn case(self) -> ContentId<HistoricalReductionCaseArtifact> {
        self.case
    }

    #[must_use]
    pub const fn ulp_distance(self) -> ReductionUlpDistance {
        self.ulp_distance
    }
}

/// Exact numerical evidence used by one historical reduction candidate verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionCandidateComparisonWire")]
pub struct HistoricalReductionCandidateComparisonV1 {
    schema_version: u16,
    admitted_oracle: ContentId<cairn_verification::AdmittedOracleArtifact>,
    reference_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    candidate: ContentId<ImplementationBundleArtifact>,
    build: ContentId<VariantBuildReceiptArtifact>,
    candidate_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    cases: Vec<HistoricalReductionCandidateCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionCandidateComparisonWire {
    schema_version: u16,
    admitted_oracle: ContentId<cairn_verification::AdmittedOracleArtifact>,
    reference_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    candidate: ContentId<ImplementationBundleArtifact>,
    build: ContentId<VariantBuildReceiptArtifact>,
    candidate_execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    cases: Vec<HistoricalReductionCandidateCaseV1>,
}

impl TryFrom<HistoricalReductionCandidateComparisonWire>
    for HistoricalReductionCandidateComparisonV1
{
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionCandidateComparisonWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        if wire.cases.is_empty()
            || wire
                .cases
                .windows(2)
                .any(|cases| cases[0].case.to_wire() >= cases[1].case.to_wire())
            || wire.cases.iter().any(|case| {
                case.ulp_distance != ReductionUlpDistance::between(case.reference, case.candidate)
            })
        {
            return Err(HistoricalReductionControlError::InconsistentCandidateComparison);
        }
        Ok(Self {
            schema_version: 1,
            admitted_oracle: wire.admitted_oracle,
            reference_execution: wire.reference_execution,
            candidate: wire.candidate,
            build: wire.build,
            candidate_execution: wire.candidate_execution,
            environment: wire.environment,
            corpus: wire.corpus,
            allowance: wire.allowance,
            cases: wire.cases,
        })
    }
}

impl HistoricalReductionCandidateComparisonV1 {
    #[must_use]
    pub fn cases(&self) -> &[HistoricalReductionCandidateCaseV1] {
        &self.cases
    }
}

/// Content domain for exact historical reduction candidate comparison evidence.
pub enum HistoricalReductionCandidateComparisonArtifact {}

impl ContentType for HistoricalReductionCandidateComparisonArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-candidate-comparison.v1";
}

/// Prepared product comparison and terminal domain-neutral verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionCandidateVerdict {
    comparison: HistoricalReductionCandidateComparisonV1,
    comparison_bytes: Vec<u8>,
    comparison_id: ContentId<HistoricalReductionCandidateComparisonArtifact>,
    verdict_input: CandidateVerdictInput,
    verdict: PreparedCandidateVerdict,
}

impl PreparedHistoricalReductionCandidateVerdict {
    #[must_use]
    pub const fn comparison(&self) -> &HistoricalReductionCandidateComparisonV1 {
        &self.comparison
    }

    #[must_use]
    pub fn comparison_bytes(&self) -> &[u8] {
        &self.comparison_bytes
    }

    #[must_use]
    pub const fn comparison_id(&self) -> ContentId<HistoricalReductionCandidateComparisonArtifact> {
        self.comparison_id
    }

    #[must_use]
    pub const fn verdict(&self) -> &PreparedCandidateVerdict {
        &self.verdict
    }

    /// Revalidates a persisted generic verdict against this product-computed comparison input.
    ///
    /// # Errors
    ///
    /// Rejects changed outcome/failed-case metadata or any changed admitted-oracle edge.
    pub fn validate_verdict(
        &self,
        verdict: &CandidateVerdictV1,
        admitted: &PreparedHistoricalReductionAdmission,
    ) -> Result<(), HistoricalReductionControlError> {
        verdict
            .validate_inputs(
                self.verdict_input.clone(),
                admitted.oracle(),
                admitted.receipt(),
            )
            .map_err(composition)
    }
}

/// Independent evidence required to judge one candidate.
pub struct HistoricalReductionCandidateInputs<'a> {
    pub admitted: &'a PreparedHistoricalReductionAdmission,
    pub admission_inputs: &'a HistoricalReductionAdmissionInputs<'a>,
    pub build: &'a ValidatedVariantBuild,
    pub job: &'a PreparedHistoricalReductionJob,
    pub run: &'a ValidatedHistoricalReductionRun,
}

/// Recomputes admission and candidate observations before emitting a terminal verdict.
///
/// # Errors
///
/// Rejects a changed admitted graph, non-candidate run, different build/environment/corpus,
/// inadmissible allowance, or any comparison fact that does not match exact output bits.
#[expect(
    clippy::too_many_lines,
    reason = "the candidate trust boundary visibly sequences admission, role, execution, comparison, and verdict recomputation"
)]
pub fn compose_historical_reduction_candidate_verdict(
    input: &HistoricalReductionCandidateInputs<'_>,
) -> Result<PreparedHistoricalReductionCandidateVerdict, HistoricalReductionControlError> {
    let recomputed_admission = compose_historical_reduction_admission(input.admission_inputs)?;
    if recomputed_admission != *input.admitted {
        return Err(HistoricalReductionControlError::InconsistentControl);
    }
    crate::reduction_control::validate_control_run(
        input.admission_inputs.corpus,
        input.admission_inputs.reference_job,
        input.admission_inputs.reference_run,
    )?;
    crate::reduction_control::validate_control_run(
        input.admission_inputs.corpus,
        input.job,
        input.run,
    )?;

    let (candidate, build) = match input.job.plan().subject() {
        HistoricalReductionExecutionSubjectV1::Candidate {
            implementation,
            build,
        } => (*implementation, *build),
        HistoricalReductionExecutionSubjectV1::Reference { .. }
        | HistoricalReductionExecutionSubjectV1::AdmissionVariant { .. } => {
            return Err(HistoricalReductionControlError::CandidateOutsideAdmission);
        }
    };
    if candidate != input.build.build_receipt().implementation()
        || build != input.build.build_receipt_id()
        || input.job.plan().corpus() != input.admission_inputs.corpus.corpus_id()
        || input.admission_inputs.reference_run.execution_receipt_id()
            != input
                .admission_inputs
                .control
                .control()
                .reference_execution()
    {
        return Err(HistoricalReductionControlError::CandidateOutsideAdmission);
    }

    let allowance = reduction_allowance(input.admission_inputs.allowance)?;
    let allowance_bytes =
        cairn_codec::to_vec(input.admission_inputs.allowance).map_err(composition)?;
    let allowance_id =
        ContentId::<NumericalAllowanceArtifact>::derive(&allowance_bytes).map_err(composition)?;
    if allowance_id != input.admitted.oracle().oracle().allowance() {
        return Err(HistoricalReductionControlError::CandidateOutsideAdmission);
    }
    let cases = compare_outputs(input.admission_inputs.reference_run, input.run)?;
    let comparison = HistoricalReductionCandidateComparisonV1::try_from(
        HistoricalReductionCandidateComparisonWire {
            schema_version: 1,
            admitted_oracle: input.admitted.oracle().oracle_id(),
            reference_execution: input.admission_inputs.reference_run.execution_receipt_id(),
            candidate,
            build,
            candidate_execution: input.run.execution_receipt_id(),
            environment: input.job.plan().environment(),
            corpus: input.job.plan().corpus(),
            allowance: allowance_id,
            cases,
        },
    )?;
    let comparison_bytes = cairn_codec::to_vec(&comparison).map_err(composition)?;
    let comparison_id =
        ContentId::<HistoricalReductionCandidateComparisonArtifact>::derive(&comparison_bytes)
            .map_err(composition)?;
    let failed_cases = comparison
        .cases
        .iter()
        .filter(|case| case.ulp_distance > allowance)
        .map(|case| {
            let bytes = cairn_codec::to_vec(case).map_err(composition)?;
            ContentId::<CandidateFailedCaseArtifact>::derive(&bytes).map_err(composition)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let environment = ContentId::<AdmissionEnvironmentArtifact>::derive(
        input.job.plan().environment().to_wire().as_bytes(),
    )
    .map_err(composition)?;
    let candidate_wire = candidate.to_wire();
    let verdict_input = CandidateVerdictInput {
        candidate: reidentify::<CandidateArtifact>(candidate_wire.as_bytes())?,
        source: reidentify::<CandidateSourceArtifact>(candidate_wire.as_bytes())?,
        build: reidentify::<CandidateBuildArtifact>(input.build.build_receipt_bytes())?,
        run: reidentify::<CandidateRunArtifact>(input.run.execution_receipt_bytes())?,
        environment,
        corpus: ContentId::<AdmissionCorpusArtifact>::derive(
            input.admission_inputs.corpus.corpus_bytes(),
        )
        .map_err(composition)?,
        comparison: reidentify::<CandidateComparisonArtifact>(&comparison_bytes)?,
        failed_cases,
    };
    let verdict = prepare_candidate_verdict(
        verdict_input.clone(),
        input.admitted.oracle(),
        input.admitted.receipt(),
    )
    .map_err(composition)?;
    verdict
        .verdict()
        .validate_inputs(
            verdict_input.clone(),
            input.admitted.oracle(),
            input.admitted.receipt(),
        )
        .map_err(composition)?;
    Ok(PreparedHistoricalReductionCandidateVerdict {
        comparison,
        comparison_bytes,
        comparison_id,
        verdict_input,
        verdict,
    })
}

fn reduction_allowance(
    allowance: &cairn_verification::NumericalAllowanceV1,
) -> Result<ReductionUlpDistance, HistoricalReductionControlError> {
    let Some(absolute) = allowance.absolute() else {
        return Err(HistoricalReductionControlError::InadmissibleAllowance);
    };
    if allowance.relative().is_some() {
        return Err(HistoricalReductionControlError::InadmissibleAllowance);
    }
    absolute
        .as_str()
        .parse::<u32>()
        .map(ReductionUlpDistance::new)
        .map_err(|_| HistoricalReductionControlError::InadmissibleAllowance)
}

fn compare_outputs(
    reference: &ValidatedHistoricalReductionRun,
    candidate: &ValidatedHistoricalReductionRun,
) -> Result<Vec<HistoricalReductionCandidateCaseV1>, HistoricalReductionControlError> {
    if reference.observation().corpus() != candidate.observation().corpus()
        || reference.observation().outputs().len() != candidate.observation().outputs().len()
    {
        return Err(HistoricalReductionControlError::InconsistentCandidateComparison);
    }
    reference
        .observation()
        .outputs()
        .iter()
        .zip(candidate.observation().outputs())
        .map(|(reference, candidate)| {
            if reference.case() != candidate.case() {
                return Err(HistoricalReductionControlError::InconsistentCandidateComparison);
            }
            Ok(HistoricalReductionCandidateCaseV1 {
                case: reference.case(),
                reference: reference.value(),
                candidate: candidate.value(),
                ulp_distance: ReductionUlpDistance::between(reference.value(), candidate.value()),
            })
        })
        .collect()
}

fn reidentify<T: ContentType>(
    bytes: &[u8],
) -> Result<ContentId<T>, HistoricalReductionControlError> {
    ContentId::derive(bytes).map_err(composition)
}

fn composition(error: impl std::fmt::Display) -> HistoricalReductionControlError {
    HistoricalReductionControlError::Composition {
        message: error.to_string(),
    }
}
