//! Immutable admitted-oracle receipts and complete identity manifests.

use cairn_protocol::{ContentId, ContentType, TaskId};
use serde::{Deserialize, Serialize};

use super::{
    AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyArtifact, AdmissionPolicyV1,
    AllowanceClaimClass, GenericMutantSetArtifact, MutationGridArtifact, MutationGridCellV1,
    MutationGridProofArtifact, MutationGridProofV1, NumericalAllowanceArtifact,
    NumericalAllowanceV1, OracleProposalArtifact, OracleProposalV1, OracleStrength,
    PreparedGenericMutantSet, PreparedMutationGrid, VerificationContractError,
    VerificationSchemaV1, validate_content_id_order,
};
use crate::{DeclaredDomainArtifact, DeclaredDomainV1, DomainRefinementArtifact};

macro_rules! admission_artifact {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        pub enum $name {}

        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

admission_artifact!(
    /// Frozen domain accepted by one admission attempt.
    AdmittedDomainArtifact,
    "verification.admitted-domain.v1"
);
admission_artifact!(
    /// One explicit exclusion translated into the admitted-domain graph.
    AdmittedDomainExclusionArtifact,
    "verification.admitted-domain-exclusion.v1"
);
admission_artifact!(
    /// Product-specific control whose inputs were fully recomputed before admission.
    AdmissionControlArtifact,
    "verification.admission-control.v1"
);
admission_artifact!(
    /// One execution environment exercised by admission.
    AdmissionEnvironmentArtifact,
    "verification.admission-environment.v1"
);
admission_artifact!(
    /// One source/reference admission observation.
    SourceAdmissionObservationArtifact,
    "verification.source-admission-observation.v1"
);
admission_artifact!(
    /// One correct or deliberately wrong variant trial.
    AdmissionVariantTrialArtifact,
    "verification.admission-variant-trial.v1"
);
admission_artifact!(
    /// Evidence for one mutation-search saturation round.
    AdmissionSaturationEvidenceArtifact,
    "verification.admission-saturation-evidence.v1"
);
admission_artifact!(
    /// Domain or historical-failure coverage evidence.
    AdmissionCoverageArtifact,
    "verification.admission-coverage.v1"
);
admission_artifact!(
    /// One observed source/reference/case disagreement.
    AdmissionDisagreementArtifact,
    "verification.admission-disagreement.v1"
);
admission_artifact!(
    /// Trusted adjudication of one or more disagreements.
    AdmissionAdjudicationArtifact,
    "verification.admission-adjudication.v1"
);
admission_artifact!(
    /// One failed admission proof obligation.
    AdmissionProofFailureArtifact,
    "verification.admission-proof-failure.v1"
);
admission_artifact!(
    /// Complete calibration and admission receipt.
    AdmissionReceiptArtifact,
    "verification.admission-receipt.v1"
);
admission_artifact!(
    /// Immutable oracle admitted by one complete receipt.
    AdmittedOracleArtifact,
    "verification.admitted-oracle.v1"
);

/// Exact admitted-domain graph, including an explicit exclusion set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "AdmittedDomainWire")]
pub struct AdmittedDomainV1 {
    schema_version: VerificationSchemaV1,
    proposal: ContentId<OracleProposalArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    refinements: Vec<ContentId<DomainRefinementArtifact>>,
    exclusions: Vec<ContentId<AdmittedDomainExclusionArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmittedDomainWire {
    schema_version: VerificationSchemaV1,
    proposal: ContentId<OracleProposalArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    refinements: Vec<ContentId<DomainRefinementArtifact>>,
    exclusions: Vec<ContentId<AdmittedDomainExclusionArtifact>>,
}

impl TryFrom<AdmittedDomainWire> for AdmittedDomainV1 {
    type Error = VerificationContractError;

    fn try_from(wire: AdmittedDomainWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        validate_content_id_order(&wire.refinements, "admitted domain refinements")?;
        validate_content_id_order(&wire.exclusions, "admitted domain exclusions")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            proposal: wire.proposal,
            declared_domain: wire.declared_domain,
            refinements: wire.refinements,
            exclusions: wire.exclusions,
        })
    }
}

impl AdmittedDomainV1 {
    /// Returns the proposal whose domain was frozen.
    #[must_use]
    pub const fn proposal(&self) -> ContentId<OracleProposalArtifact> {
        self.proposal
    }

    /// Returns the exact caller declaration retained by admission.
    #[must_use]
    pub const fn declared_domain(&self) -> ContentId<DeclaredDomainArtifact> {
        self.declared_domain
    }

    /// Returns the exact refinements admitted over the caller declaration.
    #[must_use]
    pub fn refinements(&self) -> &[ContentId<DomainRefinementArtifact>] {
        &self.refinements
    }

    /// Returns every explicit exclusion in canonical identity order.
    #[must_use]
    pub fn exclusions(&self) -> &[ContentId<AdmittedDomainExclusionArtifact>] {
        &self.exclusions
    }
}

/// Canonical admitted domain ready for receipt composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAdmittedDomain {
    domain: AdmittedDomainV1,
    domain_bytes: Vec<u8>,
    domain_id: ContentId<AdmittedDomainArtifact>,
}

impl PreparedAdmittedDomain {
    #[must_use]
    pub const fn domain(&self) -> &AdmittedDomainV1 {
        &self.domain
    }

    #[must_use]
    pub fn domain_bytes(&self) -> &[u8] {
        &self.domain_bytes
    }

    #[must_use]
    pub const fn domain_id(&self) -> ContentId<AdmittedDomainArtifact> {
        self.domain_id
    }
}

/// Freezes the exact proposal/domain/exclusion graph.
///
/// # Errors
///
/// Rejects a proposal/declaration mismatch or non-canonical exclusions.
pub fn prepare_admitted_domain(
    proposal: &OracleProposalV1,
    declared_domain: &DeclaredDomainV1,
    exclusions: Vec<ContentId<AdmittedDomainExclusionArtifact>>,
) -> Result<PreparedAdmittedDomain, VerificationContractError> {
    let proposal_bytes = cairn_codec::to_vec(proposal).map_err(codec)?;
    let declared_bytes = cairn_codec::to_vec(declared_domain).map_err(codec)?;
    let proposal_id =
        ContentId::<OracleProposalArtifact>::derive(&proposal_bytes).map_err(codec)?;
    let declared_id =
        ContentId::<DeclaredDomainArtifact>::derive(&declared_bytes).map_err(codec)?;
    if proposal.declared_domain() != declared_id || proposal.task_id() != declared_domain.task_id()
    {
        return invalid(
            "admitted domain",
            "proposal and caller declaration do not match",
        );
    }
    let domain = AdmittedDomainV1::try_from(AdmittedDomainWire {
        schema_version: VerificationSchemaV1,
        proposal: proposal_id,
        declared_domain: declared_id,
        refinements: proposal.domain_refinements().to_vec(),
        exclusions,
    })?;
    let domain_bytes = cairn_codec::to_vec(&domain).map_err(codec)?;
    let domain_id = ContentId::<AdmittedDomainArtifact>::derive(&domain_bytes).map_err(codec)?;
    Ok(PreparedAdmittedDomain {
        domain,
        domain_bytes,
        domain_id,
    })
}

/// One completed mutation-search round used to establish configured saturation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionSaturationRoundV1 {
    round: u32,
    evidence: ContentId<AdmissionSaturationEvidenceArtifact>,
    newly_discovered_fault_classes: u32,
}

impl AdmissionSaturationRoundV1 {
    /// Records one non-zero, externally evidenced saturation round.
    ///
    /// A zero discovery count is required for the terminal consecutive rounds used by admission.
    ///
    /// # Errors
    ///
    /// Rejects round zero.
    pub fn new(
        round: u32,
        evidence: ContentId<AdmissionSaturationEvidenceArtifact>,
        newly_discovered_fault_classes: u32,
    ) -> Result<Self, VerificationContractError> {
        if round == 0 {
            return Err(VerificationContractError::NonPositive {
                field: "admission saturation round",
            });
        }
        Ok(Self {
            round,
            evidence,
            newly_discovered_fault_classes,
        })
    }
}

/// Explicit handling of source/reference/case disagreement evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "disposition", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AdmissionDisagreementDispositionV1 {
    /// No disagreement was observed in the frozen admission corpus.
    NoneObserved,
    /// Every observed disagreement has separately identified adjudication evidence.
    Adjudicated {
        disagreements: Vec<ContentId<AdmissionDisagreementArtifact>>,
        adjudications: Vec<ContentId<AdmissionAdjudicationArtifact>>,
    },
}

/// Typed assumptions carried into every future candidate verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionAssumptionV1 {
    /// Correct-variant construction evidence is independent of the oracle under test.
    ConstructionEvidenceIndependent,
    /// The recorded host environment remains representative only for the admitted host scope.
    HostEnvironmentStable,
}

/// Claims deliberately not established by this admission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionUnverifiedClaimV1 {
    SourceAcceleratorBehavior,
    TargetBuildBehavior,
    TargetDeviceBehavior,
    TargetSpecificFailureCoverage,
    DeviceRunnerIndependentAttestation,
}

/// Event that invalidates continued use of an admitted oracle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionRevalidationTriggerV1 {
    ProposalChanged,
    PolicyChanged,
    DomainChanged,
    CorpusChanged,
    AllowanceChanged,
    ObservationPathChanged,
    ExecutionEnvironmentChanged,
}

/// Immutable expiration/revalidation behavior.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "AdmissionRevalidationPolicyWire")]
pub struct AdmissionRevalidationPolicyV1 {
    schema_version: VerificationSchemaV1,
    expires_at_unix_millis: Option<u64>,
    triggers: Vec<AdmissionRevalidationTriggerV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionRevalidationPolicyWire {
    schema_version: VerificationSchemaV1,
    expires_at_unix_millis: Option<u64>,
    triggers: Vec<AdmissionRevalidationTriggerV1>,
}

impl AdmissionRevalidationPolicyV1 {
    /// Creates an explicit revalidation policy.
    ///
    /// # Errors
    ///
    /// Rejects timestamp zero or an empty/non-canonical trigger set.
    pub fn new(
        expires_at_unix_millis: Option<u64>,
        triggers: Vec<AdmissionRevalidationTriggerV1>,
    ) -> Result<Self, VerificationContractError> {
        if expires_at_unix_millis == Some(0) {
            return Err(VerificationContractError::NonPositive {
                field: "oracle expiration timestamp",
            });
        }
        validate_ordered_nonempty(&triggers, "revalidation triggers")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            expires_at_unix_millis,
            triggers,
        })
    }
}

impl TryFrom<AdmissionRevalidationPolicyWire> for AdmissionRevalidationPolicyV1 {
    type Error = VerificationContractError;

    fn try_from(wire: AdmissionRevalidationPolicyWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.expires_at_unix_millis, wire.triggers)
    }
}

/// Final lifecycle decision recorded by an admission receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionDecisionV1 {
    Rejected,
    Unverifiable,
    Admitted,
}

/// Product evidence needed to compose one admitted receipt after product-specific recomputation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedReceiptInput {
    pub admitted_domain: ContentId<AdmittedDomainArtifact>,
    pub admission_corpus: ContentId<AdmissionCorpusArtifact>,
    pub admission_control: ContentId<AdmissionControlArtifact>,
    pub environments: Vec<ContentId<AdmissionEnvironmentArtifact>>,
    pub source_observations: Vec<ContentId<SourceAdmissionObservationArtifact>>,
    pub execution_scopes: Vec<AdmissionExecutionScope>,
    pub correct_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    pub wrong_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    pub saturation_rounds: Vec<AdmissionSaturationRoundV1>,
    pub coverage: Vec<ContentId<AdmissionCoverageArtifact>>,
    pub disagreement: AdmissionDisagreementDispositionV1,
    pub assumptions: Vec<AdmissionAssumptionV1>,
    pub unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    pub admitted_strength: OracleStrength,
}

/// Complete immutable admission receipt. Its decision is checked from supplied proof inputs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "AdmissionReceiptWire")]
pub struct AdmissionReceiptV1 {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    admitted_domain: ContentId<AdmittedDomainArtifact>,
    admission_corpus: ContentId<AdmissionCorpusArtifact>,
    admission_control: ContentId<AdmissionControlArtifact>,
    environments: Vec<ContentId<AdmissionEnvironmentArtifact>>,
    source_observations: Vec<ContentId<SourceAdmissionObservationArtifact>>,
    requested_strength: OracleStrength,
    admitted_strength: OracleStrength,
    execution_scopes: Vec<AdmissionExecutionScope>,
    correct_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    wrong_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    mutant_set: ContentId<GenericMutantSetArtifact>,
    mutation_grid: ContentId<MutationGridArtifact>,
    mutation_proof: ContentId<MutationGridProofArtifact>,
    saturation_rounds: Vec<AdmissionSaturationRoundV1>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    coverage: Vec<ContentId<AdmissionCoverageArtifact>>,
    blind_spots: Vec<MutationGridCellV1>,
    non_injectable: Vec<MutationGridCellV1>,
    disagreement: AdmissionDisagreementDispositionV1,
    assumptions: Vec<AdmissionAssumptionV1>,
    unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    decision: AdmissionDecisionV1,
    failed_proof_obligations: Vec<ContentId<AdmissionProofFailureArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionReceiptWire {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    admitted_domain: ContentId<AdmittedDomainArtifact>,
    admission_corpus: ContentId<AdmissionCorpusArtifact>,
    admission_control: ContentId<AdmissionControlArtifact>,
    environments: Vec<ContentId<AdmissionEnvironmentArtifact>>,
    source_observations: Vec<ContentId<SourceAdmissionObservationArtifact>>,
    requested_strength: OracleStrength,
    admitted_strength: OracleStrength,
    execution_scopes: Vec<AdmissionExecutionScope>,
    correct_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    wrong_variant_trials: Vec<ContentId<AdmissionVariantTrialArtifact>>,
    mutant_set: ContentId<GenericMutantSetArtifact>,
    mutation_grid: ContentId<MutationGridArtifact>,
    mutation_proof: ContentId<MutationGridProofArtifact>,
    saturation_rounds: Vec<AdmissionSaturationRoundV1>,
    allowance: ContentId<NumericalAllowanceArtifact>,
    coverage: Vec<ContentId<AdmissionCoverageArtifact>>,
    blind_spots: Vec<MutationGridCellV1>,
    non_injectable: Vec<MutationGridCellV1>,
    disagreement: AdmissionDisagreementDispositionV1,
    assumptions: Vec<AdmissionAssumptionV1>,
    unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    decision: AdmissionDecisionV1,
    failed_proof_obligations: Vec<ContentId<AdmissionProofFailureArtifact>>,
}

impl TryFrom<AdmissionReceiptWire> for AdmissionReceiptV1 {
    type Error = VerificationContractError;

    fn try_from(wire: AdmissionReceiptWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        validate_receipt_shape(&wire)?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            task_id: wire.task_id,
            proposal: wire.proposal,
            policy: wire.policy,
            admitted_domain: wire.admitted_domain,
            admission_corpus: wire.admission_corpus,
            admission_control: wire.admission_control,
            environments: wire.environments,
            source_observations: wire.source_observations,
            requested_strength: wire.requested_strength,
            admitted_strength: wire.admitted_strength,
            execution_scopes: wire.execution_scopes,
            correct_variant_trials: wire.correct_variant_trials,
            wrong_variant_trials: wire.wrong_variant_trials,
            mutant_set: wire.mutant_set,
            mutation_grid: wire.mutation_grid,
            mutation_proof: wire.mutation_proof,
            saturation_rounds: wire.saturation_rounds,
            allowance: wire.allowance,
            coverage: wire.coverage,
            blind_spots: wire.blind_spots,
            non_injectable: wire.non_injectable,
            disagreement: wire.disagreement,
            assumptions: wire.assumptions,
            unverified_claims: wire.unverified_claims,
            decision: wire.decision,
            failed_proof_obligations: wire.failed_proof_obligations,
        })
    }
}

fn validate_receipt_shape(wire: &AdmissionReceiptWire) -> Result<(), VerificationContractError> {
    validate_content_id_order(&wire.environments, "admission environments")?;
    validate_content_id_order(&wire.source_observations, "source admission observations")?;
    validate_content_id_order(&wire.correct_variant_trials, "correct variant trials")?;
    validate_content_id_order(&wire.wrong_variant_trials, "wrong variant trials")?;
    validate_content_id_order(&wire.coverage, "admission coverage")?;
    validate_content_id_order(
        &wire.failed_proof_obligations,
        "failed admission proof obligations",
    )?;
    validate_ordered(&wire.execution_scopes, "admission execution scopes")?;
    validate_ordered(&wire.assumptions, "admission assumptions")?;
    validate_ordered(&wire.unverified_claims, "unverified admission claims")?;
    if wire
        .correct_variant_trials
        .iter()
        .any(|correct| wire.wrong_variant_trials.contains(correct))
    {
        return invalid("admission receipt", "variant trial roles overlap");
    }
    if wire
        .saturation_rounds
        .windows(2)
        .any(|rounds| rounds[0].round >= rounds[1].round)
    {
        return invalid("admission receipt", "saturation rounds are non-canonical");
    }
    match &wire.disagreement {
        AdmissionDisagreementDispositionV1::NoneObserved => {}
        AdmissionDisagreementDispositionV1::Adjudicated {
            disagreements,
            adjudications,
        } => {
            validate_content_id_order(disagreements, "admission disagreements")?;
            validate_content_id_order(adjudications, "admission adjudications")?;
            if disagreements.is_empty() || adjudications.is_empty() {
                return invalid(
                    "admission receipt",
                    "adjudication requires both evidence sets",
                );
            }
        }
    }
    match wire.decision {
        AdmissionDecisionV1::Admitted
            if wire.environments.is_empty()
                || wire.source_observations.is_empty()
                || wire.correct_variant_trials.is_empty()
                || wire.wrong_variant_trials.is_empty()
                || wire.coverage.is_empty()
                || wire.execution_scopes.is_empty()
                || !wire.failed_proof_obligations.is_empty()
                || wire.requested_strength != wire.admitted_strength =>
        {
            return invalid("admission receipt", "admitted receipt shape is incomplete");
        }
        AdmissionDecisionV1::Rejected | AdmissionDecisionV1::Unverifiable
            if wire.failed_proof_obligations.is_empty()
                || wire.admitted_strength != OracleStrength::Unavailable =>
        {
            return invalid(
                "admission receipt",
                "non-admitted receipt requires failures and unavailable strength",
            );
        }
        AdmissionDecisionV1::Admitted
        | AdmissionDecisionV1::Rejected
        | AdmissionDecisionV1::Unverifiable => {}
    }
    Ok(())
}

/// Canonical admission receipt ready for final-oracle composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAdmissionReceipt {
    receipt: AdmissionReceiptV1,
    receipt_bytes: Vec<u8>,
    receipt_id: ContentId<AdmissionReceiptArtifact>,
}

impl PreparedAdmissionReceipt {
    #[must_use]
    pub const fn receipt(&self) -> &AdmissionReceiptV1 {
        &self.receipt
    }

    #[must_use]
    pub fn receipt_bytes(&self) -> &[u8] {
        &self.receipt_bytes
    }

    #[must_use]
    pub const fn receipt_id(&self) -> ContentId<AdmissionReceiptArtifact> {
        self.receipt_id
    }
}

/// Recomputes domain-neutral proof obligations and emits an admitted receipt.
///
/// Product-specific callers must validate their control before invoking this boundary.
///
/// # Errors
///
/// Rejects any incomplete identity set, policy mismatch, failed mutation proof, insufficient
/// family/scope/saturation evidence, or allowance that cannot support admission.
pub fn prepare_admission_receipt(
    input: AdmittedReceiptInput,
    proposal: &OracleProposalV1,
    policy: &AdmissionPolicyV1,
    allowance: &NumericalAllowanceV1,
    mutant_set: &PreparedGenericMutantSet,
    mutation_grid: &PreparedMutationGrid,
    mutation_proof: &MutationGridProofV1,
) -> Result<PreparedAdmissionReceipt, VerificationContractError> {
    mutation_proof
        .validate_against(policy, mutant_set, mutation_grid)
        .map_err(codec)?;
    if !mutation_proof.obligations_satisfied()
        || mutation_grid.grid().corpus() != input.admission_corpus
        || policy.mutant_set() != mutant_set.mutant_set_id()
        || !policy
            .accepted_strengths()
            .contains(&input.admitted_strength)
        || input.admitted_strength != proposal.requested_strength()
        || allowance.maximum_claim_class() == AllowanceClaimClass::InsufficientEvidence
        || !allowance
            .derivation_corpora()
            .contains(&input.admission_corpus)
        || policy
            .required_execution_scopes()
            .iter()
            .any(|scope| !input.execution_scopes.contains(scope))
        || input.correct_variant_trials.len()
            < usize::try_from(policy.minimum_correct_variants().get()).map_err(codec)?
        || input.wrong_variant_trials.len()
            < usize::try_from(policy.minimum_incorrect_variants().get()).map_err(codec)?
        || input.saturation_rounds.len()
            < usize::try_from(policy.saturation_rounds().get()).map_err(codec)?
    {
        return invalid(
            "admission receipt",
            "underlying admission obligations are not satisfied",
        );
    }
    let required_saturation = usize::try_from(policy.saturation_rounds().get()).map_err(codec)?;
    if input
        .saturation_rounds
        .iter()
        .rev()
        .take(required_saturation)
        .any(|round| round.newly_discovered_fault_classes != 0)
    {
        return invalid(
            "admission receipt",
            "terminal mutation rounds did not establish saturation",
        );
    }
    let proposal_bytes = cairn_codec::to_vec(proposal).map_err(codec)?;
    let policy_bytes = cairn_codec::to_vec(policy).map_err(codec)?;
    let allowance_bytes = cairn_codec::to_vec(allowance).map_err(codec)?;
    let wire = AdmissionReceiptWire {
        schema_version: VerificationSchemaV1,
        task_id: proposal.task_id(),
        proposal: ContentId::derive(&proposal_bytes).map_err(codec)?,
        policy: ContentId::derive(&policy_bytes).map_err(codec)?,
        admitted_domain: input.admitted_domain,
        admission_corpus: input.admission_corpus,
        admission_control: input.admission_control,
        environments: input.environments,
        source_observations: input.source_observations,
        requested_strength: proposal.requested_strength(),
        admitted_strength: input.admitted_strength,
        execution_scopes: input.execution_scopes,
        correct_variant_trials: input.correct_variant_trials,
        wrong_variant_trials: input.wrong_variant_trials,
        mutant_set: mutant_set.mutant_set_id(),
        mutation_grid: mutation_grid.grid_id(),
        mutation_proof: ContentId::derive(&cairn_codec::to_vec(mutation_proof).map_err(codec)?)
            .map_err(codec)?,
        saturation_rounds: input.saturation_rounds,
        allowance: ContentId::derive(&allowance_bytes).map_err(codec)?,
        coverage: input.coverage,
        blind_spots: mutation_proof.blind_spots().to_vec(),
        non_injectable: mutation_proof.non_injectable().to_vec(),
        disagreement: input.disagreement,
        assumptions: input.assumptions,
        unverified_claims: input.unverified_claims,
        decision: AdmissionDecisionV1::Admitted,
        failed_proof_obligations: Vec::new(),
    };
    let receipt = AdmissionReceiptV1::try_from(wire)?;
    let receipt_bytes = cairn_codec::to_vec(&receipt).map_err(codec)?;
    let receipt_id =
        ContentId::<AdmissionReceiptArtifact>::derive(&receipt_bytes).map_err(codec)?;
    Ok(PreparedAdmissionReceipt {
        receipt,
        receipt_bytes,
        receipt_id,
    })
}

impl AdmissionReceiptV1 {
    /// Recomputes this persisted receipt from independent proposal, policy, and proof inputs.
    ///
    /// # Errors
    ///
    /// Rejects a changed identity edge or any underlying admission obligation that no longer holds.
    #[expect(
        clippy::too_many_arguments,
        reason = "the receipt is revalidated from independently trusted proposal, policy, allowance, mutant, grid, and proof inputs"
    )]
    pub fn validate_inputs(
        &self,
        input: AdmittedReceiptInput,
        proposal: &OracleProposalV1,
        policy: &AdmissionPolicyV1,
        allowance: &NumericalAllowanceV1,
        mutant_set: &PreparedGenericMutantSet,
        mutation_grid: &PreparedMutationGrid,
        mutation_proof: &MutationGridProofV1,
    ) -> Result<(), VerificationContractError> {
        let recomputed = prepare_admission_receipt(
            input,
            proposal,
            policy,
            allowance,
            mutant_set,
            mutation_grid,
            mutation_proof,
        )?;
        if recomputed.receipt != *self {
            return invalid(
                "admission receipt",
                "persisted receipt differs from trusted recomputation",
            );
        }
        Ok(())
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OracleProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn admitted_domain(&self) -> ContentId<AdmittedDomainArtifact> {
        self.admitted_domain
    }

    #[must_use]
    pub const fn admission_corpus(&self) -> ContentId<AdmissionCorpusArtifact> {
        self.admission_corpus
    }

    #[must_use]
    pub const fn policy(&self) -> ContentId<AdmissionPolicyArtifact> {
        self.policy
    }

    #[must_use]
    pub const fn allowance(&self) -> ContentId<NumericalAllowanceArtifact> {
        self.allowance
    }

    #[must_use]
    pub const fn admitted_strength(&self) -> OracleStrength {
        self.admitted_strength
    }

    #[must_use]
    pub fn blind_spots(&self) -> &[MutationGridCellV1] {
        &self.blind_spots
    }

    #[must_use]
    pub fn assumptions(&self) -> &[AdmissionAssumptionV1] {
        &self.assumptions
    }

    #[must_use]
    pub fn unverified_claims(&self) -> &[AdmissionUnverifiedClaimV1] {
        &self.unverified_claims
    }

    #[must_use]
    pub const fn decision(&self) -> AdmissionDecisionV1 {
        self.decision
    }
}

/// Immutable admitted-oracle manifest. It duplicates receipt-critical edges for fail-closed reads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "AdmittedOracleWire")]
pub struct AdmittedOracleV1 {
    schema_version: VerificationSchemaV1,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    admission_receipt: ContentId<AdmissionReceiptArtifact>,
    admitted_domain: ContentId<AdmittedDomainArtifact>,
    admitted_strength: OracleStrength,
    allowance: ContentId<NumericalAllowanceArtifact>,
    frozen_corpus: ContentId<AdmissionCorpusArtifact>,
    blind_spots: Vec<MutationGridCellV1>,
    assumptions: Vec<AdmissionAssumptionV1>,
    unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    revalidation: AdmissionRevalidationPolicyV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmittedOracleWire {
    schema_version: VerificationSchemaV1,
    proposal: ContentId<OracleProposalArtifact>,
    policy: ContentId<AdmissionPolicyArtifact>,
    admission_receipt: ContentId<AdmissionReceiptArtifact>,
    admitted_domain: ContentId<AdmittedDomainArtifact>,
    admitted_strength: OracleStrength,
    allowance: ContentId<NumericalAllowanceArtifact>,
    frozen_corpus: ContentId<AdmissionCorpusArtifact>,
    blind_spots: Vec<MutationGridCellV1>,
    assumptions: Vec<AdmissionAssumptionV1>,
    unverified_claims: Vec<AdmissionUnverifiedClaimV1>,
    revalidation: AdmissionRevalidationPolicyV1,
}

impl TryFrom<AdmittedOracleWire> for AdmittedOracleV1 {
    type Error = VerificationContractError;

    fn try_from(wire: AdmittedOracleWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        validate_ordered(&wire.assumptions, "oracle assumptions")?;
        validate_ordered(&wire.unverified_claims, "oracle unverified claims")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            proposal: wire.proposal,
            policy: wire.policy,
            admission_receipt: wire.admission_receipt,
            admitted_domain: wire.admitted_domain,
            admitted_strength: wire.admitted_strength,
            allowance: wire.allowance,
            frozen_corpus: wire.frozen_corpus,
            blind_spots: wire.blind_spots,
            assumptions: wire.assumptions,
            unverified_claims: wire.unverified_claims,
            revalidation: wire.revalidation,
        })
    }
}

/// Canonical immutable admitted oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAdmittedOracle {
    oracle: AdmittedOracleV1,
    oracle_bytes: Vec<u8>,
    oracle_id: ContentId<AdmittedOracleArtifact>,
}

impl PreparedAdmittedOracle {
    #[must_use]
    pub const fn oracle(&self) -> &AdmittedOracleV1 {
        &self.oracle
    }

    #[must_use]
    pub fn oracle_bytes(&self) -> &[u8] {
        &self.oracle_bytes
    }

    #[must_use]
    pub const fn oracle_id(&self) -> ContentId<AdmittedOracleArtifact> {
        self.oracle_id
    }
}

/// Emits an immutable oracle from one recomputed admitted receipt.
///
/// # Errors
///
/// Rejects a non-admitted receipt or invalid revalidation policy.
pub fn prepare_admitted_oracle(
    receipt: &PreparedAdmissionReceipt,
    revalidation: AdmissionRevalidationPolicyV1,
) -> Result<PreparedAdmittedOracle, VerificationContractError> {
    if receipt.receipt.decision() != AdmissionDecisionV1::Admitted {
        return invalid("admitted oracle", "receipt did not admit the oracle");
    }
    let oracle = AdmittedOracleV1::try_from(AdmittedOracleWire {
        schema_version: VerificationSchemaV1,
        proposal: receipt.receipt.proposal,
        policy: receipt.receipt.policy,
        admission_receipt: receipt.receipt_id,
        admitted_domain: receipt.receipt.admitted_domain,
        admitted_strength: receipt.receipt.admitted_strength,
        allowance: receipt.receipt.allowance,
        frozen_corpus: receipt.receipt.admission_corpus,
        blind_spots: receipt.receipt.blind_spots.clone(),
        assumptions: receipt.receipt.assumptions.clone(),
        unverified_claims: receipt.receipt.unverified_claims.clone(),
        revalidation,
    })?;
    let oracle_bytes = cairn_codec::to_vec(&oracle).map_err(codec)?;
    let oracle_id = ContentId::<AdmittedOracleArtifact>::derive(&oracle_bytes).map_err(codec)?;
    Ok(PreparedAdmittedOracle {
        oracle,
        oracle_bytes,
        oracle_id,
    })
}

impl AdmittedOracleV1 {
    /// Revalidates every receipt-mirrored edge and the receipt identity.
    ///
    /// # Errors
    ///
    /// Rejects a changed receipt or any manifest edge that no longer agrees with it.
    pub fn validate_receipt(
        &self,
        receipt: &PreparedAdmissionReceipt,
    ) -> Result<(), VerificationContractError> {
        let recomputed = prepare_admitted_oracle(receipt, self.revalidation.clone())?;
        if recomputed.oracle != *self {
            return invalid(
                "admitted oracle",
                "manifest differs from its admission receipt",
            );
        }
        Ok(())
    }
}

fn validate_ordered_nonempty<T: Ord>(
    values: &[T],
    field: &'static str,
) -> Result<(), VerificationContractError> {
    if values.is_empty() {
        return Err(VerificationContractError::EmptySet { field });
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(VerificationContractError::NonCanonicalSet { field });
    }
    Ok(())
}

fn validate_ordered<T: Ord>(
    values: &[T],
    field: &'static str,
) -> Result<(), VerificationContractError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(VerificationContractError::NonCanonicalSet { field });
    }
    Ok(())
}

fn codec(error: impl std::fmt::Display) -> VerificationContractError {
    let _ = error;
    VerificationContractError::InvalidArtifactCombination {
        artifact: "admission identity graph",
        reason: "canonical encoding or cited proof validation failed",
    }
}

fn invalid<T>(
    artifact: &'static str,
    reason: &'static str,
) -> Result<T, VerificationContractError> {
    Err(VerificationContractError::InvalidArtifactCombination { artifact, reason })
}
