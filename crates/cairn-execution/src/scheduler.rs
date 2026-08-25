use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

use cairn_protocol::{
    AggregateId, AggregateKind, AssignmentId, AttemptId, CommandId, ContentId, ContentType,
    CredentialId, EventId, LeaseId, ObservedAtUnixMillis, PlacementId, ReservationId, SchemaName,
    SchemaVersion, StreamRevision, WorkerId, WorkerIncarnationId,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AcceleratorDeviceId, AssignmentControlError, AssignmentLeaseGrant, ExecutionAssignmentState,
    ExecutionAttemptAuthority, JobContract, JobContractArtifact, LeasedExecutionAssignment,
    LogicalCpuCount, MemoryByteCount, QuantitativeResourceRequest, RegisteredWorkerSession,
    ReservationClaimTimeoutMillis, ScratchByteCount, WorkerAvailabilityArtifact,
    WorkerControlError, WorkerMatchFailure, WorkerProfileArtifact, WorkerResourceObservation,
    WorkerResourceObservationArtifact, WorkerSessionState, WorkerSessionTimeoutMillis,
    grant_assignment_lease, match_worker_at, recover_execution_assignment, recover_worker_session,
};

const PLACEMENT_RECORDED: &str = "execution.placement-recorded";
const RESERVATION_RELEASED: &str = "execution.placement-reservation-released";

/// Immutable content domain for a complete scheduler candidate evaluation.
pub struct PlacementSnapshotArtifact;
impl ContentType for PlacementSnapshotArtifact {
    const DOMAIN: &'static str = "execution.placement-snapshot.v2";
}

/// Deterministic scheduler algorithm frozen into each placement snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulerPolicyVersion {
    /// Historical slot-only policy retained only for explicit pre-public rejection/migration.
    StableWorkerIdV1,
    /// Canonical filtering, quantitative reservation, then ascending stable `WorkerId`.
    StableWorkerIdQuantitativeV2,
}

/// Configurable scheduler policy that separates liveness and orphan-claim timing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerPolicy {
    version: SchedulerPolicyVersion,
    session_timeout: WorkerSessionTimeoutMillis,
    reservation_claim_timeout: ReservationClaimTimeoutMillis,
}

impl SchedulerPolicy {
    /// Creates an explicit versioned policy with independently typed positive time bounds.
    #[must_use]
    pub const fn new(
        version: SchedulerPolicyVersion,
        session_timeout: WorkerSessionTimeoutMillis,
        reservation_claim_timeout: ReservationClaimTimeoutMillis,
    ) -> Self {
        Self {
            version,
            session_timeout,
            reservation_claim_timeout,
        }
    }

    /// Returns the deterministic selection algorithm.
    #[must_use]
    pub const fn version(self) -> SchedulerPolicyVersion {
        self.version
    }

    /// Returns the worker-session liveness timeout used for the frozen snapshot.
    #[must_use]
    pub const fn session_timeout(self) -> WorkerSessionTimeoutMillis {
        self.session_timeout
    }

    /// Returns the deadline for safely identifying an unclaimed orphan reservation.
    #[must_use]
    pub const fn reservation_claim_timeout(self) -> ReservationClaimTimeoutMillis {
        self.reservation_claim_timeout
    }
}

/// Trusted application authority consulted independently of worker self-reports.
pub trait WorkerPlacementAuthority {
    /// Observes whether the exact worker credential remains authorized at `observed_at`.
    ///
    /// # Errors
    ///
    /// Returns an adapter-neutral error when authority cannot be established. Callers must fail
    /// closed rather than treating an unavailable registry as an ordinary rejected candidate.
    fn observe_credential_authority(
        &self,
        worker_id: WorkerId,
        credential_id: CredentialId,
        observed_at: ObservedAtUnixMillis,
    ) -> Result<PlacementAuthorityObservation, PlacementAuthorityError>;
}

/// Controller-owned authority result captured independently of worker claims.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementAuthorityObservation {
    active: bool,
    evidence_revision: Option<EventId>,
}

impl PlacementAuthorityObservation {
    /// Creates one authority observation. Event-sourced adapters should cite their latest event;
    /// external/static transition adapters may use `None` until imported into managed history.
    #[must_use]
    pub const fn new(active: bool, evidence_revision: Option<EventId>) -> Self {
        Self {
            active,
            evidence_revision,
        }
    }

    /// Returns whether this exact credential was active.
    #[must_use]
    pub const fn active(self) -> bool {
        self.active
    }

    /// Returns the authority event revision supporting the observation, when available.
    #[must_use]
    pub const fn evidence_revision(self) -> Option<EventId> {
        self.evidence_revision
    }
}

/// Adapter-neutral failure to establish current worker authority.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("worker placement authority is unavailable: {0}")]
pub struct PlacementAuthorityError(String);

impl PlacementAuthorityError {
    /// Creates a bounded-domain diagnostic without exposing an adapter type.
    #[must_use]
    pub fn new(diagnostic: impl Into<String>) -> Self {
        Self(diagnostic.into())
    }
}

/// Stable, replayable reason one worker was excluded from a placement snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum PlacementCandidateRejection {
    /// No registration facts exist for the requested stable worker.
    NotFound,
    /// The last registered incarnation disconnected explicitly.
    Disconnected,
    /// The last registered incarnation exceeded the configured liveness boundary.
    Expired,
    /// Controller credential/worker authority is inactive.
    AuthorityInactive,
    /// Static or dynamic worker properties do not match the frozen contract.
    WorkerMismatch(WorkerMatchFailure),
    /// Existing durable reservations consume all registered capacity.
    CapacityReserved,
    /// Existing durable reservations consume requested quantitative resources.
    QuantitativeCapacityReserved,
}

/// Exact quantities and device identities consumed by one durable scheduler reservation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReservedWorkerResources {
    logical_cpus: Option<LogicalCpuCount>,
    memory_bytes: Option<MemoryByteCount>,
    scratch_bytes: Option<ScratchByteCount>,
    accelerator_device_ids: Vec<AcceleratorDeviceId>,
}

impl ReservedWorkerResources {
    /// Returns reserved logical CPUs, or `None` when the contract did not request them.
    #[must_use]
    pub const fn logical_cpus(&self) -> Option<LogicalCpuCount> {
        self.logical_cpus
    }

    /// Returns reserved memory bytes, or `None` when the contract did not request them.
    #[must_use]
    pub const fn memory_bytes(&self) -> Option<MemoryByteCount> {
        self.memory_bytes
    }

    /// Returns reserved scratch bytes, or `None` when the contract did not request them.
    #[must_use]
    pub const fn scratch_bytes(&self) -> Option<ScratchByteCount> {
        self.scratch_bytes
    }

    /// Returns canonical accelerator device identities exclusively held by the reservation.
    #[must_use]
    pub fn accelerator_device_ids(&self) -> &[AcceleratorDeviceId] {
        &self.accelerator_device_ids
    }

    fn validate(&self) -> Result<(), SchedulerError> {
        if self
            .accelerator_device_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            invalid_history("reserved accelerator devices are not canonical")
        } else {
            Ok(())
        }
    }
}

/// Explainable result of evaluating one canonical candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum CandidateDisposition {
    /// Candidate passed authority, liveness, matching, and reservation-capacity checks.
    Eligible {
        /// Static maximum concurrency registered by the worker profile.
        registered_slots: u16,
        /// Latest worker-reported dynamic availability.
        reported_available_slots: u16,
        /// Unreleased scheduler reservations already bound to this stable worker.
        active_reservations: u16,
        /// Reservations not yet reflected in the worker's active-attempt heartbeat set.
        unreflected_reservations: u16,
        /// Exact quantitative resources this candidate would reserve.
        quantitative_reservation: ReservedWorkerResources,
    },
    /// Candidate failed one deterministic gate.
    Rejected {
        /// First stable rejection reason.
        reason: PlacementCandidateRejection,
    },
}

/// Exact worker evidence considered by one scheduler decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementCandidateSnapshot {
    worker_id: WorkerId,
    incarnation_id: Option<WorkerIncarnationId>,
    credential_id: Option<CredentialId>,
    profile_id: Option<ContentId<WorkerProfileArtifact>>,
    resource_observation_id: Option<ContentId<WorkerResourceObservationArtifact>>,
    resource_observation_revision: Option<EventId>,
    resource_admission_revision: Option<EventId>,
    availability_id: Option<ContentId<WorkerAvailabilityArtifact>>,
    last_seen_at: Option<ObservedAtUnixMillis>,
    authority_revision: Option<EventId>,
    disposition: CandidateDisposition,
}

impl PlacementCandidateSnapshot {
    /// Returns the stable worker considered by this entry.
    #[must_use]
    pub const fn worker_id(&self) -> WorkerId {
        self.worker_id
    }

    /// Returns the complete deterministic disposition.
    #[must_use]
    pub const fn disposition(&self) -> &CandidateDisposition {
        &self.disposition
    }

    /// Returns the controller authority event cited by this evaluation, when available.
    #[must_use]
    pub const fn authority_revision(&self) -> Option<EventId> {
        self.authority_revision
    }
}

/// Immutable candidate set, evidence, policy, rejection trace, and deterministic choice.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementSnapshot {
    schema_version: u16,
    placement_id: PlacementId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    observed_at: ObservedAtUnixMillis,
    policy: SchedulerPolicy,
    candidates: Vec<PlacementCandidateSnapshot>,
    selected_worker_id: Option<WorkerId>,
}

impl PlacementSnapshot {
    /// Returns canonical candidate entries in ascending stable-worker order.
    #[must_use]
    pub fn candidates(&self) -> &[PlacementCandidateSnapshot] {
        &self.candidates
    }

    /// Returns the deterministic choice, or `None` when every candidate was rejected.
    #[must_use]
    pub const fn selected_worker_id(&self) -> Option<WorkerId> {
        self.selected_worker_id
    }

    /// Returns the frozen deterministic and timing policy.
    #[must_use]
    pub const fn policy(&self) -> SchedulerPolicy {
        self.policy
    }

    /// Returns the logical observation time shared by every candidate evaluation.
    #[must_use]
    pub const fn observed_at(&self) -> ObservedAtUnixMillis {
        self.observed_at
    }
}

/// Durable capacity reservation frozen by a selected placement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReservationBinding {
    reservation_id: ReservationId,
    assignment_id: AssignmentId,
    lease_id: LeaseId,
    worker_id: WorkerId,
    worker_incarnation_id: WorkerIncarnationId,
    credential_id: CredentialId,
    worker_profile_id: ContentId<WorkerProfileArtifact>,
    worker_resource_observation_id: ContentId<WorkerResourceObservationArtifact>,
    worker_resource_observation_revision: EventId,
    worker_resource_admission_revision: Option<EventId>,
    worker_availability_id: ContentId<WorkerAvailabilityArtifact>,
    worker_last_seen_at: ObservedAtUnixMillis,
    claim_deadline: ObservedAtUnixMillis,
    resources: ReservedWorkerResources,
}

/// Auditable result of one immutable placement attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementRecord {
    placement_id: PlacementId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    snapshot_id: ContentId<PlacementSnapshotArtifact>,
    reservation: Option<ReservationBinding>,
}

impl PlacementRecord {
    /// Returns the immutable placement-attempt identity.
    #[must_use]
    pub const fn placement_id(&self) -> PlacementId {
        self.placement_id
    }

    /// Returns the execution attempt this decision serves.
    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    /// Returns the exact frozen candidate snapshot.
    #[must_use]
    pub const fn snapshot_id(&self) -> ContentId<PlacementSnapshotArtifact> {
        self.snapshot_id
    }

    /// Returns the immutable job contract evaluated by this placement.
    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Returns the selected worker, if capacity was reserved.
    #[must_use]
    pub const fn selected_worker_id(&self) -> Option<WorkerId> {
        match &self.reservation {
            Some(reservation) => Some(reservation.worker_id),
            None => None,
        }
    }

    /// Returns the durable reservation identity, if a worker was selected.
    #[must_use]
    pub const fn reservation_id(&self) -> Option<ReservationId> {
        match &self.reservation {
            Some(reservation) => Some(reservation.reservation_id),
            None => None,
        }
    }
}

/// Result of recording one scheduler evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementOutcome {
    /// One worker was selected and one capacity slot is durably reserved.
    Selected(PlacementRecord),
    /// Every candidate was rejected; the complete explanation remains archived.
    NoCandidate(PlacementRecord),
}

/// Durable proof that a reservation may stop consuming scheduler capacity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReservationReleaseReason {
    /// No assignment claimed the reservation before its explicit deadline.
    Unclaimed,
    /// The assignment lease expired before authoritative execution start.
    ExpiredBeforeStart,
    /// The corresponding execution attempt reached a durable terminal state.
    ExecutionTerminal,
}

/// Scheduler persistence, authority, recovery, or invariant failure.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Worker state recovery or matching failed unexpectedly.
    #[error(transparent)]
    Worker(#[from] WorkerControlError),
    /// Assignment lease creation/recovery failed.
    #[error(transparent)]
    Assignment(#[from] AssignmentControlError),
    /// Event persistence failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Snapshot storage or verification failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Current application credential authority could not be established.
    #[error(transparent)]
    Authority(#[from] PlacementAuthorityError),
    /// Candidate identities must form a canonical set.
    #[error("scheduler candidate worker identities contain a duplicate")]
    DuplicateCandidate,
    /// Historical scheduler algorithms are never silently reinterpreted.
    #[error("scheduler policy version is unsupported by the quantitative ledger")]
    UnsupportedPolicy,
    /// One execution attempt cannot consume capacity through parallel placement decisions.
    #[error("execution attempt already has an active scheduler reservation")]
    AttemptAlreadyReserved,
    /// Time arithmetic or persisted scheduler history is contradictory.
    #[error("invalid scheduler history: {0}")]
    InvalidHistory(String),
    /// A selected worker changed after the frozen snapshot.
    #[error("selected worker evidence is stale")]
    StaleCandidate,
    /// The selected worker lost controller authority before assignment grant.
    #[error("selected worker credential is no longer authorized")]
    AuthorityChanged,
    /// An unclaimed reservation cannot create an assignment after its frozen deadline.
    #[error("scheduler reservation claim deadline has elapsed")]
    ReservationClaimExpired,
    /// A no-candidate placement cannot create an assignment.
    #[error("placement has no selected capacity reservation")]
    NoReservation,
    /// Reservation still protects a possibly active or in-doubt execution.
    #[error("scheduler reservation cannot be released safely")]
    UnsafeRelease,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlacementRecordedPayload {
    record: PlacementRecord,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReservationReleasedPayload {
    reservation_id: ReservationId,
    reason: ReservationReleaseReason,
}

struct LedgerProjection {
    placements: BTreeMap<PlacementId, PlacementRecord>,
    reservations: BTreeMap<ReservationId, PlacementRecord>,
    released: BTreeMap<ReservationId, ReservationReleaseReason>,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    last_observed_at: Option<ObservedAtUnixMillis>,
}

/// Freezes and records one deterministic scheduler decision.
///
/// A selected result reserves exactly one slot in the singleton V1 scheduler ledger before an
/// assignment lease can be granted. An immutable no-candidate result records the same complete
/// rejection trace without inventing capacity.
///
/// # Errors
///
/// Returns an error for duplicate candidates, unavailable authority, invalid worker history,
/// concurrent ledger revision, content persistence, or time overflow.
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the scheduler boundary keeps one linear audit path across identities, policy, authority, stores, and observation"
)]
pub fn reserve_worker_placement<E: EventStore, C: ContentStore, A: WorkerPlacementAuthority>(
    events: &mut E,
    content: &mut C,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
    contract: &JobContract,
    candidate_worker_ids: &[WorkerId],
    authority: &A,
    policy: SchedulerPolicy,
    placement_id: PlacementId,
    reservation_id: ReservationId,
    assignment_grant: AssignmentLeaseGrant,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<PlacementOutcome, SchedulerError> {
    if policy.version != SchedulerPolicyVersion::StableWorkerIdQuantitativeV2 {
        return Err(SchedulerError::UnsupportedPolicy);
    }
    let ledger = project_ledger(events)?;
    for existing in ledger.placements.values() {
        validate_record_snapshot(content, existing)?;
    }
    let mut candidates = candidate_worker_ids.to_vec();
    candidates.sort();
    if candidates.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SchedulerError::DuplicateCandidate);
    }
    verify_contract(content, contract_id, contract)?;
    if let Some(existing) = ledger.placements.get(&placement_id) {
        let snapshot = validate_record_snapshot(content, existing)?;
        if existing.attempt_id != attempt_id
            || existing.contract_id != contract_id
            || snapshot.policy != policy
            || snapshot
                .candidates
                .iter()
                .map(|entry| entry.worker_id)
                .ne(candidates.iter().copied())
            || existing.reservation.as_ref().is_some_and(|reservation| {
                reservation.reservation_id != reservation_id
                    || reservation.assignment_id != assignment_grant.assignment_id()
                    || reservation.lease_id != assignment_grant.lease_id()
            })
        {
            return invalid_history("placement identity was reused for different input");
        }
        return Ok(outcome(existing.clone()));
    }
    if ledger
        .last_observed_at
        .is_some_and(|previous| observed_at < previous)
    {
        return invalid_history("placement observation time regressed");
    }
    let active_by_worker = active_reservations_by_worker(&ledger)?;
    if active_by_worker
        .values()
        .flatten()
        .any(|reservation| reservation.attempt_id == attempt_id)
    {
        return Err(SchedulerError::AttemptAlreadyReserved);
    }
    let mut sessions = BTreeMap::new();
    let mut entries = Vec::with_capacity(candidates.len());
    for worker_id in candidates {
        let state = recover_worker_session(
            events,
            content,
            worker_id,
            policy.session_timeout,
            observed_at,
        )?;
        let entry = match state {
            WorkerSessionState::NotFound => {
                rejected(worker_id, PlacementCandidateRejection::NotFound)
            }
            WorkerSessionState::Disconnected { incarnation_id } => PlacementCandidateSnapshot {
                worker_id,
                incarnation_id: Some(incarnation_id),
                credential_id: None,
                profile_id: None,
                resource_observation_id: None,
                resource_observation_revision: None,
                resource_admission_revision: None,
                availability_id: None,
                last_seen_at: None,
                authority_revision: None,
                disposition: CandidateDisposition::Rejected {
                    reason: PlacementCandidateRejection::Disconnected,
                },
            },
            WorkerSessionState::Expired { incarnation_id, .. } => PlacementCandidateSnapshot {
                worker_id,
                incarnation_id: Some(incarnation_id),
                credential_id: None,
                profile_id: None,
                resource_observation_id: None,
                resource_observation_revision: None,
                resource_admission_revision: None,
                availability_id: None,
                last_seen_at: None,
                authority_revision: None,
                disposition: CandidateDisposition::Rejected {
                    reason: PlacementCandidateRejection::Expired,
                },
            },
            WorkerSessionState::Live(session) => evaluate_live_candidate(
                &session,
                contract,
                authority,
                observed_at,
                active_by_worker
                    .get(&worker_id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                &mut sessions,
            )?,
        };
        entries.push(entry);
    }
    let selected_worker_id = entries.iter().find_map(|entry| {
        matches!(entry.disposition, CandidateDisposition::Eligible { .. })
            .then_some(entry.worker_id)
    });
    let snapshot = PlacementSnapshot {
        schema_version: 2,
        placement_id,
        attempt_id,
        contract_id,
        observed_at,
        policy,
        candidates: entries,
        selected_worker_id,
    };
    validate_snapshot(&snapshot)?;
    let snapshot_id = write_snapshot(content, &snapshot)?;
    let reservation = selected_worker_id
        .map(|worker_id| {
            let session = sessions.get(&worker_id).ok_or_else(|| {
                SchedulerError::InvalidHistory("eligible candidate lost its exact session".into())
            })?;
            let selected = snapshot
                .candidates
                .iter()
                .find(|candidate| candidate.worker_id == worker_id)
                .ok_or_else(|| {
                    SchedulerError::InvalidHistory(
                        "eligible candidate is absent from its snapshot".into(),
                    )
                })?;
            let CandidateDisposition::Eligible {
                quantitative_reservation,
                ..
            } = &selected.disposition
            else {
                return Err(SchedulerError::InvalidHistory(
                    "selected candidate is not eligible".into(),
                ));
            };
            let availability_id = session.availability_id().ok_or_else(|| {
                SchedulerError::InvalidHistory(
                    "eligible worker has no availability identity".into(),
                )
            })?;
            let timeout = i64::try_from(policy.reservation_claim_timeout.get()).map_err(|_| {
                SchedulerError::InvalidHistory("reservation claim timeout exceeds i64".into())
            })?;
            let claim_deadline = ObservedAtUnixMillis::new(
                observed_at.get().checked_add(timeout).ok_or_else(|| {
                    SchedulerError::InvalidHistory("reservation claim deadline overflowed".into())
                })?,
            );
            Ok::<ReservationBinding, SchedulerError>(ReservationBinding {
                reservation_id,
                assignment_id: assignment_grant.assignment_id(),
                lease_id: assignment_grant.lease_id(),
                worker_id,
                worker_incarnation_id: session.incarnation_id(),
                credential_id: session.credential_id(),
                worker_profile_id: session.profile_id(),
                worker_resource_observation_id: session.resource_observation_id(),
                worker_resource_observation_revision: session.resource_observation_revision(),
                worker_resource_admission_revision: session.resource_admission_revision(),
                worker_availability_id: availability_id,
                worker_last_seen_at: session.last_seen_at(),
                claim_deadline,
                resources: quantitative_reservation.clone(),
            })
        })
        .transpose()?;
    if reservation.is_some() && ledger.reservations.contains_key(&reservation_id) {
        return invalid_history("reservation identity was reused");
    }
    let record = PlacementRecord {
        placement_id,
        attempt_id,
        contract_id,
        snapshot_id,
        reservation,
    };
    let event = fact(
        PLACEMENT_RECORDED,
        ledger.last_event_id,
        observed_at,
        &PlacementRecordedPayload {
            record: record.clone(),
        },
    )?;
    events.append(
        &scheduler_stream()?,
        ledger
            .revision
            .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
        command_id,
        &[event],
    )?;
    Ok(outcome(record))
}

/// Rechecks frozen evidence and grants the downstream assignment lease.
///
/// # Errors
///
/// Returns an error when the placement has no reservation, identities contradict the execution
/// authority, worker evidence changed, authority became inactive, or lease persistence fails.
#[expect(
    clippy::too_many_arguments,
    reason = "the cross-authority handoff keeps every durable identity and observation explicit"
)]
pub fn grant_reserved_assignment<E: EventStore, C: ContentStore, A: WorkerPlacementAuthority>(
    events: &mut E,
    content: &C,
    execution_authority: ExecutionAttemptAuthority,
    placement: &PlacementRecord,
    assignment_grant: AssignmentLeaseGrant,
    authority: &A,
    session_timeout: WorkerSessionTimeoutMillis,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<LeasedExecutionAssignment, SchedulerError> {
    let ledger = project_ledger(events)?;
    let durable = ledger
        .placements
        .get(&placement.placement_id)
        .filter(|durable| *durable == placement)
        .ok_or_else(|| SchedulerError::InvalidHistory("placement is not durable".into()))?;
    let snapshot = validate_record_snapshot(content, durable)?;
    let reservation = durable
        .reservation
        .as_ref()
        .ok_or(SchedulerError::NoReservation)?;
    if ledger.released.contains_key(&reservation.reservation_id) {
        return invalid_history("released reservation cannot grant an assignment");
    }
    if observed_at >= reservation.claim_deadline {
        return Err(SchedulerError::ReservationClaimExpired);
    }
    if durable.attempt_id != execution_authority.attempt_id()
        || durable.contract_id != execution_authority.contract_id()
        || reservation.assignment_id != assignment_grant.assignment_id()
        || reservation.lease_id != assignment_grant.lease_id()
        || snapshot.policy.session_timeout != session_timeout
        || assignment_grant.policy().session_timeout() != session_timeout
    {
        return invalid_history("placement, execution authority, or assignment grant differs");
    }
    let WorkerSessionState::Live(worker) = recover_worker_session(
        events,
        content,
        reservation.worker_id,
        session_timeout,
        observed_at,
    )?
    else {
        return Err(SchedulerError::StaleCandidate);
    };
    if worker.incarnation_id() != reservation.worker_incarnation_id
        || worker.credential_id() != reservation.credential_id
        || worker.profile_id() != reservation.worker_profile_id
        || worker.resource_observation_id() != reservation.worker_resource_observation_id
        || worker.resource_observation_revision()
            != reservation.worker_resource_observation_revision
        || worker.resource_admission_revision() != reservation.worker_resource_admission_revision
        || worker.availability_id() != Some(reservation.worker_availability_id)
        || worker.last_seen_at() != reservation.worker_last_seen_at
    {
        return Err(SchedulerError::StaleCandidate);
    }
    if !authority
        .observe_credential_authority(worker.worker_id(), worker.credential_id(), observed_at)?
        .active()
    {
        return Err(SchedulerError::AuthorityChanged);
    }
    Ok(grant_assignment_lease(
        events,
        content,
        execution_authority,
        &worker,
        assignment_grant,
        command_id,
        observed_at,
    )?)
}

/// Recovers and validates one immutable scheduler decision and its archived snapshot.
///
/// # Errors
///
/// Returns an error for missing, contradictory, corrupt, or causally invalid history.
pub fn recover_scheduler_placement<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    placement_id: PlacementId,
) -> Result<Option<PlacementRecord>, SchedulerError> {
    let ledger = project_ledger(events)?;
    let Some(record) = ledger.placements.get(&placement_id) else {
        return Ok(None);
    };
    validate_record_snapshot(content, record)?;
    Ok(Some(record.clone()))
}

/// Releases capacity only after durable assignment recovery proves that doing so is safe.
///
/// # Errors
///
/// Returns an error while an assignment is live/in-doubt or while an unclaimed reservation is
/// still inside its configurable claim deadline.
pub fn release_scheduler_reservation<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &C,
    reservation_id: ReservationId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ReservationReleaseReason, SchedulerError> {
    let ledger = project_ledger(events)?;
    if let Some(reason) = ledger.released.get(&reservation_id) {
        return Ok(*reason);
    }
    let placement = ledger
        .reservations
        .get(&reservation_id)
        .ok_or_else(|| SchedulerError::InvalidHistory("reservation does not exist".into()))?;
    let reservation = placement
        .reservation
        .as_ref()
        .ok_or_else(|| SchedulerError::InvalidHistory("reservation index is invalid".into()))?;
    let reason =
        match recover_execution_assignment(events, content, placement.attempt_id, observed_at)? {
            ExecutionAssignmentState::NotFound if observed_at >= reservation.claim_deadline => {
                ReservationReleaseReason::Unclaimed
            }
            state @ ExecutionAssignmentState::ExpiredBeforeStart { .. } => {
                ensure_assignment_matches(reservation, &state)?;
                ReservationReleaseReason::ExpiredBeforeStart
            }
            state @ ExecutionAssignmentState::ExecutionTerminal { .. } => {
                ensure_assignment_matches(reservation, &state)?;
                ReservationReleaseReason::ExecutionTerminal
            }
            ExecutionAssignmentState::NotFound
            | ExecutionAssignmentState::Leased(_)
            | ExecutionAssignmentState::Accepted(_)
            | ExecutionAssignmentState::Running { .. }
            | ExecutionAssignmentState::ReconciliationRequired { .. } => {
                return Err(SchedulerError::UnsafeRelease);
            }
        };
    if ledger
        .last_observed_at
        .is_some_and(|previous| observed_at < previous)
    {
        return invalid_history("reservation release observation time regressed");
    }
    let event = fact(
        RESERVATION_RELEASED,
        ledger.last_event_id,
        observed_at,
        &ReservationReleasedPayload {
            reservation_id,
            reason,
        },
    )?;
    events.append(
        &scheduler_stream()?,
        ExpectedRevision::Exact(
            ledger
                .revision
                .ok_or_else(|| SchedulerError::InvalidHistory("missing ledger revision".into()))?,
        ),
        command_id,
        &[event],
    )?;
    Ok(reason)
}

fn evaluate_live_candidate<A: WorkerPlacementAuthority>(
    session: &RegisteredWorkerSession,
    contract: &JobContract,
    authority: &A,
    observed_at: ObservedAtUnixMillis,
    active_reservations: &[ActiveWorkerReservation],
    sessions: &mut BTreeMap<WorkerId, RegisteredWorkerSession>,
) -> Result<PlacementCandidateSnapshot, SchedulerError> {
    let worker_id = session.worker_id();
    let authority_observation =
        authority.observe_credential_authority(worker_id, session.credential_id(), observed_at)?;
    let disposition = if !authority_observation.active() {
        CandidateDisposition::Rejected {
            reason: PlacementCandidateRejection::AuthorityInactive,
        }
    } else if let Err(reason) = match_worker_at(session, contract, observed_at) {
        CandidateDisposition::Rejected {
            reason: PlacementCandidateRejection::WorkerMismatch(reason),
        }
    } else {
        let registered_slots = session.profile().max_concurrency().get();
        let availability = session.availability().ok_or_else(|| {
            SchedulerError::InvalidHistory("matching worker lost its availability evidence".into())
        })?;
        let reported_available_slots = availability.available_slots();
        let active_reservation_count = u16::try_from(active_reservations.len()).map_err(|_| {
            SchedulerError::InvalidHistory("active reservation count exceeds u16".into())
        })?;
        let unreflected_reservations = u16::try_from(
            active_reservations
                .iter()
                .filter(|reservation| {
                    !availability
                        .active_attempts()
                        .contains(&reservation.attempt_id)
                })
                .count(),
        )
        .map_err(|_| {
            SchedulerError::InvalidHistory("unreflected reservation count exceeds u16".into())
        })?;
        if active_reservation_count >= registered_slots
            || reported_available_slots == 0
            || registered_slots.saturating_sub(active_reservation_count) == 0
            || reported_available_slots.saturating_sub(unreflected_reservations) == 0
        {
            CandidateDisposition::Rejected {
                reason: PlacementCandidateRejection::CapacityReserved,
            }
        } else if let Some(quantitative_reservation) = plan_quantitative_reservation(
            session.resource_observation(),
            contract.resources().quantitative(),
            active_reservations,
        )? {
            CandidateDisposition::Eligible {
                registered_slots,
                reported_available_slots,
                active_reservations: active_reservation_count,
                unreflected_reservations,
                quantitative_reservation,
            }
        } else {
            CandidateDisposition::Rejected {
                reason: PlacementCandidateRejection::QuantitativeCapacityReserved,
            }
        }
    };
    if matches!(disposition, CandidateDisposition::Eligible { .. }) {
        sessions.insert(worker_id, session.clone());
    }
    Ok(PlacementCandidateSnapshot {
        worker_id,
        incarnation_id: Some(session.incarnation_id()),
        credential_id: Some(session.credential_id()),
        profile_id: Some(session.profile_id()),
        resource_observation_id: Some(session.resource_observation_id()),
        resource_observation_revision: Some(session.resource_observation_revision()),
        resource_admission_revision: session.resource_admission_revision(),
        availability_id: session.availability_id(),
        last_seen_at: Some(session.last_seen_at()),
        authority_revision: authority_observation.evidence_revision(),
        disposition,
    })
}

fn rejected(
    worker_id: WorkerId,
    reason: PlacementCandidateRejection,
) -> PlacementCandidateSnapshot {
    PlacementCandidateSnapshot {
        worker_id,
        incarnation_id: None,
        credential_id: None,
        profile_id: None,
        resource_observation_id: None,
        resource_observation_revision: None,
        resource_admission_revision: None,
        availability_id: None,
        last_seen_at: None,
        authority_revision: None,
        disposition: CandidateDisposition::Rejected { reason },
    }
}

#[derive(Clone)]
struct ActiveWorkerReservation {
    attempt_id: AttemptId,
    resources: ReservedWorkerResources,
}

fn active_reservations_by_worker(
    ledger: &LedgerProjection,
) -> Result<BTreeMap<WorkerId, Vec<ActiveWorkerReservation>>, SchedulerError> {
    let mut attempts = BTreeMap::<WorkerId, Vec<ActiveWorkerReservation>>::new();
    for (reservation_id, placement) in &ledger.reservations {
        if ledger.released.contains_key(reservation_id) {
            continue;
        }
        let binding = placement
            .reservation
            .as_ref()
            .ok_or_else(|| SchedulerError::InvalidHistory("reservation index is invalid".into()))?;
        attempts
            .entry(binding.worker_id)
            .or_default()
            .push(ActiveWorkerReservation {
                attempt_id: placement.attempt_id,
                resources: binding.resources.clone(),
            });
    }
    for reserved_attempts in attempts.values_mut() {
        reserved_attempts.sort_by_key(|reservation| reservation.attempt_id);
        if reserved_attempts
            .windows(2)
            .any(|pair| pair[0].attempt_id == pair[1].attempt_id)
        {
            return invalid_history("attempt has parallel active reservations");
        }
    }
    Ok(attempts)
}

fn plan_quantitative_reservation(
    observed: &WorkerResourceObservation,
    requested: &QuantitativeResourceRequest,
    active: &[ActiveWorkerReservation],
) -> Result<Option<ReservedWorkerResources>, SchedulerError> {
    let consumed_logical = checked_consumed(active, |value| value.logical_cpus)?;
    let consumed_memory = checked_consumed(active, |value| value.memory_bytes)?;
    let consumed_scratch = checked_consumed(active, |value| value.scratch_bytes)?;
    if !fits(
        observed.logical_cpus().get(),
        consumed_logical,
        requested.minimum_logical_cpus().map(LogicalCpuCount::get),
    ) || !fits(
        observed.memory_bytes().get(),
        consumed_memory,
        requested.minimum_memory_bytes().map(MemoryByteCount::get),
    ) || !fits(
        observed.scratch_available_bytes().get(),
        consumed_scratch,
        requested.minimum_scratch_bytes().map(ScratchByteCount::get),
    ) {
        return Ok(None);
    }
    let mut used_devices = BTreeSet::new();
    for reservation in active {
        for device_id in &reservation.resources.accelerator_device_ids {
            if !used_devices.insert(device_id.clone()) {
                return invalid_history("accelerator device has parallel active reservations");
            }
        }
    }
    if used_devices.iter().any(|reserved_id| {
        !observed
            .accelerators()
            .iter()
            .any(|device| device.device_id() == reserved_id)
    }) {
        return Ok(None);
    }
    let mut accelerator_device_ids = Vec::new();
    if let Some(accelerator) = requested.accelerator() {
        for device in observed.accelerators() {
            if used_devices.contains(device.device_id())
                || !accelerator.capabilities().iter().all(|required| {
                    device.capabilities().iter().any(|available| {
                        available.name == required.name && available.value == required.value
                    })
                })
            {
                continue;
            }
            accelerator_device_ids.push(device.device_id().clone());
            if u64::try_from(accelerator_device_ids.len()).unwrap_or(u64::MAX)
                == accelerator.minimum_devices().get()
            {
                break;
            }
        }
        if u64::try_from(accelerator_device_ids.len()).unwrap_or(u64::MAX)
            < accelerator.minimum_devices().get()
        {
            return Ok(None);
        }
    }
    Ok(Some(ReservedWorkerResources {
        logical_cpus: requested.minimum_logical_cpus(),
        memory_bytes: requested.minimum_memory_bytes(),
        scratch_bytes: requested.minimum_scratch_bytes(),
        accelerator_device_ids,
    }))
}

fn checked_consumed<T>(
    active: &[ActiveWorkerReservation],
    select: impl Fn(&ReservedWorkerResources) -> Option<T>,
) -> Result<u64, SchedulerError>
where
    T: Into<u64>,
{
    active.iter().try_fold(0_u64, |total, reservation| {
        let quantity = select(&reservation.resources).map_or(0, Into::into);
        total
            .checked_add(quantity)
            .ok_or_else(|| SchedulerError::InvalidHistory("reserved quantity overflowed".into()))
    })
}

fn fits(total: u64, consumed: u64, requested: Option<u64>) -> bool {
    consumed <= total && requested.unwrap_or(0) <= total - consumed
}

fn outcome(record: PlacementRecord) -> PlacementOutcome {
    if record.reservation.is_some() {
        PlacementOutcome::Selected(record)
    } else {
        PlacementOutcome::NoCandidate(record)
    }
}

fn validate_record_snapshot<C: ContentStore>(
    content: &C,
    record: &PlacementRecord,
) -> Result<PlacementSnapshot, SchedulerError> {
    let snapshot = read_snapshot(content, record.snapshot_id)?;
    validate_snapshot(&snapshot)?;
    if snapshot.placement_id != record.placement_id
        || snapshot.attempt_id != record.attempt_id
        || snapshot.contract_id != record.contract_id
        || snapshot.selected_worker_id != record.selected_worker_id()
    {
        return invalid_history("placement record contradicts its snapshot");
    }
    if record.reservation.is_some() != snapshot.selected_worker_id.is_some() {
        return invalid_history("placement reservation differs from its selection");
    }
    if let Some(reservation) = &record.reservation {
        let selected = snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.worker_id == reservation.worker_id)
            .ok_or_else(|| {
                SchedulerError::InvalidHistory("selected worker is absent from snapshot".into())
            })?;
        let CandidateDisposition::Eligible {
            quantitative_reservation,
            ..
        } = &selected.disposition
        else {
            return invalid_history("selected candidate is not eligible");
        };
        if quantitative_reservation != &reservation.resources
            || selected.incarnation_id != Some(reservation.worker_incarnation_id)
            || selected.credential_id != Some(reservation.credential_id)
            || selected.profile_id != Some(reservation.worker_profile_id)
            || selected.resource_observation_id != Some(reservation.worker_resource_observation_id)
            || selected.resource_observation_revision
                != Some(reservation.worker_resource_observation_revision)
            || selected.resource_admission_revision
                != reservation.worker_resource_admission_revision
            || selected.availability_id != Some(reservation.worker_availability_id)
            || selected.last_seen_at != Some(reservation.worker_last_seen_at)
            || reservation.claim_deadline <= snapshot.observed_at
        {
            return invalid_history("reservation contradicts selected candidate evidence");
        }
        validate_reserved_resources(
            content,
            record.contract_id,
            reservation,
            snapshot.observed_at,
        )?;
    }
    Ok(snapshot)
}

fn validate_reserved_resources<C: ContentStore>(
    content: &C,
    contract_id: ContentId<JobContractArtifact>,
    reservation: &ReservationBinding,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), SchedulerError> {
    let mut contract_bytes = Vec::new();
    content.write_to(&contract_id, &mut contract_bytes)?;
    let contract: JobContract = cairn_codec::from_slice(&contract_bytes)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?;
    contract
        .validate()
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?;
    let requested = contract.resources().quantitative();
    if reservation.resources.logical_cpus != requested.minimum_logical_cpus()
        || reservation.resources.memory_bytes != requested.minimum_memory_bytes()
        || reservation.resources.scratch_bytes != requested.minimum_scratch_bytes()
    {
        return invalid_history("reserved quantities differ from the frozen contract");
    }
    let mut observation_bytes = Vec::new();
    content.write_to(
        &reservation.worker_resource_observation_id,
        &mut observation_bytes,
    )?;
    let observation: WorkerResourceObservation = cairn_codec::from_slice(&observation_bytes)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?;
    if observed_at < observation.observed_at()
        || observation
            .valid_until()
            .is_some_and(|valid_until| observed_at >= valid_until)
        || requested
            .minimum_logical_cpus()
            .is_some_and(|value| value > observation.logical_cpus())
        || requested
            .minimum_memory_bytes()
            .is_some_and(|value| value > observation.memory_bytes())
        || requested
            .minimum_scratch_bytes()
            .is_some_and(|value| value > observation.scratch_available_bytes())
    {
        return invalid_history("reservation cites insufficient or stale resource evidence");
    }
    match requested.accelerator() {
        None if reservation.resources.accelerator_device_ids.is_empty() => Ok(()),
        Some(accelerator)
            if u64::try_from(reservation.resources.accelerator_device_ids.len())
                .unwrap_or(u64::MAX)
                == accelerator.minimum_devices().get()
                && reservation
                    .resources
                    .accelerator_device_ids
                    .iter()
                    .all(|reserved_id| {
                        observation.accelerators().iter().any(|device| {
                            device.device_id() == reserved_id
                                && accelerator.capabilities().iter().all(|required| {
                                    device.capabilities().iter().any(|available| {
                                        available.name == required.name
                                            && available.value == required.value
                                    })
                                })
                        })
                    }) =>
        {
            Ok(())
        }
        None | Some(_) => {
            invalid_history("reserved accelerator devices differ from the contract or observation")
        }
    }
}

fn ensure_assignment_matches(
    reservation: &ReservationBinding,
    state: &ExecutionAssignmentState,
) -> Result<(), SchedulerError> {
    let lease = match state {
        ExecutionAssignmentState::Leased(value) => value.lease(),
        ExecutionAssignmentState::Accepted(value) => value.lease(),
        ExecutionAssignmentState::Running { lease }
        | ExecutionAssignmentState::ExpiredBeforeStart { lease }
        | ExecutionAssignmentState::ReconciliationRequired { lease }
        | ExecutionAssignmentState::ExecutionTerminal { lease, .. } => lease,
        ExecutionAssignmentState::NotFound => return invalid_history("assignment is absent"),
    };
    let binding = lease.binding();
    if binding.assignment_id() != reservation.assignment_id
        || binding.lease_id() != reservation.lease_id
        || binding.worker_id() != reservation.worker_id
        || binding.worker_incarnation_id() != reservation.worker_incarnation_id
        || binding.worker_profile_id() != reservation.worker_profile_id
    {
        return invalid_history("assignment does not claim the scheduler reservation");
    }
    Ok(())
}

fn verify_contract<C: ContentStore>(
    content: &C,
    contract_id: ContentId<JobContractArtifact>,
    expected: &JobContract,
) -> Result<(), SchedulerError> {
    let mut bytes = Vec::new();
    content.write_to(&contract_id, &mut bytes)?;
    let actual: JobContract = cairn_codec::from_slice(&bytes)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?;
    if &actual != expected {
        return invalid_history("contract identity does not contain the supplied contract");
    }
    Ok(())
}

fn validate_snapshot(snapshot: &PlacementSnapshot) -> Result<(), SchedulerError> {
    if snapshot.schema_version != 2
        || snapshot.policy.version != SchedulerPolicyVersion::StableWorkerIdQuantitativeV2
        || snapshot
            .candidates
            .windows(2)
            .any(|pair| pair[0].worker_id >= pair[1].worker_id)
    {
        return invalid_history("placement snapshot schema or candidate order is invalid");
    }
    for candidate in &snapshot.candidates {
        if let CandidateDisposition::Eligible {
            quantitative_reservation,
            ..
        } = &candidate.disposition
        {
            quantitative_reservation.validate()?;
        }
    }
    let expected = snapshot.candidates.iter().find_map(|entry| {
        matches!(entry.disposition, CandidateDisposition::Eligible { .. })
            .then_some(entry.worker_id)
    });
    if snapshot.selected_worker_id != expected {
        return invalid_history("placement selection does not follow its policy");
    }
    Ok(())
}

fn write_snapshot<C: ContentStore>(
    content: &mut C,
    snapshot: &PlacementSnapshot,
) -> Result<ContentId<PlacementSnapshotArtifact>, SchedulerError> {
    let bytes = cairn_codec::to_vec(snapshot)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?;
    Ok(content
        .put::<PlacementSnapshotArtifact>(&mut Cursor::new(bytes))?
        .content_id)
}

fn read_snapshot<C: ContentStore>(
    content: &C,
    snapshot_id: ContentId<PlacementSnapshotArtifact>,
) -> Result<PlacementSnapshot, SchedulerError> {
    let mut bytes = Vec::new();
    content.write_to(&snapshot_id, &mut bytes)?;
    cairn_codec::from_slice(&bytes)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))
}

fn project_ledger(events: &impl EventStore) -> Result<LedgerProjection, SchedulerError> {
    let history = events.read_stream(&scheduler_stream()?, None)?;
    let mut projection = LedgerProjection {
        placements: BTreeMap::new(),
        reservations: BTreeMap::new(),
        released: BTreeMap::new(),
        revision: None,
        last_event_id: None,
        last_observed_at: None,
    };
    for event in history {
        if event.schema_version.get() != 2 || event.parent_event_id != projection.last_event_id {
            return invalid_history("scheduler event schema or causal parent differs");
        }
        let observed_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
        if projection
            .last_observed_at
            .is_some_and(|previous| observed_at < previous)
        {
            return invalid_history("scheduler event time regressed");
        }
        match event.schema_name.as_str() {
            PLACEMENT_RECORDED => {
                let payload: PlacementRecordedPayload = decode(&event)?;
                if projection
                    .placements
                    .insert(payload.record.placement_id, payload.record.clone())
                    .is_some()
                {
                    return invalid_history("placement identity was recorded twice");
                }
                if let Some(reservation) = &payload.record.reservation {
                    reservation.resources.validate()?;
                    if reservation.claim_deadline <= observed_at
                        || projection
                            .reservations
                            .insert(reservation.reservation_id, payload.record)
                            .is_some()
                    {
                        return invalid_history("reservation identity or deadline is invalid");
                    }
                }
            }
            RESERVATION_RELEASED => {
                let payload: ReservationReleasedPayload = decode(&event)?;
                if !projection
                    .reservations
                    .contains_key(&payload.reservation_id)
                    || projection
                        .released
                        .insert(payload.reservation_id, payload.reason)
                        .is_some()
                {
                    return invalid_history("unknown or released reservation was released");
                }
            }
            _ => return invalid_history("unknown scheduler event schema"),
        }
        projection.revision = Some(revision(event.sequence)?);
        projection.last_event_id = Some(event.event_id);
        projection.last_observed_at = Some(observed_at);
    }
    Ok(projection)
}

fn scheduler_stream() -> Result<StreamId, SchedulerError> {
    Ok(StreamId {
        kind: AggregateKind::new("execution-scheduler")
            .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new("scheduler:global-v1")
            .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?,
    })
}

fn fact<T: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &T,
) -> Result<NewEvent, SchedulerError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(2)
            .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(event: &EventEnvelope) -> Result<T, SchedulerError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))
}

fn revision(sequence: cairn_protocol::EventSequence) -> Result<StreamRevision, SchedulerError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| SchedulerError::InvalidHistory(error.to_string()))
}

fn invalid_history<T>(message: &str) -> Result<T, SchedulerError> {
    Err(SchedulerError::InvalidHistory(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        io::Cursor,
        sync::{Arc, Barrier},
    };

    use cairn_protocol::{
        AssignmentId, ContentType, ControlMessageId, JobId, LeaseId, PlacementId, ReservationId,
    };
    use cairn_record::ContentStore;
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::*;
    use crate::{
        ArchitectureName, AssignmentControlMessageIds, AssignmentLeaseDurationMillis,
        AssignmentLeasePolicy, AuthenticatedWorkerIdentity, CapturePolicy, CommandContract,
        DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionEnvironmentArtifact,
        ExecutionPlatform, ExecutionPlatformRequirement, ExecutionTimeoutMillis, ExecutorError,
        InputBundleArtifact, NetworkPolicy, OperatingSystemName, OutputByteLimit, PlacementRequest,
        PreparedExecutionJob, RecordedWorkerAuthenticator, ResourceRequest, SandboxPath,
        ScriptedExecutor, TargetEnvironmentName, WorkerAuthenticationSubject, WorkerAvailability,
        WorkerBinaryIdentity, WorkerHealth, WorkerHello, WorkerPoolName, WorkerProfile,
        WorkerProtocolVersion, WorkerResourceClaim, WorkerResourceInventory, WorkerResourceSource,
        WorkerSlotCount, accept_assignment, authorize_execution_attempt, execute_execution_attempt,
        prepare_execution_job, record_worker_heartbeat, recover_execution_job, register_worker,
        start_accepted_assignment,
    };

    struct ToggleAuthority(Cell<bool>);

    impl ToggleAuthority {
        fn active() -> Self {
            Self(Cell::new(true))
        }

        fn set(&self, active: bool) {
            self.0.set(active);
        }
    }

    impl WorkerPlacementAuthority for ToggleAuthority {
        fn observe_credential_authority(
            &self,
            _worker_id: WorkerId,
            _credential_id: CredentialId,
            _observed_at: ObservedAtUnixMillis,
        ) -> Result<PlacementAuthorityObservation, PlacementAuthorityError> {
            Ok(PlacementAuthorityObservation::new(self.0.get(), None))
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        content_database: std::path::PathBuf,
        event_database: std::path::PathBuf,
        cas: std::path::PathBuf,
        content: SqliteContentStore,
        events: SqliteEventStore,
        contract: JobContract,
        prepared: PreparedExecutionJob,
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_quantitative(QuantitativeResourceRequest::default())
        }

        fn with_quantitative(quantitative: QuantitativeResourceRequest) -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let content_database = directory.path().join("content.db");
            let event_database = directory.path().join("events.db");
            let cas = directory.path().join("cas");
            let mut content =
                SqliteContentStore::open(&content_database, &cas).expect("content store");
            let events = SqliteEventStore::open(&event_database).expect("event store");
            let input = put::<InputBundleArtifact>(&mut content, b"input");
            let environment =
                put::<ExecutionEnvironmentArtifact>(&mut content, br#"{"image":"sha256:fixture"}"#);
            let contract = JobContract::new(
                JobId::new(),
                input,
                environment,
                ExecutionBackend::new("container").expect("backend"),
                CommandContract::new(
                    SandboxPath::new("bin/run").expect("program"),
                    Vec::new(),
                    SandboxPath::new("work").expect("working directory"),
                ),
                ResourceRequest::new_with_quantitative(
                    ExecutionTimeoutMillis::new(1_000).expect("timeout"),
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
                    quantitative,
                )
                .expect("resources"),
                NetworkPolicy::Disabled,
                CapturePolicy::new(
                    OutputByteLimit::new(1_024).expect("stdout"),
                    OutputByteLimit::new(1_024).expect("stderr"),
                    DiagnosticByteLimit::new(1_024).expect("diagnostic"),
                    EvidenceByteLimit::new(4_096).expect("evidence"),
                    Vec::new(),
                )
                .expect("capture"),
            );
            let prepared = prepare_execution_job(&mut content, &contract).expect("prepare");
            Self {
                _directory: directory,
                content_database,
                event_database,
                cas,
                content,
                events,
                contract,
                prepared,
            }
        }

        fn reopen(&mut self) {
            self.content = SqliteContentStore::open(&self.content_database, &self.cas)
                .expect("reopen content");
            self.events = SqliteEventStore::open(&self.event_database).expect("reopen event store");
        }

        fn register(&mut self, worker_id: WorkerId, observed_at: i64) -> RegisteredWorkerSession {
            self.register_with_slots(worker_id, observed_at, 1)
        }

        fn register_with_slots(
            &mut self,
            worker_id: WorkerId,
            observed_at: i64,
            slots: u16,
        ) -> RegisteredWorkerSession {
            let profile = WorkerProfile::new(
                WorkerProtocolVersion::new(1).expect("protocol"),
                WorkerBinaryIdentity::new(format!("sha256:{worker_id}")).expect("binary"),
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
                    crate::worker::test_resource_observation(observed_at),
                    WorkerSlotCount::new(slots).expect("slots"),
                )
                .expect("resources"),
            )
            .expect("profile");
            let hello = WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile);
            let mut authenticator = RecordedWorkerAuthenticator::new([(
                worker_id,
                AuthenticatedWorkerIdentity::new(
                    WorkerAuthenticationSubject::new(format!("worker-principal:{worker_id}"))
                        .expect("subject"),
                    CredentialId::new(),
                    WorkerPoolName::new("fixture").expect("pool"),
                ),
            )]);
            let registered = register_worker(
                &mut self.events,
                &mut self.content,
                &mut authenticator,
                &hello,
                session_timeout(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_at),
            )
            .expect("register");
            record_worker_heartbeat(
                &mut self.events,
                &mut self.content,
                &registered,
                &WorkerAvailability::new(WorkerHealth::Ready, false, slots, Vec::new())
                    .expect("availability"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_at + 1),
            )
            .expect("heartbeat")
        }

        fn authorize(
            &mut self,
            attempt_id: AttemptId,
            observed_at: i64,
        ) -> ExecutionAttemptAuthority {
            authorize_execution_attempt(
                &mut self.events,
                self.prepared.clone(),
                attempt_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(observed_at),
            )
            .expect("authorize")
        }
    }

    fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put")
            .content_id
    }

    fn session_timeout() -> WorkerSessionTimeoutMillis {
        WorkerSessionTimeoutMillis::new(100).expect("session timeout")
    }

    fn scheduler_policy() -> SchedulerPolicy {
        SchedulerPolicy::new(
            SchedulerPolicyVersion::StableWorkerIdQuantitativeV2,
            session_timeout(),
            ReservationClaimTimeoutMillis::new(10).expect("claim timeout"),
        )
    }

    fn assignment_grant() -> AssignmentLeaseGrant {
        AssignmentLeaseGrant::new(
            AssignmentId::new(),
            LeaseId::new(),
            AssignmentControlMessageIds::new(ControlMessageId::new(), ControlMessageId::new()),
            AssignmentLeasePolicy::new(
                session_timeout(),
                AssignmentLeaseDurationMillis::new(20).expect("lease duration"),
            ),
        )
    }

    fn selected(outcome: PlacementOutcome) -> PlacementRecord {
        let PlacementOutcome::Selected(record) = outcome else {
            panic!("selected placement");
        };
        record
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one restart control keeps deterministic selection, exact replay, identity reuse, recovery, and lease linkage together"
    )]
    fn deterministic_snapshot_survives_restart_and_grants_exact_assignment() {
        let mut fixture = Fixture::new();
        let first_id = WorkerId::new();
        let second_id = WorkerId::new();
        fixture.register(first_id, 0);
        fixture.register(second_id, 0);
        let (expected, other) = if first_id < second_id {
            (first_id, second_id)
        } else {
            (second_id, first_id)
        };
        let attempt_id = AttemptId::new();
        let grant = assignment_grant();
        let placement_id = PlacementId::new();
        let reservation_id = ReservationId::new();
        let placement_command = CommandId::new();
        let authority_check = ToggleAuthority::active();
        let record = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[other, expected],
                &authority_check,
                scheduler_policy(),
                placement_id,
                reservation_id,
                grant,
                &placement_command,
                ObservedAtUnixMillis::new(2),
            )
            .expect("reserve"),
        );
        assert_eq!(record.selected_worker_id(), Some(expected));
        let snapshot = read_snapshot(&fixture.content, record.snapshot_id()).expect("snapshot");
        assert_eq!(snapshot.selected_worker_id(), Some(expected));
        assert_eq!(snapshot.candidates()[0].worker_id(), expected);
        assert_eq!(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[other, expected],
                &authority_check,
                scheduler_policy(),
                placement_id,
                reservation_id,
                grant,
                &placement_command,
                ObservedAtUnixMillis::new(2),
            )
            .expect("exact placement replay"),
            PlacementOutcome::Selected(record.clone())
        );
        assert!(matches!(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[expected],
                &authority_check,
                scheduler_policy(),
                placement_id,
                reservation_id,
                grant,
                &placement_command,
                ObservedAtUnixMillis::new(2),
            ),
            Err(SchedulerError::InvalidHistory(_))
        ));
        assert!(matches!(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[other, expected],
                &authority_check,
                scheduler_policy(),
                PlacementId::new(),
                ReservationId::new(),
                assignment_grant(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            ),
            Err(SchedulerError::AttemptAlreadyReserved)
        ));
        fixture.reopen();
        assert_eq!(
            recover_scheduler_placement(&fixture.events, &fixture.content, placement_id)
                .expect("recover"),
            Some(record.clone())
        );
        let execution_authority = fixture.authorize(attempt_id, 3);
        let leased = grant_reserved_assignment(
            &mut fixture.events,
            &fixture.content,
            execution_authority,
            &record,
            grant,
            &authority_check,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("grant assignment");
        assert_eq!(leased.lease().binding().worker_id(), expected);
        assert_eq!(leased.lease().binding().attempt_id(), attempt_id);
    }

    #[test]
    fn reservations_prevent_overcommit_and_unclaimed_release_restores_capacity() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        fixture.register(worker_id, 0);
        let authority = ToggleAuthority::active();
        let first_reservation = ReservationId::new();
        let first = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                AttemptId::new(),
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                first_reservation,
                assignment_grant(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("first reserve"),
        );
        assert_eq!(first.selected_worker_id(), Some(worker_id));
        let no_capacity = reserve_worker_placement(
            &mut fixture.events,
            &mut fixture.content,
            AttemptId::new(),
            fixture.prepared.contract_id(),
            &fixture.contract,
            &[worker_id],
            &authority,
            scheduler_policy(),
            PlacementId::new(),
            ReservationId::new(),
            assignment_grant(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("capacity decision");
        let PlacementOutcome::NoCandidate(rejected) = no_capacity else {
            panic!("second placement must retain a no-capacity trace");
        };
        let rejected_snapshot =
            read_snapshot(&fixture.content, rejected.snapshot_id()).expect("rejection snapshot");
        assert!(matches!(
            rejected_snapshot.candidates()[0].disposition(),
            CandidateDisposition::Rejected {
                reason: PlacementCandidateRejection::CapacityReserved
            }
        ));
        assert_eq!(
            release_scheduler_reservation(
                &mut fixture.events,
                &fixture.content,
                first_reservation,
                &CommandId::new(),
                ObservedAtUnixMillis::new(12),
            )
            .expect("release orphan"),
            ReservationReleaseReason::Unclaimed
        );
        assert!(matches!(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                AttemptId::new(),
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                ReservationId::new(),
                assignment_grant(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(13),
            )
            .expect("capacity restored"),
            PlacementOutcome::Selected(_)
        ));
    }

    #[test]
    fn quantitative_reservations_are_additive_and_devices_are_exclusive() {
        let device = |id: &str| {
            crate::AcceleratorDevice::new(
                AcceleratorDeviceId::new(id).expect("device ID"),
                Vec::new(),
            )
            .expect("device")
        };
        let observed = WorkerResourceObservation::new(
            WorkerResourceSource::BuiltinProbe,
            crate::ResourceProbeVersion::new("fixture-probe-v1").expect("probe version"),
            ObservedAtUnixMillis::new(0),
            None,
            LogicalCpuCount::new(8).expect("CPUs"),
            MemoryByteCount::new(16_000).expect("memory"),
            ScratchByteCount::new(64_000).expect("scratch"),
            crate::AcceleratorDiscoveryCompleteness::Complete,
            vec![device("accel0"), device("accel1")],
        )
        .expect("observation");
        let request = QuantitativeResourceRequest::new(
            Some(LogicalCpuCount::new(5).expect("CPUs")),
            Some(MemoryByteCount::new(10_000).expect("memory")),
            Some(ScratchByteCount::new(40_000).expect("scratch")),
            Some(
                crate::AcceleratorResourceRequest::new(
                    crate::AcceleratorDeviceCount::new(1).expect("device count"),
                    Vec::new(),
                )
                .expect("accelerator request"),
            ),
            true,
        );
        let first = plan_quantitative_reservation(&observed, &request, &[])
            .expect("plan")
            .expect("capacity");
        assert_eq!(first.accelerator_device_ids()[0].as_str(), "accel0");
        let active = [ActiveWorkerReservation {
            attempt_id: AttemptId::new(),
            resources: first,
        }];
        assert_eq!(
            plan_quantitative_reservation(&observed, &request, &active).expect("plan"),
            None
        );

        let devices = QuantitativeResourceRequest::new(
            None,
            None,
            None,
            Some(
                crate::AcceleratorResourceRequest::new(
                    crate::AcceleratorDeviceCount::new(2).expect("device count"),
                    Vec::new(),
                )
                .expect("accelerator request"),
            ),
            true,
        );
        assert_eq!(
            plan_quantitative_reservation(&observed, &devices, &active).expect("plan"),
            None
        );
    }

    #[test]
    fn durable_quantitative_reservation_blocks_then_release_restores_capacity() {
        let quantitative = QuantitativeResourceRequest::new(
            Some(LogicalCpuCount::new(5).expect("CPUs")),
            None,
            None,
            None,
            false,
        );
        let mut fixture = Fixture::with_quantitative(quantitative);
        let worker_id = WorkerId::new();
        fixture.register_with_slots(worker_id, 0, 4);
        let authority = ToggleAuthority::active();
        let reservation_id = ReservationId::new();
        let first = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                AttemptId::new(),
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                reservation_id,
                assignment_grant(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("first reservation"),
        );
        let snapshot = read_snapshot(&fixture.content, first.snapshot_id()).expect("snapshot");
        assert!(matches!(
            snapshot.candidates()[0].disposition(),
            CandidateDisposition::Eligible {
                quantitative_reservation,
                ..
            } if quantitative_reservation.logical_cpus().map(LogicalCpuCount::get) == Some(5)
        ));
        let second = reserve_worker_placement(
            &mut fixture.events,
            &mut fixture.content,
            AttemptId::new(),
            fixture.prepared.contract_id(),
            &fixture.contract,
            &[worker_id],
            &authority,
            scheduler_policy(),
            PlacementId::new(),
            ReservationId::new(),
            assignment_grant(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("second decision");
        let PlacementOutcome::NoCandidate(second) = second else {
            panic!("quantitative capacity must be reserved");
        };
        let snapshot = read_snapshot(&fixture.content, second.snapshot_id()).expect("snapshot");
        assert!(matches!(
            snapshot.candidates()[0].disposition(),
            CandidateDisposition::Rejected {
                reason: PlacementCandidateRejection::QuantitativeCapacityReserved
            }
        ));
        assert_eq!(
            release_scheduler_reservation(
                &mut fixture.events,
                &fixture.content,
                reservation_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(12),
            )
            .expect("release"),
            ReservationReleaseReason::Unclaimed
        );
        assert!(matches!(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                AttemptId::new(),
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                ReservationId::new(),
                assignment_grant(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(13),
            )
            .expect("capacity restored"),
            PlacementOutcome::Selected(_)
        ));
    }

    #[test]
    fn resource_refresh_between_reservation_and_grant_fails_closed() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let worker = fixture.register(worker_id, 0);
        let authority = ToggleAuthority::active();
        let attempt_id = AttemptId::new();
        let grant = assignment_grant();
        let record = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                ReservationId::new(),
                grant,
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("reserve"),
        );
        crate::record_worker_resource_observation(
            &mut fixture.events,
            &mut fixture.content,
            &worker,
            &crate::worker::test_resource_observation(3),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("resource refresh");
        let execution_authority = fixture.authorize(attempt_id, 4);
        assert!(matches!(
            grant_reserved_assignment(
                &mut fixture.events,
                &fixture.content,
                execution_authority,
                &record,
                grant,
                &authority,
                session_timeout(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(5),
            ),
            Err(SchedulerError::StaleCandidate)
        ));
    }

    #[test]
    fn concurrent_sqlite_placements_cannot_overcommit_quantitative_capacity() {
        let mut fixture = Fixture::with_quantitative(QuantitativeResourceRequest::new(
            Some(LogicalCpuCount::new(5).expect("CPUs")),
            None,
            None,
            None,
            false,
        ));
        let worker_id = WorkerId::new();
        fixture.register_with_slots(worker_id, 0, 2);
        let content_database = fixture.content_database.clone();
        let event_database = fixture.event_database.clone();
        let cas = fixture.cas.clone();
        let contract = fixture.contract.clone();
        let contract_id = fixture.prepared.contract_id();
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let content_database = content_database.clone();
            let event_database = event_database.clone();
            let cas = cas.clone();
            let contract = contract.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let mut content =
                    SqliteContentStore::open(content_database, cas).expect("thread content");
                let mut events = SqliteEventStore::open(event_database).expect("thread events");
                barrier.wait();
                match reserve_worker_placement(
                    &mut events,
                    &mut content,
                    AttemptId::new(),
                    contract_id,
                    &contract,
                    &[worker_id],
                    &ToggleAuthority::active(),
                    scheduler_policy(),
                    PlacementId::new(),
                    ReservationId::new(),
                    assignment_grant(),
                    &CommandId::new(),
                    ObservedAtUnixMillis::new(2),
                ) {
                    Ok(PlacementOutcome::Selected(_)) => true,
                    Ok(PlacementOutcome::NoCandidate(_))
                    | Err(SchedulerError::Event(EventStoreError::RevisionConflict { .. })) => false,
                    Err(error) => panic!("unexpected concurrent scheduler result: {error}"),
                }
            }));
        }
        let selected = handles
            .into_iter()
            .map(|handle| handle.join().expect("scheduler thread"))
            .filter(|selected| *selected)
            .count();
        assert_eq!(
            selected, 1,
            "eight observed CPUs cannot admit two five-CPU reservations"
        );
    }

    #[test]
    fn authority_and_worker_evidence_are_rechecked_before_assignment() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let worker = fixture.register(worker_id, 0);
        let authority = ToggleAuthority::active();
        let attempt_id = AttemptId::new();
        let grant = assignment_grant();
        let record = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority,
                scheduler_policy(),
                PlacementId::new(),
                ReservationId::new(),
                grant,
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("reserve"),
        );
        authority.set(false);
        let execution_authority = fixture.authorize(attempt_id, 3);
        assert!(matches!(
            grant_reserved_assignment(
                &mut fixture.events,
                &fixture.content,
                execution_authority,
                &record,
                grant,
                &authority,
                session_timeout(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(4),
            ),
            Err(SchedulerError::AuthorityChanged)
        ));

        authority.set(true);
        record_worker_heartbeat(
            &mut fixture.events,
            &mut fixture.content,
            &worker,
            &WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())
                .expect("availability"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("new heartbeat");
        let job = crate::ExecutionJob::new(fixture.contract.job_id()).expect("job");
        let crate::ExecutionJobState::ReadyToStart(recovered_authority) =
            recover_execution_job(&fixture.events, &fixture.content, &job).expect("recover job")
        else {
            panic!("attempt authority must remain recoverable");
        };
        assert!(matches!(
            grant_reserved_assignment(
                &mut fixture.events,
                &fixture.content,
                recovered_authority,
                &record,
                grant,
                &authority,
                session_timeout(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(6),
            ),
            Err(SchedulerError::StaleCandidate)
        ));
    }

    #[test]
    fn started_in_doubt_assignment_keeps_capacity_until_terminal() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let worker = fixture.register(worker_id, 0);
        let authority_check = ToggleAuthority::active();
        let attempt_id = AttemptId::new();
        let grant = assignment_grant();
        let reservation_id = ReservationId::new();
        let record = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority_check,
                scheduler_policy(),
                PlacementId::new(),
                reservation_id,
                grant,
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("reserve"),
        );
        let execution_authority = fixture.authorize(attempt_id, 3);
        let leased = grant_reserved_assignment(
            &mut fixture.events,
            &fixture.content,
            execution_authority,
            &record,
            grant,
            &authority_check,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("grant");
        let accepted = accept_assignment(
            &mut fixture.events,
            &fixture.content,
            leased,
            &worker,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("accept");
        let started = start_accepted_assignment(
            &mut fixture.events,
            &fixture.content,
            accepted,
            &worker,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("start");
        assert!(matches!(
            release_scheduler_reservation(
                &mut fixture.events,
                &fixture.content,
                reservation_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(25),
            ),
            Err(SchedulerError::UnsafeRelease)
        ));
        let mut executor = ScriptedExecutor::new(|_: &crate::ExecutionInput<'_>| {
            Err(ExecutorError::NotStarted(
                "fixture proved the workload did not begin".into(),
            ))
        });
        execute_execution_attempt(
            &mut fixture.events,
            &mut fixture.content,
            &mut executor,
            started,
            &CommandId::new(),
            ObservedAtUnixMillis::new(26),
        )
        .expect("record terminal not-started result");
        assert_eq!(
            release_scheduler_reservation(
                &mut fixture.events,
                &fixture.content,
                reservation_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(27),
            )
            .expect("release terminal reservation"),
            ReservationReleaseReason::ExecutionTerminal
        );
    }

    #[test]
    fn prestart_lease_expiry_releases_the_matching_reservation() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        fixture.register(worker_id, 0);
        let authority_check = ToggleAuthority::active();
        let attempt_id = AttemptId::new();
        let grant = assignment_grant();
        let reservation_id = ReservationId::new();
        let record = selected(
            reserve_worker_placement(
                &mut fixture.events,
                &mut fixture.content,
                attempt_id,
                fixture.prepared.contract_id(),
                &fixture.contract,
                &[worker_id],
                &authority_check,
                scheduler_policy(),
                PlacementId::new(),
                reservation_id,
                grant,
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            )
            .expect("reserve"),
        );
        let execution_authority = fixture.authorize(attempt_id, 3);
        grant_reserved_assignment(
            &mut fixture.events,
            &fixture.content,
            execution_authority,
            &record,
            grant,
            &authority_check,
            session_timeout(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("grant");
        assert_eq!(
            release_scheduler_reservation(
                &mut fixture.events,
                &fixture.content,
                reservation_id,
                &CommandId::new(),
                ObservedAtUnixMillis::new(24),
            )
            .expect("release pre-start expiry"),
            ReservationReleaseReason::ExpiredBeforeStart
        );
    }
}
