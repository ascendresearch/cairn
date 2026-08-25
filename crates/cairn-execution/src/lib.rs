//! Domain-neutral opaque execution contracts, durable attempts, and trusted evidence capture.

mod assignment;
mod contract;
mod control;
mod coordinator;
mod executor;
mod scheduler;
mod worker;

pub use assignment::{
    AcceptedExecutionAssignment, AssignmentBinding, AssignmentControlError,
    AssignmentControlMessageIds, AssignmentExecutionTerminal, AssignmentLeaseGrant,
    AssignmentLeasePolicy, AssignmentLeaseRecord, ExecutionAssignmentState, ExpiredLeaseClass,
    LeasedExecutionAssignment, accept_assignment, grant_assignment_lease, reap_expired_assignment,
    recover_execution_assignment, renew_assignment_lease, start_accepted_assignment,
};
pub use contract::{
    AcceleratorDeviceCount, AcceleratorResourceRequest, ArchitectureName, ArchivedOutput,
    CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, CommandArgument,
    CommandContract, ContractValueError, DeclaredOutputArtifact, DiagnosticByteLimit,
    EvidenceByteLimit, ExecutionBackend, ExecutionElapsedMillis, ExecutionEnvironmentArtifact,
    ExecutionEvidenceArtifact, ExecutionObservation, ExecutionOutcome, ExecutionPlatform,
    ExecutionPlatformRequirement, ExecutionReceipt, ExecutionReceiptArtifact,
    ExecutionStderrArtifact, ExecutionStdoutArtifact, ExecutionTimeoutMillis, ExpectedOutput,
    InputBundleArtifact, JobContract, JobContractArtifact, LogicalCpuCount, MemoryByteCount,
    NetworkPolicy, OperatingSystemName, OutputByteLimit, OutputName, PlacementRequest,
    QuantitativeResourceRequest, ResolvedProgramIdentity, ResourceRequest, SandboxPath,
    ScratchByteCount, TargetEnvironmentName, TrustedExecutionEvidence, WorkerPoolName,
};
pub use control::{
    AssignmentMaterialByteLimit, AssignmentMaterialChunk, AssignmentMaterialChunkRequest,
    AssignmentMaterialChunkSize, AssignmentMaterialKind, AssignmentMaterialManifest,
    ControlEnqueueOutcome, ControlFrame, ControlFrameByteLimit, ControlFramePolicy,
    ControlProtocolError, ControllerControlMessage, DurableControlMessage, InboundControlSession,
    VerifiedAssignmentMaterials, WorkerAdmissionOutcome, WorkerControlMessage,
    WorkerExecutionAuthority, WorkerResultReconciliation, accept_worker_assignment,
    acknowledge_controller_messages, acknowledge_worker_messages, active_worker_attempts,
    admit_worker_assignment, assignment_offer_message, decode_control_frame,
    deliver_controller_acknowledgement, deliver_controller_messages,
    deliver_worker_acknowledgement, deliver_worker_messages, encode_control_frame,
    enqueue_controller_message, execute_worker_attempt, execution_start_message,
    load_assignment_material_manifest, pending_controller_messages, pending_worker_messages,
    read_assignment_material_chunk, reconcile_worker_result, record_worker_execution_start,
    validate_assignment_material_manifest, verify_persisted_assignment_materials,
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
pub use scheduler::{
    CandidateDisposition, PlacementAuthorityError, PlacementAuthorityObservation,
    PlacementCandidateRejection, PlacementCandidateSnapshot, PlacementOutcome, PlacementRecord,
    PlacementSnapshot, PlacementSnapshotArtifact, ReservationReleaseReason,
    ReservedWorkerResources, SchedulerError, SchedulerPolicy, SchedulerPolicyVersion,
    WorkerPlacementAuthority, grant_reserved_assignment, recover_scheduler_placement,
    release_scheduler_reservation, reserve_worker_placement,
};
pub use worker::{
    AcceleratorDevice, AcceleratorDeviceId, AcceleratorDiscoveryCompleteness,
    AssignmentLeaseDurationMillis, AuthenticatedWorkerIdentity, RecordedWorkerAuthenticator,
    RegisteredWorkerSession, ReservationClaimTimeoutMillis, ResourceProbeVersion,
    TrustedWorkerPoolAssignment, TrustedWorkerResourceAdmission, WorkerAuthenticationError,
    WorkerAuthenticationSubject, WorkerAuthenticator, WorkerAvailability,
    WorkerAvailabilityArtifact, WorkerBinaryIdentity, WorkerControlError, WorkerHealth,
    WorkerHello, WorkerMatchFailure, WorkerProfile, WorkerProfileArtifact, WorkerProtocolVersion,
    WorkerResourceClaim, WorkerResourceInventory, WorkerResourceObservation,
    WorkerResourceObservationArtifact, WorkerResourceSource, WorkerSessionState,
    WorkerSessionTimeoutMillis, WorkerSlotCount, WorkerValueError,
    admit_trusted_worker_resource_observation, disconnect_worker, match_worker, match_worker_at,
    record_worker_heartbeat, record_worker_resource_observation, recover_worker_session,
    register_worker, synchronize_worker_pool_assignment,
};
