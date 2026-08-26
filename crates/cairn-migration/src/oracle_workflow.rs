//! Model-authored proposal revisions, red attacks, and trusted admission feedback.

use std::collections::HashSet;

use cairn_protocol::{ContentId, ContentType, EpisodeId};
use cairn_verification::{
    AuthorshipOrigin, CorpusCaseArtifact, ImplementationVariantArtifact, ImplementationVariantV1,
    OracleProposalArtifact, OracleProposalV1, VariantExpectation,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ExternalTestSearchResultArtifact, OracleAgentRole, OracleSearchPlanArtifact, OracleSearchPlanV1,
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
    OracleProposalRevisionArtifact,
    "migration.oracle-proposal-revision.v1"
);
artifact!(OracleAttackArtifact, "migration.oracle-attack.v1");
artifact!(
    OracleAdmissionAttemptArtifact,
    "migration.oracle-admission-attempt.v1"
);
artifact!(
    OracleDiagnosticEvidenceArtifact,
    "migration.oracle-diagnostic-evidence.v1"
);
artifact!(
    OracleAdmissionFeedbackArtifact,
    "migration.oracle-admission-feedback.v1"
);

/// Immutable blue submission and its explicit revision lineage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "OracleProposalRevisionWire",
    into = "OracleProposalRevisionWire"
)]
pub struct OracleProposalRevisionV1 {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    parent: Option<ContentId<OracleProposalRevisionArtifact>>,
    proposal: ContentId<OracleProposalArtifact>,
    submitted_by: EpisodeId,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleProposalRevisionWire {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    parent: Option<ContentId<OracleProposalRevisionArtifact>>,
    proposal: ContentId<OracleProposalArtifact>,
    submitted_by: EpisodeId,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

impl OracleProposalRevisionV1 {
    /// Returns the exact `OracleSearch` plan.
    #[must_use]
    pub const fn search_plan(&self) -> ContentId<OracleSearchPlanArtifact> {
        self.search_plan
    }

    /// Returns the prior immutable revision, absent only for the first proposal.
    #[must_use]
    pub const fn parent(&self) -> Option<ContentId<OracleProposalRevisionArtifact>> {
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

    fn validate_shape(&self) -> Result<(), OracleWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleWorkflowError::UnsupportedSchema(self.schema_version));
        }
        validate_canonical_ids(&self.external_research, "external research")
    }
}

impl TryFrom<OracleProposalRevisionWire> for OracleProposalRevisionV1 {
    type Error = OracleWorkflowError;

    fn try_from(wire: OracleProposalRevisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_plan: wire.search_plan,
            parent: wire.parent,
            proposal: wire.proposal,
            submitted_by: wire.submitted_by,
            external_research: wire.external_research,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleProposalRevisionV1> for OracleProposalRevisionWire {
    fn from(value: OracleProposalRevisionV1) -> Self {
        Self {
            schema_version: value.schema_version,
            search_plan: value.search_plan,
            parent: value.parent,
            proposal: value.proposal,
            submitted_by: value.submitted_by,
            external_research: value.external_research,
        }
    }
}

/// Privately prepared proposal revision whose identities were recomputed from exact bodies.
#[derive(Clone, Debug)]
pub struct PreparedOracleProposalRevision {
    body: OracleProposalRevisionV1,
    id: ContentId<OracleProposalRevisionArtifact>,
    proposal: OracleProposalV1,
}

impl PreparedOracleProposalRevision {
    /// Returns the strict revision body.
    #[must_use]
    pub const fn body(&self) -> &OracleProposalRevisionV1 {
        &self.body
    }

    /// Returns its recomputed semantic identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleProposalRevisionArtifact> {
        self.id
    }

    /// Returns the validated ordinary proposal body.
    #[must_use]
    pub const fn proposal(&self) -> &OracleProposalV1 {
        &self.proposal
    }
}

/// Prepares the first or a changed blue proposal revision.
///
/// # Errors
///
/// Rejects task/domain/input disagreement, non-model or wrong-episode authorship, wrong frozen
/// model configuration, non-canonical research identities, or an unchanged child revision.
pub fn prepare_oracle_proposal_revision(
    plan: &OracleSearchPlanV1,
    parent: Option<&PreparedOracleProposalRevision>,
    proposal: OracleProposalV1,
    external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
) -> Result<PreparedOracleProposalRevision, OracleWorkflowError> {
    if plan.blue().role() != OracleAgentRole::Blue
        || proposal.task_id() != plan.task_id()
        || proposal.task_inputs() != plan.task_inputs()
        || proposal.declared_domain() != plan.declared_domain()
        || proposal.authorship().origin() != AuthorshipOrigin::Model
        || proposal.authorship().episode_id() != Some(plan.blue().episode_id())
        || proposal.authorship().model_configuration()
            != Some(plan.blue().authorship_configuration())
    {
        return Err(OracleWorkflowError::BlueProposalMismatch);
    }
    validate_canonical_ids(&external_research, "external research")?;
    let search_plan = content_id::<OracleSearchPlanArtifact>(plan)?;
    if let Some(parent) = parent {
        if parent.body.search_plan != search_plan {
            return Err(OracleWorkflowError::RevisionLineageMismatch);
        }
    }
    let proposal_id = content_id::<OracleProposalArtifact>(&proposal)?;
    if parent.is_some_and(|parent| parent.body.proposal == proposal_id) {
        return Err(OracleWorkflowError::UnchangedRevision);
    }
    let body = OracleProposalRevisionV1 {
        schema_version: SCHEMA_V1,
        search_plan,
        parent: parent.map(PreparedOracleProposalRevision::id),
        proposal: proposal_id,
        submitted_by: plan.blue().episode_id(),
        external_research,
    };
    body.validate_shape()?;
    let id = content_id::<OracleProposalRevisionArtifact>(&body)?;
    Ok(PreparedOracleProposalRevision { body, id, proposal })
}

/// Constructor input for one frozen red attack over a blue proposal revision.
pub struct OracleAttackInput {
    /// Correct-by-construction variants submitted by red.
    pub correct_variants: Vec<ImplementationVariantV1>,
    /// Deliberately wrong variants submitted by red.
    pub wrong_variants: Vec<ImplementationVariantV1>,
    /// Additional adversarial case proposals in canonical identity order.
    pub adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

/// Red-submitted attack manifest. It contains no admission decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleAttackWire", into = "OracleAttackWire")]
pub struct OracleAttackV1 {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    proposal_revision: ContentId<OracleProposalRevisionArtifact>,
    submitted_by: EpisodeId,
    correct_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    wrong_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleAttackWire {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    proposal_revision: ContentId<OracleProposalRevisionArtifact>,
    submitted_by: EpisodeId,
    correct_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    wrong_variants: Vec<ContentId<ImplementationVariantArtifact>>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

impl OracleAttackV1 {
    /// Returns the exact attacked proposal revision.
    #[must_use]
    pub const fn proposal_revision(&self) -> ContentId<OracleProposalRevisionArtifact> {
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

    fn validate_shape(&self) -> Result<(), OracleWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleWorkflowError::UnsupportedSchema(self.schema_version));
        }
        if self.correct_variants.is_empty() || self.wrong_variants.is_empty() {
            return Err(OracleWorkflowError::IncompleteAttack);
        }
        validate_canonical_ids(&self.correct_variants, "correct variants")?;
        validate_canonical_ids(&self.wrong_variants, "wrong variants")?;
        validate_canonical_ids(&self.adversarial_cases, "adversarial cases")
    }
}

impl TryFrom<OracleAttackWire> for OracleAttackV1 {
    type Error = OracleWorkflowError;

    fn try_from(wire: OracleAttackWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_plan: wire.search_plan,
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

impl From<OracleAttackV1> for OracleAttackWire {
    fn from(value: OracleAttackV1) -> Self {
        Self {
            schema_version: value.schema_version,
            search_plan: value.search_plan,
            proposal_revision: value.proposal_revision,
            submitted_by: value.submitted_by,
            correct_variants: value.correct_variants,
            wrong_variants: value.wrong_variants,
            adversarial_cases: value.adversarial_cases,
        }
    }
}

/// Privately prepared red attack with validated variant bodies.
#[derive(Clone, Debug)]
pub struct PreparedOracleAttack {
    body: OracleAttackV1,
    id: ContentId<OracleAttackArtifact>,
}

impl PreparedOracleAttack {
    /// Returns the strict attack body.
    #[must_use]
    pub const fn body(&self) -> &OracleAttackV1 {
        &self.body
    }

    /// Returns the recomputed attack identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleAttackArtifact> {
        self.id
    }
}

/// Prepares a red attack over one exact blue revision.
///
/// # Errors
///
/// Rejects missing correct/wrong families, wrong expectation classes, wrong role/model authorship,
/// non-canonical cases, or inconsistent plan/revision identities.
pub fn prepare_oracle_attack(
    plan: &OracleSearchPlanV1,
    revision: &PreparedOracleProposalRevision,
    input: OracleAttackInput,
) -> Result<PreparedOracleAttack, OracleWorkflowError> {
    let search_plan = content_id::<OracleSearchPlanArtifact>(plan)?;
    if revision.body.search_plan != search_plan || plan.red().role() != OracleAgentRole::Red {
        return Err(OracleWorkflowError::AttackLineageMismatch);
    }
    if input.correct_variants.is_empty() || input.wrong_variants.is_empty() {
        return Err(OracleWorkflowError::IncompleteAttack);
    }
    for variant in input.correct_variants.iter().chain(&input.wrong_variants) {
        if variant.authorship().origin() != AuthorshipOrigin::Model
            || variant.authorship().episode_id() != Some(plan.red().episode_id())
            || variant.authorship().model_configuration()
                != Some(plan.red().authorship_configuration())
        {
            return Err(OracleWorkflowError::RedVariantMismatch);
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
        return Err(OracleWorkflowError::VariantExpectationMismatch);
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
    let body = OracleAttackV1 {
        schema_version: SCHEMA_V1,
        search_plan,
        proposal_revision: revision.id,
        submitted_by: plan.red().episode_id(),
        correct_variants,
        wrong_variants,
        adversarial_cases: input.adversarial_cases,
    };
    body.validate_shape()?;
    let id = content_id::<OracleAttackArtifact>(&body)?;
    Ok(PreparedOracleAttack { body, id })
}

/// Role-visible classification of one trusted admission diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleDiagnosticKind {
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

/// Which role may inspect and act on one trusted diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleFeedbackTarget {
    /// Blue proposal revision is required.
    Blue,
    /// Red attack/variant revision is required.
    Red,
    /// Both roles receive the same submitted diagnostic artifact.
    Both,
}

/// One typed diagnostic carrying only submitted evidence, never private role continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleDiagnosticV1 {
    /// Intended role audience.
    pub target: OracleFeedbackTarget,
    /// Closed failure/limitation class.
    pub kind: OracleDiagnosticKind,
    /// Exact trusted evidence or diagnostic body.
    pub evidence: ContentId<OracleDiagnosticEvidenceArtifact>,
}

/// Trusted diagnostic bundle returned after one admission attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "OracleAdmissionFeedbackWire",
    into = "OracleAdmissionFeedbackWire"
)]
pub struct OracleAdmissionFeedbackV1 {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    proposal_revision: ContentId<OracleProposalRevisionArtifact>,
    attack: ContentId<OracleAttackArtifact>,
    admission_attempt: ContentId<OracleAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDiagnosticV1>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionFeedbackWire {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    proposal_revision: ContentId<OracleProposalRevisionArtifact>,
    attack: ContentId<OracleAttackArtifact>,
    admission_attempt: ContentId<OracleAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDiagnosticV1>,
}

impl OracleAdmissionFeedbackV1 {
    /// Returns the exact rejected/insufficient proposal revision.
    #[must_use]
    pub const fn proposal_revision(&self) -> ContentId<OracleProposalRevisionArtifact> {
        self.proposal_revision
    }

    /// Returns canonical role-scoped diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[OracleDiagnosticV1] {
        &self.diagnostics
    }

    fn validate_shape(&self) -> Result<(), OracleWorkflowError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleWorkflowError::UnsupportedSchema(self.schema_version));
        }
        if self.diagnostics.is_empty()
            || self
                .diagnostics
                .windows(2)
                .any(|pair| diagnostic_key(&pair[0]) >= diagnostic_key(&pair[1]))
        {
            return Err(OracleWorkflowError::InvalidDiagnostics);
        }
        Ok(())
    }
}

fn diagnostic_key(
    diagnostic: &OracleDiagnosticV1,
) -> (OracleFeedbackTarget, OracleDiagnosticKind, String) {
    (
        diagnostic.target,
        diagnostic.kind,
        diagnostic.evidence.to_wire(),
    )
}

impl TryFrom<OracleAdmissionFeedbackWire> for OracleAdmissionFeedbackV1 {
    type Error = OracleWorkflowError;

    fn try_from(wire: OracleAdmissionFeedbackWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_plan: wire.search_plan,
            proposal_revision: wire.proposal_revision,
            attack: wire.attack,
            admission_attempt: wire.admission_attempt,
            diagnostics: wire.diagnostics,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleAdmissionFeedbackV1> for OracleAdmissionFeedbackWire {
    fn from(value: OracleAdmissionFeedbackV1) -> Self {
        Self {
            schema_version: value.schema_version,
            search_plan: value.search_plan,
            proposal_revision: value.proposal_revision,
            attack: value.attack,
            admission_attempt: value.admission_attempt,
            diagnostics: value.diagnostics,
        }
    }
}

/// Privately prepared feedback with recomputed lineage and identity.
#[derive(Clone, Debug)]
pub struct PreparedOracleAdmissionFeedback {
    body: OracleAdmissionFeedbackV1,
    id: ContentId<OracleAdmissionFeedbackArtifact>,
}

impl PreparedOracleAdmissionFeedback {
    /// Returns the strict feedback body.
    #[must_use]
    pub const fn body(&self) -> &OracleAdmissionFeedbackV1 {
        &self.body
    }

    /// Returns the recomputed feedback identity.
    #[must_use]
    pub const fn id(&self) -> ContentId<OracleAdmissionFeedbackArtifact> {
        self.id
    }
}

/// Builds one trusted admission feedback bundle over exact proposal and attack identities.
///
/// # Errors
///
/// Rejects empty, duplicated, unsorted diagnostics or any plan/revision/attack lineage mismatch.
pub fn prepare_oracle_admission_feedback(
    plan: &OracleSearchPlanV1,
    revision: &PreparedOracleProposalRevision,
    attack: &PreparedOracleAttack,
    admission_attempt: ContentId<OracleAdmissionAttemptArtifact>,
    diagnostics: Vec<OracleDiagnosticV1>,
) -> Result<PreparedOracleAdmissionFeedback, OracleWorkflowError> {
    let search_plan = content_id::<OracleSearchPlanArtifact>(plan)?;
    if revision.body.search_plan != search_plan
        || attack.body.search_plan != search_plan
        || attack.body.proposal_revision != revision.id
    {
        return Err(OracleWorkflowError::FeedbackLineageMismatch);
    }
    let body = OracleAdmissionFeedbackV1 {
        schema_version: SCHEMA_V1,
        search_plan,
        proposal_revision: revision.id,
        attack: attack.id,
        admission_attempt,
        diagnostics,
    };
    body.validate_shape()?;
    let id = content_id::<OracleAdmissionFeedbackArtifact>(&body)?;
    Ok(PreparedOracleAdmissionFeedback { body, id })
}

fn validate_canonical_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleWorkflowError> {
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(OracleWorkflowError::NonCanonicalSet { field });
    }
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value.to_wire())) {
        return Err(OracleWorkflowError::NonCanonicalSet { field });
    }
    Ok(())
}

fn content_id<T: ContentType>(value: &impl Serialize) -> Result<ContentId<T>, OracleWorkflowError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| OracleWorkflowError::Encoding(error.to_string()))?;
    ContentId::derive(&bytes).map_err(|error| OracleWorkflowError::Encoding(error.to_string()))
}

/// Invalid model-authored Oracle Agent workflow composition.
#[derive(Debug, Error)]
pub enum OracleWorkflowError {
    /// A schema other than the current V1 was supplied.
    #[error("unsupported Oracle Agent workflow schema version {0}")]
    UnsupportedSchema(u16),
    /// Blue proposal disagrees with the plan or its exact model authorship.
    #[error("blue proposal does not match its OracleSearch task, domain, episode, or model")]
    BlueProposalMismatch,
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
    /// Red variant has wrong episode/model authorship.
    #[error("red variant does not match its OracleSearch episode or model")]
    RedVariantMismatch,
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
        OracleAdmissionAttemptArtifact, OracleAttackInput, OracleDiagnosticEvidenceArtifact,
        OracleDiagnosticKind, OracleDiagnosticV1, OracleFeedbackTarget,
        prepare_oracle_admission_feedback, prepare_oracle_attack, prepare_oracle_proposal_revision,
    };
    use crate::{
        OracleAgentRole, OracleRoleEpisodeInput, OracleSearchPlanInput, OracleSearchPlanV1,
        prepare_oracle_role_episode,
    };

    fn id<T: ContentType>(label: &str) -> ContentId<T> {
        ContentId::derive(label.as_bytes()).expect("identity")
    }

    fn plan() -> OracleSearchPlanV1 {
        let blue_episode = EpisodeId::new();
        let red_episode = EpisodeId::new();
        OracleSearchPlanV1::new(OracleSearchPlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller")],
            blue: prepare_oracle_role_episode(OracleRoleEpisodeInput {
                role: OracleAgentRole::Blue,
                episode_id: blue_episode,
                model_configuration: id::<ResolvedRuntimeModelArtifact>("blue-runtime-model"),
                authorship_configuration: id::<ModelConfigurationArtifact>("blue-author-model"),
                role_instruction: id::<InstructionBlock>("blue"),
                private_context: Vec::new(),
                budget: cairn_agent::EpisodeBudget::default(),
            })
            .expect("blue"),
            red: prepare_oracle_role_episode(OracleRoleEpisodeInput {
                role: OracleAgentRole::Red,
                episode_id: red_episode,
                model_configuration: id::<ResolvedRuntimeModelArtifact>("red-runtime-model"),
                authorship_configuration: id::<ModelConfigurationArtifact>("red-author-model"),
                role_instruction: id::<InstructionBlock>("red"),
                private_context: Vec::new(),
                budget: cairn_agent::EpisodeBudget::default(),
            })
            .expect("red"),
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

    fn proposal(plan: &OracleSearchPlanV1, label: &str) -> OracleProposalV1 {
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
                plan.blue().episode_id(),
                plan.blue().authorship_configuration(),
            ),
        })
        .expect("proposal")
    }

    fn correct_variant(plan: &OracleSearchPlanV1, label: &str) -> ImplementationVariantV1 {
        ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>(label),
            VariantExpectation::MustAccept {
                construction_claim: id::<ConstructionClaimArtifact>("construction"),
            },
            authorship(
                plan.red().episode_id(),
                plan.red().authorship_configuration(),
            ),
        )
    }

    fn wrong_variant(plan: &OracleSearchPlanV1, label: &str) -> ImplementationVariantV1 {
        ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>(label),
            VariantExpectation::MustReject {
                fault_class: FaultClassName::new("zero-output").expect("fault"),
                fault_evidence: id::<FaultInjectionEvidenceArtifact>("fault-evidence"),
            },
            authorship(
                plan.red().episode_id(),
                plan.red().authorship_configuration(),
            ),
        )
    }

    #[test]
    fn isolated_model_authorship_drives_revision_attack_and_feedback() {
        let plan = plan();
        let first = prepare_oracle_proposal_revision(
            &plan,
            None,
            proposal(&plan, "refinement-v1"),
            Vec::new(),
        )
        .expect("first proposal");
        let attack = prepare_oracle_attack(
            &plan,
            &first,
            OracleAttackInput {
                correct_variants: vec![correct_variant(&plan, "correct")],
                wrong_variants: vec![wrong_variant(&plan, "wrong")],
                adversarial_cases: vec![id::<CorpusCaseArtifact>("adversarial")],
            },
        )
        .expect("attack");
        let feedback = prepare_oracle_admission_feedback(
            &plan,
            &first,
            &attack,
            id::<OracleAdmissionAttemptArtifact>("attempt"),
            vec![OracleDiagnosticV1 {
                target: OracleFeedbackTarget::Blue,
                kind: OracleDiagnosticKind::FalseAccept,
                evidence: id::<OracleDiagnosticEvidenceArtifact>("diagnostic"),
            }],
        )
        .expect("feedback");
        assert_eq!(feedback.body().proposal_revision(), first.id());

        let second = prepare_oracle_proposal_revision(
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
    fn blue_and_red_authorship_cannot_be_swapped() {
        let plan = plan();
        let mut wrong_blue = proposal(&plan, "refinement");
        let mut value = serde_json::to_value(&wrong_blue).expect("proposal JSON");
        value["authorship"] = serde_json::to_value(authorship(
            plan.red().episode_id(),
            plan.red().authorship_configuration(),
        ))
        .expect("red authorship");
        wrong_blue = serde_json::from_value(value).expect("structurally valid proposal");
        assert!(prepare_oracle_proposal_revision(&plan, None, wrong_blue, Vec::new()).is_err());

        let first = prepare_oracle_proposal_revision(
            &plan,
            None,
            proposal(&plan, "refinement"),
            Vec::new(),
        )
        .expect("first");
        let blue_authored_variant = ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>("wrong-author"),
            VariantExpectation::MustAccept {
                construction_claim: id::<ConstructionClaimArtifact>("construction"),
            },
            authorship(
                plan.blue().episode_id(),
                plan.blue().authorship_configuration(),
            ),
        );
        assert!(
            prepare_oracle_attack(
                &plan,
                &first,
                OracleAttackInput {
                    correct_variants: vec![blue_authored_variant],
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
        let first =
            prepare_oracle_proposal_revision(&plan, None, proposal(&plan, "same"), Vec::new())
                .expect("first");
        assert!(
            prepare_oracle_proposal_revision(
                &plan,
                Some(&first),
                proposal(&plan, "same"),
                Vec::new(),
            )
            .is_err()
        );
    }
}
