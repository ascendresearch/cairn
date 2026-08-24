//! Stable protocol foundations shared by Cairn layers.
//!
//! Identity derivation is intentionally not implemented until OQ-013 is resolved. These types make
//! identity categories explicit without silently choosing a hash preimage or wire format.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

mod identity;

pub use identity::{
    AttemptId, BlobDigest, BranchId, CommandId, ContentId, ContentType, DerivedId, EpisodeId,
    EventId, HashAlgorithm, IdentityError, IdentityReadError, JobId, ModelAttemptId, OperationId,
    TaskId,
};

const MAX_IDENTIFIER_LEN: usize = 255;

/// Validation failure for a protocol identifier.
#[derive(Debug, Clone, Eq, Error, PartialEq)]
pub enum IdentifierError {
    /// An identifier must contain at least one byte.
    #[error("identifier cannot be empty")]
    Empty,
    /// Identifiers have a conservative wire-size limit.
    #[error("identifier exceeds {MAX_IDENTIFIER_LEN} bytes")]
    TooLong,
    /// V1 identifiers are printable ASCII without whitespace.
    #[error("identifier must contain only printable non-whitespace ASCII")]
    InvalidCharacter,
}

fn validate_identifier(value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value.len() > MAX_IDENTIFIER_LEN {
        return Err(IdentifierError::TooLong);
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(IdentifierError::InvalidCharacter);
    }
    Ok(())
}

macro_rules! identifier_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated identifier.
            ///
            /// # Errors
            ///
            /// Returns [`IdentifierError`] when the value is empty, too long, or contains bytes
            /// outside the printable non-whitespace ASCII wire alphabet.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            /// Returns the wire value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

identifier_type!(
    /// Names an aggregate lifecycle; it is not a content digest.
    AggregateId
);
identifier_type!(
    /// Names an aggregate category.
    AggregateKind
);
identifier_type!(
    /// Names a versioned schema.
    SchemaName
);

/// A non-zero schema version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Creates a schema version. Version zero is reserved and invalid.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaVersionError`] when `value` is zero.
    pub fn new(value: u32) -> Result<Self, SchemaVersionError> {
        if value == 0 {
            return Err(SchemaVersionError);
        }
        Ok(Self(value))
    }

    /// Returns the integer version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Schema version zero is never a persisted schema.
#[derive(Debug, Clone, Copy, Eq, Error, PartialEq)]
#[error("schema version must be greater than zero")]
pub struct SchemaVersionError;

macro_rules! positive_u64_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive protocol value.
            ///
            /// # Errors
            ///
            /// Returns [`PositiveValueError`] when `value` is zero.
            pub fn new(value: u64) -> Result<Self, PositiveValueError> {
                if value == 0 {
                    return Err(PositiveValueError);
                }
                Ok(Self(value))
            }

            /// Returns the positive wire value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

/// A positive protocol counter cannot use zero as an implicit sentinel.
#[derive(Debug, Clone, Copy, Eq, Error, PartialEq)]
#[error("protocol sequence/revision must be greater than zero")]
pub struct PositiveValueError;

/// Observed wall-clock time in Unix milliseconds. It is evidence, never an ordering authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ObservedAtUnixMillis(i64);

impl ObservedAtUnixMillis {
    /// Wraps an observed timestamp without assigning it ordering authority.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Returns the wire integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

positive_u64_type!(
    /// One-based position of an event within an aggregate stream.
    ///
    /// Sequence and revision carry different authority even when their values happen to match:
    ///
    /// ```compile_fail
    /// use cairn_protocol::{EventSequence, StreamRevision};
    ///
    /// let sequence = EventSequence::new(1).unwrap();
    /// let _revision: StreamRevision = sequence;
    /// ```
    EventSequence
);
positive_u64_type!(
    /// Committed revision of an existing aggregate stream.
    StreamRevision
);

/// A stable schema name/version pair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaRef {
    /// Stable schema name.
    pub name: SchemaName,
    /// Positive schema version.
    pub version: SchemaVersion,
}

#[cfg(test)]
mod tests {
    use super::{AggregateId, EventSequence, IdentifierError, SchemaVersion, StreamRevision};

    #[test]
    fn identifiers_reject_ambiguous_wire_values() {
        assert_eq!(AggregateId::new(""), Err(IdentifierError::Empty));
        assert_eq!(
            AggregateId::new("contains whitespace"),
            Err(IdentifierError::InvalidCharacter)
        );
        assert!(AggregateId::new("task:01/example-v1").is_ok());
    }

    #[test]
    fn schema_version_zero_is_rejected_during_decode() {
        let error = serde_json::from_str::<SchemaVersion>("0").expect_err("zero must fail");
        assert!(error.to_string().contains("greater than zero"));
    }

    #[test]
    fn sequence_and_revision_are_distinct_positive_types() {
        let sequence = EventSequence::new(1).expect("sequence");
        let revision = StreamRevision::new(1).expect("revision");
        assert_eq!(sequence.get(), revision.get());
        assert!(EventSequence::new(0).is_err());
        assert!(StreamRevision::new(0).is_err());
    }
}
