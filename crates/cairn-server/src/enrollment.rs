use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::BufReader,
    num::NonZeroU64,
};

use cairn_control_transport::{
    CertificateFingerprint, EnrollmentBundle, EnrollmentEndpoint, EnrollmentPurpose,
    EnrollmentRequest, EnrollmentSecret, IssuedWorkerCredential,
};
use cairn_execution::WorkerPoolName;
use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, CredentialId, EnrollmentId, EventId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StreamRevision, WorkerId,
};
use cairn_record::{EventEnvelope, EventStore, ExpectedRevision, NewEvent, StreamId};
use rcgen::{
    Certificate, CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use time::OffsetDateTime;

use crate::{EnrolledWorker, EnrollmentServiceConfig};

const OFFER_CREATED: &str = "execution.worker-enrollment-offered";
const CREDENTIAL_ISSUED: &str = "execution.worker-credential-issued";
const CREDENTIAL_REVOKED: &str = "execution.worker-credential-revoked";
const STATIC_CREDENTIALS_IMPORTED: &str = "execution.worker-static-credentials-imported";
const WORKER_DISABLED: &str = "execution.worker-disabled";
const WORKER_ENABLED: &str = "execution.worker-enabled";
const WORKER_POOL_ASSIGNED: &str = "execution.worker-registry-pool-assigned";
const ENROLLMENT_REVOKED: &str = "execution.worker-enrollment-revoked";
const MAX_CSR_PEM_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OfferCreatedPayload {
    enrollment_id: EnrollmentId,
    token_digest: String,
    pool: WorkerPoolName,
    expires_at: ObservedAtUnixMillis,
    #[serde(default)]
    purpose: EnrollmentPurpose,
    #[serde(default)]
    rotation_overlap_ms: Option<NonZeroU64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialIssuedPayload {
    enrollment_id: EnrollmentId,
    csr_digest: String,
    credential: IssuedWorkerCredential,
    certificate_fingerprint: CertificateFingerprint,
    issued_at: ObservedAtUnixMillis,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialRevokedPayload {
    credential_id: CredentialId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StaticCredentialImport {
    pub(crate) worker_id: WorkerId,
    pub(crate) credential_id: CredentialId,
    pub(crate) pool: WorkerPoolName,
    pub(crate) certificate_fingerprint: CertificateFingerprint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StaticCredentialsImportedPayload {
    credentials: Vec<StaticCredentialImport>,
}

/// Durable result of importing one canonical legacy static-credential batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticEnrollmentImportOutcome {
    imported_credentials: usize,
    event_id: EventId,
    was_replay: bool,
}

impl StaticEnrollmentImportOutcome {
    /// Returns the number of credential bindings frozen by the import fact.
    #[must_use]
    pub const fn imported_credentials(self) -> usize {
        self.imported_credentials
    }

    /// Returns the exact registry fact retaining import provenance.
    #[must_use]
    pub const fn event_id(self) -> EventId {
        self.event_id
    }

    /// Returns whether the same explicit command had already committed this exact batch.
    #[must_use]
    pub const fn was_replay(self) -> bool {
        self.was_replay
    }
}

/// Durable result of one explicit registry lifecycle mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryMutationOutcome {
    event_id: EventId,
    was_replay: bool,
}

impl RegistryMutationOutcome {
    /// Returns the exact lifecycle fact authorized by the command.
    #[must_use]
    pub const fn event_id(self) -> EventId {
        self.event_id
    }

    /// Returns whether this call recovered an already committed exact command.
    #[must_use]
    pub const fn was_replay(self) -> bool {
        self.was_replay
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerDisabledPayload {
    worker_id: WorkerId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerEnabledPayload {
    worker_id: WorkerId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerPoolAssignedPayload {
    worker_id: WorkerId,
    previous_pool: WorkerPoolName,
    pool: WorkerPoolName,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentRevokedPayload {
    enrollment_id: EnrollmentId,
}

#[derive(Clone)]
struct Offer {
    token_digest: [u8; 32],
    pool: WorkerPoolName,
    expires_at: ObservedAtUnixMillis,
    purpose: EnrollmentPurpose,
    rotation_overlap_ms: Option<NonZeroU64>,
    issued: Option<Issued>,
    revoked: bool,
}

#[derive(Clone)]
struct Issued {
    csr_digest: [u8; 32],
    credential: IssuedWorkerCredential,
}

#[derive(Clone)]
struct CredentialRecord {
    fingerprint: CertificateFingerprint,
    enrolled: EnrolledWorker,
    provenance: CredentialProvenance,
    revoked: bool,
    superseded_by: Option<CredentialId>,
    retire_at: Option<ObservedAtUnixMillis>,
    predecessor: Option<CredentialId>,
}

#[derive(Clone)]
struct WorkerPoolAssignment {
    pool: WorkerPoolName,
    authority_revision: EventId,
}

#[derive(Clone, Copy)]
enum CredentialProvenance {
    Issued { enrollment_id: EnrollmentId },
    ImportedStatic { event_id: EventId },
}

pub(crate) struct EnrollmentRegistry {
    offers: BTreeMap<EnrollmentId, Offer>,
    credentials: BTreeMap<CredentialId, CredentialRecord>,
    worker_pools: BTreeMap<WorkerId, WorkerPoolAssignment>,
    disabled_workers: BTreeSet<WorkerId>,
    enrolled: BTreeMap<CertificateFingerprint, EnrolledWorker>,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    evaluated_at: ObservedAtUnixMillis,
}

pub(crate) struct EnrollmentIssuer {
    issuer: Certificate,
    issuer_key: KeyPair,
    issuer_pem: String,
    credential_validity_ms: NonZeroU64,
}

pub(crate) trait WorkerCredentialIssuer {
    fn issue(
        &self,
        csr_pem: &str,
        worker_id: WorkerId,
        credential_id: CredentialId,
        now: ObservedAtUnixMillis,
    ) -> Result<String, EnrollmentError>;
}

#[derive(Debug, Error)]
pub(crate) enum EnrollmentError {
    #[error("enrollment registry storage failed: {0}")]
    Storage(String),
    #[error("enrollment registry history is invalid: {0}")]
    InvalidHistory(String),
    #[error("enrollment authority is invalid")]
    InvalidAuthority,
    #[error("enrollment request is invalid: {0}")]
    InvalidRequest(String),
    #[error("enrollment authority has expired")]
    Expired,
    #[error("enrollment authority was already used by another key")]
    AlreadyUsed,
    #[error("enrollment authority was revoked")]
    Revoked,
    #[error("credential is unknown or already revoked")]
    CredentialNotActive,
    #[error("worker is unknown or already disabled")]
    WorkerNotActive,
    #[error("worker is unknown or not disabled")]
    WorkerNotDisabled,
    #[error("worker pool assignment does not change the current pool")]
    WorkerPoolUnchanged,
    #[error("issued enrollment authority cannot be revoked")]
    EnrollmentAlreadyIssued,
    #[error("static credential import conflicts with registry authority: {0}")]
    StaticImportConflict(String),
    #[error("registry command identity was already used for another operation")]
    CommandConflict,
    #[error("credential issuance failed: {0}")]
    Issuance(String),
}

impl EnrollmentRegistry {
    pub(crate) fn load(
        events: &impl EventStore,
        now: ObservedAtUnixMillis,
    ) -> Result<Self, EnrollmentError> {
        project(events, now)
    }

    pub(crate) fn enrolled(&self) -> &BTreeMap<CertificateFingerprint, EnrolledWorker> {
        &self.enrolled
    }

    pub(crate) fn credential_is_authorized(
        &self,
        credential_id: CredentialId,
        worker_id: WorkerId,
    ) -> bool {
        self.credentials.get(&credential_id).is_some_and(|record| {
            record.enrolled.worker_id == worker_id
                && record.is_authorized_at(&self.disabled_workers, self.evaluated_at)
        })
    }

    pub(crate) fn worker_ids(&self) -> BTreeSet<WorkerId> {
        self.credentials
            .values()
            .map(|record| record.enrolled.worker_id)
            .collect()
    }

    pub(crate) const fn last_event_id(&self) -> Option<EventId> {
        self.last_event_id
    }
}

impl CredentialRecord {
    fn is_authorized_at(
        &self,
        disabled_workers: &BTreeSet<WorkerId>,
        now: ObservedAtUnixMillis,
    ) -> bool {
        !self.revoked
            && !disabled_workers.contains(&self.enrolled.worker_id)
            && self.retire_at.is_none_or(|retire_at| now < retire_at)
    }
}

impl EnrollmentIssuer {
    pub(crate) fn load(config: &EnrollmentServiceConfig) -> Result<Self, EnrollmentError> {
        let issuer_pem = fs::read_to_string(&config.issuer_certificate)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
        let issuer_key_pem = fs::read_to_string(&config.issuer_private_key)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
        let issuer_key = KeyPair::from_pem(&issuer_key_pem)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
        if certificate_public_key(&issuer_pem)? != issuer_key.public_key_der() {
            return Err(EnrollmentError::Issuance(
                "issuer certificate does not match issuer private key".into(),
            ));
        }
        let params = CertificateParams::from_ca_cert_pem(&issuer_pem)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
        let issuer = params
            .self_signed(&issuer_key)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
        Ok(Self {
            issuer,
            issuer_key,
            issuer_pem,
            credential_validity_ms: config.credential_validity_ms,
        })
    }
}

impl WorkerCredentialIssuer for EnrollmentIssuer {
    fn issue(
        &self,
        csr_pem: &str,
        worker_id: WorkerId,
        credential_id: CredentialId,
        now: ObservedAtUnixMillis,
    ) -> Result<String, EnrollmentError> {
        let mut request = CertificateSigningRequestParams::from_pem(csr_pem)
            .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, worker_id.to_string());
        request.params.distinguished_name = distinguished_name;
        request.params.subject_alt_names.clear();
        request.params.is_ca = IsCa::NoCa;
        request.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        request.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        request.params.serial_number = Some(credential_id.as_uuid().as_bytes().to_vec().into());

        let not_before_ms = now.get().saturating_sub(60_000);
        let validity = i64::try_from(self.credential_validity_ms.get())
            .map_err(|_| EnrollmentError::Issuance("credential validity is too large".into()))?;
        let not_after_ms = now
            .get()
            .checked_add(validity)
            .ok_or_else(|| EnrollmentError::Issuance("credential validity overflow".into()))?;
        request.params.not_before = timestamp(not_before_ms)?;
        request.params.not_after = timestamp(not_after_ms)?;
        let leaf = request
            .signed_by(&self.issuer, &self.issuer_key)
            .map_err(|error| EnrollmentError::Issuance(error.to_string()))?
            .pem();
        Ok(format!("{}\n{}", leaf.trim_end(), self.issuer_pem.trim()))
    }
}

pub(crate) fn create_offer(
    events: &mut impl EventStore,
    config: &EnrollmentServiceConfig,
    pool: WorkerPoolName,
    ttl_ms: NonZeroU64,
    now: ObservedAtUnixMillis,
) -> Result<EnrollmentBundle, EnrollmentError> {
    let registry = project(events, now)?;
    let ttl = i64::try_from(ttl_ms.get())
        .map_err(|_| EnrollmentError::InvalidRequest("enrollment TTL is too large".into()))?;
    let expires_at = ObservedAtUnixMillis::new(
        now.get()
            .checked_add(ttl)
            .ok_or_else(|| EnrollmentError::InvalidRequest("enrollment TTL overflow".into()))?,
    );
    let enrollment_id = EnrollmentId::new();
    let mut secret_bytes = [0_u8; 32];
    getrandom::fill(&mut secret_bytes)
        .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
    let secret = EnrollmentSecret::from_bytes(secret_bytes);
    let payload = OfferCreatedPayload {
        enrollment_id,
        token_digest: digest_wire(secret.expose()),
        pool,
        expires_at,
        purpose: EnrollmentPurpose::Bootstrap,
        rotation_overlap_ms: None,
    };
    append(
        events,
        &registry,
        OFFER_CREATED,
        2,
        &payload,
        CommandId::new(),
        now,
    )?;
    let server_ca_pem = fs::read_to_string(&config.server_ca)
        .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
    Ok(EnrollmentBundle {
        schema_version: 2,
        enrollment_id,
        purpose: EnrollmentPurpose::Bootstrap,
        secret,
        expires_at,
        endpoint: EnrollmentEndpoint {
            tcp_address: config.public_tcp_address.clone(),
            websocket_uri: config.websocket_uri.clone(),
            server_name: config.server_name.clone(),
            server_ca_pem,
        },
        handshake_timeout_ms: config.handshake_timeout_ms,
        transport: config.transport,
    })
}

pub(crate) fn create_rotation_offer(
    events: &mut impl EventStore,
    config: &EnrollmentServiceConfig,
    predecessor_credential_id: CredentialId,
    ttl_ms: NonZeroU64,
    overlap_ms: Option<NonZeroU64>,
    now: ObservedAtUnixMillis,
) -> Result<EnrollmentBundle, EnrollmentError> {
    let registry = project(events, now)?;
    let predecessor = registry
        .credentials
        .get(&predecessor_credential_id)
        .filter(|record| {
            record.is_authorized_at(&registry.disabled_workers, now)
                && record.superseded_by.is_none()
        })
        .ok_or(EnrollmentError::CredentialNotActive)?;
    let pool = registry
        .worker_pools
        .get(&predecessor.enrolled.worker_id)
        .ok_or_else(|| EnrollmentError::InvalidHistory("worker has no pool assignment".into()))?
        .pool
        .clone();
    let ttl = i64::try_from(ttl_ms.get())
        .map_err(|_| EnrollmentError::InvalidRequest("rotation TTL is too large".into()))?;
    let expires_at = ObservedAtUnixMillis::new(
        now.get()
            .checked_add(ttl)
            .ok_or_else(|| EnrollmentError::InvalidRequest("rotation TTL overflow".into()))?,
    );
    let enrollment_id = EnrollmentId::new();
    let purpose = EnrollmentPurpose::Rotation {
        worker_id: predecessor.enrolled.worker_id,
        predecessor_credential_id,
    };
    let mut secret_bytes = [0_u8; 32];
    getrandom::fill(&mut secret_bytes)
        .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
    let secret = EnrollmentSecret::from_bytes(secret_bytes);
    append(
        events,
        &registry,
        OFFER_CREATED,
        2,
        &OfferCreatedPayload {
            enrollment_id,
            token_digest: digest_wire(secret.expose()),
            pool,
            expires_at,
            purpose: purpose.clone(),
            rotation_overlap_ms: overlap_ms,
        },
        CommandId::new(),
        now,
    )?;
    let server_ca_pem = fs::read_to_string(&config.server_ca)
        .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
    Ok(EnrollmentBundle {
        schema_version: 2,
        enrollment_id,
        purpose,
        secret,
        expires_at,
        endpoint: EnrollmentEndpoint {
            tcp_address: config.public_tcp_address.clone(),
            websocket_uri: config.websocket_uri.clone(),
            server_name: config.server_name.clone(),
            server_ca_pem,
        },
        handshake_timeout_ms: config.handshake_timeout_ms,
        transport: config.transport,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "redemption keeps one-shot authorization, rotation lineage, issuance, and persistence linear"
)]
pub(crate) fn redeem(
    events: &mut impl EventStore,
    issuer: &impl WorkerCredentialIssuer,
    request: &EnrollmentRequest,
    now: ObservedAtUnixMillis,
) -> Result<IssuedWorkerCredential, EnrollmentError> {
    if request.schema_version != 1 {
        return Err(EnrollmentError::InvalidRequest(
            "unsupported enrollment request schema".into(),
        ));
    }
    if request.csr_pem.is_empty() || request.csr_pem.len() > MAX_CSR_PEM_BYTES {
        return Err(EnrollmentError::InvalidRequest(
            "CSR is empty or exceeds 16 KiB".into(),
        ));
    }
    let registry = project(events, now)?;
    let offer = registry
        .offers
        .get(&request.enrollment_id)
        .ok_or(EnrollmentError::InvalidAuthority)?;
    let presented_digest = Sha256::digest(request.secret.expose());
    if !bool::from(offer.token_digest.ct_eq(presented_digest.as_ref())) {
        return Err(EnrollmentError::InvalidAuthority);
    }
    let csr_digest: [u8; 32] = Sha256::digest(request.csr_pem.as_bytes()).into();
    if let Some(issued) = &offer.issued {
        return if bool::from(issued.csr_digest.ct_eq(&csr_digest)) {
            Ok(issued.credential.clone())
        } else {
            Err(EnrollmentError::AlreadyUsed)
        };
    }
    if offer.revoked {
        return Err(EnrollmentError::Revoked);
    }
    if now > offer.expires_at {
        return Err(EnrollmentError::Expired);
    }
    let (worker_id, predecessor_credential_id, predecessor_retire_at) = match offer.purpose {
        EnrollmentPurpose::Bootstrap => (WorkerId::new(), None, None),
        EnrollmentPurpose::Rotation {
            worker_id,
            predecessor_credential_id,
        } => {
            let predecessor = registry
                .credentials
                .get(&predecessor_credential_id)
                .filter(|record| {
                    record.enrolled.worker_id == worker_id
                        && registry
                            .worker_pools
                            .get(&worker_id)
                            .is_some_and(|assignment| assignment.pool == offer.pool)
                        && record.is_authorized_at(&registry.disabled_workers, now)
                        && record.superseded_by.is_none()
                })
                .ok_or(EnrollmentError::CredentialNotActive)?;
            let retire_at = if let Some(overlap) = offer.rotation_overlap_ms {
                let overlap = i64::try_from(overlap.get()).map_err(|_| {
                    EnrollmentError::InvalidRequest("rotation overlap is too large".into())
                })?;
                Some(ObservedAtUnixMillis::new(
                    now.get().checked_add(overlap).ok_or_else(|| {
                        EnrollmentError::InvalidRequest("rotation overlap overflows time".into())
                    })?,
                ))
            } else {
                None
            };
            (
                predecessor.enrolled.worker_id,
                Some(predecessor_credential_id),
                retire_at,
            )
        }
    };
    let credential_id = CredentialId::new();
    let certificate_chain_pem = issuer.issue(&request.csr_pem, worker_id, credential_id, now)?;
    let certificate_fingerprint = CertificateFingerprint::from_pem(&certificate_chain_pem)
        .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
    let credential = IssuedWorkerCredential {
        schema_version: 2,
        worker_id,
        credential_id,
        pool: offer.pool.clone(),
        predecessor_credential_id,
        predecessor_retire_at,
        certificate_chain_pem,
    };
    let payload = CredentialIssuedPayload {
        enrollment_id: request.enrollment_id,
        csr_digest: hex_wire(&csr_digest),
        credential: credential.clone(),
        certificate_fingerprint,
        issued_at: now,
    };
    append(
        events,
        &registry,
        CREDENTIAL_ISSUED,
        2,
        &payload,
        CommandId::new(),
        now,
    )?;
    Ok(credential)
}

/// Imports one canonical batch of legacy static certificate bindings into managed history.
///
/// The exact `CommandId` is part of the operator request. Repeating it with the same canonical
/// batch returns the original fact even when wall-clock time or registry head has advanced.
///
/// # Errors
///
/// Rejects an empty batch, command reuse for different input, or any credential, certificate, or
/// worker ownership collision.
pub(crate) fn import_static_credentials(
    events: &mut impl EventStore,
    mut credentials: Vec<StaticCredentialImport>,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<StaticEnrollmentImportOutcome, EnrollmentError> {
    credentials.sort_by_key(|credential| credential.credential_id);
    if credentials.is_empty() {
        return Err(EnrollmentError::StaticImportConflict(
            "import batch is empty".into(),
        ));
    }
    if credentials
        .windows(2)
        .any(|pair| pair[0].credential_id == pair[1].credential_id)
    {
        return Err(EnrollmentError::StaticImportConflict(
            "batch repeats a credential identity".into(),
        ));
    }
    let payload = StaticCredentialsImportedPayload { credentials };
    let history = events
        .read_stream(&stream()?, None)
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let registry = project_history(&history, now)?;
    if let Some(prior) = history.iter().find(|event| event.command_id == *command_id) {
        if prior.schema_name.as_str() != STATIC_CREDENTIALS_IMPORTED
            || prior.schema_version.get() != 1
            || decode::<StaticCredentialsImportedPayload>(&prior.payload)? != payload
        {
            return Err(EnrollmentError::CommandConflict);
        }
        return Ok(StaticEnrollmentImportOutcome {
            imported_credentials: payload.credentials.len(),
            event_id: prior.event_id,
            was_replay: true,
        });
    }
    validate_static_import(&registry, &payload.credentials)?;
    let event = NewEvent {
        schema_name: SchemaName::new(STATIC_CREDENTIALS_IMPORTED)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        parent_event_id: registry.last_event_id,
        observed_at_unix_ms: now.get(),
        payload: cairn_codec::to_vec(&payload)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
    };
    let outcome = events
        .append(
            &stream()?,
            registry
                .revision
                .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
            command_id,
            &[event],
        )
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let event_id = outcome.event_ids.first().copied().ok_or_else(|| {
        EnrollmentError::Storage("static import append returned no event identity".into())
    })?;
    Ok(StaticEnrollmentImportOutcome {
        imported_credentials: payload.credentials.len(),
        event_id,
        was_replay: outcome.was_replay,
    })
}

fn validate_static_import(
    registry: &EnrollmentRegistry,
    credentials: &[StaticCredentialImport],
) -> Result<(), EnrollmentError> {
    let mut workers = BTreeSet::new();
    let mut fingerprints = BTreeSet::new();
    for credential in credentials {
        if !workers.insert(credential.worker_id) {
            return Err(EnrollmentError::StaticImportConflict(
                "batch assigns more than one credential to a worker".into(),
            ));
        }
        if !fingerprints.insert(credential.certificate_fingerprint) {
            return Err(EnrollmentError::StaticImportConflict(
                "batch repeats a certificate fingerprint".into(),
            ));
        }
        if registry.credentials.contains_key(&credential.credential_id) {
            return Err(EnrollmentError::StaticImportConflict(format!(
                "credential {} already exists",
                credential.credential_id
            )));
        }
        if registry
            .credentials
            .values()
            .any(|record| record.fingerprint == credential.certificate_fingerprint)
        {
            return Err(EnrollmentError::StaticImportConflict(format!(
                "certificate {} already exists",
                credential.certificate_fingerprint
            )));
        }
        if registry.worker_pools.contains_key(&credential.worker_id) {
            return Err(EnrollmentError::StaticImportConflict(format!(
                "worker {} already has registry ownership",
                credential.worker_id
            )));
        }
    }
    Ok(())
}

pub(crate) fn revoke_credential(
    events: &mut impl EventStore,
    credential_id: CredentialId,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let payload = CredentialRevokedPayload { credential_id };
    let (history, registry) = history_and_registry(events, now)?;
    if let Some(replay) = mutation_replay(&history, command_id, CREDENTIAL_REVOKED, 1, &payload)? {
        return Ok(replay);
    }
    if registry
        .credentials
        .get(&credential_id)
        .is_none_or(|record| record.revoked)
    {
        return Err(EnrollmentError::CredentialNotActive);
    }
    append_mutation(
        events,
        &registry,
        CREDENTIAL_REVOKED,
        1,
        &payload,
        command_id,
        now,
    )
}

pub(crate) fn disable_worker(
    events: &mut impl EventStore,
    worker_id: WorkerId,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let payload = WorkerDisabledPayload { worker_id };
    let (history, registry) = history_and_registry(events, now)?;
    if let Some(replay) = mutation_replay(&history, command_id, WORKER_DISABLED, 1, &payload)? {
        return Ok(replay);
    }
    if registry.disabled_workers.contains(&worker_id)
        || !registry.worker_pools.contains_key(&worker_id)
    {
        return Err(EnrollmentError::WorkerNotActive);
    }
    append_mutation(
        events,
        &registry,
        WORKER_DISABLED,
        1,
        &payload,
        command_id,
        now,
    )
}

pub(crate) fn enable_worker(
    events: &mut impl EventStore,
    worker_id: WorkerId,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let payload = WorkerEnabledPayload { worker_id };
    let (history, registry) = history_and_registry(events, now)?;
    if let Some(replay) = mutation_replay(&history, command_id, WORKER_ENABLED, 1, &payload)? {
        return Ok(replay);
    }
    if !registry.disabled_workers.contains(&worker_id) {
        return Err(EnrollmentError::WorkerNotDisabled);
    }
    append_mutation(
        events,
        &registry,
        WORKER_ENABLED,
        1,
        &payload,
        command_id,
        now,
    )
}

pub(crate) fn assign_worker_pool(
    events: &mut impl EventStore,
    worker_id: WorkerId,
    pool: WorkerPoolName,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let (history, registry) = history_and_registry(events, now)?;
    if let Some(prior) = history.iter().find(|event| event.command_id == *command_id) {
        if prior.schema_name.as_str() != WORKER_POOL_ASSIGNED || prior.schema_version.get() != 1 {
            return Err(EnrollmentError::CommandConflict);
        }
        let payload: WorkerPoolAssignedPayload = decode(&prior.payload)?;
        if payload.worker_id != worker_id || payload.pool != pool {
            return Err(EnrollmentError::CommandConflict);
        }
        return Ok(RegistryMutationOutcome {
            event_id: prior.event_id,
            was_replay: true,
        });
    }
    if !registry.disabled_workers.contains(&worker_id) {
        return Err(EnrollmentError::WorkerNotDisabled);
    }
    let previous_pool = registry
        .worker_pools
        .get(&worker_id)
        .ok_or(EnrollmentError::WorkerNotDisabled)?
        .pool
        .clone();
    if previous_pool == pool {
        return Err(EnrollmentError::WorkerPoolUnchanged);
    }
    append_mutation(
        events,
        &registry,
        WORKER_POOL_ASSIGNED,
        1,
        &WorkerPoolAssignedPayload {
            worker_id,
            previous_pool,
            pool,
        },
        command_id,
        now,
    )
}

pub(crate) fn revoke_enrollment(
    events: &mut impl EventStore,
    enrollment_id: EnrollmentId,
    command_id: &CommandId,
    now: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let payload = EnrollmentRevokedPayload { enrollment_id };
    let (history, registry) = history_and_registry(events, now)?;
    if let Some(replay) = mutation_replay(&history, command_id, ENROLLMENT_REVOKED, 1, &payload)? {
        return Ok(replay);
    }
    let offer = registry
        .offers
        .get(&enrollment_id)
        .ok_or(EnrollmentError::InvalidAuthority)?;
    if offer.issued.is_some() {
        return Err(EnrollmentError::EnrollmentAlreadyIssued);
    }
    if offer.revoked {
        return Err(EnrollmentError::Revoked);
    }
    append_mutation(
        events,
        &registry,
        ENROLLMENT_REVOKED,
        1,
        &payload,
        command_id,
        now,
    )
}

fn history_and_registry(
    events: &impl EventStore,
    now: ObservedAtUnixMillis,
) -> Result<(Vec<EventEnvelope>, EnrollmentRegistry), EnrollmentError> {
    let history = events
        .read_stream(&stream()?, None)
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let registry = project_history(&history, now)?;
    Ok((history, registry))
}

fn mutation_replay<T: for<'de> Deserialize<'de> + PartialEq>(
    history: &[EventEnvelope],
    command_id: &CommandId,
    schema: &str,
    schema_version: u32,
    payload: &T,
) -> Result<Option<RegistryMutationOutcome>, EnrollmentError> {
    let Some(prior) = history.iter().find(|event| event.command_id == *command_id) else {
        return Ok(None);
    };
    if prior.schema_name.as_str() != schema
        || prior.schema_version.get() != schema_version
        || decode::<T>(&prior.payload)? != *payload
    {
        return Err(EnrollmentError::CommandConflict);
    }
    Ok(Some(RegistryMutationOutcome {
        event_id: prior.event_id,
        was_replay: true,
    }))
}

fn append_mutation<T: Serialize>(
    events: &mut impl EventStore,
    registry: &EnrollmentRegistry,
    schema: &str,
    schema_version: u32,
    payload: &T,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<RegistryMutationOutcome, EnrollmentError> {
    let event = NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(schema_version)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        parent_event_id: registry.last_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
    };
    let outcome = events
        .append(
            &stream()?,
            registry
                .revision
                .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
            command_id,
            &[event],
        )
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let event_id = outcome.event_ids.first().copied().ok_or_else(|| {
        EnrollmentError::Storage("registry mutation append returned no event identity".into())
    })?;
    Ok(RegistryMutationOutcome {
        event_id,
        was_replay: outcome.was_replay,
    })
}

fn project(
    events: &impl EventStore,
    now: ObservedAtUnixMillis,
) -> Result<EnrollmentRegistry, EnrollmentError> {
    let history = events
        .read_stream(&stream()?, None)
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    project_history(&history, now)
}

#[expect(
    clippy::too_many_lines,
    reason = "the authority projector validates every lifecycle event and cross-event invariant linearly"
)]
fn project_history(
    history: &[EventEnvelope],
    now: ObservedAtUnixMillis,
) -> Result<EnrollmentRegistry, EnrollmentError> {
    let mut registry = EnrollmentRegistry {
        offers: BTreeMap::new(),
        credentials: BTreeMap::new(),
        worker_pools: BTreeMap::new(),
        disabled_workers: BTreeSet::new(),
        enrolled: BTreeMap::new(),
        revision: None,
        last_event_id: None,
        evaluated_at: now,
    };
    for event in history {
        if event.parent_event_id != registry.last_event_id {
            return Err(EnrollmentError::InvalidHistory(
                "enrollment registry causal parent differs".into(),
            ));
        }
        match event.schema_name.as_str() {
            OFFER_CREATED => {
                if !matches!(event.schema_version.get(), 1 | 2) {
                    return Err(EnrollmentError::InvalidHistory(
                        "offer schema version is unsupported".into(),
                    ));
                }
                let payload: OfferCreatedPayload = decode(&event.payload)?;
                if event.schema_version.get() == 1
                    && (payload.purpose != EnrollmentPurpose::Bootstrap
                        || payload.rotation_overlap_ms.is_some())
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "V1 offer contains rotation state".into(),
                    ));
                }
                match &payload.purpose {
                    EnrollmentPurpose::Bootstrap if payload.rotation_overlap_ms.is_some() => {
                        return Err(EnrollmentError::InvalidHistory(
                            "bootstrap offer contains rotation overlap".into(),
                        ));
                    }
                    EnrollmentPurpose::Rotation {
                        worker_id,
                        predecessor_credential_id,
                    } => {
                        let offered_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
                        let predecessor = registry
                            .credentials
                            .get(predecessor_credential_id)
                            .filter(|record| {
                                record.enrolled.worker_id == *worker_id
                                    && registry
                                        .worker_pools
                                        .get(worker_id)
                                        .is_some_and(|assignment| assignment.pool == payload.pool)
                                    && record
                                        .is_authorized_at(&registry.disabled_workers, offered_at)
                                    && record.superseded_by.is_none()
                            })
                            .ok_or_else(|| {
                                EnrollmentError::InvalidHistory(
                                    "rotation offer has no active predecessor".into(),
                                )
                            })?;
                        let _ = predecessor;
                    }
                    EnrollmentPurpose::Bootstrap => {}
                }
                let token_digest = parse_digest(&payload.token_digest)?;
                if registry
                    .offers
                    .insert(
                        payload.enrollment_id,
                        Offer {
                            token_digest,
                            pool: payload.pool,
                            expires_at: payload.expires_at,
                            purpose: payload.purpose,
                            rotation_overlap_ms: payload.rotation_overlap_ms,
                            issued: None,
                            revoked: false,
                        },
                    )
                    .is_some()
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "enrollment identity was offered twice".into(),
                    ));
                }
            }
            CREDENTIAL_ISSUED => {
                if !matches!(event.schema_version.get(), 1 | 2) {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential issuance schema version is unsupported".into(),
                    ));
                }
                let payload: CredentialIssuedPayload = decode(&event.payload)?;
                let csr_digest = parse_digest(&payload.csr_digest)?;
                let offer = registry
                    .offers
                    .get(&payload.enrollment_id)
                    .cloned()
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory("credential precedes its offer".into())
                    })?;
                if offer.issued.is_some() || offer.revoked || offer.pool != payload.credential.pool
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential duplicates or changes the authorized pool".into(),
                    ));
                }
                if event.schema_version.get() == 1
                    && (payload.credential.schema_version != 1
                        || payload.credential.predecessor_credential_id.is_some()
                        || payload.credential.predecessor_retire_at.is_some()
                        || offer.purpose != EnrollmentPurpose::Bootstrap)
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "V1 issuance contains rotation state".into(),
                    ));
                }
                let rotation_predecessor = match &offer.purpose {
                    EnrollmentPurpose::Bootstrap => {
                        if payload.credential.predecessor_credential_id.is_some()
                            || payload.credential.predecessor_retire_at.is_some()
                        {
                            return Err(EnrollmentError::InvalidHistory(
                                "bootstrap credential cites a predecessor".into(),
                            ));
                        }
                        None
                    }
                    EnrollmentPurpose::Rotation {
                        worker_id,
                        predecessor_credential_id,
                    } => {
                        if payload.credential.schema_version != 2
                            || payload.credential.worker_id != *worker_id
                            || payload.credential.predecessor_credential_id
                                != Some(*predecessor_credential_id)
                            || offer.rotation_overlap_ms.is_some()
                                != payload.credential.predecessor_retire_at.is_some()
                        {
                            return Err(EnrollmentError::InvalidHistory(
                                "rotated credential contradicts its offer".into(),
                            ));
                        }
                        let issued_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
                        let expected_retire_at = if let Some(overlap) = offer.rotation_overlap_ms {
                            let overlap = i64::try_from(overlap.get()).map_err(|_| {
                                EnrollmentError::InvalidHistory(
                                    "rotation overlap exceeds time range".into(),
                                )
                            })?;
                            Some(ObservedAtUnixMillis::new(
                                issued_at.get().checked_add(overlap).ok_or_else(|| {
                                    EnrollmentError::InvalidHistory(
                                        "rotation overlap overflows time".into(),
                                    )
                                })?,
                            ))
                        } else {
                            None
                        };
                        if payload.credential.predecessor_retire_at != expected_retire_at {
                            return Err(EnrollmentError::InvalidHistory(
                                "rotation retirement differs from frozen overlap".into(),
                            ));
                        }
                        let predecessor = registry
                            .credentials
                            .get(predecessor_credential_id)
                            .filter(|record| {
                                record.enrolled.worker_id == *worker_id
                                    && registry
                                        .worker_pools
                                        .get(worker_id)
                                        .is_some_and(|assignment| assignment.pool == offer.pool)
                                    && record
                                        .is_authorized_at(&registry.disabled_workers, issued_at)
                                    && record.superseded_by.is_none()
                            })
                            .ok_or_else(|| {
                                EnrollmentError::InvalidHistory(
                                    "rotation issuance has no active predecessor".into(),
                                )
                            })?;
                        if payload
                            .credential
                            .predecessor_retire_at
                            .is_some_and(|retire_at| retire_at <= issued_at)
                        {
                            return Err(EnrollmentError::InvalidHistory(
                                "rotation retirement is not after issuance".into(),
                            ));
                        }
                        let _ = predecessor;
                        Some(*predecessor_credential_id)
                    }
                };
                let mut enrolled = EnrolledWorker {
                    worker_id: payload.credential.worker_id,
                    credential_id: payload.credential.credential_id,
                    pool: payload.credential.pool.clone(),
                    pool_assignment_revision: event.event_id,
                };
                if rotation_predecessor.is_none() {
                    if registry
                        .worker_pools
                        .insert(
                            enrolled.worker_id,
                            WorkerPoolAssignment {
                                pool: enrolled.pool.clone(),
                                authority_revision: event.event_id,
                            },
                        )
                        .is_some()
                    {
                        return Err(EnrollmentError::InvalidHistory(
                            "bootstrap credential reused an existing worker identity".into(),
                        ));
                    }
                } else if registry
                    .worker_pools
                    .get(&enrolled.worker_id)
                    .is_none_or(|assignment| assignment.pool != enrolled.pool)
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "rotated credential differs from current worker pool".into(),
                    ));
                }
                enrolled.pool_assignment_revision = registry
                    .worker_pools
                    .get(&enrolled.worker_id)
                    .map(|assignment| assignment.authority_revision)
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory(
                            "credential has no worker pool authority".into(),
                        )
                    })?;
                if registry
                    .credentials
                    .insert(
                        payload.credential.credential_id,
                        CredentialRecord {
                            fingerprint: payload.certificate_fingerprint,
                            enrolled: enrolled.clone(),
                            provenance: CredentialProvenance::Issued {
                                enrollment_id: payload.enrollment_id,
                            },
                            revoked: false,
                            superseded_by: None,
                            retire_at: None,
                            predecessor: rotation_predecessor,
                        },
                    )
                    .is_some()
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential identity was issued twice".into(),
                    ));
                }
                if registry.credentials.values().any(|record| {
                    record.enrolled.credential_id != payload.credential.credential_id
                        && record.fingerprint == payload.certificate_fingerprint
                }) {
                    return Err(EnrollmentError::InvalidHistory(
                        "certificate fingerprint was issued twice".into(),
                    ));
                }
                if let Some(predecessor_id) = rotation_predecessor {
                    let predecessor = registry
                        .credentials
                        .get_mut(&predecessor_id)
                        .expect("validated predecessor exists");
                    predecessor.superseded_by = Some(payload.credential.credential_id);
                    predecessor.retire_at = payload.credential.predecessor_retire_at;
                }
                registry
                    .offers
                    .get_mut(&payload.enrollment_id)
                    .expect("validated offer exists")
                    .issued = Some(Issued {
                    csr_digest,
                    credential: payload.credential,
                });
            }
            STATIC_CREDENTIALS_IMPORTED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "static credential import schema version is unsupported".into(),
                    ));
                }
                let payload: StaticCredentialsImportedPayload = decode(&event.payload)?;
                if payload.credentials.is_empty()
                    || payload
                        .credentials
                        .windows(2)
                        .any(|pair| pair[0].credential_id >= pair[1].credential_id)
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "static credential import batch is not canonical".into(),
                    ));
                }
                validate_static_import(&registry, &payload.credentials).map_err(|error| {
                    EnrollmentError::InvalidHistory(format!(
                        "contradictory static credential import: {error}"
                    ))
                })?;
                for credential in payload.credentials {
                    registry.worker_pools.insert(
                        credential.worker_id,
                        WorkerPoolAssignment {
                            pool: credential.pool.clone(),
                            authority_revision: event.event_id,
                        },
                    );
                    registry.credentials.insert(
                        credential.credential_id,
                        CredentialRecord {
                            fingerprint: credential.certificate_fingerprint,
                            enrolled: EnrolledWorker {
                                worker_id: credential.worker_id,
                                credential_id: credential.credential_id,
                                pool: credential.pool,
                                pool_assignment_revision: event.event_id,
                            },
                            provenance: CredentialProvenance::ImportedStatic {
                                event_id: event.event_id,
                            },
                            revoked: false,
                            superseded_by: None,
                            retire_at: None,
                            predecessor: None,
                        },
                    );
                }
            }
            CREDENTIAL_REVOKED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential revocation schema version is unsupported".into(),
                    ));
                }
                let payload: CredentialRevokedPayload = decode(&event.payload)?;
                let revoked_at = ObservedAtUnixMillis::new(event.observed_at_unix_ms);
                let record = registry
                    .credentials
                    .get(&payload.credential_id)
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory("unknown credential was revoked".into())
                    })?;
                if record.revoked {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential was revoked twice".into(),
                    ));
                }
                let rollback_predecessor = record.predecessor.filter(|predecessor_id| {
                    registry
                        .credentials
                        .get(predecessor_id)
                        .is_some_and(|predecessor| {
                            predecessor.superseded_by == Some(payload.credential_id)
                                && predecessor
                                    .retire_at
                                    .is_none_or(|retire_at| revoked_at < retire_at)
                        })
                });
                registry
                    .credentials
                    .get_mut(&payload.credential_id)
                    .expect("validated credential exists")
                    .revoked = true;
                if let Some(predecessor_id) = rollback_predecessor {
                    let predecessor = registry
                        .credentials
                        .get_mut(&predecessor_id)
                        .expect("validated predecessor exists");
                    predecessor.superseded_by = None;
                    predecessor.retire_at = None;
                }
            }
            WORKER_DISABLED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "worker disable schema version is unsupported".into(),
                    ));
                }
                let payload: WorkerDisabledPayload = decode(&event.payload)?;
                if !registry.worker_pools.contains_key(&payload.worker_id)
                    || !registry.disabled_workers.insert(payload.worker_id)
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "unknown or disabled worker was disabled".into(),
                    ));
                }
            }
            WORKER_ENABLED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "worker enable schema version is unsupported".into(),
                    ));
                }
                let payload: WorkerEnabledPayload = decode(&event.payload)?;
                if !registry.disabled_workers.remove(&payload.worker_id) {
                    return Err(EnrollmentError::InvalidHistory(
                        "unknown or enabled worker was enabled".into(),
                    ));
                }
            }
            WORKER_POOL_ASSIGNED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "worker pool assignment schema version is unsupported".into(),
                    ));
                }
                let payload: WorkerPoolAssignedPayload = decode(&event.payload)?;
                let assignment = registry
                    .worker_pools
                    .get_mut(&payload.worker_id)
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory(
                            "unknown worker received a pool assignment".into(),
                        )
                    })?;
                if !registry.disabled_workers.contains(&payload.worker_id)
                    || assignment.pool != payload.previous_pool
                    || assignment.pool == payload.pool
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "worker pool assignment contradicts lifecycle state".into(),
                    ));
                }
                assignment.pool = payload.pool;
                assignment.authority_revision = event.event_id;
            }
            ENROLLMENT_REVOKED => {
                if event.schema_version.get() != 1 {
                    return Err(EnrollmentError::InvalidHistory(
                        "enrollment revocation schema version is unsupported".into(),
                    ));
                }
                let payload: EnrollmentRevokedPayload = decode(&event.payload)?;
                let offer = registry
                    .offers
                    .get_mut(&payload.enrollment_id)
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory("unknown enrollment was revoked".into())
                    })?;
                if offer.revoked || offer.issued.is_some() {
                    return Err(EnrollmentError::InvalidHistory(
                        "used or revoked enrollment was revoked".into(),
                    ));
                }
                offer.revoked = true;
            }
            other => {
                return Err(EnrollmentError::InvalidHistory(format!(
                    "unsupported enrollment event {other}"
                )));
            }
        }
        registry.revision = Some(
            StreamRevision::new(event.sequence.get())
                .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        );
        registry.last_event_id = Some(event.event_id);
    }
    for record in registry.credentials.values() {
        match record.provenance {
            CredentialProvenance::Issued { enrollment_id } => {
                if registry
                    .offers
                    .get(&enrollment_id)
                    .and_then(|offer| offer.issued.as_ref())
                    .is_none_or(|issued| {
                        issued.credential.credential_id != record.enrolled.credential_id
                    })
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "issued credential lost its enrollment provenance".into(),
                    ));
                }
            }
            CredentialProvenance::ImportedStatic { event_id } => {
                if !history.iter().any(|event| {
                    event.event_id == event_id
                        && event.schema_name.as_str() == STATIC_CREDENTIALS_IMPORTED
                }) {
                    return Err(EnrollmentError::InvalidHistory(
                        "static credential lost its import provenance".into(),
                    ));
                }
            }
        }
        let assignment = registry
            .worker_pools
            .get(&record.enrolled.worker_id)
            .ok_or_else(|| {
                EnrollmentError::InvalidHistory("credential has no worker pool history".into())
            })?;
        let mut enrolled = record.enrolled.clone();
        enrolled.pool = assignment.pool.clone();
        enrolled.pool_assignment_revision = assignment.authority_revision;
        if record.is_authorized_at(&registry.disabled_workers, now)
            && registry
                .enrolled
                .insert(record.fingerprint, enrolled)
                .is_some()
        {
            return Err(EnrollmentError::InvalidHistory(
                "active certificate fingerprint is not unique".into(),
            ));
        }
    }
    Ok(registry)
}

fn append<T: Serialize>(
    events: &mut impl EventStore,
    registry: &EnrollmentRegistry,
    schema: &str,
    schema_version: u32,
    payload: &T,
    command_id: CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), EnrollmentError> {
    let event = NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(schema_version)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        parent_event_id: registry.last_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
    };
    events
        .append(
            &stream()?,
            registry
                .revision
                .map_or(ExpectedRevision::NoStream, ExpectedRevision::Exact),
            &command_id,
            &[event],
        )
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    Ok(())
}

fn stream() -> Result<StreamId, EnrollmentError> {
    Ok(StreamId {
        kind: AggregateKind::new("worker-enrollment-registry")
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new("worker-enrollment-registry:singleton")
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, EnrollmentError> {
    cairn_codec::from_slice(bytes)
        .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))
}

fn digest_wire(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex_wire(&digest)
}

fn hex_wire(digest: &[u8; 32]) -> String {
    let mut wire = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(wire, "{byte:02x}");
    }
    wire
}

fn parse_digest(wire: &str) -> Result<[u8; 32], EnrollmentError> {
    if wire.len() != 64
        || !wire
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(EnrollmentError::InvalidHistory(
            "digest wire value is not canonical".into(),
        ));
    }
    let mut result = [0_u8; 32];
    for (index, pair) in wire.as_bytes().chunks_exact(2).enumerate() {
        result[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(result)
}

fn hex_nibble(byte: u8) -> Result<u8, EnrollmentError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(EnrollmentError::InvalidHistory(
            "digest contains non-hexadecimal bytes".into(),
        )),
    }
}

fn timestamp(unix_ms: i64) -> Result<OffsetDateTime, EnrollmentError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000)
        .map_err(|error| EnrollmentError::Issuance(error.to_string()))
}

fn certificate_public_key(pem: &str) -> Result<Vec<u8>, EnrollmentError> {
    let mut reader = BufReader::new(pem.as_bytes());
    let certificate = rustls_pemfile::certs(&mut reader)
        .next()
        .transpose()
        .map_err(|error| EnrollmentError::Issuance(error.to_string()))?
        .ok_or_else(|| EnrollmentError::Issuance("issuer certificate is empty".into()))?;
    let (_, parsed) = x509_parser::parse_x509_certificate(certificate.as_ref())
        .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
    Ok(parsed.public_key().raw.to_vec())
}

#[cfg(test)]
mod tests {
    use cairn_store_sqlite::SqliteEventStore;

    use super::*;

    fn imported(
        worker_id: WorkerId,
        credential_id: CredentialId,
        fingerprint_seed: &[u8],
    ) -> StaticCredentialImport {
        StaticCredentialImport {
            worker_id,
            credential_id,
            pool: WorkerPoolName::new("imported-pool").expect("pool"),
            certificate_fingerprint: CertificateFingerprint::from_der(fingerprint_seed),
        }
    }

    #[test]
    fn static_import_is_explicitly_idempotent_and_rejects_every_ownership_collision() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let worker_id = WorkerId::new();
        let credential_id = CredentialId::new();
        let import = imported(worker_id, credential_id, b"certificate-a");
        let command_id = CommandId::new();
        let first = import_static_credentials(
            &mut events,
            vec![import.clone()],
            &command_id,
            ObservedAtUnixMillis::new(1),
        )
        .expect("first import");
        assert!(!first.was_replay());
        let replay = import_static_credentials(
            &mut events,
            vec![import.clone()],
            &command_id,
            ObservedAtUnixMillis::new(2),
        )
        .expect("exact replay");
        assert!(replay.was_replay());
        assert_eq!(replay.event_id(), first.event_id());

        assert!(matches!(
            import_static_credentials(
                &mut events,
                vec![imported(worker_id, credential_id, b"different-input")],
                &command_id,
                ObservedAtUnixMillis::new(3),
            ),
            Err(EnrollmentError::CommandConflict)
        ));
        assert!(matches!(
            import_static_credentials(
                &mut events,
                vec![import.clone()],
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(EnrollmentError::StaticImportConflict(_))
        ));
        assert!(matches!(
            import_static_credentials(
                &mut events,
                vec![imported(worker_id, CredentialId::new(), b"certificate-b")],
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(EnrollmentError::StaticImportConflict(_))
        ));
        let fingerprint_collision = StaticCredentialImport {
            worker_id: WorkerId::new(),
            credential_id: CredentialId::new(),
            pool: WorkerPoolName::new("another-pool").expect("pool"),
            certificate_fingerprint: import.certificate_fingerprint,
        };
        assert!(matches!(
            import_static_credentials(
                &mut events,
                vec![fingerprint_collision],
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(EnrollmentError::StaticImportConflict(_))
        ));

        let registry = project(&events, ObservedAtUnixMillis::new(4)).expect("registry replay");
        assert!(registry.credential_is_authorized(credential_id, worker_id));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one lifecycle proof covers command replay, conflicts, projection, and forged history"
    )]
    fn worker_lifecycle_is_explicit_idempotent_and_pool_authority_survives_replay() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let worker_id = WorkerId::new();
        let credential_id = CredentialId::new();
        let imported = imported(worker_id, credential_id, b"lifecycle-certificate");
        let fingerprint = imported.certificate_fingerprint;
        import_static_credentials(
            &mut events,
            vec![imported],
            &CommandId::new(),
            ObservedAtUnixMillis::new(1),
        )
        .expect("import");

        assert!(matches!(
            assign_worker_pool(
                &mut events,
                worker_id,
                WorkerPoolName::new("moved-pool").expect("pool"),
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            ),
            Err(EnrollmentError::WorkerNotDisabled)
        ));

        let disable_command = CommandId::new();
        let disabled = disable_worker(
            &mut events,
            worker_id,
            &disable_command,
            ObservedAtUnixMillis::new(3),
        )
        .expect("disable");
        let disable_replay = disable_worker(
            &mut events,
            worker_id,
            &disable_command,
            ObservedAtUnixMillis::new(4),
        )
        .expect("disable replay");
        assert!(disable_replay.was_replay());
        assert_eq!(disable_replay.event_id(), disabled.event_id());
        assert!(matches!(
            enable_worker(
                &mut events,
                worker_id,
                &disable_command,
                ObservedAtUnixMillis::new(4),
            ),
            Err(EnrollmentError::CommandConflict)
        ));

        let pool = WorkerPoolName::new("moved-pool").expect("pool");
        let pool_command = CommandId::new();
        let assigned = assign_worker_pool(
            &mut events,
            worker_id,
            pool.clone(),
            &pool_command,
            ObservedAtUnixMillis::new(5),
        )
        .expect("assign pool");
        let assignment_replay = assign_worker_pool(
            &mut events,
            worker_id,
            pool.clone(),
            &pool_command,
            ObservedAtUnixMillis::new(6),
        )
        .expect("assignment replay");
        assert!(assignment_replay.was_replay());
        assert_eq!(assignment_replay.event_id(), assigned.event_id());
        assert!(matches!(
            assign_worker_pool(
                &mut events,
                worker_id,
                pool.clone(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(6),
            ),
            Err(EnrollmentError::WorkerPoolUnchanged)
        ));

        let enable_command = CommandId::new();
        let enabled = enable_worker(
            &mut events,
            worker_id,
            &enable_command,
            ObservedAtUnixMillis::new(7),
        )
        .expect("enable");
        let enable_replay = enable_worker(
            &mut events,
            worker_id,
            &enable_command,
            ObservedAtUnixMillis::new(8),
        )
        .expect("enable replay");
        assert!(enable_replay.was_replay());
        assert_eq!(enable_replay.event_id(), enabled.event_id());

        let registry = project(&events, ObservedAtUnixMillis::new(9)).expect("registry replay");
        assert!(registry.credential_is_authorized(credential_id, worker_id));
        let enrolled = registry
            .enrolled()
            .get(&fingerprint)
            .expect("active worker");
        assert_eq!(enrolled.pool, pool);
        assert_eq!(enrolled.pool_assignment_revision, assigned.event_id());

        events
            .append(
                &stream().expect("stream"),
                ExpectedRevision::Exact(registry.revision.expect("registry revision")),
                &CommandId::new(),
                &[NewEvent {
                    schema_name: SchemaName::new(WORKER_POOL_ASSIGNED).expect("schema"),
                    schema_version: SchemaVersion::new(1).expect("version"),
                    parent_event_id: registry.last_event_id,
                    observed_at_unix_ms: 10,
                    payload: cairn_codec::to_vec(&WorkerPoolAssignedPayload {
                        worker_id,
                        previous_pool: WorkerPoolName::new("moved-pool").expect("pool"),
                        pool: WorkerPoolName::new("forged-pool").expect("pool"),
                    })
                    .expect("payload"),
                }],
            )
            .expect("append forged lifecycle fact");
        assert!(matches!(
            project(&events, ObservedAtUnixMillis::new(11)),
            Err(EnrollmentError::InvalidHistory(_))
        ));
    }

    #[test]
    fn contradictory_import_fact_fails_closed_during_projection() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let worker_id = WorkerId::new();
        let credential_id = CredentialId::new();
        let import_command = CommandId::new();
        import_static_credentials(
            &mut events,
            vec![imported(worker_id, credential_id, b"certificate-a")],
            &import_command,
            ObservedAtUnixMillis::new(1),
        )
        .expect("first import");
        let registry = project(&events, ObservedAtUnixMillis::new(2)).expect("registry");
        let payload = StaticCredentialsImportedPayload {
            credentials: vec![imported(worker_id, CredentialId::new(), b"certificate-b")],
        };
        events
            .append(
                &stream().expect("stream"),
                ExpectedRevision::Exact(registry.revision.expect("revision")),
                &CommandId::new(),
                &[NewEvent {
                    schema_name: SchemaName::new(STATIC_CREDENTIALS_IMPORTED).expect("schema"),
                    schema_version: SchemaVersion::new(1).expect("version"),
                    parent_event_id: registry.last_event_id,
                    observed_at_unix_ms: 2,
                    payload: cairn_codec::to_vec(&payload).expect("payload"),
                }],
            )
            .expect("forge structurally valid fact");

        assert!(matches!(
            project(&events, ObservedAtUnixMillis::new(3)),
            Err(EnrollmentError::InvalidHistory(_))
        ));
        assert!(matches!(
            import_static_credentials(
                &mut events,
                vec![imported(worker_id, credential_id, b"certificate-a")],
                &import_command,
                ObservedAtUnixMillis::new(3),
            ),
            Err(EnrollmentError::InvalidHistory(_))
        ));
    }
}
