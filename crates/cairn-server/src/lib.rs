//! Runnable Cairn controller composition root.

pub mod controller_workflow;

#[cfg(feature = "migration-runtime")]
mod app_api;
#[cfg(feature = "migration-runtime")]
mod controller_manager;
#[cfg(feature = "migration-runtime")]
mod controller_state;
mod enrollment;
#[cfg(feature = "migration-runtime")]
mod intent_admission_supervisor;
#[cfg(feature = "migration-runtime")]
mod proposal_step_runner;
mod scheduling;

#[cfg(feature = "migration-runtime")]
pub use controller_manager::{
    ControllerWorkflowManagerStatusV1, authorize_controller_oracle_admission,
    dispatch_controller_oracle_worker_request, drive_controller_workflow_once,
    freeze_sir_controller_request, initialize_candidate_build, initialize_candidate_proposal_loop,
    initialize_controller_oracle_exploration, initialize_product_oracle_exploration,
    prepare_candidate_strategy_proposal_step_request,
    prepare_oracle_strategy_proposal_step_request, reauthorize_controller_intent_admission,
    record_controller_oracle_strategy_submission, record_controller_oracle_strategy_terminal,
    record_controller_user_intent_decision,
};
#[cfg(feature = "migration-runtime")]
pub use controller_state::{
    ControllerWorkflowError, ControllerWorkflowNextActionV1, ControllerWorkflowStateV1,
    ControllerWorkflowV1, FrozenCandidateAdmissionAuthorityV1, FrozenCandidateBuildAuthorityV1,
    FrozenCandidateOracleAuthorityV1, FrozenCandidateProposalAuthorityV1,
    FrozenOracleAdmissionAuthorityV1, FrozenOracleControlAuthorityV1,
    FrozenOracleExplorationAuthorityV1, FrozenOraclePortfolioAuthorityV1,
    FrozenOracleStrategyAuthorityV1, FrozenSirAuthorityV1, MigrationTerminalStatusV1,
    OracleStrategyCompletionV1, authorize_candidate_admission, authorize_candidate_build,
    authorize_candidate_proposal_episode, authorize_intent_admission, authorize_oracle_admission,
    authorize_oracle_strategy, authorize_sir_episode, cancel_controller_workflow,
    freeze_candidate_build, freeze_candidate_oracle_contract, freeze_candidate_proposal_request,
    freeze_controller_workflow, freeze_oracle_portfolio, open_oracle_exploration,
    record_admitted_intent, record_candidate_admission_outcome, record_candidate_build_observation,
    record_candidate_proposal, record_intent_decision_requests, record_oracle_strategy_completion,
    record_oracle_strategy_observations, record_sir_proposal, record_user_intent_decision,
    recover_controller_workflow,
};
#[cfg(feature = "migration-runtime")]
pub use intent_admission_supervisor::{
    IntentAdmissionProcessBlockedV1, IntentAdmissionProcessConfigV1,
    IntentAdmissionProcessTimeoutMillis, IntentAdmissionStderrByteLimit,
    IntentAdmissionStdoutByteLimit,
};
#[cfg(feature = "migration-runtime")]
pub use proposal_step_runner::{ProposalStepConfigV1, execute_controller_workflow_tools};

pub use enrollment::{
    RegistryCredentialInspection, RegistryCredentialProvenance, RegistryCredentialStatus,
    RegistryMutationOutcome, RegistryWorkerInspection, WorkerRegistryAudit,
    WorkerRegistryInspection,
};

pub use scheduling::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome,
    ScheduledAssignmentPhase, SchedulerServiceConfig, release_execution_reservation,
    release_execution_reservation_at, schedule_execution_contract, schedule_execution_contract_at,
};

use std::{
    ffi::OsString,
    fs,
    io::Write,
    net::SocketAddr,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "migration-runtime")]
use cairn_agent::{ModelTemplate, ModelTemplateRegistry, RuntimeModelAlias, RuntimeModelCatalog};
use cairn_control_transport::{
    CertificateFingerprint, ControllerRejectCode, ControllerWireMessage, EnrollmentBundle,
    EnrollmentRejectCode, EnrollmentRequest, EnrollmentResponse, ServerTlsFiles, TransportPolicy,
    WorkerWireMessage, accept_enrollment_socket, accept_worker_socket, read_wire_message,
    validate_material_chunk_wire_size, write_wire_message,
};
use cairn_execution::{
    AcceptedExecutionAssignment, AssignmentLeaseRecord, AuthenticatedWorkerIdentity, ControlFrame,
    ExecutionAssignmentState, ExecutionCompletion, InboundControlSession,
    RecordedWorkerAuthenticator, RegisteredWorkerSession, SchedulerPolicyVersion,
    TrustedWorkerPoolAssignment, WorkerAuthenticationSubject, WorkerControlMessage, WorkerPoolName,
    WorkerProtocolVersion, WorkerResultReconciliation, WorkerSessionTimeoutMillis,
    accept_worker_assignment, acknowledge_controller_messages, deliver_controller_acknowledgement,
    deliver_controller_messages, disconnect_worker, enqueue_controller_message,
    execution_start_message, read_assignment_material_chunk, reconcile_worker_result,
    record_worker_heartbeat, record_worker_resource_observation, recover_execution_assignment,
    register_worker, start_accepted_assignment, synchronize_worker_pool_assignment,
};
use cairn_protocol::{
    CommandId, ControlConnectionId, ControlSequence, CredentialId, EnrollmentId, EventId,
    ObservedAtUnixMillis, ReservationId, WorkerId,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex, time::Instant};

use enrollment::{
    EnrollmentError, EnrollmentIssuer, EnrollmentRegistry, create_offer, create_rotation_offer,
    inspect_registry, redeem,
};
use enrollment::{
    assign_worker_pool, audit_registry, disable_worker, enable_worker, revoke_credential,
    revoke_enrollment,
};

/// Strict controller process configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub schema_version: u16,
    pub listen: SocketAddr,
    pub tls: ServerTlsFiles,
    pub enrollment_service: Option<EnrollmentServiceConfig>,
    pub storage: ServerStorageConfig,
    /// Optional local product API. The field is required in current-V1 configuration; `null`
    /// disables client task intake explicitly.
    pub app_api: Option<AppApiConfigV1>,
    pub protocol_version: WorkerProtocolVersion,
    pub session_timeout_ms: WorkerSessionTimeoutMillis,
    /// Optional generic scheduler service. `null` disables new placement while worker control and
    /// reconciliation remain available.
    #[serde(default)]
    pub scheduler: Option<SchedulerServiceConfig>,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub idle_timeout_ms: Option<NonZeroU64>,
    pub outbox_poll_interval_ms: Option<NonZeroU64>,
    pub authority_poll_interval_ms: NonZeroU64,
    /// Maximum admitted worker clock lead. `null` requires the worker clock not to be ahead.
    #[serde(default)]
    pub resource_clock_skew_tolerance_ms: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
    pub diagnostic_byte_limit: Option<NonZeroU64>,
}

/// Local product API and migration-runtime composition.
#[cfg(feature = "migration-runtime")]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppApiConfigV1 {
    pub unix_socket: PathBuf,
    /// Authenticated principal represented by access to this local server boundary.
    pub intent_authority_subject: cairn_admission::TaskIntentAuthoritySubject,
    pub proposal_step: ProposalStepConfigV1,
    pub intent_admission: IntentAdmissionProcessConfigV1,
    pub oracle: ProductOracleConfigV1,
}

/// Product-owned, task-generic Oracle Exploration policy and public context.
#[cfg(feature = "migration-runtime")]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductOracleConfigV1 {
    pub coverage_profile: cairn_migration::OracleCoverageProfileV1,
    pub adversarial_policy: cairn_migration::OracleAdversarialPolicyV1,
    pub budget: cairn_migration::OracleExplorationBudgetV1,
    pub documentation: String,
    pub build_and_tests: String,
}

/// Placeholder definition for builds that intentionally exclude migration Proposal step support.
#[cfg(not(feature = "migration-runtime"))]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppApiConfigV1 {
    pub unix_socket: PathBuf,
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
    /// Server identity dedicated to the bootstrap listener.
    pub server_tls: EnrollmentServerTlsFiles,
    /// Ordinary-control endpoint embedded into enrollment bundles.
    pub control_endpoint: PublicWorkerControlEndpointConfig,
    pub issuer_certificate: PathBuf,
    pub issuer_private_key: PathBuf,
    pub credential_validity_ms: NonZeroU64,
    #[serde(default)]
    pub rotation_overlap_ms: Option<NonZeroU64>,
    pub handshake_timeout_ms: Option<NonZeroU64>,
    pub diagnostic_byte_limit: Option<NonZeroU64>,
    #[serde(default)]
    pub transport: TransportPolicy,
}

/// Server certificate and key for the server-authenticated bootstrap listener.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentServerTlsFiles {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

/// Externally routable worker-control endpoint and its independently pinned server authority.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicWorkerControlEndpointConfig {
    pub tcp_address: String,
    pub websocket_uri: String,
    pub server_name: String,
    pub server_ca: PathBuf,
}

#[derive(Clone)]
pub(crate) struct EnrolledWorker {
    pub(crate) worker_id: WorkerId,
    pub(crate) credential_id: CredentialId,
    pub(crate) pool: WorkerPoolName,
    pub(crate) pool_assignment_revision: EventId,
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
        "usage: cairn-server <config.json> | cairn-server model resolve <runtime-catalog.json> <model-template.json> <alias> <output.json> | cairn-server registry list|audit <config.json> | cairn-server registry show-worker <config.json> <worker-id> | cairn-server registry show-credential <config.json> <credential-id> | cairn-server enrollment create <config.json> <pool> <ttl-ms> <bundle.json> | cairn-server enrollment revoke <config.json> <enrollment-id> <command-id> | cairn-server credential rotate <config.json> <credential-id> <ttl-ms> <bundle.json> | cairn-server credential revoke <config.json> <credential-id> <command-id> | cairn-server worker disable|enable <config.json> <worker-id> <command-id> | cairn-server worker set-pool <config.json> <worker-id> <pool> <command-id> | cairn-server reservation release <config.json> <reservation-id> <command-id>"
    )]
    Usage,
    #[error("controller configuration failed: {0}")]
    Configuration(String),
    #[error("controller startup failed: {0}")]
    Startup(String),
    #[error("worker session failed: {0}")]
    Session(String),
    #[error("controller scheduling failed: {0}")]
    Scheduling(String),
    #[error("migration workflow composition failed: {0}")]
    MigrationWorkflow(String),
    #[error("registry entry not found: {0}")]
    RegistryEntryNotFound(String),
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
#[expect(
    clippy::too_many_lines,
    reason = "the small command-line surface keeps strict positional parsing in one visible boundary"
)]
pub async fn run_from_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(), ServerError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let first = arguments.next().ok_or(ServerError::Usage)?;
    #[cfg(feature = "migration-runtime")]
    if first == "model" {
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new("resolve")) {
            return Err(ServerError::Usage);
        }
        let catalog_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let template_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let alias = RuntimeModelAlias::new(
            arguments
                .next()
                .ok_or(ServerError::Usage)?
                .into_string()
                .map_err(|_| ServerError::Usage)?,
        )
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let output_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        let catalog: RuntimeModelCatalog = serde_json::from_slice(
            &fs::read(catalog_path)
                .map_err(|error| ServerError::Configuration(error.to_string()))?,
        )
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let template: ModelTemplate = serde_json::from_slice(
            &fs::read(template_path)
                .map_err(|error| ServerError::Configuration(error.to_string()))?,
        )
        .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let templates = ModelTemplateRegistry::from_templates([template])
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let resolved = catalog
            .resolve(&templates, Some(&alias))
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        write_new_secret_file(
            &output_path,
            &resolved
                .canonical_bytes()
                .map_err(|error| ServerError::Configuration(error.to_string()))?,
        )?;
        tracing::info!(
            target: "cairn.server.model",
            event = "runtime_model_resolved",
            model_alias = alias.as_str(),
            output = %output_path.display(),
            "resolved secret-free runtime model snapshot"
        );
        return Ok(());
    }
    if first == "reservation" {
        let action = arguments.next().ok_or(ServerError::Usage)?;
        if action != "release" {
            return Err(ServerError::Usage);
        }
        let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let reservation_id = parse_argument::<ReservationId>(arguments.next())?;
        let command_id = parse_argument::<CommandId>(arguments.next())?;
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        let reason = release_execution_reservation(
            &load_config(&config_path)?,
            reservation_id,
            &command_id,
        )?;
        tracing::info!(
            target: "cairn.server.registry",
            event = "reservation_released",
            reservation_id = %reservation_id,
            reason = ?reason,
            "scheduler reservation released"
        );
        return Ok(());
    }
    if first == "registry" {
        return run_registry_command(&mut arguments);
    }
    if first == "enrollment" {
        let action = arguments.next().ok_or(ServerError::Usage)?;
        let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        if action == "revoke" {
            let enrollment_id = parse_argument::<EnrollmentId>(arguments.next())?;
            let command_id = parse_argument::<CommandId>(arguments.next())?;
            if arguments.next().is_some() {
                return Err(ServerError::Usage);
            }
            return revoke_enrollment_authority(
                &load_config(&config_path)?,
                enrollment_id,
                &command_id,
            )
            .map(|outcome| report_registry_mutation("revoked enrollment", outcome));
        }
        if action != "create" {
            return Err(ServerError::Usage);
        }
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
        tracing::info!(
            target: "cairn.server.enrollment",
            event = "enrollment_bundle_written",
            "enrollment bundle written to operator-selected secret file"
        );
        return Ok(());
    }
    if first == "credential" {
        let action = arguments.next().ok_or(ServerError::Usage)?;
        let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let credential_id = parse_argument::<CredentialId>(arguments.next())?;
        let config = load_config(&config_path)?;
        if action == "revoke" {
            let command_id = parse_argument::<CommandId>(arguments.next())?;
            if arguments.next().is_some() {
                return Err(ServerError::Usage);
            }
            return revoke_worker_credential(&config, credential_id, &command_id)
                .map(|outcome| report_registry_mutation("revoked credential", outcome));
        }
        if action == "rotate" {
            let ttl_ms = parse_nonzero(arguments.next())?;
            let output_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
            if arguments.next().is_some() {
                return Err(ServerError::Usage);
            }
            let bundle = create_rotation_bundle(&config, credential_id, ttl_ms)?;
            let bytes = serde_json::to_vec_pretty(&bundle)
                .map_err(|error| ServerError::Configuration(error.to_string()))?;
            write_new_secret_file(&output_path, &bytes)?;
            tracing::info!(
                target: "cairn.server.enrollment",
                event = "credential_rotation_bundle_written",
                credential_id = %credential_id,
                "credential rotation bundle written to operator-selected secret file"
            );
            return Ok(());
        }
        return Err(ServerError::Usage);
    }
    if first == "worker" {
        let action = arguments.next().ok_or(ServerError::Usage)?;
        let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
        let worker_id = parse_argument::<WorkerId>(arguments.next())?;
        let config = load_config(&config_path)?;
        if action == "disable" || action == "enable" {
            let command_id = parse_argument::<CommandId>(arguments.next())?;
            if arguments.next().is_some() {
                return Err(ServerError::Usage);
            }
            let outcome = if action == "disable" {
                disable_enrolled_worker(&config, worker_id, &command_id)
            } else {
                enable_enrolled_worker(&config, worker_id, &command_id)
            }?;
            report_registry_mutation(
                if action == "disable" {
                    "disabled worker"
                } else {
                    "enabled worker"
                },
                outcome,
            );
            return Ok(());
        }
        if action == "set-pool" {
            let pool = WorkerPoolName::new(
                arguments
                    .next()
                    .ok_or(ServerError::Usage)?
                    .into_string()
                    .map_err(|_| ServerError::Usage)?,
            )
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
            let command_id = parse_argument::<CommandId>(arguments.next())?;
            if arguments.next().is_some() {
                return Err(ServerError::Usage);
            }
            return assign_enrolled_worker_pool(&config, worker_id, pool, &command_id)
                .map(|outcome| report_registry_mutation("assigned worker pool", outcome));
        }
        return Err(ServerError::Usage);
    }
    let config_path = PathBuf::from(first);
    if arguments.next().is_some() {
        return Err(ServerError::Usage);
    }
    run(load_config(&config_path)?).await
}

fn run_registry_command(arguments: &mut impl Iterator<Item = OsString>) -> Result<(), ServerError> {
    let action = arguments.next().ok_or(ServerError::Usage)?;
    let config_path = PathBuf::from(arguments.next().ok_or(ServerError::Usage)?);
    let config = load_config(&config_path)?;
    if action == "list" || action == "audit" {
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        return if action == "list" {
            write_json_stdout(&inspect_worker_registry(&config)?)
        } else {
            write_json_stdout(&audit_worker_registry(&config)?)
        };
    }
    if action == "show-worker" {
        let worker_id = parse_argument::<WorkerId>(arguments.next())?;
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        let inspection = inspect_worker_registry(&config)?;
        let worker = inspection
            .worker(worker_id)
            .ok_or_else(|| ServerError::RegistryEntryNotFound(format!("worker {worker_id}")))?;
        return write_json_stdout(worker);
    }
    if action == "show-credential" {
        let credential_id = parse_argument::<CredentialId>(arguments.next())?;
        if arguments.next().is_some() {
            return Err(ServerError::Usage);
        }
        let inspection = inspect_worker_registry(&config)?;
        let credential = inspection.credential(credential_id).ok_or_else(|| {
            ServerError::RegistryEntryNotFound(format!("credential {credential_id}"))
        })?;
        return write_json_stdout(credential);
    }
    Err(ServerError::Usage)
}

fn parse_argument<T>(value: Option<OsString>) -> Result<T, ServerError>
where
    T: std::str::FromStr,
{
    value
        .ok_or(ServerError::Usage)?
        .into_string()
        .map_err(|_| ServerError::Usage)?
        .parse()
        .map_err(|_| ServerError::Usage)
}

fn parse_nonzero(value: Option<OsString>) -> Result<NonZeroU64, ServerError> {
    NonZeroU64::new(parse_argument::<u64>(value)?).ok_or(ServerError::Usage)
}

fn report_registry_mutation(action: &str, outcome: RegistryMutationOutcome) {
    tracing::info!(
        target: "cairn.server.registry",
        event = "registry_mutation_completed",
        action,
        event_id = %outcome.event_id(),
        idempotent_replay = outcome.was_replay(),
        "registry mutation completed"
    );
}

fn write_json_stdout(value: &impl Serialize) -> Result<(), ServerError> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    bytes.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| ServerError::Startup(error.to_string()))
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

/// Reconstructs the canonical current worker and credential registry view.
///
/// The report contains no bearer secret, private key, certificate bytes, or unstable source path.
///
/// # Errors
///
/// Returns an error for invalid configuration, storage failure, or contradictory history.
pub fn inspect_worker_registry(
    config: &ServerConfig,
) -> Result<WorkerRegistryInspection, ServerError> {
    validate_registry_query(config)?;
    let events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    inspect_registry(&events, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Validates the complete registry history and returns a compact authority summary.
///
/// Invalid history fails instead of producing a partially trusted report.
///
/// # Errors
///
/// Returns an error for invalid configuration, storage failure, or contradictory history.
pub fn audit_worker_registry(config: &ServerConfig) -> Result<WorkerRegistryAudit, ServerError> {
    validate_registry_query(config)?;
    let events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    audit_registry(&events, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

fn validate_registry_query(config: &ServerConfig) -> Result<(), ServerError> {
    config.validate_schema()
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

/// Creates a one-shot rotation authority bound to one active predecessor credential.
///
/// # Errors
///
/// Returns an error for invalid configuration, inactive credentials, storage, entropy, or time.
pub fn create_rotation_bundle(
    config: &ServerConfig,
    predecessor_credential_id: CredentialId,
    ttl_ms: NonZeroU64,
) -> Result<EnrollmentBundle, ServerError> {
    config.validate()?;
    let service = config
        .enrollment_service
        .as_ref()
        .ok_or_else(|| ServerError::Configuration("enrollment_service is not configured".into()))?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    create_rotation_offer(
        &mut events,
        service,
        predecessor_credential_id,
        ttl_ms,
        service.rotation_overlap_ms,
        observed_now()?,
    )
    .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Permanently revokes one issued managed credential.
///
/// # Errors
///
/// Returns an error if the credential is unknown, already revoked, or storage fails.
pub fn revoke_worker_credential(
    config: &ServerConfig,
    credential_id: CredentialId,
    command_id: &CommandId,
) -> Result<RegistryMutationOutcome, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    revoke_credential(&mut events, credential_id, command_id, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Disables one managed logical worker independently of all of its credentials.
///
/// # Errors
///
/// Returns an error if the worker is unknown, already disabled, or storage fails.
pub fn disable_enrolled_worker(
    config: &ServerConfig,
    worker_id: WorkerId,
    command_id: &CommandId,
) -> Result<RegistryMutationOutcome, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    disable_worker(&mut events, worker_id, command_id, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Re-enables one explicitly disabled logical worker without changing its credentials or pool.
///
/// # Errors
///
/// Returns an error if the worker is unknown/not disabled, command input conflicts, or storage
/// fails.
pub fn enable_enrolled_worker(
    config: &ServerConfig,
    worker_id: WorkerId,
    command_id: &CommandId,
) -> Result<RegistryMutationOutcome, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    enable_worker(&mut events, worker_id, command_id, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Assigns a disabled worker to a new pool as an independent auditable lifecycle fact.
///
/// # Errors
///
/// Returns an error unless the worker is disabled and the pool changes, or for command/storage
/// failure.
pub fn assign_enrolled_worker_pool(
    config: &ServerConfig,
    worker_id: WorkerId,
    pool: WorkerPoolName,
    command_id: &CommandId,
) -> Result<RegistryMutationOutcome, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    assign_worker_pool(&mut events, worker_id, pool, command_id, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Permanently invalidates an unused one-shot enrollment authority.
///
/// # Errors
///
/// Returns an error if the authority is unknown, used, already revoked, or storage fails.
pub fn revoke_enrollment_authority(
    config: &ServerConfig,
    enrollment_id: EnrollmentId,
    command_id: &CommandId,
) -> Result<RegistryMutationOutcome, ServerError> {
    config.validate_schema()?;
    let mut events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    revoke_enrollment(&mut events, enrollment_id, command_id, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))
}

/// Runs the authenticated controller listener.
///
/// # Errors
///
/// Returns an error for invalid configuration, TLS/storage startup, or listener failure.
#[allow(
    clippy::too_many_lines,
    reason = "startup keeps listener, storage, enrollment, and App API lifetimes visibly composed"
)]
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    config.validate()?;
    let tls = config
        .tls
        .load()
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    let events = SqliteEventStore::open(&config.storage.event_database)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    EnrollmentRegistry::load(&events, observed_now()?)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
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
    #[cfg(feature = "migration-runtime")]
    if let Some(app_api_config) = config.app_api.clone() {
        let app_api_server = config.clone();
        tokio::spawn(async move {
            if let Err(error) = app_api::run_listener(app_api_server, app_api_config).await {
                tracing::error!(
                    target: "cairn.server.app-api",
                    event = "app_api_listener_failed",
                    error = %error,
                    "App API listener terminated"
                );
            }
        });
    }
    if let Some(service) = config.enrollment_service.clone() {
        let enrollment_tls_files = ServerTlsFiles {
            certificate: service.server_tls.certificate.clone(),
            private_key: service.server_tls.private_key.clone(),
            client_ca: config.tls.client_ca.clone(),
        };
        let enrollment_tls = enrollment_tls_files
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
        let enrollment_config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = enrollment_listener_loop(
                enrollment_listener,
                enrollment_tls,
                enrollment_state,
                issuer,
                enrollment_config,
            )
            .await
            {
                tracing::error!(
                    target: "cairn.server.enrollment",
                    event = "enrollment_listener_failed",
                    error = %error,
                    "enrollment listener terminated"
                );
            }
        });
    }
    let local_address = listener
        .local_addr()
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    tracing::info!(
        target: "cairn.server",
        event = "control_listener_ready",
        listen_address = %local_address,
        scheduler_enabled = config.scheduler.is_some(),
        enrollment_enabled = config.enrollment_service.is_some(),
        "controller control listener ready"
    );
    loop {
        let (tcp, _) = listener
            .accept()
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let session_config = config.clone();
        let session_tls = Arc::clone(&tls);
        let session_state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(error) = Box::pin(handle_connection(
                tcp,
                session_tls,
                session_state,
                session_config,
            ))
            .await
            {
                tracing::warn!(
                    target: "cairn.server.session",
                    event = "worker_connection_failed",
                    error = %error,
                    "worker connection terminated with an error"
                );
            }
        });
    }
}

impl ServerConfig {
    fn validate_schema(&self) -> Result<(), ServerError> {
        if self.schema_version != 1 {
            return Err(ServerError::Configuration(
                "only server schema_version 1 is supported".into(),
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ServerError> {
        self.validate_schema()?;
        #[cfg(feature = "migration-runtime")]
        if let Some(app_api) = &self.app_api {
            if !app_api.unix_socket.is_absolute() {
                return Err(ServerError::Configuration(
                    "App API unix_socket must be absolute".into(),
                ));
            }
            app_api.proposal_step.validate()?;
            app_api.intent_admission.validate(self)?;
            if app_api.oracle.documentation.trim() != app_api.oracle.documentation
                || app_api.oracle.documentation.is_empty()
                || app_api.oracle.build_and_tests.trim() != app_api.oracle.build_and_tests
                || app_api.oracle.build_and_tests.is_empty()
            {
                return Err(ServerError::Configuration(
                    "Oracle documentation and build/test context must be non-empty and trimmed"
                        .into(),
                ));
            }
        }
        if self.scheduler.as_ref().is_some_and(|scheduler| {
            scheduler.policy_version != SchedulerPolicyVersion::StableWorkerIdQuantitativeV1
        }) {
            return Err(ServerError::Configuration(
                "only scheduler policy stable-worker-id-quantitative-v1 is supported".into(),
            ));
        }
        if let Some(scheduler) = self.scheduler {
            validate_material_chunk_wire_size(
                self.transport,
                scheduler.assignment_material_chunk_size,
            )
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
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
            if service.control_endpoint.tcp_address.is_empty()
                || service.control_endpoint.websocket_uri.is_empty()
                || service.control_endpoint.server_name.is_empty()
            {
                return Err(ServerError::Configuration(
                    "enrollment control_endpoint must have a non-empty public endpoint".into(),
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
        #[cfg(feature = "migration-runtime")]
        if let Some(app_api) = &mut self.app_api {
            resolve(&mut app_api.unix_socket, base);
            app_api.proposal_step.resolve_paths(base);
            app_api.intent_admission.resolve_paths(base);
        }
        if let Some(service) = &mut self.enrollment_service {
            resolve(&mut service.server_ca, base);
            resolve(&mut service.server_tls.certificate, base);
            resolve(&mut service.server_tls.private_key, base);
            resolve(&mut service.control_endpoint.server_ca, base);
            resolve(&mut service.issuer_certificate, base);
            resolve(&mut service.issuer_private_key, base);
        }
        resolve(&mut self.storage.event_database, base);
        resolve(&mut self.storage.content_database, base);
        resolve(&mut self.storage.content_directory, base);
    }
}

async fn enrollment_listener_loop(
    listener: TcpListener,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<Mutex<ControllerState>>,
    issuer: Arc<EnrollmentIssuer>,
    config: ServerConfig,
) -> Result<(), ServerError> {
    let local_address = listener
        .local_addr()
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    tracing::info!(
        target: "cairn.server.enrollment",
        event = "enrollment_listener_ready",
        listen_address = %local_address,
        "controller enrollment listener ready"
    );
    loop {
        let (tcp, _) = listener
            .accept()
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let connection_tls = Arc::clone(&tls);
        let connection_state = Arc::clone(&state);
        let connection_issuer = Arc::clone(&issuer);
        let connection_config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = Box::pin(handle_enrollment_connection(
                tcp,
                connection_tls,
                connection_state,
                connection_issuer,
                connection_config,
            ))
            .await
            {
                tracing::warn!(
                    target: "cairn.server.enrollment",
                    event = "enrollment_connection_failed",
                    error = %error,
                    "enrollment connection terminated with an error"
                );
            }
        });
    }
}

async fn handle_enrollment_connection(
    tcp: tokio::net::TcpStream,
    tls: Arc<rustls::ServerConfig>,
    state: Arc<Mutex<ControllerState>>,
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
        EnrollmentError::Revoked => (
            EnrollmentRejectCode::InvalidAuthority,
            "enrollment authority was revoked",
        ),
        EnrollmentError::InvalidRequest(_) => (
            EnrollmentRejectCode::InvalidRequest,
            "enrollment request is invalid",
        ),
        EnrollmentError::Storage(_)
        | EnrollmentError::InvalidHistory(_)
        | EnrollmentError::CredentialNotActive
        | EnrollmentError::WorkerNotActive
        | EnrollmentError::WorkerNotDisabled
        | EnrollmentError::WorkerPoolUnchanged
        | EnrollmentError::EnrollmentAlreadyIssued
        | EnrollmentError::CommandConflict
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
    // The registry, rather than a startup snapshot, owns current credential and pool authority.
    // This makes administrative lifecycle changes visible to every new handshake immediately.
    let enrolled_worker = current_enrolled_worker(&state, fingerprint).await?;
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
    if !credential_is_authorized(&state, &enrolled_worker).await? {
        reject(
            &mut socket,
            config.transport,
            ControllerRejectCode::IdentityMismatch,
            "worker credential is revoked or worker is disabled",
            config.diagnostic_byte_limit,
        )
        .await;
        return Err(ServerError::Session(
            "worker credential is revoked or worker is disabled".into(),
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
    let now = admitted_resource_observation_time(
        observed_now()?,
        hello.resource_observation().observed_at(),
        config.resource_clock_skew_tolerance_ms,
    )?;
    let connection_id = ControlConnectionId::new();
    let subject = WorkerAuthenticationSubject::new(enrolled_worker.worker_id.to_string())
        .map_err(|error| ServerError::Session(error.to_string()))?;
    let mut session = {
        let mut locked = state.lock().await;
        let ControllerState { events, content } = &mut *locked;
        let registry = EnrollmentRegistry::load(events, now)
            .map_err(|error| ServerError::Session(error.to_string()))?;
        let enrolled_worker = registry
            .enrolled()
            .get(&fingerprint)
            .cloned()
            .filter(|worker| worker.worker_id == hello.worker_id())
            .ok_or_else(|| {
                ServerError::Session(
                    "worker credential lost registry authority during handshake".into(),
                )
            })?;
        synchronize_worker_pool_assignment(
            events,
            hello.worker_id(),
            &TrustedWorkerPoolAssignment::from_registry(
                enrolled_worker.pool.clone(),
                enrolled_worker.pool_assignment_revision,
            ),
            config.session_timeout_ms,
            &command("sync-worker-pool"),
            now,
        )
        .map_err(|error| ServerError::Session(error.to_string()))?;
        let mut authenticator = RecordedWorkerAuthenticator::new([(
            hello.worker_id(),
            AuthenticatedWorkerIdentity::new(
                subject,
                enrolled_worker.credential_id,
                enrolled_worker.pool.clone(),
            ),
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
        let registered = record_worker_resource_observation(
            events,
            content,
            &registered,
            hello.resource_observation(),
            &command("hello-resources"),
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

    tracing::info!(
        target: "cairn.server.session",
        event = "worker_session_registered",
        worker_id = %session.worker_id(),
        connection_id = %connection_id,
        incarnation_id = %session.incarnation_id(),
        pool = %session.pool().as_str(),
        available_slots = availability.available_slots(),
        active_attempts = availability.active_attempts().len(),
        "authenticated worker session registered"
    );

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
    tracing::info!(
        target: "cairn.server.session",
        event = "worker_session_disconnected",
        worker_id = %session.worker_id(),
        connection_id = %connection_id,
        clean = outcome.is_ok(),
        "worker session disconnected and durable state was updated"
    );
    outcome
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
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
        if !session_credential_is_authorized(state, session).await? {
            return Err(ServerError::Session(
                "worker credential was revoked or worker was disabled".into(),
            ));
        }
        flush_controller(
            socket,
            state,
            config,
            connection_id,
            session.worker_id(),
            inbound.acknowledge_through(),
            highest_sent,
            acknowledgement_sent,
        )
        .await?;
        let read = timeout_at_optional(
            idle_deadline,
            read_wire_message::<_, WorkerWireMessage>(socket, config.transport),
        );
        let poll_ms = config
            .outbox_poll_interval_ms
            .map_or(config.authority_poll_interval_ms.get(), |outbox| {
                outbox.get().min(config.authority_poll_interval_ms.get())
            });
        let incoming = tokio::select! {
            message = read => Some(message),
            () = tokio::time::sleep(Duration::from_millis(poll_ms)) => None,
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
                tracing::debug!(
                    target: "cairn.server.session",
                    event = "worker_heartbeat_accepted",
                    worker_id = %session.worker_id(),
                    connection_id = %connection_id,
                    health = ?availability.health(),
                    draining = availability.draining(),
                    available_slots = availability.available_slots(),
                    active_attempts = availability.active_attempts().len(),
                    "worker heartbeat accepted"
                );
            }
            WorkerWireMessage::ResourcesObserved { observation } => {
                let now = admitted_resource_observation_time(
                    observed_now()?,
                    observation.observed_at(),
                    config.resource_clock_skew_tolerance_ms,
                )?;
                let observation_id = {
                    let mut locked = state.lock().await;
                    let ControllerState { events, content } = &mut *locked;
                    *session = record_worker_resource_observation(
                        events,
                        content,
                        session,
                        &observation,
                        &command("resource-refresh"),
                        now,
                    )
                    .map_err(|error| ServerError::Session(error.to_string()))?;
                    session.resource_observation_id()
                };
                write_wire_message(
                    socket,
                    &ControllerWireMessage::ResourcesAccepted {
                        accepted_at: now,
                        observation_id,
                    },
                    config.transport,
                )
                .await
                .map_err(|error| ServerError::Session(error.to_string()))?;
                tracing::debug!(
                    target: "cairn.server.session",
                    event = "worker_resources_accepted",
                    worker_id = %session.worker_id(),
                    connection_id = %connection_id,
                    observation_id = %observation_id,
                    "worker resource refresh accepted"
                );
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
                    inbound.acknowledge_through(),
                    highest_sent,
                    acknowledgement_sent,
                )
                .await?;
            }
            WorkerWireMessage::MaterialChunkRequest { request } => {
                let chunk = {
                    let locked = state.lock().await;
                    read_assignment_material_chunk(
                        &locked.events,
                        &locked.content,
                        session.worker_id(),
                        &request,
                    )
                    .map_err(|error| ServerError::Session(error.to_string()))?
                };
                write_wire_message(
                    socket,
                    &ControllerWireMessage::MaterialChunk { chunk },
                    config.transport,
                )
                .await
                .map_err(|error| ServerError::Session(error.to_string()))?;
            }
            WorkerWireMessage::Hello { .. } => {
                return Err(ServerError::Session("hello repeated after welcome".into()));
            }
        }
    }
}

async fn current_enrolled_worker(
    state: &Arc<Mutex<ControllerState>>,
    fingerprint: CertificateFingerprint,
) -> Result<Option<EnrolledWorker>, ServerError> {
    let locked = state.lock().await;
    let registry = EnrollmentRegistry::load(&locked.events, observed_now()?)
        .map_err(|error| ServerError::Session(error.to_string()))?;
    Ok(registry.enrolled().get(&fingerprint).cloned())
}

async fn credential_is_authorized(
    state: &Arc<Mutex<ControllerState>>,
    worker: &EnrolledWorker,
) -> Result<bool, ServerError> {
    let locked = state.lock().await;
    let registry = EnrollmentRegistry::load(&locked.events, observed_now()?)
        .map_err(|error| ServerError::Session(error.to_string()))?;
    Ok(registry.credential_is_authorized(worker.credential_id, worker.worker_id))
}

async fn session_credential_is_authorized(
    state: &Arc<Mutex<ControllerState>>,
    session: &RegisteredWorkerSession,
) -> Result<bool, ServerError> {
    let locked = state.lock().await;
    let registry = EnrollmentRegistry::load(&locked.events, observed_now()?)
        .map_err(|error| ServerError::Session(error.to_string()))?;
    Ok(registry.credential_is_authorized(session.credential_id(), session.worker_id()))
}

#[expect(
    clippy::too_many_lines,
    reason = "worker message validation, durable transition, and correlated lifecycle logging remain one linear trust boundary"
)]
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
            tracing::info!(
                target: "cairn.server.assignment",
                event = "assignment_acceptance_received",
                worker_id = %session.worker_id(),
                connection_id = %connection_id,
                assignment_id = %binding.assignment_id(),
                job_id = %binding.job_id(),
                attempt_id = %binding.attempt_id(),
                "controller received worker assignment acceptance"
            );
            if binding.worker_id() != session.worker_id()
                || binding.worker_incarnation_id() != session.incarnation_id()
            {
                return Err(ServerError::Session(
                    "assignment acceptance claimant differs from the authenticated session".into(),
                ));
            }
            let assignment =
                recover_execution_assignment(events, content, binding.attempt_id(), now)
                    .map_err(|error| ServerError::Session(error.to_string()))?;
            match assignment {
                ExecutionAssignmentState::Leased(lease) => {
                    let accepted = accept_worker_assignment(
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
                    start_and_enqueue_assignment(events, content, accepted, session, config, now)?;
                }
                ExecutionAssignmentState::Accepted(accepted) => {
                    ensure_binding(accepted.lease().binding(), binding)?;
                    start_and_enqueue_assignment(events, content, accepted, session, config, now)?;
                }
                ExecutionAssignmentState::Running { lease } => {
                    ensure_binding(lease.binding(), binding)?;
                    enqueue_assignment_start(events, &lease, session, now)?;
                }
                ExecutionAssignmentState::ExpiredBeforeStart { lease }
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
        WorkerControlMessage::ExecutionResult { binding, .. } => {
            let result: WorkerResultReconciliation = reconcile_worker_result(
                events,
                content,
                session.worker_id(),
                message,
                &command("result"),
                now,
            )
            .map_err(|error| ServerError::Session(error.to_string()))?;
            let reconciliation = match &result {
                WorkerResultReconciliation::Published(completion) => match completion.as_ref() {
                    ExecutionCompletion::Completed { .. } => "published_completed",
                    ExecutionCompletion::NotStarted { .. } => "published_not_started",
                    ExecutionCompletion::Ambiguous { .. } => "published_ambiguous",
                },
                WorkerResultReconciliation::AlreadyTerminal => "already_terminal",
            };
            tracing::info!(
                target: "cairn.server.assignment",
                event = "execution_result_reconciled",
                worker_id = %session.worker_id(),
                connection_id = %connection_id,
                assignment_id = %binding.assignment_id(),
                job_id = %binding.job_id(),
                attempt_id = %binding.attempt_id(),
                reconciliation,
                "controller reconciled worker execution result"
            );
        }
    }
    Ok(())
}

fn start_and_enqueue_assignment(
    events: &mut SqliteEventStore,
    content: &SqliteContentStore,
    accepted: AcceptedExecutionAssignment,
    session: &RegisteredWorkerSession,
    config: &ServerConfig,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), ServerError> {
    let start = execution_start_message(accepted.lease());
    start_accepted_assignment(
        events,
        content,
        accepted,
        session,
        config.session_timeout_ms,
        &command("start-attempt"),
        observed_at,
    )
    .map_err(|error| ServerError::Session(error.to_string()))?;
    enqueue_controller_message(
        events,
        session.worker_id(),
        &start,
        &command("enqueue-start"),
        observed_at,
    )
    .map_err(|error| ServerError::Session(error.to_string()))?;
    Ok(())
}

fn enqueue_assignment_start(
    events: &mut SqliteEventStore,
    lease: &AssignmentLeaseRecord,
    session: &RegisteredWorkerSession,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), ServerError> {
    enqueue_controller_message(
        events,
        session.worker_id(),
        &execution_start_message(lease),
        &command("recover-start-outbox"),
        observed_at,
    )
    .map_err(|error| ServerError::Session(error.to_string()))?;
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

fn admitted_resource_observation_time(
    controller_time: ObservedAtUnixMillis,
    worker_time: ObservedAtUnixMillis,
    tolerance_ms: Option<NonZeroU64>,
) -> Result<ObservedAtUnixMillis, ServerError> {
    if worker_time <= controller_time {
        return Ok(controller_time);
    }
    let lead_ms = worker_time
        .get()
        .checked_sub(controller_time.get())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| ServerError::Session("worker resource clock lead overflowed".into()))?;
    if tolerance_ms.is_some_and(|tolerance| lead_ms <= tolerance.get()) {
        Ok(worker_time)
    } else {
        Err(ServerError::Session(format!(
            "worker resource clock leads controller by {lead_ms} ms"
        )))
    }
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
    use std::num::NonZeroU64;

    use cairn_protocol::ObservedAtUnixMillis;

    use super::{ServerConfig, admitted_resource_observation_time, write_new_secret_file};

    #[test]
    fn resource_clock_lead_requires_an_explicit_bound() {
        let controller = ObservedAtUnixMillis::new(1_000);
        let worker = ObservedAtUnixMillis::new(1_250);
        assert!(admitted_resource_observation_time(controller, worker, None).is_err());
        assert_eq!(
            admitted_resource_observation_time(controller, worker, NonZeroU64::new(250),)
                .expect("lead at configured boundary"),
            worker
        );
        assert!(
            admitted_resource_observation_time(controller, worker, NonZeroU64::new(249),).is_err()
        );
        assert_eq!(
            admitted_resource_observation_time(controller, ObservedAtUnixMillis::new(900), None,)
                .expect("worker behind controller"),
            controller
        );
    }

    #[test]
    fn documented_configuration_is_strictly_decodable() {
        let config: ServerConfig =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("documented server configuration");
        assert!(config.scheduler.is_some());
    }

    #[test]
    fn scheduler_can_be_disabled_or_omitted_but_enabled_durations_are_positive() {
        let mut documented: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("documented JSON");
        documented["scheduler"] = serde_json::Value::Null;
        let disabled: ServerConfig =
            serde_json::from_value(documented.clone()).expect("disabled scheduler");
        assert!(disabled.scheduler.is_none());

        documented
            .as_object_mut()
            .expect("controller object")
            .remove("scheduler");
        let omitted: ServerConfig =
            serde_json::from_value(documented).expect("omitted scheduler configuration");
        assert!(omitted.scheduler.is_none());

        let mut invalid: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("documented JSON");
        invalid["scheduler"]["assignment_lease_duration_ms"] = 0.into();
        assert!(serde_json::from_value::<ServerConfig>(invalid).is_err());

        let mut invalid_retry: serde_json::Value =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("documented JSON");
        invalid_retry["scheduler"]["optimistic_retry_limit"] = 0.into();
        assert!(serde_json::from_value::<ServerConfig>(invalid_retry).is_err());
    }

    #[test]
    fn only_schema_version_one_is_accepted() {
        let documented = include_str!("../../../config/controller.example.json");
        let unsupported: ServerConfig = serde_json::from_str(
            &documented.replace("\"schema_version\": 1", "\"schema_version\": 99"),
        )
        .expect("schema field remains structurally decodable");
        assert!(unsupported.validate_schema().is_err());
    }

    #[test]
    fn rotation_overlap_can_be_explicitly_disabled() {
        let documented = include_str!("../../../config/controller.example.json");
        let disabled = documented.replace(
            "\"rotation_overlap_ms\": 300000",
            "\"rotation_overlap_ms\": null",
        );
        let config: ServerConfig =
            serde_json::from_str(&disabled).expect("disabled rotation overlap configuration");
        assert!(
            config
                .enrollment_service
                .expect("enrollment service")
                .rotation_overlap_ms
                .is_none()
        );
    }

    #[test]
    fn missing_optional_rotation_overlap_disables_retirement() {
        let documented = include_str!("../../../config/controller.example.json");
        let omitted = documented.replace("    \"rotation_overlap_ms\": 300000,\n", "");
        let config: ServerConfig =
            serde_json::from_str(&omitted).expect("omitted optional rotation overlap");
        assert!(
            config
                .enrollment_service
                .expect("enrollment service")
                .rotation_overlap_ms
                .is_none()
        );
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
