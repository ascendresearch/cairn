//! Exact semantic comparison of reference and judged-subject corpus observations.

use std::collections::BTreeSet;

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::CallerDomainBodyArtifact;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ArgumentIndex, BufferName, CallAdapterCompletionV1, CallAdapterOutputBytesArtifact,
    CallAdapterResultArtifact, CorpusBufferByteLength, CorpusExecutionPlanArtifact,
    CorpusExecutionSubjectV1, CorpusObligationIdentityV1, CorpusObservationSetArtifact,
    MigrationDomainContractV1, PreparedCorpusExecutionPlan, SemanticClaimKind,
    ValidatedCorpusObservationSet,
};

/// Failure to produce an exact, role-safe reference/subject comparison.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ExactCorpusComparisonError {
    /// Only the current pre-release V1 comparison is accepted.
    #[error("exact corpus comparison schema version must be 1")]
    UnsupportedSchemaVersion,
    /// The caller domain requests another semantic comparison strength.
    #[error("exact comparison requires an exact semantic claim")]
    NonExactDomain,
    /// The baseline plan is not explicitly bound to a reference artifact.
    #[error("exact comparison baseline must have the reference subject role")]
    ReferenceRoleRequired,
    /// The judged plan is neither a candidate nor an admission variant.
    #[error("exact comparison subject must have a candidate or admission-variant role")]
    JudgedSubjectRoleRequired,
    /// Plans do not cover the same domain and mandatory obligations.
    #[error("reference and judged-subject corpus plans are incompatible")]
    IncompatiblePlans,
    /// Prepared plan or observation-set bytes/identities no longer agree.
    #[error("exact comparison input identity is inconsistent")]
    InconsistentInputIdentity,
    /// Paired observations disagree on ABI output metadata rather than only values.
    #[error("reference and judged-subject observation shapes are incompatible")]
    IncompatibleObservations,
    /// Persisted comparison collections are non-canonical or contradictory.
    #[error("exact corpus comparison is inconsistent")]
    InconsistentComparison,
    /// Canonical encoding or typed identity derivation failed.
    #[error("exact corpus comparison codec error: {message}")]
    Codec { message: String },
}

/// Exact value identities observed for one paired ABI output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExactOutputComparisonV1 {
    argument_index: ArgumentIndex,
    buffer: BufferName,
    byte_length: CorpusBufferByteLength,
    reference: ContentId<CallAdapterOutputBytesArtifact>,
    subject: ContentId<CallAdapterOutputBytesArtifact>,
}

impl ExactOutputComparisonV1 {
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

    #[must_use]
    pub const fn reference(&self) -> ContentId<CallAdapterOutputBytesArtifact> {
        self.reference
    }

    #[must_use]
    pub const fn subject(&self) -> ContentId<CallAdapterOutputBytesArtifact> {
        self.subject
    }

    /// Recomputes equality from the two immutable value identities.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.reference == self.subject
    }
}

/// Exact comparison facts for one typed corpus obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExactCaseComparisonV1 {
    obligation: CorpusObligationIdentityV1,
    reference_result: ContentId<CallAdapterResultArtifact>,
    subject_result: ContentId<CallAdapterResultArtifact>,
    reference_completion: CallAdapterCompletionV1,
    subject_completion: CallAdapterCompletionV1,
    outputs: Vec<ExactOutputComparisonV1>,
}

impl ExactCaseComparisonV1 {
    fn validate(&self) -> Result<(), ExactCorpusComparisonError> {
        let mut buffers = BTreeSet::new();
        if self
            .outputs
            .windows(2)
            .any(|pair| pair[0].argument_index >= pair[1].argument_index)
            || self
                .outputs
                .iter()
                .any(|output| !buffers.insert(&output.buffer))
        {
            return Err(ExactCorpusComparisonError::InconsistentComparison);
        }
        Ok(())
    }

    #[must_use]
    pub const fn obligation(&self) -> CorpusObligationIdentityV1 {
        self.obligation
    }

    #[must_use]
    pub const fn reference_result(&self) -> ContentId<CallAdapterResultArtifact> {
        self.reference_result
    }

    #[must_use]
    pub const fn subject_result(&self) -> ContentId<CallAdapterResultArtifact> {
        self.subject_result
    }

    #[must_use]
    pub const fn reference_completion(&self) -> &CallAdapterCompletionV1 {
        &self.reference_completion
    }

    #[must_use]
    pub const fn subject_completion(&self) -> &CallAdapterCompletionV1 {
        &self.subject_completion
    }

    #[must_use]
    pub fn outputs(&self) -> &[ExactOutputComparisonV1] {
        &self.outputs
    }

    /// Recomputes the case-level exact match; no stored `passed` field is trusted.
    #[must_use]
    pub fn matches(&self) -> bool {
        self.reference_completion == self.subject_completion
            && self.outputs.iter().all(ExactOutputComparisonV1::matches)
    }
}

/// Strict V1 exact-comparison facts with no oracle admission or subject verdict field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ExactCorpusComparisonWire")]
pub struct ExactCorpusComparisonV1 {
    schema_version: u16,
    domain: ContentId<CallerDomainBodyArtifact>,
    reference_plan: ContentId<CorpusExecutionPlanArtifact>,
    subject_plan: ContentId<CorpusExecutionPlanArtifact>,
    reference_observations: ContentId<CorpusObservationSetArtifact>,
    subject_observations: ContentId<CorpusObservationSetArtifact>,
    comparisons: Vec<ExactCaseComparisonV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactCorpusComparisonWire {
    schema_version: u16,
    domain: ContentId<CallerDomainBodyArtifact>,
    reference_plan: ContentId<CorpusExecutionPlanArtifact>,
    subject_plan: ContentId<CorpusExecutionPlanArtifact>,
    reference_observations: ContentId<CorpusObservationSetArtifact>,
    subject_observations: ContentId<CorpusObservationSetArtifact>,
    comparisons: Vec<ExactCaseComparisonV1>,
}

impl ExactCorpusComparisonV1 {
    fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        reference_plan: ContentId<CorpusExecutionPlanArtifact>,
        subject_plan: ContentId<CorpusExecutionPlanArtifact>,
        reference_observations: ContentId<CorpusObservationSetArtifact>,
        subject_observations: ContentId<CorpusObservationSetArtifact>,
        comparisons: Vec<ExactCaseComparisonV1>,
    ) -> Result<Self, ExactCorpusComparisonError> {
        if reference_plan == subject_plan
            || reference_observations == subject_observations
            || comparisons.is_empty()
            || comparisons.windows(2).any(|pair| {
                obligation_key(pair[0].obligation) >= obligation_key(pair[1].obligation)
            })
            || comparisons
                .iter()
                .any(|comparison| comparison.validate().is_err())
        {
            return Err(ExactCorpusComparisonError::InconsistentComparison);
        }
        let mut reference_results = BTreeSet::new();
        let mut subject_results = BTreeSet::new();
        if comparisons.iter().any(|comparison| {
            !reference_results.insert(comparison.reference_result.to_wire())
                || !subject_results.insert(comparison.subject_result.to_wire())
        }) {
            return Err(ExactCorpusComparisonError::InconsistentComparison);
        }
        Ok(Self {
            schema_version: 1,
            domain,
            reference_plan,
            subject_plan,
            reference_observations,
            subject_observations,
            comparisons,
        })
    }

    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    #[must_use]
    pub const fn reference_plan(&self) -> ContentId<CorpusExecutionPlanArtifact> {
        self.reference_plan
    }

    #[must_use]
    pub const fn subject_plan(&self) -> ContentId<CorpusExecutionPlanArtifact> {
        self.subject_plan
    }

    #[must_use]
    pub const fn reference_observations(&self) -> ContentId<CorpusObservationSetArtifact> {
        self.reference_observations
    }

    #[must_use]
    pub const fn subject_observations(&self) -> ContentId<CorpusObservationSetArtifact> {
        self.subject_observations
    }

    #[must_use]
    pub fn comparisons(&self) -> &[ExactCaseComparisonV1] {
        &self.comparisons
    }

    /// Recomputes whether every recorded exact fact matches.
    ///
    /// This is not an oracle admission or trusted adjudication.
    #[must_use]
    pub fn all_match(&self) -> bool {
        self.comparisons.iter().all(ExactCaseComparisonV1::matches)
    }

    /// Fully recomputes this persisted comparison from the cited execution evidence.
    ///
    /// # Errors
    ///
    /// Rejects changed inputs or any stored fact that differs from trusted recomputation.
    pub fn validate_inputs(
        &self,
        domain: &MigrationDomainContractV1,
        reference_plan: &PreparedCorpusExecutionPlan,
        reference: &ValidatedCorpusObservationSet,
        subject_plan: &PreparedCorpusExecutionPlan,
        subject: &ValidatedCorpusObservationSet,
    ) -> Result<(), ExactCorpusComparisonError> {
        let recomputed = compare_exact_corpus_observations(
            domain,
            reference_plan,
            reference,
            subject_plan,
            subject,
        )?;
        if recomputed.comparison != *self {
            return Err(ExactCorpusComparisonError::InconsistentComparison);
        }
        Ok(())
    }
}

impl TryFrom<ExactCorpusComparisonWire> for ExactCorpusComparisonV1 {
    type Error = ExactCorpusComparisonError;

    fn try_from(wire: ExactCorpusComparisonWire) -> Result<Self, Self::Error> {
        if wire.schema_version != 1 {
            return Err(ExactCorpusComparisonError::UnsupportedSchemaVersion);
        }
        Self::new(
            wire.domain,
            wire.reference_plan,
            wire.subject_plan,
            wire.reference_observations,
            wire.subject_observations,
            wire.comparisons,
        )
    }
}

/// Content domain for exact reference/subject corpus-comparison facts.
pub enum ExactCorpusComparisonArtifact {}

impl ContentType for ExactCorpusComparisonArtifact {
    const DOMAIN: &'static str = "migration.exact-corpus-comparison.v1";
}

/// Canonical exact-comparison artifact ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedExactCorpusComparison {
    comparison: ExactCorpusComparisonV1,
    comparison_bytes: Vec<u8>,
    comparison_id: ContentId<ExactCorpusComparisonArtifact>,
}

impl PreparedExactCorpusComparison {
    #[must_use]
    pub const fn comparison(&self) -> &ExactCorpusComparisonV1 {
        &self.comparison
    }

    #[must_use]
    pub fn comparison_bytes(&self) -> &[u8] {
        &self.comparison_bytes
    }

    #[must_use]
    pub const fn comparison_id(&self) -> ContentId<ExactCorpusComparisonArtifact> {
        self.comparison_id
    }
}

/// Compares a proposed reference observation set with a candidate or admission variant byte-exactly.
///
/// The result is comparison evidence only. Reference admission and terminal adjudication remain
/// separate trusted stages.
///
/// # Errors
///
/// Rejects non-exact domains, wrong subject roles, incompatible plans, inconsistent identities,
/// non-corresponding observations, or non-canonical comparison material.
pub fn compare_exact_corpus_observations(
    domain: &MigrationDomainContractV1,
    reference_plan: &PreparedCorpusExecutionPlan,
    reference: &ValidatedCorpusObservationSet,
    subject_plan: &PreparedCorpusExecutionPlan,
    subject: &ValidatedCorpusObservationSet,
) -> Result<PreparedExactCorpusComparison, ExactCorpusComparisonError> {
    validate_comparison_inputs(domain, reference_plan, reference, subject_plan, subject)?;
    let comparisons = reference
        .cases()
        .iter()
        .zip(subject.cases())
        .map(|(reference, subject)| compare_case(reference, subject))
        .collect::<Result<Vec<_>, _>>()?;
    let domain_bytes = cairn_codec::to_vec(domain).map_err(codec)?;
    let domain_id = ContentId::<CallerDomainBodyArtifact>::derive(&domain_bytes).map_err(codec)?;
    let comparison = ExactCorpusComparisonV1::new(
        domain_id,
        reference_plan.plan_id(),
        subject_plan.plan_id(),
        reference.observation_set_id(),
        subject.observation_set_id(),
        comparisons,
    )?;
    let comparison_bytes = cairn_codec::to_vec(&comparison).map_err(codec)?;
    let comparison_id =
        ContentId::<ExactCorpusComparisonArtifact>::derive(&comparison_bytes).map_err(codec)?;
    Ok(PreparedExactCorpusComparison {
        comparison,
        comparison_bytes,
        comparison_id,
    })
}

fn validate_comparison_inputs(
    domain: &MigrationDomainContractV1,
    reference_plan: &PreparedCorpusExecutionPlan,
    reference: &ValidatedCorpusObservationSet,
    subject_plan: &PreparedCorpusExecutionPlan,
    subject: &ValidatedCorpusObservationSet,
) -> Result<(), ExactCorpusComparisonError> {
    if domain.semantic_claim() != SemanticClaimKind::Exact {
        return Err(ExactCorpusComparisonError::NonExactDomain);
    }
    if !matches!(
        reference_plan.plan().subject(),
        CorpusExecutionSubjectV1::Reference { .. }
    ) {
        return Err(ExactCorpusComparisonError::ReferenceRoleRequired);
    }
    if !matches!(
        subject_plan.plan().subject(),
        CorpusExecutionSubjectV1::Candidate { .. }
            | CorpusExecutionSubjectV1::AdmissionVariant { .. }
    ) {
        return Err(ExactCorpusComparisonError::JudgedSubjectRoleRequired);
    }
    validate_plan_identity(reference_plan)?;
    validate_plan_identity(subject_plan)?;
    validate_observation_identity(reference_plan, reference)?;
    validate_observation_identity(subject_plan, subject)?;
    let domain_id =
        ContentId::<CallerDomainBodyArtifact>::derive(&cairn_codec::to_vec(domain).map_err(codec)?)
            .map_err(codec)?;
    let reference = reference_plan.plan();
    let subject = subject_plan.plan();
    if reference.domain() != domain_id
        || subject.domain() != domain_id
        || reference.quantitative_obligations() != subject.quantitative_obligations()
        || reference.input_value_obligations() != subject.input_value_obligations()
        || reference.memory_surface_obligations() != subject.memory_surface_obligations()
        || reference.items().len() != subject.items().len()
        || reference
            .items()
            .iter()
            .zip(subject.items())
            .any(|(reference, subject)| {
                reference.obligation() != subject.obligation()
                    || reference.expected_outcome() != subject.expected_outcome()
            })
    {
        return Err(ExactCorpusComparisonError::IncompatiblePlans);
    }
    Ok(())
}

fn validate_plan_identity(
    plan: &PreparedCorpusExecutionPlan,
) -> Result<(), ExactCorpusComparisonError> {
    let bytes = cairn_codec::to_vec(plan.plan()).map_err(codec)?;
    if bytes != plan.plan_bytes()
        || ContentId::<CorpusExecutionPlanArtifact>::derive(&bytes).map_err(codec)?
            != plan.plan_id()
    {
        return Err(ExactCorpusComparisonError::InconsistentInputIdentity);
    }
    Ok(())
}

fn validate_observation_identity(
    plan: &PreparedCorpusExecutionPlan,
    observations: &ValidatedCorpusObservationSet,
) -> Result<(), ExactCorpusComparisonError> {
    observations
        .observation_set()
        .validate_plan(plan.plan())
        .map_err(|_| ExactCorpusComparisonError::InconsistentInputIdentity)?;
    let bytes = cairn_codec::to_vec(observations.observation_set()).map_err(codec)?;
    if bytes != observations.observation_set_bytes()
        || ContentId::<CorpusObservationSetArtifact>::derive(&bytes).map_err(codec)?
            != observations.observation_set_id()
        || observations.cases().len() != observations.observation_set().observations().len()
        || observations
            .cases()
            .iter()
            .zip(observations.observation_set().observations())
            .any(|(case, observation)| case.observation() != observation)
    {
        return Err(ExactCorpusComparisonError::InconsistentInputIdentity);
    }
    Ok(())
}

fn compare_case(
    reference: &crate::ValidatedCorpusExecutionCase,
    subject: &crate::ValidatedCorpusExecutionCase,
) -> Result<ExactCaseComparisonV1, ExactCorpusComparisonError> {
    if reference.observation().obligation() != subject.observation().obligation() {
        return Err(ExactCorpusComparisonError::IncompatibleObservations);
    }
    let reference_result = reference.execution().observation().result();
    let subject_result = subject.execution().observation().result();
    if reference_result.outputs().len() != subject_result.outputs().len() {
        return Err(ExactCorpusComparisonError::IncompatibleObservations);
    }
    let outputs = reference_result
        .outputs()
        .iter()
        .zip(subject_result.outputs())
        .map(|(reference, subject)| {
            if reference.argument_index() != subject.argument_index()
                || reference.buffer() != subject.buffer()
                || reference.byte_length() != subject.byte_length()
            {
                return Err(ExactCorpusComparisonError::IncompatibleObservations);
            }
            Ok(ExactOutputComparisonV1 {
                argument_index: reference.argument_index(),
                buffer: reference.buffer().clone(),
                byte_length: reference.byte_length(),
                reference: reference.bytes(),
                subject: subject.bytes(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ExactCaseComparisonV1 {
        obligation: reference.observation().obligation(),
        reference_result: reference.observation().result(),
        subject_result: subject.observation().result(),
        reference_completion: reference_result.completion().clone(),
        subject_completion: subject_result.completion().clone(),
        outputs,
    })
}

fn obligation_key(obligation: CorpusObligationIdentityV1) -> (u8, String) {
    match obligation {
        CorpusObligationIdentityV1::Boundary { case } => (0, case.to_wire()),
        CorpusObligationIdentityV1::InputValue { case } => (1, case.to_wire()),
        CorpusObligationIdentityV1::MemorySurface { case } => (2, case.to_wire()),
    }
}

fn codec(error: impl std::fmt::Display) -> ExactCorpusComparisonError {
    ExactCorpusComparisonError::Codec {
        message: error.to_string(),
    }
}
