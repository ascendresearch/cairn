//! Task-generic Candidate authority derived only from admitted Oracle claims.

use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
};

use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use cairn_verification::{
    CorpusCaseArtifact, DomainRefinementArtifact, ObservationPlanArtifact,
    PropertyRelationArtifact, ReferenceArtifact, SourceAdmissionPlanArtifact,
    ValidFamilyPlanArtifact,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, IntentRecoveryInputArtifact,
    MigrationIntentContractArtifact, OracleAdmissionOutcomeArtifact, OracleAdmissionOutcomeV1,
    OracleBuildTestSnapshotArtifact, OracleClaimAdmissionStatusV1, OracleClaimArtifact,
    OracleClaimV1, OracleComparatorProposalArtifact, OracleDocumentationSnapshotArtifact,
    OracleExecutionSafetyProposalArtifact, OracleKnowledgeSnapshotArtifact,
    OracleObligationEntryV1, OracleObligationResolutionV1, OraclePortfolioElementKindV1,
    OraclePortfolioElementV1, OraclePortfolioProposalArtifact, OraclePortfolioProposalV1,
    OracleWorkspaceArtifact, OracleWorkspaceV1, SirTaskBundleArtifact,
};

const SCHEMA_V1: u16 = 1;
const MAX_CANDIDATE_FILES: usize = 32;
const MAX_CANDIDATE_SOURCE_BYTES: usize = 512 * 1024;
const MAX_CANDIDATE_ORACLE_MATERIAL_BYTES: usize = 2 * 1024 * 1024;

/// Exact admitted Oracle subset visible to Candidate Search.
pub enum CandidateOracleContractArtifact {}

impl ContentType for CandidateOracleContractArtifact {
    const DOMAIN: &'static str = "migration.candidate-oracle-contract.v1";
}

/// Exact public task/material authority frozen for one Candidate Proposal Loop.
pub enum CandidateWorkspaceArtifact {}

impl ContentType for CandidateWorkspaceArtifact {
    const DOMAIN: &'static str = "migration.candidate-workspace.v1";
}

/// Task-generic Candidate workspace derived from admitted upstream artifacts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateWorkspaceV1 {
    schema_version: u16,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    oracle_workspace: ContentId<OracleWorkspaceArtifact>,
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    documentation: ContentId<OracleDocumentationSnapshotArtifact>,
    build_and_tests: ContentId<OracleBuildTestSnapshotArtifact>,
    knowledge: ContentId<OracleKnowledgeSnapshotArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateWorkspaceWire {
    schema_version: u16,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    oracle_workspace: ContentId<OracleWorkspaceArtifact>,
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    documentation: ContentId<OracleDocumentationSnapshotArtifact>,
    build_and_tests: ContentId<OracleBuildTestSnapshotArtifact>,
    knowledge: ContentId<OracleKnowledgeSnapshotArtifact>,
}

impl CandidateWorkspaceV1 {
    /// Derives the exact Candidate workspace from the admitted Oracle workspace and contract.
    ///
    /// # Errors
    ///
    /// Rejects a portfolio/workspace/contract identity mismatch.
    pub fn derive(
        oracle_workspace: &OracleWorkspaceV1,
        proposal: &OraclePortfolioProposalV1,
        contract: &CandidateOracleContractV1,
    ) -> Result<Self, CandidateExplorationError> {
        let workspace_id = oracle_workspace.identity().map_err(codec)?;
        if proposal.workspace() != workspace_id
            || contract.proposal() != proposal.identity().map_err(codec)?
        {
            return Err(CandidateExplorationError::BindingMismatch);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            task_id: oracle_workspace.task_id(),
            recovery_input: oracle_workspace.sir_input(),
            admitted_intent: oracle_workspace.admitted_intent(),
            oracle_workspace: workspace_id,
            oracle_contract: contract.identity()?,
            task_bundle: oracle_workspace.sir_task_bundle(),
            documentation: oracle_workspace.documentation(),
            build_and_tests: oracle_workspace.build_and_tests(),
            knowledge: oracle_workspace.knowledge(),
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }

    #[must_use]
    pub const fn admitted_intent(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.admitted_intent
    }

    #[must_use]
    pub const fn oracle_workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.oracle_workspace
    }

    #[must_use]
    pub const fn oracle_contract(&self) -> ContentId<CandidateOracleContractArtifact> {
        self.oracle_contract
    }

    #[must_use]
    pub const fn task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.task_bundle
    }

    #[must_use]
    pub const fn documentation(&self) -> ContentId<OracleDocumentationSnapshotArtifact> {
        self.documentation
    }

    #[must_use]
    pub const fn build_and_tests(&self) -> ContentId<OracleBuildTestSnapshotArtifact> {
        self.build_and_tests
    }

    #[must_use]
    pub const fn knowledge(&self) -> ContentId<OracleKnowledgeSnapshotArtifact> {
        self.knowledge
    }

    /// Derives the exact typed workspace identity.
    ///
    /// # Errors
    ///
    /// Rejects non-V1 or unencodable workspace material.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateWorkspaceArtifact>, CandidateExplorationError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateExplorationError::WorkspaceDrift);
        }
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }
}

impl TryFrom<CandidateWorkspaceWire> for CandidateWorkspaceV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateWorkspaceWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            recovery_input: wire.recovery_input,
            admitted_intent: wire.admitted_intent,
            oracle_workspace: wire.oracle_workspace,
            oracle_contract: wire.oracle_contract,
            task_bundle: wire.task_bundle,
            documentation: wire.documentation,
            build_and_tests: wire.build_and_tests,
            knowledge: wire.knowledge,
        };
        let _ = value.identity()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateWorkspaceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateWorkspaceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Immutable task-generic Candidate source proposal.
///
/// A Candidate proposal cannot be substituted for its admitted Oracle authority.
///
/// ```compile_fail
/// use cairn_migration::{CandidateOracleContractArtifact, CandidateProposalArtifact};
/// use cairn_protocol::ContentId;
/// fn require_contract(_: ContentId<CandidateOracleContractArtifact>) {}
/// fn invalid(proposal: ContentId<CandidateProposalArtifact>) {
///     require_contract(proposal);
/// }
/// ```
pub enum CandidateProposalArtifact {}

impl ContentType for CandidateProposalArtifact {
    const DOMAIN: &'static str = "migration.candidate-proposal.v1";
}

macro_rules! candidate_text {
    ($name:ident, $field:literal, $maximum:expr) => {
        #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates one bounded non-empty current-V1 Candidate value.
            ///
            /// # Errors
            ///
            /// Rejects blank, oversized, NUL-containing, or carriage-return-containing text.
            pub fn new(value: impl Into<String>) -> Result<Self, CandidateExplorationError> {
                let value = value.into();
                if value.trim().is_empty()
                    || value.len() > $maximum
                    || value.contains('\0')
                    || value.contains('\r')
                {
                    return Err(CandidateExplorationError::InvalidValue($field));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

candidate_text!(CandidateSourceText, "Candidate source text", 256 * 1024);
candidate_text!(CandidateExplanation, "Candidate explanation", 16 * 1024);

/// Canonical Candidate-relative source path, distinct from task source paths.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CandidateSourcePath(String);

impl CandidateSourcePath {
    /// Creates one canonical relative Candidate path.
    ///
    /// # Errors
    ///
    /// Rejects empty, absolute, traversing, backslash, control-containing, or oversized paths.
    pub fn new(value: impl Into<String>) -> Result<Self, CandidateExplorationError> {
        let value = value.into();
        let path = Path::new(&value);
        if value.is_empty()
            || value.len() > 512
            || value.trim() != value
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(CandidateExplorationError::InvalidValue(
                "Candidate source path",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CandidateSourcePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// One complete source file in an immutable Candidate proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSourceFileV1 {
    path: CandidateSourcePath,
    source: CandidateSourceText,
}

impl CandidateSourceFileV1 {
    #[must_use]
    pub const fn path(&self) -> &CandidateSourcePath {
        &self.path
    }

    #[must_use]
    pub const fn source(&self) -> &CandidateSourceText {
        &self.source
    }
}

/// Strict model-authored source tree without trusted authority or provenance fields.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateProposalSubmissionV1 {
    schema_version: u16,
    files: Vec<CandidateSourceFileV1>,
    primary_source: CandidateSourcePath,
    explanation: CandidateExplanation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateProposalSubmissionWire {
    schema_version: u16,
    files: Vec<CandidateSourceFileV1>,
    primary_source: CandidateSourcePath,
    explanation: CandidateExplanation,
}

impl CandidateProposalSubmissionV1 {
    #[must_use]
    pub fn files(&self) -> &[CandidateSourceFileV1] {
        &self.files
    }

    #[must_use]
    pub const fn primary_source(&self) -> &CandidateSourcePath {
        &self.primary_source
    }

    #[must_use]
    pub const fn explanation(&self) -> &CandidateExplanation {
        &self.explanation
    }

    fn validate(&self) -> Result<(), CandidateExplorationError> {
        if self.schema_version != SCHEMA_V1
            || self.files.is_empty()
            || self.files.len() > MAX_CANDIDATE_FILES
            || self
                .files
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
            || !self
                .files
                .iter()
                .any(|file| file.path == self.primary_source)
        {
            return Err(CandidateExplorationError::ProposalDrift);
        }
        let total = self.files.iter().try_fold(0_usize, |total, file| {
            total
                .checked_add(file.source.0.len())
                .ok_or(CandidateExplorationError::ProposalDrift)
        })?;
        if total > MAX_CANDIDATE_SOURCE_BYTES {
            return Err(CandidateExplorationError::ProposalDrift);
        }
        Ok(())
    }
}

impl TryFrom<CandidateProposalSubmissionWire> for CandidateProposalSubmissionV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateProposalSubmissionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            files: wire.files,
            primary_source: wire.primary_source,
            explanation: wire.explanation,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateProposalSubmissionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateProposalSubmissionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Immutable non-authoritative Candidate proposal with trusted Host provenance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateProposalV1 {
    schema_version: u16,
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    submission: CandidateProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateProposalWire {
    schema_version: u16,
    oracle_contract: ContentId<CandidateOracleContractArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    submission: CandidateProposalSubmissionV1,
}

impl CandidateProposalV1 {
    /// Binds one validated model submission to trusted Candidate authority and provenance.
    ///
    /// # Errors
    ///
    /// Rejects a noncanonical or invalid source-tree submission.
    pub fn new(
        oracle_contract: ContentId<CandidateOracleContractArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        submission: CandidateProposalSubmissionV1,
    ) -> Result<Self, CandidateExplorationError> {
        submission.validate()?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            oracle_contract,
            episode_id,
            model_configuration,
            submission,
        })
    }

    #[must_use]
    pub const fn oracle_contract(&self) -> ContentId<CandidateOracleContractArtifact> {
        self.oracle_contract
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn model_configuration(&self) -> ContentId<AgentResolvedRuntimeModelArtifact> {
        self.model_configuration
    }

    #[must_use]
    pub const fn submission(&self) -> &CandidateProposalSubmissionV1 {
        &self.submission
    }

    /// Derives the exact typed identity after revalidating the complete source tree.
    ///
    /// # Errors
    ///
    /// Rejects non-V1, noncanonical, invalid, or unencodable proposal material.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateProposalArtifact>, CandidateExplorationError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateExplorationError::ProposalDrift);
        }
        self.submission.validate()?;
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }
}

impl TryFrom<CandidateProposalWire> for CandidateProposalV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateProposalWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            oracle_contract: wire.oracle_contract,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        };
        let _ = value.identity()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One admitted claim and every independently admitted work item that defines its Oracle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateAdmittedOracleClaimV1 {
    claim: ContentId<OracleClaimArtifact>,
    entries: Vec<OracleObligationEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateAdmittedOracleClaimWire {
    claim: ContentId<OracleClaimArtifact>,
    entries: Vec<OracleObligationEntryV1>,
}

impl CandidateAdmittedOracleClaimV1 {
    #[must_use]
    pub const fn claim(&self) -> ContentId<OracleClaimArtifact> {
        self.claim
    }

    #[must_use]
    pub fn entries(&self) -> &[OracleObligationEntryV1] {
        &self.entries
    }

    fn validate(&self) -> Result<(), CandidateExplorationError> {
        if self.entries.is_empty() {
            return Err(CandidateExplorationError::ContractDrift);
        }
        let mut prior = None;
        for entry in &self.entries {
            let OracleObligationResolutionV1::Contributed { elements, .. } = entry.resolution()
            else {
                return Err(CandidateExplorationError::ContractDrift);
            };
            if elements.is_empty() || entry.item().claim() != self.claim {
                return Err(CandidateExplorationError::ContractDrift);
            }
            let identity = entry.item().identity().map_err(codec)?;
            if prior.is_some_and(|prior: String| prior >= identity.to_wire()) {
                return Err(CandidateExplorationError::ContractDrift);
            }
            prior = Some(identity.to_wire());
        }
        Ok(())
    }
}

impl TryFrom<CandidateAdmittedOracleClaimWire> for CandidateAdmittedOracleClaimV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateAdmittedOracleClaimWire) -> Result<Self, Self::Error> {
        let value = Self {
            claim: wire.claim,
            entries: wire.entries,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateAdmittedOracleClaimV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateAdmittedOracleClaimWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Candidate-visible Oracle authority; partial and rejected claims are absent by construction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateOracleContractV1 {
    schema_version: u16,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    admitted_claims: Vec<CandidateAdmittedOracleClaimV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateOracleContractWire {
    schema_version: u16,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    outcome: ContentId<OracleAdmissionOutcomeArtifact>,
    admitted_claims: Vec<CandidateAdmittedOracleClaimV1>,
}

impl CandidateOracleContractV1 {
    /// Projects only admitted claims from one exact independently recomputed Oracle outcome.
    ///
    /// # Errors
    ///
    /// Rejects proposal/outcome drift, an inconsistent item partition, or an outcome with no
    /// admitted claim. Partial and rejected work cannot become Candidate authority.
    pub fn derive(
        proposal: &OraclePortfolioProposalV1,
        outcome: &OracleAdmissionOutcomeV1,
    ) -> Result<Self, CandidateExplorationError> {
        let proposal_id = proposal.identity().map_err(codec)?;
        if outcome.proposal() != proposal_id {
            return Err(CandidateExplorationError::BindingMismatch);
        }
        let mut entries_by_id = proposal
            .entries()
            .iter()
            .map(|entry| Ok((entry.item().identity().map_err(codec)?, entry.clone())))
            .collect::<Result<HashMap<_, _>, CandidateExplorationError>>()?;
        let mut admitted_claims = Vec::new();
        for claim in outcome.claims() {
            if claim.status() != OracleClaimAdmissionStatusV1::Admitted {
                continue;
            }
            if !claim.unresolved_items().is_empty() || !claim.rejected_items().is_empty() {
                return Err(CandidateExplorationError::BindingMismatch);
            }
            let entries = claim
                .admitted_items()
                .iter()
                .map(|item| {
                    entries_by_id
                        .remove(item)
                        .ok_or(CandidateExplorationError::BindingMismatch)
                })
                .collect::<Result<Vec<_>, _>>()?;
            admitted_claims.push(CandidateAdmittedOracleClaimV1 {
                claim: claim.claim(),
                entries,
            });
        }
        admitted_claims.sort_by_key(|claim| claim.claim.to_wire());
        let value = Self {
            schema_version: SCHEMA_V1,
            proposal: proposal_id,
            outcome: outcome.identity().map_err(codec)?,
            admitted_claims,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn outcome(&self) -> ContentId<OracleAdmissionOutcomeArtifact> {
        self.outcome
    }

    #[must_use]
    pub fn admitted_claims(&self) -> &[CandidateAdmittedOracleClaimV1] {
        &self.admitted_claims
    }

    /// Derives the typed Candidate Oracle contract identity.
    ///
    /// # Errors
    ///
    /// Rejects empty, noncanonical, duplicated, or unencodable admitted authority.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateOracleContractArtifact>, CandidateExplorationError> {
        self.validate()?;
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }

    fn validate(&self) -> Result<(), CandidateExplorationError> {
        if self.schema_version != SCHEMA_V1 || self.admitted_claims.is_empty() {
            return Err(CandidateExplorationError::NoAdmittedOracleClaims);
        }
        if self
            .admitted_claims
            .windows(2)
            .any(|pair| pair[0].claim.to_wire() >= pair[1].claim.to_wire())
        {
            return Err(CandidateExplorationError::ContractDrift);
        }
        let mut items = HashSet::new();
        for claim in &self.admitted_claims {
            claim.validate()?;
            for entry in &claim.entries {
                if !items.insert(entry.item().identity().map_err(codec)?) {
                    return Err(CandidateExplorationError::ContractDrift);
                }
            }
        }
        Ok(())
    }
}

impl TryFrom<CandidateOracleContractWire> for CandidateOracleContractV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateOracleContractWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            proposal: wire.proposal,
            outcome: wire.outcome,
            admitted_claims: wire.admitted_claims,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateOracleContractV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateOracleContractWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact public body of one typed Oracle artifact admitted for Candidate use.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CandidateOracleMaterialV1 {
    DomainRefinement {
        identity: ContentId<DomainRefinementArtifact>,
        bytes: Vec<u8>,
    },
    CorpusCase {
        identity: ContentId<CorpusCaseArtifact>,
        bytes: Vec<u8>,
    },
    Reference {
        identity: ContentId<ReferenceArtifact>,
        bytes: Vec<u8>,
    },
    PropertyRelation {
        identity: ContentId<PropertyRelationArtifact>,
        bytes: Vec<u8>,
    },
    SourceAdmissionPlan {
        identity: ContentId<SourceAdmissionPlanArtifact>,
        bytes: Vec<u8>,
    },
    ValidFamilyPlan {
        identity: ContentId<ValidFamilyPlanArtifact>,
        bytes: Vec<u8>,
    },
    ObservationPlan {
        identity: ContentId<ObservationPlanArtifact>,
        bytes: Vec<u8>,
    },
    Comparator {
        identity: ContentId<OracleComparatorProposalArtifact>,
        bytes: Vec<u8>,
    },
    ExecutionSafety {
        identity: ContentId<OracleExecutionSafetyProposalArtifact>,
        bytes: Vec<u8>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum CandidateOracleMaterialWire {
    DomainRefinement {
        identity: ContentId<DomainRefinementArtifact>,
        bytes: Vec<u8>,
    },
    CorpusCase {
        identity: ContentId<CorpusCaseArtifact>,
        bytes: Vec<u8>,
    },
    Reference {
        identity: ContentId<ReferenceArtifact>,
        bytes: Vec<u8>,
    },
    PropertyRelation {
        identity: ContentId<PropertyRelationArtifact>,
        bytes: Vec<u8>,
    },
    SourceAdmissionPlan {
        identity: ContentId<SourceAdmissionPlanArtifact>,
        bytes: Vec<u8>,
    },
    ValidFamilyPlan {
        identity: ContentId<ValidFamilyPlanArtifact>,
        bytes: Vec<u8>,
    },
    ObservationPlan {
        identity: ContentId<ObservationPlanArtifact>,
        bytes: Vec<u8>,
    },
    Comparator {
        identity: ContentId<OracleComparatorProposalArtifact>,
        bytes: Vec<u8>,
    },
    ExecutionSafety {
        identity: ContentId<OracleExecutionSafetyProposalArtifact>,
        bytes: Vec<u8>,
    },
}

impl CandidateOracleMaterialV1 {
    /// Binds exact CAS bytes to the semantic material type carried by a portfolio element.
    ///
    /// # Errors
    ///
    /// Rejects empty/oversized bytes or a typed content-identity mismatch.
    pub fn from_portfolio_kind(
        kind: &OraclePortfolioElementKindV1,
        bytes: Vec<u8>,
    ) -> Result<Self, CandidateExplorationError> {
        if bytes.is_empty() || bytes.len() > MAX_CANDIDATE_ORACLE_MATERIAL_BYTES {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let value = match kind {
            OraclePortfolioElementKindV1::DomainRefinement(identity) => Self::DomainRefinement {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::CorpusCase(identity) => Self::CorpusCase {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::Reference(identity) => Self::Reference {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::PropertyRelation(identity) => Self::PropertyRelation {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::SourceAdmissionPlan(identity) => {
                Self::SourceAdmissionPlan {
                    identity: *identity,
                    bytes,
                }
            }
            OraclePortfolioElementKindV1::ValidFamilyPlan(identity) => Self::ValidFamilyPlan {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::ObservationPlan(identity) => Self::ObservationPlan {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::Comparator(identity) => Self::Comparator {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::ExecutionSafety(identity) => Self::ExecutionSafety {
                identity: *identity,
                bytes,
            },
            OraclePortfolioElementKindV1::CoverageGap(_) => {
                return Err(CandidateExplorationError::MaterialDrift);
            }
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::DomainRefinement { bytes, .. }
            | Self::CorpusCase { bytes, .. }
            | Self::Reference { bytes, .. }
            | Self::PropertyRelation { bytes, .. }
            | Self::SourceAdmissionPlan { bytes, .. }
            | Self::ValidFamilyPlan { bytes, .. }
            | Self::ObservationPlan { bytes, .. }
            | Self::Comparator { bytes, .. }
            | Self::ExecutionSafety { bytes, .. } => bytes,
        }
    }

    fn validates_kind(&self, kind: &OraclePortfolioElementKindV1) -> bool {
        matches!(
            (self, kind),
            (
                Self::DomainRefinement { identity: left, .. },
                OraclePortfolioElementKindV1::DomainRefinement(right)
            ) if left == right
        ) || matches!(
            (self, kind),
            (Self::CorpusCase { identity: left, .. }, OraclePortfolioElementKindV1::CorpusCase(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::Reference { identity: left, .. }, OraclePortfolioElementKindV1::Reference(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::PropertyRelation { identity: left, .. }, OraclePortfolioElementKindV1::PropertyRelation(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::SourceAdmissionPlan { identity: left, .. }, OraclePortfolioElementKindV1::SourceAdmissionPlan(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::ValidFamilyPlan { identity: left, .. }, OraclePortfolioElementKindV1::ValidFamilyPlan(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::ObservationPlan { identity: left, .. }, OraclePortfolioElementKindV1::ObservationPlan(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::Comparator { identity: left, .. }, OraclePortfolioElementKindV1::Comparator(right)) if left == right
        ) || matches!(
            (self, kind),
            (Self::ExecutionSafety { identity: left, .. }, OraclePortfolioElementKindV1::ExecutionSafety(right)) if left == right
        )
    }

    fn validate(&self) -> Result<(), CandidateExplorationError> {
        let bytes = self.bytes();
        if bytes.is_empty() || bytes.len() > MAX_CANDIDATE_ORACLE_MATERIAL_BYTES {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let valid = match self {
            Self::DomainRefinement { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::CorpusCase { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::Reference { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::PropertyRelation { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::SourceAdmissionPlan { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::ValidFamilyPlan { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::ObservationPlan { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::Comparator { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
            Self::ExecutionSafety { identity, .. } => ContentId::derive(bytes) == Ok(*identity),
        };
        if valid {
            Ok(())
        } else {
            Err(CandidateExplorationError::MaterialDrift)
        }
    }
}

impl TryFrom<CandidateOracleMaterialWire> for CandidateOracleMaterialV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateOracleMaterialWire) -> Result<Self, Self::Error> {
        let value = match wire {
            CandidateOracleMaterialWire::DomainRefinement { identity, bytes } => {
                Self::DomainRefinement { identity, bytes }
            }
            CandidateOracleMaterialWire::CorpusCase { identity, bytes } => {
                Self::CorpusCase { identity, bytes }
            }
            CandidateOracleMaterialWire::Reference { identity, bytes } => {
                Self::Reference { identity, bytes }
            }
            CandidateOracleMaterialWire::PropertyRelation { identity, bytes } => {
                Self::PropertyRelation { identity, bytes }
            }
            CandidateOracleMaterialWire::SourceAdmissionPlan { identity, bytes } => {
                Self::SourceAdmissionPlan { identity, bytes }
            }
            CandidateOracleMaterialWire::ValidFamilyPlan { identity, bytes } => {
                Self::ValidFamilyPlan { identity, bytes }
            }
            CandidateOracleMaterialWire::ObservationPlan { identity, bytes } => {
                Self::ObservationPlan { identity, bytes }
            }
            CandidateOracleMaterialWire::Comparator { identity, bytes } => {
                Self::Comparator { identity, bytes }
            }
            CandidateOracleMaterialWire::ExecutionSafety { identity, bytes } => {
                Self::ExecutionSafety { identity, bytes }
            }
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateOracleMaterialV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateOracleMaterialWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One admitted portfolio element paired with its exact typed public artifact body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateOracleElementMaterialV1 {
    element: OraclePortfolioElementV1,
    material: CandidateOracleMaterialV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateOracleElementMaterialWire {
    element: OraclePortfolioElementV1,
    material: CandidateOracleMaterialV1,
}

impl CandidateOracleElementMaterialV1 {
    /// Pairs one portfolio element with the body named by its exact typed material edge.
    ///
    /// # Errors
    ///
    /// Rejects a material kind or identity that differs from the portfolio element.
    pub fn new(
        element: OraclePortfolioElementV1,
        material: CandidateOracleMaterialV1,
    ) -> Result<Self, CandidateExplorationError> {
        if !material.validates_kind(element.kind()) {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        Ok(Self { element, material })
    }

    #[must_use]
    pub const fn element(&self) -> &OraclePortfolioElementV1 {
        &self.element
    }

    #[must_use]
    pub const fn material(&self) -> &CandidateOracleMaterialV1 {
        &self.material
    }

    fn validate(&self) -> Result<(), CandidateExplorationError> {
        self.material.validate()?;
        if self.material.validates_kind(self.element.kind()) {
            Ok(())
        } else {
            Err(CandidateExplorationError::MaterialDrift)
        }
    }
}

impl TryFrom<CandidateOracleElementMaterialWire> for CandidateOracleElementMaterialV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateOracleElementMaterialWire) -> Result<Self, Self::Error> {
        Self::new(wire.element, wire.material)
    }
}

impl<'de> Deserialize<'de> for CandidateOracleElementMaterialV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateOracleElementMaterialWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Complete Candidate-visible Oracle bodies, mechanically checked against the admitted contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateOracleMaterialsV1 {
    schema_version: u16,
    claims: Vec<OracleClaimV1>,
    elements: Vec<CandidateOracleElementMaterialV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateOracleMaterialsWire {
    schema_version: u16,
    claims: Vec<OracleClaimV1>,
    elements: Vec<CandidateOracleElementMaterialV1>,
}

impl CandidateOracleMaterialsV1 {
    /// Freezes the exact public claim and Oracle material bodies admitted for Candidate use.
    ///
    /// # Errors
    ///
    /// Rejects missing, extra, duplicated, noncanonical, cross-cell, or body-drifted material.
    pub fn new(
        contract: &CandidateOracleContractV1,
        claims: Vec<OracleClaimV1>,
        elements: Vec<CandidateOracleElementMaterialV1>,
    ) -> Result<Self, CandidateExplorationError> {
        let mut claims = claims
            .into_iter()
            .map(|claim| Ok((claim.identity().map_err(codec)?.to_wire(), claim)))
            .collect::<Result<Vec<_>, CandidateExplorationError>>()?;
        claims.sort_by(|left, right| left.0.cmp(&right.0));
        let claims = claims.into_iter().map(|(_, claim)| claim).collect();
        let mut elements = elements
            .into_iter()
            .map(|element| {
                Ok((
                    element.element.identity().map_err(codec)?.to_wire(),
                    element,
                ))
            })
            .collect::<Result<Vec<_>, CandidateExplorationError>>()?;
        elements.sort_by(|left, right| left.0.cmp(&right.0));
        let elements = elements.into_iter().map(|(_, element)| element).collect();
        let value = Self {
            schema_version: SCHEMA_V1,
            claims,
            elements,
        };
        value.validate_against(contract)?;
        Ok(value)
    }

    #[must_use]
    pub fn claims(&self) -> &[OracleClaimV1] {
        &self.claims
    }

    #[must_use]
    pub fn elements(&self) -> &[CandidateOracleElementMaterialV1] {
        &self.elements
    }

    /// Recomputes the exact claim, work-item, element, kind and body bindings.
    ///
    /// # Errors
    ///
    /// Rejects any difference from the complete admitted contract projection.
    pub fn validate_against(
        &self,
        contract: &CandidateOracleContractV1,
    ) -> Result<(), CandidateExplorationError> {
        self.validate_structure()?;
        let expected_claims = contract
            .admitted_claims
            .iter()
            .map(|claim| claim.claim)
            .collect::<Vec<_>>();
        let actual_claims = self
            .claims
            .iter()
            .map(|claim| claim.identity().map_err(codec))
            .collect::<Result<Vec<_>, _>>()?;
        if actual_claims != expected_claims {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let mut expected_elements = HashMap::new();
        for claim in &contract.admitted_claims {
            for entry in &claim.entries {
                let item = entry.item().identity().map_err(codec)?;
                let OracleObligationResolutionV1::Contributed { elements, .. } = entry.resolution()
                else {
                    return Err(CandidateExplorationError::MaterialDrift);
                };
                for element in elements {
                    if expected_elements.insert(*element, item).is_some() {
                        return Err(CandidateExplorationError::MaterialDrift);
                    }
                }
            }
        }
        if self.elements.len() != expected_elements.len() {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let mut prior = None;
        for value in &self.elements {
            value.validate()?;
            let element = value.element.identity().map_err(codec)?;
            if prior
                .as_ref()
                .is_some_and(|prior: &String| prior >= &element.to_wire())
                || expected_elements.remove(&element) != Some(value.element.item())
            {
                return Err(CandidateExplorationError::MaterialDrift);
            }
            prior = Some(element.to_wire());
        }
        if expected_elements.is_empty() {
            Ok(())
        } else {
            Err(CandidateExplorationError::MaterialDrift)
        }
    }

    fn validate_structure(&self) -> Result<(), CandidateExplorationError> {
        if self.schema_version != SCHEMA_V1 || self.claims.is_empty() || self.elements.is_empty() {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let claim_ids = self
            .claims
            .iter()
            .map(|claim| claim.identity().map_err(codec))
            .collect::<Result<Vec<_>, _>>()?;
        if claim_ids
            .windows(2)
            .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
        {
            return Err(CandidateExplorationError::MaterialDrift);
        }
        let mut prior = None;
        for element in &self.elements {
            element.validate()?;
            let identity = element.element.identity().map_err(codec)?.to_wire();
            if prior
                .as_ref()
                .is_some_and(|prior: &String| prior >= &identity)
            {
                return Err(CandidateExplorationError::MaterialDrift);
            }
            prior = Some(identity);
        }
        Ok(())
    }
}

impl TryFrom<CandidateOracleMaterialsWire> for CandidateOracleMaterialsV1 {
    type Error = CandidateExplorationError;

    fn try_from(wire: CandidateOracleMaterialsWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            claims: wire.claims,
            elements: wire.elements,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateOracleMaterialsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CandidateOracleMaterialsWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CandidateExplorationError {
    #[error("invalid Candidate value: {0}")]
    InvalidValue(&'static str),
    #[error("Candidate authority does not match the admitted Oracle proposal and outcome")]
    BindingMismatch,
    #[error("Candidate cannot start because the Oracle outcome has no admitted claims")]
    NoAdmittedOracleClaims,
    #[error("Candidate Oracle contract is noncanonical or changed")]
    ContractDrift,
    #[error("Candidate proposal is noncanonical or changed")]
    ProposalDrift,
    #[error("Candidate workspace is non-V1 or changed")]
    WorkspaceDrift,
    #[error("Candidate-visible Oracle material changed type, identity, body, or admitted scope")]
    MaterialDrift,
    #[error("Candidate Oracle contract codec failed: {0}")]
    Codec(String),
}

fn codec(error: impl std::fmt::Display) -> CandidateExplorationError {
    CandidateExplorationError::Codec(error.to_string())
}
