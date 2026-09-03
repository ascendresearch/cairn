//! Strict client resources shared by Cairn clients and the App Server boundary.

use std::{future::Future, path::PathBuf};

use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentHypothesisSetProposalV1, IntentRecoveryRequestV1,
    SirHypothesisId, SirIntentHypothesisSetProposalArtifact, UserIntentAuthorityScopeV1,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1, UserProvidedIntentClaimV1,
};
use cairn_protocol::{
    BlobDigest, CommandId, ContentId, EventId, EventSequence, ObservedAtUnixMillis, TaskId,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const SCHEMA_V1: u16 = 1;
const MAX_FRAME_BYTES: u32 = 4 * 1024 * 1024;
/// Room reserved inside one frame for the JSON envelope around a chunk's base64 payload.
const CHUNK_ENVELOPE_RESERVE_BYTES: usize = 4096;

/// Largest raw archive slice whose base64 encoding still leaves room for its request envelope.
///
/// Base64 turns three bytes into four characters, so what a frame can carry is three quarters of
/// what remains once the envelope is reserved. This is the size a client chunks at. The server
/// applies no separate per-chunk bound: the frame limit already bounds one request and the
/// declared archive length already bounds the whole transfer, so a third bound would guard
/// nothing.
pub const MAX_ARCHIVE_CHUNK_BYTES: usize =
    (MAX_FRAME_BYTES as usize - CHUNK_ENVELOPE_RESERVE_BYTES) / 4 * 3;

/// What one submitted archive is, declared before any of its bytes are sent.
///
/// The length lets the server refuse an archive above its configured bound before it accepts a
/// single byte. The digest is what makes a completed upload either exactly the bytes the client
/// held or a refusal: a transfer that ended early without saying so would otherwise arrive as a
/// shorter archive that is still well-formed, and nothing downstream could tell the difference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TaskArchiveManifestV1 {
    schema_version: u16,
    byte_len: u64,
    digest: BlobDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskArchiveManifestWire {
    schema_version: u16,
    byte_len: u64,
    digest: BlobDigest,
}

impl TaskArchiveManifestV1 {
    /// Describes the exact archive bytes a client is about to upload.
    ///
    /// # Errors
    ///
    /// Rejects an empty archive, which no upload could complete.
    pub fn describing(archive: &[u8]) -> Result<Self, SdkError> {
        let byte_len =
            u64::try_from(archive.len()).map_err(|_| SdkError::Invalid("task archive manifest"))?;
        let value = Self {
            schema_version: SCHEMA_V1,
            byte_len,
            digest: BlobDigest::derive(archive),
        };
        value.validate()?;
        Ok(value)
    }

    /// Returns the declared archive length in bytes.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Returns the digest the reassembled upload has to reproduce.
    #[must_use]
    pub const fn digest(&self) -> BlobDigest {
        self.digest
    }

    fn validate(&self) -> Result<(), SdkError> {
        if self.schema_version != SCHEMA_V1 || self.byte_len == 0 {
            return Err(SdkError::Invalid("task archive manifest"));
        }
        Ok(())
    }
}

impl TryFrom<TaskArchiveManifestWire> for TaskArchiveManifestV1 {
    type Error = SdkError;

    fn try_from(wire: TaskArchiveManifestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            byte_len: wire.byte_len,
            digest: wire.digest,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for TaskArchiveManifestV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TaskArchiveManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// The caller-owned recovery declaration that turns an uploaded archive into a task.
///
/// The archive is not in here. It reaches the server as its own bounded chunk sequence on the same
/// connection, so a real operator source tree is no longer limited to what fits in one frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSubmissionV1 {
    schema_version: u16,
    recovery_request: IntentRecoveryRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskSubmissionWire {
    schema_version: u16,
    recovery_request: IntentRecoveryRequestV1,
}

mod canonical_base64 {
    use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        STANDARD_NO_PAD
            .decode(encoded.as_bytes())
            .map_err(D::Error::custom)
    }
}

impl TaskSubmissionV1 {
    /// Creates one submission for the archive uploaded on the same connection.
    ///
    /// Nothing here can be malformed once the archive has left, so this cannot fail. The same
    /// invariant is still checked on the way in, where wire bytes can carry any schema version.
    #[must_use]
    pub const fn new(recovery_request: IntentRecoveryRequestV1) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            recovery_request,
        }
    }

    #[must_use]
    pub const fn recovery_request(&self) -> &IntentRecoveryRequestV1 {
        &self.recovery_request
    }

    fn validate(&self) -> Result<(), SdkError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(SdkError::Invalid("task submission structure"));
        }
        Ok(())
    }
}

impl TryFrom<TaskSubmissionWire> for TaskSubmissionV1 {
    type Error = SdkError;

    fn try_from(wire: TaskSubmissionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            recovery_request: wire.recovery_request,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for TaskSubmissionV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TaskSubmissionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Stable product phase exposed to clients instead of internal Controller state variants.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskPhaseV1 {
    Submitted,
    RecoveringIntent,
    AwaitingIntentReview,
    AdmittingIntent,
    PreparingOracle,
    ExploringOracle,
    AwaitingOracleControls,
    RunningOracleControls,
    OracleAccepted,
    OraclePartial,
    ExploringCandidate,
    OracleRejected,
    Cancelled,
    Blocked,
}

/// Explicit operator attention currently required by a task.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskAttentionV1 {
    IntentReview,
    IntentAdmissionReconciliation,
    OracleWorkspace,
    OracleMechanisms,
    OracleExperiment,
    AgentExecution,
    WorkflowFailure,
    Reconciliation,
    CandidateSearchStopped,
}

/// Current public task resource.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskResourceV1 {
    schema_version: u16,
    task_id: TaskId,
    latest_sequence: EventSequence,
    phase: TaskPhaseV1,
    attention: Option<TaskAttentionV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskResourceWire {
    schema_version: u16,
    task_id: TaskId,
    latest_sequence: EventSequence,
    phase: TaskPhaseV1,
    attention: Option<TaskAttentionV1>,
}

impl TaskResourceV1 {
    #[must_use]
    pub const fn new(
        task_id: TaskId,
        latest_sequence: EventSequence,
        phase: TaskPhaseV1,
        attention: Option<TaskAttentionV1>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            task_id,
            latest_sequence,
            phase,
            attention,
        }
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn latest_sequence(&self) -> EventSequence {
        self.latest_sequence
    }

    #[must_use]
    pub const fn phase(&self) -> TaskPhaseV1 {
        self.phase
    }

    #[must_use]
    pub const fn attention(&self) -> Option<TaskAttentionV1> {
        self.attention
    }
}

impl TryFrom<TaskResourceWire> for TaskResourceV1 {
    type Error = SdkError;

    fn try_from(wire: TaskResourceWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SdkError::Invalid("task resource schema"));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            latest_sequence: wire.latest_sequence,
            phase: wire.phase,
            attention: wire.attention,
        })
    }
}

impl<'de> Deserialize<'de> for TaskResourceV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TaskResourceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Product lifecycle update reconstructed from one durable Controller fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProgressItemV1 {
    pub sequence: EventSequence,
    pub event_id: EventId,
    pub observed_at: ObservedAtUnixMillis,
    pub phase: TaskPhaseV1,
}

/// Reconnectable task progress page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskProgressPageV1 {
    pub task: TaskResourceV1,
    pub items: Vec<TaskProgressItemV1>,
}

/// Exact SIR material offered for task-authority review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentReviewResourceV1 {
    pub task_id: TaskId,
    pub proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    pub proposal: IntentHypothesisSetProposalV1,
    pub requests_id: Option<ContentId<IntentDecisionRequestBatchArtifact>>,
    pub requests: Vec<IntentReviewRequestResourceV1>,
}

/// One exact decision request with the identity required by a later selection command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentReviewRequestResourceV1 {
    pub request_id: ContentId<UserIntentDecisionRequestArtifact>,
    pub request: UserIntentDecisionRequestV1,
}

/// Mutation and query requests accepted by the current App API.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "request", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CairnRequestV1 {
    BeginArchiveUpload {
        archive: TaskArchiveManifestV1,
    },
    PutArchiveChunk {
        offset: u64,
        #[serde(with = "canonical_base64")]
        bytes: Vec<u8>,
    },
    SubmitTask {
        command_id: CommandId,
        task_id: TaskId,
        submission: TaskSubmissionV1,
    },
    ListTasks,
    GetTask {
        task_id: TaskId,
    },
    GetTaskProgress {
        task_id: TaskId,
        after_sequence: Option<EventSequence>,
    },
    CancelTask {
        command_id: CommandId,
        task_id: TaskId,
    },
    GetIntentReview {
        task_id: TaskId,
    },
    SelectIntentHypothesis {
        command_id: CommandId,
        task_id: TaskId,
        request_id: ContentId<UserIntentDecisionRequestArtifact>,
        hypothesis: SirHypothesisId,
        authority_scope: UserIntentAuthorityScopeV1,
    },
    KeepIntentUnknown {
        command_id: CommandId,
        task_id: TaskId,
        request_id: ContentId<UserIntentDecisionRequestArtifact>,
        authority_scope: UserIntentAuthorityScopeV1,
    },
    ProvideIntentClaim {
        command_id: CommandId,
        task_id: TaskId,
        request_id: ContentId<UserIntentDecisionRequestArtifact>,
        authority_scope: UserIntentAuthorityScopeV1,
        claim: UserProvidedIntentClaimV1,
    },
    ReconcileIntentAdmission {
        command_id: CommandId,
        task_id: TaskId,
    },
}

/// Stable public failure classification; internal diagnostics remain server-side.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppApiErrorCodeV1 {
    #[error("invalid request")]
    InvalidRequest,
    #[error("task not found")]
    TaskNotFound,
    #[error("request conflicts with durable state")]
    Conflict,
    #[error("task is not ready for this command")]
    NotReady,
    #[error("server could not complete the request")]
    Internal,
}

/// One strict App API response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "response", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CairnResponseV1 {
    ArchiveUpload {
        received: u64,
    },
    Task {
        task: TaskResourceV1,
    },
    Mutation {
        command_id: CommandId,
        task: TaskResourceV1,
    },
    Tasks {
        tasks: Vec<TaskResourceV1>,
    },
    Progress {
        page: TaskProgressPageV1,
    },
    IntentReview {
        review: Box<IntentReviewResourceV1>,
    },
    Error {
        code: AppApiErrorCodeV1,
    },
}

/// Local App API endpoint used by the reference client.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CairnClientConfigV1 {
    pub schema_version: u16,
    pub unix_socket: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CairnClientConfigWire {
    schema_version: u16,
    unix_socket: PathBuf,
}

impl CairnClientConfigV1 {
    /// Validates the current client configuration.
    ///
    /// # Errors
    ///
    /// Rejects a non-V1 configuration or a non-absolute Unix-socket path.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.schema_version != SCHEMA_V1 || !self.unix_socket.is_absolute() {
            return Err(SdkError::Invalid("client configuration"));
        }
        Ok(())
    }
}

impl TryFrom<CairnClientConfigWire> for CairnClientConfigV1 {
    type Error = SdkError;

    fn try_from(wire: CairnClientConfigWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            unix_socket: wire.unix_socket,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CairnClientConfigV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CairnClientConfigWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Transport-neutral client operation used by CLI, TUI, and test clients.
pub trait CairnClient {
    fn exchange(
        &self,
        request: CairnRequestV1,
    ) -> impl Future<Output = Result<CairnResponseV1, SdkError>> + Send;

    /// Uploads one source archive in bounded chunks and submits it as a task.
    ///
    /// The upload and the submission share one connection because that is what binds them. The
    /// staged bytes belong to this exchange and to no other, and the server releases them the
    /// moment the client goes away, so this cannot be expressed as independent exchanges.
    fn submit_task(
        &self,
        command_id: CommandId,
        task_id: TaskId,
        archive: &[u8],
        submission: TaskSubmissionV1,
    ) -> impl Future<Output = Result<CairnResponseV1, SdkError>> + Send;
}

/// Current local Unix-socket SDK adapter.
pub struct UnixCairnClient {
    config: CairnClientConfigV1,
}

impl UnixCairnClient {
    /// Creates a validated client.
    ///
    /// # Errors
    ///
    /// Returns the client-configuration validation error.
    pub fn new(config: CairnClientConfigV1) -> Result<Self, SdkError> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl CairnClient for UnixCairnClient {
    async fn exchange(&self, request: CairnRequestV1) -> Result<CairnResponseV1, SdkError> {
        let mut stream = tokio::net::UnixStream::connect(&self.config.unix_socket).await?;
        write_frame(&mut stream, &request).await?;
        read_frame(&mut stream).await
    }

    async fn submit_task(
        &self,
        command_id: CommandId,
        task_id: TaskId,
        archive: &[u8],
        submission: TaskSubmissionV1,
    ) -> Result<CairnResponseV1, SdkError> {
        let manifest = TaskArchiveManifestV1::describing(archive)?;
        let mut stream = tokio::net::UnixStream::connect(&self.config.unix_socket).await?;
        let response = exchange_on(
            &mut stream,
            &CairnRequestV1::BeginArchiveUpload { archive: manifest },
        )
        .await?;
        if let Some(refusal) = upload_refusal(&response, 0)? {
            return Ok(refusal);
        }
        let mut sent: u64 = 0;
        for chunk in archive.chunks(MAX_ARCHIVE_CHUNK_BYTES) {
            let response = exchange_on(
                &mut stream,
                &CairnRequestV1::PutArchiveChunk {
                    offset: sent,
                    bytes: chunk.to_vec(),
                },
            )
            .await?;
            sent += u64::try_from(chunk.len()).map_err(|_| SdkError::FrameLimit)?;
            if let Some(refusal) = upload_refusal(&response, sent)? {
                return Ok(refusal);
            }
        }
        exchange_on(
            &mut stream,
            &CairnRequestV1::SubmitTask {
                command_id,
                task_id,
                submission,
            },
        )
        .await
    }
}

async fn exchange_on<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    request: &CairnRequestV1,
) -> Result<CairnResponseV1, SdkError> {
    write_frame(stream, request).await?;
    read_frame(stream).await
}

/// Compares the server's byte accounting with the client's, passing a refusal back untouched.
///
/// A server that stopped accepting the upload has already said why. Reporting a local accounting
/// mismatch instead would replace that reason with a less specific one.
fn upload_refusal(
    response: &CairnResponseV1,
    expected: u64,
) -> Result<Option<CairnResponseV1>, SdkError> {
    match response {
        CairnResponseV1::ArchiveUpload { received } if *received == expected => Ok(None),
        CairnResponseV1::ArchiveUpload { .. } => {
            Err(SdkError::Invalid("archive upload accounting"))
        }
        other => Ok(Some(other.clone())),
    }
}

/// Writes one canonical bounded length-prefixed frame.
///
/// # Errors
///
/// Returns a codec, frame-limit, or transport error.
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), SdkError> {
    let bytes = cairn_codec::to_vec(value)?;
    let length = u32::try_from(bytes.len()).map_err(|_| SdkError::FrameLimit)?;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(SdkError::FrameLimit);
    }
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads and strict-decodes one canonical bounded length-prefixed frame.
///
/// # Errors
///
/// Returns a frame-limit, transport, codec, or non-canonical-input error, including a stream that
/// ended where a frame was required.
pub async fn read_frame<R: AsyncRead + Unpin, T>(reader: &mut R) -> Result<T, SdkError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    read_optional_frame(reader)
        .await?
        .ok_or_else(|| SdkError::Io(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)))
}

/// Reads one frame, or reports a stream that ended cleanly between frames.
///
/// A connection carrying more than one request has to be able to end. Ending between frames is how
/// a client says it has nothing further; ending inside one is a truncated frame and stays an error.
///
/// # Errors
///
/// Returns a frame-limit, transport, codec, or non-canonical-input error.
pub async fn read_optional_frame<R: AsyncRead + Unpin, T>(
    reader: &mut R,
) -> Result<Option<T>, SdkError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let mut header = [0_u8; 4];
    if reader.read(&mut header[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut header[1..]).await?;
    let length = u32::from_be_bytes(header);
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(SdkError::FrameLimit);
    }
    let mut bytes = vec![0_u8; usize::try_from(length).map_err(|_| SdkError::FrameLimit)?];
    reader.read_exact(&mut bytes).await?;
    let value: T = cairn_codec::from_slice(&bytes)?;
    if cairn_codec::to_vec(&value)? != bytes {
        return Err(SdkError::NonCanonical);
    }
    Ok(Some(value))
}

/// SDK configuration, codec, framing, or transport failure.
#[derive(Debug, Error)]
pub enum SdkError {
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("App API frame exceeds the current bound")]
    FrameLimit,
    #[error("App API frame is not canonical current V1")]
    NonCanonical,
    #[error(transparent)]
    Codec(#[from] cairn_codec::CodecError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A task identity cannot be used as mutation authority.
///
/// ```compile_fail
/// use cairn_protocol::{CommandId, TaskId};
/// fn require_command(_: CommandId) {}
/// require_command(TaskId::new());
/// ```
fn _identity_boundary() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery_request() -> IntentRecoveryRequestV1 {
        serde_json::from_str(include_str!(
            "../../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json"
        ))
        .expect("recovery request")
    }

    // The manifest is derived from the bytes rather than declared alongside them, so a client
    // cannot describe an archive it does not hold. What may be inside those bytes stays the
    // server's decision, because that is where the limits are configured.
    #[test]
    fn a_manifest_describes_the_exact_archive_and_refuses_an_empty_one() {
        assert!(TaskArchiveManifestV1::describing(&[]).is_err());
        let manifest = TaskArchiveManifestV1::describing(b"not-really-gzip").expect("describes");
        assert_eq!(manifest.byte_len(), 15);
        assert_eq!(manifest.digest(), BlobDigest::derive(b"not-really-gzip"));
        let document = serde_json::to_value(manifest).expect("document");
        let decoded: TaskArchiveManifestV1 = serde_json::from_value(document).expect("round trip");
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn a_submission_no_longer_carries_the_archive() {
        let submission = TaskSubmissionV1::new(recovery_request());
        let bytes = cairn_codec::to_vec(&submission).expect("canonical submission");
        assert!(
            !String::from_utf8(bytes.clone())
                .expect("JSON")
                .contains("archive")
        );
        let decoded: TaskSubmissionV1 = cairn_codec::from_slice(&bytes).expect("round trip");
        assert_eq!(decoded, submission);
    }

    // The chunk size is derived from the frame limit, so it is the derivation that has to hold: a
    // client chunking at the published size must be able to send a full one.
    #[tokio::test]
    async fn a_full_size_chunk_still_fits_one_frame() {
        let request = CairnRequestV1::PutArchiveChunk {
            offset: 0,
            bytes: vec![0xA5; MAX_ARCHIVE_CHUNK_BYTES],
        };
        let mut frame = Vec::new();
        write_frame(&mut frame, &request)
            .await
            .expect("a full chunk fits one frame");
    }

    #[tokio::test]
    async fn a_stream_ending_between_frames_is_not_an_error_but_a_truncated_one_is() {
        let mut ended: &[u8] = &[];
        assert!(matches!(
            read_optional_frame::<_, CairnRequestV1>(&mut ended).await,
            Ok(None)
        ));
        let mut truncated: &[u8] = &[0, 0];
        assert!(matches!(
            read_optional_frame::<_, CairnRequestV1>(&mut truncated).await,
            Err(SdkError::Io(_))
        ));
    }

    #[test]
    fn wire_decode_reruns_current_v1_and_path_invariants() {
        assert!(
            serde_json::from_value::<CairnClientConfigV1>(serde_json::json!({
                "schema_version": 2,
                "unix_socket": "/tmp/cairn.sock"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CairnClientConfigV1>(serde_json::json!({
                "schema_version": 1,
                "unix_socket": "relative.sock"
            }))
            .is_err()
        );

        let task = TaskResourceV1::new(
            TaskId::new(),
            EventSequence::new(1).expect("sequence"),
            TaskPhaseV1::Submitted,
            None,
        );
        let mut value = serde_json::to_value(task).expect("task JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<TaskResourceV1>(value).is_err());
    }

    #[test]
    fn intent_selection_wire_carries_only_exact_request_option_and_scope() {
        let request = CairnRequestV1::SelectIntentHypothesis {
            command_id: CommandId::new(),
            task_id: TaskId::new(),
            request_id: ContentId::<UserIntentDecisionRequestArtifact>::derive(b"request")
                .expect("request identity"),
            hypothesis: SirHypothesisId::new("preserve-exact-semantics").expect("hypothesis"),
            authority_scope: UserIntentAuthorityScopeV1::new(vec![
                cairn_migration::SirCallerClaimId::new("caller-contract").expect("caller claim"),
            ])
            .expect("authority scope"),
        };
        let bytes = cairn_codec::to_vec(&request).expect("canonical request");
        assert_eq!(
            cairn_codec::from_slice::<CairnRequestV1>(&bytes).expect("strict request"),
            request
        );
        let text = String::from_utf8(bytes).expect("JSON");
        assert!(!text.contains("authority_grant"));
        assert!(!text.contains("authoritative_claim"));
        assert!(!text.contains("intent_authority_subject"));
    }

    #[tokio::test]
    async fn frame_decode_rejects_noncanonical_json() {
        let bytes = br#"{ "request": "list-tasks" }"#;
        let mut frame = Vec::new();
        frame.extend_from_slice(&u32::try_from(bytes.len()).expect("length").to_be_bytes());
        frame.extend_from_slice(bytes);
        assert!(matches!(
            read_frame::<_, CairnRequestV1>(&mut frame.as_slice()).await,
            Err(SdkError::Codec(_) | SdkError::NonCanonical)
        ));
    }
}
