//! Optional model-backed proposal revisions, attacks, and trusted admission feedback.

use std::collections::HashSet;

use cairn_protocol::{ContentId, ContentType, EpisodeId};
use cairn_verification::{
    AuthorshipOrigin, CorpusCaseArtifact, ImplementationVariantArtifact, ImplementationVariantV1,
    OracleProposalArtifact, OracleProposalV1, VariantExpectation,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ExternalTestSearchResultArtifact, OracleDebateStrategy, OracleModelDebatePlanArtifact,
    OracleModelDebatePlanV1,
};

const SCHEMA_V1: u16 = 1;

macro_rules! artifact {
    ($name:ident, $domain:literal) => {
        /// Marker for one strict Oracle Agent workflow artifact.
        pub enum $name {}
        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

artifact!(
    OracleDebateProposalRevisionArtifact,
    "migration.oracle-model-debate-proposal-revision.v1"
);
artifact!(
    OracleDebateAttackArtifact,
    "migration.oracle-model-debate-attack.v1"
);
artifact!(
    OracleDebateAdmissionAttemptArtifact,
    "migration.oracle-model-debate-admission-attempt.v1"
);
artifact!(
    OracleDebateDiagnosticEvidenceArtifact,
    "migration.oracle-model-debate-diagnostic-evidence.v1"
);
artifact!(
    OracleDebateAdmissionFeedbackArtifact,
    "migration.oracle-model-debate-admission-feedback.v1"
);

/// Immutable synthesis submission and its explicit revision lineage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "OracleDebateProposalRevisionWire",
    into = "OracleDebateProposalRevisionWire"
)]
pub struct OracleDebateProposalRevisionV1 {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    parent: Option<ContentId<OracleDebateProposalRevisionArtifact>>,
    proposal: ContentId<OracleProposalArtifact>,
    submitted_by: EpisodeId,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDebateProposalRevisionWire {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    parent: Option<ContentId<OracleDebateProposalRevisionArtifact>>,
    proposal: ContentId<OracleProposalArtifact>,
    submitted_by: EpisodeId,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

impl OracleDebateProposalRevisionV1 {
    /// Returns the exact `OracleModelDebate` plan.
    #[must_use]
    pub const fn debate_plan(&self) -> ContentId<OracleModelDebatePlanArtifact> {
        self.debate_plan
    }

    /// Returns the prior immutable revision, absent only for the first proposal.
    #[must_use]
    pub const fn parent(&self) -> Option<ContentId<OracleDebateProposalRevisionArtifact>> {
        self.parent
    }

    /// Returns the exact ordinary oracle proposal.
    #[must_use]
    pub const fn proposal(&self) -> ContentId<OracleProposalArtifact> {
        self.proposal
    }

    /// Returns external research result identities in canonical order.
    #[must_use]
    pub fn external_research(&self) -> &[ContentId<ExternalTestSearchResultArtifact>] {
        &self.external_research
    }

    fn validate_shape(&self) -> Result<(), OracleDebateWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleDebateWorkflowError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        validate_canonical_ids(&self.external_research, "external research")
    }
}

impl TryFrom<OracleDebateProposalRevisionWire> for OracleDebateProposalRevisionV1 {
    type Error = OracleDebateWorkflowError;

    fn try_from(wire: OracleDebateProposalRevisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            debate_plan: wire.debate_plan,
            parent: wire.parent,
            proposal: wire.proposal,
            submitted_by: wire.submitted_by,
            external_research: wire.external_research,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleDebateProposalRevisionV1> for OracleDebateProposalRevisionWire {
    fn from(value: OracleDebateProposalRevisionV1) -> Self {
        Self {
            schema_version: value.schema_version,
            debate_plan: value.debate_plan,
            parent: value.parent,
            proposal: value.proposal,
            submitted_by: value.submitted_by,
            external_research: value.external_research,
        }
    }
}

/// Privately prepared proposal revision whose identities were recomputed from exact bodies.
#[derive(Clone, Debug)]
pub struct PreparedOracleDebateProposalRevision {
    body: OracleDebateProposalRevisionV1,
    id: ContentId<OracleDebateProposalRevisionArtifact>,
    proposal: OracleProposalV1,
}

impl PreparedOracleDebateProposalRevision {
    /// Returns the strict revision body.
    #[must_use]
    pub const fn body(&self) -> &OracleDebateProposalRevisionV1 {
        &self.body
    }

    /// Returns its recomputed semantic identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleDebateProposalRevisionArtifact> {
        self.id
    }

    /// Returns the validated ordinary proposal body.
    #[must_use]
    pub const fn proposal(&self) -> &OracleProposalV1 {
        &self.proposal
    }
}

/// Prepares the first or a changed synthesis proposal revision.
///
/// # Errors
///
/// Rejects task/domain/input disagreement, non-model or wrong-episode authorship, wrong frozen
/// model configuration, non-canonical research identities, or an unchanged child revision.
pub fn prepare_oracle_debate_proposal_revision(
    plan: &OracleModelDebatePlanV1,
    parent: Option<&PreparedOracleDebateProposalRevision>,
    proposal: OracleProposalV1,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
) -> Result<PreparedOracleDebateProposalRevision, OracleDebateWorkflowError> {
    if plan.synthesis().strategy() != OracleDebateStrategy::Synthesis
        || proposal.task_id() != plan.task_id()
        || proposal.task_inputs() != plan.task_inputs()
        || proposal.declared_domain() != plan.declared_domain()
        || proposal.authorship().origin() != AuthorshipOrigin::Model
        || proposal.authorship().episode_id() != Some(plan.synthesis().episode_id())
        || proposal.authorship().model_configuration()
            != Some(plan.synthesis().authorship_configuration())
    {
        return Err(OracleDebateWorkflowError::SynthesisProposalMismatch);
    }
    validate_canonical_ids(&external_research, "external research")?;
    let debate_plan = content_id::<OracleModelDebatePlanArtifact>(plan)?;
    if let Some(parent) = parent {
        if parent.body.debate_plan != debate_plan {
            return Err(OracleDebateWorkflowError::RevisionLineageMismatch);
        }
    }
    let proposal_id = content_id::<OracleProposalArtifact>(&proposal)?;
    if parent.is_some_and(|parent| parent.body.proposal == proposal_id) {
        return Err(OracleDebateWorkflowError::UnchangedRevision);
    }
    let body = OracleDebateProposalRevisionV1 {
        schema_version: SCHEMA_V1,
        debate_plan,
        parent: parent.map(PreparedOracleDebateProposalRevision::id),
        proposal: proposal_id,
        submitted_by: plan.synthesis().episode_id(),
        external_research,
    };
    body.validate_shape()?;
    let id = content_id::<OracleDebateProposalRevisionArtifact>(&body)?;
    Ok(PreparedOracleDebateProposalRevision { body, id, proposal })
}

/// Constructor input for one frozen adversarial attack over a synthesis proposal revision.
pub struct OracleDebateAttackInput {
    /// Correct-by-construction variants submitted by adversarial.
    pub correct_variants: Vec<ImplementationVariantV1>,
    /// Deliberately wrong variants submitted by adversarial.
    pub wrong_variants: Vec<ImplementationVariantV1>,
    /// Additional adversarial case proposals in canonical identity order.
    pub adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

/// Adversarial-submitted attack manifest. It contains no admission decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleDebateAttackWire", into = "OracleDebateAttackWire")]
pub struct OracleDebateAttackV1 {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    proposal_revision: ContentId<OracleDebateProposalRevisionArtifact>,
    submitted_by: EpisodeId,
    correct_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    wrong_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDebateAttackWire {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    proposal_revision: ContentId<OracleDebateProposalRevisionArtifact>,
    submitted_by: EpisodeId,
    correct_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    wrong_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

impl OracleDebateAttackV1 {
    /// Returns the exact attacked proposal revision.
    #[must_use]
    pub const fn proposal_revision(&self) -> ContentId<OracleDebateProposalRevisionArtifact> {
        self.proposal_revision
    }

    /// Returns correct-by-construction variant identities.
    #[must_use]
    pub fn correct_variants(&self) -> &[ContentId<ImplementationVariantArtifact>] {
        &self.correct_variants
    }

    /// Returns deliberately wrong variant identities.
    #[must_use]
    pub fn wrong_variants(&self) -> &[ContentId<ImplementationVariantArtifact>] {
        &self.wrong_variants
    }

    fn validate_shape(&self) -> Result<(), OracleDebateWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleDebateWorkflowError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.correct_variants.is_empty() || self.wrong_variants.is_empty() {
            return Err(OracleDebateWorkflowError::IncompleteAttack);
        }
        validate_canonical_ids(&self.correct_variants, "correct variants")?;
        validate_canonical_ids(&self.wrong_variants, "wrong variants")?;
        validate_canonical_ids(&self.adversarial_cases, "adversarial cases")
    }
}

impl TryFrom<OracleDebateAttackWire> for OracleDebateAttackV1 {
    type Error = OracleDebateWorkflowError;

    fn try_from(wire: OracleDebateAttackWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            debate_plan: wire.debate_plan,
            proposal_revision: wire.proposal_revision,
            submitted_by: wire.submitted_by,
            correct_variants: wire.correct_variants,
            wrong_variants: wire.wrong_variants,
            adversarial_cases: wire.adversarial_cases,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleDebateAttackV1> for OracleDebateAttackWire {
    fn from(value: OracleDebateAttackV1) -> Self {
        Self {
            schema_version: value.schema_version,
            debate_plan: value.debate_plan,
            proposal_revision: value.proposal_revision,
            submitted_by: value.submitted_by,
            correct_variants: value.correct_variants,
            wrong_variants: value.wrong_variants,
            adversarial_cases: value.adversarial_cases,
        }
    }
}

/// Privately prepared adversarial attack with validated variant bodies.
#[derive(Clone, Debug)]
pub struct PreparedOracleDebateAttack {
    body: OracleDebateAttackV1,
    id: ContentId<OracleDebateAttackArtifact>,
}

impl PreparedOracleDebateAttack {
    /// Returns the strict attack body.
    #[must_use]
    pub const fn body(&self) -> &OracleDebateAttackV1 {
        &self.body
    }

    /// Returns the recomputed attack identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleDebateAttackArtifact> {
        self.id
    }
}

/// Prepares a adversarial attack over one exact synthesis revision.
///
/// # Errors
///
/// Rejects missing correct/wrong families, wrong expectation classes, wrong strategy/model authorship,
/// non-canonical cases, or inconsistent plan/revision identities.
pub fn prepare_oracle_debate_attack(
    plan: &OracleModelDebatePlanV1,
    revision: &PreparedOracleDebateProposalRevision,
    input: OracleDebateAttackInput,
) -> Result<PreparedOracleDebateAttack, OracleDebateWorkflowError> {
    let debate_plan = content_id::<OracleModelDebatePlanArtifact>(plan)?;
    if revision.body.debate_plan != debate_plan
        || plan.adversarial().strategy() != OracleDebateStrategy::Adversarial
    {
        return Err(OracleDebateWorkflowError::AttackLineageMismatch);
    }
    if input.correct_variants.is_empty() || input.wrong_variants.is_empty() {
        return Err(OracleDebateWorkflowError::IncompleteAttack);
    }
    for variant in input.correct_variants.iter().chain(&input.wrong_variants) {
        if variant.authorship().origin() != AuthorshipOrigin::Model
            || variant.authorship().episode_id() != Some(plan.adversarial().episode_id())
            || variant.authorship().model_configuration()
                != Some(plan.adversarial().authorship_configuration())
        {
            return Err(OracleDebateWorkflowError::AdversarialVariantMismatch);
        }
    }
    if input
        .correct_variants
        .iter()
        .any(|variant| !matches!(variant.expectation(), VariantExpectation::MustAccept { .. }))
        || input
            .wrong_variants
            .iter()
            .any(|variant| !matches!(variant.expectation(), VariantExpectation::MustReject { .. }))
    {
        return Err(OracleDebateWorkflowError::VariantExpectationMismatch);
    }
    validate_canonical_ids(&input.adversarial_cases, "adversarial cases")?;
    let mut correct_variants = input
        .correct_variants
        .iter()
        .map(content_id::<ImplementationVariantArtifact>)
        .collect::<Result<Vec<_>, _>>()?;
    let mut wrong_variants = input
        .wrong_variants
        .iter()
        .map(content_id::<ImplementationVariantArtifact>)
        .collect::<Result<Vec<_>, _>>()?;
    correct_variants.sort_by_key(ContentId::to_wire);
    wrong_variants.sort_by_key(ContentId::to_wire);
    validate_canonical_ids(&correct_variants, "correct variants")?;
    validate_canonical_ids(&wrong_variants, "wrong variants")?;
    let body = OracleDebateAttackV1 {
        schema_version: SCHEMA_V1,
        debate_plan,
        proposal_revision: revision.id,
        submitted_by: plan.adversarial().episode_id(),
        correct_variants,
        wrong_variants,
        adversarial_cases: input.adversarial_cases,
    };
    body.validate_shape()?;
    let id = content_id::<OracleDebateAttackArtifact>(&body)?;
    Ok(PreparedOracleDebateAttack { body, id })
}

/// Strategy-visible classification of one trusted admission diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleDebateDiagnosticKind {
    /// Required proposal evidence is absent or inconsistent.
    MissingEvidence,
    /// Caller, source, external, or model domain claims disagree.
    DomainConflict,
    /// An incorrect implementation passed the proposed instrument.
    FalseAccept,
    /// A correct-by-construction implementation failed the proposed instrument.
    FalseReject,
    /// A target/device claim remains outside executed evidence.
    UnverifiedTarget,
    /// Required evidence could not run due to infrastructure state.
    InfrastructureBlocked,
}

/// Which strategy may inspect and act on one trusted diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleDebateFeedbackTarget {
    /// Synthesis proposal revision is required.
    Synthesis,
    /// Adversarial attack/variant revision is required.
    Adversarial,
    /// Both strategies receive the same submitted diagnostic artifact.
    Both,
}

/// One typed diagnostic carrying only submitted evidence, never private strategy continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleDebateDiagnosticV1 {
    /// Intended strategy audience.
    pub target: OracleDebateFeedbackTarget,
    /// Closed failure/limitation class.
    pub kind: OracleDebateDiagnosticKind,
    /// Exact trusted evidence or diagnostic body.
    pub evidence: ContentId<OracleDebateDiagnosticEvidenceArtifact>,
}

/// Trusted diagnostic bundle returned after one admission attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "OracleDebateAdmissionFeedbackWire",
    into = "OracleDebateAdmissionFeedbackWire"
)]
pub struct OracleDebateAdmissionFeedbackV1 {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    proposal_revision: ContentId<OracleDebateProposalRevisionArtifact>,
    attack: ContentId<OracleDebateAttackArtifact>,
    admission_attempt: ContentId<OracleDebateAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDebateDiagnosticV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDebateAdmissionFeedbackWire {
    schema_version: u16,
    debate_plan: ContentId<OracleModelDebatePlanArtifact>,
    proposal_revision: ContentId<OracleDebateProposalRevisionArtifact>,
    attack: ContentId<OracleDebateAttackArtifact>,
    admission_attempt: ContentId<OracleDebateAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDebateDiagnosticV1>,
}

impl OracleDebateAdmissionFeedbackV1 {
    /// Returns the exact rejected/insufficient proposal revision.
    #[must_use]
    pub const fn proposal_revision(&self) -> ContentId<OracleDebateProposalRevisionArtifact> {
        self.proposal_revision
    }

    /// Returns canonical strategy-scoped diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[OracleDebateDiagnosticV1] {
        &self.diagnostics
    }

    fn validate_shape(&self) -> Result<(), OracleDebateWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleDebateWorkflowError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.diagnostics.is_empty()
            || self
                .diagnostics
                .windows(2)
                .any(|pair| diagnostic_key(&pair[0]) >= diagnostic_key(&pair[1]))
        {
            return Err(OracleDebateWorkflowError::InvalidDiagnostics);
        }
        Ok(())
    }
}

fn diagnostic_key(
    diagnostic: &OracleDebateDiagnosticV1,
) -> (
    OracleDebateFeedbackTarget,
    OracleDebateDiagnosticKind,
    String,
) {
    (
        diagnostic.target,
        diagnostic.kind,
        diagnostic.evidence.to_wire(),
    )
}

impl TryFrom<OracleDebateAdmissionFeedbackWire> for OracleDebateAdmissionFeedbackV1 {
    type Error = OracleDebateWorkflowError;

    fn try_from(wire: OracleDebateAdmissionFeedbackWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            debate_plan: wire.debate_plan,
            proposal_revision: wire.proposal_revision,
            attack: wire.attack,
            admission_attempt: wire.admission_attempt,
            diagnostics: wire.diagnostics,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleDebateAdmissionFeedbackV1> for OracleDebateAdmissionFeedbackWire {
    fn from(value: OracleDebateAdmissionFeedbackV1) -> Self {
        Self {
            schema_version: value.schema_version,
            debate_plan: value.debate_plan,
            proposal_revision: value.proposal_revision,
            attack: value.attack,
            admission_attempt: value.admission_attempt,
            diagnostics: value.diagnostics,
        }
    }
}

/// Privately prepared feedback with recomputed lineage and identity.
#[derive(Clone, Debug)]
pub struct PreparedOracleDebateAdmissionFeedback {
    body: OracleDebateAdmissionFeedbackV1,
    id: ContentId<OracleDebateAdmissionFeedbackArtifact>,
}

impl PreparedOracleDebateAdmissionFeedback {
    /// Returns the strict feedback body.
    #[must_use]
    pub const fn body(&self) -> &OracleDebateAdmissionFeedbackV1 {
        &self.body
    }

    /// Returns the recomputed feedback identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleDebateAdmissionFeedbackArtifact> {
        self.id
    }
}

/// Builds one trusted admission feedback bundle over exact proposal and attack identities.
///
/// # Errors
///
/// Rejects empty, duplicated, unsorted diagnostics or any plan/revision/attack lineage mismatch.
pub fn prepare_oracle_debate_admission_feedback(
    plan: &OracleModelDebatePlanV1,
    revision: &PreparedOracleDebateProposalRevision,
    attack: &PreparedOracleDebateAttack,
    admission_attempt: ContentId<OracleDebateAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDebateDiagnosticV1>,
) -> Result<PreparedOracleDebateAdmissionFeedback, OracleDebateWorkflowError> {
    let debate_plan = content_id::<OracleModelDebatePlanArtifact>(plan)?;
    if revision.body.debate_plan != debate_plan
        || attack.body.debate_plan != debate_plan
        || attack.body.proposal_revision != revision.id
    {
        return Err(OracleDebateWorkflowError::FeedbackLineageMismatch);
    }
    let body = OracleDebateAdmissionFeedbackV1 {
        schema_version: SCHEMA_V1,
        debate_plan,
        proposal_revision: revision.id,
        attack: attack.id,
        admission_attempt,
        diagnostics,
    };
    body.validate_shape()?;
    let id = content_id::<OracleDebateAdmissionFeedbackArtifact>(&body)?;
    Ok(PreparedOracleDebateAdmissionFeedback { body, id })
}

fn validate_canonical_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleDebateWorkflowError> {
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(OracleDebateWorkflowError::NonCanonicalSet { field });
    }
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value.to_wire())) {
        return Err(OracleDebateWorkflowError::NonCanonicalSet { field });
    }
    Ok(())
}

fn content_id<T: ContentType>(
    value: &impl Serialize,
) -> Result<ContentId<T>, OracleDebateWorkflowError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| OracleDebateWorkflowError::Encoding(error.to_string()))?;
    ContentId::derive(&bytes)
        .map_err(|error| OracleDebateWorkflowError::Encoding(error.to_string()))
}

/// Invalid model-authored Oracle Agent workflow composition.
#[derive(Debug, Error)]
pub enum OracleDebateWorkflowError {
    /// A schema other than the current V1 was supplied.
    #[error("unsupported Oracle Agent workflow schema version {0}")]
    UnsupportedSchema(u16),
    /// Synthesis proposal disagrees with the plan or its exact model authorship.
    #[error(
        "synthesis proposal does not match its OracleModelDebate task, domain, episode, or model"
    )]
    SynthesisProposalMismatch,
    /// Parent revision belongs to another plan.
    #[error("oracle proposal revision lineage does not match")]
    RevisionLineageMismatch,
    /// A child revision repeats the exact parent proposal.
    #[error("oracle proposal revision must change proposal identity")]
    UnchangedRevision,
    /// Attack is missing either false-accept or false-reject controls.
    #[error("oracle attack requires both correct and wrong variants")]
    IncompleteAttack,
    /// Attack and proposal revision belong to different plans.
    #[error("oracle attack lineage does not match")]
    AttackLineageMismatch,
    /// Adversarial variant has wrong episode/model authorship.
    #[error("adversarial variant does not match its OracleModelDebate episode or model")]
    AdversarialVariantMismatch,
    /// Correct/wrong variant is in the opposite expectation set.
    #[error("oracle attack variant expectation is inconsistent")]
    VariantExpectationMismatch,
    /// A content-addressed set is duplicated or not in canonical wire order.
    #[error("oracle workflow {field} is not a canonical set")]
    NonCanonicalSet { field: &'static str },
    /// Feedback has no diagnostics or is not in canonical order.
    #[error("oracle admission feedback diagnostics are invalid")]
    InvalidDiagnostics,
    /// Feedback references another plan, revision, or attack.
    #[error("oracle admission feedback lineage does not match")]
    FeedbackLineageMismatch,
    /// Canonical encoding or content identity failed.
    #[error("oracle workflow encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use cairn_agent::{ContextBlock, InstructionBlock, ResolvedRuntimeModelArtifact};
    use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
    use cairn_verification::{
        AdmissionPolicyArtifact, ArtifactAuthorId, ArtifactAuthorshipV1, AuthorshipOrigin,
        ConstructionClaimArtifact, CorpusCaseArtifact, CorpusProposalArtifact,
        DeclaredDomainArtifact, DomainRefinementArtifact, FaultClassName,
        FaultInjectionEvidenceArtifact, ImplementationBundleArtifact, ImplementationVariantV1,
        ModelConfigurationArtifact, ObservationPlanArtifact, OracleProposalInput, OracleProposalV1,
        OracleStrength, OracleTaskInputArtifact, ReferenceArtifact, SourceAdmissionPlanArtifact,
        ValidFamilyPlanArtifact, VariantExpectation,
    };

    use super::{
        OracleDebateAdmissionAttemptArtifact, OracleDebateAttackInput,
        OracleDebateDiagnosticEvidenceArtifact, OracleDebateDiagnosticKind,
        OracleDebateDiagnosticV1, OracleDebateFeedbackTarget,
        prepare_oracle_debate_admission_feedback, prepare_oracle_debate_attack,
        prepare_oracle_debate_proposal_revision,
    };
    use crate::{
        OracleDebateEpisodeInput, OracleDebateStrategy, OracleModelDebatePlanInput,
        OracleModelDebatePlanV1, prepare_oracle_debate_episode,
    };

    fn id<T: ContentType>(label: &str) -> ContentId<T> {
        ContentId::derive(label.as_bytes()).expect("identity")
    }

    fn plan() -> OracleModelDebatePlanV1 {
        let synthesis_episode = EpisodeId::new();
        let adversarial_episode = EpisodeId::new();
        OracleModelDebatePlanV1::new(OracleModelDebatePlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller")],
            synthesis: prepare_oracle_debate_episode(OracleDebateEpisodeInput {
                strategy: OracleDebateStrategy::Synthesis,
                episode_id: synthesis_episode,
                model_configuration: id::<ResolvedRuntimeModelArtifact>("synthesis-runtime-model"),
                authorship_configuration: id::<ModelConfigurationArtifact>(
                    "synthesis-author-model",
                ),
                strategy_instruction: id::<InstructionBlock>("synthesis"),
                private_context: Vec::new(),
                budget: cairn_agent::EpisodeBudget::default(),
            })
            .expect("synthesis"),
            adversarial: prepare_oracle_debate_episode(OracleDebateEpisodeInput {
                strategy: OracleDebateStrategy::Adversarial,
                episode_id: adversarial_episode,
                model_configuration: id::<ResolvedRuntimeModelArtifact>(
                    "adversarial-runtime-model",
                ),
                authorship_configuration: id::<ModelConfigurationArtifact>(
                    "adversarial-author-model",
                ),
                strategy_instruction: id::<InstructionBlock>("adversarial"),
                private_context: Vec::new(),
                budget: cairn_agent::EpisodeBudget::default(),
            })
            .expect("adversarial"),
        })
        .expect("plan")
    }

    fn authorship(
        episode: EpisodeId,
        model: ContentId<ModelConfigurationArtifact>,
    ) -> ArtifactAuthorshipV1 {
        ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Model,
            ArtifactAuthorId::new("recorded-model").expect("author"),
            Some(episode),
            Some(model),
        )
        .expect("authorship")
    }

    fn proposal(plan: &OracleModelDebatePlanV1, label: &str) -> OracleProposalV1 {
        OracleProposalV1::new(OracleProposalInput {
            task_id: plan.task_id(),
            task_inputs: plan.task_inputs(),
            declared_domain: plan.declared_domain(),
            domain_refinements: vec![id::<DomainRefinementArtifact>(label)],
            corpus_proposal: id::<CorpusProposalArtifact>("corpus"),
            references: vec![id::<ReferenceArtifact>("reference")],
            properties: Vec::new(),
            source_admission_plan: id::<SourceAdmissionPlanArtifact>("source-plan"),
            valid_family_plan: id::<ValidFamilyPlanArtifact>("family-plan"),
            observation_plan: id::<ObservationPlanArtifact>("observation-plan"),
            requested_strength: OracleStrength::Reference,
            authorship: authorship(
                plan.synthesis().episode_id(),
                plan.synthesis().authorship_configuration(),
            ),
        })
        .expect("proposal")
    }

    fn correct_variant(plan: &OracleModelDebatePlanV1, label: &str) -> ImplementationVariantV1 {
        ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>(label),
            VariantExpectation::MustAccept {
                construction_claim: id::<ConstructionClaimArtifact>("construction"),
            },
            authorship(
                plan.adversarial().episode_id(),
                plan.adversarial().authorship_configuration(),
            ),
        )
    }

    fn wrong_variant(plan: &OracleModelDebatePlanV1, label: &str) -> ImplementationVariantV1 {
        ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>(label),
            VariantExpectation::MustReject {
                fault_class: FaultClassName::new("zero-output").expect("fault"),
                fault_evidence: id::<FaultInjectionEvidenceArtifact>("fault-evidence"),
            },
            authorship(
                plan.adversarial().episode_id(),
                plan.adversarial().authorship_configuration(),
            ),
        )
    }

    #[test]
    fn isolated_model_authorship_drives_revision_attack_and_feedback() {
        let plan = plan();
        let first = prepare_oracle_debate_proposal_revision(
            &plan,
            None,
            proposal(&plan, "refinement-v1"),
            Vec::new(),
        )
        .expect("first proposal");
        let attack = prepare_oracle_debate_attack(
            &plan,
            &first,
            OracleDebateAttackInput {
                correct_variants: vec![correct_variant(&plan, "correct")],
                wrong_variants: vec![wrong_variant(&plan, "wrong")],
                adversarial_cases: vec![id::<CorpusCaseArtifact>("adversarial")],
            },
        )
        .expect("attack");
        let feedback = prepare_oracle_debate_admission_feedback(
            &plan,
            &first,
            &attack,
            id::<OracleDebateAdmissionAttemptArtifact>("attempt"),
            vec![OracleDebateDiagnosticV1 {
                target: OracleDebateFeedbackTarget::Synthesis,
                kind: OracleDebateDiagnosticKind::FalseAccept,
                evidence: id::<OracleDebateDiagnosticEvidenceArtifact>("diagnostic"),
            }],
        )
        .expect("feedback");
        assert_eq!(feedback.body().proposal_revision(), first.id());

        let second = prepare_oracle_debate_proposal_revision(
            &plan,
            Some(&first),
            proposal(&plan, "refinement-v2"),
            Vec::new(),
        )
        .expect("revision");
        assert_eq!(second.body().parent(), Some(first.id()));
        assert_ne!(second.id(), first.id());
    }

    #[test]
    fn synthesis_and_adversarial_authorship_cannot_be_swapped() {
        let plan = plan();
        let mut wrong_synthesis = proposal(&plan, "refinement");
        let mut value = serde_json::to_value(&wrong_synthesis).expect("proposal JSON");
        value["authorship"] = serde_json::to_value(authorship(
            plan.adversarial().episode_id(),
            plan.adversarial().authorship_configuration(),
        ))
        .expect("adversarial authorship");
        wrong_synthesis = serde_json::from_value(value).expect("structurally valid proposal");
        assert!(
            prepare_oracle_debate_proposal_revision(&plan, None, wrong_synthesis, Vec::new())
                .is_err()
        );

        let first = prepare_oracle_debate_proposal_revision(
            &plan,
            None,
            proposal(&plan, "refinement"),
            Vec::new(),
        )
        .expect("first");
        let synthesis_authored_variant = ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>("wrong-author"),
            VariantExpectation::MustAccept {
                construction_claim: id::<ConstructionClaimArtifact>("construction"),
            },
            authorship(
                plan.synthesis().episode_id(),
                plan.synthesis().authorship_configuration(),
            ),
        );
        assert!(
            prepare_oracle_debate_attack(
                &plan,
                &first,
                OracleDebateAttackInput {
                    correct_variants: vec![synthesis_authored_variant],
                    wrong_variants: vec![wrong_variant(&plan, "wrong")],
                    adversarial_cases: Vec::new(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn unchanged_revision_and_noncanonical_feedback_fail_closed() {
        let plan = plan();
        let first = prepare_oracle_debate_proposal_revision(
            &plan,
            None,
            proposal(&plan, "same"),
            Vec::new(),
        )
        .expect("first");
        assert!(
            prepare_oracle_debate_proposal_revision(
                &plan,
                Some(&first),
                proposal(&plan, "same"),
                Vec::new(),
            )
            .is_err()
        );
    }
}
