use std::{collections::BTreeSet, io::Cursor};

use cairn_protocol::{
    AggregateId, AggregateKind, AttemptId, CommandId, ContentId, EventId, JobId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StreamRevision,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ArchivedOutput, ContractValueError, DeclaredOutputArtifact, ExecutionCapture,
    ExecutionEvidenceArtifact, ExecutionInput, ExecutionOutcome, ExecutionReceipt,
    ExecutionReceiptArtifact, ExecutionStderrArtifact, ExecutionStdoutArtifact, Executor,
    ExecutorFailureClass, InputBundleArtifact, JobContract, JobContractArtifact,
};

const ATTEMPT_AUTHORIZED: &str = "execution.attempt-authorized";
const ATTEMPT_STARTED: &str = "execution.attempt-started";
const ATTEMPT_COMPLETED: &str = "execution.attempt-completed";
const ATTEMPT_NOT_STARTED: &str = "execution.attempt-not-started";
const ATTEMPT_AMBIGUOUS: &str = "execution.attempt-ambiguous";

/// Aggregate boundary for one logical opaque execution job and all of its attempts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionJob {
    job_id: JobId,
    stream: StreamId,
}

impl ExecutionJob {
    /// Creates the canonical job stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the protocol stream representation cannot be constructed.
    pub fn new(job_id: JobId) -> Result<Self, ExecutionCoordinatorError> {
        Ok(Self {
            job_id,
            stream: StreamId {
                kind: AggregateKind::new("execution-job").map_err(|error| {
                    ExecutionCoordinatorError::InvalidHistory(error.to_string())
                })?,
                id: AggregateId::new(job_id.to_string()).map_err(|error| {
                    ExecutionCoordinatorError::InvalidHistory(error.to_string())
                })?,
            },
        })
    }

    /// Returns the logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the canonical event stream.
    #[must_use]
    pub const fn stream_id(&self) -> &StreamId {
        &self.stream
    }
}

/// Validated and archived job contract. Construction is restricted to [`prepare_execution_job`].
#[derive(Clone, Debug)]
pub struct PreparedExecutionJob {
    contract_id: ContentId<JobContractArtifact>,
    contract: JobContract,
}

impl PreparedExecutionJob {
    /// Returns the exact immutable contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Returns the validated opaque contract.
    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.contract
    }
}

/// One-shot authority to durably mark one concrete attempt started.
pub struct ExecutionAttemptAuthority {
    stream: StreamId,
    revision: StreamRevision,
    authority_event_id: EventId,
    attempt_id: AttemptId,
    prepared: Box<PreparedExecutionJob>,
}

impl ExecutionAttemptAuthority {
    /// Returns the stable logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.prepared.contract.job_id()
    }

    /// Returns the fresh concrete attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the exact archived job contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.prepared.contract_id
    }

    /// Returns the immutable job contract used for capability matching.
    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.prepared.contract
    }
}

/// One-shot proof that execution was marked started before invoking an executor.
pub struct StartedExecutionAttempt {
    stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
    attempt_id: AttemptId,
    prepared: PreparedExecutionJob,
}

impl StartedExecutionAttempt {
    /// Returns the stable logical job identity.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.prepared.contract.job_id()
    }

    /// Returns the concrete started-attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the immutable job contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.prepared.contract_id
    }
}

/// Terminal result after executor capture and event publication.
#[derive(Debug)]
pub enum ExecutionCompletion {
    /// Complete stdout/stderr/evidence/output receipt is durable.
    Completed {
        /// Exact receipt artifact.
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        /// Verified canonical receipt.
        receipt: ExecutionReceipt,
    },
    /// Executor proved that no workload started.
    NotStarted {
        /// Failed concrete attempt.
        attempt_id: AttemptId,
        /// Bounded durable diagnostic.
        diagnostic: String,
    },
    /// Workload outcome cannot be determined and must not be blindly retried.
    Ambiguous {
        /// Uncertain concrete attempt.
        attempt_id: AttemptId,
        /// Bounded durable diagnostic.
        diagnostic: String,
    },
}

/// Terminal observation returned by a remote worker and independently revalidated by the
/// controller before it can become an authoritative execution fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum ReconciledExecutionResult {
    /// Complete bounded capture material.
    Completed { capture: ExecutionCapture },
    /// The worker supervisor proved that the workload never started.
    NotStarted { diagnostic: String },
    /// The worker cannot prove the external-effect outcome.
    Ambiguous { diagnostic: String },
}

/// Durable state reconstructed from events and verified content only.
pub enum ExecutionJobState {
    /// No attempt authority exists.
    NotFound,
    /// A concrete attempt is authorized but has not been marked started.
    ReadyToStart(ExecutionAttemptAuthority),
    /// The attempt may have executed; reconciliation is required.
    InDoubt {
        /// Concrete attempt whose terminal state is unknown.
        attempt_id: AttemptId,
    },
    /// Complete terminal evidence is durable.
    Completed {
        /// Exact receipt artifact.
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        /// Verified canonical receipt.
        receipt: ExecutionReceipt,
    },
    /// The executor proved the workload did not start.
    NotStarted {
        /// Concrete attempt.
        attempt_id: AttemptId,
        /// Durable diagnostic.
        diagnostic: String,
    },
    /// Outcome is uncertain and requires reconciliation.
    Ambiguous {
        /// Concrete attempt.
        attempt_id: AttemptId,
        /// Durable diagnostic.
        diagnostic: String,
    },
}

/// Failure while preparing, executing, publishing, or recovering an opaque job.
#[derive(Debug, Error)]
pub enum ExecutionCoordinatorError {
    /// Contract value validation failed.
    #[error(transparent)]
    Contract(#[from] ContractValueError),
    /// Content verification or archival failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Event publication or reading failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Durable facts or content contradict the execution state machine.
    #[error("invalid execution history: {0}")]
    InvalidHistory(String),
    /// Executor capture violates the immutable job contract.
    #[error("invalid executor capture: {0}")]
    InvalidCapture(String),
    /// Complete artifacts exist but their terminal fact did not commit.
    #[error(
        "attempt {attempt_id} archived execution receipt {receipt_id}, but terminal recording failed ({record})"
    )]
    UnrecordedCapture {
        /// Attempt whose artifacts are inert until reconciled.
        attempt_id: AttemptId,
        /// Recoverable receipt identity.
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        /// Event-store diagnostic.
        record: String,
    },
    /// Executor failure occurred but its classified fact did not commit.
    #[error("attempt {attempt_id} failed at executor boundary, but recording failed ({record})")]
    UnrecordedFailure {
        /// Attempt requiring reconciliation.
        attempt_id: AttemptId,
        /// Event-store diagnostic.
        record: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "durable event schema keeps explicit typed identity field names"
)]
struct AuthorizedPayload {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "durable event schema keeps explicit typed identity field names"
)]
struct StartedPayload {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "durable event schema keeps explicit typed identity field names"
)]
struct CompletedPayload {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FailurePayload {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    diagnostic: String,
}

#[derive(Clone)]
enum ProjectedState {
    Authorized {
        attempt_id: AttemptId,
        event_id: EventId,
    },
    Started {
        attempt_id: AttemptId,
        event_id: EventId,
    },
    Completed {
        attempt_id: AttemptId,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
    },
    NotStarted {
        attempt_id: AttemptId,
        diagnostic: String,
    },
    Ambiguous {
        attempt_id: AttemptId,
        diagnostic: String,
    },
}

struct Projection {
    contract_id: ContentId<JobContractArtifact>,
    used_attempts: BTreeSet<AttemptId>,
    state: ProjectedState,
}

/// Verifies every immutable contract reference and archives canonical contract bytes.
///
/// # Errors
///
/// Returns an error for unsupported contracts, missing/corrupt inputs, or CAS failures.
pub fn prepare_execution_job<C: ContentStore>(
    content: &mut C,
    contract: &JobContract,
) -> Result<PreparedExecutionJob, ExecutionCoordinatorError> {
    contract.validate()?;
    verify_content::<C, InputBundleArtifact>(content, contract.input_bundle_id())?;
    verify_content::<C, crate::ExecutionEnvironmentArtifact>(content, contract.environment_id())?;
    let bytes = cairn_codec::to_vec(contract)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?;
    let contract_id = content
        .put::<JobContractArtifact>(&mut Cursor::new(bytes))?
        .content_id;
    Ok(PreparedExecutionJob {
        contract_id,
        contract: contract.clone(),
    })
}

/// Commits authority for a fresh attempt. A later attempt may be authorized only after a terminal
/// completed/not-started attempt; an ambiguous or merely started attempt blocks retries.
///
/// # Errors
///
/// Returns an error for contract mutation, reused attempt identity, unsafe retry, or append failure.
pub fn authorize_execution_attempt<E: EventStore>(
    events: &mut E,
    prepared: PreparedExecutionJob,
    attempt_id: AttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionAttemptAuthority, ExecutionCoordinatorError> {
    let job = ExecutionJob::new(prepared.contract.job_id())?;
    let history = events.read_stream(&job.stream, None)?;
    let (expected, parent) = if history.is_empty() {
        (ExpectedRevision::NoStream, None)
    } else {
        let projection = project(&history, job.job_id)?;
        if projection.contract_id != prepared.contract_id {
            return invalid_history("job contract changed across attempts");
        }
        if let ProjectedState::Authorized {
            attempt_id: authorized,
            event_id,
        } = projection.state
        {
            let last = history.last().ok_or_else(|| {
                ExecutionCoordinatorError::InvalidHistory("missing active authority fact".into())
            })?;
            if authorized == attempt_id && last.command_id == *command_id {
                return Ok(ExecutionAttemptAuthority {
                    stream: job.stream,
                    revision: revision(last.sequence)?,
                    authority_event_id: event_id,
                    attempt_id,
                    prepared: Box::new(prepared),
                });
            }
            return invalid_history("job already has active attempt authority");
        }
        if matches!(
            projection.state,
            ProjectedState::Started { .. } | ProjectedState::Ambiguous { .. }
        ) {
            return invalid_history("job has an unreconciled attempt");
        }
        if projection.used_attempts.contains(&attempt_id) {
            return invalid_history("execution attempt identity was reused");
        }
        let last = history.last().ok_or_else(|| {
            ExecutionCoordinatorError::InvalidHistory("missing execution history".into())
        })?;
        (
            ExpectedRevision::Exact(revision(last.sequence)?),
            Some(last.event_id),
        )
    };
    let event = fact(
        ATTEMPT_AUTHORIZED,
        parent,
        observed_at,
        &AuthorizedPayload {
            job_id: job.job_id,
            attempt_id,
            contract_id: prepared.contract_id,
        },
    )?;
    let outcome = events.append(&job.stream, expected, command_id, &[event])?;
    Ok(ExecutionAttemptAuthority {
        stream: job.stream,
        revision: revision(outcome.last_sequence)?,
        authority_event_id: outcome.event_ids[0],
        attempt_id,
        prepared: Box::new(prepared),
    })
}

/// Commits the started fact before any executor capability may run.
///
/// # Errors
///
/// Returns an error when the start fact cannot commit.
pub fn begin_execution_attempt<E: EventStore>(
    events: &mut E,
    authority: ExecutionAttemptAuthority,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StartedExecutionAttempt, ExecutionCoordinatorError> {
    let event = fact(
        ATTEMPT_STARTED,
        Some(authority.authority_event_id),
        observed_at,
        &StartedPayload {
            job_id: authority.prepared.contract.job_id(),
            attempt_id: authority.attempt_id,
            contract_id: authority.prepared.contract_id,
        },
    )?;
    let outcome = events.append(
        &authority.stream,
        ExpectedRevision::Exact(authority.revision),
        command_id,
        &[event],
    )?;
    tracing::info!(
        target: "cairn.execution",
        event = "execution_attempt_started",
        job_id = %authority.prepared.contract.job_id(),
        attempt_id = %authority.attempt_id,
        contract_id = %authority.prepared.contract_id,
        "execution attempt durably started"
    );
    Ok(StartedExecutionAttempt {
        stream: authority.stream,
        revision: revision(outcome.last_sequence)?,
        started_event_id: outcome.event_ids[0],
        attempt_id: authority.attempt_id,
        prepared: *authority.prepared,
    })
}

/// Executes exactly one started attempt, archives complete capture material, and commits one
/// terminal fact. The consumed start proof prevents in-process double execution.
///
/// # Errors
///
/// Returns an error for invalid capture, content failures, or unrecorded terminal state.
#[expect(
    clippy::needless_pass_by_value,
    reason = "consuming the one-shot start proof prevents a second executor invocation"
)]
pub fn execute_execution_attempt<E: EventStore, C: ContentStore, X: Executor>(
    events: &mut E,
    content: &mut C,
    executor: &mut X,
    started: StartedExecutionAttempt,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionCompletion, ExecutionCoordinatorError> {
    let input = ExecutionInput {
        job_id: started.prepared.contract.job_id(),
        attempt_id: started.attempt_id,
        contract_id: started.prepared.contract_id,
        contract: &started.prepared.contract,
    };
    match executor.execute(&input) {
        Ok(capture) => publish_capture(events, content, &started, capture, command_id, observed_at),
        Err(error) => {
            let diagnostic = bounded_diagnostic(
                &error.to_string(),
                started.prepared.contract.capture().diagnostic_limit(),
            );
            let (schema, completion) = match error.failure_class() {
                ExecutorFailureClass::NotStarted => (
                    ATTEMPT_NOT_STARTED,
                    ExecutionCompletion::NotStarted {
                        attempt_id: started.attempt_id,
                        diagnostic: diagnostic.clone(),
                    },
                ),
                ExecutorFailureClass::Ambiguous => (
                    ATTEMPT_AMBIGUOUS,
                    ExecutionCompletion::Ambiguous {
                        attempt_id: started.attempt_id,
                        diagnostic: diagnostic.clone(),
                    },
                ),
            };
            let event = fact(
                schema,
                Some(started.started_event_id),
                observed_at,
                &FailurePayload {
                    job_id: started.prepared.contract.job_id(),
                    attempt_id: started.attempt_id,
                    contract_id: started.prepared.contract_id,
                    diagnostic,
                },
            )?;
            events
                .append(
                    &started.stream,
                    ExpectedRevision::Exact(started.revision),
                    command_id,
                    &[event],
                )
                .map_err(|record| ExecutionCoordinatorError::UnrecordedFailure {
                    attempt_id: started.attempt_id,
                    record: record.to_string(),
                })?;
            tracing::warn!(
                target: "cairn.execution",
                event = "execution_attempt_failed",
                job_id = %started.prepared.contract.job_id(),
                attempt_id = %started.attempt_id,
                failure_class = ?error.failure_class(),
                diagnostic_archived = true,
                "executor failed; diagnostic omitted from logs"
            );
            Ok(completion)
        }
    }
}

/// Reconstructs the next safe action from the event stream and verified CAS artifacts.
///
/// # Errors
///
/// Returns an error when history, contracts, receipts, or cited capture artifacts disagree.
pub fn recover_execution_job<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    job: &ExecutionJob,
) -> Result<ExecutionJobState, ExecutionCoordinatorError> {
    let history = events.read_stream(&job.stream, None)?;
    if history.is_empty() {
        return Ok(ExecutionJobState::NotFound);
    }
    let projection = project(&history, job.job_id)?;
    let prepared = recover_prepared(content, projection.contract_id, job.job_id)?;
    let last = history.last().ok_or_else(|| {
        ExecutionCoordinatorError::InvalidHistory("missing execution history".into())
    })?;
    match projection.state {
        ProjectedState::Authorized {
            attempt_id,
            event_id,
        } => Ok(ExecutionJobState::ReadyToStart(ExecutionAttemptAuthority {
            stream: job.stream.clone(),
            revision: revision(last.sequence)?,
            authority_event_id: event_id,
            attempt_id,
            prepared: Box::new(prepared),
        })),
        ProjectedState::Started { attempt_id, .. } => Ok(ExecutionJobState::InDoubt { attempt_id }),
        ProjectedState::Completed {
            attempt_id,
            receipt_id,
        } => {
            let receipt = recover_receipt(content, receipt_id, job.job_id, attempt_id, &prepared)?;
            Ok(ExecutionJobState::Completed {
                receipt_id,
                receipt,
            })
        }
        ProjectedState::NotStarted {
            attempt_id,
            diagnostic,
        } => Ok(ExecutionJobState::NotStarted {
            attempt_id,
            diagnostic,
        }),
        ProjectedState::Ambiguous {
            attempt_id,
            diagnostic,
        } => Ok(ExecutionJobState::Ambiguous {
            attempt_id,
            diagnostic,
        }),
    }
}

/// Publishes a remote-worker terminal observation only for the exact attempt already marked
/// started by the controller. This reconstructs reconciliation authority, never execution
/// authority: it cannot invoke an executor or advance an authorized-but-unstarted attempt.
///
/// # Errors
///
/// Returns an error when the job is not in doubt for `attempt_id`, the immutable contract differs,
/// the result violates that contract, or the terminal fact cannot commit.
#[expect(
    clippy::too_many_arguments,
    reason = "stores, job, attempt, contract, result, command, and observation are independent"
)]
pub fn reconcile_execution_result<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    job: &ExecutionJob,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    result: ReconciledExecutionResult,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionCompletion, ExecutionCoordinatorError> {
    let history = events.read_stream(&job.stream, None)?;
    let projection = project(&history, job.job_id)?;
    if projection.contract_id != contract_id {
        return invalid_history("remote result contract identity differs from execution job");
    }
    let ProjectedState::Started {
        attempt_id: active_attempt_id,
        event_id,
    } = projection.state
    else {
        return invalid_history("remote result does not reconcile an in-doubt execution");
    };
    if active_attempt_id != attempt_id {
        return invalid_history("remote result attempt identity differs from active execution");
    }
    let last = history.last().ok_or_else(|| {
        ExecutionCoordinatorError::InvalidHistory("missing started execution fact".into())
    })?;
    let prepared = recover_prepared(content, contract_id, job.job_id)?;
    let started = StartedExecutionAttempt {
        stream: job.stream.clone(),
        revision: revision(last.sequence)?,
        started_event_id: event_id,
        attempt_id,
        prepared,
    };
    match result {
        ReconciledExecutionResult::Completed { capture } => {
            publish_capture(events, content, &started, capture, command_id, observed_at)
        }
        ReconciledExecutionResult::NotStarted { diagnostic } => publish_remote_failure(
            events,
            &started,
            ExecutorFailureClass::NotStarted,
            &diagnostic,
            command_id,
            observed_at,
        ),
        ReconciledExecutionResult::Ambiguous { diagnostic } => publish_remote_failure(
            events,
            &started,
            ExecutorFailureClass::Ambiguous,
            &diagnostic,
            command_id,
            observed_at,
        ),
    }
}

fn publish_remote_failure<E: EventStore>(
    events: &mut E,
    started: &StartedExecutionAttempt,
    class: ExecutorFailureClass,
    diagnostic: &str,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionCompletion, ExecutionCoordinatorError> {
    let diagnostic = bounded_diagnostic(
        diagnostic,
        started.prepared.contract.capture().diagnostic_limit(),
    );
    let (schema, completion) = match class {
        ExecutorFailureClass::NotStarted => (
            ATTEMPT_NOT_STARTED,
            ExecutionCompletion::NotStarted {
                attempt_id: started.attempt_id,
                diagnostic: diagnostic.clone(),
            },
        ),
        ExecutorFailureClass::Ambiguous => (
            ATTEMPT_AMBIGUOUS,
            ExecutionCompletion::Ambiguous {
                attempt_id: started.attempt_id,
                diagnostic: diagnostic.clone(),
            },
        ),
    };
    let event = fact(
        schema,
        Some(started.started_event_id),
        observed_at,
        &FailurePayload {
            job_id: started.prepared.contract.job_id(),
            attempt_id: started.attempt_id,
            contract_id: started.prepared.contract_id,
            diagnostic,
        },
    )?;
    events
        .append(
            &started.stream,
            ExpectedRevision::Exact(started.revision),
            command_id,
            &[event],
        )
        .map_err(|record| ExecutionCoordinatorError::UnrecordedFailure {
            attempt_id: started.attempt_id,
            record: record.to_string(),
        })?;
    tracing::warn!(
        target: "cairn.execution",
        event = "execution_reconciliation_failed",
        job_id = %started.prepared.contract.job_id(),
        attempt_id = %started.attempt_id,
        failure_class = ?class,
        diagnostic_archived = true,
        "remote execution failure reconciled; diagnostic omitted from logs"
    );
    Ok(completion)
}

fn publish_capture<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    started: &StartedExecutionAttempt,
    mut capture: ExecutionCapture,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionCompletion, ExecutionCoordinatorError> {
    validate_capture(&started.prepared.contract, &capture, content)?;
    capture
        .outputs
        .sort_by(|left, right| left.name.cmp(&right.name));
    let stdout_id = content
        .put::<ExecutionStdoutArtifact>(&mut Cursor::new(&capture.stdout))?
        .content_id;
    let stderr_id = content
        .put::<ExecutionStderrArtifact>(&mut Cursor::new(&capture.stderr))?
        .content_id;
    let evidence_bytes = cairn_codec::to_vec(&capture.evidence)
        .map_err(|error| ExecutionCoordinatorError::InvalidCapture(error.to_string()))?;
    let evidence_id = content
        .put::<ExecutionEvidenceArtifact>(&mut Cursor::new(evidence_bytes))?
        .content_id;
    let mut outputs = Vec::with_capacity(capture.outputs.len());
    for output in capture.outputs {
        let content_id = content
            .put::<DeclaredOutputArtifact>(&mut Cursor::new(output.bytes))?
            .content_id;
        outputs.push(ArchivedOutput {
            name: output.name,
            content_id,
        });
    }
    let receipt = ExecutionReceipt {
        schema_version: 1,
        job_id: started.prepared.contract.job_id(),
        attempt_id: started.attempt_id,
        contract_id: started.prepared.contract_id,
        outcome: capture.outcome,
        exit_code: capture.exit_code,
        elapsed_ms: capture.elapsed_ms,
        stdout_id,
        stderr_id,
        evidence_id,
        outputs,
    };
    let receipt_bytes = cairn_codec::to_vec(&receipt)
        .map_err(|error| ExecutionCoordinatorError::InvalidCapture(error.to_string()))?;
    let receipt_id = content
        .put::<ExecutionReceiptArtifact>(&mut Cursor::new(receipt_bytes))?
        .content_id;
    let event = fact(
        ATTEMPT_COMPLETED,
        Some(started.started_event_id),
        observed_at,
        &CompletedPayload {
            job_id: started.prepared.contract.job_id(),
            attempt_id: started.attempt_id,
            contract_id: started.prepared.contract_id,
            receipt_id,
        },
    )?;
    events
        .append(
            &started.stream,
            ExpectedRevision::Exact(started.revision),
            command_id,
            &[event],
        )
        .map_err(|record| ExecutionCoordinatorError::UnrecordedCapture {
            attempt_id: started.attempt_id,
            receipt_id,
            record: record.to_string(),
        })?;
    tracing::info!(
        target: "cairn.execution",
        event = "execution_attempt_completed",
        job_id = %receipt.job_id,
        attempt_id = %receipt.attempt_id,
        receipt_id = %receipt_id,
        outcome = ?receipt.outcome,
        exit_code = receipt.exit_code,
        elapsed_ms = receipt.elapsed_ms.get(),
        output_count = receipt.outputs.len(),
        "execution attempt completed and receipt was published"
    );
    Ok(ExecutionCompletion::Completed {
        receipt_id,
        receipt,
    })
}

fn validate_capture<C: ContentStore>(
    contract: &JobContract,
    capture: &ExecutionCapture,
    content: &C,
) -> Result<(), ExecutionCoordinatorError> {
    capture.evidence.validate()?;
    let backend_matches = capture.evidence.backend() == contract.backend();
    verify_content::<C, crate::ExecutionEnvironmentArtifact>(
        content,
        capture.evidence.observed_environment_id(),
    )?;
    let environment_matches =
        capture.evidence.observed_environment_id() == contract.environment_id();
    if (!backend_matches || !environment_matches)
        && capture.outcome != ExecutionOutcome::IntegrityViolation
    {
        return invalid_capture(
            "observed backend/environment differs without an integrity-violation outcome",
        );
    }
    if u64::try_from(capture.stdout.len()).unwrap_or(u64::MAX)
        > contract.capture().stdout_limit().get()
    {
        return invalid_capture("stdout exceeds its capture limit");
    }
    if u64::try_from(capture.stderr.len()).unwrap_or(u64::MAX)
        > contract.capture().stderr_limit().get()
    {
        return invalid_capture("stderr exceeds its capture limit");
    }
    let evidence_bytes = cairn_codec::to_vec(&capture.evidence)
        .map_err(|error| ExecutionCoordinatorError::InvalidCapture(error.to_string()))?;
    if u64::try_from(evidence_bytes.len()).unwrap_or(u64::MAX)
        > contract.capture().evidence_limit().get()
    {
        return invalid_capture("trusted evidence exceeds its capture limit");
    }
    match capture.outcome {
        ExecutionOutcome::Succeeded if capture.exit_code != Some(0) => {
            return invalid_capture("successful execution must have exit code zero");
        }
        ExecutionOutcome::SubjectFailed
            if capture.exit_code.is_none() || capture.exit_code == Some(0) =>
        {
            return invalid_capture("subject failure must have a nonzero exit code");
        }
        _ => {}
    }

    let expected = contract
        .capture()
        .expected_outputs()
        .iter()
        .map(|output| (output.name.as_str(), output))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut captured = BTreeSet::new();
    for output in &capture.outputs {
        let Some(declared) = expected.get(output.name.as_str()) else {
            return invalid_capture("executor returned an undeclared output");
        };
        if !captured.insert(output.name.as_str()) {
            return invalid_capture("executor returned a duplicate output");
        }
        if u64::try_from(output.bytes.len()).unwrap_or(u64::MAX) > declared.byte_limit.get() {
            return invalid_capture("declared output exceeds its ingestion limit");
        }
    }
    if capture.outcome == ExecutionOutcome::Succeeded && captured.len() != expected.len() {
        return invalid_capture("successful execution omitted a required output");
    }
    Ok(())
}

fn recover_prepared<C: ContentStore>(
    content: &C,
    contract_id: ContentId<JobContractArtifact>,
    expected_job_id: JobId,
) -> Result<PreparedExecutionJob, ExecutionCoordinatorError> {
    let mut bytes = Vec::new();
    content.write_to(&contract_id, &mut bytes)?;
    let contract: JobContract = cairn_codec::from_slice(&bytes)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?;
    contract.validate()?;
    if contract.job_id() != expected_job_id {
        return invalid_history("contract job identity differs from its stream");
    }
    verify_content::<C, InputBundleArtifact>(content, contract.input_bundle_id())?;
    verify_content::<C, crate::ExecutionEnvironmentArtifact>(content, contract.environment_id())?;
    Ok(PreparedExecutionJob {
        contract_id,
        contract,
    })
}

fn recover_receipt<C: ContentStore>(
    content: &C,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    job_id: JobId,
    attempt_id: AttemptId,
    prepared: &PreparedExecutionJob,
) -> Result<ExecutionReceipt, ExecutionCoordinatorError> {
    let bytes = read_content(content, receipt_id)?;
    let receipt: ExecutionReceipt = cairn_codec::from_slice(&bytes)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?;
    if receipt.schema_version != 1
        || receipt.job_id != job_id
        || receipt.attempt_id != attempt_id
        || receipt.contract_id != prepared.contract_id
    {
        return invalid_history("execution receipt identity metadata changed");
    }
    if receipt
        .outputs
        .windows(2)
        .any(|pair| pair[0].name >= pair[1].name)
    {
        return invalid_history("execution receipt outputs are not in canonical name order");
    }
    let stdout = read_content(content, receipt.stdout_id)?;
    let stderr = read_content(content, receipt.stderr_id)?;
    let evidence_bytes = read_content(content, receipt.evidence_id)?;
    let evidence = cairn_codec::from_slice(&evidence_bytes)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?;
    let mut outputs = Vec::with_capacity(receipt.outputs.len());
    for output in &receipt.outputs {
        outputs.push(crate::CapturedOutput {
            name: output.name.clone(),
            bytes: read_content(content, output.content_id)?,
        });
    }
    let capture = ExecutionCapture::new(
        receipt.outcome,
        receipt.exit_code,
        receipt.elapsed_ms,
        stdout,
        stderr,
        outputs,
        evidence,
    );
    validate_capture(&prepared.contract, &capture, content)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?;
    Ok(receipt)
}

#[expect(
    clippy::too_many_lines,
    reason = "the event-fold transition table remains contiguous for state-machine audit"
)]
fn project(
    events: &[EventEnvelope],
    expected_job_id: JobId,
) -> Result<Projection, ExecutionCoordinatorError> {
    let mut contract_id = None;
    let mut state = None;
    let mut used_attempts = BTreeSet::new();
    let mut previous_event_id = None;
    for event in events {
        if event.schema_version.get() != 1 {
            return invalid_history("unsupported execution event schema version");
        }
        if event.parent_event_id != previous_event_id {
            return invalid_history("execution event causal chain is broken");
        }
        match event.schema_name.as_str() {
            ATTEMPT_AUTHORIZED => {
                let payload: AuthorizedPayload = decode(event)?;
                validate_common(
                    payload.job_id,
                    payload.contract_id,
                    expected_job_id,
                    &mut contract_id,
                )?;
                if state.as_ref().is_some_and(|current| {
                    !matches!(
                        current,
                        ProjectedState::Completed { .. } | ProjectedState::NotStarted { .. }
                    )
                }) {
                    return invalid_history("attempt authority followed a nonterminal attempt");
                }
                if !used_attempts.insert(payload.attempt_id) {
                    return invalid_history("execution attempt identity is duplicated");
                }
                state = Some(ProjectedState::Authorized {
                    attempt_id: payload.attempt_id,
                    event_id: event.event_id,
                });
            }
            ATTEMPT_STARTED => {
                let payload: StartedPayload = decode(event)?;
                validate_common(
                    payload.job_id,
                    payload.contract_id,
                    expected_job_id,
                    &mut contract_id,
                )?;
                if !matches!(
                    state,
                    Some(ProjectedState::Authorized { attempt_id, .. })
                        if attempt_id == payload.attempt_id
                ) {
                    return invalid_history("attempt start has no matching authority");
                }
                state = Some(ProjectedState::Started {
                    attempt_id: payload.attempt_id,
                    event_id: event.event_id,
                });
            }
            ATTEMPT_COMPLETED => {
                let payload: CompletedPayload = decode(event)?;
                validate_common(
                    payload.job_id,
                    payload.contract_id,
                    expected_job_id,
                    &mut contract_id,
                )?;
                if !matches!(
                    state,
                    Some(ProjectedState::Started { attempt_id, .. })
                        if attempt_id == payload.attempt_id
                ) {
                    return invalid_history("attempt completion has no matching start");
                }
                state = Some(ProjectedState::Completed {
                    attempt_id: payload.attempt_id,
                    receipt_id: payload.receipt_id,
                });
            }
            ATTEMPT_NOT_STARTED | ATTEMPT_AMBIGUOUS => {
                let payload: FailurePayload = decode(event)?;
                validate_common(
                    payload.job_id,
                    payload.contract_id,
                    expected_job_id,
                    &mut contract_id,
                )?;
                if !matches!(
                    state,
                    Some(ProjectedState::Started { attempt_id, .. })
                        if attempt_id == payload.attempt_id
                ) {
                    return invalid_history("executor failure has no matching start");
                }
                state = Some(if event.schema_name.as_str() == ATTEMPT_NOT_STARTED {
                    ProjectedState::NotStarted {
                        attempt_id: payload.attempt_id,
                        diagnostic: payload.diagnostic,
                    }
                } else {
                    ProjectedState::Ambiguous {
                        attempt_id: payload.attempt_id,
                        diagnostic: payload.diagnostic,
                    }
                });
            }
            _ => return invalid_history("unknown execution event schema"),
        }
        previous_event_id = Some(event.event_id);
    }
    Ok(Projection {
        contract_id: contract_id
            .ok_or_else(|| ExecutionCoordinatorError::InvalidHistory("missing contract".into()))?,
        used_attempts,
        state: state
            .ok_or_else(|| ExecutionCoordinatorError::InvalidHistory("missing state".into()))?,
    })
}

fn validate_common(
    actual_job_id: JobId,
    actual_contract_id: ContentId<JobContractArtifact>,
    expected_job_id: JobId,
    contract_id: &mut Option<ContentId<JobContractArtifact>>,
) -> Result<(), ExecutionCoordinatorError> {
    if actual_job_id != expected_job_id {
        return invalid_history("execution event job identity changed");
    }
    if contract_id.is_some_and(|expected| expected != actual_contract_id) {
        return invalid_history("execution event contract identity changed");
    }
    *contract_id = Some(actual_contract_id);
    Ok(())
}

fn verify_content<C: ContentStore, T: cairn_protocol::ContentType>(
    content: &C,
    id: ContentId<T>,
) -> Result<(), ExecutionCoordinatorError> {
    content.write_to(&id, &mut std::io::sink())?;
    Ok(())
}

fn read_content<C: ContentStore, T: cairn_protocol::ContentType>(
    content: &C,
    id: ContentId<T>,
) -> Result<Vec<u8>, ExecutionCoordinatorError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes)?;
    Ok(bytes)
}

fn fact<P: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, ExecutionCoordinatorError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, ExecutionCoordinatorError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))
}

fn revision(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, ExecutionCoordinatorError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| ExecutionCoordinatorError::InvalidHistory(error.to_string()))
}

fn bounded_diagnostic(value: &str, limit: crate::DiagnosticByteLimit) -> String {
    let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
    let mut diagnostic = String::with_capacity(value.len().min(limit));
    for character in value.chars().filter(|character| !character.is_control()) {
        if diagnostic.len().saturating_add(character.len_utf8()) > limit {
            break;
        }
        diagnostic.push(character);
    }
    diagnostic
}

fn invalid_history<T>(message: &str) -> Result<T, ExecutionCoordinatorError> {
    Err(ExecutionCoordinatorError::InvalidHistory(
        message.to_owned(),
    ))
}

fn invalid_capture<T>(message: &str) -> Result<T, ExecutionCoordinatorError> {
    Err(ExecutionCoordinatorError::InvalidCapture(
        message.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{AttemptId, CommandId, ContentId, ContentType, JobId};
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        ExecutionCompletion, ExecutionCoordinatorError, ExecutionJob, ExecutionJobState,
        authorize_execution_attempt, begin_execution_attempt, execute_execution_attempt,
        prepare_execution_job, recover_execution_job,
    };
    use crate::{
        CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, CapturedOutput,
        CommandArgument, CommandContract, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend,
        ExecutionCapture, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionOutcome,
        ExecutionPlatformRequirement, ExecutionTimeoutMillis, ExecutorError, ExpectedOutput,
        InputBundleArtifact, JobContract, NetworkPolicy, OutputByteLimit, OutputName,
        PlacementRequest, RecordedExecution, RecordedExecutor, ResolvedProgramIdentity,
        ResourceRequest, SandboxPath, ScriptedExecutor, TrustedExecutionEvidence,
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        content_database: std::path::PathBuf,
        event_database: std::path::PathBuf,
        cas: std::path::PathBuf,
        content: SqliteContentStore,
        events: SqliteEventStore,
        contract: JobContract,
        environment_id: ContentId<ExecutionEnvironmentArtifact>,
    }

    impl Fixture {
        fn new(argument: &str, stdout_limit: u64) -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let content_database = directory.path().join("content.db");
            let event_database = directory.path().join("events.db");
            let cas = directory.path().join("cas");
            let mut content = SqliteContentStore::open(&content_database, &cas).expect("content");
            let events = SqliteEventStore::open(&event_database).expect("events");
            let input_bundle_id = put::<InputBundleArtifact>(&mut content, b"bundle-v1");
            let environment_id =
                put::<ExecutionEnvironmentArtifact>(&mut content, br#"{"image":"sha256:fixture"}"#);
            let contract = JobContract::new(
                JobId::new(),
                input_bundle_id,
                environment_id,
                ExecutionBackend::new("recorded-process").expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/run").expect("program"),
                    vec![CommandArgument::new(argument).expect("argument")],
                    SandboxPath::new("work").expect("working directory"),
                ),
                ResourceRequest::new(
                    ExecutionTimeoutMillis::new(5_000).expect("timeout"),
                    PlacementRequest::new(
                        ExecutionPlatformRequirement::default(),
                        Vec::new(),
                        vec![CapabilityRequirement {
                            name: CapabilityName::new("fixture-runtime").expect("capability"),
                            value: CapabilityValue::new("v1").expect("value"),
                        }],
                    )
                    .expect("placement"),
                )
                .expect("resources"),
                NetworkPolicy::Disabled,
                CapturePolicy::new(
                    OutputByteLimit::new(stdout_limit).expect("stdout"),
                    OutputByteLimit::new(1024).expect("stderr"),
                    DiagnosticByteLimit::new(1024).expect("diagnostic"),
                    EvidenceByteLimit::new(4096).expect("evidence"),
                    vec![ExpectedOutput {
                        name: OutputName::new("report").expect("output"),
                        path: SandboxPath::new("out/report.json").expect("path"),
                        byte_limit: OutputByteLimit::new(1024).expect("output limit"),
                    }],
                )
                .expect("capture"),
            );
            Self {
                _directory: directory,
                content_database,
                event_database,
                cas,
                content,
                events,
                contract,
                environment_id,
            }
        }

        fn reopen(&mut self) {
            self.content = SqliteContentStore::open(&self.content_database, &self.cas)
                .expect("reopen content");
            self.events = SqliteEventStore::open(&self.event_database).expect("reopen events");
        }

        fn capture(&self) -> ExecutionCapture {
            ExecutionCapture::new(
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
                    ExecutionBackend::new("recorded-process").expect("backend"),
                    self.environment_id,
                    ResolvedProgramIdentity::new("sha256:resolved-program")
                        .expect("program identity"),
                    Vec::new(),
                )
                .expect("evidence"),
            )
        }
    }

    fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put content")
            .content_id
    }

    #[test]
    fn complete_capture_survives_restart_with_separate_trust_domains() {
        let mut fixture = Fixture::new("--fixture", 1024);
        let job = ExecutionJob::new(fixture.contract.job_id()).expect("job");
        let attempt_id = AttemptId::new();
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare job");
        let contract_id = prepared.contract_id();
        let _lost = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        fixture.reopen();

        let ExecutionJobState::ReadyToStart(authority) =
            recover_execution_job(&fixture.events, &fixture.content, &job)
                .expect("recover authority")
        else {
            panic!("ready to start");
        };
        let started = begin_execution_attempt(
            &mut fixture.events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut executor = RecordedExecutor::new([RecordedExecution {
            contract_id,
            capture: fixture.capture(),
        }]);
        let ExecutionCompletion::Completed {
            receipt_id,
            receipt,
        } = execute_execution_attempt(
            &mut fixture.events,
            &mut fixture.content,
            &mut executor,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("execute")
        else {
            panic!("completed");
        };
        assert_eq!(receipt.outcome(), ExecutionOutcome::Succeeded);
        assert_eq!(receipt.contract_id(), contract_id);
        assert_eq!(receipt.exit_code(), Some(0));
        assert_eq!(receipt.elapsed(), ExecutionElapsedMillis::new(12));
        assert_eq!(receipt.outputs().len(), 1);
        assert_ne!(
            receipt.stdout_id().to_wire(),
            receipt.outputs()[0].content_id.to_wire(),
            "stream and candidate output retain different semantic domains"
        );
        fixture.reopen();
        let ExecutionJobState::Completed {
            receipt_id: recovered_id,
            receipt: recovered,
        } = recover_execution_job(&fixture.events, &fixture.content, &job)
            .expect("recover receipt")
        else {
            panic!("completed after restart");
        };
        assert_eq!(recovered_id, receipt_id);
        assert_eq!(recovered, receipt);
        let mut stdout = Vec::new();
        fixture
            .content
            .write_to(&recovered.stdout_id(), &mut stdout)
            .expect("stdout");
        assert_eq!(stdout, b"stdout");
        let mut output = Vec::new();
        fixture
            .content
            .write_to(&recovered.outputs()[0].content_id, &mut output)
            .expect("declared output");
        assert_eq!(output, br#"{"passed":true}"#);
    }

    #[test]
    fn started_attempt_recovers_in_doubt_without_executor_authority() {
        let mut fixture = Fixture::new("--in-doubt", 1024);
        let job = ExecutionJob::new(fixture.contract.job_id()).expect("job");
        let attempt_id = AttemptId::new();
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        let _lost_started = begin_execution_attempt(
            &mut fixture.events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        fixture.reopen();
        assert!(matches!(
            recover_execution_job(&fixture.events, &fixture.content, &job)
                .expect("recover in doubt"),
            ExecutionJobState::InDoubt { attempt_id: found } if found == attempt_id
        ));
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare retry");
        assert!(
            authorize_execution_attempt(
                &mut fixture.events,
                prepared,
                AttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(3),
            )
            .is_err()
        );
    }

    #[test]
    fn proven_not_started_allows_fresh_attempt_but_never_identity_reuse() {
        let mut fixture = Fixture::new("--retry", 1024);
        let first_attempt = AttemptId::new();
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            first_attempt,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        let started = begin_execution_attempt(
            &mut fixture.events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut unavailable = ScriptedExecutor::new(|_: &crate::ExecutionInput<'_>| {
            Err(ExecutorError::NotStarted("capacity unavailable".to_owned()))
        });
        assert!(matches!(
            execute_execution_attempt(
                &mut fixture.events,
                &mut fixture.content,
                &mut unavailable,
                started,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(3),
            )
            .expect("not started"),
            ExecutionCompletion::NotStarted { attempt_id, .. } if attempt_id == first_attempt
        ));

        let reused =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare reused");
        assert!(
            authorize_execution_attempt(
                &mut fixture.events,
                reused,
                first_attempt,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(4),
            )
            .is_err()
        );
        let second_attempt = AttemptId::new();
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare second");
        let authority = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            second_attempt,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(5),
        )
        .expect("fresh retry");
        assert_eq!(authority.attempt_id(), second_attempt);
    }

    #[test]
    fn ambiguous_executor_failure_blocks_retry() {
        let mut fixture = Fixture::new("--ambiguous", 1024);
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        let started = begin_execution_attempt(
            &mut fixture.events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let mut ambiguous = ScriptedExecutor::new(|_: &crate::ExecutionInput<'_>| {
            Err(ExecutorError::Ambiguous("lost supervisor".to_owned()))
        });
        assert!(matches!(
            execute_execution_attempt(
                &mut fixture.events,
                &mut fixture.content,
                &mut ambiguous,
                started,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(3),
            )
            .expect("ambiguous"),
            ExecutionCompletion::Ambiguous { .. }
        ));
        let retry =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare retry");
        assert!(
            authorize_execution_attempt(
                &mut fixture.events,
                retry,
                AttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(4),
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_capture_never_publishes_a_terminal_fact() {
        let mut fixture = Fixture::new("--bounded", 3);
        let job = ExecutionJob::new(fixture.contract.job_id()).expect("job");
        let prepared =
            prepare_execution_job(&mut fixture.content, &fixture.contract).expect("prepare");
        let authority = authorize_execution_attempt(
            &mut fixture.events,
            prepared,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("authorize");
        let started = begin_execution_attempt(
            &mut fixture.events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let capture = fixture.capture();
        let mut oversized =
            ScriptedExecutor::new(move |_: &crate::ExecutionInput<'_>| Ok(capture.clone()));
        assert!(matches!(
            execute_execution_attempt(
                &mut fixture.events,
                &mut fixture.content,
                &mut oversized,
                started,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(3),
            ),
            Err(ExecutionCoordinatorError::InvalidCapture(_))
        ));
        assert!(matches!(
            recover_execution_job(&fixture.events, &fixture.content, &job)
                .expect("recover invalid capture"),
            ExecutionJobState::InDoubt { .. }
        ));
        assert_eq!(
            fixture
                .events
                .read_stream(job.stream_id(), None)
                .expect("events")
                .len(),
            2
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the acceptance test enumerates every immutable job-contract dimension"
    )]
    fn every_job_contract_dimension_changes_its_identity() {
        let mut fixture = Fixture::new("--one", 1024);
        let baseline = prepare_execution_job(&mut fixture.content, &fixture.contract)
            .expect("first contract")
            .contract_id();
        let alternate_input = put::<InputBundleArtifact>(&mut fixture.content, b"bundle-v2");
        let alternate_environment = put::<ExecutionEnvironmentArtifact>(
            &mut fixture.content,
            br#"{"image":"sha256:other"}"#,
        );
        let alternate_command = CommandContract::new(
            SandboxPath::new("bin/other").expect("program"),
            vec![],
            SandboxPath::new("work").expect("working directory"),
        );
        let alternate_resources = ResourceRequest::new(
            ExecutionTimeoutMillis::new(7_000).expect("timeout"),
            fixture.contract.resources().placement().clone(),
        )
        .expect("resources");
        let alternate_capture = CapturePolicy::new(
            OutputByteLimit::new(2048).expect("stdout"),
            fixture.contract.capture().stderr_limit(),
            fixture.contract.capture().diagnostic_limit(),
            fixture.contract.capture().evidence_limit(),
            fixture.contract.capture().expected_outputs().to_vec(),
        )
        .expect("capture");
        let variants = [
            JobContract::new(
                JobId::new(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                alternate_input,
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                alternate_environment,
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                ExecutionBackend::new("other-backend").expect("backend"),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                alternate_command,
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                alternate_resources,
                fixture.contract.network(),
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                NetworkPolicy::DependencyFetch,
                fixture.contract.capture().clone(),
            ),
            JobContract::new(
                fixture.contract.job_id(),
                fixture.contract.input_bundle_id(),
                fixture.contract.environment_id(),
                fixture.contract.backend().clone(),
                fixture.contract.command().clone(),
                fixture.contract.resources().clone(),
                fixture.contract.network(),
                alternate_capture,
            ),
        ];
        for variant in variants {
            let identity = prepare_execution_job(&mut fixture.content, &variant)
                .expect("changed contract")
                .contract_id();
            assert_ne!(baseline, identity);
        }
        assert!(SandboxPath::new("../host").is_err());
        assert!(SandboxPath::new("/host/bin").is_err());
    }
}
