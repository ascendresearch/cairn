//! Receipt-bound Candidate build feedback and immutable source revision lineage.

use std::io::Cursor;

use cairn_execution::{
    DOCKER_BACKEND, ExecutionEnvironmentArtifact, ExecutionEvidenceArtifact, ExecutionOutcome,
    ExecutionReceipt, ExecutionReceiptArtifact, ExecutionStderrArtifact, InputBundleArtifact,
    JobContractArtifact, TrustedExecutionEvidence,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    CollectionCandidateProposalArtifact, CollectionCandidateProposalSubmissionV1,
    CollectionCandidateProposalV1, CollectionCandidateSearchInputArtifact,
    PreparedCandidateBuildJob, SirResolvedRuntimeModelArtifact,
};

const SCHEMA_V1: u16 = 1;
const MAX_VISIBLE_DIAGNOSTIC_BYTES: usize = 16 * 1024;

/// Exact applicant-visible build diagnostic selected from one trusted failed receipt.
pub enum CollectionCandidateBuildDiagnosticArtifact {}

impl ContentType for CollectionCandidateBuildDiagnosticArtifact {
    const DOMAIN: &'static str = "migration.candidate-build-diagnostic.v1";
}

/// Bounded untrusted compiler text, distinct from trusted worker evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CandidateBuildDiagnosticText(String);

impl CandidateBuildDiagnosticText {
    /// Accepts one complete small UTF-8 diagnostic for the current public build lane.
    ///
    /// # Errors
    ///
    /// Rejects blank, oversized, or terminal-control-containing text.
    pub fn new(value: impl Into<String>) -> Result<Self, CandidateRevisionError> {
        let value = value.into();
        if value.trim().is_empty()
            || value.len() > MAX_VISIBLE_DIAGNOSTIC_BYTES
            || value
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        {
            return Err(CandidateRevisionError::InvalidDiagnostic);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CandidateBuildDiagnosticText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Current-V1 build feedback whose authority is entirely derived by trusted code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateBuildDiagnosticV1 {
    schema_version: u16,
    parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
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
struct CollectionCandidateBuildDiagnosticWire {
    schema_version: u16,
    parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    stderr: ContentId<ExecutionStderrArtifact>,
    evidence: ContentId<ExecutionEvidenceArtifact>,
    diagnostic: CandidateBuildDiagnosticText,
}

impl CollectionCandidateBuildDiagnosticV1 {
    fn validate(&self) -> Result<(), CandidateRevisionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateRevisionError::UnsupportedSchema);
        }
        CandidateBuildDiagnosticText::new(self.diagnostic.0.clone())?;
        Ok(())
    }

    #[must_use]
    pub const fn parent_proposal(&self) -> ContentId<CollectionCandidateProposalArtifact> {
        self.parent_proposal
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

impl TryFrom<CollectionCandidateBuildDiagnosticWire> for CollectionCandidateBuildDiagnosticV1 {
    type Error = CandidateRevisionError;

    fn try_from(wire: CollectionCandidateBuildDiagnosticWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            parent_proposal: wire.parent_proposal,
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

impl<'de> Deserialize<'de> for CollectionCandidateBuildDiagnosticV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateBuildDiagnosticWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical build feedback ready for archival and a new Candidate episode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCandidateBuildDiagnostic {
    diagnostic: CollectionCandidateBuildDiagnosticV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
}

impl PreparedCandidateBuildDiagnostic {
    #[must_use]
    pub const fn diagnostic(&self) -> &CollectionCandidateBuildDiagnosticV1 {
        &self.diagnostic
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateBuildDiagnosticArtifact> {
        self.id
    }

    /// Archives exact diagnostic bytes under their typed identity.
    ///
    /// # Errors
    ///
    /// Fails if storage changes the expected identity or cannot publish the bytes.
    pub fn archive<C: ContentStore>(&self, content: &mut C) -> Result<(), CandidateRevisionError> {
        let archived = content
            .put::<CollectionCandidateBuildDiagnosticArtifact>(&mut Cursor::new(&self.bytes))?
            .content_id;
        if archived != self.id {
            return Err(CandidateRevisionError::BindingMismatch);
        }
        Ok(())
    }
}

/// Verifies a generic failed execution and creates minimal Candidate-visible build feedback.
///
/// # Errors
///
/// Rejects every mismatch in the proposal/build/receipt/stderr/evidence chain, non-subject
/// outcomes, non-no-device execution, and diagnostics outside the current public bound.
pub fn prepare_candidate_build_diagnostic(
    build: &PreparedCandidateBuildJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    stderr_bytes: &[u8],
    evidence_bytes: &[u8],
) -> Result<PreparedCandidateBuildDiagnostic, CandidateRevisionError> {
    let receipt_bytes = cairn_codec::to_vec(receipt).map_err(codec)?;
    if ContentId::derive(&receipt_bytes).map_err(codec)? != receipt_id
        || receipt.job_id() != build.contract().job_id()
        || receipt.contract_id() != build.contract_id()
        || receipt.outcome() != ExecutionOutcome::SubjectFailed
        || ContentId::derive(stderr_bytes).map_err(codec)? != receipt.stderr_id()
        || ContentId::derive(evidence_bytes).map_err(codec)? != receipt.evidence_id()
    {
        return Err(CandidateRevisionError::BindingMismatch);
    }
    let evidence: TrustedExecutionEvidence =
        cairn_codec::from_slice(evidence_bytes).map_err(codec)?;
    if evidence.backend().as_str() != DOCKER_BACKEND
        || evidence.observed_environment_id() != build.environment_id()
        || !evidence
            .observations()
            .iter()
            .any(|observation| observation.as_str() == "docker:accelerator:none")
    {
        return Err(CandidateRevisionError::BindingMismatch);
    }
    let diagnostic_text =
        std::str::from_utf8(stderr_bytes).map_err(|_| CandidateRevisionError::InvalidDiagnostic)?;
    let diagnostic = CollectionCandidateBuildDiagnosticV1 {
        schema_version: SCHEMA_V1,
        parent_proposal: build.proposal_id(),
        input_bundle: build.input_bundle_id(),
        environment: build.environment_id(),
        contract: build.contract_id(),
        receipt: receipt_id,
        stderr: receipt.stderr_id(),
        evidence: receipt.evidence_id(),
        diagnostic: CandidateBuildDiagnosticText::new(diagnostic_text)?,
    };
    let bytes = encode(&diagnostic)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCandidateBuildDiagnostic {
        diagnostic,
        bytes,
        id,
    })
}

/// One immutable full-source revision produced by a distinct Candidate episode.
pub enum CollectionCandidateRevisionArtifact {}

impl ContentType for CollectionCandidateRevisionArtifact {
    const DOMAIN: &'static str = "migration.candidate-collection-revision.v1";
}

/// Parent-linked non-authoritative source revision with trusted episode provenance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateRevisionV1 {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
    build_diagnostic: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCandidateRevisionWire {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
    build_diagnostic: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

impl CollectionCandidateRevisionV1 {
    pub(crate) fn new(
        search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
        build_diagnostic: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
        submission: CollectionCandidateProposalSubmissionV1,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            search_input,
            parent_proposal,
            build_diagnostic,
            episode_id,
            model_configuration,
            submission,
        }
    }

    fn validate(&self) -> Result<(), CandidateRevisionError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateRevisionError::UnsupportedSchema);
        }
        Ok(())
    }

    #[must_use]
    pub const fn search_input(&self) -> ContentId<CollectionCandidateSearchInputArtifact> {
        self.search_input
    }

    #[must_use]
    pub const fn parent_proposal(&self) -> ContentId<CollectionCandidateProposalArtifact> {
        self.parent_proposal
    }

    #[must_use]
    pub const fn build_diagnostic(&self) -> ContentId<CollectionCandidateBuildDiagnosticArtifact> {
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

    /// Derives the exact immutable revision identity.
    ///
    /// # Errors
    ///
    /// Rejects non-V1 or unencodable revision material.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CollectionCandidateRevisionArtifact>, CandidateRevisionError> {
        self.validate()?;
        ContentId::derive(&encode(self)?).map_err(codec)
    }
}

impl TryFrom<CollectionCandidateRevisionWire> for CollectionCandidateRevisionV1 {
    type Error = CandidateRevisionError;

    fn try_from(wire: CollectionCandidateRevisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_input: wire.search_input,
            parent_proposal: wire.parent_proposal,
            build_diagnostic: wire.build_diagnostic,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionCandidateRevisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateRevisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical changed Candidate revision ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCollectionCandidateRevision {
    revision: CollectionCandidateRevisionV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateRevisionArtifact>,
}

impl PreparedCollectionCandidateRevision {
    #[must_use]
    pub const fn revision(&self) -> &CollectionCandidateRevisionV1 {
        &self.revision
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateRevisionArtifact> {
        self.id
    }
}

/// Binds a model-authored full-source submission to exact parent and diagnostic authority.
///
/// A prior revision identity cannot be confused with the required initial parent domain.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionCandidateProposalV1, CollectionCandidateRevisionArtifact,
///     PreparedCandidateBuildDiagnostic, prepare_collection_candidate_revision,
/// };
/// use cairn_protocol::{ContentId, EpisodeId};
/// fn invalid(
///     parent: &CollectionCandidateProposalV1,
///     wrong: ContentId<CollectionCandidateRevisionArtifact>,
///     diagnostic: &PreparedCandidateBuildDiagnostic,
/// ) {
///     let _ = prepare_collection_candidate_revision(
///         parent, wrong, diagnostic, EpisodeId::new(), todo!(), todo!()
///     );
/// }
/// ```
///
/// # Errors
///
/// Rejects parent/diagnostic mismatches and unchanged submissions.
pub fn prepare_collection_candidate_revision(
    parent: &CollectionCandidateProposalV1,
    parent_id: ContentId<CollectionCandidateProposalArtifact>,
    diagnostic: &PreparedCandidateBuildDiagnostic,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
) -> Result<PreparedCollectionCandidateRevision, CandidateRevisionError> {
    if parent
        .identity()
        .map_err(CandidateRevisionError::Proposal)?
        != parent_id
        || diagnostic.diagnostic.parent_proposal != parent_id
        || submission == *parent.submission()
    {
        return Err(CandidateRevisionError::BindingMismatch);
    }
    let revision = CollectionCandidateRevisionV1::new(
        parent.search_input(),
        parent_id,
        diagnostic.id,
        episode_id,
        model_configuration,
        submission,
    );
    let bytes = encode(&revision)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCollectionCandidateRevision {
        revision,
        bytes,
        id,
    })
}

/// Revalidates one exact published Candidate revision under its typed identity.
///
/// An initial proposal identity cannot substitute for a revision publication identity.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionCandidateProposalArtifact, validate_archived_collection_candidate_revision,
/// };
/// use cairn_protocol::ContentId;
/// fn invalid(bytes: &[u8], wrong: ContentId<CollectionCandidateProposalArtifact>) {
///     let _ = validate_archived_collection_candidate_revision(bytes, wrong);
/// }
/// ```
///
/// # Errors
///
/// Rejects noncanonical, non-V1, structurally invalid, or identity-mismatched revision bytes.
pub fn validate_archived_collection_candidate_revision(
    bytes: &[u8],
    expected: ContentId<CollectionCandidateRevisionArtifact>,
) -> Result<CollectionCandidateRevisionV1, CandidateRevisionError> {
    let revision: CollectionCandidateRevisionV1 = cairn_codec::from_slice(bytes).map_err(codec)?;
    let canonical = encode(&revision)?;
    let identity = ContentId::derive(&canonical).map_err(codec)?;
    if canonical != bytes || identity != expected {
        return Err(CandidateRevisionError::BindingMismatch);
    }
    Ok(revision)
}

/// Failure while deriving public Candidate build feedback or a source revision.
#[derive(Debug, Error)]
pub enum CandidateRevisionError {
    #[error("Candidate revision uses a schema other than current V1")]
    UnsupportedSchema,
    #[error("Candidate build diagnostic is invalid or exceeds the public bound")]
    InvalidDiagnostic,
    #[error("Candidate revision authority binding is inconsistent")]
    BindingMismatch,
    #[error("Candidate revision codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Proposal(#[from] crate::CandidateEpisodeError),
    #[error(transparent)]
    Content(#[from] ContentStoreError),
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, CandidateRevisionError> {
    cairn_codec::to_vec(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CandidateRevisionError {
    CandidateRevisionError::Codec(error.to_string())
}

#[cfg(test)]
mod tests {
    use cairn_execution::{
        ExecutionBackend, ExecutionEvidenceArtifact, ExecutionObservation,
        ExecutionReceiptArtifact, ExecutionStderrArtifact, ExecutionStdoutArtifact,
        ResolvedProgramIdentity,
    };
    use cairn_protocol::{AttemptId, ContentId, ContentType, EpisodeId, JobId};
    use serde_json::json;

    use super::*;
    use crate::{
        CandidateBuildEnvironmentProfileV1, CollectionCandidateSearchInputArtifact,
        SirResolvedRuntimeModelArtifact, prepare_candidate_build_job,
    };

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn proposal_bytes() -> Vec<u8> {
        cairn_codec::to_vec(&json!({
            "schema_version":1,
            "search_input":id::<CollectionCandidateSearchInputArtifact>(b"search"),
            "episode_id":EpisodeId::new(),
            "model_configuration":id::<SirResolvedRuntimeModelArtifact>(b"initial model"),
            "submission":{
                "schema_version":1,
                "files":[
                    {"path":"CMakeLists.txt","source":"project(candidate LANGUAGES CXX)\nadd_library(candidate STATIC src/kernel.cpp)\n"},
                    {"path":"src/kernel.cpp","source":"#include \"missing.h\"\n"}
                ],
                "primary_source":"src/kernel.cpp",
                "explanation":"Initial unbuilt source."
            }
        }))
        .expect("proposal bytes")
    }

    struct Fixture {
        parent: CollectionCandidateProposalV1,
        parent_id: ContentId<CollectionCandidateProposalArtifact>,
        build: PreparedCandidateBuildJob,
        receipt: ExecutionReceipt,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        stderr: Vec<u8>,
        evidence: Vec<u8>,
    }

    fn fixture() -> Fixture {
        let proposal_bytes = proposal_bytes();
        let parent_id = ContentId::derive(&proposal_bytes).expect("proposal ID");
        let parent: CollectionCandidateProposalV1 =
            cairn_codec::from_slice(&proposal_bytes).expect("proposal");
        let job_id = JobId::new();
        let build = prepare_candidate_build_job(
            job_id,
            &proposal_bytes,
            parent_id,
            cairn_execution::DockerImageId::new(format!("sha256:{}", "a".repeat(64)))
                .expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
        )
        .expect("build");
        let stderr = b"src/kernel.cpp:1: fatal error: missing.h: No such file\n".to_vec();
        let stderr_id = ContentId::<ExecutionStderrArtifact>::derive(&stderr).expect("stderr ID");
        let evidence_value = TrustedExecutionEvidence::new(
            ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
            build.environment_id(),
            ResolvedProgramIdentity::new("sha256:program").expect("program"),
            vec![ExecutionObservation::new("docker:accelerator:none").expect("observation")],
        )
        .expect("evidence");
        let evidence = cairn_codec::to_vec(&evidence_value).expect("evidence bytes");
        let evidence_id =
            ContentId::<ExecutionEvidenceArtifact>::derive(&evidence).expect("evidence ID");
        let receipt_bytes = cairn_codec::to_vec(&json!({
            "schema_version":1,
            "job_id":job_id,
            "attempt_id":AttemptId::new(),
            "contract_id":build.contract_id(),
            "outcome":"subject-failed",
            "exit_code":1,
            "elapsed_ms":10,
            "stdout_id":id::<ExecutionStdoutArtifact>(b"stdout"),
            "stderr_id":stderr_id,
            "evidence_id":evidence_id,
            "outputs":[]
        }))
        .expect("receipt bytes");
        let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
        let receipt_id = ContentId::derive(&receipt_bytes).expect("receipt ID");
        Fixture {
            parent,
            parent_id,
            build,
            receipt,
            receipt_id,
            stderr,
            evidence,
        }
    }

    #[test]
    fn failed_receipt_becomes_exact_bounded_applicant_visible_feedback() {
        let fixture = fixture();
        let prepared = prepare_candidate_build_diagnostic(
            &fixture.build,
            fixture.receipt_id,
            &fixture.receipt,
            &fixture.stderr,
            &fixture.evidence,
        )
        .expect("diagnostic");
        assert_eq!(prepared.diagnostic().parent_proposal(), fixture.parent_id);
        assert_eq!(
            prepared.diagnostic().input_bundle(),
            fixture.build.input_bundle_id()
        );
        assert_eq!(
            prepared.diagnostic().environment(),
            fixture.build.environment_id()
        );
        assert_eq!(
            prepared.diagnostic().contract(),
            fixture.build.contract_id()
        );
        assert_eq!(prepared.diagnostic().receipt(), fixture.receipt_id);
        assert_eq!(prepared.diagnostic().stderr(), fixture.receipt.stderr_id());
        assert_eq!(
            prepared.diagnostic().evidence(),
            fixture.receipt.evidence_id()
        );
        assert_eq!(
            prepared.diagnostic().diagnostic().as_str().as_bytes(),
            fixture.stderr
        );
        let decoded: CollectionCandidateBuildDiagnosticV1 =
            cairn_codec::from_slice(prepared.bytes()).expect("strict diagnostic");
        assert_eq!(decoded, *prepared.diagnostic());

        let mut oversized = vec![b'x'; MAX_VISIBLE_DIAGNOSTIC_BYTES + 1];
        oversized.push(b'\n');
        assert!(
            prepare_candidate_build_diagnostic(
                &fixture.build,
                fixture.receipt_id,
                &fixture.receipt,
                &oversized,
                &fixture.evidence,
            )
            .is_err()
        );
        let wrong_evidence = cairn_codec::to_vec(
            &TrustedExecutionEvidence::new(
                ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
                fixture.build.environment_id(),
                ResolvedProgramIdentity::new("sha256:program").expect("program"),
                Vec::new(),
            )
            .expect("evidence"),
        )
        .expect("evidence bytes");
        assert!(
            prepare_candidate_build_diagnostic(
                &fixture.build,
                fixture.receipt_id,
                &fixture.receipt,
                &fixture.stderr,
                &wrong_evidence,
            )
            .is_err()
        );
    }

    #[test]
    fn changed_full_source_revision_preserves_parent_and_feedback_lineage() {
        let fixture = fixture();
        let diagnostic = prepare_candidate_build_diagnostic(
            &fixture.build,
            fixture.receipt_id,
            &fixture.receipt,
            &fixture.stderr,
            &fixture.evidence,
        )
        .expect("diagnostic");
        let changed: CollectionCandidateProposalSubmissionV1 = cairn_codec::from_slice(
            &cairn_codec::to_vec(&json!({
                "schema_version":1,
                "files":[
                    {"path":"CMakeLists.txt","source":"project(candidate LANGUAGES CXX)\ninclude_directories(/toolkit/include)\nadd_library(candidate STATIC src/kernel.cpp)\n"},
                    {"path":"src/kernel.cpp","source":"#include \"missing.h\"\n"}
                ],
                "primary_source":"src/kernel.cpp",
                "explanation":"Changed in response to the cited build diagnostic."
            }))
            .expect("submission bytes"),
        )
        .expect("submission");
        let revision = prepare_collection_candidate_revision(
            &fixture.parent,
            fixture.parent_id,
            &diagnostic,
            EpisodeId::new(),
            id::<SirResolvedRuntimeModelArtifact>(b"revision model"),
            changed,
        )
        .expect("revision");
        assert_eq!(revision.revision().parent_proposal(), fixture.parent_id);
        assert_eq!(revision.revision().build_diagnostic(), diagnostic.id());
        assert_ne!(
            revision.revision().submission(),
            fixture.parent.submission()
        );
        let decoded: CollectionCandidateRevisionV1 =
            cairn_codec::from_slice(revision.bytes()).expect("strict revision");
        assert_eq!(decoded, *revision.revision());

        assert!(
            prepare_collection_candidate_revision(
                &fixture.parent,
                fixture.parent_id,
                &diagnostic,
                EpisodeId::new(),
                id::<SirResolvedRuntimeModelArtifact>(b"revision model"),
                fixture.parent.submission().clone(),
            )
            .is_err()
        );
    }
}
