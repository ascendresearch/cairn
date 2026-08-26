use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EnvironmentVariable, ExecutionEnvironmentV1, MaterialFormatError};

/// The single container backend implemented by the initial private-environment worker.
pub const DOCKER_BACKEND: &str = "docker-v1";

/// Immutable local Docker image identity.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct DockerImageId(String);

impl DockerImageId {
    /// Creates one canonical `sha256:<64 lowercase hex>` Docker image ID.
    ///
    /// # Errors
    ///
    /// Rejects tags, short IDs, uppercase text, and non-SHA-256 values.
    pub fn new(value: impl Into<String>) -> Result<Self, DockerEnvironmentError> {
        let value = value.into();
        let Some(digest) = value.strip_prefix("sha256:") else {
            return Err(DockerEnvironmentError::InvalidImageId);
        };
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DockerEnvironmentError::InvalidImageId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DockerImageId {
    type Error = DockerEnvironmentError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DockerImageId> for String {
    fn from(value: DockerImageId) -> Self {
        value.0
    }
}

/// Canonical Docker image and environment material stored in an execution-environment artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerExecutionEnvironmentV1 {
    schema_version: u16,
    image: DockerImageId,
    variables: Vec<EnvironmentVariable>,
}

impl DockerExecutionEnvironmentV1 {
    /// Creates canonical V1 material.
    ///
    /// # Errors
    ///
    /// Rejects duplicate environment names and invalid values.
    pub fn new(
        image: DockerImageId,
        variables: Vec<EnvironmentVariable>,
    ) -> Result<Self, DockerEnvironmentError> {
        let canonical = ExecutionEnvironmentV1::new(variables)?;
        Ok(Self {
            schema_version: 1,
            image,
            variables: canonical.variables().to_vec(),
        })
    }

    #[must_use]
    pub const fn image(&self) -> &DockerImageId {
        &self.image
    }

    #[must_use]
    pub fn variables(&self) -> &[EnvironmentVariable] {
        &self.variables
    }

    /// Decodes strict canonical V1 JSON.
    ///
    /// # Errors
    ///
    /// Rejects noncanonical JSON, non-V1 material, invalid image IDs, and invalid variables.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DockerEnvironmentError> {
        let environment: Self = cairn_codec::from_slice(bytes)
            .map_err(|error| DockerEnvironmentError::Codec(error.to_string()))?;
        environment.validate()?;
        Ok(environment)
    }

    /// Encodes canonical V1 JSON.
    ///
    /// # Errors
    ///
    /// Rejects invalid in-memory material or codec failure.
    pub fn to_bytes(&self) -> Result<Vec<u8>, DockerEnvironmentError> {
        self.validate()?;
        cairn_codec::to_vec(self).map_err(|error| DockerEnvironmentError::Codec(error.to_string()))
    }

    fn validate(&self) -> Result<(), DockerEnvironmentError> {
        if self.schema_version != 1 {
            return Err(DockerEnvironmentError::UnsupportedSchema);
        }
        let canonical = ExecutionEnvironmentV1::new(self.variables.clone())?;
        if canonical.variables() != self.variables {
            return Err(DockerEnvironmentError::NonCanonicalEnvironment);
        }
        Ok(())
    }
}

/// Invalid Docker execution-environment material.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DockerEnvironmentError {
    #[error("Docker image must be one canonical full sha256 image ID")]
    InvalidImageId,
    #[error("Docker execution environment schema version is unsupported")]
    UnsupportedSchema,
    #[error("Docker environment variables are not in canonical order")]
    NonCanonicalEnvironment,
    #[error("Docker execution environment JSON failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Material(#[from] MaterialFormatError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EnvironmentVariableName;

    const IMAGE: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn image_identity_and_environment_are_strict_canonical_v1() {
        assert!(DockerImageId::new(IMAGE).is_ok());
        assert!(DockerImageId::new("ubuntu:latest").is_err());
        let environment = DockerExecutionEnvironmentV1::new(
            DockerImageId::new(IMAGE).expect("image"),
            vec![EnvironmentVariable::new(
                EnvironmentVariableName::new("MESSAGE").expect("name"),
                "hello".into(),
            )],
        )
        .expect("environment");
        let bytes = environment.to_bytes().expect("encode");
        assert_eq!(
            DockerExecutionEnvironmentV1::from_bytes(&bytes).expect("decode"),
            environment
        );
        assert!(
            DockerExecutionEnvironmentV1::from_bytes(
                format!("{{\"image\":\"{IMAGE}\",\"schema_version\":2,\"variables\":[]}}")
                    .as_bytes()
            )
            .is_err()
        );
    }
}
