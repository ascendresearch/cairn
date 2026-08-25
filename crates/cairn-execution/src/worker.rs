use std::{collections::BTreeMap, io::Cursor};

use cairn_protocol::{
    AggregateId, AggregateKind, AttemptId, CommandId, ContentId, ContentType, EventId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StreamRevision, WorkerId, WorkerIncarnationId,
};
use cairn_record::{
    ContentStore, ContentStoreError, EventEnvelope, EventStore, EventStoreError, ExpectedRevision,
    NewEvent, StreamId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    CapabilityRequirement, ExecutionBackend, ExecutionPlatform, JobContract, WorkerPoolName,
};

const WORKER_REGISTERED: &str = "execution.worker-registered";
const WORKER_REPLACED: &str = "execution.worker-replaced-after-expiry";
const WORKER_HEARTBEAT: &str = "execution.worker-heartbeat";
const WORKER_DISCONNECTED: &str = "execution.worker-disconnected";

macro_rules! worker_label {
    ($(#[$meta:meta])* $name:ident, $error:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Creates a validated worker-control label.
            ///
            /// # Errors
            ///
            /// Rejects empty, untrimmed, or control-containing values.
            pub fn new(value: impl Into<String>) -> Result<Self, WorkerValueError> {
                let value = value.into();
                if value.is_empty()
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(WorkerValueError::$error);
                }
                Ok(Self(value))
            }

            /// Returns the validated value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = WorkerValueError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

worker_label!(
    /// Stable authentication principal bound to one logical worker.
    WorkerAuthenticationSubject,
    InvalidAuthenticationSubject
);
worker_label!(
    /// Immutable worker binary/build identity reported during registration.
    WorkerBinaryIdentity,
    InvalidBinaryIdentity
);

macro_rules! positive_worker_quantity {
    ($(#[$meta:meta])* $name:ident, $wire:ty, $error:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $name($wire);

        impl $name {
            /// Creates a positive worker-control quantity.
            ///
            /// # Errors
            ///
            /// Rejects zero.
            pub fn new(value: $wire) -> Result<Self, WorkerValueError> {
                if value == 0 {
                    Err(WorkerValueError::$error)
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the wire value.
            #[must_use]
            pub const fn get(self) -> $wire {
                self.0
            }
        }

        impl TryFrom<$wire> for $name {
            type Error = WorkerValueError;

            fn try_from(value: $wire) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for $wire {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(<$wire>::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

positive_worker_quantity!(
    /// Worker control-protocol version.
    WorkerProtocolVersion,
    u16,
    ZeroProtocolVersion
);
positive_worker_quantity!(
    /// Maximum parallel assignments admitted by one worker incarnation.
    WorkerSlotCount,
    u16,
    ZeroSlotCount
);
positive_worker_quantity!(
    /// Configurable worker-session liveness timeout.
    WorkerSessionTimeoutMillis,
    u64,
    ZeroSessionTimeout
);
positive_worker_quantity!(
    /// Configurable duration of one assignment lease or renewal.
    AssignmentLeaseDurationMillis,
    u64,
    ZeroLeaseDuration
);

/// Invalid worker-control value.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum WorkerValueError {
    /// Authentication principals have a conservative text boundary.
    #[error("worker authentication subject is invalid")]
    InvalidAuthenticationSubject,
    /// Binary identities have a conservative text boundary.
    #[error("worker binary identity is invalid")]
    InvalidBinaryIdentity,
    /// Protocol version zero is not a version.
    #[error("worker protocol version must be greater than zero")]
    ZeroProtocolVersion,
    /// A worker with no configured capacity cannot register as execution capacity.
    #[error("worker slot count must be greater than zero")]
    ZeroSlotCount,
    /// Session timeout must be explicitly positive.
    #[error("worker session timeout must be greater than zero")]
    ZeroSessionTimeout,
    /// Assignment lease duration must be explicitly positive.
    #[error("assignment lease duration must be greater than zero")]
    ZeroLeaseDuration,
    /// Resource claims must retain one canonical entry for each backend/capability.
    #[error("worker resource claims must be unique and in canonical order")]
    NonCanonicalResourceClaims,
    /// A hello cannot elevate its own resource claim to controller/external assurance.
    #[error("worker hello contains resource provenance it is not authorized to assert")]
    UnadmittedResourceProvenance,
    /// Active attempt snapshots must be a canonical set.
    #[error("worker active attempts must be unique and in canonical order")]
    NonCanonicalActiveAttempts,
    /// Dynamic availability exceeded the registered capacity.
    #[error("worker available slots exceed registered maximum concurrency")]
    SlotsExceedCapacity,
    /// Only the implemented V2 worker profile is accepted.
    #[error("worker profile schema version is unsupported")]
    UnsupportedProfileSchema,
}

/// Provenance of one static worker resource claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerResourceSource {
    /// Reported by a built-in Cairn platform/device probe.
    BuiltinProbe,
    /// Declared in deployment configuration without independent verification.
    OperatorDeclared,
    /// Checked by a controller challenge or trusted probe job.
    ControllerVerified,
    /// Supported by an external attestation authority.
    ExternalAttestation,
}

/// One resource value together with the evidence class that introduced it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResourceClaim<T> {
    value: T,
    source: WorkerResourceSource,
}

impl<T> WorkerResourceClaim<T> {
    /// Creates a resource claim without elevating its source.
    #[must_use]
    pub const fn new(value: T, source: WorkerResourceSource) -> Self {
        Self { value, source }
    }

    /// Returns the claimed resource value.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Returns how this claim was established.
    #[must_use]
    pub const fn source(&self) -> WorkerResourceSource {
        self.source
    }
}

/// Static resource inventory advertised by one worker incarnation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResourceInventory {
    platform: WorkerResourceClaim<ExecutionPlatform>,
    backends: Vec<WorkerResourceClaim<ExecutionBackend>>,
    capabilities: Vec<WorkerResourceClaim<CapabilityRequirement>>,
    max_concurrency: WorkerSlotCount,
}

impl WorkerResourceInventory {
    /// Creates a canonical static inventory.
    ///
    /// # Errors
    ///
    /// Rejects an empty/duplicate backend set or duplicate capability keys.
    pub fn new(
        platform: WorkerResourceClaim<ExecutionPlatform>,
        mut backends: Vec<WorkerResourceClaim<ExecutionBackend>>,
        mut capabilities: Vec<WorkerResourceClaim<CapabilityRequirement>>,
        max_concurrency: WorkerSlotCount,
    ) -> Result<Self, WorkerValueError> {
        backends.sort_by(|left, right| left.value.cmp(&right.value));
        capabilities.sort_by(|left, right| left.value.name.cmp(&right.value.name));
        let inventory = Self {
            platform,
            backends,
            capabilities,
            max_concurrency,
        };
        inventory.validate()?;
        Ok(inventory)
    }

    fn validate(&self) -> Result<(), WorkerValueError> {
        if self.backends.is_empty()
            || self
                .backends
                .windows(2)
                .any(|pair| pair[0].value >= pair[1].value)
            || self
                .capabilities
                .windows(2)
                .any(|pair| pair[0].value.name >= pair[1].value.name)
        {
            return Err(WorkerValueError::NonCanonicalResourceClaims);
        }
        Ok(())
    }

    /// Returns the exact native worker platform claim.
    #[must_use]
    pub const fn platform(&self) -> &WorkerResourceClaim<ExecutionPlatform> {
        &self.platform
    }

    /// Returns canonical backend claims.
    #[must_use]
    pub fn backends(&self) -> &[WorkerResourceClaim<ExecutionBackend>] {
        &self.backends
    }

    /// Returns canonical additional capability claims.
    #[must_use]
    pub fn capabilities(&self) -> &[WorkerResourceClaim<CapabilityRequirement>] {
        &self.capabilities
    }

    /// Returns maximum concurrent assignments.
    #[must_use]
    pub const fn max_concurrency(&self) -> WorkerSlotCount {
        self.max_concurrency
    }
}

/// Static, incarnation-scoped worker capabilities.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProfile {
    schema_version: u16,
    protocol_version: WorkerProtocolVersion,
    binary_identity: WorkerBinaryIdentity,
    resources: WorkerResourceInventory,
}

impl WorkerProfile {
    /// Creates a canonical static worker profile.
    ///
    /// # Errors
    ///
    /// Rejects duplicate backend names or capability keys.
    pub fn new(
        protocol_version: WorkerProtocolVersion,
        binary_identity: WorkerBinaryIdentity,
        resources: WorkerResourceInventory,
    ) -> Result<Self, WorkerValueError> {
        let profile = Self {
            schema_version: 2,
            protocol_version,
            binary_identity,
            resources,
        };
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), WorkerValueError> {
        if self.schema_version != 2 {
            return Err(WorkerValueError::UnsupportedProfileSchema);
        }
        self.resources.validate()
    }

    fn validate_advertised(&self) -> Result<(), WorkerValueError> {
        self.validate()?;
        if self.resources.platform.source != WorkerResourceSource::BuiltinProbe
            || self
                .resources
                .backends
                .iter()
                .any(|claim| claim.source != WorkerResourceSource::OperatorDeclared)
            || self
                .resources
                .capabilities
                .iter()
                .any(|claim| claim.source != WorkerResourceSource::OperatorDeclared)
        {
            return Err(WorkerValueError::UnadmittedResourceProvenance);
        }
        Ok(())
    }

    /// Returns the control-protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> WorkerProtocolVersion {
        self.protocol_version
    }

    /// Returns the immutable worker build identity.
    #[must_use]
    pub const fn binary_identity(&self) -> &WorkerBinaryIdentity {
        &self.binary_identity
    }

    /// Returns the immutable resource inventory.
    #[must_use]
    pub const fn resources(&self) -> &WorkerResourceInventory {
        &self.resources
    }

    /// Returns maximum concurrent assignments.
    #[must_use]
    pub const fn max_concurrency(&self) -> WorkerSlotCount {
        self.resources.max_concurrency
    }
}

/// Worker hello authenticated before durable registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerHello {
    worker_id: WorkerId,
    incarnation_id: WorkerIncarnationId,
    profile: WorkerProfile,
}

impl WorkerHello {
    /// Creates one worker hello without assigning trust to it.
    #[must_use]
    pub const fn new(
        worker_id: WorkerId,
        incarnation_id: WorkerIncarnationId,
        profile: WorkerProfile,
    ) -> Self {
        Self {
            worker_id,
            incarnation_id,
            profile,
        }
    }

    /// Returns the stable worker identity.
    #[must_use]
    pub const fn worker_id(&self) -> WorkerId {
        self.worker_id
    }

    /// Returns the process/boot incarnation identity.
    #[must_use]
    pub const fn incarnation_id(&self) -> WorkerIncarnationId {
        self.incarnation_id
    }

    /// Returns the untrusted advertised profile.
    #[must_use]
    pub const fn profile(&self) -> &WorkerProfile {
        &self.profile
    }
}

/// Authentication failure at the transport/security adapter boundary.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("worker authentication failed: {0}")]
pub struct WorkerAuthenticationError(pub String);

/// Controller-authoritative identity and pool membership established during authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedWorkerIdentity {
    subject: WorkerAuthenticationSubject,
    pool: WorkerPoolName,
}

impl AuthenticatedWorkerIdentity {
    /// Binds an authenticated principal to an operator-owned scheduling pool.
    #[must_use]
    pub const fn new(subject: WorkerAuthenticationSubject, pool: WorkerPoolName) -> Self {
        Self { subject, pool }
    }

    /// Returns the authenticated principal.
    #[must_use]
    pub const fn subject(&self) -> &WorkerAuthenticationSubject {
        &self.subject
    }

    /// Returns the authenticated, controller-authorized worker pool.
    #[must_use]
    pub const fn pool(&self) -> &WorkerPoolName {
        &self.pool
    }
}

/// Replaceable verifier for mTLS enrollment, local development credentials, or future mechanisms.
pub trait WorkerAuthenticator {
    /// Verifies a hello and returns the stable principal proven by the transport.
    ///
    /// # Errors
    ///
    /// Rejects unknown, revoked, or mismatched credentials.
    fn authenticate(
        &mut self,
        hello: &WorkerHello,
    ) -> Result<AuthenticatedWorkerIdentity, WorkerAuthenticationError>;
}

/// Deterministic authenticator mapping worker identities to enrolled principals.
pub struct RecordedWorkerAuthenticator {
    identities: BTreeMap<WorkerId, AuthenticatedWorkerIdentity>,
}

impl RecordedWorkerAuthenticator {
    /// Creates a deterministic enrollment fixture.
    pub fn new(
        identities: impl IntoIterator<Item = (WorkerId, AuthenticatedWorkerIdentity)>,
    ) -> Self {
        Self {
            identities: identities.into_iter().collect(),
        }
    }
}

impl WorkerAuthenticator for RecordedWorkerAuthenticator {
    fn authenticate(
        &mut self,
        hello: &WorkerHello,
    ) -> Result<AuthenticatedWorkerIdentity, WorkerAuthenticationError> {
        self.identities
            .get(&hello.worker_id)
            .cloned()
            .ok_or_else(|| WorkerAuthenticationError("worker is not enrolled".to_owned()))
    }
}

/// Dynamic health reported by a worker heartbeat.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerHealth {
    /// Worker admits new compatible work.
    Ready,
    /// Worker remains observable but does not admit new work.
    Degraded,
    /// Worker cannot currently service assignments.
    Unavailable,
}

/// Dynamic availability kept separate from static capability claims.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerAvailability {
    health: WorkerHealth,
    draining: bool,
    available_slots: u16,
    active_attempts: Vec<AttemptId>,
}

impl WorkerAvailability {
    /// Creates a canonical heartbeat snapshot.
    ///
    /// # Errors
    ///
    /// Rejects duplicate active attempt identities.
    pub fn new(
        health: WorkerHealth,
        draining: bool,
        available_slots: u16,
        mut active_attempts: Vec<AttemptId>,
    ) -> Result<Self, WorkerValueError> {
        active_attempts.sort();
        if active_attempts.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(WorkerValueError::NonCanonicalActiveAttempts);
        }
        Ok(Self {
            health,
            draining,
            available_slots,
            active_attempts,
        })
    }

    fn validate(&self, profile: &WorkerProfile) -> Result<(), WorkerValueError> {
        if self
            .active_attempts
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(WorkerValueError::NonCanonicalActiveAttempts);
        }
        if self.available_slots > profile.max_concurrency().get() {
            return Err(WorkerValueError::SlotsExceedCapacity);
        }
        Ok(())
    }

    /// Returns current health.
    #[must_use]
    pub const fn health(&self) -> WorkerHealth {
        self.health
    }

    /// Returns whether the worker refuses new assignments intentionally.
    #[must_use]
    pub const fn draining(&self) -> bool {
        self.draining
    }

    /// Returns currently available concurrency slots.
    #[must_use]
    pub const fn available_slots(&self) -> u16 {
        self.available_slots
    }

    /// Returns the worker's advisory view of active attempts.
    #[must_use]
    pub fn active_attempts(&self) -> &[AttemptId] {
        &self.active_attempts
    }
}

/// Immutable content domain for canonical worker profiles.
pub struct WorkerProfileArtifact;
impl ContentType for WorkerProfileArtifact {
    const DOMAIN: &'static str = "execution.worker-profile.v2";
}

/// Immutable content domain for dynamic availability snapshots.
pub struct WorkerAvailabilityArtifact;
impl ContentType for WorkerAvailabilityArtifact {
    const DOMAIN: &'static str = "execution.worker-availability.v1";
}

/// Verified live worker session reconstructed from facts and CAS.
#[derive(Clone, Debug)]
pub struct RegisteredWorkerSession {
    worker_id: WorkerId,
    incarnation_id: WorkerIncarnationId,
    authentication_subject: WorkerAuthenticationSubject,
    pool: WorkerPoolName,
    profile_id: ContentId<WorkerProfileArtifact>,
    profile: WorkerProfile,
    availability_id: Option<ContentId<WorkerAvailabilityArtifact>>,
    availability: Option<WorkerAvailability>,
    last_seen_at: ObservedAtUnixMillis,
}

impl RegisteredWorkerSession {
    /// Returns the stable worker identity.
    #[must_use]
    pub const fn worker_id(&self) -> WorkerId {
        self.worker_id
    }

    /// Returns the current process/boot incarnation.
    #[must_use]
    pub const fn incarnation_id(&self) -> WorkerIncarnationId {
        self.incarnation_id
    }

    /// Returns the authenticated principal bound to this stable worker.
    #[must_use]
    pub const fn authentication_subject(&self) -> &WorkerAuthenticationSubject {
        &self.authentication_subject
    }

    /// Returns the controller-authorized worker pool.
    #[must_use]
    pub const fn pool(&self) -> &WorkerPoolName {
        &self.pool
    }

    /// Returns the exact static profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> ContentId<WorkerProfileArtifact> {
        self.profile_id
    }

    /// Returns the verified static profile.
    #[must_use]
    pub const fn profile(&self) -> &WorkerProfile {
        &self.profile
    }

    /// Returns the latest availability identity, if a heartbeat has committed.
    #[must_use]
    pub const fn availability_id(&self) -> Option<ContentId<WorkerAvailabilityArtifact>> {
        self.availability_id
    }

    /// Returns the latest verified availability, if present.
    #[must_use]
    pub const fn availability(&self) -> Option<&WorkerAvailability> {
        self.availability.as_ref()
    }

    /// Returns the registration/heartbeat observation used for liveness.
    #[must_use]
    pub const fn last_seen_at(&self) -> ObservedAtUnixMillis {
        self.last_seen_at
    }
}

/// Reconstructed worker state at an explicit observed time.
pub enum WorkerSessionState {
    /// The current incarnation is within its configured liveness window.
    Live(Box<RegisteredWorkerSession>),
    /// No worker facts exist.
    NotFound,
    /// The current incarnation explicitly disconnected.
    Disconnected {
        /// Last registered incarnation.
        incarnation_id: WorkerIncarnationId,
    },
    /// The current incarnation exceeded the caller-supplied liveness timeout.
    Expired {
        /// Last registered incarnation.
        incarnation_id: WorkerIncarnationId,
        /// Exact first millisecond at which the session is no longer live.
        expired_at: ObservedAtUnixMillis,
    },
}

/// Capability or availability reason that prevents placement.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum WorkerMatchFailure {
    /// No heartbeat has established dynamic availability.
    #[error("worker has no durable availability heartbeat")]
    MissingAvailability,
    /// The authenticated worker pool is outside the request's allow-list.
    #[error("worker pool {0} is not allowed by the placement request")]
    Pool(String),
    /// The native worker architecture differs from the placement request.
    #[error("worker architecture does not satisfy the placement request")]
    Architecture,
    /// The native worker operating system differs from the placement request.
    #[error("worker operating system does not satisfy the placement request")]
    OperatingSystem,
    /// The native worker target environment/ABI differs from the placement request.
    #[error("worker target environment does not satisfy the placement request")]
    TargetEnvironment,
    /// Worker does not advertise the requested execution backend.
    #[error("worker does not support backend {0}")]
    Backend(String),
    /// A required static capability is absent or has another value.
    #[error("worker does not satisfy capability {0}")]
    Capability(String),
    /// Worker health does not admit new assignments.
    #[error("worker health does not admit new assignments")]
    Health,
    /// Worker is intentionally draining.
    #[error("worker is draining")]
    Draining,
    /// Worker reports no free slot.
    #[error("worker has no available assignment slot")]
    NoCapacity,
}

/// Worker control-plane persistence or invariant failure.
#[derive(Debug, Error)]
pub enum WorkerControlError {
    /// Static/dynamic worker value validation failed.
    #[error(transparent)]
    Value(#[from] WorkerValueError),
    /// Transport authentication rejected the hello.
    #[error(transparent)]
    Authentication(#[from] WorkerAuthenticationError),
    /// CAS verification or archival failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// Event storage failed.
    #[error(transparent)]
    Event(#[from] EventStoreError),
    /// Stable identity is already owned by a different authenticated principal.
    #[error("worker identity is bound to another authentication subject")]
    AuthenticationSubjectChanged,
    /// Pool membership cannot change implicitly with a reconnect or process restart.
    #[error("worker pool changed without explicit reassignment")]
    WorkerPoolChanged,
    /// A different incarnation attempted to replace a session still inside its liveness window.
    #[error("worker {worker_id} already has live incarnation {live_incarnation}")]
    DuplicateLiveWorker {
        /// Stable worker identity.
        worker_id: WorkerId,
        /// Incarnation that still owns liveness.
        live_incarnation: WorkerIncarnationId,
    },
    /// The same incarnation changed its immutable profile.
    #[error("worker incarnation changed its immutable profile")]
    IncarnationProfileChanged,
    /// Heartbeat/disconnect came from a stale incarnation.
    #[error("worker control message came from a stale incarnation")]
    StaleIncarnation,
    /// Observed wall time regressed within one worker stream.
    #[error("worker observation time regressed")]
    ObservationTimeRegressed,
    /// Durable worker history is internally contradictory.
    #[error("invalid worker history: {0}")]
    InvalidHistory(String),
    /// Worker cannot accept the job contract.
    #[error(transparent)]
    Match(#[from] WorkerMatchFailure),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistrationPayload {
    worker_id: WorkerId,
    incarnation_id: WorkerIncarnationId,
    authentication_subject: WorkerAuthenticationSubject,
    pool: WorkerPoolName,
    profile_id: ContentId<WorkerProfileArtifact>,
    replaced_incarnation_id: Option<WorkerIncarnationId>,
    predecessor_expired_at: Option<ObservedAtUnixMillis>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "durable heartbeat schema keeps explicit typed identity field names"
)]
struct HeartbeatPayload {
    worker_id: WorkerId,
    incarnation_id: WorkerIncarnationId,
    availability_id: ContentId<WorkerAvailabilityArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DisconnectedPayload {
    worker_id: WorkerId,
    incarnation_id: WorkerIncarnationId,
}

struct WorkerProjection {
    incarnation_id: WorkerIncarnationId,
    authentication_subject: WorkerAuthenticationSubject,
    pool: WorkerPoolName,
    profile_id: ContentId<WorkerProfileArtifact>,
    availability_id: Option<ContentId<WorkerAvailabilityArtifact>>,
    last_seen_at: ObservedAtUnixMillis,
    disconnected: bool,
    last_event_id: EventId,
    revision: StreamRevision,
}

/// Authenticates and durably registers one worker incarnation.
///
/// A new incarnation cannot replace a live one. Once the previous incarnation is explicitly
/// disconnected or outside the configured timeout, replacement is one durable fact. The stable
/// authentication subject can never change implicitly.
///
/// # Errors
///
/// Returns an error for failed authentication, duplicate live identity, profile mutation, clock
/// regression, invalid history/content, or append failure.
pub fn register_worker<E: EventStore, C: ContentStore, A: WorkerAuthenticator>(
    events: &mut E,
    content: &mut C,
    authenticator: &mut A,
    hello: &WorkerHello,
    session_timeout: WorkerSessionTimeoutMillis,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<RegisteredWorkerSession, WorkerControlError> {
    hello.profile.validate_advertised()?;
    let authenticated = authenticator.authenticate(hello)?;
    let profile_bytes = cairn_codec::to_vec(&hello.profile)
        .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?;
    let profile_id = content
        .put::<WorkerProfileArtifact>(&mut Cursor::new(profile_bytes))?
        .content_id;
    let stream = worker_stream(hello.worker_id)?;
    let history = events.read_stream(&stream, None)?;
    let (expected, parent, schema, replaced_incarnation_id, predecessor_expired_at) =
        if history.is_empty() {
            (
                ExpectedRevision::NoStream,
                None,
                WORKER_REGISTERED,
                None,
                None,
            )
        } else {
            let projection = project_worker(&history, hello.worker_id)?;
            if projection.authentication_subject != *authenticated.subject() {
                return Err(WorkerControlError::AuthenticationSubjectChanged);
            }
            if projection.pool != *authenticated.pool() {
                return Err(WorkerControlError::WorkerPoolChanged);
            }
            ensure_nonregressing(observed_at, projection.last_seen_at)?;
            if !projection.disconnected
                && observed_at.get() < expiry_at(projection.last_seen_at, session_timeout)?.get()
            {
                if projection.incarnation_id != hello.incarnation_id {
                    return Err(WorkerControlError::DuplicateLiveWorker {
                        worker_id: hello.worker_id,
                        live_incarnation: projection.incarnation_id,
                    });
                }
                if projection.profile_id != profile_id {
                    return Err(WorkerControlError::IncarnationProfileChanged);
                }
                return materialize_session(content, hello.worker_id, projection);
            }
            if projection.disconnected {
                (
                    ExpectedRevision::Exact(projection.revision),
                    Some(projection.last_event_id),
                    WORKER_REGISTERED,
                    None,
                    None,
                )
            } else {
                (
                    ExpectedRevision::Exact(projection.revision),
                    Some(projection.last_event_id),
                    WORKER_REPLACED,
                    Some(projection.incarnation_id),
                    Some(expiry_at(projection.last_seen_at, session_timeout)?),
                )
            }
        };
    let event = fact(
        schema,
        2,
        parent,
        observed_at,
        &RegistrationPayload {
            worker_id: hello.worker_id,
            incarnation_id: hello.incarnation_id,
            authentication_subject: authenticated.subject().clone(),
            pool: authenticated.pool().clone(),
            profile_id,
            replaced_incarnation_id,
            predecessor_expired_at,
        },
    )?;
    let outcome = events.append(&stream, expected, command_id, &[event])?;
    let _event_id = only_event_id(&outcome.event_ids)?;
    Ok(RegisteredWorkerSession {
        worker_id: hello.worker_id,
        incarnation_id: hello.incarnation_id,
        authentication_subject: authenticated.subject,
        pool: authenticated.pool,
        profile_id,
        profile: hello.profile.clone(),
        availability_id: None,
        availability: None,
        last_seen_at: observed_at,
    })
}

/// Commits a dynamic availability heartbeat for the current live incarnation.
///
/// Heartbeat active-attempt claims are advisory. They do not start, complete, or reconcile an
/// execution attempt and they do not renew assignment leases by themselves.
///
/// # Errors
///
/// Returns an error for stale incarnation, invalid availability, clock regression, or storage
/// failure.
pub fn record_worker_heartbeat<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    session: &RegisteredWorkerSession,
    availability: &WorkerAvailability,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<RegisteredWorkerSession, WorkerControlError> {
    availability.validate(&session.profile)?;
    let bytes = cairn_codec::to_vec(availability)
        .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?;
    let availability_id = content
        .put::<WorkerAvailabilityArtifact>(&mut Cursor::new(bytes))?
        .content_id;
    let stream = worker_stream(session.worker_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_worker(&history, session.worker_id)?;
    ensure_current(session, &projection)?;
    if projection.availability_id == Some(availability_id) && projection.last_seen_at == observed_at
    {
        return materialize_session(content, session.worker_id, projection);
    }
    ensure_nonregressing(observed_at, projection.last_seen_at)?;
    let event = fact(
        WORKER_HEARTBEAT,
        1,
        Some(projection.last_event_id),
        observed_at,
        &HeartbeatPayload {
            worker_id: session.worker_id,
            incarnation_id: session.incarnation_id,
            availability_id,
        },
    )?;
    events.append(
        &stream,
        ExpectedRevision::Exact(projection.revision),
        command_id,
        &[event],
    )?;
    Ok(RegisteredWorkerSession {
        worker_id: session.worker_id,
        incarnation_id: session.incarnation_id,
        authentication_subject: session.authentication_subject.clone(),
        pool: session.pool.clone(),
        profile_id: session.profile_id,
        profile: session.profile.clone(),
        availability_id: Some(availability_id),
        availability: Some(availability.clone()),
        last_seen_at: observed_at,
    })
}

/// Durably closes the current worker incarnation.
///
/// # Errors
///
/// Returns an error for stale incarnation, clock regression, history contradiction, or append
/// failure.
pub fn disconnect_worker<E: EventStore>(
    events: &mut E,
    session: &RegisteredWorkerSession,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), WorkerControlError> {
    let stream = worker_stream(session.worker_id)?;
    let history = events.read_stream(&stream, None)?;
    let projection = project_worker(&history, session.worker_id)?;
    if projection.incarnation_id != session.incarnation_id
        || projection.authentication_subject != session.authentication_subject
        || projection.pool != session.pool
        || projection.profile_id != session.profile_id
    {
        return Err(WorkerControlError::StaleIncarnation);
    }
    if projection.disconnected {
        return Ok(());
    }
    ensure_nonregressing(observed_at, projection.last_seen_at)?;
    let event = fact(
        WORKER_DISCONNECTED,
        1,
        Some(projection.last_event_id),
        observed_at,
        &DisconnectedPayload {
            worker_id: session.worker_id,
            incarnation_id: session.incarnation_id,
        },
    )?;
    events.append(
        &stream,
        ExpectedRevision::Exact(projection.revision),
        command_id,
        &[event],
    )?;
    Ok(())
}

/// Reconstructs a worker session and evaluates liveness using an explicit configured timeout.
///
/// # Errors
///
/// Returns an error when event history or cited profile/availability artifacts are invalid.
pub fn recover_worker_session<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    worker_id: WorkerId,
    session_timeout: WorkerSessionTimeoutMillis,
    observed_at: ObservedAtUnixMillis,
) -> Result<WorkerSessionState, WorkerControlError> {
    let history = events.read_stream(&worker_stream(worker_id)?, None)?;
    if history.is_empty() {
        return Ok(WorkerSessionState::NotFound);
    }
    let projection = project_worker(&history, worker_id)?;
    ensure_nonregressing(observed_at, projection.last_seen_at)?;
    if projection.disconnected {
        return Ok(WorkerSessionState::Disconnected {
            incarnation_id: projection.incarnation_id,
        });
    }
    let expired_at = expiry_at(projection.last_seen_at, session_timeout)?;
    if observed_at.get() >= expired_at.get() {
        return Ok(WorkerSessionState::Expired {
            incarnation_id: projection.incarnation_id,
            expired_at,
        });
    }
    Ok(WorkerSessionState::Live(Box::new(materialize_session(
        content, worker_id, projection,
    )?)))
}

/// Matches a frozen job contract against static capability and dynamic availability.
///
/// # Errors
///
/// Returns the first deterministic reason the worker cannot accept the contract.
pub fn match_worker(
    session: &RegisteredWorkerSession,
    contract: &JobContract,
) -> Result<(), WorkerMatchFailure> {
    let placement = contract.resources().placement();
    if !placement.allowed_worker_pools().is_empty()
        && !placement.allowed_worker_pools().contains(&session.pool)
    {
        return Err(WorkerMatchFailure::Pool(session.pool.as_str().to_owned()));
    }
    let platform = session.profile.resources.platform.value();
    if placement
        .platform()
        .architecture()
        .is_some_and(|required| required != platform.architecture())
    {
        return Err(WorkerMatchFailure::Architecture);
    }
    if placement
        .platform()
        .operating_system()
        .is_some_and(|required| required != platform.operating_system())
    {
        return Err(WorkerMatchFailure::OperatingSystem);
    }
    if placement
        .platform()
        .target_environment()
        .is_some_and(|required| required != platform.target_environment())
    {
        return Err(WorkerMatchFailure::TargetEnvironment);
    }
    if !session
        .profile
        .resources
        .backends
        .iter()
        .any(|claim| claim.value == *contract.backend())
    {
        return Err(WorkerMatchFailure::Backend(
            contract.backend().as_str().to_owned(),
        ));
    }
    let capabilities = session
        .profile
        .resources
        .capabilities
        .iter()
        .map(|claim| (&claim.value.name, &claim.value.value))
        .collect::<BTreeMap<_, _>>();
    for required in placement.capabilities() {
        if capabilities.get(&required.name) != Some(&&required.value) {
            return Err(WorkerMatchFailure::Capability(
                required.name.as_str().to_owned(),
            ));
        }
    }
    let availability = session
        .availability
        .as_ref()
        .ok_or(WorkerMatchFailure::MissingAvailability)?;
    if availability.health != WorkerHealth::Ready {
        return Err(WorkerMatchFailure::Health);
    }
    if availability.draining {
        return Err(WorkerMatchFailure::Draining);
    }
    if availability.available_slots == 0 {
        return Err(WorkerMatchFailure::NoCapacity);
    }
    Ok(())
}

fn materialize_session<C: ContentStore>(
    content: &C,
    worker_id: WorkerId,
    projection: WorkerProjection,
) -> Result<RegisteredWorkerSession, WorkerControlError> {
    let profile: WorkerProfile = read_json(content, projection.profile_id)?;
    profile.validate()?;
    let availability = projection
        .availability_id
        .map(|id| read_json::<C, WorkerAvailabilityArtifact, WorkerAvailability>(content, id))
        .transpose()?;
    if let Some(value) = &availability {
        value.validate(&profile)?;
    }
    Ok(RegisteredWorkerSession {
        worker_id,
        incarnation_id: projection.incarnation_id,
        authentication_subject: projection.authentication_subject,
        pool: projection.pool,
        profile_id: projection.profile_id,
        profile,
        availability_id: projection.availability_id,
        availability,
        last_seen_at: projection.last_seen_at,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the event projector checks every versioned registration/liveness invariant linearly"
)]
fn project_worker(
    events: &[EventEnvelope],
    expected_worker_id: WorkerId,
) -> Result<WorkerProjection, WorkerControlError> {
    let mut projection: Option<WorkerProjection> = None;
    let mut bound_subject = None;
    let mut bound_pool = None;
    let mut previous = None;
    for event in events {
        if event.parent_event_id != previous {
            return invalid_history("worker event causal chain is invalid");
        }
        match event.schema_name.as_str() {
            WORKER_REGISTERED | WORKER_REPLACED => {
                if event.schema_version.get() != 2 {
                    return invalid_history("worker registration schema version is unsupported");
                }
                let payload: RegistrationPayload = decode(event)?;
                if payload.worker_id != expected_worker_id {
                    return invalid_history("worker event identity differs from its stream");
                }
                if bound_subject
                    .as_ref()
                    .is_some_and(|subject| subject != &payload.authentication_subject)
                {
                    return invalid_history("worker authentication subject changed in history");
                }
                if bound_pool
                    .as_ref()
                    .is_some_and(|pool| pool != &payload.pool)
                {
                    return invalid_history("worker pool changed in history");
                }
                if event.schema_name.as_str() == WORKER_REGISTERED
                    && projection.as_ref().is_some_and(|state| !state.disconnected)
                {
                    return invalid_history("worker registered over an active incarnation");
                }
                if event.schema_name.as_str() == WORKER_REGISTERED
                    && (payload.replaced_incarnation_id.is_some()
                        || payload.predecessor_expired_at.is_some())
                {
                    return invalid_history(
                        "ordinary worker registration carries replacement data",
                    );
                }
                if event.schema_name.as_str() == WORKER_REPLACED
                    && projection.as_ref().is_none_or(|state| {
                        state.disconnected
                            || payload.incarnation_id == state.incarnation_id
                            || payload.replaced_incarnation_id != Some(state.incarnation_id)
                            || payload.predecessor_expired_at.is_none_or(|expired_at| {
                                event.observed_at_unix_ms < expired_at.get()
                                    || expired_at <= state.last_seen_at
                            })
                    })
                {
                    return invalid_history(
                        "worker replacement has no verified expired predecessor",
                    );
                }
                bound_subject = Some(payload.authentication_subject.clone());
                bound_pool = Some(payload.pool.clone());
                projection = Some(WorkerProjection {
                    incarnation_id: payload.incarnation_id,
                    authentication_subject: payload.authentication_subject,
                    pool: payload.pool,
                    profile_id: payload.profile_id,
                    availability_id: None,
                    last_seen_at: ObservedAtUnixMillis::new(event.observed_at_unix_ms),
                    disconnected: false,
                    last_event_id: event.event_id,
                    revision: revision(event)?,
                });
            }
            WORKER_HEARTBEAT => {
                if event.schema_version.get() != 1 {
                    return invalid_history("worker heartbeat schema version is unsupported");
                }
                let payload: HeartbeatPayload = decode(event)?;
                let state = projection.as_mut().ok_or_else(|| {
                    WorkerControlError::InvalidHistory("heartbeat before registration".into())
                })?;
                if payload.worker_id != expected_worker_id
                    || payload.incarnation_id != state.incarnation_id
                    || state.disconnected
                    || event.observed_at_unix_ms < state.last_seen_at.get()
                {
                    return invalid_history("worker heartbeat contradicts current incarnation");
                }
                state.availability_id = Some(payload.availability_id);
                state.last_seen_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
                state.last_event_id = event.event_id;
                state.revision = revision(event)?;
            }
            WORKER_DISCONNECTED => {
                if event.schema_version.get() != 1 {
                    return invalid_history("worker disconnect schema version is unsupported");
                }
                let payload: DisconnectedPayload = decode(event)?;
                let state = projection.as_mut().ok_or_else(|| {
                    WorkerControlError::InvalidHistory("disconnect before registration".into())
                })?;
                if payload.worker_id != expected_worker_id
                    || payload.incarnation_id != state.incarnation_id
                    || state.disconnected
                    || event.observed_at_unix_ms < state.last_seen_at.get()
                {
                    return invalid_history("worker disconnect contradicts current incarnation");
                }
                state.disconnected = true;
                state.last_seen_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
                state.last_event_id = event.event_id;
                state.revision = revision(event)?;
            }
            _ => return invalid_history("unknown worker event schema"),
        }
        previous = Some(event.event_id);
    }
    projection.ok_or_else(|| WorkerControlError::InvalidHistory("missing registration".into()))
}

fn ensure_current(
    session: &RegisteredWorkerSession,
    projection: &WorkerProjection,
) -> Result<(), WorkerControlError> {
    if projection.incarnation_id != session.incarnation_id
        || projection.authentication_subject != session.authentication_subject
        || projection.pool != session.pool
        || projection.profile_id != session.profile_id
        || projection.disconnected
    {
        return Err(WorkerControlError::StaleIncarnation);
    }
    Ok(())
}

fn ensure_nonregressing(
    observed_at: ObservedAtUnixMillis,
    previous: ObservedAtUnixMillis,
) -> Result<(), WorkerControlError> {
    if observed_at < previous {
        Err(WorkerControlError::ObservationTimeRegressed)
    } else {
        Ok(())
    }
}

pub(crate) fn expiry_at(
    base: ObservedAtUnixMillis,
    duration: WorkerSessionTimeoutMillis,
) -> Result<ObservedAtUnixMillis, WorkerControlError> {
    let duration = i64::try_from(duration.get())
        .map_err(|_| WorkerControlError::InvalidHistory("session timeout exceeds i64".into()))?;
    base.get()
        .checked_add(duration)
        .map(ObservedAtUnixMillis::new)
        .ok_or_else(|| WorkerControlError::InvalidHistory("session expiry overflowed".into()))
}

fn worker_stream(worker_id: WorkerId) -> Result<StreamId, WorkerControlError> {
    Ok(StreamId {
        kind: AggregateKind::new("execution-worker")
            .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new(worker_id.to_string())
            .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?,
    })
}

fn fact<P: Serialize>(
    schema: &str,
    schema_version: u32,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, WorkerControlError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(schema_version)
            .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(event: &EventEnvelope) -> Result<T, WorkerControlError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))
}

fn revision(event: &EventEnvelope) -> Result<StreamRevision, WorkerControlError> {
    StreamRevision::new(event.sequence.get())
        .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))
}

fn read_json<C: ContentStore, T: ContentType, V: for<'de> Deserialize<'de>>(
    content: &C,
    id: ContentId<T>,
) -> Result<V, WorkerControlError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes)?;
    cairn_codec::from_slice(&bytes)
        .map_err(|error| WorkerControlError::InvalidHistory(error.to_string()))
}

fn only_event_id(event_ids: &[EventId]) -> Result<EventId, WorkerControlError> {
    if let [event_id] = event_ids {
        Ok(*event_id)
    } else {
        invalid_history("event store returned an invalid append outcome")
    }
}

fn invalid_history<T>(message: &str) -> Result<T, WorkerControlError> {
    Err(WorkerControlError::InvalidHistory(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{CommandId, ContentId, JobId};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::*;
    use crate::{
        ArchitectureName, CapturePolicy, CommandContract, DiagnosticByteLimit, EvidenceByteLimit,
        ExecutionEnvironmentArtifact, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
        InputBundleArtifact, NetworkPolicy, OperatingSystemName, OutputByteLimit, PlacementRequest,
        ResourceRequest, SandboxPath, TargetEnvironmentName,
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        content_database: std::path::PathBuf,
        event_database: std::path::PathBuf,
        cas: std::path::PathBuf,
        content: SqliteContentStore,
        events: SqliteEventStore,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let content_database = directory.path().join("content.db");
            let event_database = directory.path().join("events.db");
            let cas = directory.path().join("cas");
            let content = SqliteContentStore::open(&content_database, &cas).expect("content");
            let events = SqliteEventStore::open(&event_database).expect("events");
            Self {
                _directory: directory,
                content_database,
                event_database,
                cas,
                content,
                events,
            }
        }

        fn reopen(&mut self) {
            self.content = SqliteContentStore::open(&self.content_database, &self.cas)
                .expect("reopen content");
            self.events = SqliteEventStore::open(&self.event_database).expect("reopen events");
        }
    }

    fn profile(architecture: &str) -> WorkerProfile {
        WorkerProfile::new(
            WorkerProtocolVersion::new(1).expect("protocol"),
            WorkerBinaryIdentity::new("sha256:worker-v1").expect("binary"),
            WorkerResourceInventory::new(
                WorkerResourceClaim::new(
                    platform(architecture),
                    WorkerResourceSource::BuiltinProbe,
                ),
                vec![WorkerResourceClaim::new(
                    ExecutionBackend::new("container").expect("backend"),
                    WorkerResourceSource::OperatorDeclared,
                )],
                vec![WorkerResourceClaim::new(
                    CapabilityRequirement {
                        name: crate::CapabilityName::new("sandbox").expect("capability"),
                        value: crate::CapabilityValue::new("container").expect("value"),
                    },
                    WorkerResourceSource::OperatorDeclared,
                )],
                WorkerSlotCount::new(2).expect("slots"),
            )
            .expect("resources"),
        )
        .expect("profile")
    }

    fn platform(architecture: &str) -> ExecutionPlatform {
        ExecutionPlatform::new(
            ArchitectureName::new(architecture).expect("architecture"),
            OperatingSystemName::new("linux").expect("os"),
            TargetEnvironmentName::new("gnu").expect("environment"),
        )
    }

    fn contract(architecture: &str, pool: &str) -> JobContract {
        contract_for_platform(
            ExecutionPlatformRequirement::new(
                Some(ArchitectureName::new(architecture).expect("architecture")),
                None,
                None,
            ),
            pool,
        )
    }

    fn contract_for_platform(
        platform_requirement: ExecutionPlatformRequirement,
        pool: &str,
    ) -> JobContract {
        JobContract::new(
            JobId::new(),
            ContentId::<InputBundleArtifact>::derive(b"input").expect("input"),
            ContentId::<ExecutionEnvironmentArtifact>::derive(b"environment").expect("environment"),
            ExecutionBackend::new("container").expect("backend"),
            CommandContract::new(
                SandboxPath::new("bin/run").expect("program"),
                Vec::new(),
                SandboxPath::new("work").expect("working directory"),
            ),
            ResourceRequest::new(
                ExecutionTimeoutMillis::new(1_000).expect("timeout"),
                PlacementRequest::new(
                    platform_requirement,
                    vec![WorkerPoolName::new(pool).expect("pool")],
                    vec![CapabilityRequirement {
                        name: crate::CapabilityName::new("sandbox").expect("capability"),
                        value: crate::CapabilityValue::new("container").expect("value"),
                    }],
                )
                .expect("placement"),
            )
            .expect("resources"),
            NetworkPolicy::Disabled,
            CapturePolicy::new(
                OutputByteLimit::new(1024).expect("stdout"),
                OutputByteLimit::new(1024).expect("stderr"),
                DiagnosticByteLimit::new(1024).expect("diagnostic"),
                EvidenceByteLimit::new(4096).expect("evidence"),
                Vec::new(),
            )
            .expect("capture"),
        )
    }

    fn authenticator(worker_id: WorkerId, subject: &str) -> RecordedWorkerAuthenticator {
        RecordedWorkerAuthenticator::new([(
            worker_id,
            AuthenticatedWorkerIdentity::new(
                WorkerAuthenticationSubject::new(subject).expect("subject"),
                WorkerPoolName::new("fixture").expect("pool"),
            ),
        )])
    }

    #[test]
    fn authenticated_profile_and_heartbeat_survive_restart_and_match() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let hello = WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"));
        let mut auth = authenticator(worker_id, "spiffe://cairn/worker/one");
        let session = register_worker(
            &mut fixture.events,
            &mut fixture.content,
            &mut auth,
            &hello,
            WorkerSessionTimeoutMillis::new(100).expect("timeout"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(0),
        )
        .expect("register");
        assert_eq!(
            match_worker(&session, &contract("x86_64", "fixture")),
            Err(WorkerMatchFailure::MissingAvailability)
        );
        let available = WorkerAvailability::new(WorkerHealth::Ready, false, 2, Vec::new())
            .expect("availability");
        let session = record_worker_heartbeat(
            &mut fixture.events,
            &mut fixture.content,
            &session,
            &available,
            &CommandId::new(),
            ObservedAtUnixMillis::new(10),
        )
        .expect("heartbeat");
        assert!(match_worker(&session, &contract("x86_64", "fixture")).is_ok());
        assert!(matches!(
            match_worker(&session, &contract("aarch64", "fixture")),
            Err(WorkerMatchFailure::Architecture)
        ));
        assert!(matches!(
            match_worker(
                &session,
                &contract_for_platform(
                    ExecutionPlatformRequirement::new(
                        None,
                        Some(OperatingSystemName::new("other-os").expect("os")),
                        None,
                    ),
                    "fixture",
                ),
            ),
            Err(WorkerMatchFailure::OperatingSystem)
        ));
        assert!(matches!(
            match_worker(
                &session,
                &contract_for_platform(
                    ExecutionPlatformRequirement::new(
                        None,
                        None,
                        Some(TargetEnvironmentName::new("musl").expect("environment")),
                    ),
                    "fixture",
                ),
            ),
            Err(WorkerMatchFailure::TargetEnvironment)
        ));
        assert!(matches!(
            match_worker(&session, &contract("x86_64", "another-pool")),
            Err(WorkerMatchFailure::Pool(pool)) if pool == "fixture"
        ));

        fixture.reopen();
        let WorkerSessionState::Live(recovered) = recover_worker_session(
            &fixture.events,
            &fixture.content,
            worker_id,
            WorkerSessionTimeoutMillis::new(100).expect("timeout"),
            ObservedAtUnixMillis::new(50),
        )
        .expect("recover") else {
            panic!("live worker");
        };
        assert_eq!(recovered.profile_id(), session.profile_id());
        assert_eq!(recovered.pool().as_str(), "fixture");
        assert_eq!(
            recovered.profile().resources().platform().source(),
            WorkerResourceSource::BuiltinProbe
        );
        assert_eq!(recovered.availability(), Some(&available));
    }

    #[test]
    fn duplicate_live_identity_is_rejected_and_expired_incarnation_can_be_replaced() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let first_incarnation = WorkerIncarnationId::new();
        let hello = WorkerHello::new(worker_id, first_incarnation, profile("x86_64"));
        let mut auth = authenticator(worker_id, "spiffe://cairn/worker/one");
        let first = register_worker(
            &mut fixture.events,
            &mut fixture.content,
            &mut auth,
            &hello,
            WorkerSessionTimeoutMillis::new(100).expect("timeout"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(0),
        )
        .expect("first");
        let replacement =
            WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"));
        assert!(matches!(
            register_worker(
                &mut fixture.events,
                &mut fixture.content,
                &mut auth,
                &replacement,
                WorkerSessionTimeoutMillis::new(100).expect("timeout"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(99),
            ),
            Err(WorkerControlError::DuplicateLiveWorker { live_incarnation, .. })
                if live_incarnation == first_incarnation
        ));
        let replacement_session = register_worker(
            &mut fixture.events,
            &mut fixture.content,
            &mut auth,
            &replacement,
            WorkerSessionTimeoutMillis::new(100).expect("timeout"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(100),
        )
        .expect("replace expired");
        assert_ne!(replacement_session.incarnation_id(), first.incarnation_id());
        let availability = WorkerAvailability::new(WorkerHealth::Ready, false, 1, Vec::new())
            .expect("availability");
        assert!(matches!(
            record_worker_heartbeat(
                &mut fixture.events,
                &mut fixture.content,
                &first,
                &availability,
                &CommandId::new(),
                ObservedAtUnixMillis::new(101),
            ),
            Err(WorkerControlError::StaleIncarnation)
        ));
    }

    #[test]
    fn stable_worker_identity_cannot_change_authentication_subject() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let hello = WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"));
        let mut first_auth = authenticator(worker_id, "spiffe://cairn/worker/one");
        let _session = register_worker(
            &mut fixture.events,
            &mut fixture.content,
            &mut first_auth,
            &hello,
            WorkerSessionTimeoutMillis::new(10).expect("timeout"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(0),
        )
        .expect("register");
        let replacement =
            WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"));
        let mut changed_auth = authenticator(worker_id, "spiffe://cairn/worker/other");
        assert!(matches!(
            register_worker(
                &mut fixture.events,
                &mut fixture.content,
                &mut changed_auth,
                &replacement,
                WorkerSessionTimeoutMillis::new(10).expect("timeout"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(10),
            ),
            Err(WorkerControlError::AuthenticationSubjectChanged)
        ));
    }

    #[test]
    fn authenticated_pool_cannot_change_implicitly() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let hello = WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"));
        let mut first_auth = authenticator(worker_id, "spiffe://cairn/worker/one");
        register_worker(
            &mut fixture.events,
            &mut fixture.content,
            &mut first_auth,
            &hello,
            WorkerSessionTimeoutMillis::new(10).expect("timeout"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(0),
        )
        .expect("register");
        let mut moved = RecordedWorkerAuthenticator::new([(
            worker_id,
            AuthenticatedWorkerIdentity::new(
                WorkerAuthenticationSubject::new("spiffe://cairn/worker/one").expect("subject"),
                WorkerPoolName::new("another-pool").expect("pool"),
            ),
        )]);
        assert!(matches!(
            register_worker(
                &mut fixture.events,
                &mut fixture.content,
                &mut moved,
                &WorkerHello::new(worker_id, WorkerIncarnationId::new(), profile("x86_64"),),
                WorkerSessionTimeoutMillis::new(10).expect("timeout"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(10),
            ),
            Err(WorkerControlError::WorkerPoolChanged)
        ));
    }

    #[test]
    fn worker_hello_cannot_self_assert_verified_provenance() {
        let mut fixture = Fixture::new();
        let worker_id = WorkerId::new();
        let forged = WorkerProfile::new(
            WorkerProtocolVersion::new(1).expect("protocol"),
            WorkerBinaryIdentity::new("sha256:worker-v1").expect("binary"),
            WorkerResourceInventory::new(
                WorkerResourceClaim::new(
                    platform("x86_64"),
                    WorkerResourceSource::ControllerVerified,
                ),
                vec![WorkerResourceClaim::new(
                    ExecutionBackend::new("container").expect("backend"),
                    WorkerResourceSource::OperatorDeclared,
                )],
                Vec::new(),
                WorkerSlotCount::new(1).expect("slots"),
            )
            .expect("resources"),
        )
        .expect("profile structure");
        let mut auth = authenticator(worker_id, "spiffe://cairn/worker/one");
        assert!(matches!(
            register_worker(
                &mut fixture.events,
                &mut fixture.content,
                &mut auth,
                &WorkerHello::new(worker_id, WorkerIncarnationId::new(), forged),
                WorkerSessionTimeoutMillis::new(10).expect("timeout"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(0),
            ),
            Err(WorkerControlError::Value(
                WorkerValueError::UnadmittedResourceProvenance
            ))
        ));
    }
}
