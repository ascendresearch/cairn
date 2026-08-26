//! Final admitted-oracle composition for the historical reduction control.

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::{
    AdmissionAssumptionV1, AdmissionControlArtifact, AdmissionCoverageArtifact,
    AdmissionDisagreementDispositionV1, AdmissionEnvironmentArtifact,
    AdmissionRevalidationPolicyV1, AdmissionSaturationRoundV1, AdmissionUnverifiedClaimV1,
    AdmissionVariantTrialArtifact, AdmittedDomainExclusionArtifact, AdmittedReceiptInput,
    OracleStrength, PreparedAdmissionReceipt, PreparedAdmittedDomain, PreparedAdmittedOracle,
    SourceAdmissionObservationArtifact, prepare_admission_receipt, prepare_admitted_domain,
    prepare_admitted_oracle,
};

use crate::{
    HistoricalFailureCoverageV1, HistoricalFailureObligationV1, HistoricalFailureRecordV1,
    HistoricalReductionCaseArtifact, HistoricalReductionControlError,
    HistoricalReductionCorrectVariantEvidence, HistoricalReductionWrongVariantEvidence,
    MigrationDomainContractV1, PreparedHistoricalReductionControl,
    PreparedHistoricalReductionCorpus, PreparedHistoricalReductionJob,
    PreparedHistoricalReductionMutationGrid, ValidatedHistoricalReductionRun,
};
use cairn_verification::{
    AdmissionPolicyV1, CorpusProposalV1, DeclaredDomainV1, ImplementationVariantArtifact,
    NumericalAllowanceV1, OracleProposalV1,
};

/// Every independently validated input required to promote the historical control.
pub struct HistoricalReductionAdmissionInputs<'a> {
    pub control: &'a PreparedHistoricalReductionControl,
    pub domain: &'a MigrationDomainContractV1,
    pub declared_domain: &'a DeclaredDomainV1,
    pub corpus_proposal: &'a CorpusProposalV1,
    pub proposal: &'a OracleProposalV1,
    pub historical_record: &'a HistoricalFailureRecordV1,
    pub historical_obligation: &'a HistoricalFailureObligationV1,
    pub historical_coverage: &'a HistoricalFailureCoverageV1,
    pub policy: &'a AdmissionPolicyV1,
    pub allowance: &'a NumericalAllowanceV1,
    pub corpus: &'a PreparedHistoricalReductionCorpus,
    pub old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
    pub old_baseline_variant: ContentId<ImplementationVariantArtifact>,
    pub reference_job: &'a PreparedHistoricalReductionJob,
    pub reference_run: &'a ValidatedHistoricalReductionRun,
    pub correct: &'a [HistoricalReductionCorrectVariantEvidence<'a>],
    pub wrong: &'a [HistoricalReductionWrongVariantEvidence<'a>],
    pub mutation: &'a PreparedHistoricalReductionMutationGrid,
    pub saturation_rounds: &'a [AdmissionSaturationRoundV1],
    pub revalidation: &'a AdmissionRevalidationPolicyV1,
}

/// Complete prepared domain, receipt, and admitted-oracle artifacts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedHistoricalReductionAdmission {
    admitted_domain: PreparedAdmittedDomain,
    receipt: PreparedAdmissionReceipt,
    oracle: PreparedAdmittedOracle,
}

impl PreparedHistoricalReductionAdmission {
    #[must_use]
    pub const fn admitted_domain(&self) -> &PreparedAdmittedDomain {
        &self.admitted_domain
    }

    #[must_use]
    pub const fn receipt(&self) -> &PreparedAdmissionReceipt {
        &self.receipt
    }

    #[must_use]
    pub const fn oracle(&self) -> &PreparedAdmittedOracle {
        &self.oracle
    }
}

/// Recomputes the complete historical control and emits the first immutable admitted oracle.
///
/// # Errors
///
/// Rejects any changed product evidence, incomplete generic receipt edge, insufficient saturation
/// evidence, or non-canonical final manifest.
#[expect(
    clippy::too_many_lines,
    reason = "the final trust boundary keeps product recomputation and every receipt edge visibly sequenced"
)]
pub fn compose_historical_reduction_admission(
    input: &HistoricalReductionAdmissionInputs<'_>,
) -> Result<PreparedHistoricalReductionAdmission, HistoricalReductionControlError> {
    input.control.control().validate_inputs(
        input.domain,
        input.declared_domain,
        input.corpus_proposal,
        input.proposal,
        input.historical_record,
        input.historical_obligation,
        input.historical_coverage,
        input.policy,
        input.allowance,
        input.corpus,
        input.old_sample_case,
        input.old_baseline_variant,
        input.reference_job,
        input.reference_run,
        input.correct,
        input.wrong,
        input.mutation,
    )?;

    let exclusions = input
        .domain
        .exclusions()
        .iter()
        .map(|exclusion| {
            reidentify::<AdmittedDomainExclusionArtifact>(exclusion.to_wire().as_bytes())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let admitted_domain =
        prepare_admitted_domain(input.proposal, input.declared_domain, exclusions)
            .map_err(composition)?;

    let mut environments = std::iter::once(input.reference_job)
        .chain(input.correct.iter().map(|evidence| evidence.job))
        .chain(input.wrong.iter().map(|evidence| evidence.job))
        .map(|job| {
            reidentify::<AdmissionEnvironmentArtifact>(
                job.plan().environment().to_wire().as_bytes(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    environments.sort_by_key(ContentId::to_wire);
    environments.dedup();

    let mut correct_variant_trials = input
        .control
        .control()
        .correct_trials()
        .iter()
        .map(trial_identity)
        .collect::<Result<Vec<_>, _>>()?;
    correct_variant_trials.sort_by_key(ContentId::to_wire);
    let mut wrong_variant_trials = input
        .control
        .control()
        .wrong_trials()
        .iter()
        .map(trial_identity)
        .collect::<Result<Vec<_>, _>>()?;
    wrong_variant_trials.sort_by_key(ContentId::to_wire);

    let coverage_bytes = cairn_codec::to_vec(input.historical_coverage).map_err(composition)?;
    let source_observation = reidentify::<SourceAdmissionObservationArtifact>(
        input.reference_run.execution_receipt_bytes(),
    )?;
    let receipt_input = AdmittedReceiptInput {
        admitted_domain: admitted_domain.domain_id(),
        admission_corpus: input.mutation.grid().grid().corpus(),
        admission_control: reidentify::<AdmissionControlArtifact>(input.control.control_bytes())?,
        environments,
        source_observations: vec![source_observation],
        execution_scopes: input.policy.required_execution_scopes().to_vec(),
        correct_variant_trials,
        wrong_variant_trials,
        saturation_rounds: input.saturation_rounds.to_vec(),
        coverage: vec![reidentify::<AdmissionCoverageArtifact>(&coverage_bytes)?],
        disagreement: AdmissionDisagreementDispositionV1::NoneObserved,
        assumptions: vec![
            AdmissionAssumptionV1::ConstructionEvidenceIndependent,
            AdmissionAssumptionV1::HostEnvironmentStable,
        ],
        unverified_claims: vec![
            AdmissionUnverifiedClaimV1::SourceAcceleratorBehavior,
            AdmissionUnverifiedClaimV1::TargetBuildBehavior,
            AdmissionUnverifiedClaimV1::TargetDeviceBehavior,
            AdmissionUnverifiedClaimV1::TargetSpecificFailureCoverage,
            AdmissionUnverifiedClaimV1::DeviceRunnerIndependentAttestation,
        ],
        admitted_strength: OracleStrength::Reference,
    };
    let receipt = prepare_admission_receipt(
        receipt_input.clone(),
        input.proposal,
        input.policy,
        input.allowance,
        input.mutation.mutant_set(),
        input.mutation.grid(),
        input.mutation.proof().proof(),
    )
    .map_err(composition)?;
    receipt
        .receipt()
        .validate_inputs(
            receipt_input,
            input.proposal,
            input.policy,
            input.allowance,
            input.mutation.mutant_set(),
            input.mutation.grid(),
            input.mutation.proof().proof(),
        )
        .map_err(composition)?;
    let oracle =
        prepare_admitted_oracle(&receipt, input.revalidation.clone()).map_err(composition)?;
    Ok(PreparedHistoricalReductionAdmission {
        admitted_domain,
        receipt,
        oracle,
    })
}

fn trial_identity(
    trial: &crate::HistoricalReductionVariantTrialV1,
) -> Result<ContentId<AdmissionVariantTrialArtifact>, HistoricalReductionControlError> {
    let bytes = cairn_codec::to_vec(trial).map_err(composition)?;
    reidentify(&bytes)
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
