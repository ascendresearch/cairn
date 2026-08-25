use std::{
    error::Error, fs, io::Cursor, net::TcpListener as StdTcpListener, num::NonZeroU64,
    time::Duration,
};

use cairn_control_transport::{ClientTlsFiles, ServerTlsFiles, TransportPolicy};
use cairn_execution::{
    AssignmentLeaseDurationMillis, CapturePolicy, CommandContract, DiagnosticByteLimit,
    EvidenceByteLimit, ExecutionAssignmentState, ExecutionBackend, ExecutionEnvironmentArtifact,
    ExecutionPlatformRequirement, ExecutionTimeoutMillis, InputBundleArtifact, NetworkPolicy,
    OutputByteLimit, PlacementRequest, ReservationClaimTimeoutMillis, ResourceRequest, SandboxPath,
    SchedulerPolicyVersion, WorkerAvailability, WorkerBinaryIdentity, WorkerHealth, WorkerPoolName,
    WorkerProtocolVersion, WorkerSessionState, WorkerSessionTimeoutMillis, WorkerSlotCount,
    recover_execution_assignment, recover_worker_session,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentType, ControlMessageId, CredentialId, JobId,
    LeaseId, ObservedAtUnixMillis, PlacementId, ReservationId, WorkerId,
};
use cairn_record::ContentStore;
use cairn_server::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome,
    ScheduledAssignmentPhase, SchedulerServiceConfig, ServerConfig, ServerStorageConfig,
    WorkerEnrollment, import_static_enrollments, release_execution_reservation_at,
    schedule_execution_contract_at,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_worker::{
    ControllerEndpoint, ExpectedResourceConstraints, ResourceProbeConfig, WorkerConfig,
    WorkerIdentityConfig, WorkerProfileConfig,
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[expect(
    clippy::similar_names,
    clippy::too_many_lines,
    reason = "the two symmetric worker fixtures are clearest side by side"
)]
async fn two_outbound_workers_become_durably_live() -> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let ca = directory.path().join("ca.pem");
    let server_certificate = directory.path().join("server.pem");
    let server_key = directory.path().join("server-key.pem");
    fs::write(&ca, &pki.ca_certificate)?;
    fs::write(&server_certificate, &pki.server.certificate)?;
    fs::write(&server_key, &pki.server.private_key)?;
    let worker_a = write_identity(directory.path(), "worker-a", &pki.worker_a)?;
    let worker_b = write_identity(directory.path(), "worker-b", &pki.worker_b)?;
    let worker_a_id = WorkerId::new();
    let worker_b_id = WorkerId::new();
    let worker_a_credential = CredentialId::new();
    let worker_b_credential = CredentialId::new();

    let port_probe = StdTcpListener::bind("127.0.0.1:0")?;
    let listen = port_probe.local_addr()?;
    drop(port_probe);
    let event_database = directory.path().join("controller-events.sqlite3");
    let content_database = directory.path().join("controller-content.sqlite3");
    let content_directory = directory.path().join("controller-content");
    let protocol = WorkerProtocolVersion::new(1)?;
    let session_timeout = WorkerSessionTimeoutMillis::new(10_000)?;
    let mut controller_config = ServerConfig {
        schema_version: 2,
        listen,
        tls: ServerTlsFiles {
            certificate: server_certificate,
            private_key: server_key,
            client_ca: ca.clone(),
        },
        enrollment: vec![
            WorkerEnrollment {
                worker_id: worker_a_id,
                credential_id: worker_a_credential,
                pool: WorkerPoolName::new("fixture").expect("pool"),
                certificate: worker_a.0.clone(),
            },
            WorkerEnrollment {
                worker_id: worker_b_id,
                credential_id: worker_b_credential,
                pool: WorkerPoolName::new("fixture").expect("pool"),
                certificate: worker_b.0.clone(),
            },
        ],
        enrollment_service: None,
        storage: ServerStorageConfig {
            event_database: event_database.clone(),
            content_database: content_database.clone(),
            content_directory: content_directory.clone(),
        },
        protocol_version: protocol,
        session_timeout_ms: session_timeout,
        scheduler: Some(SchedulerServiceConfig {
            policy_version: SchedulerPolicyVersion::StableWorkerIdQuantitativeV2,
            reservation_claim_timeout_ms: ReservationClaimTimeoutMillis::new(2_000)?,
            assignment_lease_duration_ms: AssignmentLeaseDurationMillis::new(2_000)?,
            assignment_material_byte_limit: None,
        }),
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: NonZeroU64::new(500),
        outbox_poll_interval_ms: NonZeroU64::new(25),
        authority_poll_interval_ms: NonZeroU64::new(25).expect("authority poll"),
        transport: TransportPolicy::default(),
        diagnostic_byte_limit: NonZeroU64::new(256),
    };
    let import_command = CommandId::new();
    let imported = import_static_enrollments(&controller_config, &import_command)?;
    assert_eq!(imported.imported_credentials(), 2);
    assert!(!imported.was_replay());
    let replayed = import_static_enrollments(&controller_config, &import_command)?;
    assert_eq!(replayed.event_id(), imported.event_id());
    assert!(replayed.was_replay());
    assert!(
        import_static_enrollments(&controller_config, &CommandId::new()).is_err(),
        "a new command cannot erase prior import provenance"
    );
    controller_config.schema_version = 3;
    controller_config.enrollment.clear();
    let scheduling_config = controller_config.clone();
    let server = tokio::spawn(cairn_server::run(controller_config));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let controller = ControllerEndpoint {
        tcp_address: listen.to_string(),
        websocket_uri: format!("wss://localhost:{}/control", listen.port()),
    };
    let mismatched_identity = worker_config(
        directory.path(),
        "mismatched",
        worker_b_id,
        controller.clone(),
        &ca,
        worker_a.clone(),
        protocol,
    )?;
    let mismatch = tokio::time::timeout(
        Duration::from_secs(2),
        cairn_worker::run(mismatched_identity),
    )
    .await?;
    assert!(
        mismatch.is_err(),
        "a certificate enrolled to another WorkerId must be rejected"
    );
    let worker_config_a = worker_config(
        directory.path(),
        "a",
        worker_a_id,
        controller.clone(),
        &ca,
        worker_a,
        protocol,
    )?;
    let worker_task_a = tokio::spawn(async move {
        let outcome = cairn_worker::run(worker_config_a).await;
        eprintln!("worker A outcome: {outcome:?}");
        outcome
    });
    let worker_config_b = worker_config(
        directory.path(),
        "b",
        worker_b_id,
        controller,
        &ca,
        worker_b,
        protocol,
    )?;
    let worker_task_b = tokio::spawn(async move {
        let outcome = cairn_worker::run(worker_config_b).await;
        eprintln!("worker B outcome: {outcome:?}");
        outcome
    });

    let live = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = SqliteEventStore::open(&event_database).expect("open event projection");
            let content = SqliteContentStore::open(&content_database, &content_directory)
                .expect("open content projection");
            let now =
                ObservedAtUnixMillis::new(chrono_free_unix_millis().expect("current Unix time"));
            let a = recover_worker_session(&events, &content, worker_a_id, session_timeout, now);
            let b = recover_worker_session(&events, &content, worker_b_id, session_timeout, now);
            if let (Ok(WorkerSessionState::Live(a)), Ok(WorkerSessionState::Live(b))) = (a, b) {
                break (
                    a.resource_observation_revision(),
                    b.resource_observation_revision(),
                );
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(
        live.is_ok(),
        "durable liveness timed out; server_finished={}, worker_a_finished={}, worker_b_finished={}",
        server.is_finished(),
        worker_task_a.is_finished(),
        worker_task_b.is_finished()
    );
    let initial_resource_revisions = live.expect("durable liveness");

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    assert!(
        !server.is_finished() && !worker_task_a.is_finished() && !worker_task_b.is_finished(),
        "heartbeat acknowledgements must keep both idle-bounded sessions open"
    );
    let refreshed_events = SqliteEventStore::open(&event_database)?;
    let refreshed_content = SqliteContentStore::open(&content_database, &content_directory)?;
    let refreshed_at = ObservedAtUnixMillis::new(chrono_free_unix_millis()?);
    let WorkerSessionState::Live(refreshed_a) = recover_worker_session(
        &refreshed_events,
        &refreshed_content,
        worker_a_id,
        session_timeout,
        refreshed_at,
    )?
    else {
        return Err("worker A lost liveness during resource refresh".into());
    };
    let WorkerSessionState::Live(refreshed_b) = recover_worker_session(
        &refreshed_events,
        &refreshed_content,
        worker_b_id,
        session_timeout,
        refreshed_at,
    )?
    else {
        return Err("worker B lost liveness during resource refresh".into());
    };
    assert_ne!(
        refreshed_a.resource_observation_revision(),
        initial_resource_revisions.0,
        "worker A resource refresh must become a distinct durable fact"
    );
    assert_ne!(
        refreshed_b.resource_observation_revision(),
        initial_resource_revisions.1,
        "worker B resource refresh must become a distinct durable fact"
    );

    let mut content = SqliteContentStore::open(&content_database, &content_directory)?;
    let input = put::<InputBundleArtifact>(&mut content, b"dual-worker-input")?;
    let environment = put::<ExecutionEnvironmentArtifact>(&mut content, b"dual-worker-env")?;
    let contract = cairn_execution::JobContract::new(
        JobId::new(),
        input,
        environment,
        ExecutionBackend::new("transport-test")?,
        CommandContract::new(
            SandboxPath::new("bin/fixture")?,
            Vec::new(),
            SandboxPath::new("work")?,
        ),
        ResourceRequest::new(
            ExecutionTimeoutMillis::new(1_000)?,
            PlacementRequest::new(
                ExecutionPlatformRequirement::default(),
                vec![WorkerPoolName::new("fixture")?],
                Vec::new(),
            )?,
        )?,
        NetworkPolicy::Disabled,
        CapturePolicy::new(
            OutputByteLimit::new(1_024)?,
            OutputByteLimit::new(1_024)?,
            DiagnosticByteLimit::new(1_024)?,
            EvidenceByteLimit::new(4_096)?,
            Vec::new(),
        )?,
    );
    let schedule_ids = schedule_ids();
    let scheduled_at = ObservedAtUnixMillis::new(chrono_free_unix_millis()?);
    let ControllerSchedulingOutcome::Scheduled {
        placement, binding, ..
    } = schedule_execution_contract_at(&scheduling_config, &contract, schedule_ids, scheduled_at)?
    else {
        return Err("ready dual-worker fixture had no scheduling candidate".into());
    };
    assert_eq!(binding.worker_id(), worker_a_id.min(worker_b_id));
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = SqliteEventStore::open(&event_database).expect("open assignment events");
            let content = SqliteContentStore::open(&content_database, &content_directory)
                .expect("open assignment content");
            let now =
                ObservedAtUnixMillis::new(chrono_free_unix_millis().expect("current Unix time"));
            if matches!(
                recover_execution_assignment(&events, &content, schedule_ids.attempt_id, now),
                Ok(ExecutionAssignmentState::ExecutionTerminal { .. })
            ) {
                break now;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    let ControllerSchedulingOutcome::Scheduled {
        binding: recovered_binding,
        phase: recovered_phase,
        ..
    } = schedule_execution_contract_at(&scheduling_config, &contract, schedule_ids, terminal)?
    else {
        return Err("terminal scheduling retry lost its durable placement".into());
    };
    assert_eq!(recovered_binding, binding);
    assert_eq!(recovered_phase, ScheduledAssignmentPhase::Terminal);
    let selected_worker_suffix = if binding.worker_id() == worker_a_id {
        "a"
    } else {
        assert_eq!(binding.worker_id(), worker_b_id);
        "b"
    };
    let selected_worker_content = SqliteContentStore::open(
        directory
            .path()
            .join(format!("worker-{selected_worker_suffix}-content.sqlite3")),
        directory
            .path()
            .join(format!("worker-{selected_worker_suffix}-content")),
    )?;
    assert_eq!(
        read::<InputBundleArtifact>(&selected_worker_content, &contract.input_bundle_id())?,
        b"dual-worker-input"
    );
    assert_eq!(
        read::<ExecutionEnvironmentArtifact>(&selected_worker_content, &contract.environment_id(),)?,
        b"dual-worker-env"
    );
    assert_eq!(
        release_execution_reservation_at(
            &scheduling_config,
            placement.reservation_id().expect("reservation"),
            &CommandId::new(),
            terminal,
        )?,
        cairn_execution::ReservationReleaseReason::ExecutionTerminal
    );

    worker_task_a.abort();
    worker_task_b.abort();
    server.abort();
    Ok(())
}

fn worker_config(
    directory: &std::path::Path,
    suffix: &str,
    worker_id: WorkerId,
    controller: ControllerEndpoint,
    ca: &std::path::Path,
    identity: (std::path::PathBuf, std::path::PathBuf),
    protocol: WorkerProtocolVersion,
) -> Result<WorkerConfig, Box<dyn Error + Send + Sync>> {
    Ok(WorkerConfig {
        schema_version: 6,
        controller,
        identity: WorkerIdentityConfig::External {
            worker_id,
            tls: ClientTlsFiles {
                certificate: identity.0,
                private_key: identity.1,
                server_ca: ca.to_path_buf(),
                server_name: "localhost".into(),
            },
        },
        profile: WorkerProfileConfig {
            schema_version: 2,
            protocol_version: protocol,
            binary_identity: WorkerBinaryIdentity::new("sha256:transport-test")?,
            backends: vec![ExecutionBackend::new("transport-test")?],
            capabilities: Vec::new(),
            max_concurrency: WorkerSlotCount::new(1)?,
        },
        expected_platform: ExecutionPlatformRequirement::default(),
        resource_probe: ResourceProbeConfig {
            scratch_path: directory.to_path_buf(),
            accelerator_sysfs: None,
            freshness_ms: None,
            refresh_interval_ms: NonZeroU64::new(500),
            expected: ExpectedResourceConstraints::default(),
        },
        availability: WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())?,
        journal_database: directory.join(format!("worker-{suffix}.sqlite3")),
        content: cairn_worker::WorkerContentConfig {
            database: directory.join(format!("worker-{suffix}-content.sqlite3")),
            directory: directory.join(format!("worker-{suffix}-content")),
            assignment_material_byte_limit: None,
        },
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: None,
        // Keep several heartbeats inside the idle window while leaving scheduling a meaningful
        // quiescent interval; a 50 ms cadence made the explicit-time scheduler test race its own
        // fixture heartbeat under a loaded CI host.
        heartbeat_interval_ms: NonZeroU64::new(250),
        identity_poll_interval_ms: NonZeroU64::new(25).expect("identity poll"),
        reconnect_delay_ms: None,
        transport: TransportPolicy::default(),
    })
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

fn put<T: ContentType>(
    content: &mut SqliteContentStore,
    bytes: &[u8],
) -> Result<cairn_protocol::ContentId<T>, Box<dyn Error + Send + Sync>> {
    Ok(content.put::<T>(&mut Cursor::new(bytes))?.content_id)
}

fn read<T: ContentType>(
    content: &SqliteContentStore,
    content_id: &cairn_protocol::ContentId<T>,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let mut bytes = Vec::new();
    content.write_to(content_id, &mut bytes)?;
    Ok(bytes)
}

fn chrono_free_unix_millis() -> Result<i64, Box<dyn Error + Send + Sync>> {
    let duration = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_millis())?)
}

fn write_identity(
    directory: &std::path::Path,
    name: &str,
    identity: &PemIdentity,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn Error + Send + Sync>> {
    let certificate = directory.join(format!("{name}.pem"));
    let key = directory.join(format!("{name}-key.pem"));
    fs::write(&certificate, &identity.certificate)?;
    fs::write(&key, &identity.private_key)?;
    Ok((certificate, key))
}

struct TestPki {
    ca_certificate: String,
    server: PemIdentity,
    worker_a: PemIdentity,
    worker_b: PemIdentity,
}

struct PemIdentity {
    certificate: String,
    private_key: String,
}

fn test_pki() -> Result<TestPki, rcgen::Error> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Cairn dual-worker test CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let ca = ca_params.self_signed(&ca_key)?;
    Ok(TestPki {
        ca_certificate: ca.pem(),
        server: signed_identity(
            "localhost",
            vec!["localhost".into()],
            ExtendedKeyUsagePurpose::ServerAuth,
            &ca,
            &ca_key,
        )?,
        worker_a: signed_identity(
            "worker-a",
            Vec::new(),
            ExtendedKeyUsagePurpose::ClientAuth,
            &ca,
            &ca_key,
        )?,
        worker_b: signed_identity(
            "worker-b",
            Vec::new(),
            ExtendedKeyUsagePurpose::ClientAuth,
            &ca,
            &ca_key,
        )?,
    })
}

fn signed_identity(
    common_name: &str,
    subject_alt_names: Vec<String>,
    purpose: ExtendedKeyUsagePurpose,
    ca: &rcgen::Certificate,
    ca_key: &KeyPair,
) -> Result<PemIdentity, rcgen::Error> {
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::new(subject_alt_names)?;
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![purpose];
    let certificate = params.signed_by(&key, ca, ca_key)?;
    Ok(PemIdentity {
        certificate: certificate.pem(),
        private_key: key.serialize_pem(),
    })
}
