use std::{error::Error, io::Cursor, net::SocketAddr, num::NonZeroU64};

use cairn_control_transport::{ServerTlsFiles, TransportPolicy};
use cairn_execution::{
    AcceleratorDiscoveryCompleteness, ArchitectureName, AssignmentLeaseDurationMillis,
    AuthenticatedWorkerIdentity, CapturePolicy, CommandContract, DiagnosticByteLimit,
    EvidenceByteLimit, ExecutionBackend, ExecutionEnvironmentArtifact, ExecutionPlatform,
    ExecutionTimeoutMillis, InputBundleArtifact, LogicalCpuCount, MemoryByteCount, NetworkPolicy,
    OperatingSystemName, OutputByteLimit, RecordedWorkerAuthenticator,
    ReservationClaimTimeoutMillis, ResourceProbeVersion, SandboxPath, SchedulerPolicyVersion,
    ScratchByteCount, WorkerAuthenticationSubject, WorkerAvailability, WorkerBinaryIdentity,
    WorkerHealth, WorkerHello, WorkerPoolName, WorkerProfile, WorkerProtocolVersion,
    WorkerResourceClaim, WorkerResourceInventory, WorkerResourceObservation, WorkerResourceSource,
    WorkerSessionTimeoutMillis, WorkerSlotCount, pending_controller_messages,
    record_worker_heartbeat, register_worker,
};
use cairn_migration::{MigrationExecutionNeed, MigrationValidationTier};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ControlMessageId, CredentialId, JobId, LeaseId,
    ObservedAtUnixMillis, PlacementId, ReservationId, WorkerId, WorkerIncarnationId,
};
use cairn_record::ContentStore;
use cairn_server::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome,
    ScheduledAssignmentPhase, SchedulerServiceConfig, ServerConfig, ServerStorageConfig,
    WorkerEnrollment, release_execution_reservation_at, schedule_execution_contract_at,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one fixture proves migration translation, scheduling retry, outbox, and safe release"
)]
fn migration_need_reaches_durable_worker_assignment_and_releases_only_when_safe()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let event_database = directory.path().join("events.sqlite3");
    let content_database = directory.path().join("content.sqlite3");
    let content_directory = directory.path().join("content");
    let worker_id = WorkerId::new();
    let credential_id = CredentialId::new();
    let pool = WorkerPoolName::new("target-lab")?;
    let config = ServerConfig {
        schema_version: 2,
        listen: "127.0.0.1:7443".parse::<SocketAddr>()?,
        tls: ServerTlsFiles {
            certificate: directory.path().join("unused-controller.pem"),
            private_key: directory.path().join("unused-controller-key.pem"),
            client_ca: directory.path().join("unused-ca.pem"),
        },
        enrollment: vec![WorkerEnrollment {
            worker_id,
            credential_id,
            pool: pool.clone(),
            certificate: directory.path().join("unused-worker.pem"),
        }],
        enrollment_service: None,
        storage: ServerStorageConfig {
            event_database: event_database.clone(),
            content_database: content_database.clone(),
            content_directory: content_directory.clone(),
        },
        protocol_version: WorkerProtocolVersion::new(1)?,
        session_timeout_ms: WorkerSessionTimeoutMillis::new(100)?,
        scheduler: Some(SchedulerServiceConfig {
            policy_version: SchedulerPolicyVersion::StableWorkerIdV1,
            reservation_claim_timeout_ms: ReservationClaimTimeoutMillis::new(20)?,
            assignment_lease_duration_ms: AssignmentLeaseDurationMillis::new(40)?,
        }),
        handshake_timeout_ms: None,
        idle_timeout_ms: None,
        outbox_poll_interval_ms: None,
        authority_poll_interval_ms: NonZeroU64::new(10).expect("authority poll"),
        transport: TransportPolicy::default(),
        diagnostic_byte_limit: None,
    };

    let mut events = SqliteEventStore::open(&event_database)?;
    let mut content = SqliteContentStore::open(&content_database, &content_directory)?;
    let profile = WorkerProfile::new(
        WorkerProtocolVersion::new(1)?,
        WorkerBinaryIdentity::new("sha256:migration-fixture")?,
        WorkerResourceInventory::new(
            WorkerResourceClaim::new(
                ExecutionPlatform::new(
                    ArchitectureName::new("aarch64")?,
                    OperatingSystemName::new("linux")?,
                    cairn_execution::TargetEnvironmentName::new("gnu")?,
                ),
                WorkerResourceSource::BuiltinProbe,
            ),
            vec![WorkerResourceClaim::new(
                ExecutionBackend::new("container")?,
                WorkerResourceSource::OperatorDeclared,
            )],
            Vec::new(),
            WorkerResourceObservation::new(
                WorkerResourceSource::BuiltinProbe,
                ResourceProbeVersion::new("fixture-probe-v1")?,
                ObservedAtUnixMillis::new(10),
                None,
                LogicalCpuCount::new(8)?,
                MemoryByteCount::new(16 * 1024 * 1024 * 1024)?,
                ScratchByteCount::new(64 * 1024 * 1024 * 1024)?,
                AcceleratorDiscoveryCompleteness::Complete,
                Vec::new(),
            )?,
            WorkerSlotCount::new(1)?,
        )?,
    )?;
    let hello = WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile);
    let mut authenticator = RecordedWorkerAuthenticator::new([(
        worker_id,
        AuthenticatedWorkerIdentity::new(
            WorkerAuthenticationSubject::new("fixture-static-credential")?,
            credential_id,
            pool.clone(),
        ),
    )]);
    let registered = register_worker(
        &mut events,
        &mut content,
        &mut authenticator,
        &hello,
        config.session_timeout_ms,
        &CommandId::new(),
        ObservedAtUnixMillis::new(10),
    )?;
    record_worker_heartbeat(
        &mut events,
        &mut content,
        &registered,
        &WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())?,
        &CommandId::new(),
        ObservedAtUnixMillis::new(11),
    )?;

    let need = MigrationExecutionNeed::new(
        MigrationValidationTier::V3TargetDevice,
        ExecutionBackend::new("container")?,
        ExecutionTimeoutMillis::new(30_000)?,
        Some(ArchitectureName::new("aarch64")?),
        Some(OperatingSystemName::new("linux")?),
        Some(cairn_execution::TargetEnvironmentName::new("gnu")?),
        vec![pool],
        Vec::new(),
    )?;
    let input = content
        .put::<InputBundleArtifact>(&mut Cursor::new(b"migration-input"))?
        .content_id;
    let environment = content
        .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(b"fixture-environment"))?
        .content_id;
    let contract = cairn_execution::JobContract::new(
        JobId::new(),
        input,
        environment,
        need.backend().clone(),
        CommandContract::new(
            SandboxPath::new("bin/validate")?,
            Vec::new(),
            SandboxPath::new("work")?,
        ),
        need.to_resource_request()?,
        NetworkPolicy::Disabled,
        CapturePolicy::new(
            OutputByteLimit::new(1024)?,
            OutputByteLimit::new(1024)?,
            DiagnosticByteLimit::new(1024)?,
            EvidenceByteLimit::new(4096)?,
            Vec::new(),
        )?,
    );
    let ids = schedule_ids();
    let outcome =
        schedule_execution_contract_at(&config, &contract, ids, ObservedAtUnixMillis::new(12))?;
    let ControllerSchedulingOutcome::Scheduled {
        placement,
        binding,
        phase,
    } = outcome
    else {
        return Err("matching migration need had no candidate".into());
    };
    assert_eq!(binding.worker_id(), worker_id);
    assert_eq!(phase, ScheduledAssignmentPhase::OfferPending);
    assert_eq!(pending_controller_messages(&events, worker_id)?.len(), 1);

    let retry =
        schedule_execution_contract_at(&config, &contract, ids, ObservedAtUnixMillis::new(13))?;
    let ControllerSchedulingOutcome::Scheduled {
        binding: retried, ..
    } = retry
    else {
        return Err("exact retry lost assignment".into());
    };
    assert_eq!(retried, binding);
    let reservation_id = placement.reservation_id().expect("reservation");
    let unsafe_release = release_execution_reservation_at(
        &config,
        reservation_id,
        &CommandId::new(),
        ObservedAtUnixMillis::new(13),
    )
    .expect_err("a live lease must retain capacity");
    assert!(
        unsafe_release
            .to_string()
            .contains("cannot be released safely")
    );
    let released = release_execution_reservation_at(
        &config,
        reservation_id,
        &CommandId::new(),
        ObservedAtUnixMillis::new(52),
    )?;
    assert_eq!(
        released,
        cairn_execution::ReservationReleaseReason::ExpiredBeforeStart
    );

    let placement_wire = serde_json::to_string(contract.resources().placement())?;
    assert!(!placement_wire.contains("migration"));
    assert!(!placement_wire.contains("target-device"));
    Ok(())
}

fn schedule_ids() -> ControllerScheduleIds {
    ControllerScheduleIds {
        attempt_id: AttemptId::new(),
        placement_id: PlacementId::new(),
        reservation_id: ReservationId::new(),
        assignment_id: AssignmentId::new(),
        lease_id: LeaseId::new(),
        offer_message_id: ControlMessageId::new(),
        start_message_id: ControlMessageId::new(),
        commands: ControllerScheduleCommandIds {
            authorize_attempt: CommandId::new(),
            reserve_placement: CommandId::new(),
            grant_assignment: CommandId::new(),
            enqueue_offer: CommandId::new(),
        },
    }
}
