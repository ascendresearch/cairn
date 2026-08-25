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
use cairn_record::{EventStore, ExpectedRevision, NewEvent, StreamId};
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
const WORKER_DISABLED: &str = "execution.worker-disabled";
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialRevokedPayload {
    credential_id: CredentialId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerDisabledPayload {
    worker_id: WorkerId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    revoked: bool,
    superseded_by: Option<CredentialId>,
    retire_at: Option<ObservedAtUnixMillis>,
    predecessor: Option<CredentialId>,
}

pub(crate) struct EnrollmentRegistry {
    offers: BTreeMap<EnrollmentId, Offer>,
    credentials: BTreeMap<CredentialId, CredentialRecord>,
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
    #[error("issued enrollment authority cannot be revoked")]
    EnrollmentAlreadyIssued,
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

    pub(crate) fn credential_is_known(&self, credential_id: CredentialId) -> bool {
        self.credentials.contains_key(&credential_id)
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
            pool: predecessor.enrolled.pool.clone(),
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
                        && record.enrolled.pool == offer.pool
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

pub(crate) fn revoke_credential(
    events: &mut impl EventStore,
    credential_id: CredentialId,
    now: ObservedAtUnixMillis,
) -> Result<(), EnrollmentError> {
    let registry = project(events, now)?;
    if registry
        .credentials
        .get(&credential_id)
        .is_none_or(|record| record.revoked)
    {
        return Err(EnrollmentError::CredentialNotActive);
    }
    append(
        events,
        &registry,
        CREDENTIAL_REVOKED,
        1,
        &CredentialRevokedPayload { credential_id },
        CommandId::new(),
        now,
    )
}

pub(crate) fn disable_worker(
    events: &mut impl EventStore,
    worker_id: WorkerId,
    now: ObservedAtUnixMillis,
) -> Result<(), EnrollmentError> {
    let registry = project(events, now)?;
    if registry.disabled_workers.contains(&worker_id)
        || !registry
            .credentials
            .values()
            .any(|record| record.enrolled.worker_id == worker_id)
    {
        return Err(EnrollmentError::WorkerNotActive);
    }
    append(
        events,
        &registry,
        WORKER_DISABLED,
        1,
        &WorkerDisabledPayload { worker_id },
        CommandId::new(),
        now,
    )
}

pub(crate) fn revoke_enrollment(
    events: &mut impl EventStore,
    enrollment_id: EnrollmentId,
    now: ObservedAtUnixMillis,
) -> Result<(), EnrollmentError> {
    let registry = project(events, now)?;
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
    append(
        events,
        &registry,
        ENROLLMENT_REVOKED,
        1,
        &EnrollmentRevokedPayload { enrollment_id },
        CommandId::new(),
        now,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the authority projector validates every lifecycle event and cross-event invariant linearly"
)]
fn project(
    events: &impl EventStore,
    now: ObservedAtUnixMillis,
) -> Result<EnrollmentRegistry, EnrollmentError> {
    let history = events
        .read_stream(&stream()?, None)
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let mut registry = EnrollmentRegistry {
        offers: BTreeMap::new(),
        credentials: BTreeMap::new(),
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
                                    && record.enrolled.pool == payload.pool
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
                                    && record.enrolled.pool == offer.pool
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
                let enrolled = EnrolledWorker {
                    worker_id: payload.credential.worker_id,
                    credential_id: payload.credential.credential_id,
                    pool: payload.credential.pool.clone(),
                };
                if registry
                    .credentials
                    .insert(
                        payload.credential.credential_id,
                        CredentialRecord {
                            fingerprint: payload.certificate_fingerprint,
                            enrolled: enrolled.clone(),
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
                if !registry
                    .credentials
                    .values()
                    .any(|record| record.enrolled.worker_id == payload.worker_id)
                    || !registry.disabled_workers.insert(payload.worker_id)
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "unknown or disabled worker was disabled".into(),
                    ));
                }
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
        if record.is_authorized_at(&registry.disabled_workers, now)
            && registry
                .enrolled
                .insert(record.fingerprint, record.enrolled.clone())
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
