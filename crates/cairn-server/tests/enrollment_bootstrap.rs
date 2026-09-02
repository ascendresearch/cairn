use std::{
    error::Error,
    fs,
    net::TcpListener as StdTcpListener,
    num::NonZeroU64,
    path::{Path, PathBuf},
    time::Duration,
};

use cairn_control_transport::{ClientTlsFiles, EnrollmentSecret};
use cairn_execution::{
    AcceleratorDiscoveryCompleteness, ArchitectureName, AuthenticatedWorkerIdentity,
    ExecutionBackend, ExecutionPlatform, LogicalCpuCount, MemoryByteCount, OperatingSystemName,
    RecordedWorkerAuthenticator, ResourceProbeVersion, ScratchByteCount, SessionEndReason,
    TargetEnvironmentName, WorkerAuthenticationSubject, WorkerBinaryIdentity, WorkerHealth,
    WorkerHello, WorkerPoolName, WorkerProfile, WorkerProtocolVersion, WorkerResourceClaim,
    WorkerResourceInventory, WorkerResourceObservation, WorkerResourceSource, WorkerSessionState,
    WorkerSlotCount, recover_worker_session, register_worker,
};
use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ObservedAtUnixMillis, WorkerIncarnationId,
};
use cairn_record::{EventStore, StreamId};
use cairn_server::{
    ServerConfig, assign_enrolled_worker_pool, create_enrollment_bundle, create_rotation_bundle,
    disable_enrolled_worker, enable_enrolled_worker, revoke_enrollment_authority,
    revoke_worker_credential,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_worker::{
    WorkerConfig, WorkerIdentityConfig, enroll, join_from_bundle, rollback_rotation, rotate,
};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};

static ENROLLMENT_INTEGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_command_join_persists_and_reuses_a_runnable_worker_tree()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let _integration_guard = ENROLLMENT_INTEGRATION_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let home = directory.path().join("deployment");
    prepare_deployment(&home, &pki)?;
    let enrollment_pki = test_pki()?;
    fs::write(
        home.join("secrets/enrollment-ca.pem"),
        &enrollment_pki.ca_certificate,
    )?;
    fs::write(
        home.join("secrets/enrollment-server.pem"),
        &enrollment_pki.server_certificate,
    )?;
    fs::write(
        home.join("secrets/enrollment-server-key.pem"),
        &enrollment_pki.server_private_key,
    )?;
    let control = free_address()?;
    let enrollment = free_address()?;
    let event_database = home.join("store/events.sqlite3");
    let content_database = home.join("store/content.sqlite3");
    let content_directory = home.join("store/content");
    let config = server_config(control, enrollment, &home, Some("enrollment"))?;
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
    // One command produces a deployment, not a pile of files: identity lands in the secret tree
    // and everything this worker writes lands in the store tree.
    assert!(state.join("secrets/identity/worker-key.pem").is_file());
    assert!(state.join("secrets/identity/identity.json").is_file());
    assert!(state.join("store/scratch").is_dir());
    assert!(state.join("store/content.sqlite3").is_file());
    assert!(state.join("store/content").is_dir());
    assert!(state.join("store/transfers").is_dir());
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
    let home = directory.path().join("deployment");
    prepare_deployment(&home, &pki)?;

    let control_a = free_address()?;
    let enrollment_a = free_address()?;
    let event_database = home.join("store/events.sqlite3");
    let content_database = home.join("store/content.sqlite3");
    let content_directory = home.join("store/content");
    let config_a = server_config(control_a, enrollment_a, &home, None)?;
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

    let state_home = directory.path().join("managed-worker");
    let state = prepare_worker_home(&state_home)?;
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
    let disabled_home = directory.path().join("disabled-worker");
    let disabled_state = prepare_worker_home(&disabled_home)?;
    let disabled_identity = Box::pin(enroll(disabled_bundle, &disabled_state)).await?;
    let reassigned_bundle = create_enrollment_bundle(
        &issuance_config,
        pool.clone(),
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    let reassigned_home = directory.path().join("reassigned-worker");
    let reassigned_state = prepare_worker_home(&reassigned_home)?;
    let reassigned_identity = Box::pin(enroll(reassigned_bundle, &reassigned_state)).await?;

    // A fresh controller instance reconstructs certificate -> stable WorkerId/pool authorization
    // solely from the durable enrollment stream.
    let control_b = free_address()?;
    let enrollment_b = free_address()?;
    let config_b = server_config(control_b, enrollment_b, &home, None)?;
    let authority_config = config_b.clone();
    let server_b = tokio::spawn(cairn_server::run(config_b));
    tokio::time::sleep(Duration::from_millis(50)).await;
    disable_enrolled_worker(
        &authority_config,
        disabled_identity.worker_id,
        &CommandId::new(),
    )?;
    let mut disabled_worker_config = worker_config(control_b, &disabled_home)?;
    disabled_worker_config.reconnect_delay_ms = None;
    let disabled_worker = tokio::spawn(cairn_worker::run(disabled_worker_config));
    let worker = tokio::spawn(cairn_worker::run(worker_config(control_b, &state_home)?));
    let reassigned_worker = tokio::spawn(cairn_worker::run(worker_config(
        control_b,
        &reassigned_home,
    )?));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let session = recover_worker_session(
        &events,
        &content,
        identity.worker_id,
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
    let mut retired_config = worker_config(control_b, &state_home)?;
    retired_config.identity = WorkerIdentityConfig::External {
        worker_id: identity.worker_id,
        tls: ClientTlsFiles {
            certificate: state.join(&identity.tls.certificate),
            private_key: state.join(&identity.tls.private_key),
            server_ca: state.join(&identity.tls.server_ca),
            server_name: identity.tls.server_name.clone(),
        },
    };
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
            ObservedAtUnixMillis::new(unix_millis()?),
        )?,
        WorkerSessionState::Disconnected { .. }
    ));

    assert!(matches!(
        recover_worker_session(
            &events,
            &content,
            disabled_identity.worker_id,
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

// A controller that dies without unwinding never records the end of the sessions it was holding,
// so the durable log still calls them live. Nothing in the log can distinguish that from a worker
// that is genuinely still there, and the next controller must not inherit the ambiguity: it holds
// no socket for any of them, so it closes the book on all of them before it serves anything.
// Without that, an enrolled worker could never register again after a crash.
//
// The crash is staged by writing the open session directly rather than by aborting a server task.
// The listeners run on detached tasks, so aborting the task returned by `run` leaves the session
// loop alive, and it then records a perfectly ordinary end when the worker goes away, which is the
// one thing this test must not observe.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_starting_controller_records_the_end_of_sessions_left_open_by_a_previous_process()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let _integration_guard = ENROLLMENT_INTEGRATION_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let pki = test_pki()?;
    let home = directory.path().join("deployment");
    prepare_deployment(&home, &pki)?;
    let event_database = home.join("store/events.sqlite3");
    let content_database = home.join("store/content.sqlite3");
    let content_directory = home.join("store/content");

    let config = server_config(free_address()?, free_address()?, &home, None)?;
    let bundle = create_enrollment_bundle(
        &config,
        WorkerPoolName::new("crash-lab")?,
        NonZeroU64::new(60_000).expect("TTL"),
    )?;
    let enrolling = tokio::spawn(cairn_server::run(config.clone()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let state = directory.path().join("worker-state");
    let identity = Box::pin(enroll(bundle, &state)).await?;
    enrolling.abort();

    let orphaned_incarnation = stage_open_worker_session(
        &event_database,
        &content_database,
        &content_directory,
        identity.worker_id,
        identity.credential_id,
    )?;
    {
        let events = SqliteEventStore::open(&event_database)?;
        let content = SqliteContentStore::open(&content_database, &content_directory)?;
        assert!(
            matches!(
                recover_worker_session(
                    &events,
                    &content,
                    identity.worker_id,
                    ObservedAtUnixMillis::new(unix_millis()?),
                )?,
                WorkerSessionState::Live(_)
            ),
            "the staged session must read as live, or this test proves nothing"
        );
    }

    let restarted = tokio::spawn(cairn_server::run(server_config(
        free_address()?,
        free_address()?,
        &home,
        None,
    )?));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let events = SqliteEventStore::open(&event_database)?;
    let content = SqliteContentStore::open(&content_database, &content_directory)?;
    let WorkerSessionState::Disconnected {
        incarnation_id,
        reason,
    } = recover_worker_session(
        &events,
        &content,
        identity.worker_id,
        ObservedAtUnixMillis::new(unix_millis()?),
    )?
    else {
        return Err(
            "a controller that started over an open session left it open, so the worker holding \
             that identity could never register again"
                .into(),
        );
    };
    assert_eq!(incarnation_id, orphaned_incarnation);
    assert_eq!(reason, SessionEndReason::Lapsed);

    restarted.abort();
    Ok(())
}

/// Writes one worker session that is opened and never closed, exactly as a controller that died
/// mid-flight would leave it.
fn stage_open_worker_session(
    event_database: &Path,
    content_database: &Path,
    content_directory: &Path,
    worker_id: cairn_protocol::WorkerId,
    credential_id: cairn_protocol::CredentialId,
) -> Result<WorkerIncarnationId, Box<dyn Error + Send + Sync>> {
    let mut events = SqliteEventStore::open(event_database)?;
    let mut content = SqliteContentStore::open(content_database, content_directory)?;
    let profile = WorkerProfile::new(
        WorkerProtocolVersion::new(1)?,
        WorkerBinaryIdentity::new("sha256:orphaned-session-fixture")?,
        WorkerResourceInventory::new(
            WorkerResourceClaim::new(
                ExecutionPlatform::new(
                    ArchitectureName::new("x86_64")?,
                    OperatingSystemName::new("linux")?,
                    TargetEnvironmentName::new("gnu")?,
                ),
                WorkerResourceSource::BuiltinProbe,
            ),
            vec![WorkerResourceClaim::new(
                ExecutionBackend::new("transport-only")?,
                WorkerResourceSource::OperatorDeclared,
            )],
            Vec::new(),
            WorkerResourceObservation::new(
                WorkerResourceSource::BuiltinProbe,
                ResourceProbeVersion::new("fixture-probe-v1")?,
                ObservedAtUnixMillis::new(unix_millis()?),
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
            WorkerAuthenticationSubject::new(worker_id.to_string())?,
            credential_id,
            WorkerPoolName::new("crash-lab")?,
        ),
    )]);
    let session = register_worker(
        &mut events,
        &mut content,
        &mut authenticator,
        &hello,
        &CommandId::new(),
        ObservedAtUnixMillis::new(unix_millis()?),
    )?;
    Ok(session.incarnation_id())
}

/// Lays out one bundled deployment under `home` and writes its PKI into the secret tree.
fn prepare_deployment(home: &Path, pki: &TestPki) -> Result<(), Box<dyn Error + Send + Sync>> {
    fs::create_dir_all(home.join("secrets"))?;
    fs::create_dir_all(home.join("store"))?;
    fs::write(home.join("secrets/ca.pem"), &pki.ca_certificate)?;
    fs::write(home.join("secrets/ca-key.pem"), &pki.ca_private_key)?;
    fs::write(home.join("secrets/server.pem"), &pki.server_certificate)?;
    fs::write(home.join("secrets/server-key.pem"), &pki.server_private_key)?;
    Ok(())
}

/// Builds one controller configuration bound to the deployment rooted at `home`.
///
/// `enrollment_pki` names a separate certificate authority for the bootstrap listener, whose files
/// live beside the control PKI in the secret tree. It is a parameter rather than a field the caller
/// overrides afterwards, because paths are bound to their tree when the configuration is resolved
/// and a later assignment would leave a tree-relative name that never gets placed.
fn server_config(
    listen: std::net::SocketAddr,
    enrollment_listen: std::net::SocketAddr,
    home: &Path,
    enrollment_pki: Option<&str>,
) -> Result<ServerConfig, Box<dyn Error + Send + Sync>> {
    let enrollment_ca =
        enrollment_pki.map_or_else(|| "ca.pem".to_string(), |prefix| format!("{prefix}-ca.pem"));
    let enrollment_certificate = enrollment_pki.map_or_else(
        || "server.pem".to_string(),
        |prefix| format!("{prefix}-server.pem"),
    );
    let enrollment_key = enrollment_pki.map_or_else(
        || "server-key.pem".to_string(),
        |prefix| format!("{prefix}-server-key.pem"),
    );
    // Built by deserialization rather than by a struct literal, so these tests exercise the same
    // strict decoding a deployment does, including the rejection of unknown fields.
    let config: ServerConfig = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "listen": listen.to_string(),
        "store_root": home.join("store"),
        "workspaces_root": home.join("workspaces"),
        "tls": {
            "certificate": home.join("secrets/server.pem"),
            "private_key": home.join("secrets/server-key.pem"),
            "client_ca": home.join("secrets/ca.pem")
        },
        "enrollment_service": {
            "listen": enrollment_listen.to_string(),
            "public_tcp_address": enrollment_listen.to_string(),
            "websocket_uri": format!("wss://localhost:{}/enrollment", enrollment_listen.port()),
            "server_name": "localhost",
            "server_ca": home.join("secrets").join(&enrollment_ca),
            "server_tls": {
                "certificate": home.join("secrets").join(&enrollment_certificate),
                "private_key": home.join("secrets").join(&enrollment_key)
            },
            "control_endpoint": {
                "tcp_address": listen.to_string(),
                "websocket_uri": format!("wss://localhost:{}/control", listen.port()),
                "server_name": "localhost",
                "server_ca": home.join("secrets/ca.pem")
            },
            "issuer_certificate": home.join("secrets/ca.pem"),
            "issuer_private_key": home.join("secrets/ca-key.pem"),
            "credential_validity_ms": 3_600_000,
            "rotation_overlap_ms": 500,
            "handshake_timeout_ms": 2_000,
            "diagnostic_byte_limit": 256
        },
        "protocol_version": 1,
        "scheduler": null,
        "handshake_timeout_ms": 2_000,
        "idle_timeout_ms": null,
        "outbox_poll_interval_ms": null,
        "authority_poll_interval_ms": 25,
        "diagnostic_byte_limit": 256
    }))?;
    Ok(config)
}

/// Lays out one worker deployment under `home` and returns its identity directory.
fn prepare_worker_home(home: &Path) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    fs::create_dir_all(home.join("secrets"))?;
    fs::create_dir_all(home.join("store/scratch"))?;
    Ok(home.join("secrets/identity"))
}

fn worker_config(
    control: std::net::SocketAddr,
    home: &Path,
) -> Result<WorkerConfig, Box<dyn Error + Send + Sync>> {
    let config: WorkerConfig = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "controller": {
            "tcp_address": control.to_string(),
            "websocket_uri": format!("wss://localhost:{}/control", control.port())
        },
        "store_root": home.join("store"),
        "identity": { "mode": "managed", "state_directory": home.join("secrets/identity") },
        "profile": {
            "schema_version": 1,
            "protocol_version": 1,
            "binary_identity": "sha256:enrollment-test",
            "backends": ["transport-only"],
            "capabilities": [],
            "max_concurrency": 1
        },
        "resource_probe": {
            "scratch_path": home.join("store/scratch"),
            "accelerator_sysfs": null,
            "freshness_ms": null,
            "refresh_interval_ms": null,
            "expected": {}
        },
        "availability": {
            "health": "unavailable",
            "draining": true,
            "available_slots": 0,
            "active_attempts": []
        },
        "content": {
            "assignment_material_byte_limit": null,
            "assignment_material_chunk_size": 16_384
        },
        "execution": { "mode": "disabled" },
        "handshake_timeout_ms": 2_000,
        "idle_timeout_ms": null,
        "heartbeat_interval_ms": 50,
        "identity_poll_interval_ms": 25,
        "reconnect_delay_ms": 25
    }))?;
    Ok(config)
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
