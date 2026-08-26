//! Candidate verdict receipts derived from one frozen admitted oracle.

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Serialize};

use crate::{
    AdmissionAssumptionV1, AdmissionCorpusArtifact, AdmissionEnvironmentArtifact,
    AdmissionExecutionScope, AdmissionReceiptArtifact, AdmissionUnverifiedClaimV1,
    AdmittedOracleArtifact, MutationGridCellV1, NumericalAllowanceArtifact,
    PreparedAdmissionReceipt, PreparedAdmittedOracle, VerificationContractError,
    VerificationSchemaV1,
};

macro_rules! candidate_artifact {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        pub enum $name {}

        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

candidate_artifact!(
    /// Exact candidate identity judged by an admitted oracle.
    CandidateArtifact,
    "verification.candidate.v1"
);
candidate_artifact!(
    /// Candidate source or implementation bundle evidence.
    CandidateSourceArtifact,
    "verification.candidate-source.v1"
);
candidate_artifact!(
    /// Authoritative candidate build evidence.
    CandidateBuildArtifact,
    "verification.candidate-build.v1"
);
candidate_artifact!(
    /// Authoritative candidate execution evidence.
    CandidateRunArtifact,
    "verification.candidate-run.v1"
);
candidate_artifact!(
    /// Trusted comparison of candidate observations against the frozen oracle.
    CandidateComparisonArtifact,
    "verification.candidate-comparison.v1"
);
candidate_artifact!(
    /// One exact failed frozen-corpus case.
    CandidateFailedCaseArtifact,
    "verification.candidate-failed-case.v1"
);
candidate_artifact!(
    /// Final candidate verdict receipt.
    CandidateVerdictArtifact,
    "verification.candidate-verdict.v1"
);

/// Candidate outcome recomputed from the complete failed-case set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateVerdictOutcomeV1 {
    Pass,
    Fail,
}

/// Independently validated product evidence for one candidate judgment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateVerdictInput {
    pub candidate: ContentId<CandidateArtifact>,
    pub source: ContentId<CandidateSourceArtifact>,
    pub build: ContentId<CandidateBuildArtifact>,
    pub run: ContentId<CandidateRunArtifact>,
    pub environment: ContentId<AdmissionEnvironmentArtifact>,
    pub corpus: ContentId<AdmissionCorpusArtifact>,
    pub comparison: ContentId<CandidateComparisonArtifact>,
    pub failed_cases: Vec<ContentId<CandidateFailedCaseArtifact>>,
}

/// Immutable terminal candidate verdict with no caller-supplied pass bit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CandidateVerdictWire")]
pub struct CandidateVerdictV1 {
    schema_version: VerificationSchemaV1,
    admitted_oracle: ContentId<AdmittedOracleArtifact>,
    admission_receipt: ContentId<AdmissionReceiptArtifact>,
    candidate: ContentId<CandidateArtifact>,
    source: ContentId<CandidateSourceArtifact>,
    build: ContentId<CandidateBuildArtifact>,
    run: ContentId<CandidateRunArtifact>,
    environment: ContentId<AdmissionEnvironmentArtifact>,
    frozen_corpus: ContentId<AdmissionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    comparison: ContentId<CandidateComparisonArtifact>,
    failed_cases: Vec<ContentId<CandidateFailedCaseArtifact>>,
    oracle_blind_spots: Vec<MutationGridCellV1>,
    oracle_assumptions: Vec<AdmissionAssumptionV1>,
    oracle_unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    outcome: CandidateVerdictOutcomeV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateVerdictWire {
    schema_version: VerificationSchemaV1,
    admitted_oracle: ContentId<AdmittedOracleArtifact>,
    admission_receipt: ContentId<AdmissionReceiptArtifact>,
    candidate: ContentId<CandidateArtifact>,
    source: ContentId<CandidateSourceArtifact>,
    build: ContentId<CandidateBuildArtifact>,
    run: ContentId<CandidateRunArtifact>,
    environment: ContentId<AdmissionEnvironmentArtifact>,
    frozen_corpus: ContentId<AdmissionCorpusArtifact>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    comparison: ContentId<CandidateComparisonArtifact>,
    failed_cases: Vec<ContentId<CandidateFailedCaseArtifact>>,
    oracle_blind_spots: Vec<MutationGridCellV1>,
    oracle_assumptions: Vec<AdmissionAssumptionV1>,
    oracle_unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    outcome: CandidateVerdictOutcomeV1,
}

impl TryFrom<CandidateVerdictWire> for CandidateVerdictV1 {
    type Error = VerificationContractError;

    fn try_from(wire: CandidateVerdictWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        if wire
            .failed_cases
            .windows(2)
            .any(|cases| cases[0].to_wire() >= cases[1].to_wire())
            || wire
                .oracle_blind_spots
                .windows(2)
                .any(|cells| mutation_cell_key(cells[0]) >= mutation_cell_key(cells[1]))
            || wire
                .oracle_assumptions
                .windows(2)
                .any(|values| values[0] >= values[1])
            || wire
                .oracle_unverified_claims
                .windows(2)
                .any(|values| values[0] >= values[1])
            || wire.outcome
                != if wire.failed_cases.is_empty() {
                    CandidateVerdictOutcomeV1::Pass
                } else {
                    CandidateVerdictOutcomeV1::Fail
                }
        {
            return invalid(
                "candidate verdict",
                "failed cases, limitations, or derived outcome are inconsistent",
            );
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            admitted_oracle: wire.admitted_oracle,
            admission_receipt: wire.admission_receipt,
            candidate: wire.candidate,
            source: wire.source,
            build: wire.build,
            run: wire.run,
            environment: wire.environment,
            frozen_corpus: wire.frozen_corpus,
            allowance: wire.allowance,
            comparison: wire.comparison,
            failed_cases: wire.failed_cases,
            oracle_blind_spots: wire.oracle_blind_spots,
            oracle_assumptions: wire.oracle_assumptions,
            oracle_unverified_claims: wire.oracle_unverified_claims,
            outcome: wire.outcome,
        })
    }
}

impl CandidateVerdictV1 {
    #[must_use]
    pub const fn candidate(&self) -> ContentId<CandidateArtifact> {
        self.candidate
    }

    #[must_use]
    pub const fn outcome(&self) -> CandidateVerdictOutcomeV1 {
        self.outcome
    }

    #[must_use]
    pub fn failed_cases(&self) -> &[ContentId<CandidateFailedCaseArtifact>] {
        &self.failed_cases
    }

    #[must_use]
    pub fn oracle_blind_spots(&self) -> &[MutationGridCellV1] {
        &self.oracle_blind_spots
    }

    #[must_use]
    pub fn oracle_unverified_claims(&self) -> &[AdmissionUnverifiedClaimV1] {
        &self.oracle_unverified_claims
    }

    /// Recomputes this persisted verdict from trusted candidate evidence and admitted inputs.
    ///
    /// # Errors
    ///
    /// Rejects any changed candidate/oracle edge or caller-supplied outcome metadata.
    pub fn validate_inputs(
        &self,
        input: CandidateVerdictInput,
        oracle: &PreparedAdmittedOracle,
        admission: &PreparedAdmissionReceipt,
    ) -> Result<(), VerificationContractError> {
        let recomputed = prepare_candidate_verdict(input, oracle, admission)?;
        if recomputed.verdict != *self {
            return invalid(
                "candidate verdict",
                "persisted verdict differs from trusted recomputation",
            );
        }
        Ok(())
    }
}

/// Canonical candidate verdict ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCandidateVerdict {
    verdict: CandidateVerdictV1,
    verdict_bytes: Vec<u8>,
    verdict_id: ContentId<CandidateVerdictArtifact>,
}

impl PreparedCandidateVerdict {
    #[must_use]
    pub const fn verdict(&self) -> &CandidateVerdictV1 {
        &self.verdict
    }

    #[must_use]
    pub fn verdict_bytes(&self) -> &[u8] {
        &self.verdict_bytes
    }

    #[must_use]
    pub const fn verdict_id(&self) -> ContentId<CandidateVerdictArtifact> {
        self.verdict_id
    }
}

/// Emits a candidate verdict only within the exact frozen oracle scope.
///
/// Product-specific code must first recompute comparison and failed-case evidence.
///
/// # Errors
///
/// Rejects a changed oracle/receipt graph, corpus or environment outside admission, missing
/// implementation/observation scopes, or non-canonical failed cases.
pub fn prepare_candidate_verdict(
    mut input: CandidateVerdictInput,
    oracle: &PreparedAdmittedOracle,
    admission: &PreparedAdmissionReceipt,
) -> Result<PreparedCandidateVerdict, VerificationContractError> {
    oracle.oracle().validate_receipt(admission)?;
    input.failed_cases.sort_by_key(ContentId::to_wire);
    if input
        .failed_cases
        .windows(2)
        .any(|cases| cases[0] == cases[1])
        || input.corpus != oracle.oracle().frozen_corpus()
        || !admission
            .receipt()
            .environments()
            .contains(&input.environment)
        || !admission
            .receipt()
            .execution_scopes()
            .contains(&AdmissionExecutionScope::Implementation)
        || !admission
            .receipt()
            .execution_scopes()
            .contains(&AdmissionExecutionScope::ObservationPipeline)
    {
        return invalid(
            "candidate verdict",
            "candidate evidence is outside the admitted oracle scope",
        );
    }
    let outcome = if input.failed_cases.is_empty() {
        CandidateVerdictOutcomeV1::Pass
    } else {
        CandidateVerdictOutcomeV1::Fail
    };
    let verdict = CandidateVerdictV1::try_from(CandidateVerdictWire {
        schema_version: VerificationSchemaV1,
        admitted_oracle: oracle.oracle_id(),
        admission_receipt: admission.receipt_id(),
        candidate: input.candidate,
        source: input.source,
        build: input.build,
        run: input.run,
        environment: input.environment,
        frozen_corpus: input.corpus,
        allowance: oracle.oracle().allowance(),
        comparison: input.comparison,
        failed_cases: input.failed_cases,
        oracle_blind_spots: oracle.oracle().blind_spots().to_vec(),
        oracle_assumptions: oracle.oracle().assumptions().to_vec(),
        oracle_unverified_claims: oracle.oracle().unverified_claims().to_vec(),
        outcome,
    })?;
    let verdict_bytes = cairn_codec::to_vec(&verdict).map_err(codec)?;
    let verdict_id =
        ContentId::<CandidateVerdictArtifact>::derive(&verdict_bytes).map_err(codec)?;
    Ok(PreparedCandidateVerdict {
        verdict,
        verdict_bytes,
        verdict_id,
    })
}

fn codec(error: impl std::fmt::Display) -> VerificationContractError {
    let _ = error;
    VerificationContractError::InvalidArtifactCombination {
        artifact: "candidate verdict",
        reason: "canonical encoding or identity derivation failed",
    }
}

fn mutation_cell_key(cell: MutationGridCellV1) -> (String, String) {
    (cell.mutant().to_wire(), cell.case().to_wire())
}

fn invalid<T>(
    artifact: &'static str,
    reason: &'static str,
) -> Result<T, VerificationContractError> {
    Err(VerificationContractError::InvalidArtifactCombination { artifact, reason })
}
