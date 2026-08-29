//! Explicit, repeatable native-compiler repair lineage without an automatic repair loop.

use std::io::Cursor;

use cairn_execution::{
    ExecutionEnvironmentArtifact, ExecutionEvidenceArtifact, ExecutionReceipt,
    ExecutionReceiptArtifact, ExecutionStderrArtifact, InputBundleArtifact, JobContractArtifact,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::candidate_native_followup::validate_native_failed_receipt;
use crate::{
    CandidateBuildDiagnosticText, CandidateEpisodeError, CandidateNativeFollowupError,
    CandidateRevisionError, CollectionCandidateNativeFollowupRevisionArtifact,
    CollectionCandidateNativeFollowupRevisionV1, CollectionCandidateProposalSubmissionV1,
    CollectionCandidateSearchInputArtifact, PreparedCandidateNativeFollowupBuildJob,
    SirResolvedRuntimeModelArtifact,
};

const SCHEMA_V1: u16 = 1;

/// One Candidate-authored repair after the root native follow-up.
pub enum CollectionCandidateNativeRepairRevisionArtifact {}

impl ContentType for CollectionCandidateNativeRepairRevisionArtifact {
    const DOMAIN: &'static str = "migration.candidate-native-repair-revision.v1";
}

/// Exact immediate parent of one native repair round.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "identity", rename_all = "kebab-case")]
pub enum CandidateNativeRepairParentV1 {
    RootFollowup(ContentId<CollectionCandidateNativeFollowupRevisionArtifact>),
    Repair(ContentId<CollectionCandidateNativeRepairRevisionArtifact>),
}

/// Receipt-bound native compiler feedback for one exact repair parent.
pub enum CollectionCandidateNativeRepairBuildDiagnosticArtifact {}

impl ContentType for CollectionCandidateNativeRepairBuildDiagnosticArtifact {
    const DOMAIN: &'static str = "migration.candidate-native-repair-build-diagnostic.v1";
}

/// Current-V1 feedback selected from one exact failed native build.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateNativeRepairBuildDiagnosticV1 {
    schema_version: u16,
    parent: CandidateNativeRepairParentV1,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    stderr: ContentId<ExecutionStderrArtifact>,
    evidence: ContentId<ExecutionEvidenceArtifact>,
    diagnostic: CandidateBuildDiagnosticText,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCandidateNativeRepairBuildDiagnosticWire {
    schema_version: u16,
    parent: CandidateNativeRepairParentV1,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    stderr: ContentId<ExecutionStderrArtifact>,
    evidence: ContentId<ExecutionEvidenceArtifact>,
    diagnostic: CandidateBuildDiagnosticText,
}

impl CollectionCandidateNativeRepairBuildDiagnosticV1 {
    fn validate(&self) -> Result<(), CandidateNativeRepairError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateNativeRepairError::UnsupportedSchema);
        }
        CandidateBuildDiagnosticText::new(self.diagnostic.as_str())?;
        Ok(())
    }

    #[must_use]
    pub const fn parent(&self) -> CandidateNativeRepairParentV1 {
        self.parent
    }

    #[must_use]
    pub const fn input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle
    }

    #[must_use]
    pub const fn environment(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }

    #[must_use]
    pub const fn stderr(&self) -> ContentId<ExecutionStderrArtifact> {
        self.stderr
    }

    #[must_use]
    pub const fn evidence(&self) -> ContentId<ExecutionEvidenceArtifact> {
        self.evidence
    }

    #[must_use]
    pub const fn diagnostic(&self) -> &CandidateBuildDiagnosticText {
        &self.diagnostic
    }
}

impl TryFrom<CollectionCandidateNativeRepairBuildDiagnosticWire>
    for CollectionCandidateNativeRepairBuildDiagnosticV1
{
    type Error = CandidateNativeRepairError;

    fn try_from(
        wire: CollectionCandidateNativeRepairBuildDiagnosticWire,
    ) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            parent: wire.parent,
            input_bundle: wire.input_bundle,
            environment: wire.environment,
            contract: wire.contract,
            receipt: wire.receipt,
            stderr: wire.stderr,
            evidence: wire.evidence,
            diagnostic: wire.diagnostic,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionCandidateNativeRepairBuildDiagnosticV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateNativeRepairBuildDiagnosticWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical repair diagnostic ready for archival and one explicitly opened episode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCandidateNativeRepairBuildDiagnostic {
    diagnostic: CollectionCandidateNativeRepairBuildDiagnosticV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>,
}

impl PreparedCandidateNativeRepairBuildDiagnostic {
    #[must_use]
    pub const fn diagnostic(&self) -> &CollectionCandidateNativeRepairBuildDiagnosticV1 {
        &self.diagnostic
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact> {
        self.id
    }

    /// Archives exact diagnostic bytes under their repair-specific identity.
    ///
    /// # Errors
    ///
    /// Fails if storage changes the identity or cannot publish the bytes.
    pub fn archive<C: ContentStore>(
        &self,
        content: &mut C,
    ) -> Result<(), CandidateNativeRepairError> {
        let archived = content
            .put::<CollectionCandidateNativeRepairBuildDiagnosticArtifact>(&mut Cursor::new(
                &self.bytes,
            ))?
            .content_id;
        if archived != self.id {
            return Err(CandidateNativeRepairError::BindingMismatch);
        }
        Ok(())
    }
}

/// Derives the first repair-round diagnostic from the exact root-follow-up native build.
///
/// A root-revision native build job cannot substitute for the follow-up build domain.
///
/// ```compile_fail
/// use cairn_migration::{
///     PreparedCandidateNativeRevisionBuildJob, prepare_candidate_native_repair_build_diagnostic,
/// };
/// fn invalid(wrong: &PreparedCandidateNativeRevisionBuildJob) {
///     let _ = prepare_candidate_native_repair_build_diagnostic(
///         wrong, todo!(), todo!(), b"stderr", b"evidence"
///     );
/// }
/// ```
///
/// # Errors
///
/// Rejects every build/receipt/stderr/evidence mismatch, non-subject outcomes, non-Docker
/// execution, and execution that does not prove the expected no-device environment.
pub fn prepare_candidate_native_repair_build_diagnostic(
    build: &PreparedCandidateNativeFollowupBuildJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    stderr_bytes: &[u8],
    evidence_bytes: &[u8],
) -> Result<PreparedCandidateNativeRepairBuildDiagnostic, CandidateNativeRepairError> {
    let bounded = validate_native_failed_receipt(
        receipt_id,
        receipt,
        stderr_bytes,
        evidence_bytes,
        build.contract().job_id(),
        build.contract_id(),
        build.environment_id(),
    )?;
    let diagnostic = CollectionCandidateNativeRepairBuildDiagnosticV1 {
        schema_version: SCHEMA_V1,
        parent: CandidateNativeRepairParentV1::RootFollowup(build.followup_id()),
        input_bundle: build.input_bundle_id(),
        environment: build.environment_id(),
        contract: build.contract_id(),
        receipt: receipt_id,
        stderr: receipt.stderr_id(),
        evidence: receipt.evidence_id(),
        diagnostic: bounded,
    };
    let bytes = encode(&diagnostic)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCandidateNativeRepairBuildDiagnostic {
        diagnostic,
        bytes,
        id,
    })
}

/// Current-V1 complete source repair linked to its root and immediate parent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateNativeRepairRevisionV1 {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    root_followup: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
    parent: CandidateNativeRepairParentV1,
    build_diagnostic: ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCandidateNativeRepairRevisionWire {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    root_followup: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
    parent: CandidateNativeRepairParentV1,
    build_diagnostic: ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

impl CollectionCandidateNativeRepairRevisionV1 {
    fn validate(&self) -> Result<(), CandidateNativeRepairError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateNativeRepairError::UnsupportedSchema);
        }
        if let CandidateNativeRepairParentV1::RootFollowup(parent) = self.parent {
            if parent != self.root_followup {
                return Err(CandidateNativeRepairError::BindingMismatch);
            }
        }
        Ok(())
    }

    #[must_use]
    pub const fn search_input(&self) -> ContentId<CollectionCandidateSearchInputArtifact> {
        self.search_input
    }

    #[must_use]
    pub const fn root_followup(
        &self,
    ) -> ContentId<CollectionCandidateNativeFollowupRevisionArtifact> {
        self.root_followup
    }

    #[must_use]
    pub const fn parent(&self) -> CandidateNativeRepairParentV1 {
        self.parent
    }

    #[must_use]
    pub const fn build_diagnostic(
        &self,
    ) -> ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact> {
        self.build_diagnostic
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn model_configuration(&self) -> ContentId<SirResolvedRuntimeModelArtifact> {
        self.model_configuration
    }

    #[must_use]
    pub const fn submission(&self) -> &CollectionCandidateProposalSubmissionV1 {
        &self.submission
    }

    /// Derives the exact immutable repair identity.
    ///
    /// # Errors
    ///
    /// Rejects non-V1 or unencodable material.
    pub fn identity(
        &self,
    ) -> Result<
        ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
        CandidateNativeRepairError,
    > {
        self.validate()?;
        ContentId::derive(&encode(self)?).map_err(codec)
    }
}

impl TryFrom<CollectionCandidateNativeRepairRevisionWire>
    for CollectionCandidateNativeRepairRevisionV1
{
    type Error = CandidateNativeRepairError;

    fn try_from(wire: CollectionCandidateNativeRepairRevisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_input: wire.search_input,
            root_followup: wire.root_followup,
            parent: wire.parent,
            build_diagnostic: wire.build_diagnostic,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionCandidateNativeRepairRevisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateNativeRepairRevisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One exact previous full-source publication supplied to an explicit repair round.
#[derive(Clone, Copy)]
pub enum CandidateNativeRepairPrevious<'a> {
    Root {
        revision: &'a CollectionCandidateNativeFollowupRevisionV1,
        identity: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
    },
    Repair {
        revision: &'a CollectionCandidateNativeRepairRevisionV1,
        identity: ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
    },
}

impl<'a> CandidateNativeRepairPrevious<'a> {
    fn validate(self) -> Result<(), CandidateNativeRepairError> {
        let valid = match self {
            Self::Root { revision, identity } => revision.identity()? == identity,
            Self::Repair { revision, identity } => revision.identity()? == identity,
        };
        if !valid {
            return Err(CandidateNativeRepairError::BindingMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub const fn parent(self) -> CandidateNativeRepairParentV1 {
        match self {
            Self::Root { identity, .. } => CandidateNativeRepairParentV1::RootFollowup(identity),
            Self::Repair { identity, .. } => CandidateNativeRepairParentV1::Repair(identity),
        }
    }

    #[must_use]
    pub const fn root_followup(
        self,
    ) -> ContentId<CollectionCandidateNativeFollowupRevisionArtifact> {
        match self {
            Self::Root { identity, .. } => identity,
            Self::Repair { revision, .. } => revision.root_followup(),
        }
    }

    #[must_use]
    pub const fn search_input(self) -> ContentId<CollectionCandidateSearchInputArtifact> {
        match self {
            Self::Root { revision, .. } => revision.search_input(),
            Self::Repair { revision, .. } => revision.search_input(),
        }
    }

    #[must_use]
    pub const fn submission(self) -> &'a CollectionCandidateProposalSubmissionV1 {
        match self {
            Self::Root { revision, .. } => revision.submission(),
            Self::Repair { revision, .. } => revision.submission(),
        }
    }
}

/// Canonical changed repair ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCollectionCandidateNativeRepairRevision {
    revision: CollectionCandidateNativeRepairRevisionV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
}

impl PreparedCollectionCandidateNativeRepairRevision {
    #[must_use]
    pub const fn revision(&self) -> &CollectionCandidateNativeRepairRevisionV1 {
        &self.revision
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateNativeRepairRevisionArtifact> {
        self.id
    }
}

/// Binds one changed full-source submission to an exact immediate parent and diagnostic.
///
/// A native-follow-up diagnostic cannot substitute for a native-repair diagnostic.
///
/// ```compile_fail
/// use cairn_migration::{
///     CandidateNativeRepairPrevious, PreparedCandidateNativeBuildDiagnostic,
///     prepare_collection_candidate_native_repair_revision,
/// };
/// fn invalid(previous: CandidateNativeRepairPrevious<'_>, wrong: &PreparedCandidateNativeBuildDiagnostic) {
///     let _ = prepare_collection_candidate_native_repair_revision(
///         previous, wrong, todo!(), todo!(), todo!()
///     );
/// }
/// ```
///
/// # Errors
///
/// Rejects invalid previous identity, diagnostic mismatch, or unchanged full source.
pub fn prepare_collection_candidate_native_repair_revision(
    previous: CandidateNativeRepairPrevious<'_>,
    diagnostic: &PreparedCandidateNativeRepairBuildDiagnostic,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
) -> Result<PreparedCollectionCandidateNativeRepairRevision, CandidateNativeRepairError> {
    previous.validate()?;
    if diagnostic.diagnostic.parent != previous.parent() || submission == *previous.submission() {
        return Err(CandidateNativeRepairError::BindingMismatch);
    }
    let revision = CollectionCandidateNativeRepairRevisionV1 {
        schema_version: SCHEMA_V1,
        search_input: previous.search_input(),
        root_followup: previous.root_followup(),
        parent: previous.parent(),
        build_diagnostic: diagnostic.id,
        episode_id,
        model_configuration,
        submission,
    };
    let bytes = encode(&revision)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCollectionCandidateNativeRepairRevision {
        revision,
        bytes,
        id,
    })
}

/// Revalidates exact canonical current-V1 repair bytes under their typed identity.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, structurally invalid, or identity-mismatched bytes.
pub fn validate_archived_collection_candidate_native_repair_revision(
    bytes: &[u8],
    expected: ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
) -> Result<CollectionCandidateNativeRepairRevisionV1, CandidateNativeRepairError> {
    let revision: CollectionCandidateNativeRepairRevisionV1 =
        cairn_codec::from_slice(bytes).map_err(codec)?;
    let canonical = encode(&revision)?;
    let identity = ContentId::derive(&canonical).map_err(codec)?;
    if canonical != bytes || identity != expected {
        return Err(CandidateNativeRepairError::BindingMismatch);
    }
    Ok(revision)
}

/// Failure while deriving or publishing one explicit native repair round.
#[derive(Debug, Error)]
pub enum CandidateNativeRepairError {
    #[error("Candidate native repair uses a schema other than current V1")]
    UnsupportedSchema,
    #[error("Candidate native repair diagnostic is invalid or exceeds the public bound")]
    InvalidDiagnostic,
    #[error("Candidate native repair authority binding is inconsistent")]
    BindingMismatch,
    #[error("Candidate native repair codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    NativeFollowup(#[from] CandidateNativeFollowupError),
    #[error(transparent)]
    Revision(#[from] CandidateRevisionError),
    #[error(transparent)]
    Proposal(#[from] CandidateEpisodeError),
    #[error(transparent)]
    Content(#[from] ContentStoreError),
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, CandidateNativeRepairError> {
    cairn_codec::to_vec(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CandidateNativeRepairError {
    CandidateNativeRepairError::Codec(error.to_string())
}
