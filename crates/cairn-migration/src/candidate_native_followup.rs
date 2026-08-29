//! Native-build feedback and one explicit previous-revision Candidate follow-up.

use std::io::Cursor;

use cairn_execution::{
    DOCKER_BACKEND, ExecutionEnvironmentArtifact, ExecutionEvidenceArtifact, ExecutionOutcome,
    ExecutionReceipt, ExecutionReceiptArtifact, ExecutionStderrArtifact, InputBundleArtifact,
    JobContractArtifact, TrustedExecutionEvidence,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId, JobId};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, CandidateBuildDiagnosticText, CandidateEpisodeError,
    CandidateRevisionError, CollectionCandidateProposalSubmissionV1,
    CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
    CollectionCandidateSearchInputArtifact, PreparedCandidateNativeRevisionBuildJob,
};

const SCHEMA_V1: u16 = 1;

/// Exact native-ASC build feedback for one Candidate revision.
pub enum CollectionCandidateNativeBuildDiagnosticArtifact {}

impl ContentType for CollectionCandidateNativeBuildDiagnosticArtifact {
    const DOMAIN: &'static str = "migration.candidate-native-build-diagnostic.v1";
}

/// Current-V1 native feedback whose authority is derived from one exact failed receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateNativeBuildDiagnosticV1 {
    schema_version: u16,
    previous_revision: ContentId<CollectionCandidateRevisionArtifact>,
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
struct CollectionCandidateNativeBuildDiagnosticWire {
    schema_version: u16,
    previous_revision: ContentId<CollectionCandidateRevisionArtifact>,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    stderr: ContentId<ExecutionStderrArtifact>,
    evidence: ContentId<ExecutionEvidenceArtifact>,
    diagnostic: CandidateBuildDiagnosticText,
}

impl CollectionCandidateNativeBuildDiagnosticV1 {
    fn validate(&self) -> Result<(), CandidateNativeFollowupError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateNativeFollowupError::UnsupportedSchema);
        }
        CandidateBuildDiagnosticText::new(self.diagnostic.as_str())?;
        Ok(())
    }

    #[must_use]
    pub const fn previous_revision(&self) -> ContentId<CollectionCandidateRevisionArtifact> {
        self.previous_revision
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

impl TryFrom<CollectionCandidateNativeBuildDiagnosticWire>
    for CollectionCandidateNativeBuildDiagnosticV1
{
    type Error = CandidateNativeFollowupError;

    fn try_from(wire: CollectionCandidateNativeBuildDiagnosticWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            previous_revision: wire.previous_revision,
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

impl<'de> Deserialize<'de> for CollectionCandidateNativeBuildDiagnosticV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateNativeBuildDiagnosticWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical native build diagnostic ready for archival and a new Candidate episode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCandidateNativeBuildDiagnostic {
    diagnostic: CollectionCandidateNativeBuildDiagnosticV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>,
}

impl PreparedCandidateNativeBuildDiagnostic {
    #[must_use]
    pub const fn diagnostic(&self) -> &CollectionCandidateNativeBuildDiagnosticV1 {
        &self.diagnostic
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateNativeBuildDiagnosticArtifact> {
        self.id
    }

    /// Archives exact native diagnostic bytes under their typed identity.
    ///
    /// # Errors
    ///
    /// Fails if storage changes the identity or cannot publish the bytes.
    pub fn archive<C: ContentStore>(
        &self,
        content: &mut C,
    ) -> Result<(), CandidateNativeFollowupError> {
        let archived = content
            .put::<CollectionCandidateNativeBuildDiagnosticArtifact>(&mut Cursor::new(&self.bytes))?
            .content_id;
        if archived != self.id {
            return Err(CandidateNativeFollowupError::BindingMismatch);
        }
        Ok(())
    }
}

/// Revalidates exact canonical native-build diagnostic bytes under their typed identity.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, invalid, or identity-mismatched diagnostic bytes.
pub fn validate_archived_candidate_native_build_diagnostic(
    bytes: &[u8],
    expected: ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>,
) -> Result<PreparedCandidateNativeBuildDiagnostic, CandidateNativeFollowupError> {
    let diagnostic: CollectionCandidateNativeBuildDiagnosticV1 =
        cairn_codec::from_slice(bytes).map_err(codec)?;
    let canonical = encode(&diagnostic)?;
    let id = ContentId::derive(&canonical).map_err(codec)?;
    if canonical != bytes || id != expected {
        return Err(CandidateNativeFollowupError::BindingMismatch);
    }
    Ok(PreparedCandidateNativeBuildDiagnostic {
        diagnostic,
        bytes: canonical,
        id,
    })
}

/// Verifies one failed native ASC execution and selects bounded applicant-visible feedback.
///
/// # Errors
///
/// Rejects every revision/build/receipt/stderr/evidence mismatch, non-subject outcomes,
/// non-Docker execution, and execution that does not prove the expected no-device environment.
pub fn prepare_candidate_native_build_diagnostic(
    build: &PreparedCandidateNativeRevisionBuildJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    stderr_bytes: &[u8],
    evidence_bytes: &[u8],
) -> Result<PreparedCandidateNativeBuildDiagnostic, CandidateNativeFollowupError> {
    let bounded = validate_native_failed_receipt(
        receipt_id,
        receipt,
        stderr_bytes,
        evidence_bytes,
        build.contract().job_id(),
        build.contract_id(),
        build.environment_id(),
    )?;
    let diagnostic = CollectionCandidateNativeBuildDiagnosticV1 {
        schema_version: SCHEMA_V1,
        previous_revision: build.revision_id(),
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
    Ok(PreparedCandidateNativeBuildDiagnostic {
        diagnostic,
        bytes,
        id,
    })
}

pub(crate) fn validate_native_failed_receipt(
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    stderr_bytes: &[u8],
    evidence_bytes: &[u8],
    expected_job: JobId,
    expected_contract: ContentId<JobContractArtifact>,
    expected_environment: ContentId<ExecutionEnvironmentArtifact>,
) -> Result<CandidateBuildDiagnosticText, CandidateNativeFollowupError> {
    let receipt_bytes = cairn_codec::to_vec(receipt).map_err(codec)?;
    if ContentId::derive(&receipt_bytes).map_err(codec)? != receipt_id
        || receipt.job_id() != expected_job
        || receipt.contract_id() != expected_contract
        || receipt.outcome() != ExecutionOutcome::SubjectFailed
        || ContentId::derive(stderr_bytes).map_err(codec)? != receipt.stderr_id()
        || ContentId::derive(evidence_bytes).map_err(codec)? != receipt.evidence_id()
    {
        return Err(CandidateNativeFollowupError::BindingMismatch);
    }
    let evidence: TrustedExecutionEvidence =
        cairn_codec::from_slice(evidence_bytes).map_err(codec)?;
    if evidence.backend().as_str() != DOCKER_BACKEND
        || evidence.observed_environment_id() != expected_environment
        || !evidence
            .observations()
            .iter()
            .any(|observation| observation.as_str() == "docker:accelerator:none")
    {
        return Err(CandidateNativeFollowupError::BindingMismatch);
    }
    let text = std::str::from_utf8(stderr_bytes)
        .map_err(|_| CandidateNativeFollowupError::InvalidDiagnostic)?;
    CandidateBuildDiagnosticText::new(text).map_err(Into::into)
}

/// One complete source revision authored after exact native compiler feedback.
pub enum CollectionCandidateNativeFollowupRevisionArtifact {}

impl ContentType for CollectionCandidateNativeFollowupRevisionArtifact {
    const DOMAIN: &'static str = "migration.candidate-native-followup-revision.v1";
}

/// Previous-revision-linked, non-authoritative full source submission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateNativeFollowupRevisionV1 {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    previous_revision: ContentId<CollectionCandidateRevisionArtifact>,
    build_diagnostic: ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCandidateNativeFollowupRevisionWire {
    schema_version: u16,
    search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    previous_revision: ContentId<CollectionCandidateRevisionArtifact>,
    build_diagnostic: ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
}

impl CollectionCandidateNativeFollowupRevisionV1 {
    fn validate(&self) -> Result<(), CandidateNativeFollowupError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(CandidateNativeFollowupError::UnsupportedSchema);
        }
        Ok(())
    }

    #[must_use]
    pub const fn search_input(&self) -> ContentId<CollectionCandidateSearchInputArtifact> {
        self.search_input
    }

    #[must_use]
    pub const fn previous_revision(&self) -> ContentId<CollectionCandidateRevisionArtifact> {
        self.previous_revision
    }

    #[must_use]
    pub const fn build_diagnostic(
        &self,
    ) -> ContentId<CollectionCandidateNativeBuildDiagnosticArtifact> {
        self.build_diagnostic
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
    pub const fn submission(&self) -> &CollectionCandidateProposalSubmissionV1 {
        &self.submission
    }

    /// Derives the exact immutable follow-up identity.
    ///
    /// # Errors
    ///
    /// Rejects non-V1 or unencodable material.
    pub fn identity(
        &self,
    ) -> Result<
        ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
        CandidateNativeFollowupError,
    > {
        self.validate()?;
        ContentId::derive(&encode(self)?).map_err(codec)
    }
}

impl TryFrom<CollectionCandidateNativeFollowupRevisionWire>
    for CollectionCandidateNativeFollowupRevisionV1
{
    type Error = CandidateNativeFollowupError;

    fn try_from(wire: CollectionCandidateNativeFollowupRevisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_input: wire.search_input,
            previous_revision: wire.previous_revision,
            build_diagnostic: wire.build_diagnostic,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionCandidateNativeFollowupRevisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateNativeFollowupRevisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical changed native-feedback follow-up ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCollectionCandidateNativeFollowupRevision {
    revision: CollectionCandidateNativeFollowupRevisionV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
}

impl PreparedCollectionCandidateNativeFollowupRevision {
    #[must_use]
    pub const fn revision(&self) -> &CollectionCandidateNativeFollowupRevisionV1 {
        &self.revision
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateNativeFollowupRevisionArtifact> {
        self.id
    }
}

/// Binds a changed full-source submission to exact previous revision and native diagnostic.
///
/// A generic build diagnostic cannot substitute for the native diagnostic domain.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionCandidateBuildDiagnosticArtifact, CollectionCandidateRevisionV1,
///     prepare_collection_candidate_native_followup_revision,
/// };
/// use cairn_protocol::{ContentId, EpisodeId};
/// fn invalid(
///     previous: &CollectionCandidateRevisionV1,
///     previous_id: ContentId<cairn_migration::CollectionCandidateRevisionArtifact>,
///     wrong: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
/// ) {
///     let _ = prepare_collection_candidate_native_followup_revision(
///         previous, previous_id, wrong, EpisodeId::new(), todo!(), todo!()
///     );
/// }
/// ```
///
/// # Errors
///
/// Rejects previous revision/diagnostic mismatches and unchanged submissions.
pub fn prepare_collection_candidate_native_followup_revision(
    previous: &CollectionCandidateRevisionV1,
    previous_id: ContentId<CollectionCandidateRevisionArtifact>,
    diagnostic: &PreparedCandidateNativeBuildDiagnostic,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    submission: CollectionCandidateProposalSubmissionV1,
) -> Result<PreparedCollectionCandidateNativeFollowupRevision, CandidateNativeFollowupError> {
    if previous.identity()? != previous_id
        || diagnostic.diagnostic.previous_revision != previous_id
        || submission == *previous.submission()
    {
        return Err(CandidateNativeFollowupError::BindingMismatch);
    }
    let revision = CollectionCandidateNativeFollowupRevisionV1 {
        schema_version: SCHEMA_V1,
        search_input: previous.search_input(),
        previous_revision: previous_id,
        build_diagnostic: diagnostic.id,
        episode_id,
        model_configuration,
        submission,
    };
    let bytes = encode(&revision)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCollectionCandidateNativeFollowupRevision {
        revision,
        bytes,
        id,
    })
}

/// Revalidates one exact published native-feedback follow-up under its typed identity.
///
/// A previous Candidate revision identity cannot substitute for this follow-up publication.
///
/// ```compile_fail
/// use cairn_migration::{
///     CollectionCandidateRevisionArtifact,
///     validate_archived_collection_candidate_native_followup_revision,
/// };
/// use cairn_protocol::ContentId;
/// fn invalid(bytes: &[u8], wrong: ContentId<CollectionCandidateRevisionArtifact>) {
///     let _ = validate_archived_collection_candidate_native_followup_revision(bytes, wrong);
/// }
/// ```
///
/// # Errors
///
/// Rejects noncanonical, non-V1, structurally invalid, or identity-mismatched follow-up bytes.
pub fn validate_archived_collection_candidate_native_followup_revision(
    bytes: &[u8],
    expected: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
) -> Result<CollectionCandidateNativeFollowupRevisionV1, CandidateNativeFollowupError> {
    let revision: CollectionCandidateNativeFollowupRevisionV1 =
        cairn_codec::from_slice(bytes).map_err(codec)?;
    let canonical = encode(&revision)?;
    let identity = ContentId::derive(&canonical).map_err(codec)?;
    if canonical != bytes || identity != expected {
        return Err(CandidateNativeFollowupError::BindingMismatch);
    }
    Ok(revision)
}

/// Failure while deriving native feedback or its explicitly linked follow-up.
#[derive(Debug, Error)]
pub enum CandidateNativeFollowupError {
    #[error("Candidate native follow-up uses a schema other than current V1")]
    UnsupportedSchema,
    #[error("Candidate native build diagnostic is invalid or exceeds the public bound")]
    InvalidDiagnostic,
    #[error("Candidate native follow-up authority binding is inconsistent")]
    BindingMismatch,
    #[error("Candidate native follow-up codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Revision(#[from] CandidateRevisionError),
    #[error(transparent)]
    Proposal(#[from] CandidateEpisodeError),
    #[error(transparent)]
    Content(#[from] ContentStoreError),
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, CandidateNativeFollowupError> {
    cairn_codec::to_vec(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CandidateNativeFollowupError {
    CandidateNativeFollowupError::Codec(error.to_string())
}

#[cfg(test)]
mod tests {
    use cairn_execution::{
        DockerImageId, ExecutionBackend, ExecutionObservation, ExecutionStdoutArtifact,
        ResolvedProgramIdentity,
    };
    use cairn_protocol::{AttemptId, ContentType, JobId};
    use serde_json::json;

    use super::*;
    use crate::{
        CandidateBuildEnvironmentProfileV1, CollectionCandidateBuildDiagnosticArtifact,
        CollectionCandidateProposalArtifact, prepare_candidate_native_revision_build_job,
    };

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    struct Fixture {
        previous: CollectionCandidateRevisionV1,
        previous_id: ContentId<CollectionCandidateRevisionArtifact>,
        build: PreparedCandidateNativeRevisionBuildJob,
        receipt: ExecutionReceipt,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        stderr: Vec<u8>,
        evidence: Vec<u8>,
    }

    fn fixture() -> Fixture {
        let previous_bytes = cairn_codec::to_vec(&json!({
            "schema_version":1,
            "search_input":id::<CollectionCandidateSearchInputArtifact>(b"search"),
            "parent_proposal":id::<CollectionCandidateProposalArtifact>(b"parent"),
            "build_diagnostic":id::<CollectionCandidateBuildDiagnosticArtifact>(b"diagnostic"),
            "episode_id":EpisodeId::new(),
            "model_configuration":id::<AgentResolvedRuntimeModelArtifact>(b"previous model"),
            "submission":{
                "schema_version":1,
                "files":[
                    {"path":"src/kernel.cpp","source":"#include \"kernel_operator.h\"\nusing namespace AscendC;\nclass Kernel { public: __aicore__ Kernel() {} };\nvoid host() { Kernel kernel; }\n"}
                ],
                "primary_source":"src/kernel.cpp",
                "explanation":"Previous source before native compiler feedback."
            }
        }))
        .expect("previous bytes");
        let previous_id = ContentId::derive(&previous_bytes).expect("previous ID");
        let previous = cairn_codec::from_slice(&previous_bytes).expect("previous revision");
        let build = prepare_candidate_native_revision_build_job(
            JobId::new(),
            &previous_bytes,
            previous_id,
            DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
        )
        .expect("native build");
        let stderr =
            b"candidate_primary.asc:4: error: call to __aicore__ function from __host__ function\n"
                .to_vec();
        let evidence_value = TrustedExecutionEvidence::new(
            ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
            build.environment_id(),
            ResolvedProgramIdentity::new("sha256:native-gate").expect("program"),
            vec![ExecutionObservation::new("docker:accelerator:none").expect("observation")],
        )
        .expect("evidence");
        let evidence = cairn_codec::to_vec(&evidence_value).expect("evidence bytes");
        let receipt_bytes = receipt_bytes(
            build.contract().job_id(),
            build.contract_id(),
            "subject-failed",
            1,
            ContentId::derive(&stderr).expect("stderr ID"),
            ContentId::derive(&evidence).expect("evidence ID"),
        );
        let receipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
        let receipt_id = ContentId::derive(&receipt_bytes).expect("receipt ID");
        Fixture {
            previous,
            previous_id,
            build,
            receipt,
            receipt_id,
            stderr,
            evidence,
        }
    }

    fn receipt_bytes(
        job_id: JobId,
        contract_id: ContentId<JobContractArtifact>,
        outcome: &str,
        exit_code: i32,
        stderr_id: ContentId<ExecutionStderrArtifact>,
        evidence_id: ContentId<ExecutionEvidenceArtifact>,
    ) -> Vec<u8> {
        cairn_codec::to_vec(&json!({
            "schema_version":1,
            "job_id":job_id,
            "attempt_id":AttemptId::new(),
            "contract_id":contract_id,
            "outcome":outcome,
            "exit_code":exit_code,
            "elapsed_ms":12,
            "stdout_id":id::<ExecutionStdoutArtifact>(b"stdout"),
            "stderr_id":stderr_id,
            "evidence_id":evidence_id,
            "outputs":[]
        }))
        .expect("receipt bytes")
    }

    #[test]
    fn native_diagnostic_binds_exact_failed_receipt_and_no_device_evidence() {
        let fixture = fixture();
        let diagnostic = prepare_candidate_native_build_diagnostic(
            &fixture.build,
            fixture.receipt_id,
            &fixture.receipt,
            &fixture.stderr,
            &fixture.evidence,
        )
        .expect("native diagnostic");
        assert_eq!(
            diagnostic.diagnostic().previous_revision(),
            fixture.previous_id
        );
        assert_eq!(
            diagnostic.diagnostic().input_bundle(),
            fixture.build.input_bundle_id()
        );
        assert_eq!(
            diagnostic.diagnostic().environment(),
            fixture.build.environment_id()
        );
        assert_eq!(
            diagnostic.diagnostic().contract(),
            fixture.build.contract_id()
        );
        assert_eq!(diagnostic.diagnostic().receipt(), fixture.receipt_id);
        assert_eq!(
            diagnostic.diagnostic().diagnostic().as_str().as_bytes(),
            fixture.stderr
        );
        let decoded: CollectionCandidateNativeBuildDiagnosticV1 =
            cairn_codec::from_slice(diagnostic.bytes()).expect("strict diagnostic");
        assert_eq!(decoded, *diagnostic.diagnostic());

        assert!(
            prepare_candidate_native_build_diagnostic(
                &fixture.build,
                id::<ExecutionReceiptArtifact>(b"wrong receipt"),
                &fixture.receipt,
                &fixture.stderr,
                &fixture.evidence,
            )
            .is_err()
        );
        assert!(
            prepare_candidate_native_build_diagnostic(
                &fixture.build,
                fixture.receipt_id,
                &fixture.receipt,
                b"different stderr\n",
                &fixture.evidence,
            )
            .is_err()
        );
        assert!(
            prepare_candidate_native_build_diagnostic(
                &fixture.build,
                fixture.receipt_id,
                &fixture.receipt,
                &fixture.stderr,
                b"different evidence",
            )
            .is_err()
        );

        let no_observation = TrustedExecutionEvidence::new(
            ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
            fixture.build.environment_id(),
            ResolvedProgramIdentity::new("sha256:native-gate").expect("program"),
            Vec::new(),
        )
        .expect("evidence");
        let no_observation = cairn_codec::to_vec(&no_observation).expect("evidence bytes");
        let receipt_bytes = receipt_bytes(
            fixture.build.contract().job_id(),
            fixture.build.contract_id(),
            "subject-failed",
            1,
            ContentId::derive(&fixture.stderr).expect("stderr ID"),
            ContentId::derive(&no_observation).expect("evidence ID"),
        );
        let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
        assert!(
            prepare_candidate_native_build_diagnostic(
                &fixture.build,
                ContentId::derive(&receipt_bytes).expect("receipt ID"),
                &receipt,
                &fixture.stderr,
                &no_observation,
            )
            .is_err()
        );
    }

    #[test]
    fn native_diagnostic_rejects_wrong_job_contract_and_non_subject_outcome() {
        let fixture = fixture();
        for (job_id, contract_id, outcome, exit_code) in [
            (
                JobId::new(),
                fixture.build.contract_id(),
                "subject-failed",
                1,
            ),
            (
                fixture.build.contract().job_id(),
                id::<JobContractArtifact>(b"wrong contract"),
                "subject-failed",
                1,
            ),
            (
                fixture.build.contract().job_id(),
                fixture.build.contract_id(),
                "succeeded",
                0,
            ),
        ] {
            let bytes = receipt_bytes(
                job_id,
                contract_id,
                outcome,
                exit_code,
                ContentId::derive(&fixture.stderr).expect("stderr ID"),
                ContentId::derive(&fixture.evidence).expect("evidence ID"),
            );
            let receipt: ExecutionReceipt = cairn_codec::from_slice(&bytes).expect("receipt");
            assert!(
                prepare_candidate_native_build_diagnostic(
                    &fixture.build,
                    ContentId::derive(&bytes).expect("receipt ID"),
                    &receipt,
                    &fixture.stderr,
                    &fixture.evidence,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn followup_requires_exact_previous_revision_and_changed_complete_source() {
        let fixture = fixture();
        let diagnostic = prepare_candidate_native_build_diagnostic(
            &fixture.build,
            fixture.receipt_id,
            &fixture.receipt,
            &fixture.stderr,
            &fixture.evidence,
        )
        .expect("native diagnostic");
        assert!(
            prepare_collection_candidate_native_followup_revision(
                &fixture.previous,
                fixture.previous_id,
                &diagnostic,
                EpisodeId::new(),
                id::<AgentResolvedRuntimeModelArtifact>(b"follow-up model"),
                fixture.previous.submission().clone(),
            )
            .is_err()
        );
        let changed: CollectionCandidateProposalSubmissionV1 = cairn_codec::from_slice(
            &cairn_codec::to_vec(&json!({
                "schema_version":1,
                "files":[
                    {"path":"src/kernel.asc","source":"#include \"kernel_operator.h\"\nusing namespace AscendC;\nextern \"C\" __global__ __aicore__ void kernel(GM_ADDR input) { (void)input; }\n"}
                ],
                "primary_source":"src/kernel.asc",
                "explanation":"Changed native translation unit after exact compiler feedback."
            }))
            .expect("submission bytes"),
        )
        .expect("submission");
        assert!(
            prepare_collection_candidate_native_followup_revision(
                &fixture.previous,
                id::<CollectionCandidateRevisionArtifact>(b"wrong previous"),
                &diagnostic,
                EpisodeId::new(),
                id::<AgentResolvedRuntimeModelArtifact>(b"follow-up model"),
                changed.clone(),
            )
            .is_err()
        );
        let followup = prepare_collection_candidate_native_followup_revision(
            &fixture.previous,
            fixture.previous_id,
            &diagnostic,
            EpisodeId::new(),
            id::<AgentResolvedRuntimeModelArtifact>(b"follow-up model"),
            changed,
        )
        .expect("follow-up");
        assert_eq!(followup.revision().previous_revision(), fixture.previous_id);
        assert_eq!(followup.revision().build_diagnostic(), diagnostic.id());
        let decoded: CollectionCandidateNativeFollowupRevisionV1 =
            cairn_codec::from_slice(followup.bytes()).expect("strict follow-up");
        assert_eq!(decoded, *followup.revision());
    }
}
