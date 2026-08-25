//! Runnable outbound Cairn worker composition root.

mod probe;

use std::{
    ffi::OsString,
    fs,
    future::Future,
    io::{BufReader, Write},
    num::NonZeroU64,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cairn_control_transport::{
    ClientTlsFiles, ControllerWireMessage, EnrollmentBundle, EnrollmentPurpose, EnrollmentRequest,
    EnrollmentResponse, TransportPolicy, WorkerWireMessage, connect_enrollment_socket,
    connect_worker_socket, read_wire_message, write_wire_message,
};
use cairn_execution::{
    CapabilityRequirement, ControlFrame, ControllerControlMessage, ExecutionBackend,
    ExecutionCapture, ExecutionInput, ExecutionPlatform, ExecutionPlatformRequirement, Executor,
    ExecutorError, InboundControlSession, WorkerAvailability, WorkerBinaryIdentity, WorkerHello,
    WorkerPoolName, WorkerProfile, WorkerProtocolVersion, WorkerResourceClaim,
    WorkerResourceInventory, WorkerResourceSource, WorkerSlotCount, acknowledge_worker_messages,
    active_worker_attempts, admit_worker_assignment, deliver_worker_acknowledgement,
    deliver_worker_messages, execute_worker_attempt, record_worker_execution_start,
};
use cairn_protocol::{
    CommandId, ControlConnectionId, ControlMessageId, ControlSequence, CredentialId, EnrollmentId,
    ObservedAtUnixMillis, WorkerId, WorkerIncarnationId,
};
use cairn_store_sqlite::SqliteEventStore;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose, PublicKeyData,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::{Instant, Interval};

pub use probe::{
    ExpectedResourceConstraints, HostResourceProbe, ResourceProbeConfig, ResourceProbeError,
};

/// Strict worker process configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    pub schema_version: u16,
    pub controller: ControllerEndpoint,
    pub identity: WorkerIdentityConfig,
    pub profile: WorkerProfileConfig,
    #[serde(default)]
    pub expected_platform: ExecutionPlatformRequirement,
    pub resource_probe: ResourceProbeConfig,
    pub availability: WorkerAvailability,
    pub journal_database: PathBuf,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub idle_timeout_ms: Option<NonZeroU64>,
    pub heartbeat_interval_ms: Option<NonZeroU64>,
    pub identity_poll_interval_ms: NonZeroU64,
    pub reconnect_delay_ms: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
}

/// Selects either explicitly provisioned files or one controller-issued managed state directory.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum WorkerIdentityConfig {
    External {
        worker_id: WorkerId,
        tls: ClientTlsFiles,
    },
    Managed {
        state_directory: PathBuf,
    },
}

/// Non-secret identity metadata atomically committed after successful bootstrap.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedWorkerIdentity {
    pub schema_version: u16,
    pub enrollment_id: EnrollmentId,
    pub worker_id: WorkerId,
    pub credential_id: CredentialId,
    #[serde(default)]
    pub predecessor_credential_id: Option<CredentialId>,
    #[serde(default)]
    pub predecessor_retire_at: Option<ObservedAtUnixMillis>,
    pub pool: WorkerPoolName,
    pub tls: ClientTlsFiles,
}

#[derive(Clone)]
struct ResolvedWorkerIdentity {
    worker_id: WorkerId,
    credential_id: Option<CredentialId>,
    tls: ClientTlsFiles,
}

/// Operator configuration used to construct a runtime-observed worker profile.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProfileConfig {
    pub schema_version: u16,
    pub protocol_version: WorkerProtocolVersion,
    pub binary_identity: WorkerBinaryIdentity,
    pub backends: Vec<ExecutionBackend>,
    pub capabilities: Vec<CapabilityRequirement>,
    pub max_concurrency: WorkerSlotCount,
}

/// Separates the routable TCP address from the TLS/WebSocket authority URI.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerEndpoint {
    pub tcp_address: String,
    pub websocket_uri: String,
}

/// Configuration, transport, or durable worker-journal process failure.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error(
        "usage: cairn-worker <config.json> | cairn-worker enroll <bundle.json> <state-dir> | cairn-worker rotate <bundle.json> <state-dir> | cairn-worker rollback <state-dir>"
    )]
    Usage,
    #[error("worker configuration failed: {0}")]
    Configuration(String),
    #[error("worker session failed: {0}")]
    Session(String),
}

enum Wake<T> {
    Message(T),
    Heartbeat,
    ResourceRefresh,
}

struct NotStartedExecutor;

impl Executor for NotStartedExecutor {
    fn execute(&mut self, _input: &ExecutionInput<'_>) -> Result<ExecutionCapture, ExecutorError> {
        Err(ExecutorError::NotStarted(
            "no execution backend is configured in this transport slice".into(),
        ))
    }
}

/// Loads a single JSON configuration argument and supervises outbound reconnects.
///
/// # Errors
///
/// Returns an error for invalid arguments/configuration or when retries are disabled and the
/// controller session fails.
pub async fn run_from_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(), WorkerError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let first = arguments.next().ok_or(WorkerError::Usage)?;
    if first == "enroll" {
        let bundle_path = PathBuf::from(arguments.next().ok_or(WorkerError::Usage)?);
        let state_directory = PathBuf::from(arguments.next().ok_or(WorkerError::Usage)?);
        if arguments.next().is_some() {
            return Err(WorkerError::Usage);
        }
        Box::pin(enroll_from_bundle(&bundle_path, &state_directory)).await?;
        return Ok(());
    }
    if first == "rotate" {
        let bundle_path = PathBuf::from(arguments.next().ok_or(WorkerError::Usage)?);
        let state_directory = PathBuf::from(arguments.next().ok_or(WorkerError::Usage)?);
        if arguments.next().is_some() {
            return Err(WorkerError::Usage);
        }
        Box::pin(rotate_from_bundle(&bundle_path, &state_directory)).await?;
        return Ok(());
    }
    if first == "rollback" {
        let state_directory = PathBuf::from(arguments.next().ok_or(WorkerError::Usage)?);
        if arguments.next().is_some() {
            return Err(WorkerError::Usage);
        }
        rollback_rotation(&state_directory)?;
        return Ok(());
    }
    let config_path = first;
    if arguments.next().is_some() {
        return Err(WorkerError::Usage);
    }
    let config_path = PathBuf::from(config_path);
    let mut config: WorkerConfig = serde_json::from_slice(
        &std::fs::read(&config_path)
            .map_err(|error| WorkerError::Configuration(error.to_string()))?,
    )
    .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    config.resolve_paths(base);
    Box::pin(run(config)).await
}

/// Runs one worker process incarnation and reconnects according to configuration.
///
/// # Errors
///
/// Returns an error for invalid configuration, journal startup, or a terminal session failure when
/// reconnect is disabled.
pub async fn run(config: WorkerConfig) -> Result<(), WorkerError> {
    let profile = config.runtime_profile()?;
    config.validate(&profile)?;
    let mut incarnation_id = WorkerIncarnationId::new();
    let mut bound_credential = None;
    let mut journal = SqliteEventStore::open(&config.journal_database)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    loop {
        let identity = config.resolve_identity()?;
        if bound_credential.is_some() && bound_credential != identity.credential_id {
            incarnation_id = WorkerIncarnationId::new();
        }
        bound_credential = identity.credential_id;
        let session = Box::pin(run_session(
            &config,
            &identity,
            &profile,
            &incarnation_id,
            &mut journal,
        ));
        let outcome = if let Some(credential_id) = identity.credential_id {
            tokio::select! {
                outcome = session => Some(outcome),
                changed = wait_for_identity_change(&config, credential_id) => {
                    changed?;
                    None
                }
            }
        } else {
            Some(session.await)
        };
        let Some(outcome) = outcome else {
            continue;
        };
        if let Err(error) = &outcome {
            eprintln!("cairn-worker session: {error}");
        }
        let Some(delay) = config.reconnect_delay_ms else {
            return outcome;
        };
        tokio::time::sleep(Duration::from_millis(delay.get())).await;
    }
}

async fn wait_for_identity_change(
    config: &WorkerConfig,
    credential_id: CredentialId,
) -> Result<(), WorkerError> {
    loop {
        tokio::time::sleep(Duration::from_millis(
            config.identity_poll_interval_ms.get(),
        ))
        .await;
        if config.resolve_identity()?.credential_id != Some(credential_id) {
            return Ok(());
        }
    }
}

impl WorkerConfig {
    fn resolve_identity(&self) -> Result<ResolvedWorkerIdentity, WorkerError> {
        match &self.identity {
            WorkerIdentityConfig::External { worker_id, tls } => Ok(ResolvedWorkerIdentity {
                worker_id: *worker_id,
                credential_id: None,
                tls: tls.clone(),
            }),
            WorkerIdentityConfig::Managed { state_directory } => {
                let identity_path = state_directory.join("identity.json");
                let mut identity: ManagedWorkerIdentity = serde_json::from_slice(
                    &fs::read(&identity_path)
                        .map_err(|error| WorkerError::Configuration(error.to_string()))?,
                )
                .map_err(|error| WorkerError::Configuration(error.to_string()))?;
                if !matches!(identity.schema_version, 1 | 2) {
                    return Err(WorkerError::Configuration(
                        "only managed worker identity schema_version 1 or 2 is supported".into(),
                    ));
                }
                validate_managed_material(state_directory, &identity)?;
                resolve(&mut identity.tls.certificate, state_directory);
                resolve(&mut identity.tls.private_key, state_directory);
                resolve(&mut identity.tls.server_ca, state_directory);
                Ok(ResolvedWorkerIdentity {
                    worker_id: identity.worker_id,
                    credential_id: Some(identity.credential_id),
                    tls: identity.tls,
                })
            }
        }
    }

    fn runtime_profile(&self) -> Result<WorkerProfile, WorkerError> {
        if self.schema_version != 5 {
            return Err(WorkerError::Configuration(
                "only worker schema_version 5 is supported".into(),
            ));
        }
        if self.profile.schema_version != 2 {
            return Err(WorkerError::Configuration(
                "only worker profile configuration schema_version 2 is supported".into(),
            ));
        }
        let platform = ExecutionPlatform::detect_host()
            .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        if self
            .expected_platform
            .architecture()
            .is_some_and(|required| required != platform.architecture())
            || self
                .expected_platform
                .operating_system()
                .is_some_and(|required| required != platform.operating_system())
            || self
                .expected_platform
                .target_environment()
                .is_some_and(|required| required != platform.target_environment())
        {
            return Err(WorkerError::Configuration(format!(
                "detected platform {}/{}/{} does not satisfy expected_platform",
                platform.architecture().as_str(),
                platform.operating_system().as_str(),
                platform.target_environment().as_str()
            )));
        }
        let declared = WorkerResourceSource::OperatorDeclared;
        let quantitative = HostResourceProbe::probe(
            &self.resource_probe,
            observed_now().map_err(|error| WorkerError::Configuration(error.to_string()))?,
        )
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        let resources = WorkerResourceInventory::new(
            WorkerResourceClaim::new(platform, WorkerResourceSource::BuiltinProbe),
            self.profile
                .backends
                .iter()
                .cloned()
                .map(|value| WorkerResourceClaim::new(value, declared))
                .collect(),
            self.profile
                .capabilities
                .iter()
                .cloned()
                .map(|value| WorkerResourceClaim::new(value, declared))
                .collect(),
            quantitative,
            self.profile.max_concurrency,
        )
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        WorkerProfile::new(
            self.profile.protocol_version,
            self.profile.binary_identity.clone(),
            resources,
        )
        .map_err(|error| WorkerError::Configuration(error.to_string()))
    }

    fn validate(&self, profile: &WorkerProfile) -> Result<(), WorkerError> {
        if profile.protocol_version()
            != WorkerProtocolVersion::new(1)
                .map_err(|error| WorkerError::Configuration(error.to_string()))?
        {
            return Err(WorkerError::Configuration(
                "this worker binary implements protocol_version 1".into(),
            ));
        }
        let configured = WorkerAvailability::new(
            self.availability.health(),
            self.availability.draining(),
            self.availability.available_slots(),
            Vec::new(),
        )
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        if configured != self.availability {
            return Err(WorkerError::Configuration(
                "configured active_attempts must be empty; runtime derives them from the journal"
                    .into(),
            ));
        }
        if self.availability.available_slots() > profile.max_concurrency().get() {
            return Err(WorkerError::Configuration(
                "availability exceeds profile max_concurrency".into(),
            ));
        }
        Ok(())
    }

    fn resolve_paths(&mut self, base: &Path) {
        match &mut self.identity {
            WorkerIdentityConfig::External { tls, .. } => {
                resolve(&mut tls.certificate, base);
                resolve(&mut tls.private_key, base);
                resolve(&mut tls.server_ca, base);
            }
            WorkerIdentityConfig::Managed { state_directory } => resolve(state_directory, base),
        }
        resolve(&mut self.journal_database, base);
        resolve(&mut self.resource_probe.scratch_path, base);
        if let Some(path) = &mut self.resource_probe.accelerator_sysfs {
            resolve(path, base);
        }
    }

    fn availability(
        &self,
        journal: &SqliteEventStore,
        worker_id: WorkerId,
    ) -> Result<WorkerAvailability, WorkerError> {
        let attempts = active_worker_attempts(journal, worker_id)
            .map_err(|error| WorkerError::Session(error.to_string()))?;
        let occupied = u16::try_from(attempts.len()).unwrap_or(u16::MAX);
        let slots = self.availability.available_slots().saturating_sub(occupied);
        WorkerAvailability::new(
            self.availability.health(),
            self.availability.draining(),
            slots,
            attempts,
        )
        .map_err(|error| WorkerError::Session(error.to_string()))
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the connection lifecycle is intentionally linear"
)]
async fn run_session(
    config: &WorkerConfig,
    identity: &ResolvedWorkerIdentity,
    profile: &WorkerProfile,
    incarnation_id: &WorkerIncarnationId,
    journal: &mut SqliteEventStore,
) -> Result<(), WorkerError> {
    let connecting = connect_worker_socket(
        config.controller.tcp_address.as_str(),
        &config.controller.websocket_uri,
        &identity.tls,
        config.transport,
    );
    let mut socket = timeout_optional(config.handshake_timeout_ms, connecting)
        .await?
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    let protocol_version = profile.protocol_version();
    let hello_observed_at = observed_now()?;
    let hello_resources = HostResourceProbe::probe(&config.resource_probe, hello_observed_at)
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    write_wire_message(
        &mut socket,
        &WorkerWireMessage::Hello {
            hello: Box::new(WorkerHello::new_with_resource_observation(
                identity.worker_id,
                *incarnation_id,
                profile.clone(),
                hello_resources,
            )),
            availability: config.availability(journal, identity.worker_id)?,
        },
        config.transport,
    )
    .await
    .map_err(|error| WorkerError::Session(error.to_string()))?;
    let welcome = timeout_optional(
        config.handshake_timeout_ms,
        read_wire_message::<_, ControllerWireMessage>(&mut socket, config.transport),
    )
    .await?
    .map_err(|error| WorkerError::Session(error.to_string()))?;
    let ControllerWireMessage::Welcome {
        connection_id,
        protocol_version: negotiated,
        ..
    } = welcome
    else {
        return Err(WorkerError::Session(format!(
            "controller rejected or violated handshake: {welcome:?}"
        )));
    };
    if negotiated != protocol_version {
        return Err(WorkerError::Session(
            "controller welcome changed protocol version".into(),
        ));
    }
    eprintln!(
        "cairn-worker {} connected as {}",
        identity.worker_id, connection_id
    );
    let mut inbound = InboundControlSession::new(protocol_version, connection_id);
    let mut highest_sent = None;
    let mut acknowledgement_sent = None;
    let mut idle_deadline = config
        .idle_timeout_ms
        .map(|limit| Instant::now() + Duration::from_millis(limit.get()));
    let mut heartbeat_interval = recurring_interval(config.heartbeat_interval_ms);
    let mut resource_refresh_interval =
        recurring_interval(config.resource_probe.refresh_interval_ms);
    loop {
        flush_worker(
            &mut socket,
            journal,
            config,
            identity.worker_id,
            &connection_id,
            inbound.received_through(),
            &mut highest_sent,
            &mut acknowledgement_sent,
        )
        .await?;
        let read = timeout_at_optional(
            idle_deadline,
            read_wire_message::<_, ControllerWireMessage>(&mut socket, config.transport),
        );
        let wake = tokio::select! {
            message = read => Wake::Message(message),
            () = tick_optional(&mut heartbeat_interval) => Wake::Heartbeat,
            () = tick_optional(&mut resource_refresh_interval) => Wake::ResourceRefresh,
        };
        match wake {
            Wake::Heartbeat => {
                write_wire_message(
                    &mut socket,
                    &WorkerWireMessage::Heartbeat {
                        availability: config.availability(journal, identity.worker_id)?,
                    },
                    config.transport,
                )
                .await
                .map_err(|error| WorkerError::Session(error.to_string()))?;
            }
            Wake::ResourceRefresh => {
                let observed_at = observed_now()?;
                let observation = HostResourceProbe::probe(&config.resource_probe, observed_at)
                    .map_err(|error| WorkerError::Session(error.to_string()))?;
                write_wire_message(
                    &mut socket,
                    &WorkerWireMessage::ResourcesObserved {
                        observation: Box::new(observation),
                    },
                    config.transport,
                )
                .await
                .map_err(|error| WorkerError::Session(error.to_string()))?;
            }
            Wake::Message(message) => {
                let message = message?.map_err(|error| WorkerError::Session(error.to_string()))?;
                idle_deadline = config
                    .idle_timeout_ms
                    .map(|limit| Instant::now() + Duration::from_millis(limit.get()));
                match message {
                    ControllerWireMessage::HeartbeatAccepted { .. }
                    | ControllerWireMessage::ResourcesAccepted { .. } => {}
                    ControllerWireMessage::Control { frame } => {
                        inbound
                            .accept(&frame, highest_sent)
                            .map_err(|error| WorkerError::Session(error.to_string()))?;
                        process_controller_frame(
                            journal,
                            identity.worker_id,
                            &connection_id,
                            &frame,
                        )?;
                    }
                    ControllerWireMessage::Welcome { .. } => {
                        return Err(WorkerError::Session(
                            "welcome repeated after handshake".into(),
                        ));
                    }
                    ControllerWireMessage::Reject { diagnostic, .. } => {
                        return Err(WorkerError::Session(format!(
                            "controller rejected live session: {diagnostic}"
                        )));
                    }
                }
            }
        }
    }
}

fn process_controller_frame(
    journal: &mut SqliteEventStore,
    worker_id: WorkerId,
    connection_id: &ControlConnectionId,
    frame: &ControlFrame<ControllerControlMessage>,
) -> Result<(), WorkerError> {
    let now = observed_now()?;
    if let Some(acknowledged) = frame.acknowledges_peer_through {
        acknowledge_worker_messages(
            journal,
            worker_id,
            *connection_id,
            acknowledged,
            &command("worker-ack"),
            now,
        )
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    }
    let Some(message) = &frame.message else {
        return Ok(());
    };
    match &message.payload {
        ControllerControlMessage::AssignmentOffer { .. } => {
            admit_worker_assignment(
                journal,
                worker_id,
                message,
                ControlMessageId::new(),
                &command("admit"),
                now,
            )
            .map_err(|error| WorkerError::Session(error.to_string()))?;
        }
        ControllerControlMessage::StartExecution { .. } => {
            if let Some(authority) =
                record_worker_execution_start(journal, worker_id, message, &command("start"), now)
                    .map_err(|error| WorkerError::Session(error.to_string()))?
            {
                let mut executor = NotStartedExecutor;
                execute_worker_attempt(
                    journal,
                    &mut executor,
                    authority,
                    ControlMessageId::new(),
                    &command("execute"),
                    observed_now()?,
                )
                .map_err(|error| WorkerError::Session(error.to_string()))?;
            }
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "durable delivery cursors and the stable worker identity remain explicit"
)]
async fn flush_worker(
    socket: &mut cairn_control_transport::ClientWebSocket,
    journal: &mut SqliteEventStore,
    config: &WorkerConfig,
    worker_id: WorkerId,
    connection_id: &ControlConnectionId,
    acknowledges: Option<ControlSequence>,
    highest_sent: &mut Option<ControlSequence>,
    acknowledgement_sent: &mut Option<ControlSequence>,
) -> Result<(), WorkerError> {
    let now = observed_now()?;
    let mut frames = deliver_worker_messages(
        journal,
        worker_id,
        config.profile.protocol_version,
        *connection_id,
        acknowledges,
        &command("deliver"),
        now,
    )
    .map_err(|error| WorkerError::Session(error.to_string()))?;
    let acknowledgement_only =
        acknowledges.filter(|value| frames.is_empty() && Some(*value) > *acknowledgement_sent);
    if let Some(acknowledges) = acknowledgement_only {
        frames.push(
            deliver_worker_acknowledgement(
                journal,
                worker_id,
                config.profile.protocol_version,
                *connection_id,
                acknowledges,
                &command("deliver-ack"),
                now,
            )
            .map_err(|error| WorkerError::Session(error.to_string()))?,
        );
    }
    for frame in frames {
        write_wire_message(
            socket,
            &WorkerWireMessage::Control {
                frame: Box::new(frame.clone()),
            },
            config.transport,
        )
        .await
        .map_err(|error| WorkerError::Session(error.to_string()))?;
        *highest_sent = Some(frame.sequence);
        if frame.acknowledges_peer_through.is_some() {
            *acknowledgement_sent = frame.acknowledges_peer_through;
        }
    }
    Ok(())
}

/// Redeems one enrollment bundle using a worker-local private key and atomically persists the
/// managed identity. A retry reuses the staged key and exact CSR.
///
/// # Errors
///
/// Returns an error for invalid bundle/state, key generation, TLS, rejection, or persistence.
pub async fn enroll_from_bundle(
    bundle_path: &Path,
    state_directory: &Path,
) -> Result<ManagedWorkerIdentity, WorkerError> {
    let bundle: EnrollmentBundle = serde_json::from_slice(
        &fs::read(bundle_path).map_err(|error| WorkerError::Configuration(error.to_string()))?,
    )
    .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    Box::pin(enroll(bundle, state_directory)).await
}

/// Redeems one already-decoded enrollment bundle.
///
/// # Errors
///
/// Returns an error for invalid authority/state, network, rejection, or persistence.
#[expect(
    clippy::too_many_lines,
    reason = "the staged key, CSR, wire exchange, and atomic commit form one linear safety boundary"
)]
pub async fn enroll(
    bundle: EnrollmentBundle,
    state_directory: &Path,
) -> Result<ManagedWorkerIdentity, WorkerError> {
    if !matches!(bundle.schema_version, 1 | 2) || bundle.purpose != EnrollmentPurpose::Bootstrap {
        return Err(WorkerError::Configuration(
            "enroll requires a supported bootstrap authority".into(),
        ));
    }
    prepare_state_directory(state_directory)?;
    let identity_path = state_directory.join("identity.json");
    if identity_path.exists() {
        let identity: ManagedWorkerIdentity = serde_json::from_slice(
            &fs::read(&identity_path)
                .map_err(|error| WorkerError::Configuration(error.to_string()))?,
        )
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        if !matches!(identity.schema_version, 1 | 2) {
            return Err(WorkerError::Configuration(
                "managed identity has an unsupported schema".into(),
            ));
        }
        if identity.enrollment_id != bundle.enrollment_id {
            return Err(WorkerError::Configuration(
                "managed state was created by another enrollment authority".into(),
            ));
        }
        validate_managed_material(state_directory, &identity)?;
        return Ok(identity);
    }

    let key_path = state_directory.join("worker-key.pem");
    let key_pem = if key_path.exists() {
        fs::read_to_string(&key_path)
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
    } else {
        let generated = KeyPair::generate()
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
            .serialize_pem();
        persist_exact(&key_path, generated.as_bytes(), true)?;
        generated
    };
    let key = KeyPair::from_pem(&key_pem)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let csr_path = state_directory.join("enrollment.csr.pem");
    let csr_pem = if csr_path.exists() {
        fs::read_to_string(&csr_path)
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
    } else {
        let mut params = CertificateParams::default();
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, bundle.enrollment_id.to_string());
        params.distinguished_name = distinguished_name;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let generated = params
            .serialize_request(&key)
            .and_then(|csr| csr.pem())
            .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        persist_exact(&csr_path, generated.as_bytes(), false)?;
        generated
    };
    let parsed_csr = rcgen::CertificateSigningRequestParams::from_pem(&csr_pem)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if parsed_csr.public_key.der_bytes() != key.public_key_raw()
        || parsed_csr.public_key.algorithm() != key.algorithm()
    {
        return Err(WorkerError::Configuration(
            "staged enrollment CSR does not match the staged private key".into(),
        ));
    }

    let credential = Box::pin(request_credential(&bundle, csr_pem)).await?;
    if !matches!(credential.schema_version, 1 | 2)
        || credential.predecessor_credential_id.is_some()
        || credential.predecessor_retire_at.is_some()
    {
        return Err(WorkerError::Session(
            "controller returned an invalid bootstrap credential".into(),
        ));
    }
    if certificate_public_key(&credential.certificate_chain_pem)? != key.public_key_der() {
        return Err(WorkerError::Session(
            "issued certificate does not bind the staged worker private key".into(),
        ));
    }
    persist_exact(
        &state_directory.join("worker.pem"),
        credential.certificate_chain_pem.as_bytes(),
        false,
    )?;
    persist_exact(
        &state_directory.join("ca.pem"),
        bundle.endpoint.server_ca_pem.as_bytes(),
        false,
    )?;
    let identity = ManagedWorkerIdentity {
        schema_version: 2,
        enrollment_id: bundle.enrollment_id,
        worker_id: credential.worker_id,
        credential_id: credential.credential_id,
        predecessor_credential_id: None,
        predecessor_retire_at: None,
        pool: credential.pool,
        tls: ClientTlsFiles {
            certificate: PathBuf::from("worker.pem"),
            private_key: PathBuf::from("worker-key.pem"),
            server_ca: PathBuf::from("ca.pem"),
            server_name: bundle.endpoint.server_name,
        },
    };
    let bytes = serde_json::to_vec_pretty(&identity)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    persist_exact(&identity_path, &bytes, false)?;
    Ok(identity)
}

/// Redeems a rotation authority using a fresh staged key and atomically switches managed identity.
///
/// A retry reuses the immutable staging directory and exact CSR. The predecessor identity and its
/// key/certificate remain available for an explicit rollback during the controller-frozen overlap.
///
/// # Errors
///
/// Returns an error for mismatched authority, staging corruption, network rejection, or commit
/// failure.
#[expect(
    clippy::too_many_lines,
    reason = "rotation staging, exact replay, validation, and atomic cutover are one safety boundary"
)]
pub async fn rotate(
    bundle: EnrollmentBundle,
    state_directory: &Path,
) -> Result<ManagedWorkerIdentity, WorkerError> {
    let EnrollmentPurpose::Rotation {
        worker_id,
        predecessor_credential_id,
    } = &bundle.purpose
    else {
        return Err(WorkerError::Configuration(
            "rotate requires a credential rotation authority".into(),
        ));
    };
    if bundle.schema_version != 2 {
        return Err(WorkerError::Configuration(
            "rotation authority schema is unsupported".into(),
        ));
    }
    prepare_state_directory(state_directory)?;
    let identity_path = state_directory.join("identity.json");
    let predecessor_bytes =
        fs::read(&identity_path).map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let predecessor: ManagedWorkerIdentity = serde_json::from_slice(&predecessor_bytes)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if predecessor.enrollment_id == bundle.enrollment_id {
        if predecessor.worker_id != *worker_id
            || predecessor.predecessor_credential_id != Some(*predecessor_credential_id)
        {
            return Err(WorkerError::Configuration(
                "committed rotation identity contradicts the authority".into(),
            ));
        }
        validate_managed_material(state_directory, &predecessor)?;
        return Ok(predecessor);
    }
    if predecessor.worker_id != *worker_id
        || predecessor.credential_id != *predecessor_credential_id
    {
        return Err(WorkerError::Configuration(
            "rotation authority does not name the current managed credential".into(),
        ));
    }
    validate_managed_material(state_directory, &predecessor)?;

    let relative_directory =
        PathBuf::from("rotations").join(bundle.enrollment_id.as_uuid().to_string());
    let staging_directory = state_directory.join(&relative_directory);
    prepare_state_directory(&staging_directory)?;
    persist_exact(
        &staging_directory.join("predecessor-identity.json"),
        &predecessor_bytes,
        false,
    )?;
    let key_path = staging_directory.join("worker-key.pem");
    let key_pem = if key_path.exists() {
        fs::read_to_string(&key_path)
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
    } else {
        let generated = KeyPair::generate()
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
            .serialize_pem();
        persist_exact(&key_path, generated.as_bytes(), true)?;
        generated
    };
    let key = KeyPair::from_pem(&key_pem)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let csr_path = staging_directory.join("rotation.csr.pem");
    let csr_pem = if csr_path.exists() {
        fs::read_to_string(&csr_path)
            .map_err(|error| WorkerError::Configuration(error.to_string()))?
    } else {
        let mut params = CertificateParams::default();
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, bundle.enrollment_id.to_string());
        params.distinguished_name = distinguished_name;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let generated = params
            .serialize_request(&key)
            .and_then(|csr| csr.pem())
            .map_err(|error| WorkerError::Configuration(error.to_string()))?;
        persist_exact(&csr_path, generated.as_bytes(), false)?;
        generated
    };
    let parsed_csr = rcgen::CertificateSigningRequestParams::from_pem(&csr_pem)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if parsed_csr.public_key.der_bytes() != key.public_key_raw()
        || parsed_csr.public_key.algorithm() != key.algorithm()
    {
        return Err(WorkerError::Configuration(
            "staged rotation CSR does not match the staged private key".into(),
        ));
    }

    let credential = Box::pin(request_credential(&bundle, csr_pem)).await?;
    if credential.schema_version != 2
        || credential.worker_id != *worker_id
        || credential.pool != predecessor.pool
        || credential.credential_id == *predecessor_credential_id
        || credential.predecessor_credential_id != Some(*predecessor_credential_id)
    {
        return Err(WorkerError::Session(
            "controller returned a rotation credential with contradictory lineage".into(),
        ));
    }
    if certificate_public_key(&credential.certificate_chain_pem)? != key.public_key_der() {
        return Err(WorkerError::Session(
            "rotated certificate does not bind the staged private key".into(),
        ));
    }
    persist_exact(
        &staging_directory.join("worker.pem"),
        credential.certificate_chain_pem.as_bytes(),
        false,
    )?;
    persist_exact(
        &staging_directory.join("ca.pem"),
        bundle.endpoint.server_ca_pem.as_bytes(),
        false,
    )?;
    let identity = ManagedWorkerIdentity {
        schema_version: 2,
        enrollment_id: bundle.enrollment_id,
        worker_id: credential.worker_id,
        credential_id: credential.credential_id,
        predecessor_credential_id: credential.predecessor_credential_id,
        predecessor_retire_at: credential.predecessor_retire_at,
        pool: credential.pool,
        tls: ClientTlsFiles {
            certificate: relative_directory.join("worker.pem"),
            private_key: relative_directory.join("worker-key.pem"),
            server_ca: relative_directory.join("ca.pem"),
            server_name: bundle.endpoint.server_name,
        },
    };
    validate_managed_material(state_directory, &identity)?;
    let identity_bytes = serde_json::to_vec_pretty(&identity)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    replace_exact(&identity_path, &identity_bytes, false)?;
    Ok(identity)
}

/// Loads and redeems one rotation bundle.
///
/// # Errors
///
/// Returns an error for bundle decoding or rotation failure.
pub async fn rotate_from_bundle(
    bundle_path: &Path,
    state_directory: &Path,
) -> Result<ManagedWorkerIdentity, WorkerError> {
    let bundle: EnrollmentBundle = serde_json::from_slice(
        &fs::read(bundle_path).map_err(|error| WorkerError::Configuration(error.to_string()))?,
    )
    .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    Box::pin(rotate(bundle, state_directory)).await
}

/// Atomically restores the predecessor identity while its overlap window remains open.
///
/// # Errors
///
/// Returns an error when there is no rotation predecessor, the overlap elapsed, or staged material
/// is inconsistent.
pub fn rollback_rotation(state_directory: &Path) -> Result<ManagedWorkerIdentity, WorkerError> {
    let identity_path = state_directory.join("identity.json");
    let current: ManagedWorkerIdentity = serde_json::from_slice(
        &fs::read(&identity_path).map_err(|error| WorkerError::Configuration(error.to_string()))?,
    )
    .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let predecessor_id = current.predecessor_credential_id.ok_or_else(|| {
        WorkerError::Configuration("managed identity has no rotation predecessor".into())
    })?;
    let now = observed_now()?;
    if current
        .predecessor_retire_at
        .is_some_and(|retire_at| now >= retire_at)
    {
        return Err(WorkerError::Configuration(
            "credential rotation overlap has elapsed".into(),
        ));
    }
    let predecessor_path = state_directory
        .join("rotations")
        .join(current.enrollment_id.as_uuid().to_string())
        .join("predecessor-identity.json");
    let predecessor_bytes = fs::read(&predecessor_path)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let predecessor: ManagedWorkerIdentity = serde_json::from_slice(&predecessor_bytes)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if predecessor.worker_id != current.worker_id
        || predecessor.pool != current.pool
        || predecessor.credential_id != predecessor_id
    {
        return Err(WorkerError::Configuration(
            "staged predecessor identity contradicts current rotation lineage".into(),
        ));
    }
    validate_managed_material(state_directory, &predecessor)?;
    replace_exact(&identity_path, &predecessor_bytes, false)?;
    Ok(predecessor)
}

async fn request_credential(
    bundle: &EnrollmentBundle,
    csr_pem: String,
) -> Result<cairn_control_transport::IssuedWorkerCredential, WorkerError> {
    let connecting = connect_enrollment_socket(
        bundle.endpoint.tcp_address.as_str(),
        &bundle.endpoint.websocket_uri,
        &bundle.endpoint.server_name,
        &bundle.endpoint.server_ca_pem,
        bundle.transport,
    );
    let mut socket = timeout_optional(bundle.handshake_timeout_ms, connecting)
        .await?
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    write_wire_message(
        &mut socket,
        &EnrollmentRequest {
            schema_version: 1,
            enrollment_id: bundle.enrollment_id,
            secret: bundle.secret.clone(),
            csr_pem,
        },
        bundle.transport,
    )
    .await
    .map_err(|error| WorkerError::Session(error.to_string()))?;
    let response = timeout_optional(
        bundle.handshake_timeout_ms,
        read_wire_message::<_, EnrollmentResponse>(&mut socket, bundle.transport),
    )
    .await?
    .map_err(|error| WorkerError::Session(error.to_string()))?;
    match response {
        EnrollmentResponse::Issued { credential } => Ok(credential),
        EnrollmentResponse::Reject { code, diagnostic } => Err(WorkerError::Session(format!(
            "enrollment rejected ({code:?}): {diagnostic}"
        ))),
    }
}

fn prepare_state_directory(path: &Path) -> Result<(), WorkerError> {
    fs::create_dir_all(path).map_err(|error| WorkerError::Configuration(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    }
    Ok(())
}

fn certificate_public_key(pem: &str) -> Result<Vec<u8>, WorkerError> {
    let mut reader = BufReader::new(pem.as_bytes());
    let certificate = rustls_pemfile::certs(&mut reader)
        .next()
        .transpose()
        .map_err(|error| WorkerError::Session(error.to_string()))?
        .ok_or_else(|| WorkerError::Session("issued certificate chain is empty".into()))?;
    let (_, parsed) = x509_parser::parse_x509_certificate(certificate.as_ref())
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    Ok(parsed.public_key().raw.to_vec())
}

fn validate_managed_material(
    state_directory: &Path,
    identity: &ManagedWorkerIdentity,
) -> Result<(), WorkerError> {
    if !matches!(identity.schema_version, 1 | 2)
        || identity.predecessor_retire_at.is_some() && identity.predecessor_credential_id.is_none()
        || identity.predecessor_credential_id == Some(identity.credential_id)
    {
        return Err(WorkerError::Configuration(
            "managed identity lifecycle metadata is invalid".into(),
        ));
    }
    let key_pem = fs::read_to_string(state_directory.join(&identity.tls.private_key))
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let key = KeyPair::from_pem(&key_pem)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    let certificate = fs::read_to_string(state_directory.join(&identity.tls.certificate))
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if certificate_public_key(&certificate)? != key.public_key_der() {
        return Err(WorkerError::Configuration(
            "managed certificate does not bind the managed private key".into(),
        ));
    }
    let ca = fs::read(state_directory.join(&identity.tls.server_ca))
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    if ca.is_empty() {
        return Err(WorkerError::Configuration(
            "managed controller trust anchor is empty".into(),
        ));
    }
    Ok(())
}

fn persist_exact(path: &Path, bytes: &[u8], secret: bool) -> Result<(), WorkerError> {
    if path.exists() {
        let existing =
            fs::read(path).map_err(|error| WorkerError::Configuration(error.to_string()))?;
        return if existing == bytes {
            Ok(())
        } else {
            Err(WorkerError::Configuration(format!(
                "refusing to overwrite different managed identity material at {}",
                path.display()
            )))
        };
    }
    let suffix = CommandId::new().as_uuid();
    let temporary = path.with_extension(format!("tmp-{suffix}"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(if secret { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = options
        .open(&temporary)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    fs::rename(&temporary, path).map_err(|error| WorkerError::Configuration(error.to_string()))?;
    sync_parent(path)?;
    Ok(())
}

fn replace_exact(path: &Path, bytes: &[u8], secret: bool) -> Result<(), WorkerError> {
    let suffix = CommandId::new().as_uuid();
    let temporary = path.with_extension(format!("tmp-{suffix}"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(if secret { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = options
        .open(&temporary)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    fs::rename(&temporary, path).map_err(|error| WorkerError::Configuration(error.to_string()))?;
    sync_parent(path)
}

fn sync_parent(path: &Path) -> Result<(), WorkerError> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            WorkerError::Configuration("managed identity path has no parent".into())
        })?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    }
    Ok(())
}

async fn timeout_optional<F, T>(limit: Option<NonZeroU64>, future: F) -> Result<T, WorkerError>
where
    F: Future<Output = T>,
{
    if let Some(limit) = limit {
        tokio::time::timeout(Duration::from_millis(limit.get()), future)
            .await
            .map_err(|_| WorkerError::Session("configured timeout elapsed".into()))
    } else {
        Ok(future.await)
    }
}

async fn timeout_at_optional<F, T>(deadline: Option<Instant>, future: F) -> Result<T, WorkerError>
where
    F: Future<Output = T>,
{
    if let Some(deadline) = deadline {
        tokio::time::timeout_at(deadline, future)
            .await
            .map_err(|_| WorkerError::Session("configured idle timeout elapsed".into()))
    } else {
        Ok(future.await)
    }
}

fn recurring_interval(period: Option<NonZeroU64>) -> Option<Interval> {
    period.map(|period| {
        let duration = Duration::from_millis(period.get());
        tokio::time::interval_at(Instant::now() + duration, duration)
    })
}

async fn tick_optional(interval: &mut Option<Interval>) {
    if let Some(interval) = interval {
        interval.tick().await;
    } else {
        std::future::pending::<()>().await;
    }
}

fn observed_now() -> Result<ObservedAtUnixMillis, WorkerError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    let millis = i64::try_from(duration.as_millis())
        .map_err(|_| WorkerError::Session("wall clock exceeds i64 milliseconds".into()))?;
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

#[cfg(test)]
mod tests {
    use cairn_execution::{ArchitectureName, ExecutionPlatformRequirement, WorkerResourceSource};

    use super::WorkerConfig;

    #[test]
    fn documented_configuration_is_strictly_decodable() {
        let config: WorkerConfig =
            serde_json::from_str(include_str!("../../../config/worker.example.json"))
                .expect("documented worker configuration");
        let profile = config.runtime_profile().expect("runtime profile");
        assert_eq!(
            profile.resources().platform().source(),
            WorkerResourceSource::BuiltinProbe
        );
        assert!(
            profile
                .resources()
                .backends()
                .iter()
                .all(|claim| { claim.source() == WorkerResourceSource::OperatorDeclared })
        );
    }

    #[test]
    fn expected_platform_fails_closed_instead_of_overriding_detection() {
        let mut config: WorkerConfig =
            serde_json::from_str(include_str!("../../../config/worker.example.json"))
                .expect("documented worker configuration");
        config.expected_platform = ExecutionPlatformRequirement::new(
            Some(ArchitectureName::new("definitely-not-the-host").expect("architecture")),
            None,
            None,
        );
        assert!(config.runtime_profile().is_err());
    }
}
