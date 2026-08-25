use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::SandboxPath;

/// Canonical executable bit for a materialized input file.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputFileMode {
    /// Candidate-readable and writable regular file.
    Data,
    /// Candidate-readable, writable, and executable regular file.
    Executable,
}

/// One create-only entry in an input bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum InputBundleEntry {
    /// Empty directory. Parents must also be declared explicitly.
    Directory { path: SandboxPath },
    /// Complete regular-file bytes; symlinks and special files are intentionally absent from V1.
    File {
        path: SandboxPath,
        mode: InputFileMode,
        #[serde(with = "canonical_base64")]
        bytes: Vec<u8>,
    },
}

impl InputBundleEntry {
    /// Returns the unique sandbox-relative entry path.
    #[must_use]
    pub const fn path(&self) -> &SandboxPath {
        match self {
            Self::Directory { path } | Self::File { path, .. } => path,
        }
    }
}

/// Strict, canonical create-only sandbox input tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputBundleV1 {
    schema_version: u16,
    entries: Vec<InputBundleEntry>,
}

impl InputBundleV1 {
    /// Creates a canonical V1 bundle.
    ///
    /// # Errors
    ///
    /// Rejects duplicate/non-canonical paths, missing directory parents, and file ancestors.
    pub fn new(mut entries: Vec<InputBundleEntry>) -> Result<Self, MaterialFormatError> {
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let bundle = Self {
            schema_version: 1,
            entries,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    /// Returns entries in canonical path order.
    #[must_use]
    pub fn entries(&self) -> &[InputBundleEntry] {
        &self.entries
    }

    /// Decodes and validates canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical JSON or an invalid V1 tree.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MaterialFormatError> {
        let value: Self = cairn_codec::from_slice(bytes)
            .map_err(|error| MaterialFormatError::Codec(error.to_string()))?;
        value.validate()?;
        Ok(value)
    }

    /// Encodes canonical JSON bytes suitable for `InputBundleArtifact` identity derivation.
    ///
    /// # Errors
    ///
    /// Returns an error if the in-memory value is invalid or cannot be encoded.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MaterialFormatError> {
        self.validate()?;
        cairn_codec::to_vec(self).map_err(|error| MaterialFormatError::Codec(error.to_string()))
    }

    fn validate(&self) -> Result<(), MaterialFormatError> {
        if self.schema_version != 1 {
            return Err(MaterialFormatError::UnsupportedInputBundleSchema);
        }
        let mut paths = BTreeSet::new();
        let mut directories = BTreeSet::new();
        for entry in &self.entries {
            let path = entry.path().as_str();
            if !paths.insert(path) {
                return Err(MaterialFormatError::DuplicateEntry(path.to_owned()));
            }
            if let Some((parent, _)) = path.rsplit_once('/') {
                if !directories.contains(parent) {
                    return Err(MaterialFormatError::MissingDirectoryParent {
                        path: path.to_owned(),
                        parent: parent.to_owned(),
                    });
                }
            }
            if matches!(entry, InputBundleEntry::Directory { .. }) {
                directories.insert(path);
            }
        }
        if self
            .entries
            .windows(2)
            .any(|pair| pair[0].path() >= pair[1].path())
        {
            return Err(MaterialFormatError::NonCanonicalEntries);
        }
        Ok(())
    }
}

/// One exact environment variable supplied without inheriting the worker environment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVariable {
    name: EnvironmentVariableName,
    value: String,
}

impl EnvironmentVariable {
    /// Creates one exact environment entry.
    #[must_use]
    pub const fn new(name: EnvironmentVariableName, value: String) -> Self {
        Self { name, value }
    }

    /// Returns the validated variable name.
    #[must_use]
    pub const fn name(&self) -> &EnvironmentVariableName {
        &self.name
    }

    /// Returns the exact value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Portable process-environment variable name.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct EnvironmentVariableName(String);

impl EnvironmentVariableName {
    /// Creates an ASCII `[A-Za-z_][A-Za-z0-9_]*` name.
    ///
    /// # Errors
    ///
    /// Rejects empty, non-portable, or NUL-containing names.
    pub fn new(value: impl Into<String>) -> Result<Self, MaterialFormatError> {
        let value = value.into();
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return Err(MaterialFormatError::InvalidEnvironmentName);
        };
        if !(first.is_ascii_alphabetic() || first == '_')
            || chars.any(|character| !(character.is_ascii_alphanumeric() || character == '_'))
        {
            return Err(MaterialFormatError::InvalidEnvironmentName);
        }
        Ok(Self(value))
    }

    /// Returns the portable name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EnvironmentVariableName {
    type Error = MaterialFormatError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<EnvironmentVariableName> for String {
    fn from(value: EnvironmentVariableName) -> Self {
        value.0
    }
}

/// Strict environment material for the local-process adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEnvironmentV1 {
    schema_version: u16,
    variables: Vec<EnvironmentVariable>,
}

impl ExecutionEnvironmentV1 {
    /// Creates a canonical environment with unique sorted names.
    ///
    /// # Errors
    ///
    /// Rejects duplicate names or NUL-containing values.
    pub fn new(mut variables: Vec<EnvironmentVariable>) -> Result<Self, MaterialFormatError> {
        variables.sort_by(|left, right| left.name.cmp(&right.name));
        let environment = Self {
            schema_version: 1,
            variables,
        };
        environment.validate()?;
        Ok(environment)
    }

    /// Returns variables in canonical name order.
    #[must_use]
    pub fn variables(&self) -> &[EnvironmentVariable] {
        &self.variables
    }

    /// Decodes and validates canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical JSON or an invalid V1 environment.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MaterialFormatError> {
        let value: Self = cairn_codec::from_slice(bytes)
            .map_err(|error| MaterialFormatError::Codec(error.to_string()))?;
        value.validate()?;
        Ok(value)
    }

    /// Encodes canonical JSON bytes suitable for environment identity derivation.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is invalid or cannot be encoded.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MaterialFormatError> {
        self.validate()?;
        cairn_codec::to_vec(self).map_err(|error| MaterialFormatError::Codec(error.to_string()))
    }

    fn validate(&self) -> Result<(), MaterialFormatError> {
        if self.schema_version != 1 {
            return Err(MaterialFormatError::UnsupportedEnvironmentSchema);
        }
        if self
            .variables
            .iter()
            .any(|variable| variable.value.contains('\0'))
        {
            return Err(MaterialFormatError::InvalidEnvironmentValue);
        }
        if self
            .variables
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(MaterialFormatError::NonCanonicalEnvironment);
        }
        Ok(())
    }
}

/// Invalid versioned execution material.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MaterialFormatError {
    #[error("execution material JSON failed: {0}")]
    Codec(String),
    #[error("input bundle schema version is unsupported")]
    UnsupportedInputBundleSchema,
    #[error("execution environment schema version is unsupported")]
    UnsupportedEnvironmentSchema,
    #[error("input bundle contains duplicate path {0}")]
    DuplicateEntry(String),
    #[error("input bundle entries are not in canonical path order")]
    NonCanonicalEntries,
    #[error("input bundle path {path} requires explicitly declared directory {parent}")]
    MissingDirectoryParent { path: String, parent: String },
    #[error("environment variable name is not portable")]
    InvalidEnvironmentName,
    #[error("environment variable value contains NUL")]
    InvalidEnvironmentValue,
    #[error("environment variables are duplicated or not in canonical name order")]
    NonCanonicalEnvironment,
}

mod canonical_base64 {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let wire = String::deserialize(deserializer)?;
        let bytes = STANDARD_NO_PAD.decode(&wire).map_err(D::Error::custom)?;
        if STANDARD_NO_PAD.encode(&bytes) != wire {
            return Err(D::Error::custom("file bytes are not canonical base64"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_round_trips_and_requires_explicit_parents() {
        let bundle = InputBundleV1::new(vec![
            InputBundleEntry::File {
                path: SandboxPath::new("bin/run").expect("path"),
                mode: InputFileMode::Executable,
                bytes: b"binary".to_vec(),
            },
            InputBundleEntry::Directory {
                path: SandboxPath::new("bin").expect("path"),
            },
        ])
        .expect("bundle");
        let bytes = bundle.to_bytes().expect("encode");
        assert_eq!(InputBundleV1::from_bytes(&bytes).expect("decode"), bundle);

        assert!(
            InputBundleV1::new(vec![InputBundleEntry::File {
                path: SandboxPath::new("missing/file").expect("path"),
                mode: InputFileMode::Data,
                bytes: Vec::new(),
            }])
            .is_err()
        );
    }

    #[test]
    fn environment_is_sorted_and_rejects_duplicate_names() {
        let environment = ExecutionEnvironmentV1::new(vec![
            EnvironmentVariable::new(
                EnvironmentVariableName::new("ZED").expect("name"),
                "2".into(),
            ),
            EnvironmentVariable::new(
                EnvironmentVariableName::new("ALPHA").expect("name"),
                "1".into(),
            ),
        ])
        .expect("environment");
        assert_eq!(environment.variables()[0].name().as_str(), "ALPHA");
        assert_eq!(
            ExecutionEnvironmentV1::from_bytes(&environment.to_bytes().expect("encode"))
                .expect("decode"),
            environment
        );
        let duplicate = EnvironmentVariableName::new("DUPLICATE").expect("name");
        assert!(
            ExecutionEnvironmentV1::new(vec![
                EnvironmentVariable::new(duplicate.clone(), "one".into()),
                EnvironmentVariable::new(duplicate, "two".into()),
            ])
            .is_err()
        );
    }
}
