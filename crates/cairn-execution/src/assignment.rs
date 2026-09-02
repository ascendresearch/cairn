use std::collections::BTreeSet;

use cairn_protocol::{
    AggregateId, AggregateKind, AssignmentId, AttemptId, CommandId, ContentId, ControlMessageId,
    EventId, JobId, LeaseId, ObservedAtUnixMillis, SchemaName, SchemaVersion, StreamRevision,
    WorkerId, WorkerIncarnationId,
};
use cairn_record::{
    ContentStore, EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AssignmentLeaseDurationMillis, ExecutionAttemptAuthority, ExecutionCoordinatorError,
    ExecutionJob, ExecutionJobState, ExecutionReceiptArtifact, JobContractArtifact,
    RegisteredWorkerSession, StartedExecutionAttempt, WorkerControlError, WorkerSessionState,
    begin_execution_attempt, match_worker_at, recover_execution_job, recover_worker_session,
};

const ASSIGNMENT_LEASED: &str = "execution.assignment-leased";
const ASSIGNMENT_ACCEPTED: &str = "execution.assignment-accepted";
const ASSIGNMENT_LEASE_RENEWED: &str = "execution.assignment-lease-renewed";
const ASSIGNMENT_LEASE_EXPIRED: &str = "execution.assignment-lease-expired";

/// Immutable binding between one execution attempt and one worker incarnation/lease.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "the binding intentionally exposes only strongly typed identities"
)]
pub struct AssignmentBinding {
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    worker_id: WorkerId,
    worker_incarnation_id: WorkerIncarnationId,
    worker_profile_id: ContentId<crate::WorkerProfileArtifact>,
    offer_message_id: ControlMessageId,
    start_message_id: ControlMessageId,
}

/// Stable logical message identities reserved with an assignment before either message is sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentControlMessageIds {
    offer: ControlMessageId,
    start: ControlMessageId,
}

impl AssignmentControlMessageIds {
    /// Reserves distinct identities for assignment admission and execution start.
    #[must_use]
    pub const fn new(offer: ControlMessageId, start: ControlMessageId) -> Self {
        Self { offer, start }
    }

    /// Returns the assignment-offer message identity.
    #[must_use]
    pub const fn offer(self) -> ControlMessageId {
        self.offer
    }

    /// Returns the execution-start message identity.
    #[must_use]
    pub const fn start(self) -> ControlMessageId {
        self.start
    }
}

/// Configurable lease timing frozen by the caller for a grant or renewal decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentLeasePolicy {
    lease_duration: AssignmentLeaseDurationMillis,
}

impl AssignmentLeasePolicy {
    /// Creates a policy from the assignment lease duration.
    #[must_use]
    pub const fn new(lease_duration: AssignmentLeaseDurationMillis) -> Self {
        Self { lease_duration }
    }

    /// Returns the assignment lease/renewal duration.
    #[must_use]
    pub const fn lease_duration(self) -> AssignmentLeaseDurationMillis {
        self.lease_duration
    }
}

/// Fresh identities and configurable timing for one lease grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentLeaseGrant {
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    message_ids: AssignmentControlMessageIds,
    policy: AssignmentLeasePolicy,
}

impl AssignmentLeaseGrant {
    /// Creates one explicit lease-grant request.
    #[must_use]
    pub const fn new(
        assignment_id: AssignmentId,
        lease_id: LeaseId,
        message_ids: AssignmentControlMessageIds,
        policy: AssignmentLeasePolicy,
    ) -> Self {
        Self {
            assignment_id,
            lease_id,
            message_ids,
            policy,
        }
    }

    /// Returns the fresh logical assignment identity.
    #[must_use]
    pub const fn assignment_id(self) -> AssignmentId {
        self.assignment_id
    }

    /// Returns the fresh bounded lease identity.
    #[must_use]
    pub const fn lease_id(self) -> LeaseId {
        self.lease_id
    }

    /// Returns the frozen assignment liveness policy.
    #[must_use]
    pub const fn policy(self) -> AssignmentLeasePolicy {
        self.policy
    }
}

impl AssignmentBinding {
    /// Returns the logical assignment identity.
    #[must_use]
    pub const fn assignment_id(&self) -> AssignmentId {
        self.assignment_id
    }

    /// Returns this bounded lease identity.
    #[must_use]
    pub const fn lease_id(&self) -> LeaseId {
        self.lease_id
    }

    /// Returns the logical execution job.
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    /// Returns the concrete attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the immutable job contract identity.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Returns the stable worker identity.
    #[must_use]
    pub const fn worker_id(&self) -> WorkerId {
        self.worker_id
    }

    /// Returns the exact worker process/boot incarnation.
    #[must_use]
    pub const fn worker_incarnation_id(&self) -> WorkerIncarnationId {
        self.worker_incarnation_id
    }

    /// Returns the exact static worker profile selected by placement.
    #[must_use]
    pub const fn worker_profile_id(&self) -> ContentId<crate::WorkerProfileArtifact> {
        self.worker_profile_id
    }

    /// Returns the durable assignment-offer logical message identity.
    #[must_use]
    pub const fn offer_message_id(&self) -> ControlMessageId {
        self.offer_message_id
    }

    /// Returns the durable execution-start logical message identity.
    #[must_use]
    pub const fn start_message_id(&self) -> ControlMessageId {
        self.start_message_id
    }
}

/// Current durable timing for one assignment lease.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentLeaseRecord {
    binding: AssignmentBinding,
    granted_at: ObservedAtUnixMillis,
    renewed_at: ObservedAtUnixMillis,
    expires_at: ObservedAtUnixMillis,
    deadline_at: ObservedAtUnixMillis,
}

impl AssignmentLeaseRecord {
    /// Returns the immutable assignment/worker binding.
    #[must_use]
    pub const fn binding(&self) -> &AssignmentBinding {
        &self.binding
    }

    /// Returns the original grant observation.
    #[must_use]
    pub const fn granted_at(&self) -> ObservedAtUnixMillis {
        self.granted_at
    }

    /// Returns the attempt deadline fixed at grant time from the frozen contract's execution
    /// timeout. Renewal cannot carry a lease past it.
    #[must_use]
    pub const fn deadline_at(&self) -> ObservedAtUnixMillis {
        self.deadline_at
    }

    /// Returns the last successful renewal observation.
    #[must_use]
    pub const fn renewed_at(&self) -> ObservedAtUnixMillis {
        self.renewed_at
    }

    /// Returns the exclusive liveness bound.
    #[must_use]
    pub const fn expires_at(&self) -> ObservedAtUnixMillis {
        self.expires_at
    }
}

/// One-shot assignment delivery authority. Construction requires a committed lease fact.
pub struct LeasedExecutionAssignment {
    authority: ExecutionAttemptAuthority,
    lease: AssignmentLeaseRecord,
    revision: StreamRevision,
    last_event_id: EventId,
}

impl LeasedExecutionAssignment {
    /// Returns the durable lease record to place on the wire.
    #[must_use]
    pub const fn lease(&self) -> &AssignmentLeaseRecord {
        &self.lease
    }

    /// Returns the immutable job contract carried by the assignment offer.
    #[must_use]
    pub const fn contract(&self) -> &crate::JobContract {
        self.authority.contract()
    }
}

/// One-shot proof that the selected worker durably accepted an assignment but has no execution
/// authority yet.
pub struct AcceptedExecutionAssignment {
    authority: ExecutionAttemptAuthority,
    lease: AssignmentLeaseRecord,
}

impl AcceptedExecutionAssignment {
    /// Returns the accepted durable lease.
    #[must_use]
    pub const fn lease(&self) -> &AssignmentLeaseRecord {
        &self.lease
    }

    /// Returns the immutable job contract accepted by the worker.
    #[must_use]
    pub const fn contract(&self) -> &crate::JobContract {
        self.authority.contract()
    }
}

/// Why an expired lease can or cannot be safely placed again.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpiredLeaseClass {
    /// No execution start fact exists; a new assignment may reuse the same attempt authority.
    BeforeExecutionStart,
    /// An execution start fact exists; only reconciliation may decide the attempt.
    ExecutionInDoubt,
}

/// Terminal execution state observed while projecting an assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentExecutionTerminal {
    /// A complete execution receipt is durable.
    Completed {
        /// Exact receipt identity.
        receipt_id: ContentId<ExecutionReceiptArtifact>,
    },
    /// Executor proved that the workload never started.
    NotStarted,
    /// Executor outcome is ambiguous independently of the lease.
    Ambiguous,
}

/// Durable assignment state reconstructed jointly with the authoritative execution-job stream.
pub enum ExecutionAssignmentState {
    /// No assignment stream exists.
    NotFound,
    /// Lease is deliverable and the worker has not acknowledged durable acceptance.
    Leased(LeasedExecutionAssignment),
    /// Worker accepted durably; the controller may commit the execution start.
    Accepted(AcceptedExecutionAssignment),
    /// Execution was started and the lease is still live; no execution authority is reconstructed.
    Running {
        /// Current lease.
        lease: AssignmentLeaseRecord,
    },
    /// The lease elapsed before execution start, so re-placement is safe after reaping.
    ExpiredBeforeStart {
        /// Current/expired lease.
        lease: AssignmentLeaseRecord,
    },
    /// The lease elapsed after execution start and therefore requires reconciliation.
    ReconciliationRequired {
        /// Current/expired lease.
        lease: AssignmentLeaseRecord,
    },
    /// The corresponding execution attempt is already terminal.
    ExecutionTerminal {
        /// Current lease binding/timing retained for audit.
        lease: AssignmentLeaseRecord,
        /// Authoritative execution terminal state.
        terminal: AssignmentExecutionTerminal,
    },
}

/// Assignment/lease control-plane error.
#[derive(Debug, Error)]
pub enum AssignmentControlError {
    /// Worker registration, liveness, or matching failed.
    #[error(transparent)]
    Worker(#[from] WorkerControlError),
    /// Execution job preparation/recovery/start failed.
    #[error(transparent)]
    Execution(#[from] ExecutionCoordinatorError),
    /// Assignment event storage failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Assignment facts contradict each other or the execution stream.
    #[error("invalid execution assignment history: {0}")]
    InvalidHistory(String),
    /// Worker session handle no longer names the live registered incarnation/profile.
    #[error("assignment claimant is not the current live worker incarnation")]
    StaleWorkerSession,
    /// Worker heartbeat did not report this attempt active, so renewal is denied.
    #[error("worker heartbeat does not report the leased attempt active")]
    AttemptAbsentFromHeartbeat,
    /// The active-attempt snapshot predates the assignment state it would renew.
    #[error("worker heartbeat predates the accepted assignment state")]
    StaleHeartbeat,
    /// The attempt reached the execution budget its frozen contract declared.
    #[error("attempt exceeded the execution deadline fixed by its contract")]
    AttemptDeadlineExceeded,
    /// Lease has elapsed and cannot grant a new action.
    #[error("assignment lease has expired")]
    LeaseExpired,
    /// Reaping was requested before the lease boundary.
    #[error("assignment lease is not expired")]
    LeaseNotExpired,
    /// Assignment has already advanced beyond the requested transition.
    #[error("assignment transition is not valid from its durable state")]
    InvalidTransition,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LeasedPayload {
    binding: AssignmentBinding,
    granted_at: ObservedAtUnixMillis,
    expires_at: ObservedAtUnixMillis,
    deadline_at: ObservedAtUnixMillis,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "durable acceptance schema keeps explicit typed identity field names"
)]
struct AcceptedPayload {
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    worker_id: WorkerId,
    worker_incarnation_id: WorkerIncarnationId,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RenewedPayload {
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    worker_id: WorkerId,
    worker_incarnation_id: WorkerIncarnationId,
    renewed_at: ObservedAtUnixMillis,
    expires_at: ObservedAtUnixMillis,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExpiredPayload {
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    class: ExpiredLeaseClass,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AssignmentPhase {
    Leased,
    Accepted,
    Expired(ExpiredLeaseClass),
}

struct AssignmentProjection {
    lease: AssignmentLeaseRecord,
    phase: AssignmentPhase,
    used_assignments: BTreeSet<AssignmentId>,
    used_leases: BTreeSet<LeaseId>,
    revision: StreamRevision,
    last_event_id: EventId,
    last_observed_at: ObservedAtUnixMillis,
}

/// Persists a compatible assignment before it can be delivered to a worker.
///
/// # Errors
///
/// Returns an error if the worker is no longer live, matching fails, the assignment already exists,
/// time arithmetic fails, or persistence fails.
pub fn grant_assignment_lease<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    authority: ExecutionAttemptAuthority,
    worker: &RegisteredWorkerSession,
    grant: AssignmentLeaseGrant,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<LeasedExecutionAssignment, AssignmentControlError> {
    ensure_live_worker(events, content, worker, observed_at)?;
    match_worker_at(worker, authority.contract(), observed_at)
        .map_err(WorkerControlError::Match)?;
    let binding = AssignmentBinding {
        assignment_id: grant.assignment_id,
        lease_id: grant.lease_id,
        job_id: authority.job_id(),
        attempt_id: authority.attempt_id(),
        contract_id: authority.contract_id(),
        worker_id: worker.worker_id(),
        worker_incarnation_id: worker.incarnation_id(),
        worker_profile_id: worker.profile_id(),
        offer_message_id: grant.message_ids.offer,
        start_message_id: grant.message_ids.start,
    };
    // The lease answers "is the worker still there"; the deadline answers "has this attempt used
    // up the budget its contract declared". Only the second bounds a worker that stays perfectly
    // reachable while making no progress, which is why it is fixed here from the frozen contract
    // rather than recomputed from anything the worker reports later.
    let deadline_at = attempt_deadline_at(observed_at, authority.contract().resources().timeout())?;
    let expires_at = lease_expiry_at(observed_at, grant.policy.lease_duration)?.min(deadline_at);
    let lease = AssignmentLeaseRecord {
        binding: binding.clone(),
        granted_at: observed_at,
        renewed_at: observed_at,
        expires_at,
        deadline_at,
    };
    let stream = assignment_stream(authority.attempt_id())?;
    let history = events.read_stream(&stream, None)?;
    let (expected, parent) = if history.is_empty() {
        (ExpectedRevision::NoStream, None)
    } else {
        let projection = project_assignment(&history, authority.attempt_id())?;
        if projection.phase != AssignmentPhase::Expired(ExpiredLeaseClass::BeforeExecutionStart)
            || projection.lease.binding.job_id != authority.job_id()
            || projection.lease.binding.contract_id != authority.contract_id()
        {
            return Err(AssignmentControlError::InvalidTransition);
        }
        if projection.used_assignments.contains(&grant.assignment_id)
            || projection.used_leases.contains(&grant.lease_id)
        {
            return invalid_history("assignment or lease identity was reused");
        }
        (
            ExpectedRevision::Exact(projection.revision),
            Some(projection.last_event_id),
        )
    };
    let event = fact(
        ASSIGNMENT_LEASED,
        parent,
        observed_at,
        &LeasedPayload {
            binding,
            granted_at: observed_at,
            expires_at,
            deadline_at,
        },
    )?;
    let outcome = events.append(&stream, expected, command_id, &[event])?;
    Ok(LeasedExecutionAssignment {
        authority,
        lease,
        revision: revision_from_sequence(outcome.last_sequence)?,
        last_event_id: only_event_id(&outcome.event_ids)?,
    })
}

/// Records durable worker acceptance without granting execution authority.
///
/// # Errors
///
/// Returns an error for stale claimant, elapsed lease, concurrent transition, or storage failure.
pub fn accept_assignment<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    leased: LeasedExecutionAssignment,
    worker: &RegisteredWorkerSession,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<AcceptedExecutionAssignment, AssignmentControlError> {
    ensure_claimant(&leased.lease.binding, worker)?;
    ensure_live_worker(events, content, worker, observed_at)?;
    ensure_lease_live(&leased.lease, observed_at)?;
    let event = fact(
        ASSIGNMENT_ACCEPTED,
        Some(leased.last_event_id),
        observed_at,
        &AcceptedPayload {
            assignment_id: leased.lease.binding.assignment_id,
            lease_id: leased.lease.binding.lease_id,
            worker_id: worker.worker_id(),
            worker_incarnation_id: worker.incarnation_id(),
        },
    )?;
    events.append(
        &assignment_stream(leased.lease.binding.attempt_id)?,
        ExpectedRevision::Exact(leased.revision),
        command_id,
        &[event],
    )?;
    Ok(AcceptedExecutionAssignment {
        authority: leased.authority,
        lease: leased.lease,
    })
}

/// Commits the authoritative execution start and returns the only token that may invoke an
/// executor/remote dispatch capability.
///
/// # Errors
///
/// Returns an error if the lease elapsed or the execution start fact cannot commit.
pub fn start_accepted_assignment<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    accepted: AcceptedExecutionAssignment,
    worker: &RegisteredWorkerSession,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StartedExecutionAttempt, AssignmentControlError> {
    ensure_claimant(&accepted.lease.binding, worker)?;
    ensure_live_worker(events, content, worker, observed_at)?;
    ensure_lease_live(&accepted.lease, observed_at)?;
    Ok(begin_execution_attempt(
        events,
        accepted.authority,
        command_id,
        observed_at,
    )?)
}

/// Renews an unexpired lease only when the current live worker heartbeat reports the attempt.
///
/// Heartbeat presence is sufficient for lease renewal but never changes execution outcome.
///
/// # Errors
///
/// Returns an error for stale worker/session, absent active-attempt claim, expired/terminal lease,
/// invalid history, or persistence failure.
pub fn renew_assignment_lease<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    attempt_id: AttemptId,
    worker: &RegisteredWorkerSession,
    policy: AssignmentLeasePolicy,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<AssignmentLeaseRecord, AssignmentControlError> {
    ensure_live_worker(events, content, worker, observed_at)?;
    let stream = assignment_stream(attempt_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_assignment(&history, attempt_id)?;
    ensure_claimant(&projection.lease.binding, worker)?;
    let expires_at =
        lease_expiry_at(observed_at, policy.lease_duration)?.min(projection.lease.deadline_at);
    if projection.lease.renewed_at == observed_at && projection.lease.expires_at == expires_at {
        return Ok(projection.lease);
    }
    match projection.phase {
        AssignmentPhase::Accepted => {}
        AssignmentPhase::Expired(_) => return Err(AssignmentControlError::LeaseExpired),
        AssignmentPhase::Leased => return Err(AssignmentControlError::InvalidTransition),
    }
    // Every other condition below is evidence that the worker is reachable and still claims the
    // attempt. None of them is evidence that the attempt advanced, and no such evidence exists:
    // a wedged compile and a slow one are indistinguishable from outside. The honest control is
    // therefore a budget rather than a progress detector, and it is enforced here so that the
    // controller bounds the attempt independently of the worker's own supervisor, which is the
    // component whose failure this guards against.
    if observed_at >= projection.lease.deadline_at {
        return Err(AssignmentControlError::AttemptDeadlineExceeded);
    }
    ensure_lease_live(&projection.lease, observed_at)?;
    if !matches!(
        recover_bound_execution(events, content, &projection.lease.binding)?,
        ExecutionJobState::ReadyToStart(_) | ExecutionJobState::InDoubt { .. }
    ) {
        return Err(AssignmentControlError::InvalidTransition);
    }
    let availability = worker.availability().ok_or(WorkerControlError::Match(
        crate::WorkerMatchFailure::MissingAvailability,
    ))?;
    // Renewal needs the availability report to be at least as recent as the assignment state it
    // would renew. That is a question about the age of the report, not about whether the worker is
    // still there, so it asks when the availability was observed rather than borrowing the
    // liveness stamp that happens to advance alongside it.
    if worker
        .availability_observed_at()
        .is_none_or(|observed_at| observed_at < projection.last_observed_at)
    {
        return Err(AssignmentControlError::StaleHeartbeat);
    }
    if !availability
        .active_attempts()
        .contains(&projection.lease.binding.attempt_id)
    {
        return Err(AssignmentControlError::AttemptAbsentFromHeartbeat);
    }
    let event = fact(
        ASSIGNMENT_LEASE_RENEWED,
        Some(projection.last_event_id),
        observed_at,
        &RenewedPayload {
            assignment_id: projection.lease.binding.assignment_id,
            lease_id: projection.lease.binding.lease_id,
            worker_id: worker.worker_id(),
            worker_incarnation_id: worker.incarnation_id(),
            renewed_at: observed_at,
            expires_at,
        },
    )?;
    events.append(
        &stream,
        ExpectedRevision::Exact(projection.revision),
        command_id,
        &[event],
    )?;
    Ok(AssignmentLeaseRecord {
        binding: projection.lease.binding,
        granted_at: projection.lease.granted_at,
        renewed_at: observed_at,
        expires_at,
        deadline_at: projection.lease.deadline_at,
    })
}

/// Records an expired lease after consulting the authoritative execution stream.
///
/// # Errors
///
/// Returns an error if the lease is still live, the assignment/job linkage is invalid, or append
/// fails.
pub fn reap_expired_assignment<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    attempt_id: AttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionAssignmentState, AssignmentControlError> {
    let stream = assignment_stream(attempt_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_assignment(&history, attempt_id)?;
    if matches!(projection.phase, AssignmentPhase::Expired(_)) {
        return project_with_execution(events, content, projection, observed_at);
    }
    if observed_at.get() < projection.lease.expires_at.get() {
        return Err(AssignmentControlError::LeaseNotExpired);
    }
    let execution = recover_bound_execution(events, content, &projection.lease.binding)?;
    let class = match execution {
        ExecutionJobState::ReadyToStart(_) => ExpiredLeaseClass::BeforeExecutionStart,
        ExecutionJobState::InDoubt { .. } => ExpiredLeaseClass::ExecutionInDoubt,
        ExecutionJobState::Completed { .. }
        | ExecutionJobState::NotStarted { .. }
        | ExecutionJobState::Ambiguous { .. } => {
            return project_with_execution(events, content, projection, observed_at);
        }
        ExecutionJobState::NotFound => {
            return invalid_history("assignment cites a missing execution job");
        }
    };
    let event = fact(
        ASSIGNMENT_LEASE_EXPIRED,
        Some(projection.last_event_id),
        observed_at,
        &ExpiredPayload {
            assignment_id: projection.lease.binding.assignment_id,
            lease_id: projection.lease.binding.lease_id,
            class,
        },
    )?;
    events.append(
        &stream,
        ExpectedRevision::Exact(projection.revision),
        command_id,
        &[event],
    )?;
    let lease = projection.lease;
    Ok(match class {
        ExpiredLeaseClass::BeforeExecutionStart => {
            ExecutionAssignmentState::ExpiredBeforeStart { lease }
        }
        ExpiredLeaseClass::ExecutionInDoubt => {
            ExecutionAssignmentState::ReconciliationRequired { lease }
        }
    })
}

/// Reconstructs assignment state from its facts, the execution-job stream, and verified CAS.
///
/// Clock-derived expiry is returned conservatively even before the explicit reaper commits the
/// expiry fact. No execution or reassignment authority is reconstructed from that derived state.
///
/// # Errors
///
/// Returns an error for missing/contradictory facts, broken job linkage, or invalid CAS content.
pub fn recover_execution_assignment<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    attempt_id: AttemptId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionAssignmentState, AssignmentControlError> {
    let history = events.read_stream(&assignment_stream(attempt_id)?, None)?;
    if history.is_empty() {
        return Ok(ExecutionAssignmentState::NotFound);
    }
    project_with_execution(
        events,
        content,
        project_assignment(&history, attempt_id)?,
        observed_at,
    )
}

fn project_with_execution<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    projection: AssignmentProjection,
    observed_at: ObservedAtUnixMillis,
) -> Result<ExecutionAssignmentState, AssignmentControlError> {
    if observed_at < projection.last_observed_at {
        return invalid_history("assignment recovery observation time regressed");
    }
    let execution = recover_bound_execution(events, content, &projection.lease.binding)?;
    match execution {
        ExecutionJobState::ReadyToStart(authority) => {
            if projection.phase == AssignmentPhase::Expired(ExpiredLeaseClass::ExecutionInDoubt) {
                return invalid_history("pre-start execution contradicts in-doubt lease expiry");
            }
            if matches!(projection.phase, AssignmentPhase::Expired(_))
                || observed_at.get() >= projection.lease.expires_at.get()
            {
                return Ok(ExecutionAssignmentState::ExpiredBeforeStart {
                    lease: projection.lease,
                });
            }
            Ok(match projection.phase {
                AssignmentPhase::Leased => {
                    ExecutionAssignmentState::Leased(LeasedExecutionAssignment {
                        authority,
                        lease: projection.lease,
                        revision: projection.revision,
                        last_event_id: projection.last_event_id,
                    })
                }
                AssignmentPhase::Accepted => {
                    ExecutionAssignmentState::Accepted(AcceptedExecutionAssignment {
                        authority,
                        lease: projection.lease,
                    })
                }
                AssignmentPhase::Expired(_) => {
                    return invalid_history("expired assignment retained action authority");
                }
            })
        }
        ExecutionJobState::InDoubt { .. } => {
            if projection.phase == AssignmentPhase::Expired(ExpiredLeaseClass::BeforeExecutionStart)
            {
                return invalid_history("started execution contradicts pre-start lease expiry");
            }
            if projection.phase == AssignmentPhase::Expired(ExpiredLeaseClass::ExecutionInDoubt)
                || observed_at.get() >= projection.lease.expires_at.get()
            {
                Ok(ExecutionAssignmentState::ReconciliationRequired {
                    lease: projection.lease,
                })
            } else {
                Ok(ExecutionAssignmentState::Running {
                    lease: projection.lease,
                })
            }
        }
        ExecutionJobState::Completed { receipt_id, .. } => {
            Ok(ExecutionAssignmentState::ExecutionTerminal {
                lease: projection.lease,
                terminal: AssignmentExecutionTerminal::Completed { receipt_id },
            })
        }
        ExecutionJobState::NotStarted { .. } => Ok(ExecutionAssignmentState::ExecutionTerminal {
            lease: projection.lease,
            terminal: AssignmentExecutionTerminal::NotStarted,
        }),
        ExecutionJobState::Ambiguous { .. } => Ok(ExecutionAssignmentState::ExecutionTerminal {
            lease: projection.lease,
            terminal: AssignmentExecutionTerminal::Ambiguous,
        }),
        ExecutionJobState::NotFound => invalid_history("assignment cites a missing execution job"),
    }
}

fn recover_bound_execution<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    binding: &AssignmentBinding,
) -> Result<ExecutionJobState, AssignmentControlError> {
    let job = ExecutionJob::new(binding.job_id)?;
    let state = recover_execution_job(events, content, &job)?;
    match &state {
        ExecutionJobState::ReadyToStart(authority)
            if authority.attempt_id() == binding.attempt_id
                && authority.contract_id() == binding.contract_id => {}
        ExecutionJobState::InDoubt { attempt_id }
        | ExecutionJobState::NotStarted { attempt_id, .. }
        | ExecutionJobState::Ambiguous { attempt_id, .. }
            if *attempt_id == binding.attempt_id => {}
        ExecutionJobState::Completed { receipt, .. }
            if receipt.attempt_id() == binding.attempt_id
                && receipt.contract_id() == binding.contract_id => {}
        _ => return invalid_history("assignment does not match execution job state"),
    }
    Ok(state)
}

#[expect(
    clippy::too_many_lines,
    reason = "the assignment event-fold transition table remains contiguous for audit"
)]
fn project_assignment(
    events: &[EventEnvelope],
    expected_attempt_id: AttemptId,
) -> Result<AssignmentProjection, AssignmentControlError> {
    let mut projection: Option<AssignmentProjection> = None;
    let mut previous = None;
    for event in events {
        if event.schema_version.get() != 1 || event.parent_event_id != previous {
            return invalid_history("assignment event version or causal chain is invalid");
        }
        match event.schema_name.as_str() {
            ASSIGNMENT_LEASED => {
                let payload: LeasedPayload = decode(event)?;
                if payload.binding.attempt_id != expected_attempt_id
                    || payload.expires_at <= payload.granted_at
                {
                    return invalid_history("assignment lease identity or time bounds are invalid");
                }
                let (mut used_assignments, mut used_leases) = if let Some(previous) = projection {
                    if previous.phase
                        != AssignmentPhase::Expired(ExpiredLeaseClass::BeforeExecutionStart)
                        || previous.lease.binding.job_id != payload.binding.job_id
                        || previous.lease.binding.contract_id != payload.binding.contract_id
                        || event.observed_at_unix_ms < previous.last_observed_at.get()
                    {
                        return invalid_history("assignment was replaced from an unsafe state");
                    }
                    (previous.used_assignments, previous.used_leases)
                } else {
                    (BTreeSet::new(), BTreeSet::new())
                };
                if !used_assignments.insert(payload.binding.assignment_id)
                    || !used_leases.insert(payload.binding.lease_id)
                {
                    return invalid_history("assignment or lease identity was reused");
                }
                projection = Some(AssignmentProjection {
                    lease: AssignmentLeaseRecord {
                        binding: payload.binding,
                        granted_at: payload.granted_at,
                        renewed_at: payload.granted_at,
                        expires_at: payload.expires_at,
                        deadline_at: payload.deadline_at,
                    },
                    phase: AssignmentPhase::Leased,
                    used_assignments,
                    used_leases,
                    revision: revision(event)?,
                    last_event_id: event.event_id,
                    last_observed_at: ObservedAtUnixMillis::new(event.observed_at_unix_ms),
                });
            }
            ASSIGNMENT_ACCEPTED => {
                let payload: AcceptedPayload = decode(event)?;
                let state = projection.as_mut().ok_or_else(|| {
                    AssignmentControlError::InvalidHistory("acceptance before lease".into())
                })?;
                validate_claim_payload(
                    payload.assignment_id,
                    payload.lease_id,
                    payload.worker_id,
                    payload.worker_incarnation_id,
                    &state.lease.binding,
                )?;
                if state.phase != AssignmentPhase::Leased
                    || event.observed_at_unix_ms < state.last_observed_at.get()
                    || event.observed_at_unix_ms >= state.lease.expires_at.get()
                {
                    return invalid_history("assignment acceptance transition is invalid");
                }
                state.phase = AssignmentPhase::Accepted;
                advance_projection(state, event)?;
            }
            ASSIGNMENT_LEASE_RENEWED => {
                let payload: RenewedPayload = decode(event)?;
                let state = projection.as_mut().ok_or_else(|| {
                    AssignmentControlError::InvalidHistory("renewal before lease".into())
                })?;
                validate_claim_payload(
                    payload.assignment_id,
                    payload.lease_id,
                    payload.worker_id,
                    payload.worker_incarnation_id,
                    &state.lease.binding,
                )?;
                if matches!(state.phase, AssignmentPhase::Expired(_))
                    || payload.renewed_at.get() != event.observed_at_unix_ms
                    || payload.renewed_at < state.last_observed_at
                    || payload.renewed_at >= state.lease.expires_at
                    || payload.expires_at <= payload.renewed_at
                {
                    return invalid_history("assignment lease renewal transition is invalid");
                }
                state.lease.renewed_at = payload.renewed_at;
                state.lease.expires_at = payload.expires_at;
                advance_projection(state, event)?;
            }
            ASSIGNMENT_LEASE_EXPIRED => {
                let payload: ExpiredPayload = decode(event)?;
                let state = projection.as_mut().ok_or_else(|| {
                    AssignmentControlError::InvalidHistory("expiry before lease".into())
                })?;
                if payload.assignment_id != state.lease.binding.assignment_id
                    || payload.lease_id != state.lease.binding.lease_id
                    || matches!(state.phase, AssignmentPhase::Expired(_))
                    || event.observed_at_unix_ms < state.lease.expires_at.get()
                {
                    return invalid_history("assignment lease expiry transition is invalid");
                }
                state.phase = AssignmentPhase::Expired(payload.class);
                advance_projection(state, event)?;
            }
            _ => return invalid_history("unknown assignment event schema"),
        }
        previous = Some(event.event_id);
    }
    projection.ok_or_else(|| AssignmentControlError::InvalidHistory("missing lease".into()))
}

fn advance_projection(
    projection: &mut AssignmentProjection,
    event: &EventEnvelope,
) -> Result<(), AssignmentControlError> {
    projection.revision = revision(event)?;
    projection.last_event_id = event.event_id;
    projection.last_observed_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
    Ok(())
}

fn validate_claim_payload(
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    worker_id: WorkerId,
    worker_incarnation_id: WorkerIncarnationId,
    binding: &AssignmentBinding,
) -> Result<(), AssignmentControlError> {
    if assignment_id != binding.assignment_id
        || lease_id != binding.lease_id
        || worker_id != binding.worker_id
        || worker_incarnation_id != binding.worker_incarnation_id
    {
        return invalid_history("assignment claimant identity changed");
    }
    Ok(())
}

fn ensure_claimant(
    binding: &AssignmentBinding,
    worker: &RegisteredWorkerSession,
) -> Result<(), AssignmentControlError> {
    if binding.worker_id != worker.worker_id()
        || binding.worker_incarnation_id != worker.incarnation_id()
        || binding.worker_profile_id != worker.profile_id()
    {
        return Err(AssignmentControlError::StaleWorkerSession);
    }
    Ok(())
}

fn ensure_live_worker<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    worker: &RegisteredWorkerSession,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), AssignmentControlError> {
    let WorkerSessionState::Live(current) =
        recover_worker_session(events, content, worker.worker_id(), observed_at)?
    else {
        return Err(AssignmentControlError::StaleWorkerSession);
    };
    if current.incarnation_id() != worker.incarnation_id()
        || current.profile_id() != worker.profile_id()
        || current.availability_id() != worker.availability_id()
    {
        return Err(AssignmentControlError::StaleWorkerSession);
    }
    Ok(())
}

fn ensure_lease_live(
    lease: &AssignmentLeaseRecord,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), AssignmentControlError> {
    if observed_at < lease.renewed_at {
        return invalid_history("assignment observation time regressed");
    }
    if observed_at.get() >= lease.expires_at.get() {
        Err(AssignmentControlError::LeaseExpired)
    } else {
        Ok(())
    }
}

fn lease_expiry_at(
    base: ObservedAtUnixMillis,
    duration: AssignmentLeaseDurationMillis,
) -> Result<ObservedAtUnixMillis, AssignmentControlError> {
    let duration = i64::try_from(duration.get())
        .map_err(|_| AssignmentControlError::InvalidHistory("lease duration exceeds i64".into()))?;
    base.get()
        .checked_add(duration)
        .map(ObservedAtUnixMillis::new)
        .ok_or_else(|| AssignmentControlError::InvalidHistory("lease expiry overflowed".into()))
}

fn attempt_deadline_at(
    base: ObservedAtUnixMillis,
    timeout: crate::ExecutionTimeoutMillis,
) -> Result<ObservedAtUnixMillis, AssignmentControlError> {
    let timeout = i64::try_from(timeout.get()).map_err(|_| {
        AssignmentControlError::InvalidHistory("execution timeout exceeds i64".into())
    })?;
    base.get()
        .checked_add(timeout)
        .map(ObservedAtUnixMillis::new)
        .ok_or_else(|| AssignmentControlError::InvalidHistory("attempt deadline overflowed".into()))
}

fn assignment_stream(attempt_id: AttemptId) -> Result<StreamId, AssignmentControlError> {
    Ok(StreamId {
        kind: AggregateKind::new("execution-assignment")
            .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new(attempt_id.to_string())
            .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))?,
    })
}

fn fact<P: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, AssignmentControlError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<T, AssignmentControlError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))
}

fn revision(event: &EventEnvelope) -> Result<StreamRevision, AssignmentControlError> {
    revision_from_sequence(event.sequence)
}

fn revision_from_sequence(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, AssignmentControlError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| AssignmentControlError::InvalidHistory(error.to_string()))
}

fn only_event_id(event_ids: &[EventId]) -> Result<EventId, AssignmentControlError> {
    if let [event_id] = event_ids {
        Ok(*event_id)
    } else {
        invalid_history("event store returned an invalid append outcome")
    }
}

fn invalid_history<T>(message: &str) -> Result<T, AssignmentControlError> {
    Err(AssignmentControlError::InvalidHistory(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{
        AssignmentId, CommandId, ContentId, ContentType, ControlMessageId, JobId, LeaseId,
        WorkerId, WorkerIncarnationId,
    };
    use cairn_record::ContentStore;
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::*;
    use crate::{
        ArchitectureName, AuthenticatedWorkerIdentity, CapturePolicy, CommandContract,
        DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionEnvironmentArtifact,
        ExecutionPlatform, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
        InputBundleArtifact, NetworkPolicy, OperatingSystemName, OutputByteLimit, PlacementRequest,
        RecordedWorkerAuthenticator, ResourceRequest, SandboxPath, TargetEnvironmentName,
        WorkerAuthenticationSubject, WorkerAvailability, WorkerBinaryIdentity, WorkerHealth,
        WorkerHello, WorkerPoolName, WorkerProfile, WorkerProtocolVersion, WorkerResourceClaim,
        WorkerResourceInventory, WorkerResourceSource, WorkerSlotCount,
        authorize_execution_attempt, disconnect_worker, prepare_execution_job,
        record_worker_heartbeat, register_worker,
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        content_database: std::path::PathBuf,
        event_database: std::path::PathBuf,
        cas: std::path::PathBuf,
        content: SqliteContentStore,
        events: SqliteEventStore,
        contract: crate::JobContract,
        worker_id: WorkerId,
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_execution_timeout(1_000)
        }

        fn with_execution_timeout(timeout_ms: u64) -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let content_database = directory.path().join("content.db");
            let event_database = directory.path().join("events.db");
            let cas = directory.path().join("cas");
            let mut content = SqliteContentStore::open(&content_database, &cas).expect("content");
            let events = SqliteEventStore::open(&event_database).expect("events");
            let input = put::<InputBundleArtifact>(&mut content, b"input");
            let environment =
                put::<ExecutionEnvironmentArtifact>(&mut content, br#"{"image":"sha256:fixture"}"#);
            let contract = crate::JobContract::new(
                JobId::new(),
                input,
                environment,
                ExecutionBackend::new("container").expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/run").expect("program"),
                    Vec::new(),
                    SandboxPath::new("work").expect("working directory"),
                ),
                ResourceRequest::new(
                    ExecutionTimeoutMillis::new(timeout_ms).expect("timeout"),
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
                    Vec::new(),
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
                worker_id: WorkerId::new(),
            }
        }

        fn reopen(&mut self) {
            self.content = SqliteContentStore::open(&self.content_database, &self.cas)
                .expect("reopen content");
            self.events = SqliteEventStore::open(&self.event_database).expect("reopen events");
        }

        fn register_worker(&mut self) -> RegisteredWorkerSession {
            let profile = WorkerProfile::new(
                WorkerProtocolVersion::new(1).expect("protocol"),
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
                        ExecutionBackend::new("container").expect("backend"),
                        WorkerResourceSource::OperatorDeclared,
                    )],
                    Vec::new(),
                    crate::worker::test_resource_observation(0),
                    WorkerSlotCount::new(1).expect("slots"),
                )
                .expect("resources"),
            )
            .expect("profile");
            let hello = WorkerHello::new(self.worker_id, WorkerIncarnationId::new(), profile);
            let mut authenticator = RecordedWorkerAuthenticator::new([(
                self.worker_id,
                AuthenticatedWorkerIdentity::new(
                    WorkerAuthenticationSubject::new("spiffe://cairn/worker/fixture")
                        .expect("subject"),
                    cairn_protocol::CredentialId::new(),
                    WorkerPoolName::new("fixture").expect("pool"),
                ),
            )]);
            register_worker(
                &mut self.events,
                &mut self.content,
                &mut authenticator,
                &hello,
                &CommandId::new(),
                ObservedAtUnixMillis::new(0),
            )
            .expect("register")
        }

        fn heartbeat(
            &mut self,
            session: &RegisteredWorkerSession,
            active_attempts: Vec<AttemptId>,
            observed_at: i64,
        ) -> RegisteredWorkerSession {
            record_worker_heartbeat(
                &mut self.events,
                &mut self.content,
                session,
                &WorkerAvailability::new(WorkerHealth::Ready, false, 1, active_attempts)
                    .expect("availability"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_at),
            )
            .expect("heartbeat")
        }

        fn authority(&mut self, attempt_id: AttemptId) -> ExecutionAttemptAuthority {
            let prepared =
                prepare_execution_job(&mut self.content, &self.contract).expect("prepare");
            authorize_execution_attempt(
                &mut self.events,
                prepared,
                attempt_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(1),
            )
            .expect("authorize")
        }
    }

    // Renewal's other conditions all establish that the worker is reachable and still claims the
    // attempt. A worker stuck in a loop satisfies every one of them forever, so without a budget
    // the lease renews for as long as the process stays up. The contract's execution timeout is
    // that budget, and the controller has to enforce it itself: the worker's own supervisor is
    // the component whose failure this exists to survive.
    #[test]
    fn a_perfectly_healthy_worker_cannot_renew_past_the_contract_budget() {
        let mut fixture = Fixture::with_execution_timeout(5);
        let worker = fixture.register_worker();
        let worker = fixture.heartbeat(&worker, Vec::new(), 1);
        let attempt_id = AttemptId::new();
        let authority = fixture.authority(attempt_id);
        let leased = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                message_ids(),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("lease");
        // The lease duration is ten and the budget is five, so the grant is cut to the budget
        // rather than authorizing work the contract never allowed for.
        assert_eq!(leased.lease().deadline_at(), ObservedAtUnixMillis::new(7));
        assert_eq!(leased.lease().expires_at(), ObservedAtUnixMillis::new(7));

        let accepted = accept_assignment(
            &mut fixture.events,
            &fixture.content,
            leased,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("accept");
        start_accepted_assignment(
            &mut fixture.events,
            &fixture.content,
            accepted,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("start");

        let worker = fixture.heartbeat(&worker, vec![attempt_id], 5);
        renew_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            attempt_id,
            &worker,
            lease_policy(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("renewal inside the budget");

        // Same worker, same health, same attempt still reported active. Only the budget ran out.
        let worker = fixture.heartbeat(&worker, vec![attempt_id], 7);
        assert!(
            matches!(
                renew_assignment_lease(
                    &mut fixture.events,
                    &fixture.content,
                    attempt_id,
                    &worker,
                    lease_policy(),
                    &CommandId::new(),
                    ObservedAtUnixMillis::new(7),
                ),
                Err(AssignmentControlError::AttemptDeadlineExceeded)
            ),
            "an exhausted budget must be reported as itself, not as an ordinary lease expiry"
        );
    }

    fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put")
            .content_id
    }

    fn lease_policy() -> AssignmentLeasePolicy {
        AssignmentLeasePolicy::new(AssignmentLeaseDurationMillis::new(10).expect("lease duration"))
    }

    fn message_ids() -> AssignmentControlMessageIds {
        AssignmentControlMessageIds::new(ControlMessageId::new(), ControlMessageId::new())
    }

    #[test]
    fn active_assignment_is_unique_and_prestart_expiry_can_be_safely_replaced() {
        let mut fixture = Fixture::new();
        let registered = fixture.register_worker();
        let worker = fixture.heartbeat(&registered, Vec::new(), 1);
        let attempt_id = AttemptId::new();
        let authority = fixture.authority(attempt_id);
        let first_assignment = AssignmentId::new();
        let first_lease = LeaseId::new();
        let leased = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(first_assignment, first_lease, message_ids(), lease_policy()),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("lease");
        assert_eq!(leased.lease().binding().attempt_id(), attempt_id);
        fixture.reopen();

        let job = ExecutionJob::new(fixture.contract.job_id()).expect("job");
        let ExecutionJobState::ReadyToStart(duplicate_authority) =
            recover_execution_job(&fixture.events, &fixture.content, &job).expect("recover job")
        else {
            panic!("ready");
        };
        let WorkerSessionState::Live(worker) = recover_worker_session(
            &fixture.events,
            &fixture.content,
            fixture.worker_id,
            ObservedAtUnixMillis::new(3),
        )
        .expect("worker") else {
            panic!("live worker");
        };
        assert!(matches!(
            grant_assignment_lease(
                &mut fixture.events,
                &fixture.content,
                duplicate_authority,
                &worker,
                AssignmentLeaseGrant::new(
                    AssignmentId::new(),
                    LeaseId::new(),
                    message_ids(),
                    lease_policy(),
                ),
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(AssignmentControlError::InvalidTransition)
        ));
        assert!(matches!(
            recover_execution_assignment(
                &fixture.events,
                &fixture.content,
                attempt_id,
                ObservedAtUnixMillis::new(12),
            )
            .expect("derive expiry"),
            ExecutionAssignmentState::ExpiredBeforeStart { .. }
        ));
        reap_expired_assignment(
            &mut fixture.events,
            &fixture.content,
            attempt_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(12),
        )
        .expect("reap");
        let ExecutionJobState::ReadyToStart(authority) =
            recover_execution_job(&fixture.events, &fixture.content, &job).expect("recover job")
        else {
            panic!("ready after safe expiry");
        };
        let second_assignment = AssignmentId::new();
        let second_lease = LeaseId::new();
        let replacement = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                second_assignment,
                second_lease,
                message_ids(),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(13),
        )
        .expect("replacement");
        assert_ne!(
            replacement.lease().binding().assignment_id(),
            first_assignment
        );
        assert_ne!(replacement.lease().binding().lease_id(), first_lease);
        assert_eq!(replacement.lease().binding().attempt_id(), attempt_id);
    }

    #[test]
    fn started_lease_expiry_requires_reconciliation_after_restart() {
        let mut fixture = Fixture::new();
        let registered = fixture.register_worker();
        let worker = fixture.heartbeat(&registered, Vec::new(), 1);
        let attempt_id = AttemptId::new();
        let authority = fixture.authority(attempt_id);
        let leased = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                message_ids(),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("lease");
        let accepted = accept_assignment(
            &mut fixture.events,
            &fixture.content,
            leased,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("accept");
        let started = start_accepted_assignment(
            &mut fixture.events,
            &fixture.content,
            accepted,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("start");
        assert_eq!(started.attempt_id(), attempt_id);
        let worker = fixture.heartbeat(&worker, vec![attempt_id], 5);
        let renewed = renew_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            attempt_id,
            &worker,
            lease_policy(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("renew");
        assert_eq!(renewed.expires_at(), ObservedAtUnixMillis::new(15));
        fixture.reopen();
        assert!(matches!(
            recover_execution_assignment(
                &fixture.events,
                &fixture.content,
                attempt_id,
                ObservedAtUnixMillis::new(14),
            )
            .expect("running"),
            ExecutionAssignmentState::Running { .. }
        ));
        assert!(matches!(
            recover_execution_assignment(
                &fixture.events,
                &fixture.content,
                attempt_id,
                ObservedAtUnixMillis::new(15),
            )
            .expect("in doubt"),
            ExecutionAssignmentState::ReconciliationRequired { .. }
        ));
        assert!(matches!(
            reap_expired_assignment(
                &mut fixture.events,
                &fixture.content,
                attempt_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(15),
            )
            .expect("reap"),
            ExecutionAssignmentState::ReconciliationRequired { .. }
        ));
    }

    #[test]
    fn lease_renewal_requires_an_active_attempt_heartbeat() {
        let mut fixture = Fixture::new();
        let registered = fixture.register_worker();
        let worker = fixture.heartbeat(&registered, Vec::new(), 1);
        let attempt_id = AttemptId::new();
        let authority = fixture.authority(attempt_id);
        let leased = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                message_ids(),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("lease");
        let accepted = accept_assignment(
            &mut fixture.events,
            &fixture.content,
            leased,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("accept");
        let _started = start_accepted_assignment(
            &mut fixture.events,
            &fixture.content,
            accepted,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("start");
        let worker = fixture.heartbeat(&worker, Vec::new(), 5);
        assert!(matches!(
            renew_assignment_lease(
                &mut fixture.events,
                &fixture.content,
                attempt_id,
                &worker,
                lease_policy(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(5),
            ),
            Err(AssignmentControlError::AttemptAbsentFromHeartbeat)
        ));
    }

    #[test]
    fn disconnected_worker_cannot_turn_acceptance_into_execution_authority() {
        let mut fixture = Fixture::new();
        let registered = fixture.register_worker();
        let worker = fixture.heartbeat(&registered, Vec::new(), 1);
        let attempt_id = AttemptId::new();
        let authority = fixture.authority(attempt_id);
        let leased = grant_assignment_lease(
            &mut fixture.events,
            &fixture.content,
            authority,
            &worker,
            AssignmentLeaseGrant::new(
                AssignmentId::new(),
                LeaseId::new(),
                message_ids(),
                lease_policy(),
            ),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("lease");
        let accepted = accept_assignment(
            &mut fixture.events,
            &fixture.content,
            leased,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("accept");
        disconnect_worker(
            &mut fixture.events,
            &worker,
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("disconnect");
        assert!(matches!(
            start_accepted_assignment(
                &mut fixture.events,
                &fixture.content,
                accepted,
                &worker,
                &CommandId::new(),
                ObservedAtUnixMillis::new(5),
            ),
            Err(AssignmentControlError::StaleWorkerSession)
        ));
        assert!(matches!(
            recover_execution_assignment(
                &fixture.events,
                &fixture.content,
                attempt_id,
                ObservedAtUnixMillis::new(5),
            )
            .expect("recover"),
            ExecutionAssignmentState::Accepted(_)
        ));
    }
}
