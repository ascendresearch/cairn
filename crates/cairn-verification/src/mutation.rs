//! Trusted generic-mutant sets, complete mutation grids, and recomputed proof obligations.

use std::collections::{BTreeMap, BTreeSet};

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyArtifact, AdmissionPolicyV1,
    FaultClassName, ImplementationBundleArtifact, VerificationSchemaV1,
};

macro_rules! mutation_artifact {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        pub enum $name {}

        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

mutation_artifact!(
    /// Reviewed implementation of one trusted generic mutant.
    TrustedMutantDefinitionArtifact,
    "verification.trusted-mutant-definition.v1"
);
mutation_artifact!(
    /// Canonical set of trusted generic-mutant definitions selected by policy.
    GenericMutantSetArtifact,
    "verification.generic-mutant-set.v1"
);
mutation_artifact!(
    /// Domain-adapter identity for one case entering a generic mutation grid.
    MutationCaseArtifact,
    "verification.mutation-case.v1"
);
mutation_artifact!(
    /// Evidence that one mutant was injected for one case.
    MutationInjectionArtifact,
    "verification.mutation-injection.v1"
);
mutation_artifact!(
    /// Authoritative build/execute/observe evidence for one mutation trial.
    MutationExecutionArtifact,
    "verification.mutation-execution.v1"
);
mutation_artifact!(
    /// Trusted comparison facts from one mutation trial.
    MutationComparisonArtifact,
    "verification.mutation-comparison.v1"
);
mutation_artifact!(
    /// Exact reason one mutant cannot be injected for one case.
    NonInjectableReasonArtifact,
    "verification.mutation-non-injectable-reason.v1"
);
mutation_artifact!(
    /// Complete mutation-grid trial facts.
    MutationGridArtifact,
    "verification.mutation-grid.v1"
);
mutation_artifact!(
    /// Proof failures and blind spots recomputed from a complete mutation grid.
    MutationGridProofArtifact,
    "verification.mutation-grid-proof.v1"
);

/// Failure to construct or validate trusted mutation-grid evidence.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MutationGridError {
    /// A trusted mutant set had no definitions.
    #[error("trusted generic-mutant set must not be empty")]
    EmptyMutantSet,
    /// Mutant definitions were duplicated or not in canonical identity order.
    #[error("trusted generic-mutant definitions must be in strict canonical order")]
    NonCanonicalMutants,
    /// A mutation grid had no case identities.
    #[error("mutation grid cases must not be empty")]
    EmptyCases,
    /// Case identities were duplicated or not in canonical identity order.
    #[error("mutation grid cases must be in strict canonical order")]
    NonCanonicalCases,
    /// Trial cells were incomplete, duplicated, non-canonical, or internally contradictory.
    #[error("mutation grid trials do not form the exact canonical mutant/case product")]
    InconsistentGrid,
    /// The grid and policy selected different trusted mutant sets.
    #[error("admission policy does not select the supplied trusted mutant set")]
    PolicyMutantSetMismatch,
    /// Canonical bytes or a supplied typed identity did not agree with the artifact.
    #[error("mutation-grid artifact identity is inconsistent")]
    InconsistentIdentity,
    /// Stored proof fields were not the exact recomputation of their cited grid.
    #[error("mutation-grid proof is inconsistent")]
    InconsistentProof,
    /// Canonical encoding or typed content identity derivation failed.
    #[error("mutation-grid codec error: {message}")]
    Codec {
        /// Stable codec diagnostic.
        message: String,
    },
}

/// One reviewed generic mutant and its policy-visible fault class.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedMutantV1 {
    definition: ContentId<TrustedMutantDefinitionArtifact>,
    fault_class: FaultClassName,
}

impl TrustedMutantV1 {
    /// Binds one exact mutant implementation to the fault class policies reason about.
    #[must_use]
    pub const fn new(
        definition: ContentId<TrustedMutantDefinitionArtifact>,
        fault_class: FaultClassName,
    ) -> Self {
        Self {
            definition,
            fault_class,
        }
    }

    /// Returns the exact reviewed mutant definition.
    #[must_use]
    pub const fn definition(&self) -> ContentId<TrustedMutantDefinitionArtifact> {
        self.definition
    }

    /// Returns the fault class this mutant exercises.
    #[must_use]
    pub const fn fault_class(&self) -> &FaultClassName {
        &self.fault_class
    }
}

/// Strict V1 trusted generic-mutant set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "GenericMutantSetWire")]
pub struct GenericMutantSetV1 {
    schema_version: VerificationSchemaV1,
    mutants: Vec<TrustedMutantV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericMutantSetWire {
    schema_version: VerificationSchemaV1,
    mutants: Vec<TrustedMutantV1>,
}

impl GenericMutantSetV1 {
    /// Constructs a non-empty set in strict mutant-definition identity order.
    ///
    /// # Errors
    ///
    /// Rejects an empty, duplicated, or reordered set.
    pub fn new(mutants: Vec<TrustedMutantV1>) -> Result<Self, MutationGridError> {
        if mutants.is_empty() {
            return Err(MutationGridError::EmptyMutantSet);
        }
        if mutants
            .windows(2)
            .any(|pair| pair[0].definition.to_wire() >= pair[1].definition.to_wire())
        {
            return Err(MutationGridError::NonCanonicalMutants);
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            mutants,
        })
    }

    /// Returns the exact versioned mutant definitions in canonical order.
    #[must_use]
    pub fn mutants(&self) -> &[TrustedMutantV1] {
        &self.mutants
    }
}

impl TryFrom<GenericMutantSetWire> for GenericMutantSetV1 {
    type Error = MutationGridError;

    fn try_from(wire: GenericMutantSetWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.mutants)
    }
}

/// Canonical trusted-mutant set ready for archival and policy binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedGenericMutantSet {
    mutant_set: GenericMutantSetV1,
    mutant_set_bytes: Vec<u8>,
    mutant_set_id: ContentId<GenericMutantSetArtifact>,
}

impl PreparedGenericMutantSet {
    /// Returns the validated mutant set.
    #[must_use]
    pub const fn mutant_set(&self) -> &GenericMutantSetV1 {
        &self.mutant_set
    }

    /// Returns canonical mutant-set bytes.
    #[must_use]
    pub fn mutant_set_bytes(&self) -> &[u8] {
        &self.mutant_set_bytes
    }

    /// Returns the typed mutant-set content identity.
    #[must_use]
    pub const fn mutant_set_id(&self) -> ContentId<GenericMutantSetArtifact> {
        self.mutant_set_id
    }
}

/// Validates and canonically encodes a trusted generic-mutant set.
///
/// # Errors
///
/// Returns an error for invalid ordering or canonical encoding failure.
pub fn prepare_generic_mutant_set(
    mutants: Vec<TrustedMutantV1>,
) -> Result<PreparedGenericMutantSet, MutationGridError> {
    let mutant_set = GenericMutantSetV1::new(mutants)?;
    let mutant_set_bytes = cairn_codec::to_vec(&mutant_set).map_err(codec)?;
    let mutant_set_id =
        ContentId::<GenericMutantSetArtifact>::derive(&mutant_set_bytes).map_err(codec)?;
    Ok(PreparedGenericMutantSet {
        mutant_set,
        mutant_set_bytes,
        mutant_set_id,
    })
}

/// Stable coordinates of one mutant/case trial cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MutationGridCellV1 {
    mutant: ContentId<TrustedMutantDefinitionArtifact>,
    case: ContentId<MutationCaseArtifact>,
}

impl MutationGridCellV1 {
    /// Identifies one exact cell of the Cartesian mutation grid.
    #[must_use]
    pub const fn new(
        mutant: ContentId<TrustedMutantDefinitionArtifact>,
        case: ContentId<MutationCaseArtifact>,
    ) -> Self {
        Self { mutant, case }
    }

    /// Returns the exact mutant definition.
    #[must_use]
    pub const fn mutant(self) -> ContentId<TrustedMutantDefinitionArtifact> {
        self.mutant
    }

    /// Returns the exact domain-adapter mutation case.
    #[must_use]
    pub const fn case(self) -> ContentId<MutationCaseArtifact> {
        self.case
    }
}

/// How one applicable mutation is sized relative to the announced semantic boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MutationSizing {
    /// Derived from the announced boundary; a miss violates the comparator's own contract.
    PolicySized,
    /// Invariantly destructive or outside any admitted numerical allowance.
    ScaleFree,
    /// Its detectability legitimately depends on the concrete case and allowance.
    CaseDependent,
}

/// Direct trusted comparison observation for one applied mutant.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MutationDetection {
    /// The cited comparison distinguished the mutation.
    Detected,
    /// The cited comparison did not distinguish the mutation.
    Missed,
}

/// Evidence shape for one mutation cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "disposition", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MutationTrialResultV1 {
    /// The mutant was injected and traversed a recorded execution/comparison path.
    Applied {
        /// Boundary relationship controlling whether a miss is fatal.
        sizing: MutationSizing,
        /// Evidence that this mutation was constructed for this cell.
        injection: ContentId<MutationInjectionArtifact>,
        /// Authoritative execution/observation evidence.
        execution: ContentId<MutationExecutionArtifact>,
        /// Exact scopes traversed, in strict canonical order.
        execution_scopes: Vec<AdmissionExecutionScope>,
        /// Trusted comparison facts for the resulting observations.
        comparison: ContentId<MutationComparisonArtifact>,
        /// Whether those comparison facts detected the injected mutation.
        detection: MutationDetection,
    },
    /// This mutant cannot be constructed for this case.
    NotInjectable {
        /// Exact reviewed explanation; absence is never treated as not-injectable.
        reason: ContentId<NonInjectableReasonArtifact>,
    },
}

impl MutationTrialResultV1 {
    fn validate(&self) -> Result<(), MutationGridError> {
        if let Self::Applied {
            execution_scopes, ..
        } = self
        {
            if execution_scopes.is_empty()
                || execution_scopes.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(MutationGridError::InconsistentGrid);
            }
        }
        Ok(())
    }
}

/// One exact trial in the complete Cartesian mutation grid.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MutationTrialV1 {
    cell: MutationGridCellV1,
    result: MutationTrialResultV1,
}

impl MutationTrialV1 {
    /// Records an applied mutation and its underlying evidence facts.
    #[must_use]
    pub const fn applied(
        cell: MutationGridCellV1,
        sizing: MutationSizing,
        injection: ContentId<MutationInjectionArtifact>,
        execution: ContentId<MutationExecutionArtifact>,
        execution_scopes: Vec<AdmissionExecutionScope>,
        comparison: ContentId<MutationComparisonArtifact>,
        detection: MutationDetection,
    ) -> Self {
        Self {
            cell,
            result: MutationTrialResultV1::Applied {
                sizing,
                injection,
                execution,
                execution_scopes,
                comparison,
                detection,
            },
        }
    }

    /// Records a reviewed reason that a mutant cannot be injected for this case.
    #[must_use]
    pub const fn not_injectable(
        cell: MutationGridCellV1,
        reason: ContentId<NonInjectableReasonArtifact>,
    ) -> Self {
        Self {
            cell,
            result: MutationTrialResultV1::NotInjectable { reason },
        }
    }

    /// Returns this trial's exact Cartesian coordinates.
    #[must_use]
    pub const fn cell(&self) -> MutationGridCellV1 {
        self.cell
    }

    /// Returns the recorded trial facts.
    #[must_use]
    pub const fn result(&self) -> &MutationTrialResultV1 {
        &self.result
    }
}

/// Strict V1 complete mutation-grid trial facts, without a stored pass/fail result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MutationGridWire")]
pub struct MutationGridV1 {
    schema_version: VerificationSchemaV1,
    policy: ContentId<AdmissionPolicyArtifact>,
    mutant_set: ContentId<GenericMutantSetArtifact>,
    subject: ContentId<ImplementationBundleArtifact>,
    corpus: ContentId<AdmissionCorpusArtifact>,
    mutants: Vec<ContentId<TrustedMutantDefinitionArtifact>>,
    cases: Vec<ContentId<MutationCaseArtifact>>,
    trials: Vec<MutationTrialV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationGridWire {
    schema_version: VerificationSchemaV1,
    policy: ContentId<AdmissionPolicyArtifact>,
    mutant_set: ContentId<GenericMutantSetArtifact>,
    subject: ContentId<ImplementationBundleArtifact>,
    corpus: ContentId<AdmissionCorpusArtifact>,
    mutants: Vec<ContentId<TrustedMutantDefinitionArtifact>>,
    cases: Vec<ContentId<MutationCaseArtifact>>,
    trials: Vec<MutationTrialV1>,
}

impl MutationGridV1 {
    fn new(
        policy: ContentId<AdmissionPolicyArtifact>,
        mutant_set: ContentId<GenericMutantSetArtifact>,
        subject: ContentId<ImplementationBundleArtifact>,
        corpus: ContentId<AdmissionCorpusArtifact>,
        mutants: Vec<ContentId<TrustedMutantDefinitionArtifact>>,
        cases: Vec<ContentId<MutationCaseArtifact>>,
        trials: Vec<MutationTrialV1>,
    ) -> Result<Self, MutationGridError> {
        validate_identity_order(&mutants, MutationGridError::NonCanonicalMutants)?;
        if cases.is_empty() {
            return Err(MutationGridError::EmptyCases);
        }
        validate_identity_order(&cases, MutationGridError::NonCanonicalCases)?;

        let expected_len = mutants
            .len()
            .checked_mul(cases.len())
            .ok_or(MutationGridError::InconsistentGrid)?;
        if trials.len() != expected_len
            || trials
                .windows(2)
                .any(|pair| cell_key(pair[0].cell) >= cell_key(pair[1].cell))
            || trials.iter().any(|trial| trial.result.validate().is_err())
        {
            return Err(MutationGridError::InconsistentGrid);
        }

        let mutant_ids: BTreeSet<_> = mutants.iter().map(ContentId::to_wire).collect();
        let case_ids: BTreeSet<_> = cases.iter().map(ContentId::to_wire).collect();
        for trial in &trials {
            if !mutant_ids.contains(&trial.cell.mutant.to_wire())
                || !case_ids.contains(&trial.cell.case.to_wire())
            {
                return Err(MutationGridError::InconsistentGrid);
            }
        }

        Ok(Self {
            schema_version: VerificationSchemaV1,
            policy,
            mutant_set,
            subject,
            corpus,
            mutants,
            cases,
            trials,
        })
    }

    /// Returns the exact admission policy governing this grid.
    #[must_use]
    pub const fn policy(&self) -> ContentId<AdmissionPolicyArtifact> {
        self.policy
    }

    /// Returns the exact generic-mutant set selected by that policy.
    #[must_use]
    pub const fn mutant_set(&self) -> ContentId<GenericMutantSetArtifact> {
        self.mutant_set
    }

    /// Returns the implementation whose observation path was mutated.
    #[must_use]
    pub const fn subject(&self) -> ContentId<ImplementationBundleArtifact> {
        self.subject
    }

    /// Returns the frozen admission corpus whose cases form the grid axis.
    #[must_use]
    pub const fn corpus(&self) -> ContentId<AdmissionCorpusArtifact> {
        self.corpus
    }

    /// Returns the canonical mutant axes.
    #[must_use]
    pub fn mutants(&self) -> &[ContentId<TrustedMutantDefinitionArtifact>] {
        &self.mutants
    }

    /// Returns the canonical case axes.
    #[must_use]
    pub fn cases(&self) -> &[ContentId<MutationCaseArtifact>] {
        &self.cases
    }

    /// Returns every cell in canonical mutant/case order.
    #[must_use]
    pub fn trials(&self) -> &[MutationTrialV1] {
        &self.trials
    }

    fn validate_against(
        &self,
        policy: &AdmissionPolicyV1,
        mutant_set: &PreparedGenericMutantSet,
    ) -> Result<(), MutationGridError> {
        let policy_bytes = cairn_codec::to_vec(policy).map_err(codec)?;
        let policy_id =
            ContentId::<AdmissionPolicyArtifact>::derive(&policy_bytes).map_err(codec)?;
        let recomputed_set_id =
            ContentId::<GenericMutantSetArtifact>::derive(&mutant_set.mutant_set_bytes)
                .map_err(codec)?;
        if recomputed_set_id != mutant_set.mutant_set_id {
            return Err(MutationGridError::InconsistentIdentity);
        }
        if policy.mutant_set() != mutant_set.mutant_set_id
            || self.mutant_set != mutant_set.mutant_set_id
        {
            return Err(MutationGridError::PolicyMutantSetMismatch);
        }
        let expected_mutants: Vec<_> = mutant_set
            .mutant_set
            .mutants
            .iter()
            .map(TrustedMutantV1::definition)
            .collect();
        if self.policy != policy_id || self.mutants != expected_mutants {
            return Err(MutationGridError::InconsistentIdentity);
        }
        Ok(())
    }
}

impl TryFrom<MutationGridWire> for MutationGridV1 {
    type Error = MutationGridError;

    fn try_from(wire: MutationGridWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.policy,
            wire.mutant_set,
            wire.subject,
            wire.corpus,
            wire.mutants,
            wire.cases,
            wire.trials,
        )
    }
}

/// Canonical complete mutation grid ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMutationGrid {
    grid: MutationGridV1,
    grid_bytes: Vec<u8>,
    grid_id: ContentId<MutationGridArtifact>,
}

impl PreparedMutationGrid {
    /// Returns the complete validated grid.
    #[must_use]
    pub const fn grid(&self) -> &MutationGridV1 {
        &self.grid
    }

    /// Returns canonical grid bytes.
    #[must_use]
    pub fn grid_bytes(&self) -> &[u8] {
        &self.grid_bytes
    }

    /// Returns the typed grid identity.
    #[must_use]
    pub const fn grid_id(&self) -> ContentId<MutationGridArtifact> {
        self.grid_id
    }
}

/// Constructs a complete Cartesian mutation grid bound to one policy and mutant set.
///
/// The caller supplies every mutant/case trial. Missing, extra, duplicated, or reordered cells fail
/// closed instead of becoming implicit skips.
///
/// # Errors
///
/// Returns an error for mismatched policy identities, incomplete grids, or codec failure. One
/// batched execution or comparison artifact may legitimately support multiple cells.
pub fn prepare_mutation_grid(
    policy: &AdmissionPolicyV1,
    mutant_set: &PreparedGenericMutantSet,
    subject: ContentId<ImplementationBundleArtifact>,
    corpus: ContentId<AdmissionCorpusArtifact>,
    cases: Vec<ContentId<MutationCaseArtifact>>,
    trials: Vec<MutationTrialV1>,
) -> Result<PreparedMutationGrid, MutationGridError> {
    if policy.mutant_set() != mutant_set.mutant_set_id {
        return Err(MutationGridError::PolicyMutantSetMismatch);
    }
    let policy_bytes = cairn_codec::to_vec(policy).map_err(codec)?;
    let policy_id = ContentId::<AdmissionPolicyArtifact>::derive(&policy_bytes).map_err(codec)?;
    let mutants = mutant_set
        .mutant_set
        .mutants
        .iter()
        .map(TrustedMutantV1::definition)
        .collect();
    let grid = MutationGridV1::new(
        policy_id,
        mutant_set.mutant_set_id,
        subject,
        corpus,
        mutants,
        cases,
        trials,
    )?;
    grid.validate_against(policy, mutant_set)?;
    let grid_bytes = cairn_codec::to_vec(&grid).map_err(codec)?;
    let grid_id = ContentId::<MutationGridArtifact>::derive(&grid_bytes).map_err(codec)?;
    Ok(PreparedMutationGrid {
        grid,
        grid_bytes,
        grid_id,
    })
}

/// One failed proof obligation recomputed from underlying mutation trials.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "obligation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MutationGridProofFailureV1 {
    /// Every cell was classified not-injectable.
    EmptyApplicableGrid,
    /// No applicable trial exercised a policy-required fault class.
    RequiredFaultClassUnexercised {
        /// Required class with no applied cell.
        fault_class: FaultClassName,
    },
    /// An applied cell did not traverse both implementation and observation-pipeline scopes.
    ImplementationPathNotExercised {
        /// Exact deficient trial.
        cell: MutationGridCellV1,
    },
    /// A policy-sized or scale-free mutation was missed.
    FatalMutationMiss {
        /// Exact missed trial.
        cell: MutationGridCellV1,
        /// Fatal sizing class; case-dependent misses are blind spots instead.
        sizing: MutationSizing,
    },
}

/// Strict V1 recomputation product with no stored `passed` field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "MutationGridProofWire")]
pub struct MutationGridProofV1 {
    schema_version: VerificationSchemaV1,
    policy: ContentId<AdmissionPolicyArtifact>,
    grid: ContentId<MutationGridArtifact>,
    failures: Vec<MutationGridProofFailureV1>,
    blind_spots: Vec<MutationGridCellV1>,
    non_injectable: Vec<MutationGridCellV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationGridProofWire {
    schema_version: VerificationSchemaV1,
    policy: ContentId<AdmissionPolicyArtifact>,
    grid: ContentId<MutationGridArtifact>,
    failures: Vec<MutationGridProofFailureV1>,
    blind_spots: Vec<MutationGridCellV1>,
    non_injectable: Vec<MutationGridCellV1>,
}

impl MutationGridProofV1 {
    fn new(
        policy: ContentId<AdmissionPolicyArtifact>,
        grid: ContentId<MutationGridArtifact>,
        failures: Vec<MutationGridProofFailureV1>,
        blind_spots: Vec<MutationGridCellV1>,
        non_injectable: Vec<MutationGridCellV1>,
    ) -> Result<Self, MutationGridError> {
        if failures
            .windows(2)
            .any(|pair| proof_failure_key(&pair[0]) >= proof_failure_key(&pair[1]))
            || blind_spots
                .windows(2)
                .any(|pair| cell_key(pair[0]) >= cell_key(pair[1]))
            || non_injectable
                .windows(2)
                .any(|pair| cell_key(pair[0]) >= cell_key(pair[1]))
            || failures.iter().any(|failure| {
                matches!(
                    failure,
                    MutationGridProofFailureV1::FatalMutationMiss {
                        sizing: MutationSizing::CaseDependent,
                        ..
                    }
                )
            })
        {
            return Err(MutationGridError::InconsistentProof);
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            policy,
            grid,
            failures,
            blind_spots,
            non_injectable,
        })
    }

    /// Returns failed obligations in canonical class/cell order.
    #[must_use]
    pub fn failures(&self) -> &[MutationGridProofFailureV1] {
        &self.failures
    }

    /// Returns mandatory non-fatal case-dependent misses.
    #[must_use]
    pub fn blind_spots(&self) -> &[MutationGridCellV1] {
        &self.blind_spots
    }

    /// Returns every explicitly non-injectable cell.
    #[must_use]
    pub fn non_injectable(&self) -> &[MutationGridCellV1] {
        &self.non_injectable
    }

    /// Derives whether mutation-grid obligations are satisfied from the failure list.
    #[must_use]
    pub fn obligations_satisfied(&self) -> bool {
        self.failures.is_empty()
    }

    /// Recomputes the proof from the exact cited policy, mutant set, and grid.
    ///
    /// # Errors
    ///
    /// Rejects any changed input identity or persisted derived field.
    pub fn validate_against(
        &self,
        policy: &AdmissionPolicyV1,
        mutant_set: &PreparedGenericMutantSet,
        grid: &PreparedMutationGrid,
    ) -> Result<(), MutationGridError> {
        let recomputed = recompute_mutation_grid_proof(policy, mutant_set, grid)?;
        if recomputed.proof != *self {
            return Err(MutationGridError::InconsistentProof);
        }
        Ok(())
    }
}

impl TryFrom<MutationGridProofWire> for MutationGridProofV1 {
    type Error = MutationGridError;

    fn try_from(wire: MutationGridProofWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.policy,
            wire.grid,
            wire.failures,
            wire.blind_spots,
            wire.non_injectable,
        )
    }
}

/// Canonical recomputed mutation-grid proof ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMutationGridProof {
    proof: MutationGridProofV1,
    proof_bytes: Vec<u8>,
    proof_id: ContentId<MutationGridProofArtifact>,
}

impl PreparedMutationGridProof {
    /// Returns recomputed proof facts.
    #[must_use]
    pub const fn proof(&self) -> &MutationGridProofV1 {
        &self.proof
    }

    /// Returns canonical proof bytes.
    #[must_use]
    pub fn proof_bytes(&self) -> &[u8] {
        &self.proof_bytes
    }

    /// Returns the typed proof identity.
    #[must_use]
    pub const fn proof_id(&self) -> ContentId<MutationGridProofArtifact> {
        self.proof_id
    }
}

/// Recomputes fatal misses, path failures, required-class coverage, blind spots, and
/// non-injectable cells from the complete grid.
///
/// No caller-supplied summary or `passed` bit participates in this calculation.
///
/// # Errors
///
/// Returns an error if any cited identity or grid invariant is inconsistent.
pub fn recompute_mutation_grid_proof(
    policy: &AdmissionPolicyV1,
    mutant_set: &PreparedGenericMutantSet,
    grid: &PreparedMutationGrid,
) -> Result<PreparedMutationGridProof, MutationGridError> {
    grid.grid.validate_against(policy, mutant_set)?;
    let recomputed_grid_id =
        ContentId::<MutationGridArtifact>::derive(&grid.grid_bytes).map_err(codec)?;
    if recomputed_grid_id != grid.grid_id {
        return Err(MutationGridError::InconsistentIdentity);
    }

    let fault_classes: BTreeMap<_, _> = mutant_set
        .mutant_set
        .mutants
        .iter()
        .map(|mutant| (mutant.definition.to_wire(), mutant.fault_class.clone()))
        .collect();
    let mut applicable_count = 0_usize;
    let mut exercised_fault_classes = BTreeSet::new();
    let mut path_failures = Vec::new();
    let mut fatal_misses = Vec::new();
    let mut blind_spots = Vec::new();
    let mut non_injectable = Vec::new();

    for trial in &grid.grid.trials {
        match &trial.result {
            MutationTrialResultV1::Applied {
                sizing,
                execution_scopes,
                detection,
                ..
            } => {
                applicable_count += 1;
                let fault_class = fault_classes
                    .get(&trial.cell.mutant.to_wire())
                    .ok_or(MutationGridError::InconsistentIdentity)?;
                exercised_fault_classes.insert(fault_class.clone());
                if !execution_scopes.contains(&AdmissionExecutionScope::ObservationPipeline)
                    || !execution_scopes.contains(&AdmissionExecutionScope::Implementation)
                {
                    path_failures.push(
                        MutationGridProofFailureV1::ImplementationPathNotExercised {
                            cell: trial.cell,
                        },
                    );
                }
                if *detection == MutationDetection::Missed {
                    match sizing {
                        MutationSizing::PolicySized | MutationSizing::ScaleFree => {
                            fatal_misses.push(MutationGridProofFailureV1::FatalMutationMiss {
                                cell: trial.cell,
                                sizing: *sizing,
                            });
                        }
                        MutationSizing::CaseDependent => blind_spots.push(trial.cell),
                    }
                }
            }
            MutationTrialResultV1::NotInjectable { .. } => non_injectable.push(trial.cell),
        }
    }

    let mut failures = Vec::new();
    if applicable_count == 0 {
        failures.push(MutationGridProofFailureV1::EmptyApplicableGrid);
    }
    failures.extend(
        policy
            .required_fault_classes()
            .iter()
            .filter(|fault_class| !exercised_fault_classes.contains(*fault_class))
            .cloned()
            .map(
                |fault_class| MutationGridProofFailureV1::RequiredFaultClassUnexercised {
                    fault_class,
                },
            ),
    );
    failures.extend(path_failures);
    failures.extend(fatal_misses);

    let proof = MutationGridProofV1::new(
        grid.grid.policy,
        grid.grid_id,
        failures,
        blind_spots,
        non_injectable,
    )?;
    let proof_bytes = cairn_codec::to_vec(&proof).map_err(codec)?;
    let proof_id = ContentId::<MutationGridProofArtifact>::derive(&proof_bytes).map_err(codec)?;
    Ok(PreparedMutationGridProof {
        proof,
        proof_bytes,
        proof_id,
    })
}

fn validate_identity_order<T: ContentType>(
    values: &[ContentId<T>],
    error: MutationGridError,
) -> Result<(), MutationGridError> {
    if values.is_empty()
        || values
            .windows(2)
            .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(error);
    }
    Ok(())
}

fn cell_key(cell: MutationGridCellV1) -> (String, String) {
    (cell.mutant.to_wire(), cell.case.to_wire())
}

fn proof_failure_key(failure: &MutationGridProofFailureV1) -> (u8, String, String, u8) {
    match failure {
        MutationGridProofFailureV1::EmptyApplicableGrid => (0, String::new(), String::new(), 0),
        MutationGridProofFailureV1::RequiredFaultClassUnexercised { fault_class } => {
            (1, fault_class.as_str().to_owned(), String::new(), 0)
        }
        MutationGridProofFailureV1::ImplementationPathNotExercised { cell } => {
            let (mutant, case) = cell_key(*cell);
            (2, mutant, case, 0)
        }
        MutationGridProofFailureV1::FatalMutationMiss { cell, sizing } => {
            let (mutant, case) = cell_key(*cell);
            let sizing = match sizing {
                MutationSizing::PolicySized => 0,
                MutationSizing::ScaleFree => 1,
                MutationSizing::CaseDependent => 2,
            };
            (3, mutant, case, sizing)
        }
    }
}

fn codec(error: impl std::fmt::Display) -> MutationGridError {
    MutationGridError::Codec {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use cairn_protocol::ContentId;

    use super::*;
    use crate::{
        AdmissionPolicyInput, BudgetExhaustionOutcome, ConstructionClassName,
        CorrectVariantMinimum, IncorrectVariantMinimum, OracleStrength, SaturationRoundCount,
        StructuralIndependenceRequirement,
    };

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("test identity")
    }

    fn ordered<T: ContentType>(mut values: Vec<ContentId<T>>) -> Vec<ContentId<T>> {
        values.sort_by_key(ContentId::to_wire);
        values
    }

    fn fixture_mutants() -> PreparedGenericMutantSet {
        let definitions = ordered(vec![
            id::<TrustedMutantDefinitionArtifact>(b"offset-mutant"),
            id::<TrustedMutantDefinitionArtifact>(b"zero-mutant"),
        ]);
        prepare_generic_mutant_set(
            definitions
                .into_iter()
                .map(|definition| {
                    let fault_class =
                        if definition == id::<TrustedMutantDefinitionArtifact>(b"offset-mutant") {
                            FaultClassName::new("offset").expect("fault class")
                        } else {
                            FaultClassName::new("zero-output").expect("fault class")
                        };
                    TrustedMutantV1::new(definition, fault_class)
                })
                .collect(),
        )
        .expect("mutant set")
    }

    fn policy(mutant_set: ContentId<GenericMutantSetArtifact>) -> AdmissionPolicyV1 {
        AdmissionPolicyV1::new(AdmissionPolicyInput {
            mutant_set,
            minimum_correct_variants: CorrectVariantMinimum::new(2).expect("minimum"),
            minimum_incorrect_variants: IncorrectVariantMinimum::new(3).expect("minimum"),
            required_construction_classes: vec![
                ConstructionClassName::new("linear-order").expect("class"),
                ConstructionClassName::new("tree-order").expect("class"),
            ],
            required_fault_classes: vec![
                FaultClassName::new("offset").expect("class"),
                FaultClassName::new("zero-output").expect("class"),
            ],
            structural_independence: StructuralIndependenceRequirement::DistinctConstructionClaims,
            saturation_rounds: SaturationRoundCount::new(2).expect("rounds"),
            accepted_strengths: vec![OracleStrength::Reference],
            required_execution_scopes: vec![
                AdmissionExecutionScope::ObservationPipeline,
                AdmissionExecutionScope::Implementation,
            ],
            budget_exhaustion_outcome: BudgetExhaustionOutcome::Unverifiable,
        })
        .expect("policy")
    }

    fn evidence<T: ContentType>(prefix: &str, cell: MutationGridCellV1) -> ContentId<T> {
        ContentId::derive(
            format!("{prefix}:{}:{}", cell.mutant.to_wire(), cell.case.to_wire()).as_bytes(),
        )
        .expect("evidence identity")
    }

    fn applied(
        cell: MutationGridCellV1,
        sizing: MutationSizing,
        detection: MutationDetection,
        scopes: Vec<AdmissionExecutionScope>,
    ) -> MutationTrialV1 {
        MutationTrialV1::applied(
            cell,
            sizing,
            evidence::<MutationInjectionArtifact>("injection", cell),
            evidence::<MutationExecutionArtifact>("execution", cell),
            scopes,
            evidence::<MutationComparisonArtifact>("comparison", cell),
            detection,
        )
    }

    fn full_scope() -> Vec<AdmissionExecutionScope> {
        vec![
            AdmissionExecutionScope::ObservationPipeline,
            AdmissionExecutionScope::Implementation,
        ]
    }

    #[test]
    fn proof_is_recomputed_from_every_trial_without_a_stored_pass_field() {
        let mutant_set = fixture_mutants();
        let policy = policy(mutant_set.mutant_set_id());
        let cases = ordered(vec![
            id::<MutationCaseArtifact>(b"case-a"),
            id::<MutationCaseArtifact>(b"case-b"),
        ]);
        let mutants: Vec<_> = mutant_set
            .mutant_set()
            .mutants()
            .iter()
            .map(TrustedMutantV1::definition)
            .collect();
        let cells = [
            MutationGridCellV1::new(mutants[0], cases[0]),
            MutationGridCellV1::new(mutants[0], cases[1]),
            MutationGridCellV1::new(mutants[1], cases[0]),
            MutationGridCellV1::new(mutants[1], cases[1]),
        ];
        let trials = vec![
            applied(
                cells[0],
                MutationSizing::PolicySized,
                MutationDetection::Detected,
                full_scope(),
            ),
            applied(
                cells[1],
                MutationSizing::CaseDependent,
                MutationDetection::Missed,
                full_scope(),
            ),
            applied(
                cells[2],
                MutationSizing::ScaleFree,
                MutationDetection::Missed,
                full_scope(),
            ),
            MutationTrialV1::not_injectable(
                cells[3],
                evidence::<NonInjectableReasonArtifact>("reason", cells[3]),
            ),
        ];
        let grid = prepare_mutation_grid(
            &policy,
            &mutant_set,
            id::<ImplementationBundleArtifact>(b"subject"),
            id::<AdmissionCorpusArtifact>(b"corpus"),
            cases,
            trials,
        )
        .expect("complete grid");
        let proof = recompute_mutation_grid_proof(&policy, &mutant_set, &grid).expect("proof");

        assert!(!proof.proof().obligations_satisfied());
        assert_eq!(proof.proof().failures().len(), 1);
        assert!(matches!(
            proof.proof().failures()[0],
            MutationGridProofFailureV1::FatalMutationMiss {
                sizing: MutationSizing::ScaleFree,
                ..
            }
        ));
        assert_eq!(proof.proof().blind_spots(), &[cells[1]]);
        assert_eq!(proof.proof().non_injectable(), &[cells[3]]);
        proof
            .proof()
            .validate_against(&policy, &mutant_set, &grid)
            .expect("recomputed proof");

        let decoded_grid: MutationGridV1 =
            cairn_codec::from_slice(grid.grid_bytes()).expect("strict grid round trip");
        assert_eq!(&decoded_grid, grid.grid());
        let decoded_proof: MutationGridProofV1 =
            cairn_codec::from_slice(proof.proof_bytes()).expect("strict proof round trip");
        assert_eq!(&decoded_proof, proof.proof());

        let mut value: serde_json::Value =
            serde_json::from_slice(proof.proof_bytes()).expect("proof json");
        value["passed"] = serde_json::json!(true);
        assert!(serde_json::from_value::<MutationGridProofV1>(value).is_err());
    }

    #[test]
    fn incomplete_and_reordered_grids_fail_closed_while_batch_evidence_may_be_shared() {
        let mutant_set = fixture_mutants();
        let policy = policy(mutant_set.mutant_set_id());
        let cases = ordered(vec![
            id::<MutationCaseArtifact>(b"case-a"),
            id::<MutationCaseArtifact>(b"case-b"),
        ]);
        let mutants: Vec<_> = mutant_set
            .mutant_set()
            .mutants()
            .iter()
            .map(TrustedMutantV1::definition)
            .collect();
        let mut trials = Vec::new();
        for mutant in &mutants {
            for case in &cases {
                let cell = MutationGridCellV1::new(*mutant, *case);
                trials.push(applied(
                    cell,
                    MutationSizing::PolicySized,
                    MutationDetection::Detected,
                    full_scope(),
                ));
            }
        }

        let mut missing = trials.clone();
        missing.pop();
        assert_eq!(
            prepare_mutation_grid(
                &policy,
                &mutant_set,
                id::<ImplementationBundleArtifact>(b"subject"),
                id::<AdmissionCorpusArtifact>(b"corpus"),
                cases.clone(),
                missing,
            ),
            Err(MutationGridError::InconsistentGrid)
        );

        let mut reordered = trials.clone();
        reordered.swap(0, 1);
        assert_eq!(
            prepare_mutation_grid(
                &policy,
                &mutant_set,
                id::<ImplementationBundleArtifact>(b"subject"),
                id::<AdmissionCorpusArtifact>(b"corpus"),
                cases.clone(),
                reordered,
            ),
            Err(MutationGridError::InconsistentGrid)
        );

        let reused = trials[0].result.clone();
        trials[1].result = reused;
        prepare_mutation_grid(
            &policy,
            &mutant_set,
            id::<ImplementationBundleArtifact>(b"subject"),
            id::<AdmissionCorpusArtifact>(b"corpus"),
            cases,
            trials,
        )
        .expect("one batch artifact may support multiple case comparisons");
    }

    #[test]
    fn empty_applicable_grid_and_comparator_only_trials_are_explicit_failures() {
        let mutant_set = fixture_mutants();
        let policy = policy(mutant_set.mutant_set_id());
        let cases = vec![id::<MutationCaseArtifact>(b"only-case")];
        let mutants: Vec<_> = mutant_set
            .mutant_set()
            .mutants()
            .iter()
            .map(TrustedMutantV1::definition)
            .collect();
        let trials: Vec<_> = mutants
            .iter()
            .map(|mutant| {
                let cell = MutationGridCellV1::new(*mutant, cases[0]);
                MutationTrialV1::not_injectable(
                    cell,
                    evidence::<NonInjectableReasonArtifact>("reason", cell),
                )
            })
            .collect();
        let empty = prepare_mutation_grid(
            &policy,
            &mutant_set,
            id::<ImplementationBundleArtifact>(b"subject"),
            id::<AdmissionCorpusArtifact>(b"corpus"),
            cases.clone(),
            trials,
        )
        .expect("recorded non-injectable grid");
        let proof = recompute_mutation_grid_proof(&policy, &mutant_set, &empty).expect("proof");
        assert!(matches!(
            proof.proof().failures()[0],
            MutationGridProofFailureV1::EmptyApplicableGrid
        ));
        assert_eq!(proof.proof().failures().len(), 3);

        let trials: Vec<_> = mutants
            .iter()
            .map(|mutant| {
                applied(
                    MutationGridCellV1::new(*mutant, cases[0]),
                    MutationSizing::ScaleFree,
                    MutationDetection::Detected,
                    vec![AdmissionExecutionScope::Comparator],
                )
            })
            .collect();
        let comparator_only = prepare_mutation_grid(
            &policy,
            &mutant_set,
            id::<ImplementationBundleArtifact>(b"subject"),
            id::<AdmissionCorpusArtifact>(b"corpus"),
            cases.clone(),
            trials,
        )
        .expect("comparator-only facts remain recordable");
        let proof =
            recompute_mutation_grid_proof(&policy, &mutant_set, &comparator_only).expect("proof");
        assert_eq!(proof.proof().failures().len(), 2);
        assert!(proof.proof().failures().iter().all(|failure| matches!(
            failure,
            MutationGridProofFailureV1::ImplementationPathNotExercised { .. }
        )));

        let trials: Vec<_> = mutants
            .iter()
            .map(|mutant| {
                applied(
                    MutationGridCellV1::new(*mutant, cases[0]),
                    MutationSizing::ScaleFree,
                    MutationDetection::Detected,
                    full_scope(),
                )
            })
            .collect();
        let complete = prepare_mutation_grid(
            &policy,
            &mutant_set,
            id::<ImplementationBundleArtifact>(b"subject"),
            id::<AdmissionCorpusArtifact>(b"corpus"),
            cases,
            trials,
        )
        .expect("implementation-path grid");
        let proof = recompute_mutation_grid_proof(&policy, &mutant_set, &complete).expect("proof");
        assert!(proof.proof().obligations_satisfied());
        assert!(proof.proof().failures().is_empty());
    }

    #[test]
    fn policy_identity_selects_one_exact_mutant_set() {
        let mutant_set = fixture_mutants();
        let other_set = prepare_generic_mutant_set(vec![TrustedMutantV1::new(
            id::<TrustedMutantDefinitionArtifact>(b"different"),
            FaultClassName::new("offset").expect("fault class"),
        )])
        .expect("other set");
        let policy = policy(mutant_set.mutant_set_id());
        assert_ne!(mutant_set.mutant_set_id(), other_set.mutant_set_id());
        assert_eq!(
            prepare_mutation_grid(
                &policy,
                &other_set,
                id::<ImplementationBundleArtifact>(b"subject"),
                id::<AdmissionCorpusArtifact>(b"corpus"),
                vec![id::<MutationCaseArtifact>(b"case")],
                Vec::new(),
            ),
            Err(MutationGridError::PolicyMutantSetMismatch)
        );
    }
}
