use std::path::PathBuf;

use cairn_execution::{
    AssignmentBinding, AssignmentControlMessageIds, AssignmentLeaseDurationMillis,
    AssignmentLeaseGrant, AssignmentLeasePolicy, AssignmentMaterialByteLimit,
    ControlEnqueueOutcome, ExecutionAssignmentState, JobContract, PlacementAuthorityError,
    PlacementAuthorityObservation, PlacementOutcome, PlacementRecord,
    ReservationClaimTimeoutMillis, ReservationReleaseReason, SchedulerPolicy,
    SchedulerPolicyVersion, WorkerPlacementAuthority, assignment_offer_message,
    authorize_execution_attempt, enqueue_controller_message, grant_reserved_assignment,
    load_assignment_materials, prepare_execution_job, recover_execution_assignment,
    release_scheduler_reservation, reserve_worker_placement,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ControlMessageId, CredentialId, LeaseId,
    ObservedAtUnixMillis, PlacementId, ReservationId, WorkerId,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Serialize};

use crate::{ServerConfig, ServerError, enrollment::EnrollmentRegistry, observed_now};

/// User-selectable scheduler behavior. Positive duration types reject zero during decoding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerServiceConfig {
    pub policy_version: SchedulerPolicyVersion,
    pub reservation_claim_timeout_ms: ReservationClaimTimeoutMillis,
    pub assignment_lease_duration_ms: AssignmentLeaseDurationMillis,
    /// Aggregate input-bundle plus environment bytes copied into one offer; `null` disables it.
    pub assignment_material_byte_limit: Option<AssignmentMaterialByteLimit>,
}

/// Stable command identities for each independently committed scheduling boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerScheduleCommandIds {
    pub authorize_attempt: CommandId,
    pub reserve_placement: CommandId,
    pub grant_assignment: CommandId,
    pub enqueue_offer: CommandId,
}

/// Strong identities allocated by product orchestration before scheduling starts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerScheduleIds {
    pub attempt_id: AttemptId,
    pub placement_id: PlacementId,
    pub reservation_id: ReservationId,
    pub assignment_id: AssignmentId,
    pub lease_id: LeaseId,
    pub offer_message_id: ControlMessageId,
    pub start_message_id: ControlMessageId,
    pub commands: ControllerScheduleCommandIds,
}

/// Durable assignment phase recovered when a scheduling request is retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledAssignmentPhase {
    /// The lease and offer are durable and await worker admission.
    OfferPending,
    /// The worker durably accepted the offer.
    Accepted,
    /// Authoritative execution start is durable.
    Running,
    /// The lease elapsed before execution started and its reservation may be released.
    ExpiredBeforeStart,
    /// Execution might have started and therefore requires reconciliation, not replacement.
    ReconciliationRequired,
    /// The execution attempt reached a durable terminal state.
    Terminal,
}

/// Result of composing preparation, placement, authorization, lease, and durable delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerSchedulingOutcome {
    /// One worker was assigned, or the exact prior assignment was safely recovered.
    Scheduled {
        placement: PlacementRecord,
        binding: AssignmentBinding,
        phase: ScheduledAssignmentPhase,
    },
    /// The frozen placement explains why no enrolled worker was eligible.
    NoCandidate { placement: PlacementRecord },
}

struct ControllerPlacementAuthority {
    event_database: PathBuf,
}

impl WorkerPlacementAuthority for ControllerPlacementAuthority {
    fn observe_credential_authority(
        &self,
        worker_id: WorkerId,
        credential_id: CredentialId,
        observed_at: ObservedAtUnixMillis,
    ) -> Result<PlacementAuthorityObservation, PlacementAuthorityError> {
        let events = SqliteEventStore::open(&self.event_database)
            .map_err(|error| PlacementAuthorityError::new(error.to_string()))?;
        let registry = EnrollmentRegistry::load(&events, observed_at)
            .map_err(|error| PlacementAuthorityError::new(error.to_string()))?;
        Ok(PlacementAuthorityObservation::new(
            registry.credential_is_authorized(credential_id, worker_id),
            registry.last_event_id(),
        ))
    }
}

/// Schedules one immutable execution contract using the controller wall clock.
///
/// Exact retry requires the caller to retain and reuse every identity in `ids`.
///
/// # Errors
///
/// Returns an error for disabled scheduling, invalid authority/history, storage failure, or an
/// identity reused for different input.
pub fn schedule_execution_contract(
    config: &ServerConfig,
    contract: &JobContract,
    ids: ControllerScheduleIds,
) -> Result<ControllerSchedulingOutcome, ServerError> {
    schedule_execution_contract_at(config, contract, ids, observed_now()?)
}

/// Deterministic-time form of [`schedule_execution_contract`] for orchestration and replay.
///
/// # Errors
///
/// Has the same failure modes as [`schedule_execution_contract`].
#[expect(
    clippy::too_many_lines,
    reason = "the composition keeps recovery decisions linear across five durable authorities"
)]
pub fn schedule_execution_contract_at(
    config: &ServerConfig,
    contract: &JobContract,
    ids: ControllerScheduleIds,
    observed_at: ObservedAtUnixMillis,
) -> Result<ControllerSchedulingOutcome, ServerError> {
    config.validate_schema()?;
    let scheduler = config.scheduler.ok_or_else(|| {
        ServerError::Configuration("scheduler is disabled in controller configuration".into())
    })?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let mut content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let registry = EnrollmentRegistry::load(&events, observed_at)
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let candidate_worker_ids: Vec<_> = registry.worker_ids().into_iter().collect();
    let authority = ControllerPlacementAuthority {
        event_database: config.storage.event_database.clone(),
    };

    let prepared = prepare_execution_job(&mut content, contract)
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let contract_id = prepared.contract_id();
    let assignment_grant = AssignmentLeaseGrant::new(
        ids.assignment_id,
        ids.lease_id,
        AssignmentControlMessageIds::new(ids.offer_message_id, ids.start_message_id),
        AssignmentLeasePolicy::new(
            config.session_timeout_ms,
            scheduler.assignment_lease_duration_ms,
        ),
    );
    let placement = reserve_worker_placement(
        &mut events,
        &mut content,
        ids.attempt_id,
        contract_id,
        contract,
        &candidate_worker_ids,
        &authority,
        SchedulerPolicy::new(
            scheduler.policy_version,
            config.session_timeout_ms,
            scheduler.reservation_claim_timeout_ms,
        ),
        ids.placement_id,
        ids.reservation_id,
        assignment_grant,
        &ids.commands.reserve_placement,
        observed_at,
    )
    .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let placement = match placement {
        PlacementOutcome::Selected(placement) => placement,
        PlacementOutcome::NoCandidate(placement) => {
            return Ok(ControllerSchedulingOutcome::NoCandidate { placement });
        }
    };

    let assignment = recover_execution_assignment(&events, &content, ids.attempt_id, observed_at)
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let (binding, phase, offer) = match assignment {
        ExecutionAssignmentState::NotFound => {
            let execution_authority = authorize_execution_attempt(
                &mut events,
                prepared,
                ids.attempt_id,
                &ids.commands.authorize_attempt,
                observed_at,
            )
            .map_err(|error| ServerError::Scheduling(error.to_string()))?;
            let leased = grant_reserved_assignment(
                &mut events,
                &content,
                execution_authority,
                &placement,
                assignment_grant,
                &authority,
                config.session_timeout_ms,
                &ids.commands.grant_assignment,
                observed_at,
            )
            .map_err(|error| ServerError::Scheduling(error.to_string()))?;
            let materials = load_assignment_materials(
                &content,
                leased.contract(),
                scheduler.assignment_material_byte_limit,
            )
            .map_err(|error| ServerError::Scheduling(error.to_string()))?;
            let offer = assignment_offer_message(leased.lease(), leased.contract(), materials);
            (
                leased.lease().binding().clone(),
                ScheduledAssignmentPhase::OfferPending,
                Some(offer),
            )
        }
        ExecutionAssignmentState::Leased(leased) => {
            let materials = load_assignment_materials(
                &content,
                leased.contract(),
                scheduler.assignment_material_byte_limit,
            )
            .map_err(|error| ServerError::Scheduling(error.to_string()))?;
            let offer = assignment_offer_message(leased.lease(), leased.contract(), materials);
            (
                leased.lease().binding().clone(),
                ScheduledAssignmentPhase::OfferPending,
                Some(offer),
            )
        }
        ExecutionAssignmentState::Accepted(accepted) => (
            accepted.lease().binding().clone(),
            ScheduledAssignmentPhase::Accepted,
            None,
        ),
        ExecutionAssignmentState::Running { lease } => (
            lease.binding().clone(),
            ScheduledAssignmentPhase::Running,
            None,
        ),
        ExecutionAssignmentState::ExpiredBeforeStart { lease } => (
            lease.binding().clone(),
            ScheduledAssignmentPhase::ExpiredBeforeStart,
            None,
        ),
        ExecutionAssignmentState::ReconciliationRequired { lease } => (
            lease.binding().clone(),
            ScheduledAssignmentPhase::ReconciliationRequired,
            None,
        ),
        ExecutionAssignmentState::ExecutionTerminal { lease, .. } => (
            lease.binding().clone(),
            ScheduledAssignmentPhase::Terminal,
            None,
        ),
    };
    validate_recovered_binding(&binding, &placement, contract_id, ids)?;
    if let Some(offer) = offer {
        let _: ControlEnqueueOutcome = enqueue_controller_message(
            &mut events,
            binding.worker_id(),
            &offer,
            &ids.commands.enqueue_offer,
            observed_at,
        )
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    }
    Ok(ControllerSchedulingOutcome::Scheduled {
        placement,
        binding,
        phase,
    })
}

/// Releases one scheduler reservation when durable assignment recovery proves it safe.
///
/// # Errors
///
/// Fails closed for live, accepted, running, or execution-in-doubt assignments.
pub fn release_execution_reservation(
    config: &ServerConfig,
    reservation_id: ReservationId,
    command_id: &CommandId,
) -> Result<ReservationReleaseReason, ServerError> {
    release_execution_reservation_at(config, reservation_id, command_id, observed_now()?)
}

/// Deterministic-time form of [`release_execution_reservation`].
///
/// # Errors
///
/// Has the same failure modes as [`release_execution_reservation`].
pub fn release_execution_reservation_at(
    config: &ServerConfig,
    reservation_id: ReservationId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<ReservationReleaseReason, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    let content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .map_err(|error| ServerError::Scheduling(error.to_string()))?;
    release_scheduler_reservation(
        &mut events,
        &content,
        reservation_id,
        command_id,
        observed_at,
    )
    .map_err(|error| ServerError::Scheduling(error.to_string()))
}

fn validate_recovered_binding(
    binding: &AssignmentBinding,
    placement: &PlacementRecord,
    contract_id: cairn_protocol::ContentId<cairn_execution::JobContractArtifact>,
    ids: ControllerScheduleIds,
) -> Result<(), ServerError> {
    if binding.attempt_id() == ids.attempt_id
        && binding.contract_id() == contract_id
        && binding.assignment_id() == ids.assignment_id
        && binding.lease_id() == ids.lease_id
        && binding.offer_message_id() == ids.offer_message_id
        && binding.start_message_id() == ids.start_message_id
        && placement.selected_worker_id() == Some(binding.worker_id())
    {
        Ok(())
    } else {
        Err(ServerError::Scheduling(
            "recovered assignment differs from the scheduling identities".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor, num::NonZeroU64};

    use cairn_control_transport::{EnrollmentRequest, EnrollmentSecret, TransportPolicy};
    use cairn_execution::{
        AcceleratorDiscoveryCompleteness, ArchitectureName, CapturePolicy, CommandContract,
        DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionEnvironmentArtifact,
        ExecutionPlatform, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
        InputBundleArtifact, LogicalCpuCount, MemoryByteCount, NetworkPolicy, OperatingSystemName,
        OutputByteLimit, PlacementRequest, PlacementSnapshot, RecordedWorkerAuthenticator,
        ResourceProbeVersion, ResourceRequest, SandboxPath, ScratchByteCount,
        TargetEnvironmentName, WorkerAuthenticationSubject, WorkerAvailability,
        WorkerBinaryIdentity, WorkerHealth, WorkerHello, WorkerPoolName, WorkerProfile,
        WorkerProtocolVersion, WorkerResourceClaim, WorkerResourceInventory,
        WorkerResourceObservation, WorkerResourceSource, WorkerSessionTimeoutMillis,
        WorkerSlotCount, authorize_execution_attempt, prepare_execution_job,
        record_worker_heartbeat, register_worker,
    };
    use cairn_protocol::{ContentType, JobId, WorkerIncarnationId};
    use cairn_record::ContentStore;
    use rcgen::{CertificateParams, KeyPair};

    use super::*;
    use crate::{
        EnrollmentServiceConfig,
        enrollment::{
            EnrollmentError, WorkerCredentialIssuer, create_offer, redeem, revoke_credential,
        },
    };

    struct FixtureIssuer(String);

    impl WorkerCredentialIssuer for FixtureIssuer {
        fn issue(
            &self,
            _csr_pem: &str,
            _worker_id: WorkerId,
            _credential_id: CredentialId,
            _now: ObservedAtUnixMillis,
        ) -> Result<String, EnrollmentError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the authority revision and snapshot-to-grant revocation race form one control"
    )]
    fn managed_authority_revision_is_cited_and_late_revocation_fails_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let event_database = directory.path().join("events.sqlite3");
        let content_database = directory.path().join("content.sqlite3");
        let content_directory = directory.path().join("content");
        let certificate = fixture_certificate();
        let ca = directory.path().join("ca.pem");
        fs::write(&ca, &certificate).expect("write fixture CA");
        let service = EnrollmentServiceConfig {
            listen: "127.0.0.1:7444".parse().expect("address"),
            public_tcp_address: "controller.test:7444".into(),
            websocket_uri: "wss://controller.test:7444/enrollment".into(),
            server_name: "controller.test".into(),
            server_ca: ca,
            server_tls: None,
            control_endpoint: None,
            issuer_certificate: directory.path().join("unused-ca.pem"),
            issuer_private_key: directory.path().join("unused-ca-key.pem"),
            credential_validity_ms: NonZeroU64::new(60_000).expect("validity"),
            rotation_overlap_ms: None,
            handshake_timeout_ms: None,
            diagnostic_byte_limit: None,
            transport: TransportPolicy::default(),
        };
        let mut events = SqliteEventStore::open(&event_database).expect("event store");
        let mut content =
            SqliteContentStore::open(&content_database, &content_directory).expect("content store");
        let bundle = create_offer(
            &mut events,
            &service,
            WorkerPoolName::new("managed-lab").expect("pool"),
            NonZeroU64::new(1_000).expect("TTL"),
            ObservedAtUnixMillis::new(1),
        )
        .expect("create offer");
        let credential = redeem(
            &mut events,
            &FixtureIssuer(certificate),
            &EnrollmentRequest {
                schema_version: 1,
                enrollment_id: bundle.enrollment_id,
                secret: EnrollmentSecret::from_bytes(*bundle.secret.expose()),
                csr_pem: "fixture-csr".into(),
            },
            ObservedAtUnixMillis::new(2),
        )
        .expect("redeem offer");
        let profile = WorkerProfile::new(
            WorkerProtocolVersion::new(1).expect("protocol"),
            WorkerBinaryIdentity::new("sha256:managed-authority-fixture").expect("binary"),
            WorkerResourceInventory::new(
                WorkerResourceClaim::new(
                    ExecutionPlatform::new(
                        ArchitectureName::new("x86_64").expect("architecture"),
                        OperatingSystemName::new("linux").expect("operating system"),
                        TargetEnvironmentName::new("gnu").expect("target environment"),
                    ),
                    WorkerResourceSource::BuiltinProbe,
                ),
                vec![WorkerResourceClaim::new(
                    ExecutionBackend::new("container").expect("backend"),
                    WorkerResourceSource::OperatorDeclared,
                )],
                Vec::new(),
                WorkerResourceObservation::new(
                    WorkerResourceSource::BuiltinProbe,
                    ResourceProbeVersion::new("fixture-probe-v1").expect("probe version"),
                    ObservedAtUnixMillis::new(2),
                    None,
                    LogicalCpuCount::new(8).expect("logical CPUs"),
                    MemoryByteCount::new(16 * 1024 * 1024 * 1024).expect("memory"),
                    ScratchByteCount::new(64 * 1024 * 1024 * 1024).expect("scratch"),
                    AcceleratorDiscoveryCompleteness::Complete,
                    Vec::new(),
                )
                .expect("resource observation"),
                WorkerSlotCount::new(1).expect("slots"),
            )
            .expect("resources"),
        )
        .expect("profile");
        let hello = WorkerHello::new(credential.worker_id, WorkerIncarnationId::new(), profile);
        let mut authenticator = RecordedWorkerAuthenticator::new([(
            credential.worker_id,
            cairn_execution::AuthenticatedWorkerIdentity::new(
                WorkerAuthenticationSubject::new("managed-fixture").expect("subject"),
                credential.credential_id,
                credential.pool.clone(),
            ),
        )]);
        let session_timeout = WorkerSessionTimeoutMillis::new(100).expect("session timeout");
        let registered = register_worker(
            &mut events,
            &mut content,
            &mut authenticator,
            &hello,
            session_timeout,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("register worker");
        record_worker_heartbeat(
            &mut events,
            &mut content,
            &registered,
            &WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())
                .expect("availability"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(4),
        )
        .expect("heartbeat");
        let input = put::<InputBundleArtifact>(&mut content, b"input");
        let environment = put::<ExecutionEnvironmentArtifact>(&mut content, b"environment");
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
            ResourceRequest::new(
                ExecutionTimeoutMillis::new(1_000).expect("timeout"),
                PlacementRequest::new(
                    ExecutionPlatformRequirement::default(),
                    vec![credential.pool.clone()],
                    Vec::new(),
                )
                .expect("placement"),
            )
            .expect("resource request"),
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
        let contract_id = prepared.contract_id();
        let attempt_id = AttemptId::new();
        let execution_authority = authorize_execution_attempt(
            &mut events,
            prepared,
            attempt_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(5),
        )
        .expect("authorize attempt");
        let authority = ControllerPlacementAuthority { event_database };
        let grant = AssignmentLeaseGrant::new(
            AssignmentId::new(),
            LeaseId::new(),
            AssignmentControlMessageIds::new(ControlMessageId::new(), ControlMessageId::new()),
            AssignmentLeasePolicy::new(
                session_timeout,
                AssignmentLeaseDurationMillis::new(50).expect("lease duration"),
            ),
        );
        let placement = reserve_worker_placement(
            &mut events,
            &mut content,
            attempt_id,
            contract_id,
            &contract,
            &[credential.worker_id],
            &authority,
            SchedulerPolicy::new(
                SchedulerPolicyVersion::StableWorkerIdQuantitativeV2,
                session_timeout,
                ReservationClaimTimeoutMillis::new(20).expect("claim timeout"),
            ),
            PlacementId::new(),
            ReservationId::new(),
            grant,
            &CommandId::new(),
            ObservedAtUnixMillis::new(6),
        )
        .expect("reserve placement");
        let PlacementOutcome::Selected(placement) = placement else {
            panic!("managed worker should be selected");
        };
        let mut snapshot_bytes = Vec::new();
        content
            .write_to(&placement.snapshot_id(), &mut snapshot_bytes)
            .expect("read snapshot");
        let snapshot: PlacementSnapshot =
            cairn_codec::from_slice(&snapshot_bytes).expect("decode snapshot");
        assert!(snapshot.candidates()[0].authority_revision().is_some());

        revoke_credential(
            &mut events,
            credential.credential_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(7),
        )
        .expect("revoke after snapshot");
        let error = match grant_reserved_assignment(
            &mut events,
            &content,
            execution_authority,
            &placement,
            grant,
            &authority,
            session_timeout,
            &CommandId::new(),
            ObservedAtUnixMillis::new(8),
        ) {
            Ok(_) => panic!("revocation between snapshot and grant must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            cairn_execution::SchedulerError::AuthorityChanged
        ));
    }

    fn fixture_certificate() -> String {
        let key = KeyPair::generate().expect("fixture certificate key");
        CertificateParams::new(vec!["fixture.test".into()])
            .expect("certificate parameters")
            .self_signed(&key)
            .expect("self-signed certificate")
            .pem()
    }

    fn put<T: ContentType>(
        content: &mut SqliteContentStore,
        bytes: &[u8],
    ) -> cairn_protocol::ContentId<T> {
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put content")
            .content_id
    }
}
