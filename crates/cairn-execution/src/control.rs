//! Versioned JSON worker-control contract, durable outboxes, and worker-local admission journal.
//!
//! Logical message identity is durable and connection sequence is deliberately ephemeral. Both
//! sides record a delivery mapping before returning a frame to a transport adapter, then compact
//! logical messages only after a bounded, non-regressing cumulative acknowledgement.

use std::collections::{BTreeMap, BTreeSet};

use cairn_protocol::{
    AggregateId, AggregateKind, AttemptId, CommandId, ContentId, ControlConnectionId,
    ControlMessageId, ControlSequence, EventId, ObservedAtUnixMillis, SchemaName, SchemaVersion,
    StreamRevision, WorkerId,
};
use cairn_record::{
    ContentStore, EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    AcceptedExecutionAssignment, AssignmentBinding, AssignmentControlError, AssignmentLeaseRecord,
    ExecutionAssignmentState, ExecutionCompletion, ExecutionCoordinatorError, ExecutionInput,
    ExecutionJob, Executor, ExecutorFailureClass, JobContract, JobContractArtifact,
    LeasedExecutionAssignment, ReconciledExecutionResult, RegisteredWorkerSession,
    WorkerSessionTimeoutMillis, accept_assignment, reconcile_execution_result,
    recover_execution_assignment,
};

const CONTROLLER_ENQUEUED: &str = "control.controller-message-enqueued";
const CONTROLLER_DELIVERED: &str = "control.controller-delivery-recorded";
const CONTROLLER_ACKED: &str = "control.controller-acknowledged";
const WORKER_ADMITTED: &str = "control.worker-assignment-admitted";
const WORKER_STARTED: &str = "control.worker-execution-started";
const WORKER_RESULT: &str = "control.worker-result-enqueued";
const WORKER_DELIVERED: &str = "control.worker-delivery-recorded";
const WORKER_ACKED: &str = "control.worker-acknowledged";

/// Positive configurable frame-size bound. `None` in [`ControlFramePolicy`] disables this budget.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ControlFrameByteLimit(u64);

impl ControlFrameByteLimit {
    /// Creates an enabled byte bound.
    ///
    /// # Errors
    ///
    /// Zero is rejected because disabling is represented explicitly by `None`.
    pub fn new(value: u64) -> Result<Self, ControlProtocolError> {
        if value == 0 {
            Err(ControlProtocolError::ZeroFrameLimit)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the configured byte count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for ControlFrameByteLimit {
    type Error = ControlProtocolError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ControlFrameByteLimit> for u64 {
    fn from(value: ControlFrameByteLimit) -> Self {
        value.0
    }
}

/// Transport codec policy supplied by configuration rather than hidden constants.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFramePolicy {
    /// Maximum canonical JSON frame size, or `None` to disable this transport budget.
    pub byte_limit: Option<ControlFrameByteLimit>,
}

/// Durable controller-to-worker payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum ControllerControlMessage {
    /// Offers an immutable assignment for durable local admission.
    AssignmentOffer {
        binding: AssignmentBinding,
        lease_expires_at: ObservedAtUnixMillis,
        contract: Box<JobContract>,
    },
    /// Grants execution only after the controller has committed the attempt-start fact.
    StartExecution { binding: AssignmentBinding },
}

impl ControllerControlMessage {
    fn binding(&self) -> &AssignmentBinding {
        match self {
            Self::AssignmentOffer { binding, .. } | Self::StartExecution { binding } => binding,
        }
    }
}

/// Durable worker-to-controller payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "message")]
pub enum WorkerControlMessage {
    /// Confirms that the exact offer was committed to the worker journal.
    AssignmentAccepted { binding: AssignmentBinding },
    /// Returns a terminal observation for controller-side validation and reconciliation.
    ExecutionResult {
        binding: AssignmentBinding,
        result: ReconciledExecutionResult,
    },
}

impl WorkerControlMessage {
    fn binding(&self) -> &AssignmentBinding {
        match self {
            Self::AssignmentAccepted { binding } | Self::ExecutionResult { binding, .. } => binding,
        }
    }
}

/// Stable logical message retained independently of connection delivery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DurableControlMessage<T> {
    /// Cross-connection idempotency identity.
    pub message_id: ControlMessageId,
    /// Typed protocol payload.
    pub payload: T,
}

/// One connection-local frame. Acknowledgement zero is represented as `None`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFrame<T> {
    /// Negotiated worker protocol version.
    pub protocol_version: crate::WorkerProtocolVersion,
    /// Exact connection on which the sequence has meaning.
    pub connection_id: ControlConnectionId,
    /// One-based connection-local delivery position.
    pub sequence: ControlSequence,
    /// Cumulative peer sequence successfully processed through the domain boundary.
    pub acknowledges_peer_through: Option<ControlSequence>,
    /// Stable logical message, or `None` for an acknowledgement-only frame.
    pub message: Option<DurableControlMessage<T>>,
}

/// Stateful validation for one inbound half of a live connection.
#[derive(Clone, Debug)]
pub struct InboundControlSession {
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    last_received: Option<ControlSequence>,
    last_peer_ack: Option<ControlSequence>,
}

impl InboundControlSession {
    /// Opens an empty inbound cursor for an already authenticated/negotiated connection.
    #[must_use]
    pub const fn new(
        protocol_version: crate::WorkerProtocolVersion,
        connection_id: ControlConnectionId,
    ) -> Self {
        Self {
            protocol_version,
            connection_id,
            last_received: None,
            last_peer_ack: None,
        }
    }

    /// Validates sequence, connection, protocol, and cumulative acknowledgement bounds.
    ///
    /// `highest_peer_sequence_sent` is the local outbox's durable delivery watermark.
    ///
    /// # Errors
    ///
    /// Rejects gaps/duplicates, wrong connection or version, acknowledgement regression, and an
    /// acknowledgement beyond a frame that was actually recorded as sent.
    pub fn accept<T>(
        &mut self,
        frame: &ControlFrame<T>,
        highest_peer_sequence_sent: Option<ControlSequence>,
    ) -> Result<(), ControlProtocolError> {
        if frame.protocol_version != self.protocol_version {
            return Err(ControlProtocolError::ProtocolVersionMismatch);
        }
        if frame.connection_id != self.connection_id {
            return Err(ControlProtocolError::ConnectionMismatch);
        }
        if frame.message.is_none() && frame.acknowledges_peer_through.is_none() {
            return Err(ControlProtocolError::EmptyFrame);
        }
        let expected = self.last_received.map_or(1, |value| value.get() + 1);
        if frame.sequence.get() != expected {
            return Err(ControlProtocolError::UnexpectedSequence {
                expected,
                observed: frame.sequence.get(),
            });
        }
        validate_ack(
            self.last_peer_ack,
            frame.acknowledges_peer_through,
            highest_peer_sequence_sent,
        )?;
        self.last_received = Some(frame.sequence);
        self.last_peer_ack = frame.acknowledges_peer_through;
        Ok(())
    }

    /// Returns the highest inbound sequence processed on this connection.
    #[must_use]
    pub const fn received_through(&self) -> Option<ControlSequence> {
        self.last_received
    }
}

/// Durable enqueue outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlEnqueueOutcome {
    /// A new logical message was committed.
    Enqueued,
    /// The exact logical message was already durable.
    AlreadyEnqueued,
}

/// Worker admission outcome for duplicate at-least-once delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerAdmissionOutcome {
    /// The offer and acceptance response committed together.
    Admitted,
    /// The exact offer was already in the journal and will reuse its original response.
    AlreadyAdmitted,
}

/// Controller-side result reconciliation outcome.
#[derive(Debug)]
pub enum WorkerResultReconciliation {
    /// A new authoritative terminal execution fact committed.
    Published(Box<ExecutionCompletion>),
    /// The exact assignment was already terminal; the duplicate worker result is safe to ack.
    AlreadyTerminal,
}

/// One-shot worker-local authority created only after a start command is durably journaled.
pub struct WorkerExecutionAuthority {
    stream: StreamId,
    revision: StreamRevision,
    last_event_id: EventId,
    binding: AssignmentBinding,
    contract: JobContract,
}

/// Worker-control and journal failure.
#[derive(Debug, Error)]
pub enum ControlProtocolError {
    /// Event persistence failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Assignment projection/reconciliation failed.
    #[error(transparent)]
    Assignment(#[from] AssignmentControlError),
    /// Execution result validation/publication failed.
    #[error(transparent)]
    Execution(#[from] ExecutionCoordinatorError),
    /// Frame JSON failed strict canonical encoding or decoding.
    #[error("worker-control JSON failed: {0}")]
    Codec(String),
    /// Enabled frame bounds must be positive.
    #[error("worker-control frame byte limit must be positive or disabled")]
    ZeroFrameLimit,
    /// A frame exceeded the configured budget.
    #[error("worker-control frame is {observed} bytes, exceeding configured limit {limit}")]
    FrameTooLarge { observed: u64, limit: u64 },
    /// Frame protocol does not match the negotiated version.
    #[error("worker-control protocol version changed within a connection")]
    ProtocolVersionMismatch,
    /// Connection-local frame was delivered on another connection.
    #[error("worker-control connection identity mismatch")]
    ConnectionMismatch,
    /// Connection sequence was duplicated, skipped, or reordered.
    #[error("expected worker-control sequence {expected}, observed {observed}")]
    UnexpectedSequence { expected: u64, observed: u64 },
    /// Cumulative acknowledgement moved backwards.
    #[error("worker-control cumulative acknowledgement regressed")]
    AcknowledgementRegressed,
    /// Cumulative acknowledgement cites a frame never recorded as delivered.
    #[error("worker-control acknowledgement exceeds the highest delivered sequence")]
    AcknowledgementOutOfBounds,
    /// A frame with neither a logical message nor an acknowledgement carries no meaning.
    #[error("worker-control frame contains neither message nor acknowledgement")]
    EmptyFrame,
    /// A logical message identity was reused with different content.
    #[error("worker-control logical message identity has conflicting content")]
    ConflictingMessage,
    /// Message identity or worker/attempt/contract binding does not match the payload/state.
    #[error("worker-control message does not match its durable assignment binding")]
    BindingMismatch,
    /// Durable event history is incomplete or contradictory.
    #[error("invalid worker-control history: {0}")]
    InvalidHistory(String),
    /// A requested worker transition cannot safely run from the journaled phase.
    #[error("worker-control transition is invalid from its durable phase")]
    InvalidTransition,
    /// A start was already journaled but no terminal result exists; execution cannot be repeated.
    #[error("worker attempt started without a terminal result and requires reconciliation")]
    WorkerExecutionInDoubt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnqueuedControllerPayload {
    worker_id: WorkerId,
    message: DurableControlMessage<ControllerControlMessage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    connection_id: ControlConnectionId,
    sequence: ControlSequence,
    message_id: Option<ControlMessageId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DeliveriesPayload {
    worker_id: WorkerId,
    deliveries: Vec<Delivery>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AcknowledgedPayload {
    worker_id: WorkerId,
    connection_id: ControlConnectionId,
    acknowledged_through: ControlSequence,
    message_ids: Vec<ControlMessageId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AdmittedPayload {
    worker_id: WorkerId,
    offer_message_id: ControlMessageId,
    binding: AssignmentBinding,
    lease_expires_at: ObservedAtUnixMillis,
    contract: JobContract,
    response: DurableControlMessage<WorkerControlMessage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerStartedPayload {
    worker_id: WorkerId,
    start_message_id: ControlMessageId,
    binding: AssignmentBinding,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerResultPayload {
    worker_id: WorkerId,
    binding: AssignmentBinding,
    response: DurableControlMessage<WorkerControlMessage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalPhase {
    Admitted,
    Started,
    Terminal,
}

#[derive(Clone)]
struct LocalAttempt {
    binding: AssignmentBinding,
    contract: JobContract,
    phase: LocalPhase,
}

#[derive(Clone)]
struct OutboxProjection<T> {
    pending: BTreeMap<ControlMessageId, T>,
    deliveries: BTreeMap<ControlConnectionId, BTreeMap<ControlSequence, Option<ControlMessageId>>>,
    acknowledged: BTreeMap<ControlConnectionId, ControlSequence>,
}

impl<T> Default for OutboxProjection<T> {
    fn default() -> Self {
        Self {
            pending: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            acknowledged: BTreeMap::new(),
        }
    }
}

struct ControllerProjection {
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    outbox: OutboxProjection<ControllerControlMessage>,
}

struct WorkerProjection {
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    attempts: BTreeMap<AttemptId, LocalAttempt>,
    outbox: OutboxProjection<WorkerControlMessage>,
}

/// Encodes one strict canonical JSON frame under a configurable/disable-able byte budget.
///
/// # Errors
///
/// Returns an error for codec failure or an enabled bound violation.
pub fn encode_control_frame<T: Serialize>(
    frame: &ControlFrame<T>,
    policy: ControlFramePolicy,
) -> Result<Vec<u8>, ControlProtocolError> {
    let bytes = cairn_codec::to_vec(frame)
        .map_err(|error| ControlProtocolError::Codec(error.to_string()))?;
    enforce_frame_limit(bytes.len(), policy)?;
    Ok(bytes)
}

/// Decodes one strict canonical JSON frame under a configurable/disable-able byte budget.
///
/// # Errors
///
/// Returns an error for an enabled bound violation, non-canonical JSON, or typed decode failure.
pub fn decode_control_frame<T: DeserializeOwned>(
    bytes: &[u8],
    policy: ControlFramePolicy,
) -> Result<ControlFrame<T>, ControlProtocolError> {
    enforce_frame_limit(bytes.len(), policy)?;
    cairn_codec::from_slice(bytes).map_err(|error| ControlProtocolError::Codec(error.to_string()))
}

/// Builds the offer payload from the exact persisted lease and immutable contract.
#[must_use]
pub fn assignment_offer_message(
    lease: &AssignmentLeaseRecord,
    contract: &JobContract,
) -> DurableControlMessage<ControllerControlMessage> {
    DurableControlMessage {
        message_id: lease.binding().offer_message_id(),
        payload: ControllerControlMessage::AssignmentOffer {
            binding: lease.binding().clone(),
            lease_expires_at: lease.expires_at(),
            contract: Box::new(contract.clone()),
        },
    }
}

/// Builds the stable start payload. It may be re-created after a controller restart because its
/// message identity was frozen in the assignment binding before the lease was sent.
#[must_use]
pub fn execution_start_message(
    lease: &AssignmentLeaseRecord,
) -> DurableControlMessage<ControllerControlMessage> {
    DurableControlMessage {
        message_id: lease.binding().start_message_id(),
        payload: ControllerControlMessage::StartExecution {
            binding: lease.binding().clone(),
        },
    }
}

/// Commits a controller logical message before any transport adapter may send it.
///
/// # Errors
///
/// Returns an error for an invalid binding, conflicting message identity, or persistence failure.
pub fn enqueue_controller_message<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    message: &DurableControlMessage<ControllerControlMessage>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControlEnqueueOutcome, ControlProtocolError> {
    validate_controller_message(worker_id, message)?;
    let stream = controller_stream(worker_id)?;
    let projection = project_controller(&events.read_stream(&stream, None)?, worker_id)?;
    if let Some(existing) = projection.outbox.pending.get(&message.message_id) {
        return if existing == &message.payload {
            Ok(ControlEnqueueOutcome::AlreadyEnqueued)
        } else {
            Err(ControlProtocolError::ConflictingMessage)
        };
    }
    let event = fact(
        CONTROLLER_ENQUEUED,
        projection.last_event_id,
        observed_at,
        &EnqueuedControllerPayload {
            worker_id,
            message: message.clone(),
        },
    )?;
    events.append(&stream, expected(projection.revision), command_id, &[event])?;
    Ok(ControlEnqueueOutcome::Enqueued)
}

/// Records fresh connection-local mappings for every pending controller message, then returns the
/// frames. Reusing the same connection does not redeliver; a new connection replays with sequence 1.
///
/// # Errors
///
/// Returns an error for corrupt outbox history, sequence overflow, or persistence failure.
pub fn deliver_controller_messages<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    acknowledges_worker_through: Option<ControlSequence>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<Vec<ControlFrame<ControllerControlMessage>>, ControlProtocolError> {
    let stream = controller_stream(worker_id)?;
    let projection = project_controller(&events.read_stream(&stream, None)?, worker_id)?;
    let existing = projection
        .outbox
        .deliveries
        .get(&connection_id)
        .cloned()
        .unwrap_or_default();
    let already = existing
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut next = next_sequence_value(existing.keys().next_back().copied())?;
    let mut frames = Vec::new();
    let mut deliveries = Vec::new();
    for (message_id, payload) in &projection.outbox.pending {
        if already.contains(message_id) {
            continue;
        }
        let sequence = ControlSequence::new(next)
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?;
        next = next.checked_add(1).ok_or_else(|| {
            ControlProtocolError::InvalidHistory("connection sequence overflow".into())
        })?;
        deliveries.push(Delivery {
            connection_id,
            sequence,
            message_id: Some(*message_id),
        });
        frames.push(ControlFrame {
            protocol_version,
            connection_id,
            sequence,
            acknowledges_peer_through: acknowledges_worker_through,
            message: Some(DurableControlMessage {
                message_id: *message_id,
                payload: payload.clone(),
            }),
        });
    }
    if !deliveries.is_empty() {
        let event = fact(
            CONTROLLER_DELIVERED,
            projection.last_event_id,
            observed_at,
            &DeliveriesPayload {
                worker_id,
                deliveries,
            },
        )?;
        events.append(&stream, expected(projection.revision), command_id, &[event])?;
    }
    Ok(frames)
}

/// Records and returns an acknowledgement-only controller frame. This allows the worker outbox to
/// close even when the controller has no durable logical message to piggyback the acknowledgement.
///
/// # Errors
///
/// Returns an error for corrupt connection history, sequence overflow, or persistence failure.
pub fn deliver_controller_acknowledgement<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    acknowledges_worker_through: ControlSequence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControlFrame<ControllerControlMessage>, ControlProtocolError> {
    let stream = controller_stream(worker_id)?;
    let projection = project_controller(&events.read_stream(&stream, None)?, worker_id)?;
    record_ack_delivery(
        events,
        &stream,
        projection.revision,
        projection.last_event_id,
        &projection.outbox,
        CONTROLLER_DELIVERED,
        worker_id,
        protocol_version,
        connection_id,
        acknowledges_worker_through,
        command_id,
        observed_at,
    )
}

/// Applies a valid cumulative worker acknowledgement and removes only logical messages mapped at
/// or below it on this connection.
///
/// # Errors
///
/// Returns an error for acknowledgement regression/bounds, corrupt history, or persistence failure.
pub fn acknowledge_controller_messages<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    connection_id: ControlConnectionId,
    acknowledged_through: ControlSequence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<usize, ControlProtocolError> {
    let stream = controller_stream(worker_id)?;
    let projection = project_controller(&events.read_stream(&stream, None)?, worker_id)?;
    let message_ids = acknowledged_ids(&projection.outbox, connection_id, acknowledged_through)?;
    if projection.outbox.acknowledged.get(&connection_id) == Some(&acknowledged_through) {
        return Ok(0);
    }
    let removed = message_ids
        .iter()
        .filter(|message_id| projection.outbox.pending.contains_key(message_id))
        .count();
    let event = fact(
        CONTROLLER_ACKED,
        projection.last_event_id,
        observed_at,
        &AcknowledgedPayload {
            worker_id,
            connection_id,
            acknowledged_through,
            message_ids,
        },
    )?;
    events.append(&stream, expected(projection.revision), command_id, &[event])?;
    Ok(removed)
}

/// Returns controller logical messages that have not yet been validly acknowledged.
///
/// # Errors
///
/// Returns an error when the durable controller outbox cannot be read or validated.
pub fn pending_controller_messages<E: EventStore>(
    events: &E,
    worker_id: WorkerId,
) -> Result<Vec<DurableControlMessage<ControllerControlMessage>>, ControlProtocolError> {
    let projection = project_controller(
        &events.read_stream(&controller_stream(worker_id)?, None)?,
        worker_id,
    )?;
    Ok(projection
        .outbox
        .pending
        .into_iter()
        .map(|(message_id, payload)| DurableControlMessage {
            message_id,
            payload,
        })
        .collect())
}

/// Validates a durable worker acceptance message against the exact one-shot lease authority before
/// advancing the controller assignment state.
///
/// # Errors
///
/// Returns an error for a mismatched binding, stale worker/lease, or persistence failure.
#[expect(
    clippy::too_many_arguments,
    reason = "persistence, authority, claimant, message, liveness, command, and time are independent"
)]
pub fn accept_worker_assignment<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    leased: LeasedExecutionAssignment,
    worker: &RegisteredWorkerSession,
    message: &DurableControlMessage<WorkerControlMessage>,
    session_timeout: WorkerSessionTimeoutMillis,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<AcceptedExecutionAssignment, ControlProtocolError> {
    let WorkerControlMessage::AssignmentAccepted { binding } = &message.payload else {
        return Err(ControlProtocolError::InvalidTransition);
    };
    if binding != leased.lease().binding()
        || binding.worker_id() != worker.worker_id()
        || binding.worker_incarnation_id() != worker.incarnation_id()
    {
        return Err(ControlProtocolError::BindingMismatch);
    }
    Ok(accept_assignment(
        events,
        content,
        leased,
        worker,
        session_timeout,
        command_id,
        observed_at,
    )?)
}

/// Durably admits an exact offer and enqueues its acceptance in the same event append.
///
/// # Errors
///
/// Returns an error for an expired/conflicting offer, invalid contract binding, or journal failure.
pub fn admit_worker_assignment<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    offer: &DurableControlMessage<ControllerControlMessage>,
    response_message_id: ControlMessageId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<WorkerAdmissionOutcome, ControlProtocolError> {
    validate_controller_message(worker_id, offer)?;
    let ControllerControlMessage::AssignmentOffer {
        binding,
        lease_expires_at,
        contract,
    } = &offer.payload
    else {
        return Err(ControlProtocolError::InvalidTransition);
    };
    validate_contract_binding(binding, contract)?;
    let stream = worker_stream(worker_id)?;
    let projection = project_worker(&events.read_stream(&stream, None)?, worker_id)?;
    if let Some(existing) = projection.attempts.get(&binding.attempt_id()) {
        return if existing.binding == *binding && existing.contract == **contract {
            Ok(WorkerAdmissionOutcome::AlreadyAdmitted)
        } else {
            Err(ControlProtocolError::ConflictingMessage)
        };
    }
    if observed_at >= *lease_expires_at {
        return Err(ControlProtocolError::InvalidTransition);
    }
    let response = DurableControlMessage {
        message_id: response_message_id,
        payload: WorkerControlMessage::AssignmentAccepted {
            binding: binding.clone(),
        },
    };
    let event = fact(
        WORKER_ADMITTED,
        projection.last_event_id,
        observed_at,
        &AdmittedPayload {
            worker_id,
            offer_message_id: offer.message_id,
            binding: binding.clone(),
            lease_expires_at: *lease_expires_at,
            contract: contract.as_ref().clone(),
            response,
        },
    )?;
    events.append(&stream, expected(projection.revision), command_id, &[event])?;
    Ok(WorkerAdmissionOutcome::Admitted)
}

/// Persists an exact start command before creating the only token that may invoke the worker
/// executor. Duplicate starts after a terminal result are harmless; a crash after the start fact is
/// conservatively in doubt and never grants a second invocation token.
///
/// # Errors
///
/// Returns an error for an unknown/mismatched attempt, an in-doubt prior start, or journal failure.
pub fn record_worker_execution_start<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    start: &DurableControlMessage<ControllerControlMessage>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<Option<WorkerExecutionAuthority>, ControlProtocolError> {
    validate_controller_message(worker_id, start)?;
    let ControllerControlMessage::StartExecution { binding } = &start.payload else {
        return Err(ControlProtocolError::InvalidTransition);
    };
    let stream = worker_stream(worker_id)?;
    let projection = project_worker(&events.read_stream(&stream, None)?, worker_id)?;
    let attempt = projection
        .attempts
        .get(&binding.attempt_id())
        .ok_or(ControlProtocolError::InvalidTransition)?;
    if attempt.binding != *binding {
        return Err(ControlProtocolError::BindingMismatch);
    }
    match attempt.phase {
        LocalPhase::Terminal => return Ok(None),
        LocalPhase::Started => return Err(ControlProtocolError::WorkerExecutionInDoubt),
        LocalPhase::Admitted => {}
    }
    let event = fact(
        WORKER_STARTED,
        projection.last_event_id,
        observed_at,
        &WorkerStartedPayload {
            worker_id,
            start_message_id: start.message_id,
            binding: binding.clone(),
        },
    )?;
    let outcome = events.append(&stream, expected(projection.revision), command_id, &[event])?;
    Ok(Some(WorkerExecutionAuthority {
        stream,
        revision: revision(outcome.last_sequence)?,
        last_event_id: only_event_id(&outcome.event_ids)?,
        binding: binding.clone(),
        contract: attempt.contract.clone(),
    }))
}

/// Invokes one worker executor from a consumed durable start token and atomically journals its
/// terminal observation plus worker outbox message.
///
/// # Errors
///
/// Returns an error when terminal result encoding or the atomic journal append fails.
pub fn execute_worker_attempt<E: EventStore, X: Executor>(
    events: &mut E,
    executor: &mut X,
    authority: WorkerExecutionAuthority,
    result_message_id: ControlMessageId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ReconciledExecutionResult, ControlProtocolError> {
    let input = ExecutionInput {
        job_id: authority.binding.job_id(),
        attempt_id: authority.binding.attempt_id(),
        contract_id: authority.binding.contract_id(),
        contract: &authority.contract,
    };
    let result = match executor.execute(&input) {
        Ok(capture) => ReconciledExecutionResult::Completed { capture },
        Err(error) => {
            let diagnostic = bound_utf8(
                &error.to_string(),
                authority.contract.capture().diagnostic_limit().get(),
            );
            match error.failure_class() {
                ExecutorFailureClass::NotStarted => {
                    ReconciledExecutionResult::NotStarted { diagnostic }
                }
                ExecutorFailureClass::Ambiguous => {
                    ReconciledExecutionResult::Ambiguous { diagnostic }
                }
            }
        }
    };
    let response = DurableControlMessage {
        message_id: result_message_id,
        payload: WorkerControlMessage::ExecutionResult {
            binding: authority.binding.clone(),
            result: result.clone(),
        },
    };
    let event = fact(
        WORKER_RESULT,
        Some(authority.last_event_id),
        observed_at,
        &WorkerResultPayload {
            worker_id: authority.binding.worker_id(),
            binding: authority.binding,
            response,
        },
    )?;
    events.append(
        &authority.stream,
        ExpectedRevision::Exact(authority.revision),
        command_id,
        &[event],
    )?;
    Ok(result)
}

/// Records fresh connection mappings for every pending worker response and returns replay frames.
///
/// # Errors
///
/// Returns an error for corrupt journal history, sequence overflow, or persistence failure.
pub fn deliver_worker_messages<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    acknowledges_controller_through: Option<ControlSequence>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<Vec<ControlFrame<WorkerControlMessage>>, ControlProtocolError> {
    let stream = worker_stream(worker_id)?;
    let projection = project_worker(&events.read_stream(&stream, None)?, worker_id)?;
    let existing = projection
        .outbox
        .deliveries
        .get(&connection_id)
        .cloned()
        .unwrap_or_default();
    let already = existing
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut next = next_sequence_value(existing.keys().next_back().copied())?;
    let mut frames = Vec::new();
    let mut deliveries = Vec::new();
    for (message_id, payload) in &projection.outbox.pending {
        if already.contains(message_id) {
            continue;
        }
        let sequence = ControlSequence::new(next)
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?;
        next = next.checked_add(1).ok_or_else(|| {
            ControlProtocolError::InvalidHistory("connection sequence overflow".into())
        })?;
        deliveries.push(Delivery {
            connection_id,
            sequence,
            message_id: Some(*message_id),
        });
        frames.push(ControlFrame {
            protocol_version,
            connection_id,
            sequence,
            acknowledges_peer_through: acknowledges_controller_through,
            message: Some(DurableControlMessage {
                message_id: *message_id,
                payload: payload.clone(),
            }),
        });
    }
    if !deliveries.is_empty() {
        let event = fact(
            WORKER_DELIVERED,
            projection.last_event_id,
            observed_at,
            &DeliveriesPayload {
                worker_id,
                deliveries,
            },
        )?;
        events.append(&stream, expected(projection.revision), command_id, &[event])?;
    }
    Ok(frames)
}

/// Records and returns an acknowledgement-only worker frame. This allows the controller outbox to
/// close even when the worker has no durable logical response to piggyback the acknowledgement.
///
/// # Errors
///
/// Returns an error for corrupt connection history, sequence overflow, or persistence failure.
pub fn deliver_worker_acknowledgement<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    acknowledges_controller_through: ControlSequence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControlFrame<WorkerControlMessage>, ControlProtocolError> {
    let stream = worker_stream(worker_id)?;
    let projection = project_worker(&events.read_stream(&stream, None)?, worker_id)?;
    record_ack_delivery(
        events,
        &stream,
        projection.revision,
        projection.last_event_id,
        &projection.outbox,
        WORKER_DELIVERED,
        worker_id,
        protocol_version,
        connection_id,
        acknowledges_controller_through,
        command_id,
        observed_at,
    )
}

/// Applies a controller cumulative acknowledgement to the worker's logical result outbox.
///
/// # Errors
///
/// Returns an error for acknowledgement regression/bounds, corrupt history, or persistence failure.
pub fn acknowledge_worker_messages<E: EventStore>(
    events: &mut E,
    worker_id: WorkerId,
    connection_id: ControlConnectionId,
    acknowledged_through: ControlSequence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<usize, ControlProtocolError> {
    let stream = worker_stream(worker_id)?;
    let projection = project_worker(&events.read_stream(&stream, None)?, worker_id)?;
    let message_ids = acknowledged_ids(&projection.outbox, connection_id, acknowledged_through)?;
    if projection.outbox.acknowledged.get(&connection_id) == Some(&acknowledged_through) {
        return Ok(0);
    }
    let removed = message_ids
        .iter()
        .filter(|message_id| projection.outbox.pending.contains_key(message_id))
        .count();
    let event = fact(
        WORKER_ACKED,
        projection.last_event_id,
        observed_at,
        &AcknowledgedPayload {
            worker_id,
            connection_id,
            acknowledged_through,
            message_ids,
        },
    )?;
    events.append(&stream, expected(projection.revision), command_id, &[event])?;
    Ok(removed)
}

/// Returns durable worker responses not yet acknowledged by the controller.
///
/// # Errors
///
/// Returns an error when the durable worker journal cannot be read or validated.
pub fn pending_worker_messages<E: EventStore>(
    events: &E,
    worker_id: WorkerId,
) -> Result<Vec<DurableControlMessage<WorkerControlMessage>>, ControlProtocolError> {
    let projection = project_worker(
        &events.read_stream(&worker_stream(worker_id)?, None)?,
        worker_id,
    )?;
    Ok(projection
        .outbox
        .pending
        .into_iter()
        .map(|(message_id, payload)| DurableControlMessage {
            message_id,
            payload,
        })
        .collect())
}

/// Returns the canonical set of locally admitted or started attempts for heartbeat reconciliation.
/// Terminal attempts are excluded.
///
/// # Errors
///
/// Returns an error when the worker journal cannot be read or validated.
pub fn active_worker_attempts<E: EventStore>(
    events: &E,
    worker_id: WorkerId,
) -> Result<Vec<AttemptId>, ControlProtocolError> {
    let projection = project_worker(
        &events.read_stream(&worker_stream(worker_id)?, None)?,
        worker_id,
    )?;
    Ok(projection
        .attempts
        .into_iter()
        .filter_map(|(attempt_id, attempt)| {
            (attempt.phase != LocalPhase::Terminal).then_some(attempt_id)
        })
        .collect())
}

/// Validates the exact assignment/worker incarnation and publishes a remote terminal result. Late
/// results for an expired post-start lease remain eligible reconciliation evidence; an old binding
/// from a pre-start replacement is rejected.
///
/// # Errors
///
/// Returns an error for a mismatched/non-running assignment or invalid remote terminal evidence.
pub fn reconcile_worker_result<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    worker_id: WorkerId,
    message: &DurableControlMessage<WorkerControlMessage>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<WorkerResultReconciliation, ControlProtocolError> {
    if message.payload.binding().worker_id() != worker_id {
        return Err(ControlProtocolError::BindingMismatch);
    }
    let WorkerControlMessage::ExecutionResult { binding, result } = &message.payload else {
        return Err(ControlProtocolError::InvalidTransition);
    };
    let assignment =
        recover_execution_assignment(events, content, binding.attempt_id(), observed_at)?;
    let lease = match assignment {
        ExecutionAssignmentState::Running { lease }
        | ExecutionAssignmentState::ReconciliationRequired { lease } => lease,
        ExecutionAssignmentState::ExecutionTerminal { lease, .. } => {
            if lease.binding() == binding {
                return Ok(WorkerResultReconciliation::AlreadyTerminal);
            }
            return Err(ControlProtocolError::BindingMismatch);
        }
        _ => return Err(ControlProtocolError::InvalidTransition),
    };
    if lease.binding() != binding {
        return Err(ControlProtocolError::BindingMismatch);
    }
    let completion = reconcile_execution_result(
        events,
        content,
        &ExecutionJob::new(binding.job_id())?,
        binding.attempt_id(),
        binding.contract_id(),
        result.clone(),
        command_id,
        observed_at,
    )?;
    Ok(WorkerResultReconciliation::Published(Box::new(completion)))
}

fn validate_controller_message(
    worker_id: WorkerId,
    message: &DurableControlMessage<ControllerControlMessage>,
) -> Result<(), ControlProtocolError> {
    let binding = message.payload.binding();
    if binding.worker_id() != worker_id {
        return Err(ControlProtocolError::BindingMismatch);
    }
    let expected = match message.payload {
        ControllerControlMessage::AssignmentOffer { .. } => binding.offer_message_id(),
        ControllerControlMessage::StartExecution { .. } => binding.start_message_id(),
    };
    if message.message_id != expected {
        return Err(ControlProtocolError::BindingMismatch);
    }
    Ok(())
}

fn validate_contract_binding(
    binding: &AssignmentBinding,
    contract: &JobContract,
) -> Result<(), ControlProtocolError> {
    contract
        .validate()
        .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?;
    let bytes = cairn_codec::to_vec(contract)
        .map_err(|error| ControlProtocolError::Codec(error.to_string()))?;
    let observed = ContentId::<JobContractArtifact>::derive(&bytes)
        .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?;
    if contract.job_id() != binding.job_id() || observed != binding.contract_id() {
        return Err(ControlProtocolError::BindingMismatch);
    }
    Ok(())
}

fn project_controller(
    events: &[EventEnvelope],
    worker_id: WorkerId,
) -> Result<ControllerProjection, ControlProtocolError> {
    let mut projection = ControllerProjection {
        revision: None,
        last_event_id: None,
        outbox: OutboxProjection::default(),
    };
    for event in events {
        validate_event(event, projection.last_event_id)?;
        match event.schema_name.as_str() {
            CONTROLLER_ENQUEUED => {
                let payload: EnqueuedControllerPayload = decode(event)?;
                if payload.worker_id != worker_id {
                    return invalid("controller outbox worker identity changed");
                }
                validate_controller_message(worker_id, &payload.message)?;
                insert_pending(&mut projection.outbox, payload.message)?;
            }
            CONTROLLER_DELIVERED => {
                let payload: DeliveriesPayload = decode(event)?;
                if payload.worker_id != worker_id {
                    return invalid("controller delivery worker identity changed");
                }
                apply_deliveries(&mut projection.outbox, payload.deliveries)?;
            }
            CONTROLLER_ACKED => {
                let payload: AcknowledgedPayload = decode(event)?;
                if payload.worker_id != worker_id {
                    return invalid("controller acknowledgement worker identity changed");
                }
                apply_acknowledgement(&mut projection.outbox, payload)?;
            }
            _ => return invalid("unknown controller outbox event schema"),
        }
        advance(
            &mut projection.revision,
            &mut projection.last_event_id,
            event,
        )?;
    }
    Ok(projection)
}

fn project_worker(
    events: &[EventEnvelope],
    worker_id: WorkerId,
) -> Result<WorkerProjection, ControlProtocolError> {
    let mut projection = WorkerProjection {
        revision: None,
        last_event_id: None,
        attempts: BTreeMap::new(),
        outbox: OutboxProjection::default(),
    };
    for event in events {
        validate_event(event, projection.last_event_id)?;
        match event.schema_name.as_str() {
            WORKER_ADMITTED => {
                let payload: AdmittedPayload = decode(event)?;
                if payload.worker_id != worker_id
                    || payload.offer_message_id != payload.binding.offer_message_id()
                    || payload.response.payload.binding() != &payload.binding
                    || event.observed_at_unix_ms >= payload.lease_expires_at.get()
                {
                    return invalid("worker admission binding changed");
                }
                validate_contract_binding(&payload.binding, &payload.contract)?;
                if projection
                    .attempts
                    .insert(
                        payload.binding.attempt_id(),
                        LocalAttempt {
                            binding: payload.binding,
                            contract: payload.contract,
                            phase: LocalPhase::Admitted,
                        },
                    )
                    .is_some()
                {
                    return invalid("worker attempt was admitted twice");
                }
                insert_pending(&mut projection.outbox, payload.response)?;
            }
            WORKER_STARTED => {
                let payload: WorkerStartedPayload = decode(event)?;
                if payload.worker_id != worker_id
                    || payload.start_message_id != payload.binding.start_message_id()
                {
                    return invalid("worker start binding changed");
                }
                let attempt = projection
                    .attempts
                    .get_mut(&payload.binding.attempt_id())
                    .ok_or_else(|| {
                        ControlProtocolError::InvalidHistory("start before admission".into())
                    })?;
                if attempt.binding != payload.binding || attempt.phase != LocalPhase::Admitted {
                    return invalid("worker start transition is invalid");
                }
                attempt.phase = LocalPhase::Started;
            }
            WORKER_RESULT => {
                let payload: WorkerResultPayload = decode(event)?;
                if payload.worker_id != worker_id
                    || payload.response.payload.binding() != &payload.binding
                {
                    return invalid("worker result binding changed");
                }
                let attempt = projection
                    .attempts
                    .get_mut(&payload.binding.attempt_id())
                    .ok_or_else(|| {
                        ControlProtocolError::InvalidHistory("result before admission".into())
                    })?;
                if attempt.binding != payload.binding || attempt.phase != LocalPhase::Started {
                    return invalid("worker result transition is invalid");
                }
                attempt.phase = LocalPhase::Terminal;
                insert_pending(&mut projection.outbox, payload.response)?;
            }
            WORKER_DELIVERED => {
                let payload: DeliveriesPayload = decode(event)?;
                if payload.worker_id != worker_id {
                    return invalid("worker delivery identity changed");
                }
                apply_deliveries(&mut projection.outbox, payload.deliveries)?;
            }
            WORKER_ACKED => {
                let payload: AcknowledgedPayload = decode(event)?;
                if payload.worker_id != worker_id {
                    return invalid("worker acknowledgement identity changed");
                }
                apply_acknowledgement(&mut projection.outbox, payload)?;
            }
            _ => return invalid("unknown worker journal event schema"),
        }
        advance(
            &mut projection.revision,
            &mut projection.last_event_id,
            event,
        )?;
    }
    Ok(projection)
}

fn insert_pending<T: Clone + Eq>(
    outbox: &mut OutboxProjection<T>,
    message: DurableControlMessage<T>,
) -> Result<(), ControlProtocolError> {
    if let Some(existing) = outbox.pending.get(&message.message_id) {
        if existing == &message.payload {
            return Ok(());
        }
        return Err(ControlProtocolError::ConflictingMessage);
    }
    outbox.pending.insert(message.message_id, message.payload);
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "generic durable ack delivery keeps both protocol directions behaviorally identical"
)]
fn record_ack_delivery<E: EventStore, T>(
    events: &mut E,
    stream: &StreamId,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    outbox: &OutboxProjection<T>,
    schema: &str,
    worker_id: WorkerId,
    protocol_version: crate::WorkerProtocolVersion,
    connection_id: ControlConnectionId,
    acknowledges_peer_through: ControlSequence,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControlFrame<T>, ControlProtocolError> {
    let last = outbox
        .deliveries
        .get(&connection_id)
        .and_then(|deliveries| deliveries.keys().next_back())
        .copied();
    let next = next_sequence_value(last)?;
    let sequence = ControlSequence::new(next)
        .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?;
    let event = fact(
        schema,
        last_event_id,
        observed_at,
        &DeliveriesPayload {
            worker_id,
            deliveries: vec![Delivery {
                connection_id,
                sequence,
                message_id: None,
            }],
        },
    )?;
    events.append(stream, expected(revision), command_id, &[event])?;
    Ok(ControlFrame {
        protocol_version,
        connection_id,
        sequence,
        acknowledges_peer_through: Some(acknowledges_peer_through),
        message: None,
    })
}

fn apply_deliveries<T>(
    outbox: &mut OutboxProjection<T>,
    deliveries: Vec<Delivery>,
) -> Result<(), ControlProtocolError> {
    for delivery in deliveries {
        let connection = outbox.deliveries.entry(delivery.connection_id).or_default();
        let expected_sequence = connection
            .keys()
            .next_back()
            .map_or(1, |value| value.get() + 1);
        if delivery.sequence.get() != expected_sequence {
            return invalid("connection delivery sequence or logical identity is duplicated");
        }
        if let Some(message_id) = delivery.message_id {
            if !outbox.pending.contains_key(&message_id)
                || connection
                    .values()
                    .flatten()
                    .any(|value| *value == message_id)
            {
                return invalid("delivery cites a non-pending or duplicate logical message");
            }
        }
        connection.insert(delivery.sequence, delivery.message_id);
    }
    Ok(())
}

fn apply_acknowledgement<T>(
    outbox: &mut OutboxProjection<T>,
    payload: AcknowledgedPayload,
) -> Result<(), ControlProtocolError> {
    let expected_ids =
        acknowledged_ids(outbox, payload.connection_id, payload.acknowledged_through)?;
    if expected_ids != payload.message_ids {
        return invalid("acknowledgement logical message set changed");
    }
    for message_id in payload.message_ids {
        outbox.pending.remove(&message_id);
    }
    outbox
        .acknowledged
        .insert(payload.connection_id, payload.acknowledged_through);
    Ok(())
}

fn acknowledged_ids<T>(
    outbox: &OutboxProjection<T>,
    connection_id: ControlConnectionId,
    through: ControlSequence,
) -> Result<Vec<ControlMessageId>, ControlProtocolError> {
    let deliveries = outbox
        .deliveries
        .get(&connection_id)
        .ok_or(ControlProtocolError::AcknowledgementOutOfBounds)?;
    let highest = deliveries
        .keys()
        .next_back()
        .ok_or(ControlProtocolError::AcknowledgementOutOfBounds)?;
    validate_ack(
        outbox.acknowledged.get(&connection_id).copied(),
        Some(through),
        Some(*highest),
    )?;
    Ok(deliveries
        .range(..=through)
        .filter_map(|(_, message_id)| *message_id)
        .collect())
}

fn validate_ack(
    previous: Option<ControlSequence>,
    observed: Option<ControlSequence>,
    highest_sent: Option<ControlSequence>,
) -> Result<(), ControlProtocolError> {
    if observed < previous {
        return Err(ControlProtocolError::AcknowledgementRegressed);
    }
    if observed > highest_sent {
        return Err(ControlProtocolError::AcknowledgementOutOfBounds);
    }
    Ok(())
}

fn next_sequence_value(last: Option<ControlSequence>) -> Result<u64, ControlProtocolError> {
    last.map_or(Ok(1), |value| {
        value.get().checked_add(1).ok_or_else(|| {
            ControlProtocolError::InvalidHistory("connection sequence overflow".into())
        })
    })
}

fn enforce_frame_limit(
    byte_len: usize,
    policy: ControlFramePolicy,
) -> Result<(), ControlProtocolError> {
    let observed = u64::try_from(byte_len).unwrap_or(u64::MAX);
    match policy.byte_limit {
        Some(limit) if observed > limit.get() => Err(ControlProtocolError::FrameTooLarge {
            observed,
            limit: limit.get(),
        }),
        Some(_) | None => Ok(()),
    }
}

fn controller_stream(worker_id: WorkerId) -> Result<StreamId, ControlProtocolError> {
    stream("controller-control-outbox", worker_id)
}

fn worker_stream(worker_id: WorkerId) -> Result<StreamId, ControlProtocolError> {
    stream("worker-control-journal", worker_id)
}

fn stream(kind: &str, worker_id: WorkerId) -> Result<StreamId, ControlProtocolError> {
    Ok(StreamId {
        kind: AggregateKind::new(kind)
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new(worker_id.to_string())
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?,
    })
}

fn fact<P: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, ControlProtocolError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| ControlProtocolError::Codec(error.to_string()))?,
    })
}

fn decode<T: DeserializeOwned>(event: &EventEnvelope) -> Result<T, ControlProtocolError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))
}

fn validate_event(
    event: &EventEnvelope,
    previous: Option<EventId>,
) -> Result<(), ControlProtocolError> {
    if event.schema_version.get() != 1 || event.parent_event_id != previous {
        return invalid("event version or causal chain is invalid");
    }
    Ok(())
}

fn advance(
    revision_slot: &mut Option<StreamRevision>,
    event_slot: &mut Option<EventId>,
    event: &EventEnvelope,
) -> Result<(), ControlProtocolError> {
    *revision_slot = Some(revision(event.sequence)?);
    *event_slot = Some(event.event_id);
    Ok(())
}

fn revision(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, ControlProtocolError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| ControlProtocolError::InvalidHistory(error.to_string()))
}

fn expected(revision: Option<StreamRevision>) -> ExpectedRevision {
    revision.map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact)
}

fn only_event_id(event_ids: &[EventId]) -> Result<EventId, ControlProtocolError> {
    match event_ids {
        [event_id] => Ok(*event_id),
        _ => invalid("single-event append returned an invalid identity count"),
    }
}

fn invalid<T>(message: &str) -> Result<T, ControlProtocolError> {
    Err(ControlProtocolError::InvalidHistory(message.to_owned()))
}

fn bound_utf8(value: &str, limit: u64) -> String {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{
        AssignmentId, ContentType, ControlMessageId, JobId, LeaseId, WorkerIncarnationId,
    };
    use cairn_record::ContentStore;
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::*;
    use crate::{
        ArchitectureName, AssignmentControlMessageIds, AssignmentLeaseDurationMillis,
        AssignmentLeaseGrant, AssignmentLeasePolicy, AuthenticatedWorkerIdentity, CapturePolicy,
        CapturedOutput, CommandContract, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend,
        ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionOutcome, ExecutionPlatform,
        ExecutionPlatformRequirement, ExecutionTimeoutMillis, InputBundleArtifact, NetworkPolicy,
        OperatingSystemName, OutputByteLimit, OutputName, PlacementRequest, RecordedExecution,
        RecordedExecutor, RecordedWorkerAuthenticator, ResolvedProgramIdentity, ResourceRequest,
        SandboxPath, TargetEnvironmentName, TrustedExecutionEvidence, WorkerAuthenticationSubject,
        WorkerAvailability, WorkerBinaryIdentity, WorkerHealth, WorkerHello, WorkerPoolName,
        WorkerProfile, WorkerProtocolVersion, WorkerResourceClaim, WorkerResourceInventory,
        WorkerResourceSource, WorkerSessionState, WorkerSessionTimeoutMillis, WorkerSlotCount,
        authorize_execution_attempt, grant_assignment_lease, prepare_execution_job,
        record_worker_heartbeat, recover_execution_job, recover_worker_session, register_worker,
        start_accepted_assignment,
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        controller_event_path: std::path::PathBuf,
        worker_event_path: std::path::PathBuf,
        content_path: std::path::PathBuf,
        cas_path: std::path::PathBuf,
        controller_events: SqliteEventStore,
        worker_events: SqliteEventStore,
        content: SqliteContentStore,
        contract: JobContract,
        environment_id: ContentId<ExecutionEnvironmentArtifact>,
        worker_id: WorkerId,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let controller_event_path = directory.path().join("controller-events.db");
            let worker_event_path = directory.path().join("worker-events.db");
            let content_path = directory.path().join("content.db");
            let cas_path = directory.path().join("cas");
            let controller_events =
                SqliteEventStore::open(&controller_event_path).expect("controller events");
            let worker_events = SqliteEventStore::open(&worker_event_path).expect("worker events");
            let mut content = SqliteContentStore::open(&content_path, &cas_path).expect("content");
            let input = put::<InputBundleArtifact>(&mut content, b"remote-input");
            let environment_id = put::<ExecutionEnvironmentArtifact>(
                &mut content,
                br#"{"image":"sha256:remote-fixture"}"#,
            );
            let contract = JobContract::new(
                JobId::new(),
                input,
                environment_id,
                ExecutionBackend::new("remote-recorded-process").expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/run").expect("program"),
                    Vec::new(),
                    SandboxPath::new("work").expect("workdir"),
                ),
                ResourceRequest::new(
                    ExecutionTimeoutMillis::new(5_000).expect("timeout"),
                    PlacementRequest::new(
                        ExecutionPlatformRequirement::new(
                            Some(ArchitectureName::new("x86_64").expect("architecture")),
                            None,
                            None,
                        ),
                        vec![WorkerPoolName::new("fixture").expect("pool")],
                        Vec::new(),
                    )
                    .expect("placement"),
                )
                .expect("resources"),
                NetworkPolicy::Disabled,
                CapturePolicy::new(
                    OutputByteLimit::new(1024).expect("stdout"),
                    OutputByteLimit::new(1024).expect("stderr"),
                    DiagnosticByteLimit::new(1024).expect("diagnostic"),
                    EvidenceByteLimit::new(4096).expect("evidence"),
                    vec![crate::ExpectedOutput {
                        name: OutputName::new("report").expect("output"),
                        path: SandboxPath::new("out/report.json").expect("path"),
                        byte_limit: OutputByteLimit::new(1024).expect("output limit"),
                    }],
                )
                .expect("capture"),
            );
            Self {
                _directory: directory,
                controller_event_path,
                worker_event_path,
                content_path,
                cas_path,
                controller_events,
                worker_events,
                content,
                contract,
                environment_id,
                worker_id: WorkerId::new(),
            }
        }

        fn reopen_controller(&mut self) {
            self.controller_events =
                SqliteEventStore::open(&self.controller_event_path).expect("reopen controller");
            self.content = SqliteContentStore::open(&self.content_path, &self.cas_path)
                .expect("reopen content");
        }

        fn reopen_worker(&mut self) {
            self.worker_events =
                SqliteEventStore::open(&self.worker_event_path).expect("reopen worker");
        }

        fn register_worker(&mut self) -> crate::RegisteredWorkerSession {
            let profile = WorkerProfile::new(
                protocol_version(),
                WorkerBinaryIdentity::new("sha256:worker-v1").expect("binary"),
                WorkerResourceInventory::new(
                    WorkerResourceClaim::new(
                        ExecutionPlatform::new(
                            ArchitectureName::new("x86_64").expect("architecture"),
                            OperatingSystemName::new("linux").expect("os"),
                            TargetEnvironmentName::new("gnu").expect("environment"),
                        ),
                        WorkerResourceSource::BuiltinProbe,
                    ),
                    vec![WorkerResourceClaim::new(
                        ExecutionBackend::new("remote-recorded-process").expect("backend"),
                        WorkerResourceSource::OperatorDeclared,
                    )],
                    Vec::new(),
                    WorkerSlotCount::new(1).expect("slots"),
                )
                .expect("resources"),
            )
            .expect("profile");
            let hello = WorkerHello::new(self.worker_id, WorkerIncarnationId::new(), profile);
            let subject = WorkerAuthenticationSubject::new("fixture-worker").expect("subject");
            let mut authenticator = RecordedWorkerAuthenticator::new([(
                self.worker_id,
                AuthenticatedWorkerIdentity::new(
                    subject,
                    cairn_protocol::CredentialId::new(),
                    WorkerPoolName::new("fixture").expect("pool"),
                ),
            )]);
            let registered = register_worker(
                &mut self.controller_events,
                &mut self.content,
                &mut authenticator,
                &hello,
                session_timeout(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(0),
            )
            .expect("register");
            record_worker_heartbeat(
                &mut self.controller_events,
                &mut self.content,
                &registered,
                &WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())
                    .expect("availability"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(1),
            )
            .expect("heartbeat")
        }

        fn capture(&self) -> crate::ExecutionCapture {
            crate::ExecutionCapture::new(
                ExecutionOutcome::Succeeded,
                Some(0),
                ExecutionElapsedMillis::new(12),
                b"stdout".to_vec(),
                b"stderr".to_vec(),
                vec![CapturedOutput {
                    name: OutputName::new("report").expect("output"),
                    bytes: br#"{"passed":true}"#.to_vec(),
                }],
                TrustedExecutionEvidence::new(
                    ExecutionBackend::new("remote-recorded-process").expect("backend"),
                    self.environment_id,
                    ResolvedProgramIdentity::new("sha256:resolved-program").expect("program"),
                    Vec::new(),
                )
                .expect("evidence"),
            )
        }
    }

    fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put")
            .content_id
    }

    fn protocol_version() -> crate::WorkerProtocolVersion {
        WorkerProtocolVersion::new(1).expect("protocol")
    }

    fn session_timeout() -> WorkerSessionTimeoutMillis {
        WorkerSessionTimeoutMillis::new(10_000).expect("session timeout")
    }

    fn lease_policy() -> AssignmentLeasePolicy {
        AssignmentLeasePolicy::new(
            session_timeout(),
            AssignmentLeaseDurationMillis::new(1_000).expect("lease"),
        )
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the process/reconnect test keeps the complete distributed transition visible"
    )]
    fn sqlite_restart_reconnect_replays_without_duplicate_execution_and_reconciles_result() {
        let mut fixture = Fixture::new();
        let worker = fixture.register_worker();
        let attempt_id = AttemptId::new();
        let prepared = prepare_execution_job(&mut fixture.content, &fixture.contract)
            .expect("prepare execution");
        let contract_id = prepared.contract_id();
        let authority = authorize_execution_attempt(
            &mut fixture.controller_events,
            prepared,
            attempt_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("authorize");
        let leased = grant_assignment_lease(
            &mut fixture.controller_events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                AssignmentControlMessageIds::new(ControlMessageId::new(), ControlMessageId::new()),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("lease");
        let binding = leased.lease().binding().clone();
        let offer = assignment_offer_message(leased.lease(), leased.contract());
        enqueue_controller_message(
            &mut fixture.controller_events,
            fixture.worker_id,
            &offer,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("enqueue offer");

        let first_connection = ControlConnectionId::new();
        let first_offer_frames = deliver_controller_messages(
            &mut fixture.controller_events,
            fixture.worker_id,
            protocol_version(),
            first_connection,
            None,
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("first offer delivery");
        assert_eq!(first_offer_frames.len(), 1);
        admit_worker_assignment(
            &mut fixture.worker_events,
            fixture.worker_id,
            first_offer_frames[0]
                .message
                .as_ref()
                .expect("offer message"),
            ControlMessageId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("admit");

        // Simulate losing both transport acknowledgements and both processes. The controller still
        // owns the offer; the worker still owns exactly one acceptance response.
        fixture.reopen_controller();
        fixture.reopen_worker();
        let second_connection = ControlConnectionId::new();
        let replayed_offer = deliver_controller_messages(
            &mut fixture.controller_events,
            fixture.worker_id,
            protocol_version(),
            second_connection,
            None,
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("replay offer");
        assert_eq!(replayed_offer[0].sequence.get(), 1);
        assert_eq!(
            replayed_offer[0]
                .message
                .as_ref()
                .expect("replayed message")
                .message_id,
            first_offer_frames[0]
                .message
                .as_ref()
                .expect("first message")
                .message_id
        );
        assert_eq!(
            admit_worker_assignment(
                &mut fixture.worker_events,
                fixture.worker_id,
                replayed_offer[0].message.as_ref().expect("replayed offer"),
                ControlMessageId::new(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(5),
            )
            .expect("duplicate admission"),
            WorkerAdmissionOutcome::AlreadyAdmitted
        );
        assert_eq!(
            pending_worker_messages(&fixture.worker_events, fixture.worker_id)
                .expect("pending acceptance")
                .len(),
            1
        );

        let accepted_frames = deliver_worker_messages(
            &mut fixture.worker_events,
            fixture.worker_id,
            protocol_version(),
            second_connection,
            Some(ControlSequence::new(1).expect("offer ack")),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("deliver accepted");
        let WorkerControlMessage::AssignmentAccepted {
            binding: accepted_binding,
        } = &accepted_frames[0]
            .message
            .as_ref()
            .expect("accepted message")
            .payload
        else {
            panic!("accepted response");
        };
        assert_eq!(accepted_binding, &binding);
        let ExecutionAssignmentState::Leased(recovered_lease) = recover_execution_assignment(
            &fixture.controller_events,
            &fixture.content,
            attempt_id,
            ObservedAtUnixMillis::new(5),
        )
        .expect("recover lease") else {
            panic!("leased state");
        };
        let WorkerSessionState::Live(recovered_worker) = recover_worker_session(
            &fixture.controller_events,
            &fixture.content,
            fixture.worker_id,
            session_timeout(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("recover worker") else {
            panic!("live worker");
        };
        let accepted = accept_worker_assignment(
            &mut fixture.controller_events,
            &fixture.content,
            recovered_lease,
            &recovered_worker,
            accepted_frames[0]
                .message
                .as_ref()
                .expect("accepted message"),
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("accept controller side");
        acknowledge_controller_messages(
            &mut fixture.controller_events,
            fixture.worker_id,
            second_connection,
            ControlSequence::new(1).expect("sequence"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("ack offer");
        let accepted_ack = deliver_controller_acknowledgement(
            &mut fixture.controller_events,
            fixture.worker_id,
            protocol_version(),
            second_connection,
            ControlSequence::new(1).expect("accepted through"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("send accepted ack");
        assert_eq!(accepted_ack.sequence.get(), 2);
        assert!(accepted_ack.message.is_none());
        acknowledge_worker_messages(
            &mut fixture.worker_events,
            fixture.worker_id,
            second_connection,
            accepted_ack
                .acknowledges_peer_through
                .expect("accepted acknowledgement"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("ack accepted");

        let _started = start_accepted_assignment(
            &mut fixture.controller_events,
            &fixture.content,
            accepted,
            &recovered_worker,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("controller start");
        let ExecutionAssignmentState::Running { lease } = recover_execution_assignment(
            &fixture.controller_events,
            &fixture.content,
            attempt_id,
            ObservedAtUnixMillis::new(6),
        )
        .expect("running") else {
            panic!("running state");
        };
        let start = execution_start_message(&lease);
        enqueue_controller_message(
            &mut fixture.controller_events,
            fixture.worker_id,
            &start,
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("enqueue start");
        fixture.reopen_controller();
        let third_connection = ControlConnectionId::new();
        let start_frames = deliver_controller_messages(
            &mut fixture.controller_events,
            fixture.worker_id,
            protocol_version(),
            third_connection,
            None,
            &CommandId::new(),
            ObservedAtUnixMillis::new(7),
        )
        .expect("deliver start");
        let wire = encode_control_frame(
            &start_frames[0],
            ControlFramePolicy {
                byte_limit: Some(ControlFrameByteLimit::new(64 * 1024).expect("limit")),
            },
        )
        .expect("encode frame");
        let decoded: ControlFrame<ControllerControlMessage> = decode_control_frame(
            &wire,
            ControlFramePolicy {
                byte_limit: Some(ControlFrameByteLimit::new(64 * 1024).expect("limit")),
            },
        )
        .expect("decode frame");
        assert_eq!(decoded, start_frames[0]);
        let worker_authority = record_worker_execution_start(
            &mut fixture.worker_events,
            fixture.worker_id,
            decoded.message.as_ref().expect("start message"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(7),
        )
        .expect("record worker start")
        .expect("new worker start");
        let capture = fixture.capture();
        let mut executor = RecordedExecutor::new([RecordedExecution {
            contract_id,
            capture,
        }]);
        execute_worker_attempt(
            &mut fixture.worker_events,
            &mut executor,
            worker_authority,
            ControlMessageId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(8),
        )
        .expect("execute worker");
        fixture.reopen_worker();

        let result_frames = deliver_worker_messages(
            &mut fixture.worker_events,
            fixture.worker_id,
            protocol_version(),
            third_connection,
            Some(ControlSequence::new(1).expect("start ack")),
            &CommandId::new(),
            ObservedAtUnixMillis::new(9),
        )
        .expect("deliver result");
        assert_eq!(result_frames.len(), 1);
        assert!(matches!(
            reconcile_worker_result(
                &mut fixture.controller_events,
                &mut fixture.content,
                fixture.worker_id,
                result_frames[0].message.as_ref().expect("result message"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(9),
            )
            .expect("reconcile result"),
            WorkerResultReconciliation::Published(_)
        ));
        acknowledge_controller_messages(
            &mut fixture.controller_events,
            fixture.worker_id,
            third_connection,
            ControlSequence::new(1).expect("sequence"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(9),
        )
        .expect("ack start");
        let result_ack = deliver_controller_acknowledgement(
            &mut fixture.controller_events,
            fixture.worker_id,
            protocol_version(),
            third_connection,
            ControlSequence::new(1).expect("result through"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(9),
        )
        .expect("send result ack");
        assert_eq!(result_ack.sequence.get(), 2);
        acknowledge_worker_messages(
            &mut fixture.worker_events,
            fixture.worker_id,
            third_connection,
            result_ack
                .acknowledges_peer_through
                .expect("result acknowledgement"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(9),
        )
        .expect("ack result");

        // A duplicate terminal delivery is acknowledged only after proving the assignment is the
        // already-terminal one. It cannot overwrite the accepted receipt.
        assert!(matches!(
            reconcile_worker_result(
                &mut fixture.controller_events,
                &mut fixture.content,
                fixture.worker_id,
                result_frames[0].message.as_ref().expect("result message"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(10),
            )
            .expect("duplicate result"),
            WorkerResultReconciliation::AlreadyTerminal
        ));
        fixture.reopen_controller();
        fixture.reopen_worker();
        assert!(
            pending_controller_messages(&fixture.controller_events, fixture.worker_id)
                .expect("controller empty")
                .is_empty()
        );
        assert!(
            pending_worker_messages(&fixture.worker_events, fixture.worker_id)
                .expect("worker empty")
                .is_empty()
        );
        assert!(matches!(
            recover_execution_job(
                &fixture.controller_events,
                &fixture.content,
                &ExecutionJob::new(fixture.contract.job_id()).expect("job"),
            )
            .expect("recover complete"),
            crate::ExecutionJobState::Completed { .. }
        ));
    }

    #[test]
    fn frame_cursor_rejects_ack_regression_and_unsent_ack_and_budget_is_configurable() {
        assert!(ControlFrameByteLimit::new(0).is_err());
        let tiny = ControlFramePolicy {
            byte_limit: Some(ControlFrameByteLimit::new(1).expect("tiny")),
        };
        let mut fixture = Fixture::new();
        let worker = fixture.register_worker();
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut fixture.controller_events,
            prepared,
            AttemptId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("authority");
        let leased = grant_assignment_lease(
            &mut fixture.controller_events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                AssignmentControlMessageIds::new(ControlMessageId::new(), ControlMessageId::new()),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("lease");
        let connection_id = ControlConnectionId::new();
        let frame = ControlFrame {
            protocol_version: protocol_version(),
            connection_id,
            sequence: ControlSequence::new(1).expect("sequence"),
            acknowledges_peer_through: Some(ControlSequence::new(1).expect("ack")),
            message: Some(assignment_offer_message(leased.lease(), leased.contract())),
        };
        assert!(matches!(
            encode_control_frame(&frame, tiny),
            Err(ControlProtocolError::FrameTooLarge { .. })
        ));
        assert!(encode_control_frame(&frame, ControlFramePolicy::default()).is_ok());

        let mut cursor = InboundControlSession::new(protocol_version(), connection_id);
        assert!(matches!(
            cursor.accept(&frame, None),
            Err(ControlProtocolError::AcknowledgementOutOfBounds)
        ));
        cursor
            .accept(&frame, Some(ControlSequence::new(1).expect("sent")))
            .expect("valid frame after rejected observation");
        let regressed = ControlFrame {
            sequence: ControlSequence::new(2).expect("sequence"),
            acknowledges_peer_through: None,
            ..frame
        };
        assert!(matches!(
            cursor.accept(&regressed, Some(ControlSequence::new(1).expect("sent"))),
            Err(ControlProtocolError::AcknowledgementRegressed)
        ));
        let acknowledgement_only = ControlFrame::<ControllerControlMessage> {
            protocol_version: protocol_version(),
            connection_id,
            sequence: ControlSequence::new(2).expect("sequence"),
            acknowledges_peer_through: Some(ControlSequence::new(1).expect("ack")),
            message: None,
        };
        cursor
            .accept(
                &acknowledgement_only,
                Some(ControlSequence::new(1).expect("sent")),
            )
            .expect("acknowledgement-only frame");
        let empty = ControlFrame::<ControllerControlMessage> {
            sequence: ControlSequence::new(3).expect("sequence"),
            acknowledges_peer_through: None,
            ..acknowledgement_only
        };
        assert!(matches!(
            cursor.accept(&empty, Some(ControlSequence::new(1).expect("sent"))),
            Err(ControlProtocolError::EmptyFrame)
        ));
    }
}
