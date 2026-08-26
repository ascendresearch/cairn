use std::collections::VecDeque;

use cairn_protocol::{AttemptId, ContentId, JobId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ExecutionElapsedMillis, ExecutionOutcome, JobContract, JobContractArtifact, OutputName,
    TrustedExecutionEvidence,
};

/// Exact bytes ingested for one declared output after execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedOutput {
    /// Logical name declared by the immutable job contract.
    pub name: OutputName,
    /// Complete bytes captured through the executor's ingestion channel.
    pub bytes: Vec<u8>,
}

/// Complete terminal observation returned by a trusted executor capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCapture {
    pub(crate) outcome: ExecutionOutcome,
    pub(crate) exit_code: Option<i32>,
    pub(crate) elapsed_ms: ExecutionElapsedMillis,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) outputs: Vec<CapturedOutput>,
    pub(crate) evidence: TrustedExecutionEvidence,
}

impl ExecutionCapture {
    /// Creates one terminal capture. The coordinator independently validates every byte bound,
    /// output name, environment identity, and backend before publication.
    #[must_use]
    pub fn new(
        outcome: ExecutionOutcome,
        exit_code: Option<i32>,
        elapsed_ms: ExecutionElapsedMillis,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        outputs: Vec<CapturedOutput>,
        evidence: TrustedExecutionEvidence,
    ) -> Self {
        Self {
            outcome,
            exit_code,
            elapsed_ms,
            stdout,
            stderr,
            outputs,
            evidence,
        }
    }
}

/// Read-only input granted to one executor invocation after the start fact commits.
pub struct ExecutionInput<'a> {
    pub(crate) job_id: JobId,
    pub(crate) attempt_id: AttemptId,
    pub(crate) contract_id: ContentId<JobContractArtifact>,
    pub(crate) contract: &'a JobContract,
}

impl ExecutionInput<'_> {
    /// Returns the stable logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the unique concrete attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the exact archived contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Returns the validated opaque execution contract.
    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        self.contract
    }
}

/// Recovery classification for an executor failure that produced no terminal capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorFailureClass {
    /// The executor proves that no workload was started.
    NotStarted,
    /// Workload execution may have occurred and requires reconciliation.
    Ambiguous,
}

/// Failure at the trusted executor capability boundary.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ExecutorError {
    /// No workload was started, so a fresh attempt may later be authorized.
    #[error("executor did not start the workload: {0}")]
    NotStarted(String),
    /// The executor cannot determine whether the workload executed.
    #[error("executor outcome is ambiguous: {0}")]
    Ambiguous(String),
    /// Scripted fixture failure defaults to ambiguous because it cannot prove absence of effect.
    #[error("scripted executor failed: {0}")]
    Scripted(String),
}

impl ExecutorError {
    /// Returns the durable external-effect classification.
    #[must_use]
    pub const fn failure_class(&self) -> ExecutorFailureClass {
        match self {
            Self::NotStarted(_) => ExecutorFailureClass::NotStarted,
            Self::Ambiguous(_) | Self::Scripted(_) => ExecutorFailureClass::Ambiguous,
        }
    }
}

/// Replaceable trusted execution capability. It performs exactly one already-started attempt.
pub trait Executor {
    /// Executes one opaque job attempt and returns complete bounded capture material.
    ///
    /// # Errors
    ///
    /// Returns a classified failure when no complete terminal capture can be produced.
    fn execute(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError>;
}

/// Executor capability that can reconcile an already-started durable attempt.
///
/// This is deliberately separate from [`Executor`]: a recovered start must not be handed to an
/// adapter that can only launch fresh work.
pub trait RecoverableExecutor {
    /// Reconciles the exact previously started attempt and returns its terminal capture.
    ///
    /// # Errors
    ///
    /// Returns an ambiguous failure when the existing external effect cannot be reconciled.
    fn recover(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError>;
}

/// Closure-backed executor for deterministic tests and embedders.
pub struct ScriptedExecutor<F> {
    script: F,
}

impl<F> ScriptedExecutor<F> {
    /// Creates a scripted executor capability.
    pub const fn new(script: F) -> Self {
        Self { script }
    }
}

impl<F> Executor for ScriptedExecutor<F>
where
    F: FnMut(&ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError>,
{
    fn execute(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        (self.script)(input)
    }
}

/// One recorded contract/capture exchange.
pub struct RecordedExecution {
    /// Exact job contract required for the exchange.
    pub contract_id: ContentId<JobContractArtifact>,
    /// Previously captured terminal observation.
    pub capture: ExecutionCapture,
}

/// FIFO executor replaying previously captured terminal observations.
pub struct RecordedExecutor {
    exchanges: VecDeque<RecordedExecution>,
}

impl RecordedExecutor {
    /// Creates a recorded executor with ordered exchanges.
    pub fn new(exchanges: impl IntoIterator<Item = RecordedExecution>) -> Self {
        Self {
            exchanges: exchanges.into_iter().collect(),
        }
    }
}

impl Executor for RecordedExecutor {
    fn execute(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        let exchange = self.exchanges.pop_front().ok_or_else(|| {
            ExecutorError::NotStarted("recorded executor fixture is exhausted".to_owned())
        })?;
        if exchange.contract_id != input.contract_id {
            return Err(ExecutorError::NotStarted(
                "recorded executor contract identity mismatch".to_owned(),
            ));
        }
        Ok(exchange.capture)
    }
}

impl RecoverableExecutor for RecordedExecutor {
    fn recover(&mut self, input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        self.execute(input)
    }
}
