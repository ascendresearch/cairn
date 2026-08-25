use std::{collections::BTreeMap, fs, io::BufReader, num::NonZeroU64};

use cairn_control_transport::{
    CertificateFingerprint, EnrollmentBundle, EnrollmentEndpoint, EnrollmentRequest,
    EnrollmentSecret, IssuedWorkerCredential,
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
const MAX_CSR_PEM_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OfferCreatedPayload {
    enrollment_id: EnrollmentId,
    token_digest: String,
    pool: WorkerPoolName,
    expires_at: ObservedAtUnixMillis,
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

#[derive(Clone)]
struct Offer {
    token_digest: [u8; 32],
    pool: WorkerPoolName,
    expires_at: ObservedAtUnixMillis,
    issued: Option<Issued>,
}

#[derive(Clone)]
struct Issued {
    csr_digest: [u8; 32],
    credential: IssuedWorkerCredential,
}

pub(crate) struct EnrollmentRegistry {
    offers: BTreeMap<EnrollmentId, Offer>,
    enrolled: BTreeMap<CertificateFingerprint, EnrolledWorker>,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
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
    #[error("credential issuance failed: {0}")]
    Issuance(String),
}

impl EnrollmentRegistry {
    pub(crate) fn load(events: &impl EventStore) -> Result<Self, EnrollmentError> {
        project(events)
    }

    pub(crate) fn enrolled(&self) -> &BTreeMap<CertificateFingerprint, EnrolledWorker> {
        &self.enrolled
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
    let registry = project(events)?;
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
    };
    append(
        events,
        &registry,
        OFFER_CREATED,
        &payload,
        CommandId::new(),
        now,
    )?;
    let server_ca_pem = fs::read_to_string(&config.server_ca)
        .map_err(|error| EnrollmentError::InvalidRequest(error.to_string()))?;
    Ok(EnrollmentBundle {
        schema_version: 1,
        enrollment_id,
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
    let registry = project(events)?;
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
    if now > offer.expires_at {
        return Err(EnrollmentError::Expired);
    }
    let worker_id = WorkerId::new();
    let credential_id = CredentialId::new();
    let certificate_chain_pem = issuer.issue(&request.csr_pem, worker_id, credential_id, now)?;
    let certificate_fingerprint = CertificateFingerprint::from_pem(&certificate_chain_pem)
        .map_err(|error| EnrollmentError::Issuance(error.to_string()))?;
    let credential = IssuedWorkerCredential {
        schema_version: 1,
        worker_id,
        credential_id,
        pool: offer.pool.clone(),
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
        &payload,
        CommandId::new(),
        now,
    )?;
    Ok(credential)
}

fn project(events: &impl EventStore) -> Result<EnrollmentRegistry, EnrollmentError> {
    let history = events
        .read_stream(&stream()?, None)
        .map_err(|error| EnrollmentError::Storage(error.to_string()))?;
    let mut registry = EnrollmentRegistry {
        offers: BTreeMap::new(),
        enrolled: BTreeMap::new(),
        revision: None,
        last_event_id: None,
    };
    for event in history {
        if event.schema_version.get() != 1 || event.parent_event_id != registry.last_event_id {
            return Err(EnrollmentError::InvalidHistory(
                "schema version or causal parent differs".into(),
            ));
        }
        match event.schema_name.as_str() {
            OFFER_CREATED => {
                let payload: OfferCreatedPayload = decode(&event.payload)?;
                let token_digest = parse_digest(&payload.token_digest)?;
                if registry
                    .offers
                    .insert(
                        payload.enrollment_id,
                        Offer {
                            token_digest,
                            pool: payload.pool,
                            expires_at: payload.expires_at,
                            issued: None,
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
                let payload: CredentialIssuedPayload = decode(&event.payload)?;
                let csr_digest = parse_digest(&payload.csr_digest)?;
                let offer = registry
                    .offers
                    .get_mut(&payload.enrollment_id)
                    .ok_or_else(|| {
                        EnrollmentError::InvalidHistory("credential precedes its offer".into())
                    })?;
                if offer.issued.is_some() || offer.pool != payload.credential.pool {
                    return Err(EnrollmentError::InvalidHistory(
                        "credential duplicates or changes the authorized pool".into(),
                    ));
                }
                let enrolled = EnrolledWorker {
                    worker_id: payload.credential.worker_id,
                    pool: payload.credential.pool.clone(),
                };
                if registry
                    .enrolled
                    .insert(payload.certificate_fingerprint, enrolled)
                    .is_some()
                {
                    return Err(EnrollmentError::InvalidHistory(
                        "certificate fingerprint was issued twice".into(),
                    ));
                }
                offer.issued = Some(Issued {
                    csr_digest,
                    credential: payload.credential,
                });
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
    Ok(registry)
}

fn append<T: Serialize>(
    events: &mut impl EventStore,
    registry: &EnrollmentRegistry,
    schema: &str,
    payload: &T,
    command_id: CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), EnrollmentError> {
    let event = NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| EnrollmentError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
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
