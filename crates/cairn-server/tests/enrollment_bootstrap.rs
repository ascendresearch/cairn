use std::{
    error::Error, fs, net::TcpListener as StdTcpListener, num::NonZeroU64, path::Path,
    time::Duration,
};

use cairn_control_transport::{ClientTlsFiles, EnrollmentSecret, ServerTlsFiles, TransportPolicy};
use cairn_execution::{
    ExecutionBackend, ExecutionPlatformRequirement, WorkerAvailability, WorkerBinaryIdentity,
    WorkerHealth, WorkerPoolName, WorkerProtocolVersion, WorkerSessionState,
    WorkerSessionTimeoutMillis, WorkerSlotCount, recover_worker_session,
};
use cairn_protocol::{AggregateId, AggregateKind, CommandId, ObservedAtUnixMillis};
use cairn_record::{EventStore, StreamId};
use cairn_server::{
    EnrollmentServiceConfig, ServerConfig, ServerStorageConfig, assign_enrolled_worker_pool,
    create_enrollment_bundle, create_rotation_bundle, disable_enrolled_worker,
    enable_enrolled_worker, revoke_enrollment_authority, revoke_worker_credential,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_worker::{
    ControllerEndpoint, ExpectedResourceConstraints, ResourceProbeConfig, WorkerConfig,
    WorkerExecutionConfig, WorkerIdentityConfig, WorkerProfileConfig, enroll, join_from_bundle,
    rollback_rotation, rotate,
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};

static ENROLLMENT_INTEGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[expect(
    clippy::too_many_lines,
    reason = "the separate-CA bundle, CLI join, exact rerun, and live control session form one proof"
)]
async fn one_command_join_persists_and_reuses_a_runnable_worker_tree()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let _integration_guard = ENROLLMENT_INTEGRATION_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let ca = directory.path().join("ca.pem");
    let ca_key = directory.path().join("ca-key.pem");
    let server_certificate = directory.path().join("server.pem");
    let server_key = directory.path().join("server-key.pem");
    fs::write(&ca, &pki.ca_certificate)?;
    fs::write(&ca_key, &pki.ca_private_key)?;
    fs::write(&server_certificate, &pki.server_certificate)?;
    fs::write(&server_key, &pki.server_private_key)?;
    let enrollment_pki = test_pki()?;
    let enrollment_server_ca = directory.path().join("enrollment-server-ca.pem");
    let enrollment_server_certificate = directory.path().join("enrollment-server.pem");
    let enrollment_server_key = directory.path().join("enrollment-server-key.pem");
    fs::write(&enrollment_server_ca, &enrollment_pki.ca_certificate)?;
    fs::write(
        &enrollment_server_certificate,
        &enrollment_pki.server_certificate,
    )?;
    fs::write(&enrollment_server_key, &enrollment_pki.server_private_key)?;
    let control = free_address()?;
    let enrollment = free_address()?;
    let event_database = directory.path().join("controller-events.sqlite3");
    let content_database = directory.path().join("controller-content.sqlite3");
    let content_directory = directory.path().join("controller-content");
    let mut config = server_config(
        control,
        enrollment,
        &ca,
        &ca_key,
        &server_certificate,
        &server_key,
        &event_database,
        &content_database,
        &content_directory,
    )?;
    let enrollment_service = config
        .enrollment_service
        .as_mut()
        .expect("enrollment service");
    enrollment_service
        .server_ca
        .clone_from(&enrollment_server_ca);
    enrollment_service.server_tls = cairn_server::EnrollmentServerTlsFiles {
        certificate: enrollment_server_certificate,
        private_key: enrollment_server_key,
    };
    let bundle = create_enrollment_bundle(
        &config,
        WorkerPoolName::new("join-lab")?,
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    assert_eq!(bundle.schema_version, 1);
    assert_ne!(
        bundle.endpoint.server_ca_pem,
        bundle.control_endpoint.server_ca_pem
    );
    let bundle_path = directory.path().join("join-bundle.json");
    fs::write(&bundle_path, serde_json::to_vec_pretty(&bundle)?)?;
    let server = tokio::spawn(cairn_server::run(config));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = directory.path().join("worker-state");
    cairn_worker::run_from_arguments([
        std::ffi::OsString::from("cairn-worker"),
        std::ffi::OsString::from("join"),
        bundle_path.as_os_str().to_owned(),
        state.as_os_str().to_owned(),
    ])
    .await?;
    let mut operator_config: WorkerConfig =
        serde_json::from_slice(&fs::read(state.join("worker.json"))?)?;
    operator_config.heartbeat_interval_ms = NonZeroU64::new(17_000);
    let operator_config = serde_json::to_vec_pretty(&operator_config)?;
    fs::write(state.join("worker.json"), &operator_config)?;
    let receipt = Box::pin(join_from_bundle(&bundle_path, &state)).await?;
    assert_eq!(receipt.config_path, state.join("worker.json"));
    assert!(state.join("identity/worker-key.pem").is_file());
    assert!(state.join("identity/identity.json").is_file());
    assert!(state.join("scratch").is_dir());
    assert!(state.join("content.sqlite3").is_file());
    assert!(state.join("content").is_dir());
    assert!(state.join("transfers").is_dir());
    assert_eq!(fs::read(&receipt.config_path)?, operator_config);

    let worker = tokio::spawn(cairn_worker::run_from_arguments([
        std::ffi::OsString::from("cairn-worker"),
        receipt.config_path.as_os_str().to_owned(),
    ]));
    tokio::time::sleep(Duration::from_millis(250)).await;
    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let WorkerSessionState::Live(session) = recover_worker_session(
        &events,
        &content,
        receipt.worker_id,
        WorkerSessionTimeoutMillis::new(10_000)?,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?
    else {
        return Err("joined worker did not establish a live control session".into());
    };
    assert_eq!(session.credential_id(), receipt.credential_id);
    assert_eq!(session.pool().as_str(), "join-lab");
    assert_eq!(
        session.availability().expect("availability").health(),
        WorkerHealth::Unavailable
    );

    worker.abort();
    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[expect(
    clippy::too_many_lines,
    reason = "the complete bootstrap, replay, restart, and control path is one proof"
)]
async fn one_shot_bootstrap_survives_response_loss_and_controller_restart()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let _integration_guard = ENROLLMENT_INTEGRATION_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let ca = directory.path().join("ca.pem");
    let ca_key = directory.path().join("ca-key.pem");
    let server_certificate = directory.path().join("server.pem");
    let server_key = directory.path().join("server-key.pem");
    fs::write(&ca, &pki.ca_certificate)?;
    fs::write(&ca_key, &pki.ca_private_key)?;
    fs::write(&server_certificate, &pki.server_certificate)?;
    fs::write(&server_key, &pki.server_private_key)?;

    let control_a = free_address()?;
    let enrollment_a = free_address()?;
    let event_database = directory.path().join("controller-events.sqlite3");
    let content_database = directory.path().join("controller-content.sqlite3");
    let content_directory = directory.path().join("controller-content");
    let config_a = server_config(
        control_a,
        enrollment_a,
        &ca,
        &ca_key,
        &server_certificate,
        &server_key,
        &event_database,
        &content_database,
        &content_directory,
    )?;
    let pool = WorkerPoolName::new("migration-lab")?;
    let bundle = create_enrollment_bundle(
        &config_a,
        pool.clone(),
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    let issuance_config = config_a.clone();
    let secret_wire = serde_json::to_string(&bundle.secret)?;
    let database_bytes = fs::read(&event_database)?;
    assert!(
        !database_bytes
            .windows(secret_wire.trim_matches('"').len())
            .any(|window| window == secret_wire.trim_matches('"').as_bytes()),
        "the bearer secret must not be persisted"
    );
    let server_a = tokio::spawn(cairn_server::run(config_a));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state = directory.path().join("managed-worker");
    let identity = Box::pin(enroll(bundle.clone(), &state)).await?;
    assert_eq!(identity.pool, pool);
    assert!(state.join("worker-key.pem").is_file());
    assert!(state.join("worker.pem").is_file());
    assert!(state.join("ca.pem").is_file());
    assert!(state.join("identity.json").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(fs::metadata(&state)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(state.join("worker-key.pem"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    // Simulate a lost response: only the staged key and exact CSR survive. The controller returns
    // the original credential instead of issuing a second WorkerId.
    let recovery = directory.path().join("response-loss-recovery");
    fs::create_dir(&recovery)?;
    fs::copy(
        state.join("worker-key.pem"),
        recovery.join("worker-key.pem"),
    )?;
    fs::copy(
        state.join("enrollment.csr.pem"),
        recovery.join("enrollment.csr.pem"),
    )?;
    let recovered = Box::pin(enroll(bundle.clone(), &recovery)).await?;
    assert_eq!(recovered.worker_id, identity.worker_id);
    assert_eq!(recovered.credential_id, identity.credential_id);
    assert_eq!(
        fs::read(state.join("worker.pem"))?,
        fs::read(recovery.join("worker.pem"))?
    );

    // The same bearer authority cannot enroll a different worker-local key.
    let wrong_key = directory.path().join("wrong-key");
    let error = Box::pin(enroll(bundle.clone(), &wrong_key))
        .await
        .expect_err("a second CSR must be rejected");
    assert!(error.to_string().contains("AlreadyUsed"));

    let mut invalid_secret = bundle.clone();
    invalid_secret.secret = EnrollmentSecret::from_bytes([0_u8; 32]);
    let error = Box::pin(enroll(
        invalid_secret,
        &directory.path().join("invalid-secret"),
    ))
    .await
    .expect_err("an invalid bearer secret must be rejected");
    assert!(error.to_string().contains("InvalidAuthority"));

    let expired = create_enrollment_bundle(
        &issuance_config,
        pool.clone(),
        NonZeroU64::new(1).expect("TTL"),
    )?;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let error = Box::pin(enroll(expired, &directory.path().join("expired-authority")))
        .await
        .expect_err("an expired authority must be rejected by the controller");
    assert!(error.to_string().contains("Expired"));

    let cancelled = create_enrollment_bundle(
        &issuance_config,
        pool.clone(),
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    revoke_enrollment_authority(&issuance_config, cancelled.enrollment_id, &CommandId::new())?;
    let error = Box::pin(enroll(
        cancelled,
        &directory.path().join("revoked-authority"),
    ))
    .await
    .expect_err("a revoked enrollment authority must be rejected");
    assert!(error.to_string().contains("InvalidAuthority"));

    let disabled_bundle = create_enrollment_bundle(
        &issuance_config,
        pool.clone(),
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    let disabled_state = directory.path().join("disabled-worker");
    let disabled_identity = Box::pin(enroll(disabled_bundle, &disabled_state)).await?;
    let reassigned_bundle = create_enrollment_bundle(
        &issuance_config,
        pool.clone(),
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    let reassigned_state = directory.path().join("reassigned-worker");
    let reassigned_identity = Box::pin(enroll(reassigned_bundle, &reassigned_state)).await?;

    // A fresh controller instance reconstructs certificate -> stable WorkerId/pool authorization
    // solely from the durable enrollment stream.
    let control_b = free_address()?;
    let enrollment_b = free_address()?;
    let config_b = server_config(
        control_b,
        enrollment_b,
        &ca,
        &ca_key,
        &server_certificate,
        &server_key,
        &event_database,
        &content_database,
        &content_directory,
    )?;
    let authority_config = config_b.clone();
    let server_b = tokio::spawn(cairn_server::run(config_b));
    tokio::time::sleep(Duration::from_millis(50)).await;
    disable_enrolled_worker(
        &authority_config,
        disabled_identity.worker_id,
        &CommandId::new(),
    )?;
    let mut disabled_worker_config = worker_config(control_b, &disabled_state)?;
    disabled_worker_config.reconnect_delay_ms = None;
    let disabled_worker = tokio::spawn(cairn_worker::run(disabled_worker_config));
    let worker = tokio::spawn(cairn_worker::run(worker_config(control_b, &state)?));
    let reassigned_worker = tokio::spawn(cairn_worker::run(worker_config(
        control_b,
        &reassigned_state,
    )?));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let session = recover_worker_session(
        &events,
        &content,
        identity.worker_id,
        WorkerSessionTimeoutMillis::new(10_000)?,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?;
    let WorkerSessionState::Live(session) = session else {
        return Err("issued worker did not establish a live control session".into());
    };
    assert_eq!(session.pool(), &pool);
    assert_eq!(session.credential_id(), identity.credential_id);

    // Pool authority is a separate registry lifecycle. Disabling closes the existing execution
    // incarnation; reassignment then re-enable lets the same worker process reconnect, while the
    // execution stream cites the exact registry assignment fact.
    let WorkerSessionState::Live(initial_reassigned_session) = recover_worker_session(
        &events,
        &content,
        reassigned_identity.worker_id,
        WorkerSessionTimeoutMillis::new(10_000)?,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?
    else {
        return Err("pool-reassignment worker did not establish its initial session".into());
    };
    assert_eq!(initial_reassigned_session.pool(), &pool);
    disable_enrolled_worker(
        &authority_config,
        reassigned_identity.worker_id,
        &CommandId::new(),
    )?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let moved_pool = WorkerPoolName::new("post-bootstrap-pool")?;
    let pool_outcome = assign_enrolled_worker_pool(
        &authority_config,
        reassigned_identity.worker_id,
        moved_pool.clone(),
        &CommandId::new(),
    )?;
    enable_enrolled_worker(
        &authority_config,
        reassigned_identity.worker_id,
        &CommandId::new(),
    )?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let WorkerSessionState::Live(moved_session) = recover_worker_session(
        &events,
        &content,
        reassigned_identity.worker_id,
        WorkerSessionTimeoutMillis::new(10_000)?,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?
    else {
        return Err("pool-reassignment worker did not reconnect".into());
    };
    assert_eq!(moved_session.pool(), &moved_pool);
    assert_eq!(
        moved_session.pool_assignment_revision(),
        Some(pool_outcome.event_id())
    );

    // A successor is issued to a fresh local key without changing stable worker ownership. If it
    // is revoked inside the overlap, the registry cancels predecessor retirement and the worker
    // can atomically restore its previous identity.
    let first_rotation = create_rotation_bundle(
        &authority_config,
        identity.credential_id,
        NonZeroU64::new(60_000).expect("rotation TTL"),
    )?;
    let predecessor_identity_bytes = fs::read(state.join("identity.json"))?;
    let first_successor = Box::pin(rotate(first_rotation.clone(), &state)).await?;
    let first_successor_certificate = fs::read(state.join(&first_successor.tls.certificate))?;
    // Simulate loss of the local identity commit acknowledgement after controller issuance. The
    // staged key/CSR survive; retry returns the exact credential and certificate.
    fs::write(state.join("identity.json"), predecessor_identity_bytes)?;
    let recovered_successor = Box::pin(rotate(first_rotation, &state)).await?;
    assert_eq!(recovered_successor, first_successor);
    assert_eq!(
        fs::read(state.join(&recovered_successor.tls.certificate))?,
        first_successor_certificate
    );
    assert_eq!(first_successor.worker_id, identity.worker_id);
    assert_eq!(first_successor.pool, identity.pool);
    assert_ne!(first_successor.credential_id, identity.credential_id);
    assert_eq!(
        first_successor.predecessor_credential_id,
        Some(identity.credential_id)
    );
    assert!(first_successor.predecessor_retire_at.is_some());
    revoke_worker_credential(
        &authority_config,
        first_successor.credential_id,
        &CommandId::new(),
    )?;
    let rolled_back = rollback_rotation(&state)?;
    assert_eq!(rolled_back.credential_id, identity.credential_id);

    // A second rotation is allowed because the failed successor rollback restored predecessor
    // authority. Once its overlap elapses, the controller closes the old live connection; the
    // same worker process reloads identity and reconnects under a fresh incarnation.
    let second_rotation = create_rotation_bundle(
        &authority_config,
        identity.credential_id,
        NonZeroU64::new(60_000).expect("rotation TTL"),
    )?;
    let second_successor = Box::pin(rotate(second_rotation, &state)).await?;
    assert_ne!(
        second_successor.credential_id,
        first_successor.credential_id
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let WorkerSessionState::Live(rotated_session) = recover_worker_session(
        &events,
        &content,
        identity.worker_id,
        WorkerSessionTimeoutMillis::new(10_000)?,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?
    else {
        return Err("rotated worker did not reconnect with its successor credential".into());
    };
    assert_eq!(
        rotated_session.credential_id(),
        second_successor.credential_id
    );
    assert_ne!(rotated_session.incarnation_id(), session.incarnation_id());
    assert!(
        rollback_rotation(&state)
            .expect_err("elapsed overlap must reject local rollback")
            .to_string()
            .contains("overlap has elapsed")
    );
    let mut retired_config = worker_config(control_b, &state)?;
    retired_config.identity = WorkerIdentityConfig::External {
        worker_id: identity.worker_id,
        tls: ClientTlsFiles {
            certificate: state.join(&identity.tls.certificate),
            private_key: state.join(&identity.tls.private_key),
            server_ca: state.join(&identity.tls.server_ca),
            server_name: identity.tls.server_name.clone(),
        },
    };
    retired_config.journal_database = directory.path().join("retired-worker.sqlite3");
    retired_config.reconnect_delay_ms = None;
    assert!(
        Box::pin(cairn_worker::run(retired_config))
            .await
            .expect_err("retired predecessor must fail authentication")
            .to_string()
            .contains("IdentityMismatch")
    );

    let registration_count_before_revocation = events
        .read_stream(
            &StreamId {
                kind: AggregateKind::new("execution-worker")?,
                id: AggregateId::new(identity.worker_id.to_string())?,
            },
            None,
        )?
        .iter()
        .filter(|event| {
            matches!(
                event.schema_name.as_str(),
                "execution.worker-registered" | "execution.worker-replaced-after-expiry"
            )
        })
        .count();

    // Revocation is a durable authority fact. The running controller observes it, terminates the
    // live session, and rejects the worker's automatic reconnect before a new registration fact.
    revoke_worker_credential(
        &authority_config,
        second_successor.credential_id,
        &CommandId::new(),
    )?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    assert!(matches!(
        recover_worker_session(
            &events,
            &content,
            identity.worker_id,
            WorkerSessionTimeoutMillis::new(10_000)?,
            ObservedAtUnixMillis::new(unix_millis()?),
        )?,
        WorkerSessionState::Disconnected { .. }
    ));

    assert!(matches!(
        recover_worker_session(
            &events,
            &content,
            disabled_identity.worker_id,
            WorkerSessionTimeoutMillis::new(10_000)?,
            ObservedAtUnixMillis::new(unix_millis()?),
        )?,
        WorkerSessionState::NotFound
    ));
    let worker_history = events.read_stream(
        &StreamId {
            kind: AggregateKind::new("execution-worker")?,
            id: AggregateId::new(identity.worker_id.to_string())?,
        },
        None,
    )?;
    assert_eq!(
        worker_history
            .iter()
            .filter(|event| {
                matches!(
                    event.schema_name.as_str(),
                    "execution.worker-registered" | "execution.worker-replaced-after-expiry"
                )
            })
            .count(),
        registration_count_before_revocation,
        "revoked automatic reconnect must not append another registration"
    );

    worker.abort();
    disabled_worker.abort();
    reassigned_worker.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
    server_b.abort();
    server_a.abort();
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "test configuration keeps every filesystem and listener authority explicit"
)]
fn server_config(
    listen: std::net::SocketAddr,
    enrollment_listen: std::net::SocketAddr,
    ca: &Path,
    ca_key: &Path,
    server_certificate: &Path,
    server_key: &Path,
    event_database: &Path,
    content_database: &Path,
    content_directory: &Path,
) -> Result<ServerConfig, Box<dyn Error + Send + Sync>> {
    Ok(ServerConfig {
        schema_version: 1,
        listen,
        tls: ServerTlsFiles {
            certificate: server_certificate.to_path_buf(),
            private_key: server_key.to_path_buf(),
            client_ca: ca.to_path_buf(),
        },
        enrollment_service: Some(EnrollmentServiceConfig {
            listen: enrollment_listen,
            public_tcp_address: enrollment_listen.to_string(),
            websocket_uri: format!("wss://localhost:{}/enrollment", enrollment_listen.port()),
            server_name: "localhost".into(),
            server_ca: ca.to_path_buf(),
            server_tls: cairn_server::EnrollmentServerTlsFiles {
                certificate: server_certificate.to_path_buf(),
                private_key: server_key.to_path_buf(),
            },
            control_endpoint: cairn_server::PublicWorkerControlEndpointConfig {
                tcp_address: listen.to_string(),
                websocket_uri: format!("wss://localhost:{}/control", listen.port()),
                server_name: "localhost".into(),
                server_ca: ca.to_path_buf(),
            },
            issuer_certificate: ca.to_path_buf(),
            issuer_private_key: ca_key.to_path_buf(),
            credential_validity_ms: NonZeroU64::new(3_600_000).expect("validity"),
            rotation_overlap_ms: NonZeroU64::new(500),
            handshake_timeout_ms: NonZeroU64::new(2_000),
            diagnostic_byte_limit: NonZeroU64::new(256),
            transport: TransportPolicy::default(),
        }),
        storage: ServerStorageConfig {
            event_database: event_database.to_path_buf(),
            content_database: content_database.to_path_buf(),
            content_directory: content_directory.to_path_buf(),
        },
        candidate_workflow_manager: None,
        protocol_version: WorkerProtocolVersion::new(1)?,
        session_timeout_ms: WorkerSessionTimeoutMillis::new(10_000)?,
        scheduler: None,
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: None,
        outbox_poll_interval_ms: None,
        authority_poll_interval_ms: NonZeroU64::new(25).expect("authority poll"),
        resource_clock_skew_tolerance_ms: None,
        transport: TransportPolicy::default(),
        diagnostic_byte_limit: NonZeroU64::new(256),
    })
}

fn worker_config(
    control: std::net::SocketAddr,
    state_directory: &std::path::Path,
) -> Result<WorkerConfig, Box<dyn Error + Send + Sync>> {
    let journal_database = state_directory.join("worker-journal.sqlite3");
    Ok(WorkerConfig {
        schema_version: 1,
        controller: ControllerEndpoint {
            tcp_address: control.to_string(),
            websocket_uri: format!("wss://localhost:{}/control", control.port()),
        },
        identity: WorkerIdentityConfig::Managed {
            state_directory: state_directory.to_path_buf(),
        },
        profile: WorkerProfileConfig {
            schema_version: 1,
            protocol_version: WorkerProtocolVersion::new(1)?,
            binary_identity: WorkerBinaryIdentity::new("sha256:enrollment-test")?,
            backends: vec![ExecutionBackend::new("transport-only")?],
            capabilities: Vec::new(),
            max_concurrency: WorkerSlotCount::new(1)?,
        },
        expected_platform: ExecutionPlatformRequirement::default(),
        resource_probe: ResourceProbeConfig {
            scratch_path: journal_database
                .parent()
                .expect("journal parent")
                .to_path_buf(),
            accelerator_sysfs: None,
            freshness_ms: None,
            refresh_interval_ms: None,
            expected: ExpectedResourceConstraints::default(),
        },
        availability: WorkerAvailability::new(WorkerHealth::Unavailable, true, 0, Vec::new())?,
        journal_database,
        content: cairn_worker::WorkerContentConfig {
            database: state_directory.join("content.sqlite3"),
            directory: state_directory.join("content"),
            transfer_directory: state_directory.join("transfers"),
            assignment_material_byte_limit: None,
            assignment_material_chunk_size: cairn_execution::AssignmentMaterialChunkSize::new(
                16 * 1024,
            )?,
        },
        execution: WorkerExecutionConfig::Disabled,
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: None,
        heartbeat_interval_ms: NonZeroU64::new(50),
        identity_poll_interval_ms: NonZeroU64::new(25).expect("identity poll"),
        reconnect_delay_ms: NonZeroU64::new(25),
        transport: TransportPolicy::default(),
    })
}

fn free_address() -> Result<std::net::SocketAddr, Box<dyn Error + Send + Sync>> {
    let listener = StdTcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    Ok(address)
}

fn unix_millis() -> Result<i64, Box<dyn Error + Send + Sync>> {
    let duration = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_millis())?)
}

struct TestPki {
    ca_certificate: String,
    ca_private_key: String,
    server_certificate: String,
    server_private_key: String,
}

fn test_pki() -> Result<TestPki, Box<dyn Error + Send + Sync>> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Cairn enrollment test CA");
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca = ca_params.self_signed(&ca_key)?;
    let server_key = KeyPair::generate()?;
    let mut server_params = CertificateParams::new(vec!["localhost".into()])?;
    server_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server = server_params.signed_by(&server_key, &ca, &ca_key)?;
    Ok(TestPki {
        ca_certificate: ca.pem(),
        ca_private_key: ca_key.serialize_pem(),
        server_certificate: server.pem(),
        server_private_key: server_key.serialize_pem(),
    })
}
