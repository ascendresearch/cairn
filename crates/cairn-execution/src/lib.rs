//! Domain-neutral opaque execution contracts, durable attempts, and trusted evidence capture.

mod contract;
mod coordinator;
mod executor;

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
pub use coordinator::{
    ExecutionAttemptAuthority, ExecutionCompletion, ExecutionCoordinatorError, ExecutionJob,
    ExecutionJobState, PreparedExecutionJob, StartedExecutionAttempt, authorize_execution_attempt,
    begin_execution_attempt, execute_execution_attempt, prepare_execution_job,
    recover_execution_job,
};
pub use executor::{
    CapturedOutput, ExecutionCapture, ExecutionInput, Executor, ExecutorError,
    ExecutorFailureClass, RecordedExecution, RecordedExecutor, ScriptedExecutor,
};
