//! Runnable Cairn controller composition root.

mod enrollment;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Write,
    net::SocketAddr,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cairn_control_transport::{
    CertificateFingerprint, ControllerRejectCode, ControllerWireMessage, EnrollmentBundle,
    EnrollmentRejectCode, EnrollmentRequest, EnrollmentResponse, ServerTlsFiles, TransportPolicy,
    WorkerWireMessage, accept_enrollment_socket, accept_worker_socket, read_wire_message,
    write_wire_message,
};
use cairn_execution::{
    AuthenticatedWorkerIdentity, ControlFrame, ExecutionAssignmentState, InboundControlSession,
    RecordedWorkerAuthenticator, RegisteredWorkerSession, WorkerAuthenticationSubject,
    WorkerControlMessage, WorkerPoolName, WorkerProtocolVersion, WorkerResultReconciliation,
    WorkerSessionTimeoutMillis, accept_worker_assignment, acknowledge_controller_messages,
    deliver_controller_acknowledgement, deliver_controller_messages, disconnect_worker,
    reconcile_worker_result, record_worker_heartbeat, recover_execution_assignment,
    register_worker,
};
use cairn_protocol::{
    CommandId, ControlConnectionId, ControlSequence, ObservedAtUnixMillis, WorkerId,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    sync::{Mutex, RwLock},
    time::Instant,
};

use enrollment::{EnrollmentError, EnrollmentIssuer, EnrollmentRegistry, create_offer, redeem};

/// Strict controller process configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub schema_version: u16,
    pub listen: SocketAddr,
    pub tls: ServerTlsFiles,
    pub enrollment: Vec<WorkerEnrollment>,
    pub enrollment_service: Option<EnrollmentServiceConfig>,
    pub storage: ServerStorageConfig,
    pub protocol_version: WorkerProtocolVersion,
    pub session_timeout_ms: WorkerSessionTimeoutMillis,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub idle_timeout_ms: Option<NonZeroU64>,
    pub outbox_poll_interval_ms: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
    pub diagnostic_byte_limit: Option<NonZeroU64>,
}

/// Isolated server-authenticated listener and certificate authority used only for bootstrap.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentServiceConfig {
    pub listen: SocketAddr,
    pub public_tcp_address: String,
    pub websocket_uri: String,
    pub server_name: String,
    pub server_ca: PathBuf,
    pub issuer_certificate: PathBuf,
    pub issuer_private_key: PathBuf,
    pub credential_validity_ms: NonZeroU64,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub diagnostic_byte_limit: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
}

/// Static V1 binding from a logical worker ID to one exact leaf certificate.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerEnrollment {
    pub worker_id: WorkerId,
    pub pool: WorkerPoolName,
    pub certificate: PathBuf,
}

#[derive(Clone)]
pub(crate) struct EnrolledWorker {
    pub(crate) worker_id: WorkerId,
    pub(crate) pool: WorkerPoolName,
}

/// Controller durable storage locations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerStorageConfig {
    pub event_database: PathBuf,
    pub content_database: PathBuf,
    pub content_directory: PathBuf,
}

/// Configuration, transport, or durable-domain process failure.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error(
        "usage: cairn-server <config.json> | cairn-server enrollment create <config.json> <pool> <ttl-ms> <bundle.json>"
    )]
    Usage,
    #[error("controller configuration failed: {0}")]
    Configuration(String),
    #[error("controller startup failed: {0}")]
    Startup(String),
    #[error("worker session failed: {0}")]
    Session(String),
}

struct ControllerState {
    events: SqliteEventStore,
    content: SqliteContentStore,
}

/// Loads a single JSON configuration argument and runs until process shutdown.
///
/// # Errors
///
/// Returns an error for invalid arguments/configuration or controller startup failure.
pub async fn run_from_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(), ServerError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let first = arguments.next().ok_or(ServerError::Usage)?;
    if first == "enrollment" {
        let action = arguments.next().ok_or(ServerError::Usage)?;
        if action != "create" {
            return Err(ServerError::Usage);
        }
        let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let pool = WorkerPoolName::new(
            arguments
                .next()
                .ok_or(ServerError::Usage)?
                .into_string()
                .map_err(|_| ServerError::Usage)?,
        )
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let ttl_ms = arguments
            .next()
            .ok_or(ServerError::Usage)?
            .into_string()
            .map_err(|_| ServerError::Usage)?
            .parse::<u64>()
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(ServerError::Usage)?;
        let output_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        let config = load_config(&config_path)?;
        let bundle = create_enrollment_bundle(&config, pool, ttl_ms)?;
        let bytes = serde_json::to_vec_pretty(&bundle)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        write_new_secret_file(&output_path, &bytes)?;
        eprintln!("wrote enrollment bundle to {}", output_path.display());
        return Ok(());
    }
    let config_path = PathBuf::from(first);
    if arguments.next().is_some() {
        return Err(ServerError::Usage);
    }
    run(load_config(&config_path)?).await
}

fn write_new_secret_file(path: &Path, bytes: &[u8]) -> Result<(), ServerError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| ServerError::Configuration(error.to_string()))
}

fn load_config(config_path: &Path) -> Result<ServerConfig, ServerError> {
    let mut config: ServerConfig = serde_json::from_slice(
        &std::fs::read(config_path)
            .map_err(|error| ServerError::Configuration(error.to_string()))?,
    )
    .map_err(|error| ServerError::Configuration(error.to_string()))?;
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    config.resolve_paths(base);
    Ok(config)
}

/// Creates and durably records a one-shot enrollment bundle. The secret is returned only here.
///
/// # Errors
///
/// Returns an error for invalid configuration, storage, entropy, or time.
pub fn create_enrollment_bundle(
    config: &ServerConfig,
    pool: WorkerPoolName,
    ttl_ms: NonZeroU64,
) -> Result<EnrollmentBundle, ServerError> {
    config.validate()?;
    let service = config
        .enrollment_service
        .as_ref()
        .ok_or_else(|| ServerError::Configuration("enrollment_service is not configured".into()))?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    create_offer(&mut events, service, pool, ttl_ms, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Runs the authenticated controller listener.
///
/// # Errors
///
/// Returns an error for invalid configuration, TLS/storage startup, or listener failure.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    config.validate()?;
    let tls = config
        .tls
        .load()
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let registry = EnrollmentRegistry::load(&events)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let mut enrolled = config.enrollments()?;
    for (fingerprint, worker) in registry.enrolled() {
        if enrolled.insert(*fingerprint, worker.clone()).is_some() {
            return Err(ServerError::Startup(
                "static and issued enrollments contain the same certificate".into(),
            ));
        }
    }
    let enrollments = Arc::new(RwLock::new(enrolled));
    let state = Arc::new(Mutex::new(ControllerState {
        events,
        content: SqliteContentStore::open(
            &config.storage.content_database,
            &config.storage.content_directory,
        )
        .map_err(|error| ServerError::Startup(error.to_string()))?,
    }));
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    if let Some(service) = config.enrollment_service.clone() {
        let enrollment_tls = config
            .tls
            .load_enrollment()
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let issuer = Arc::new(
            EnrollmentIssuer::load(&service)
                .map_err(|error| ServerError::Startup(error.to_string()))?,
        );
        let enrollment_listener = TcpListener::bind(service.listen)
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let enrollment_state = Arc::clone(&state);
        let issued_enrollments = Arc::clone(&enrollments);
        let enrollment_config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = enrollment_listener_loop(
                enrollment_listener,
                enrollment_tls,
                enrollment_state,
                issued_enrollments,
                issuer,
                enrollment_config,
            )
            .await
            {
                eprintln!("cairn-server enrollment listener: {error}");
            }
        });
    }
    eprintln!(
        "cairn-server listening on {}",
        listener
            .local_addr()
            .map_err(|e| ServerError::Startup(e.to_string()))?
    );
    loop {
        let (tcp, _) = listener
            .accept()
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let session_config = config.clone();
        let session_tls = Arc::clone(&tls);
        let session_state = Arc::clone(&state);
        let session_enrollments = Arc::clone(&enrollments);
        tokio::spawn(async move {
            if let Err(error) = Box::pin(handle_connection(
                tcp,
                session_tls,
                session_state,
                session_enrollments,
                session_config,
            ))
            .await
            {
                eprintln!("cairn-server worker connection: {error}");
            }
        });
    }
}

impl ServerConfig {
    fn validate(&self) -> Result<(), ServerError> {
        if self.schema_version != 1 {
            return Err(ServerError::Configuration(
                "only server schema_version 1 is supported".into(),
            ));
        }
        if self.enrollment.is_empty() && self.enrollment_service.is_none() {
            return Err(ServerError::Configuration(
                "at least one static enrollment or enrollment_service is required".into(),
            ));
        }
        if let Some(service) = &self.enrollment_service {
            if service.listen == self.listen
                || service.public_tcp_address.is_empty()
                || service.websocket_uri.is_empty()
                || service.server_name.is_empty()
            {
                return Err(ServerError::Configuration(
                    "enrollment service must use a distinct listener and non-empty public endpoint"
                        .into(),
                ));
            }
            let trusted = CertificateFingerprint::from_pem_file(&self.tls.client_ca)
                .map_err(|error| ServerError::Configuration(error.to_string()))?;
            let issuer = CertificateFingerprint::from_pem_file(&service.issuer_certificate)
                .map_err(|error| ServerError::Configuration(error.to_string()))?;
            if trusted != issuer {
                return Err(ServerError::Configuration(
                    "credential issuer must be the client CA trusted by the control listener"
                        .into(),
                ));
            }
        }
        Ok(())
    }

    fn resolve_paths(&mut self, base: &Path) {
        resolve(&mut self.tls.certificate, base);
        resolve(&mut self.tls.private_key, base);
        resolve(&mut self.tls.client_ca, base);
        for enrollment in &mut self.enrollment {
            resolve(&mut enrollment.certificate, base);
        }
        if let Some(service) = &mut self.enrollment_service {
            resolve(&mut service.server_ca, base);
            resolve(&mut service.issuer_certificate, base);
            resolve(&mut service.issuer_private_key, base);
        }
        resolve(&mut self.storage.event_database, base);
        resolve(&mut self.storage.content_database, base);
        resolve(&mut self.storage.content_directory, base);
    }

    fn enrollments(&self) -> Result<BTreeMap<CertificateFingerprint, EnrolledWorker>, ServerError> {
        let mut result = BTreeMap::new();
        let mut workers = BTreeMap::new();
        for enrollment in &self.enrollment {
            let fingerprint = CertificateFingerprint::from_pem_file(&enrollment.certificate)
                .map_err(|error| ServerError::Configuration(error.to_string()))?;
            if result
                .insert(
                    fingerprint,
                    EnrolledWorker {
                        worker_id: enrollment.worker_id,
                        pool: enrollment.pool.clone(),
                    },
                )
                .is_some()
            {
                return Err(ServerError::Configuration(
                    "one certificate is enrolled to more than one worker".into(),
                ));
            }
            if workers.insert(enrollment.worker_id, fingerprint).is_some() {
                return Err(ServerError::Configuration(format!(
                    "worker {} has more than one V1 certificate",
                    enrollment.worker_id
                )));
            }
        }
        Ok(result)
    }
}

async fn enrollment_listener_loop(
    listener: TcpListener,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<Mutex<ControllerState>>,
    enrollments: Arc<RwLock<BTreeMap<CertificateFingerprint, EnrolledWorker>>>,
    issuer: Arc<EnrollmentIssuer>,
    config: ServerConfig,
) -> Result<(), ServerError> {
    eprintln!(
        "cairn-server enrollment listening on {}",
        listener
            .local_addr()
            .map_err(|error| ServerError::Startup(error.to_string()))?
    );
    loop {
        let (tcp, _) = listener
            .accept()
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let connection_tls = Arc::clone(&tls);
        let connection_state = Arc::clone(&state);
        let connection_enrollments = Arc::clone(&enrollments);
        let connection_issuer = Arc::clone(&issuer);
        let connection_config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = Box::pin(handle_enrollment_connection(
                tcp,
                connection_tls,
                connection_state,
                connection_enrollments,
                connection_issuer,
                connection_config,
            ))
            .await
            {
                eprintln!("cairn-server enrollment connection: {error}");
            }
        });
    }
}

async fn handle_enrollment_connection(
    tcp: tokio::net::TcpStream,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<Mutex<ControllerState>>,
    enrollments: Arc<RwLock<BTreeMap<CertificateFingerprint, EnrolledWorker>>>,
    issuer: Arc<EnrollmentIssuer>,
    config: ServerConfig,
) -> Result<(), ServerError> {
    let service = config
        .enrollment_service
        .as_ref()
        .ok_or_else(|| ServerError::Session("enrollment service configuration is absent".into()))?;
    let accepted = accept_enrollment_socket(tcp, tls, service.transport);
    let (mut socket, _peer) = Box::pin(timeout_optional(service.handshake_timeout_ms, accepted))
        .await?
        .map_err(|error| ServerError::Session(error.to_string()))?;
    let request = timeout_optional(
        service.handshake_timeout_ms,
        read_wire_message::<_, EnrollmentRequest>(&mut socket, service.transport),
    )
    .await?;
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            write_enrollment_reject(
                &mut socket,
                service,
                EnrollmentRejectCode::InvalidRequest,
                &error.to_string(),
            )
            .await;
            return Err(ServerError::Session("invalid enrollment request".into()));
        }
    };
    let result = {
        let mut locked = state.lock().await;
        redeem(
            &mut locked.events,
            issuer.as_ref(),
            &request,
            observed_now()?,
        )
    };
    let credential = match result {
        Ok(credential) => credential,
        Err(error) => {
            let (code, diagnostic) = enrollment_rejection(&error);
            write_enrollment_reject(&mut socket, service, code, diagnostic).await;
            return Err(ServerError::Session(error.to_string()));
        }
    };
    let fingerprint = CertificateFingerprint::from_pem(&credential.certificate_chain_pem)
        .map_err(|error| ServerError::Session(error.to_string()))?;
    let enrolled = EnrolledWorker {
        worker_id: credential.worker_id,
        pool: credential.pool.clone(),
    };
    {
        let mut index = enrollments.write().await;
        if let Some(existing) = index.insert(fingerprint, enrolled.clone()) {
            if existing.worker_id != enrolled.worker_id || existing.pool != enrolled.pool {
                return Err(ServerError::Session(
                    "issued fingerprint collides with another enrollment".into(),
                ));
            }
        }
    }
    write_wire_message(
        &mut socket,
        &EnrollmentResponse::Issued { credential },
        service.transport,
    )
    .await
    .map_err(|error| ServerError::Session(error.to_string()))
}

fn enrollment_rejection(error: &EnrollmentError) -> (EnrollmentRejectCode, &'static str) {
    match error {
        EnrollmentError::InvalidAuthority => (
            EnrollmentRejectCode::InvalidAuthority,
            "enrollment authority is invalid",
        ),
        EnrollmentError::Expired => (
            EnrollmentRejectCode::Expired,
            "enrollment authority has expired",
        ),
        EnrollmentError::AlreadyUsed => (
            EnrollmentRejectCode::AlreadyUsed,
            "enrollment authority was already used",
        ),
        EnrollmentError::InvalidRequest(_) => (
            EnrollmentRejectCode::InvalidRequest,
            "enrollment request is invalid",
        ),
        EnrollmentError::Storage(_)
        | EnrollmentError::InvalidHistory(_)
        | EnrollmentError::Issuance(_) => (
            EnrollmentRejectCode::ControllerUnavailable,
            "controller could not durably issue a credential",
        ),
    }
}

async fn write_enrollment_reject(
    socket: &mut cairn_control_transport::ServerWebSocket,
    config: &EnrollmentServiceConfig,
    code: EnrollmentRejectCode,
    diagnostic: &str,
) {
    let _ = write_wire_message(
        socket,
        &EnrollmentResponse::Reject {
            code,
            diagnostic: bound(diagnostic, config.diagnostic_byte_limit),
        },
        config.transport,
    )
    .await;
}

#[expect(
    clippy::too_many_lines,
    reason = "the authenticated handshake is intentionally linear"
)]
async fn handle_connection(
    tcp: tokio::net::TcpStream,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<Mutex<ControllerState>>,
    enrollments: Arc<RwLock<BTreeMap<CertificateFingerprint, EnrolledWorker>>>,
    config: ServerConfig,
) -> Result<(), ServerError> {
    let accepted = accept_worker_socket(tcp, tls, config.transport);
    let (mut socket, fingerprint, _peer) =
        Box::pin(timeout_optional(config.handshake_timeout_ms, accepted))
            .await?
            .map_err(|error| ServerError::Session(error.to_string()))?;
    let hello_message = timeout_optional(
        config.handshake_timeout_ms,
        read_wire_message::<_, WorkerWireMessage>(&mut socket, config.transport),
    )
    .await?;
    let Ok(WorkerWireMessage::Hello {
        hello,
        availability,
    }) = hello_message
    else {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::InvalidHello,
            "the first message must be a valid hello",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session("invalid initial hello".into()));
    };
    let enrolled_worker = enrollments.read().await.get(&fingerprint).cloned();
    let Some(enrolled_worker) = enrolled_worker else {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::IdentityMismatch,
            "client certificate is not enrolled",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session(
            "client certificate is not enrolled".into(),
        ));
    };
    if enrolled_worker.worker_id != hello.worker_id() {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::IdentityMismatch,
            "certificate enrollment does not match worker_id",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session(
            "certificate and worker identity differ".into(),
        ));
    }
    if hello.profile().protocol_version() != config.protocol_version {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::UnsupportedProtocol,
            "worker protocol version is unsupported",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session("unsupported worker protocol".into()));
    }
    let canonical_availability = cairn_execution::WorkerAvailability::new(
        availability.health(),
        availability.draining(),
        availability.available_slots(),
        availability.active_attempts().to_vec(),
    )
    .map_err(|error| ServerError::Session(error.to_string()))?;
    if canonical_availability != availability
        || availability.available_slots() > hello.profile().max_concurrency().get()
    {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::InvalidHello,
            "hello availability is not canonical for the advertised profile",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session("hello availability is invalid".into()));
    }
    let now = observed_now()?;
    let connection_id = ControlConnectionId::new();
    let subject = WorkerAuthenticationSubject::new(fingerprint.to_string())
        .map_err(|error| ServerError::Session(error.to_string()))?;
    let mut session = {
        let mut locked = state.lock().await;
        let ControllerState { events, content } = &mut *locked;
        let mut authenticator = RecordedWorkerAuthenticator::new([(
            hello.worker_id(),
            AuthenticatedWorkerIdentity::new(subject, enrolled_worker.pool.clone()),
        )]);
        let registered = register_worker(
            events,
            content,
            &mut authenticator,
            &hello,
            config.session_timeout_ms,
            &command("register"),
            now,
        )
        .map_err(|error| ServerError::Session(error.to_string()))?;
        record_worker_heartbeat(
            events,
            content,
            &registered,
            &availability,
            &command("hello-heartbeat"),
            now,
        )
        .map_err(|error| ServerError::Session(error.to_string()))?
    };
    write_wire_message(
        &mut socket,
        &ControllerWireMessage::Welcome {
            connection_id,
            protocol_version: config.protocol_version,
            accepted_at: now,
        },
        config.transport,
    )
    .await
    .map_err(|error| ServerError::Session(error.to_string()))?;

    let mut inbound = InboundControlSession::new(config.protocol_version, connection_id);
    let mut highest_sent = None;
    let mut acknowledgement_sent = None;
    let outcome = controller_session_loop(
        &mut socket,
        &state,
        &config,
        &connection_id,
        &mut session,
        &mut inbound,
        &mut highest_sent,
        &mut acknowledgement_sent,
    )
    .await;
    let disconnect_at = observed_now()?;
    let mut locked = state.lock().await;
    disconnect_worker(
        &mut locked.events,
        &session,
        &command("disconnect"),
        disconnect_at,
    )
    .map_err(|error| ServerError::Session(error.to_string()))?;
    outcome
}

#[expect(
    clippy::too_many_arguments,
    reason = "live session state has explicit independent authorities"
)]
async fn controller_session_loop(
    socket: &mut cairn_control_transport::ServerWebSocket,
    state: &Arc<Mutex<ControllerState>>,
    config: &ServerConfig,
    connection_id: &ControlConnectionId,
    session: &mut RegisteredWorkerSession,
    inbound: &mut InboundControlSession,
    highest_sent: &mut Option<ControlSequence>,
    acknowledgement_sent: &mut Option<ControlSequence>,
) -> Result<(), ServerError> {
    let mut idle_deadline = config
        .idle_timeout_ms
        .map(|limit| Instant::now() + Duration::from_millis(limit.get()));
    loop {
        flush_controller(
            socket,
            state,
            config,
            connection_id,
            session.worker_id(),
            inbound.received_through(),
            highest_sent,
            acknowledgement_sent,
        )
        .await?;
        let read = timeout_at_optional(
            idle_deadline,
            read_wire_message::<_, WorkerWireMessage>(socket, config.transport),
        );
        let incoming = if let Some(poll) = config.outbox_poll_interval_ms {
            tokio::select! {
                message = read => Some(message),
                () = tokio::time::sleep(Duration::from_millis(poll.get())) => None,
            }
        } else {
            Some(read.await)
        };
        let Some(message) = incoming else { continue };
        let message = message
            .map_err(|error| ServerError::Session(error.to_string()))?
            .map_err(|error| ServerError::Session(error.to_string()))?;
        idle_deadline = config
            .idle_timeout_ms
            .map(|limit| Instant::now() + Duration::from_millis(limit.get()));
        match message {
            WorkerWireMessage::Heartbeat { availability } => {
                let now = observed_now()?;
                {
                    let mut locked = state.lock().await;
                    let ControllerState { events, content } = &mut *locked;
                    *session = record_worker_heartbeat(
                        events,
                        content,
                        session,
                        &availability,
                        &command("heartbeat"),
                        now,
                    )
                    .map_err(|error| ServerError::Session(error.to_string()))?;
                }
                write_wire_message(
                    socket,
                    &ControllerWireMessage::HeartbeatAccepted { accepted_at: now },
                    config.transport,
                )
                .await
                .map_err(|error| ServerError::Session(error.to_string()))?;
            }
            WorkerWireMessage::Control { frame } => {
                inbound
                    .accept(&frame, *highest_sent)
                    .map_err(|error| ServerError::Session(error.to_string()))?;
                process_worker_frame(state, config, connection_id, session, &frame).await?;
                flush_controller(
                    socket,
                    state,
                    config,
                    connection_id,
                    session.worker_id(),
                    inbound.received_through(),
                    highest_sent,
                    acknowledgement_sent,
                )
                .await?;
            }
            WorkerWireMessage::Hello { .. } => {
                return Err(ServerError::Session("hello repeated after welcome".into()));
            }
        }
    }
}

async fn process_worker_frame(
    state: &Arc<Mutex<ControllerState>>,
    config: &ServerConfig,
    connection_id: &ControlConnectionId,
    session: &RegisteredWorkerSession,
    frame: &ControlFrame<WorkerControlMessage>,
) -> Result<(), ServerError> {
    let now = observed_now()?;
    let mut locked = state.lock().await;
    let ControllerState { events, content } = &mut *locked;
    if let Some(acknowledged) = frame.acknowledges_peer_through {
        acknowledge_controller_messages(
            events,
            session.worker_id(),
            *connection_id,
            acknowledged,
            &command("controller-ack"),
            now,
        )
        .map_err(|error| ServerError::Session(error.to_string()))?;
    }
    let Some(message) = &frame.message else {
        return Ok(());
    };
    match &message.payload {
        WorkerControlMessage::AssignmentAccepted { binding } => {
            let assignment =
                recover_execution_assignment(events, content, binding.attempt_id(), now)
                    .map_err(|error| ServerError::Session(error.to_string()))?;
            match assignment {
                ExecutionAssignmentState::Leased(lease) => {
                    accept_worker_assignment(
                        events,
                        content,
                        lease,
                        session,
                        message,
                        config.session_timeout_ms,
                        &command("accept"),
                        now,
                    )
                    .map_err(|error| ServerError::Session(error.to_string()))?;
                }
                ExecutionAssignmentState::Accepted(accepted) => {
                    ensure_binding(accepted.lease().binding(), binding)?;
                }
                ExecutionAssignmentState::Running { lease }
                | ExecutionAssignmentState::ExpiredBeforeStart { lease }
                | ExecutionAssignmentState::ReconciliationRequired { lease }
                | ExecutionAssignmentState::ExecutionTerminal { lease, .. } => {
                    ensure_binding(lease.binding(), binding)?;
                }
                ExecutionAssignmentState::NotFound => {
                    return Err(ServerError::Session(
                        "assignment acceptance names no durable assignment".into(),
                    ));
                }
            }
        }
        WorkerControlMessage::ExecutionResult { .. } => {
            let _result: WorkerResultReconciliation = reconcile_worker_result(
                events,
                content,
                session.worker_id(),
                message,
                &command("result"),
                now,
            )
            .map_err(|error| ServerError::Session(error.to_string()))?;
        }
    }
    Ok(())
}

fn ensure_binding(
    expected: &cairn_execution::AssignmentBinding,
    observed: &cairn_execution::AssignmentBinding,
) -> Result<(), ServerError> {
    if expected == observed {
        Ok(())
    } else {
        Err(ServerError::Session(
            "duplicate acceptance has a conflicting assignment binding".into(),
        ))
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "durable delivery cursors are independent"
)]
async fn flush_controller(
    socket: &mut cairn_control_transport::ServerWebSocket,
    state: &Arc<Mutex<ControllerState>>,
    config: &ServerConfig,
    connection_id: &ControlConnectionId,
    worker_id: WorkerId,
    acknowledges: Option<ControlSequence>,
    highest_sent: &mut Option<ControlSequence>,
    acknowledgement_sent: &mut Option<ControlSequence>,
) -> Result<(), ServerError> {
    let now = observed_now()?;
    let frames = {
        let mut locked = state.lock().await;
        let frames = deliver_controller_messages(
            &mut locked.events,
            worker_id,
            config.protocol_version,
            *connection_id,
            acknowledges,
            &command("deliver"),
            now,
        )
        .map_err(|error| ServerError::Session(error.to_string()))?;
        let acknowledgement_only =
            acknowledges.filter(|value| frames.is_empty() && Some(*value) > *acknowledgement_sent);
        if let Some(acknowledges) = acknowledgement_only {
            vec![
                deliver_controller_acknowledgement(
                    &mut locked.events,
                    worker_id,
                    config.protocol_version,
                    *connection_id,
                    acknowledges,
                    &command("deliver-ack"),
                    now,
                )
                .map_err(|error| ServerError::Session(error.to_string()))?,
            ]
        } else {
            frames
        }
    };
    for frame in frames {
        write_wire_message(
            socket,
            &ControllerWireMessage::Control {
                frame: Box::new(frame.clone()),
            },
            config.transport,
        )
        .await
        .map_err(|error| ServerError::Session(error.to_string()))?;
        *highest_sent = Some(frame.sequence);
        if frame.acknowledges_peer_through.is_some() {
            *acknowledgement_sent = frame.acknowledges_peer_through;
        }
    }
    Ok(())
}

async fn reject(
    socket: &mut cairn_control_transport::ServerWebSocket,
    policy: TransportPolicy,
    code: ControllerRejectCode,
    diagnostic: &str,
    limit: Option<NonZeroU64>,
) {
    let diagnostic = bound(diagnostic, limit);
    let _ = write_wire_message(
        socket,
        &ControllerWireMessage::Reject { code, diagnostic },
        policy,
    )
    .await;
}

async fn timeout_optional<F, T>(limit: Option<NonZeroU64>, future: F) -> Result<T, ServerError>
where
    F: Future<Output = T>,
{
    if let Some(limit) = limit {
        tokio::time::timeout(Duration::from_millis(limit.get()), future)
            .await
            .map_err(|_| ServerError::Session("configured timeout elapsed".into()))
    } else {
        Ok(future.await)
    }
}

async fn timeout_at_optional<F, T>(deadline: Option<Instant>, future: F) -> Result<T, ServerError>
where
    F: Future<Output = T>,
{
    if let Some(deadline) = deadline {
        tokio::time::timeout_at(deadline, future)
            .await
            .map_err(|_| ServerError::Session("configured idle timeout elapsed".into()))
    } else {
        Ok(future.await)
    }
}

fn observed_now() -> Result<ObservedAtUnixMillis, ServerError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ServerError::Session(error.to_string()))?;
    let millis = i64::try_from(duration.as_millis())
        .map_err(|_| ServerError::Session("wall clock exceeds i64 milliseconds".into()))?;
    Ok(ObservedAtUnixMillis::new(millis))
}

fn command(_purpose: &str) -> CommandId {
    CommandId::new()
}

fn resolve(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
}

fn bound(value: &str, limit: Option<NonZeroU64>) -> String {
    let Some(limit) = limit.and_then(|value| usize::try_from(value.get()).ok()) else {
        return value.to_owned();
    };
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::{ServerConfig, write_new_secret_file};

    #[test]
    fn documented_configuration_is_strictly_decodable() {
        let _: ServerConfig =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("documented server configuration");
    }

    #[cfg(unix)]
    #[test]
    fn enrollment_bundle_output_is_private_and_never_overwritten() {
        use std::{fs, os::unix::fs::PermissionsExt as _};

        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("worker.enrollment.json");
        write_new_secret_file(&path, b"first").expect("first write");
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
        assert!(write_new_secret_file(&path, b"second").is_err());
        assert_eq!(fs::read(path).expect("read"), b"first");
    }
}
