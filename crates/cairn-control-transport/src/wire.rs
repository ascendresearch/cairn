use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use cairn_execution::{
    AssignmentMaterialChunk, AssignmentMaterialChunkRequest, AssignmentMaterialChunkSize,
    AssignmentMaterialKind, ControlFrame, ControllerControlMessage, WorkerAvailability,
    WorkerControlMessage, WorkerHello, WorkerProtocolVersion, WorkerResourceObservation,
    WorkerResourceObservationArtifact,
};
use cairn_protocol::{ContentId, ControlConnectionId, ControlMessageId, ObservedAtUnixMillis};

use crate::TransportError;

/// Positive configurable WebSocket message bound. `None` disables it explicitly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct TransportMessageByteLimit(u64);

impl TransportMessageByteLimit {
    /// Creates an enabled bound.
    ///
    /// # Errors
    ///
    /// Zero is rejected because the disabled state is represented by `None`.
    pub fn new(value: u64) -> Result<Self, TransportError> {
        if value == 0 {
            Err(TransportError::ZeroMessageLimit)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the configured maximum.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for TransportMessageByteLimit {
    type Error = TransportError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TransportMessageByteLimit> for u64 {
    fn from(value: TransportMessageByteLimit) -> Self {
        value.0
    }
}

/// Configurable WebSocket wire policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransportPolicy {
    /// Maximum canonical JSON bytes per WebSocket message, or `None` to disable this budget.
    pub message_byte_limit: Option<TransportMessageByteLimit>,
}

impl TransportPolicy {
    pub(crate) fn websocket_config(
        self,
    ) -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        let maximum = self
            .message_byte_limit
            .and_then(|value| usize::try_from(value.get()).ok());
        let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        config.max_message_size = maximum;
        config.max_frame_size = maximum;
        config
    }
}

/// First and subsequent worker-to-controller WebSocket messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WorkerWireMessage {
    /// First message after mTLS and WebSocket handshake.
    Hello {
        hello: Box<WorkerHello>,
        availability: WorkerAvailability,
    },
    /// Ephemeral liveness/capacity observation.
    Heartbeat { availability: WorkerAvailability },
    /// Independently refreshable quantitative resource observation.
    ResourcesObserved {
        observation: Box<WorkerResourceObservation>,
    },
    /// Durable worker outbox delivery or acknowledgement-only frame.
    Control {
        frame: Box<ControlFrame<WorkerControlMessage>>,
    },
    /// Ephemeral resumable range request authorized by one still-pending durable offer.
    MaterialChunkRequest {
        request: AssignmentMaterialChunkRequest,
    },
}

/// Stable machine-readable handshake rejection classification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerRejectCode {
    /// Certificate enrollment does not name the hello worker.
    IdentityMismatch,
    /// Worker protocol version is not supported.
    UnsupportedProtocol,
    /// Hello/profile/availability validation or registration failed.
    InvalidHello,
    /// Controller cannot safely open the durable session.
    ControllerUnavailable,
}

/// Controller-to-worker WebSocket messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ControllerWireMessage {
    /// Successful hello negotiation. No control frame may precede it.
    Welcome {
        connection_id: ControlConnectionId,
        protocol_version: WorkerProtocolVersion,
        accepted_at: ObservedAtUnixMillis,
    },
    /// Ephemeral acknowledgement sent only after the controller accepts a heartbeat.
    HeartbeatAccepted { accepted_at: ObservedAtUnixMillis },
    /// Acknowledges durable admission of one exact resource observation.
    ResourcesAccepted {
        accepted_at: ObservedAtUnixMillis,
        observation_id: ContentId<WorkerResourceObservationArtifact>,
    },
    /// Durable controller outbox delivery or acknowledgement-only frame.
    Control {
        frame: Box<ControlFrame<ControllerControlMessage>>,
    },
    /// Ephemeral chunk bytes whose final authority is the offer's typed content identity.
    MaterialChunk { chunk: AssignmentMaterialChunk },
    /// Bounded public diagnostic followed by connection close.
    Reject {
        code: ControllerRejectCode,
        diagnostic: String,
    },
}

/// Sends one canonical binary JSON WebSocket message.
///
/// # Errors
///
/// Returns an error for canonical encoding, enabled byte-bound, WebSocket, or I/O failure.
pub async fn write_wire_message<S, T>(
    socket: &mut WebSocketStream<S>,
    value: &T,
    policy: TransportPolicy,
) -> Result<(), TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes =
        cairn_codec::to_vec(value).map_err(|error| TransportError::Codec(error.to_string()))?;
    enforce_limit(bytes.len(), policy)?;
    socket
        .send(Message::Binary(bytes.into()))
        .await
        .map_err(|error| TransportError::WebSocket(error.to_string()))
}

/// Reads the next canonical binary JSON WebSocket message, transparently ignoring ping/pong.
///
/// # Errors
///
/// Returns an error for close, text/unsupported messages, enabled byte-bound, strict decoding, or
/// WebSocket I/O failure.
pub async fn read_wire_message<S, T>(
    socket: &mut WebSocketStream<S>,
    policy: TransportPolicy,
) -> Result<T, TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: DeserializeOwned,
{
    loop {
        let message = socket
            .next()
            .await
            .ok_or(TransportError::Closed)?
            .map_err(|error| TransportError::WebSocket(error.to_string()))?;
        match message {
            Message::Binary(bytes) => {
                enforce_limit(bytes.len(), policy)?;
                return cairn_codec::from_slice(&bytes)
                    .map_err(|error| TransportError::Codec(error.to_string()));
            }
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => return Err(TransportError::Closed),
            Message::Text(_) => return Err(TransportError::UnsupportedMessage("text")),
            Message::Frame(_) => return Err(TransportError::UnsupportedMessage("raw frame")),
        }
    }
}

/// Validates the exact canonical-wire upper bound for a configured raw material chunk size.
///
/// # Errors
///
/// Returns [`TransportError::MessageTooLarge`] when base64 expansion plus the largest fixed
/// response envelope exceeds an enabled WebSocket message limit.
pub fn validate_material_chunk_wire_size(
    policy: TransportPolicy,
    chunk_size: AssignmentMaterialChunkSize,
) -> Result<(), TransportError> {
    let Some(limit) = policy.message_byte_limit else {
        return Ok(());
    };
    let empty = ControllerWireMessage::MaterialChunk {
        chunk: AssignmentMaterialChunk {
            offer_message_id: ControlMessageId::new(),
            kind: AssignmentMaterialKind::ExecutionEnvironment,
            offset: u64::MAX,
            total_byte_len: u64::MAX,
            bytes: Vec::new(),
        },
    };
    let envelope = u64::try_from(
        cairn_codec::to_vec(&empty)
            .map_err(|error| TransportError::Codec(error.to_string()))?
            .len(),
    )
    .unwrap_or(u64::MAX);
    let full_groups = chunk_size.get() / 3;
    let remainder = match chunk_size.get() % 3 {
        0 => 0,
        1 => 2,
        2 => 3,
        _ => unreachable!("modulo three"),
    };
    let observed = full_groups
        .checked_mul(4)
        .and_then(|encoded| encoded.checked_add(remainder))
        .and_then(|encoded| encoded.checked_add(envelope))
        .unwrap_or(u64::MAX);
    if observed > limit.get() {
        return Err(TransportError::MessageTooLarge {
            observed,
            limit: limit.get(),
        });
    }
    Ok(())
}

fn enforce_limit(byte_len: usize, policy: TransportPolicy) -> Result<(), TransportError> {
    let observed = u64::try_from(byte_len).unwrap_or(u64::MAX);
    match policy.message_byte_limit {
        Some(limit) if observed > limit.get() => Err(TransportError::MessageTooLarge {
            observed,
            limit: limit.get(),
        }),
        Some(_) | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use cairn_execution::AssignmentMaterialChunkSize;

    use super::{TransportMessageByteLimit, TransportPolicy, validate_material_chunk_wire_size};

    #[test]
    fn material_chunk_base64_expansion_is_checked_before_session_start() {
        let chunk = AssignmentMaterialChunkSize::new(24 * 1024).expect("chunk");
        assert!(
            validate_material_chunk_wire_size(
                TransportPolicy {
                    message_byte_limit: Some(
                        TransportMessageByteLimit::new(16 * 1024).expect("wire limit"),
                    ),
                },
                chunk,
            )
            .is_err()
        );
        validate_material_chunk_wire_size(TransportPolicy::default(), chunk)
            .expect("disabled wire limit");
    }
}
