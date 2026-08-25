//! Runnable outbound Cairn worker composition root.

use std::{
    ffi::OsString,
    future::Future,
    num::NonZeroU64,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cairn_control_transport::{
    ClientTlsFiles, ControllerWireMessage, TransportPolicy, WorkerWireMessage,
    connect_worker_socket, read_wire_message, write_wire_message,
};
use cairn_execution::{
    CapabilityRequirement, ControlFrame, ControllerControlMessage, ExecutionBackend,
    ExecutionCapture, ExecutionInput, ExecutionPlatform, ExecutionPlatformRequirement, Executor,
    ExecutorError, InboundControlSession, WorkerAvailability, WorkerBinaryIdentity, WorkerHello,
    WorkerProfile, WorkerProtocolVersion, WorkerResourceClaim, WorkerResourceInventory,
    WorkerResourceSource, WorkerSlotCount, acknowledge_worker_messages, active_worker_attempts,
    admit_worker_assignment, deliver_worker_acknowledgement, deliver_worker_messages,
    execute_worker_attempt, record_worker_execution_start,
};
use cairn_protocol::{
    CommandId, ControlConnectionId, ControlMessageId, ControlSequence, ObservedAtUnixMillis,
    WorkerId, WorkerIncarnationId,
};
use cairn_store_sqlite::SqliteEventStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::Instant;

/// Strict worker process configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConfig {
    pub schema_version: u16,
    pub controller: ControllerEndpoint,
    pub tls: ClientTlsFiles,
    pub worker_id: WorkerId,
    pub profile: WorkerProfileConfig,
    #[serde(default)]
    pub expected_platform: ExecutionPlatformRequirement,
    pub availability: WorkerAvailability,
    pub journal_database: PathBuf,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub idle_timeout_ms: Option<NonZeroU64>,
    pub heartbeat_interval_ms: Option<NonZeroU64>,
    pub reconnect_delay_ms: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
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
    #[error("usage: cairn-worker <config.json>")]
    Usage,
    #[error("worker configuration failed: {0}")]
    Configuration(String),
    #[error("worker session failed: {0}")]
    Session(String),
}

enum Wake<T> {
    Message(T),
    Heartbeat,
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
    let config_path = arguments.next().ok_or(WorkerError::Usage)?;
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
    let incarnation_id = WorkerIncarnationId::new();
    let mut journal = SqliteEventStore::open(&config.journal_database)
        .map_err(|error| WorkerError::Configuration(error.to_string()))?;
    loop {
        let outcome = Box::pin(run_session(
            &config,
            &profile,
            &incarnation_id,
            &mut journal,
        ))
        .await;
        if let Err(error) = &outcome {
            eprintln!("cairn-worker session: {error}");
        }
        let Some(delay) = config.reconnect_delay_ms else {
            return outcome;
        };
        tokio::time::sleep(Duration::from_millis(delay.get())).await;
    }
}

impl WorkerConfig {
    fn runtime_profile(&self) -> Result<WorkerProfile, WorkerError> {
        if self.schema_version != 1 {
            return Err(WorkerError::Configuration(
                "only worker schema_version 1 is supported".into(),
            ));
        }
        if self.profile.schema_version != 1 {
            return Err(WorkerError::Configuration(
                "only worker profile configuration schema_version 1 is supported".into(),
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
        resolve(&mut self.tls.certificate, base);
        resolve(&mut self.tls.private_key, base);
        resolve(&mut self.tls.server_ca, base);
        resolve(&mut self.journal_database, base);
    }

    fn availability(&self, journal: &SqliteEventStore) -> Result<WorkerAvailability, WorkerError> {
        let attempts = active_worker_attempts(journal, self.worker_id)
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
    profile: &WorkerProfile,
    incarnation_id: &WorkerIncarnationId,
    journal: &mut SqliteEventStore,
) -> Result<(), WorkerError> {
    let connecting = connect_worker_socket(
        config.controller.tcp_address.as_str(),
        &config.controller.websocket_uri,
        &config.tls,
        config.transport,
    );
    let mut socket = timeout_optional(config.handshake_timeout_ms, connecting)
        .await?
        .map_err(|error| WorkerError::Session(error.to_string()))?;
    let protocol_version = profile.protocol_version();
    write_wire_message(
        &mut socket,
        &WorkerWireMessage::Hello {
            hello: WorkerHello::new(config.worker_id, *incarnation_id, profile.clone()),
            availability: config.availability(journal)?,
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
        config.worker_id, connection_id
    );
    let mut inbound = InboundControlSession::new(protocol_version, connection_id);
    let mut highest_sent = None;
    let mut acknowledgement_sent = None;
    let mut idle_deadline = config
        .idle_timeout_ms
        .map(|limit| Instant::now() + Duration::from_millis(limit.get()));
    loop {
        flush_worker(
            &mut socket,
            journal,
            config,
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
        let wake = if let Some(interval) = config.heartbeat_interval_ms {
            tokio::select! {
                message = read => Wake::Message(message),
                () = tokio::time::sleep(Duration::from_millis(interval.get())) => Wake::Heartbeat,
            }
        } else {
            Wake::Message(read.await)
        };
        match wake {
            Wake::Heartbeat => {
                write_wire_message(
                    &mut socket,
                    &WorkerWireMessage::Heartbeat {
                        availability: config.availability(journal)?,
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
                    ControllerWireMessage::HeartbeatAccepted { .. } => {}
                    ControllerWireMessage::Control { frame } => {
                        inbound
                            .accept(&frame, highest_sent)
                            .map_err(|error| WorkerError::Session(error.to_string()))?;
                        process_controller_frame(journal, config, &connection_id, &frame)?;
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
    config: &WorkerConfig,
    connection_id: &ControlConnectionId,
    frame: &ControlFrame<ControllerControlMessage>,
) -> Result<(), WorkerError> {
    let now = observed_now()?;
    if let Some(acknowledged) = frame.acknowledges_peer_through {
        acknowledge_worker_messages(
            journal,
            config.worker_id,
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
                config.worker_id,
                message,
                ControlMessageId::new(),
                &command("admit"),
                now,
            )
            .map_err(|error| WorkerError::Session(error.to_string()))?;
        }
        ControllerControlMessage::StartExecution { .. } => {
            if let Some(authority) = record_worker_execution_start(
                journal,
                config.worker_id,
                message,
                &command("start"),
                now,
            )
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

async fn flush_worker(
    socket: &mut cairn_control_transport::ClientWebSocket,
    journal: &mut SqliteEventStore,
    config: &WorkerConfig,
    connection_id: &ControlConnectionId,
    acknowledges: Option<ControlSequence>,
    highest_sent: &mut Option<ControlSequence>,
    acknowledgement_sent: &mut Option<ControlSequence>,
) -> Result<(), WorkerError> {
    let now = observed_now()?;
    let mut frames = deliver_worker_messages(
        journal,
        config.worker_id,
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
                config.worker_id,
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
