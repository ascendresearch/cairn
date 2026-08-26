//! Executed mutation composition for the hardware-free historical reduction control.

use cairn_execution::ExecutionEnvironmentArtifact;
use cairn_protocol::{ContentId, ContentType};
use cairn_verification::{
    AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyV1, FaultClassName,
    FaultInjectionEvidenceArtifact, ImplementationBundleArtifact, ImplementationVariantArtifact,
    ImplementationVariantV1, MutationCaseArtifact, MutationComparisonArtifact, MutationDetection,
    MutationExecutionArtifact, MutationGridCellV1, MutationInjectionArtifact, MutationSizing,
    MutationTrialV1, NumericalAllowanceArtifact, NumericalAllowanceV1, PreparedGenericMutantSet,
    PreparedMutationGrid, PreparedMutationGridProof, TrustedMutantDefinitionArtifact,
    TrustedMutantV1, VariantExpectation, prepare_generic_mutant_set, prepare_mutation_grid,
    recompute_mutation_grid_proof,
};
use serde::{Deserialize, Serialize};

use crate::{
    FiniteF32Bits, HistoricalReductionAlgorithm, HistoricalReductionCaseArtifact,
    HistoricalReductionControlError, HistoricalReductionCorpusArtifact,
    HistoricalReductionExecutionReceiptArtifact, HistoricalReductionExecutionSubjectV1,
    PreparedHistoricalReductionCorpus, PreparedHistoricalReductionJob, ReductionUlpDistance,
    ValidatedHistoricalReductionRun, ValidatedVariantBuild, VariantBuildReceiptArtifact,
};

/// Closed trusted mutations implemented by the historical reduction host fixtures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalReductionMutationKind {
    /// Omit the final input element.
    DropLast,
    /// Add one to the completed reduction.
    UnitOffset,
    /// Replace the completed reduction with zero.
    ZeroOutput,
}

impl HistoricalReductionMutationKind {
    const ALL: [Self; 3] = [Self::DropLast, Self::UnitOffset, Self::ZeroOutput];

    const fn name(self) -> &'static str {
        match self {
            Self::DropLast => "drop-last",
            Self::UnitOffset => "unit-offset",
            Self::ZeroOutput => "zero-output",
        }
    }

    const fn algorithm(self) -> HistoricalReductionAlgorithm {
        match self {
            Self::DropLast => HistoricalReductionAlgorithm::DropLast,
            Self::UnitOffset => HistoricalReductionAlgorithm::UnitOffset,
            Self::ZeroOutput => HistoricalReductionAlgorithm::ZeroOutput,
        }
    }

    const fn sizing(self) -> MutationSizing {
        match self {
            Self::DropLast => MutationSizing::CaseDependent,
            Self::UnitOffset | Self::ZeroOutput => MutationSizing::ScaleFree,
        }
    }

    fn fault_class(self) -> Result<FaultClassName, HistoricalReductionControlError> {
        FaultClassName::new(self.name()).map_err(composition)
    }

    fn definition(
        self,
    ) -> Result<ContentId<TrustedMutantDefinitionArtifact>, HistoricalReductionControlError> {
        ContentId::derive(self.name().as_bytes()).map_err(composition)
    }
}

/// Prepares the exact product-owned trusted-mutant set used by reduction admission.
///
/// # Errors
///
/// Returns an error if typed names, content identities, or canonical ordering cannot be produced.
pub fn prepare_historical_reduction_mutant_set()
-> Result<PreparedGenericMutantSet, HistoricalReductionControlError> {
    let mut mutants = HistoricalReductionMutationKind::ALL
        .into_iter()
        .map(|kind| {
            Ok(TrustedMutantV1::new(
                kind.definition()?,
                kind.fault_class()?,
            ))
        })
        .collect::<Result<Vec<_>, HistoricalReductionControlError>>()?;
    mutants.sort_by_key(|mutant| mutant.definition().to_wire());
    prepare_generic_mutant_set(mutants).map_err(composition)
}

/// Exact product evidence that a trusted mutation selected one built wrong implementation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionMutationInjectionWire")]
pub struct HistoricalReductionMutationInjectionV1 {
    schema_version: u16,
    subject: ContentId<ImplementationBundleArtifact>,
    mutant: ContentId<TrustedMutantDefinitionArtifact>,
    kind: HistoricalReductionMutationKind,
    variant: ContentId<ImplementationVariantArtifact>,
    fault_evidence: ContentId<FaultInjectionEvidenceArtifact>,
    injected_implementation: ContentId<ImplementationBundleArtifact>,
    build: ContentId<VariantBuildReceiptArtifact>,
    algorithm: HistoricalReductionAlgorithm,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionMutationInjectionWire {
    schema_version: u16,
    subject: ContentId<ImplementationBundleArtifact>,
    mutant: ContentId<TrustedMutantDefinitionArtifact>,
    kind: HistoricalReductionMutationKind,
    variant: ContentId<ImplementationVariantArtifact>,
    fault_evidence: ContentId<FaultInjectionEvidenceArtifact>,
    injected_implementation: ContentId<ImplementationBundleArtifact>,
    build: ContentId<VariantBuildReceiptArtifact>,
    algorithm: HistoricalReductionAlgorithm,
}

impl TryFrom<HistoricalReductionMutationInjectionWire> for HistoricalReductionMutationInjectionV1 {
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionMutationInjectionWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        if wire.subject == wire.injected_implementation
            || wire.mutant != wire.kind.definition()?
            || wire.algorithm != wire.kind.algorithm()
        {
            return Err(HistoricalReductionControlError::InconsistentMutationInjection);
        }
        Ok(Self {
            schema_version: 1,
            subject: wire.subject,
            mutant: wire.mutant,
            kind: wire.kind,
            variant: wire.variant,
            fault_evidence: wire.fault_evidence,
            injected_implementation: wire.injected_implementation,
            build: wire.build,
            algorithm: wire.algorithm,
        })
    }
}

impl HistoricalReductionMutationInjectionV1 {
    #[must_use]
    pub const fn kind(&self) -> HistoricalReductionMutationKind {
        self.kind
    }

    #[must_use]
    pub const fn mutant(&self) -> ContentId<TrustedMutantDefinitionArtifact> {
        self.mutant
    }

    #[must_use]
    pub const fn injected_implementation(&self) -> ContentId<ImplementationBundleArtifact> {
        self.injected_implementation
    }
}

/// Content domain for one exact reduction mutation injection.
pub enum HistoricalReductionMutationInjectionArtifact {}

impl ContentType for HistoricalReductionMutationInjectionArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-mutation-injection.v1";
}

/// Exact per-case comparison of an executed reduction mutation against the reference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalReductionMutationCaseComparisonWire")]
pub struct HistoricalReductionMutationCaseComparisonV1 {
    schema_version: u16,
    cell: MutationGridCellV1,
    historical_case: ContentId<HistoricalReductionCaseArtifact>,
    injection: ContentId<HistoricalReductionMutationInjectionArtifact>,
    execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    maximum_ulp_distance: ReductionUlpDistance,
    reference: FiniteF32Bits,
    mutated: FiniteF32Bits,
    ulp_distance: ReductionUlpDistance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalReductionMutationCaseComparisonWire {
    schema_version: u16,
    cell: MutationGridCellV1,
    historical_case: ContentId<HistoricalReductionCaseArtifact>,
    injection: ContentId<HistoricalReductionMutationInjectionArtifact>,
    execution: ContentId<HistoricalReductionExecutionReceiptArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    corpus: ContentId<HistoricalReductionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    maximum_ulp_distance: ReductionUlpDistance,
    reference: FiniteF32Bits,
    mutated: FiniteF32Bits,
    ulp_distance: ReductionUlpDistance,
}

impl TryFrom<HistoricalReductionMutationCaseComparisonWire>
    for HistoricalReductionMutationCaseComparisonV1
{
    type Error = HistoricalReductionControlError;

    fn try_from(wire: HistoricalReductionMutationCaseComparisonWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(HistoricalReductionControlError::UnsupportedSchemaVersion);
        }
        if wire.ulp_distance != ReductionUlpDistance::between(wire.reference, wire.mutated) {
            return Err(HistoricalReductionControlError::InconsistentMutationComparison);
        }
        Ok(Self {
            schema_version: 1,
            cell: wire.cell,
            historical_case: wire.historical_case,
            injection: wire.injection,
            execution: wire.execution,
            environment: wire.environment,
            corpus: wire.corpus,
            allowance: wire.allowance,
            maximum_ulp_distance: wire.maximum_ulp_distance,
            reference: wire.reference,
            mutated: wire.mutated,
            ulp_distance: wire.ulp_distance,
        })
    }
}

impl HistoricalReductionMutationCaseComparisonV1 {
    #[must_use]
    pub const fn cell(self) -> MutationGridCellV1 {
        self.cell
    }

    #[must_use]
    pub const fn ulp_distance(self) -> ReductionUlpDistance {
        self.ulp_distance
    }

    #[must_use]
    pub const fn maximum_ulp_distance(self) -> ReductionUlpDistance {
        self.maximum_ulp_distance
    }
}

/// Content domain for exact per-case reduction mutation comparisons.
pub enum HistoricalReductionMutationCaseComparisonArtifact {}

impl ContentType for HistoricalReductionMutationCaseComparisonArtifact {
    const DOMAIN: &'static str = "migration.historical-reduction-mutation-comparison.v1";
}

/// One wrong build and real admission-variant run selected by a trusted mutation.
pub struct HistoricalReductionMutationVariantEvidence<'a> {
    pub kind: HistoricalReductionMutationKind,
    pub variant: &'a ImplementationVariantV1,
    pub build: &'a ValidatedVariantBuild,
    pub job: &'a PreparedHistoricalReductionJob,
    pub run: &'a ValidatedHistoricalReductionRun,
}

/// Independent inputs required to build the executed mutation grid.
pub struct HistoricalReductionMutationInputs<'a> {
    pub policy: &'a AdmissionPolicyV1,
    pub mutant_set: &'a PreparedGenericMutantSet,
    pub subject: ContentId<ImplementationBundleArtifact>,
    pub allowance: &'a NumericalAllowanceV1,
    pub corpus: &'a PreparedHistoricalReductionCorpus,
    pub reference_job: &'a PreparedHistoricalReductionJob,
    pub reference_run: &'a ValidatedHistoricalReductionRun,
    pub variants: &'a [HistoricalReductionMutationVariantEvidence<'a>],
}

/// Product evidence and the generic grid/proof derived from actual executions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionMutationGrid {
    mutant_set: PreparedGenericMutantSet,
    injections: Vec<HistoricalReductionMutationInjectionV1>,
    comparisons: Vec<HistoricalReductionMutationCaseComparisonV1>,
    grid: PreparedMutationGrid,
    proof: PreparedMutationGridProof,
}

impl PreparedHistoricalReductionMutationGrid {
    #[must_use]
    pub const fn mutant_set(&self) -> &PreparedGenericMutantSet {
        &self.mutant_set
    }

    #[must_use]
    pub fn injections(&self) -> &[HistoricalReductionMutationInjectionV1] {
        &self.injections
    }

    #[must_use]
    pub fn comparisons(&self) -> &[HistoricalReductionMutationCaseComparisonV1] {
        &self.comparisons
    }

    #[must_use]
    pub const fn grid(&self) -> &PreparedMutationGrid {
        &self.grid
    }

    #[must_use]
    pub const fn proof(&self) -> &PreparedMutationGridProof {
        &self.proof
    }
}

/// Recomputes an actual mutant injection, execution, and comparison grid.
///
/// Every generic trial identity is derived from strict product evidence. Mutant observations must
/// come from the same admission-variant execution path as the red controls; callers cannot provide
/// a detection flag or opaque evidence identity.
///
/// # Errors
///
/// Rejects a changed mutant set, wrong variant/build/run relation, different corpus/environment,
/// incomplete mutation family, or comparison that is not the exact reference/run recomputation.
#[expect(
    clippy::too_many_lines,
    reason = "the mutation trust boundary keeps injection, build, execution, comparison, and generic-grid derivation visibly sequenced"
)]
pub fn compose_historical_reduction_mutation_grid(
    input: &HistoricalReductionMutationInputs<'_>,
) -> Result<PreparedHistoricalReductionMutationGrid, HistoricalReductionControlError> {
    let expected = prepare_historical_reduction_mutant_set()?;
    if expected != *input.mutant_set
        || input.policy.mutant_set() != input.mutant_set.mutant_set_id()
        || input.variants.len() != HistoricalReductionMutationKind::ALL.len()
    {
        return Err(HistoricalReductionControlError::InconsistentMutationInjection);
    }
    crate::reduction_control::validate_control_run(
        input.corpus,
        input.reference_job,
        input.reference_run,
    )?;
    if !matches!(
        input.reference_job.plan().subject(),
        HistoricalReductionExecutionSubjectV1::Reference { .. }
    ) || input.reference_job.plan().algorithm()
        != HistoricalReductionAlgorithm::HighPrecisionReference
    {
        return Err(HistoricalReductionControlError::InconsistentMutationComparison);
    }

    let maximum_ulp_distance = reduction_allowance(input.allowance)?;
    let allowance_bytes = cairn_codec::to_vec(input.allowance).map_err(composition)?;
    let allowance_id =
        ContentId::<NumericalAllowanceArtifact>::derive(&allowance_bytes).map_err(composition)?;
    let admission_corpus =
        ContentId::<AdmissionCorpusArtifact>::derive(input.corpus.corpus_bytes())
            .map_err(composition)?;

    let mut seen = Vec::new();
    let mut injections = Vec::new();
    let mut comparisons = Vec::new();
    let mut cases = Vec::new();
    for case in input.corpus.corpus().cases() {
        let case_bytes = cairn_codec::to_vec(case.body()).map_err(composition)?;
        cases.push(ContentId::<MutationCaseArtifact>::derive(&case_bytes).map_err(composition)?);
    }
    cases.sort_by_key(ContentId::to_wire);

    for evidence in input.variants {
        if seen.contains(&evidence.kind) {
            return Err(HistoricalReductionControlError::InconsistentMutationInjection);
        }
        seen.push(evidence.kind);
        let definition = evidence.kind.definition()?;
        let mutant = input
            .mutant_set
            .mutant_set()
            .mutants()
            .iter()
            .find(|mutant| mutant.definition() == definition)
            .ok_or(HistoricalReductionControlError::InconsistentMutationInjection)?;
        let fault_class = evidence.kind.fault_class()?;
        let VariantExpectation::MustReject {
            fault_class: variant_fault_class,
            fault_evidence,
        } = evidence.variant.expectation()
        else {
            return Err(HistoricalReductionControlError::InconsistentMutationInjection);
        };
        let variant_bytes = cairn_codec::to_vec(evidence.variant).map_err(composition)?;
        let variant_id = ContentId::<ImplementationVariantArtifact>::derive(&variant_bytes)
            .map_err(composition)?;
        let implementation = evidence.variant.implementation();
        if mutant.fault_class() != &fault_class
            || variant_fault_class != &fault_class
            || implementation == input.subject
            || evidence.build.build_receipt().variant() != variant_id
            || evidence.build.build_receipt().implementation() != implementation
        {
            return Err(HistoricalReductionControlError::InconsistentMutationInjection);
        }
        crate::reduction_control::validate_control_run(input.corpus, evidence.job, evidence.run)?;
        if evidence.job.plan().environment() != input.reference_job.plan().environment()
            || evidence.job.plan().corpus() != input.corpus.corpus_id()
            || evidence.job.plan().algorithm() != evidence.kind.algorithm()
            || !matches!(
                evidence.job.plan().subject(),
                HistoricalReductionExecutionSubjectV1::AdmissionVariant {
                    variant,
                    implementation: job_implementation,
                    build,
                } if *variant == variant_id
                    && *job_implementation == implementation
                    && *build == evidence.build.build_receipt_id()
            )
        {
            return Err(HistoricalReductionControlError::InconsistentMutationInjection);
        }

        let injection = HistoricalReductionMutationInjectionV1::try_from(
            HistoricalReductionMutationInjectionWire {
                schema_version: 1,
                subject: input.subject,
                mutant: mutant.definition(),
                kind: evidence.kind,
                variant: variant_id,
                fault_evidence: *fault_evidence,
                injected_implementation: implementation,
                build: evidence.build.build_receipt_id(),
                algorithm: evidence.kind.algorithm(),
            },
        )?;
        let injection_bytes = cairn_codec::to_vec(&injection).map_err(composition)?;
        let injection_id =
            ContentId::<HistoricalReductionMutationInjectionArtifact>::derive(&injection_bytes)
                .map_err(composition)?;
        let generic_injection = ContentId::<MutationInjectionArtifact>::derive(&injection_bytes)
            .map_err(composition)?;
        let generic_execution =
            ContentId::<MutationExecutionArtifact>::derive(evidence.run.execution_receipt_bytes())
                .map_err(composition)?;

        if input.reference_run.observation().outputs().len()
            != evidence.run.observation().outputs().len()
        {
            return Err(HistoricalReductionControlError::InconsistentMutationComparison);
        }

        for (reference, mutated) in input
            .reference_run
            .observation()
            .outputs()
            .iter()
            .zip(evidence.run.observation().outputs())
        {
            if reference.case() != mutated.case() {
                return Err(HistoricalReductionControlError::InconsistentMutationComparison);
            }
            let case = input
                .corpus
                .corpus()
                .cases()
                .iter()
                .find(|case| case.case() == reference.case())
                .ok_or(HistoricalReductionControlError::InconsistentMutationComparison)?;
            let case_bytes = cairn_codec::to_vec(case.body()).map_err(composition)?;
            let mutation_case =
                ContentId::<MutationCaseArtifact>::derive(&case_bytes).map_err(composition)?;
            let cell = MutationGridCellV1::new(mutant.definition(), mutation_case);
            let ulp_distance = ReductionUlpDistance::between(reference.value(), mutated.value());
            let comparison = HistoricalReductionMutationCaseComparisonV1::try_from(
                HistoricalReductionMutationCaseComparisonWire {
                    schema_version: 1,
                    cell,
                    historical_case: reference.case(),
                    injection: injection_id,
                    execution: evidence.run.execution_receipt_id(),
                    environment: evidence.job.plan().environment(),
                    corpus: input.corpus.corpus_id(),
                    allowance: allowance_id,
                    maximum_ulp_distance,
                    reference: reference.value(),
                    mutated: mutated.value(),
                    ulp_distance,
                },
            )?;
            comparisons.push((
                comparison,
                MutationTrialV1::applied(
                    cell,
                    evidence.kind.sizing(),
                    generic_injection,
                    generic_execution,
                    vec![
                        AdmissionExecutionScope::ObservationPipeline,
                        AdmissionExecutionScope::Implementation,
                    ],
                    ContentId::<MutationComparisonArtifact>::derive(
                        &cairn_codec::to_vec(&comparison).map_err(composition)?,
                    )
                    .map_err(composition)?,
                    if ulp_distance > maximum_ulp_distance {
                        MutationDetection::Detected
                    } else {
                        MutationDetection::Missed
                    },
                ),
            ));
        }
        injections.push(injection);
    }
    seen.sort();
    if seen != HistoricalReductionMutationKind::ALL {
        return Err(HistoricalReductionControlError::InconsistentMutationInjection);
    }
    injections.sort_by_key(|injection| injection.mutant().to_wire());
    comparisons.sort_by_key(|(_, trial)| {
        (
            trial.cell().mutant().to_wire(),
            trial.cell().case().to_wire(),
        )
    });
    let (comparisons, trials): (Vec<_>, Vec<_>) = comparisons.into_iter().unzip();
    let grid = prepare_mutation_grid(
        input.policy,
        input.mutant_set,
        input.subject,
        admission_corpus,
        cases,
        trials,
    )
    .map_err(composition)?;
    let proof = recompute_mutation_grid_proof(input.policy, input.mutant_set, &grid)
        .map_err(composition)?;
    Ok(PreparedHistoricalReductionMutationGrid {
        mutant_set: input.mutant_set.clone(),
        injections,
        comparisons,
        grid,
        proof,
    })
}

fn reduction_allowance(
    allowance: &NumericalAllowanceV1,
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

fn composition(error: impl std::fmt::Display) -> HistoricalReductionControlError {
    HistoricalReductionControlError::Composition {
        message: error.to_string(),
    }
}
