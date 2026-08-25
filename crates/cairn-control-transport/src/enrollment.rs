use std::{fmt, str::FromStr};

use cairn_execution::WorkerPoolName;
use cairn_protocol::{CredentialId, EnrollmentId, ObservedAtUnixMillis, WorkerId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::TransportError;
use crate::TransportPolicy;

const SECRET_LEN: usize = 32;

/// A 256-bit bearer secret carried only by a one-shot enrollment bundle.
///
/// Debug output is deliberately redacted. Controller persistence stores only its digest.
#[derive(Clone, Eq, PartialEq)]
pub struct EnrollmentSecret([u8; SECRET_LEN]);

impl EnrollmentSecret {
    /// Constructs a secret from cryptographically random bytes supplied by the authority.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SECRET_LEN]) -> Self {
        Self(bytes)
    }

    /// Exposes the secret only at the hashing/wire-authentication boundary.
    #[must_use]
    pub const fn expose(&self) -> &[u8; SECRET_LEN] {
        &self.0
    }

    fn hexadecimal(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(SECRET_LEN * 2);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

impl fmt::Debug for EnrollmentSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnrollmentSecret([REDACTED])")
    }
}

impl FromStr for EnrollmentSecret {
    type Err = TransportError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != SECRET_LEN * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(TransportError::InvalidEnrollmentSecret);
        }
        let mut bytes = [0_u8; SECRET_LEN];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for EnrollmentSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.hexadecimal())
    }
}

impl<'de> Deserialize<'de> for EnrollmentSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Public endpoint and pinned trust material required for the one-shot bootstrap connection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentEndpoint {
    pub tcp_address: String,
    pub websocket_uri: String,
    pub server_name: String,
    pub server_ca_pem: String,
}

/// Public endpoint and pinned trust material used after enrollment for ordinary worker control.
///
/// This is deliberately separate from [`EnrollmentEndpoint`]: operators may isolate bootstrap
/// traffic on another listener, DNS name, and server certificate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerControlEndpoint {
    pub tcp_address: String,
    pub websocket_uri: String,
    pub server_name: String,
    pub server_ca_pem: String,
}

/// Controller-owned lifecycle purpose of one enrollment authority.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EnrollmentPurpose {
    /// Allocates a new stable worker and its first credential.
    #[default]
    Bootstrap,
    /// Issues a successor credential for one exact existing credential.
    Rotation {
        worker_id: WorkerId,
        predecessor_credential_id: CredentialId,
    },
}

/// Short-lived file transferred to a worker before it has a client credential.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentBundle {
    pub schema_version: u16,
    pub enrollment_id: EnrollmentId,
    #[serde(default)]
    pub purpose: EnrollmentPurpose,
    pub secret: EnrollmentSecret,
    pub expires_at: ObservedAtUnixMillis,
    pub endpoint: EnrollmentEndpoint,
    pub control_endpoint: WorkerControlEndpoint,
    pub handshake_timeout_ms: Option<std::num::NonZeroU64>,
    pub transport: TransportPolicy,
}

/// First and only worker-to-controller enrollment message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentRequest {
    pub schema_version: u16,
    pub enrollment_id: EnrollmentId,
    pub secret: EnrollmentSecret,
    pub csr_pem: String,
}

/// Machine-readable enrollment rejection category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentRejectCode {
    InvalidRequest,
    InvalidAuthority,
    Expired,
    AlreadyUsed,
    ControllerUnavailable,
}

/// Public issued material. The private key never leaves the worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssuedWorkerCredential {
    pub schema_version: u16,
    pub worker_id: WorkerId,
    pub credential_id: CredentialId,
    pub pool: WorkerPoolName,
    #[serde(default)]
    pub predecessor_credential_id: Option<CredentialId>,
    #[serde(default)]
    pub predecessor_retire_at: Option<ObservedAtUnixMillis>,
    pub certificate_chain_pem: String,
}

/// Controller response to one enrollment request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EnrollmentResponse {
    Issued {
        credential: IssuedWorkerCredential,
    },
    Reject {
        code: EnrollmentRejectCode,
        diagnostic: String,
    },
}

fn nibble(byte: u8) -> Result<u8, TransportError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(TransportError::InvalidEnrollmentSecret),
    }
}

#[cfg(test)]
mod tests {
    use super::EnrollmentSecret;

    #[test]
    fn enrollment_secret_round_trips_without_debug_disclosure() {
        let secret = EnrollmentSecret::from_bytes([0xab; 32]);
        let wire = serde_json::to_string(&secret).expect("serialize secret");
        let recovered: EnrollmentSecret = serde_json::from_str(&wire).expect("decode secret");
        assert_eq!(secret, recovered);
        assert!(!format!("{secret:?}").contains("abab"));
        assert!(serde_json::from_str::<EnrollmentSecret>("\"AB\"").is_err());
    }
}
