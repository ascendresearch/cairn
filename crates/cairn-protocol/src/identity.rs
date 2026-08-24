//! Strong, domain-separated identity types.

use std::{fmt, hash::Hash, io::Read, marker::PhantomData, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const FRAME_MAGIC: &[u8] = b"cairn:id\0";
const FRAME_VERSION: u16 = 1;
const SHA256_TAG: u8 = 1;
const DIGEST_LEN: usize = 32;
const WIRE_PREFIX: &str = "cairn:v1:sha256:";

/// Hash algorithms understood by this build. V1 deliberately supports only SHA-256.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    /// SHA-256 from the maintained `sha2` crate.
    Sha256,
}

/// Identity construction or wire-validation failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum IdentityError {
    /// The identity is not in the canonical wire form.
    #[error("invalid identity wire format")]
    InvalidWire,
    /// Only lowercase hexadecimal is canonical.
    #[error("identity digest must be exactly 64 lowercase hexadecimal characters")]
    InvalidDigest,
    /// A typed identity was given bytes from another semantic domain.
    #[error("identity domain mismatch: expected {expected}, found {actual}")]
    DomainMismatch {
        /// Domain required by the Rust type.
        expected: &'static str,
        /// Domain carried on the wire.
        actual: String,
    },
    /// Identity domains use a conservative registered alphabet.
    #[error("identity domain is invalid")]
    InvalidDomain,
    /// A frame length cannot be represented by the V1 format.
    #[error("identity frame component is too large")]
    FrameTooLarge,
    /// A lifecycle wire value has the wrong type prefix or UUID version.
    #[error("invalid {kind} lifecycle identity")]
    InvalidLifecycle {
        /// Required lifecycle category.
        kind: &'static str,
    },
}

/// Failure while deriving an identity from a stream.
#[derive(Debug, Error)]
pub enum IdentityReadError {
    /// Identity framing failed.
    #[error(transparent)]
    Identity(#[from] IdentityError),
    /// The byte stream failed.
    #[error("identity input stream failed: {0}")]
    Io(#[from] std::io::Error),
    /// The stream length differs from the length committed into the identity frame.
    #[error("identity input length mismatch: expected {expected}, observed {observed}")]
    LengthMismatch {
        /// Declared byte length.
        expected: u64,
        /// Bytes actually read, capped at expected plus one.
        observed: u64,
    },
}

/// Declares the stable semantic domain for exact content bytes.
pub trait ContentType {
    /// Registered domain, for example `content.source-file.v1`.
    const DOMAIN: &'static str;
}

/// Public semantic identity for exact bytes interpreted as `T`.
pub struct ContentId<T: ContentType> {
    digest: Sha256Digest,
    marker: PhantomData<fn() -> T>,
}

impl<T: ContentType> ContentId<T> {
    /// Derives a typed identity from exact canonical/content bytes.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the registered domain or frame length is invalid.
    pub fn derive(bytes: &[u8]) -> Result<Self, IdentityError> {
        Ok(Self {
            digest: derive_domain_digest(T::DOMAIN, bytes)?,
            marker: PhantomData,
        })
    }

    /// Derives a typed identity without loading the complete content into memory.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityReadError`] when framing fails, reading fails, or the stream length does
    /// not equal `byte_len` exactly.
    pub fn derive_reader(reader: &mut dyn Read, byte_len: u64) -> Result<Self, IdentityReadError> {
        Ok(Self {
            digest: derive_domain_digest_reader(T::DOMAIN, reader, byte_len)?,
            marker: PhantomData,
        })
    }

    /// Returns the canonical tagged wire identity.
    #[must_use]
    pub fn to_wire(&self) -> String {
        tagged_wire(T::DOMAIN, self.digest)
    }
}

impl<T: ContentType> Copy for ContentId<T> {}
impl<T: ContentType> Clone for ContentId<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ContentType> PartialEq for ContentId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}
impl<T: ContentType> Eq for ContentId<T> {}
impl<T: ContentType> Hash for ContentId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}
impl<T: ContentType> fmt::Debug for ContentId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ContentId")
            .field(&self.to_wire())
            .finish()
    }
}
impl<T: ContentType> fmt::Display for ContentId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_wire())
    }
}
impl<T: ContentType> FromStr for ContentId<T> {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            digest: parse_tagged_wire(value, T::DOMAIN)?,
            marker: PhantomData,
        })
    }
}
impl<T: ContentType> Serialize for ContentId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_wire())
    }
}
impl<'de, T: ContentType> Deserialize<'de> for ContentId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Typed identity for deterministic relationship material.
pub struct DerivedId<T: ContentType>(ContentId<T>);

impl<T: ContentType> DerivedId<T> {
    /// Derives an identity from canonical relationship bytes.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the domain or frame cannot be represented.
    pub fn derive(bytes: &[u8]) -> Result<Self, IdentityError> {
        ContentId::derive(bytes).map(Self)
    }

    /// Returns the canonical tagged wire identity.
    #[must_use]
    pub fn to_wire(&self) -> String {
        self.0.to_wire()
    }
}

impl<T: ContentType> Copy for DerivedId<T> {}
impl<T: ContentType> Clone for DerivedId<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ContentType> PartialEq for DerivedId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T: ContentType> Eq for DerivedId<T> {}
impl<T: ContentType> Hash for DerivedId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<T: ContentType> fmt::Debug for DerivedId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DerivedId")
            .field(&self.to_wire())
            .finish()
    }
}
impl<T: ContentType> fmt::Display for DerivedId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_wire())
    }
}
impl<T: ContentType> FromStr for DerivedId<T> {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<ContentId<T>>().map(Self)
    }
}
impl<T: ContentType> Serialize for DerivedId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_wire())
    }
}
impl<'de, T: ContentType> Deserialize<'de> for DerivedId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Identity of one canonical event envelope without its own ID field.
///
/// Lifecycle and derived identities cannot be interchanged:
///
/// ```compile_fail
/// use cairn_protocol::{CommandId, EventId};
///
/// let command = CommandId::new();
/// let _event: EventId = command;
/// ```
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct EventId(Sha256Digest);

impl EventId {
    const DOMAIN: &'static str = "event.v1";

    /// Derives an event identity from canonical identity material.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the identity frame cannot be represented.
    pub fn derive(canonical_material: &[u8]) -> Result<Self, IdentityError> {
        derive_domain_digest(Self::DOMAIN, canonical_material).map(Self)
    }

    /// Returns the canonical tagged wire identity.
    #[must_use]
    pub fn to_wire(self) -> String {
        tagged_wire(Self::DOMAIN, self.0)
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_wire())
    }
}
impl fmt::Debug for EventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EventId")
            .field(&self.to_wire())
            .finish()
    }
}
impl FromStr for EventId {
    type Err = IdentityError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_tagged_wire(value, Self::DOMAIN).map(Self)
    }
}
impl Serialize for EventId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_wire())
    }
}
impl<'de> Deserialize<'de> for EventId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Internal exact-byte digest used for physical integrity and deduplication.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlobDigest(Sha256Digest);

impl BlobDigest {
    /// Hashes exact physical bytes without assigning them a semantic content type.
    #[must_use]
    pub fn derive(bytes: &[u8]) -> Self {
        Self(Sha256Digest(Sha256::digest(bytes).into()))
    }

    /// Hashes a physical byte stream to EOF and returns its digest and observed length.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error when the stream cannot be read completely.
    pub fn derive_reader(reader: &mut dyn Read) -> Result<(Self, u64), std::io::Error> {
        let mut hasher = Sha256::new();
        let byte_len = update_from_reader(&mut hasher, reader, None)?;
        Ok((Self(Sha256Digest(hasher.finalize().into())), byte_len))
    }

    /// Returns `sha256:<lowercase hex>` for storage metadata.
    #[must_use]
    pub fn to_wire(self) -> String {
        format!("sha256:{}", self.0.to_hex())
    }

    /// Returns the lowercase digest used for filesystem sharding.
    #[must_use]
    pub fn hex(self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Display for BlobDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_wire())
    }
}

impl FromStr for BlobDigest {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .strip_prefix("sha256:")
            .ok_or(IdentityError::InvalidWire)
            .and_then(Sha256Digest::from_hex)
            .map(Self)
    }
}

impl Serialize for BlobDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_wire())
    }
}

impl<'de> Deserialize<'de> for BlobDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Sha256Digest([u8; DIGEST_LEN]);

impl Sha256Digest {
    fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(DIGEST_LEN * 2);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }

    fn from_hex(value: &str) -> Result<Self, IdentityError> {
        if value.len() != DIGEST_LEN * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(IdentityError::InvalidDigest);
        }
        let mut digest = [0_u8; DIGEST_LEN];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Ok(Self(digest))
    }
}

fn hex_nibble(byte: u8) -> Result<u8, IdentityError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(IdentityError::InvalidDigest),
    }
}

fn derive_domain_digest(domain: &str, payload: &[u8]) -> Result<Sha256Digest, IdentityError> {
    validate_domain(domain)?;
    let domain_len = u16::try_from(domain.len()).map_err(|_| IdentityError::FrameTooLarge)?;
    let payload_len = u64::try_from(payload.len()).map_err(|_| IdentityError::FrameTooLarge)?;
    let mut hasher = Sha256::new();
    hasher.update(FRAME_MAGIC);
    hasher.update(FRAME_VERSION.to_be_bytes());
    hasher.update([SHA256_TAG]);
    hasher.update(domain_len.to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(payload_len.to_be_bytes());
    hasher.update(payload);
    Ok(Sha256Digest(hasher.finalize().into()))
}

fn derive_domain_digest_reader(
    domain: &str,
    reader: &mut dyn Read,
    payload_len: u64,
) -> Result<Sha256Digest, IdentityReadError> {
    validate_domain(domain)?;
    let domain_len = u16::try_from(domain.len()).map_err(|_| IdentityError::FrameTooLarge)?;
    let mut hasher = Sha256::new();
    hasher.update(FRAME_MAGIC);
    hasher.update(FRAME_VERSION.to_be_bytes());
    hasher.update([SHA256_TAG]);
    hasher.update(domain_len.to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(payload_len.to_be_bytes());
    let observed = update_from_reader(&mut hasher, reader, Some(payload_len))?;
    if observed != payload_len {
        return Err(IdentityReadError::LengthMismatch {
            expected: payload_len,
            observed,
        });
    }
    let mut extra = [0_u8; 1];
    if reader.read(&mut extra)? != 0 {
        return Err(IdentityReadError::LengthMismatch {
            expected: payload_len,
            observed: payload_len.saturating_add(1),
        });
    }
    Ok(Sha256Digest(hasher.finalize().into()))
}

fn update_from_reader(
    hasher: &mut Sha256,
    reader: &mut dyn Read,
    limit: Option<u64>,
) -> Result<u64, std::io::Error> {
    let mut observed = 0_u64;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let wanted = limit.map_or(buffer.len(), |remaining| {
            usize::try_from(remaining.saturating_sub(observed))
                .unwrap_or(buffer.len())
                .min(buffer.len())
        });
        if wanted == 0 {
            break;
        }
        let read = reader.read(&mut buffer[..wanted])?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        observed = observed.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    Ok(observed)
}

fn validate_domain(domain: &str) -> Result<(), IdentityError> {
    if domain.is_empty()
        || !domain
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
    {
        return Err(IdentityError::InvalidDomain);
    }
    Ok(())
}

fn tagged_wire(domain: &str, digest: Sha256Digest) -> String {
    format!("{WIRE_PREFIX}{domain}:{}", digest.to_hex())
}

fn parse_tagged_wire(
    value: &str,
    expected_domain: &'static str,
) -> Result<Sha256Digest, IdentityError> {
    let rest = value
        .strip_prefix(WIRE_PREFIX)
        .ok_or(IdentityError::InvalidWire)?;
    let (domain, digest) = rest.split_once(':').ok_or(IdentityError::InvalidWire)?;
    if domain != expected_domain {
        return Err(IdentityError::DomainMismatch {
            expected: expected_domain,
            actual: domain.to_owned(),
        });
    }
    Sha256Digest::from_hex(digest)
}

macro_rules! lifecycle_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a `UUIDv7` lifecycle identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Returns the underlying UUID at explicit protocol/storage boundaries.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}:{}", $kind, self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.to_string()).finish()
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let uuid = value
                    .strip_prefix(concat!($kind, ":"))
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .filter(|uuid| uuid.get_version_num() == 7)
                    .ok_or(IdentityError::InvalidLifecycle { kind: $kind })?;
                Ok(Self(uuid))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
            }
        }
    };
}

lifecycle_id!(/// Task aggregate lifecycle identity.
TaskId, "task");
lifecycle_id!(/// Agent episode lifecycle identity.
EpisodeId, "episode");
lifecycle_id!(/// External operation lifecycle identity.
OperationId, "operation");
lifecycle_id!(/// Logical execution job identity.
JobId, "job");
lifecycle_id!(/// Concrete execution-attempt identity.
AttemptId, "attempt");
lifecycle_id!(/// Idempotent command identity.
CommandId, "command");
lifecycle_id!(/// Counterfactual/replay branch identity.
BranchId, "branch");
lifecycle_id!(/// One externally dispatched model-attempt identity.
ModelAttemptId, "model-attempt");
lifecycle_id!(/// One model request and its resulting tool-operation boundary.
StepId, "step");

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{BlobDigest, CommandId, ContentId, ContentType, IdentityReadError};

    struct Source;
    impl ContentType for Source {
        const DOMAIN: &'static str = "content.source.v1";
    }

    struct Response;
    impl ContentType for Response {
        const DOMAIN: &'static str = "content.response.v1";
    }

    #[test]
    fn same_bytes_have_distinct_semantic_domains_and_one_blob_digest() {
        let bytes = b"same bytes";
        let source = ContentId::<Source>::derive(bytes).expect("source id");
        let response = ContentId::<Response>::derive(bytes).expect("response id");
        assert_ne!(source.to_wire(), response.to_wire());
        assert_eq!(BlobDigest::derive(bytes), BlobDigest::derive(bytes));
        assert_eq!(
            source.to_wire(),
            "cairn:v1:sha256:content.source.v1:\
             21b81c568bb22ab4208a17807cf3f525dfe2ef7d6fe808566cc52593117ebf3a"
        );
    }

    #[test]
    fn typed_parser_rejects_another_domain() {
        let response = ContentId::<Response>::derive(b"x").expect("response");
        assert!(response.to_wire().parse::<ContentId<Source>>().is_err());
    }

    #[test]
    fn lifecycle_ids_are_uuid_v7_and_type_tagged() {
        let command = CommandId::new();
        assert_eq!(command.as_uuid().get_version_num(), 7);
        assert_eq!(command.to_string().parse::<CommandId>(), Ok(command));
    }

    #[test]
    fn streaming_and_in_memory_derivation_are_identical() {
        let bytes = b"streamed identity material";
        let expected = ContentId::<Source>::derive(bytes).expect("in-memory identity");
        let actual = ContentId::<Source>::derive_reader(
            &mut Cursor::new(bytes),
            u64::try_from(bytes.len()).expect("length"),
        )
        .expect("stream identity");
        assert_eq!(actual, expected);

        let (blob, length) =
            BlobDigest::derive_reader(&mut Cursor::new(bytes)).expect("stream blob");
        assert_eq!(blob, BlobDigest::derive(bytes));
        assert_eq!(length, u64::try_from(bytes.len()).expect("length"));
    }

    #[test]
    fn streaming_identity_rejects_declared_length_mismatch() {
        let error = ContentId::<Source>::derive_reader(&mut Cursor::new(b"abc"), 2)
            .expect_err("extra byte must fail");
        assert!(matches!(error, IdentityReadError::LengthMismatch { .. }));
    }
}
