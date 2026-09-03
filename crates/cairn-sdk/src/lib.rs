//! Strict client resources shared by Cairn clients and the App Server boundary.

use std::{future::Future, path::PathBuf};

use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentHypothesisSetProposalV1, IntentRecoveryRequestV1,
    SirHypothesisId, SirIntentHypothesisSetProposalArtifact, UserIntentAuthorityScopeV1,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1, UserProvidedIntentClaimV1,
};
use cairn_protocol::{CommandId, ContentId, EventId, EventSequence, ObservedAtUnixMillis, TaskId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const SCHEMA_V1: u16 = 1;
const MAX_FRAME_BYTES: u32 = 4 * 1024 * 1024;

/// Immutable task bytes plus the caller-owned recovery declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskSubmissionV1 {
    schema_version: u16,
    /// Gzip-compressed tar archive of the task source tree.
    ///
    /// Content bounds are the server's to apply, because they are configured there. This type
    /// carries the bytes; the transport frame limit is what stops an unbounded request.
    #[serde(with = "canonical_base64")]
    archive: Vec<u8>,
    recovery_request: IntentRecoveryRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskSubmissionWire {
    schema_version: u16,
    #[serde(with = "canonical_base64")]
    archive: Vec<u8>,
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
    /// Creates one submission carrying a source archive.
    ///
    /// # Errors
    ///
    /// Rejects an empty archive. What the archive may contain is the server's decision, since the
    /// bounds are configured there and a client cannot be trusted to apply them.
    pub fn new(
        archive: Vec<u8>,
        recovery_request: IntentRecoveryRequestV1,
    ) -> Result<Self, SdkError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            archive,
            recovery_request,
        };
        value.validate()?;
        Ok(value)
    }

    /// Returns the submitted source archive.
    #[must_use]
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }

    #[must_use]
    pub const fn recovery_request(&self) -> &IntentRecoveryRequestV1 {
        &self.recovery_request
    }

    fn validate(&self) -> Result<(), SdkError> {
        if self.schema_version != SCHEMA_V1 || self.archive.is_empty() {
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
            archive: wire.archive,
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
/// Returns a frame-limit, transport, codec, or non-canonical-input error.
pub async fn read_frame<R: AsyncRead + Unpin, T>(reader: &mut R) -> Result<T, SdkError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).await?;
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
    Ok(value)
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

    // The wire type carries bytes and says so. What may be inside them is decided where the
    // limits are configured, which is the server; a client applying its own would be a second
    // policy that the first one has to agree with.
    #[test]
    fn a_submission_carries_an_archive_and_refuses_an_empty_one() {
        assert!(TaskSubmissionV1::new(Vec::new(), recovery_request()).is_err());
        let submission = TaskSubmissionV1::new(b"not-really-gzip".to_vec(), recovery_request())
            .expect("carries");
        let document = serde_json::to_value(&submission).expect("document");
        let decoded: TaskSubmissionV1 = serde_json::from_value(document).expect("round trip");
        assert_eq!(decoded.archive(), b"not-really-gzip");
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
