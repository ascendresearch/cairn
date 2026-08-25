//! Domain-neutral opaque execution contracts, durable attempts, and trusted evidence capture.

mod assignment;
mod contract;
mod control;
mod coordinator;
mod executor;
mod worker;

pub use assignment::{
    AcceptedExecutionAssignment, AssignmentBinding, AssignmentControlError,
    AssignmentControlMessageIds, AssignmentExecutionTerminal, AssignmentLeaseGrant,
    AssignmentLeasePolicy, AssignmentLeaseRecord, ExecutionAssignmentState, ExpiredLeaseClass,
    LeasedExecutionAssignment, accept_assignment, grant_assignment_lease, reap_expired_assignment,
    recover_execution_assignment, renew_assignment_lease, start_accepted_assignment,
};
pub use contract::{
    ArchivedOutput, CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy,
    CommandArgument, CommandContract, ContractValueError, DeclaredOutputArtifact,
    DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionElapsedMillis,
    ExecutionEnvironmentArtifact, ExecutionEvidenceArtifact, ExecutionObservation,
    ExecutionOutcome, ExecutionReceipt, ExecutionReceiptArtifact, ExecutionStderrArtifact,
    ExecutionStdoutArtifact, ExecutionTimeoutMillis, ExpectedOutput, InputBundleArtifact,
    JobContract, JobContractArtifact, NetworkPolicy, OutputByteLimit, OutputName,
    ResolvedProgramIdentity, ResourceRequest, SandboxPath, TrustedExecutionEvidence,
};
pub use control::{
    ControlEnqueueOutcome, ControlFrame, ControlFrameByteLimit, ControlFramePolicy,
    ControlProtocolError, ControllerControlMessage, DurableControlMessage, InboundControlSession,
    WorkerAdmissionOutcome, WorkerControlMessage, WorkerExecutionAuthority,
    WorkerResultReconciliation, accept_worker_assignment, acknowledge_controller_messages,
    acknowledge_worker_messages, active_worker_attempts, admit_worker_assignment,
    assignment_offer_message, decode_control_frame, deliver_controller_acknowledgement,
    deliver_controller_messages, deliver_worker_acknowledgement, deliver_worker_messages,
    encode_control_frame, enqueue_controller_message, execute_worker_attempt,
    execution_start_message, pending_controller_messages, pending_worker_messages,
    reconcile_worker_result, record_worker_execution_start,
};
pub use coordinator::{
    ExecutionAttemptAuthority, ExecutionCompletion, ExecutionCoordinatorError, ExecutionJob,
    ExecutionJobState, PreparedExecutionJob, ReconciledExecutionResult, StartedExecutionAttempt,
    authorize_execution_attempt, begin_execution_attempt, execute_execution_attempt,
    prepare_execution_job, reconcile_execution_result, recover_execution_job,
};
pub use executor::{
    CapturedOutput, ExecutionCapture, ExecutionInput, Executor, ExecutorError,
    ExecutorFailureClass, RecordedExecution, RecordedExecutor, ScriptedExecutor,
};
pub use worker::{
    AssignmentLeaseDurationMillis, RecordedWorkerAuthenticator, RegisteredWorkerSession,
    WorkerAuthenticationError, WorkerAuthenticationSubject, WorkerAuthenticator,
    WorkerAvailability, WorkerAvailabilityArtifact, WorkerBinaryIdentity, WorkerControlError,
    WorkerHealth, WorkerHello, WorkerMatchFailure, WorkerProfile, WorkerProfileArtifact,
    WorkerProtocolVersion, WorkerSessionState, WorkerSessionTimeoutMillis, WorkerSlotCount,
    WorkerValueError, disconnect_worker, match_worker, record_worker_heartbeat,
    recover_worker_session, register_worker,
};
