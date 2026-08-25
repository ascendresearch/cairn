use std::{error::Error, fs, net::TcpListener as StdTcpListener, num::NonZeroU64, time::Duration};

use cairn_control_transport::{ClientTlsFiles, ServerTlsFiles, TransportPolicy};
use cairn_execution::{
    ExecutionBackend, ExecutionPlatformRequirement, WorkerAvailability, WorkerBinaryIdentity,
    WorkerHealth, WorkerPoolName, WorkerProtocolVersion, WorkerSessionState,
    WorkerSessionTimeoutMillis, WorkerSlotCount, recover_worker_session,
};
use cairn_protocol::{ObservedAtUnixMillis, WorkerId};
use cairn_server::{ServerConfig, ServerStorageConfig, WorkerEnrollment};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_worker::{ControllerEndpoint, WorkerConfig, WorkerProfileConfig};
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

    let port_probe = StdTcpListener::bind("127.0.0.1:0")?;
    let listen = port_probe.local_addr()?;
    drop(port_probe);
    let event_database = directory.path().join("controller-events.sqlite3");
    let content_database = directory.path().join("controller-content.sqlite3");
    let content_directory = directory.path().join("controller-content");
    let protocol = WorkerProtocolVersion::new(1)?;
    let session_timeout = WorkerSessionTimeoutMillis::new(10_000)?;
    let server = tokio::spawn(cairn_server::run(ServerConfig {
        schema_version: 1,
        listen,
        tls: ServerTlsFiles {
            certificate: server_certificate,
            private_key: server_key,
            client_ca: ca.clone(),
        },
        enrollment: vec![
            WorkerEnrollment {
                worker_id: worker_a_id,
                pool: WorkerPoolName::new("fixture").expect("pool"),
                certificate: worker_a.0.clone(),
            },
            WorkerEnrollment {
                worker_id: worker_b_id,
                pool: WorkerPoolName::new("fixture").expect("pool"),
                certificate: worker_b.0.clone(),
            },
        ],
        storage: ServerStorageConfig {
            event_database: event_database.clone(),
            content_database: content_database.clone(),
            content_directory: content_directory.clone(),
        },
        protocol_version: protocol,
        session_timeout_ms: session_timeout,
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: NonZeroU64::new(200),
        outbox_poll_interval_ms: NonZeroU64::new(25),
        transport: TransportPolicy::default(),
        diagnostic_byte_limit: NonZeroU64::new(256),
    }));
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
            if matches!(a, Ok(WorkerSessionState::Live(_)))
                && matches!(b, Ok(WorkerSessionState::Live(_)))
            {
                break;
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

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !server.is_finished() && !worker_task_a.is_finished() && !worker_task_b.is_finished(),
        "heartbeat acknowledgements must keep both idle-bounded sessions open"
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
        schema_version: 1,
        controller,
        tls: ClientTlsFiles {
            certificate: identity.0,
            private_key: identity.1,
            server_ca: ca.to_path_buf(),
            server_name: "localhost".into(),
        },
        worker_id,
        profile: WorkerProfileConfig {
            schema_version: 1,
            protocol_version: protocol,
            binary_identity: WorkerBinaryIdentity::new("sha256:transport-test")?,
            backends: vec![ExecutionBackend::new("transport-test")?],
            capabilities: Vec::new(),
            max_concurrency: WorkerSlotCount::new(1)?,
        },
        expected_platform: ExecutionPlatformRequirement::default(),
        availability: WorkerAvailability::new(WorkerHealth::Unavailable, true, 0, Vec::new())?,
        journal_database: directory.join(format!("worker-{suffix}.sqlite3")),
        handshake_timeout_ms: NonZeroU64::new(2_000),
        idle_timeout_ms: None,
        heartbeat_interval_ms: NonZeroU64::new(50),
        reconnect_delay_ms: None,
        transport: TransportPolicy::default(),
    })
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
