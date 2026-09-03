//! Task-generic semantic-intent-recovery proposal boundary.

use std::path::{Component, Path};

#[cfg(feature = "agent-runtime")]
use std::{collections::BTreeMap, path::PathBuf};

use cairn_protocol::{ContentId, ContentType};
use cairn_record::ContentStoreError;
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

const SCHEMA_V1: u16 = 1;

/// Invalid current-V1 SIR input, proposal, or product adapter operation.
#[derive(Debug, Error)]
pub enum SirError {
    /// A validated scalar or text value violated its contract.
    #[error("invalid SIR value: {0}")]
    InvalidValue(&'static str),
    /// A persisted current-V1 object violated a cross-field invariant.
    #[error("invalid SIR structure: {0}")]
    InvalidStructure(&'static str),
    /// The task-root filesystem boundary could not be loaded safely.
    #[error("invalid SIR task root: {0}")]
    TaskRoot(String),
    /// Canonical encoding or strict decoding failed.
    #[error("SIR codec failure: {0}")]
    Codec(String),
    /// Durable content storage failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, SirError> {
    cairn_codec::to_vec(value).map_err(|error| SirError::Codec(error.to_string()))
}

macro_rules! positive_u32_type {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates a positive value.
            ///
            /// # Errors
            ///
            /// Rejects zero.
            pub const fn new(value: u32) -> Result<Self, SirError> {
                if value == 0 {
                    Err(SirError::InvalidValue($kind))
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the primitive boundary value.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

macro_rules! positive_u64_type {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive value.
            ///
            /// # Errors
            ///
            /// Rejects zero.
            pub const fn new(value: u64) -> Result<Self, SirError> {
                if value == 0 {
                    Err(SirError::InvalidValue($kind))
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the primitive boundary value.
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

positive_u32_type!(
    /// One-based source line number.
    SirSourceLineNumber,
    "source line number"
);
positive_u32_type!(
    /// Positive maximum number of task files.
    SirTaskFileLimit,
    "task file limit"
);
positive_u32_type!(
    /// Positive maximum number of lines returned by one read.
    SirReadLineLimit,
    "read line limit"
);
positive_u64_type!(
    /// Positive maximum aggregate task bytes.
    SirTaskByteLimit,
    "task byte limit"
);
positive_u64_type!(
    /// Positive maximum source bytes returned by one read.
    SirReadByteLimit,
    "read byte limit"
);

/// Source line count, which may be zero for an empty task artifact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SirSourceLineCount(u32);

impl SirSourceLineCount {
    /// Creates a source line count.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the primitive count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Task loading and model-visible read limits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirTaskLimits {
    /// Maximum regular files below the explicit task root.
    pub max_files: SirTaskFileLimit,
    /// Maximum aggregate UTF-8 source bytes.
    pub max_task_bytes: SirTaskByteLimit,
    /// Maximum source lines returned by one read tool call.
    pub max_read_lines: SirReadLineLimit,
    /// Maximum source bytes returned by one read tool call.
    pub max_read_bytes: SirReadByteLimit,
}

impl Default for SirTaskLimits {
    fn default() -> Self {
        Self {
            max_files: SirTaskFileLimit(1024),
            max_task_bytes: SirTaskByteLimit(256 * 1024 * 1024),
            max_read_lines: SirReadLineLimit(200),
            max_read_bytes: SirReadByteLimit(32 * 1024),
        }
    }
}

/// Validated path relative to one explicit task root.
///
/// ```compile_fail
/// use cairn_migration::{SirFactStatement, SirTaskArtifactPath};
/// fn require_path(_: SirTaskArtifactPath) {}
/// let fact = SirFactStatement::new("observable fact").unwrap();
/// require_path(fact);
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SirTaskArtifactPath(String);

impl SirTaskArtifactPath {
    /// Creates a canonical task-local path.
    ///
    /// # Errors
    ///
    /// Rejects empty, absolute, traversing, backslash, or control-containing paths.
    pub fn new(value: impl Into<String>) -> Result<Self, SirError> {
        let value = value.into();
        let path = Path::new(&value);
        if value.is_empty()
            || value.trim() != value
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(SirError::InvalidValue("task artifact path"));
        }
        Ok(Self(value))
    }

    /// Returns the task-local wire path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SirTaskArtifactPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Exact UTF-8 bytes of one task artifact.
pub enum SirTaskArtifactBytes {}

impl ContentType for SirTaskArtifactBytes {
    const DOMAIN: &'static str = "migration.sir-task-artifact-bytes.v1";
}

/// Exact task manifest offered to one SIR episode.
pub enum SirTaskBundleArtifact {}

impl ContentType for SirTaskBundleArtifact {
    const DOMAIN: &'static str = "migration.sir-task-bundle.v1";
}

/// Non-authoritative, task-bound SIR proposal.
pub enum SirIntentHypothesisSetProposalArtifact {}

impl ContentType for SirIntentHypothesisSetProposalArtifact {
    const DOMAIN: &'static str = "migration.sir-intent-hypothesis-set-proposal.v1";
}

/// One entry in the frozen task bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirTaskArtifactV1 {
    path: SirTaskArtifactPath,
    identity: ContentId<SirTaskArtifactBytes>,
    line_count: SirSourceLineCount,
}

impl SirTaskArtifactV1 {
    /// Returns the task-local path.
    #[must_use]
    pub const fn path(&self) -> &SirTaskArtifactPath {
        &self.path
    }

    /// Returns the exact source-byte identity.
    #[must_use]
    pub const fn identity(&self) -> ContentId<SirTaskArtifactBytes> {
        self.identity
    }

    /// Returns the source line count.
    #[must_use]
    pub const fn line_count(&self) -> SirSourceLineCount {
        self.line_count
    }
}

/// Canonical manifest of task-local model-readable artifacts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirTaskBundleV1 {
    schema_version: u16,
    artifacts: Vec<SirTaskArtifactV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirTaskBundleWire {
    schema_version: u16,
    artifacts: Vec<SirTaskArtifactV1>,
}

impl TryFrom<SirTaskBundleWire> for SirTaskBundleV1 {
    type Error = SirError;

    fn try_from(wire: SirTaskBundleWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("task bundle schema"));
        }
        if wire.artifacts.is_empty() || wire.artifacts.len() > 32 {
            return Err(SirError::InvalidStructure("task bundle artifact count"));
        }
        if wire
            .artifacts
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(SirError::InvalidStructure(
                "task bundle paths must be unique and sorted",
            ));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            artifacts: wire.artifacts,
        })
    }
}

impl<'de> Deserialize<'de> for SirTaskBundleV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirTaskBundleWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirTaskBundleV1 {
    /// Returns the frozen ordered artifacts.
    #[must_use]
    pub fn artifacts(&self) -> &[SirTaskArtifactV1] {
        &self.artifacts
    }

    /// Derives the exact bundle identity.
    ///
    /// # Errors
    ///
    /// Returns an error only if canonical encoding fails.
    pub fn identity(&self) -> Result<ContentId<SirTaskBundleArtifact>, SirError> {
        ContentId::derive(&encode(self)?).map_err(|error| SirError::Codec(error.to_string()))
    }
}

/// Frozen task sources plus their serializable bundle projection.
#[cfg(feature = "agent-runtime")]
#[derive(Clone)]
pub struct SirTaskWorkspace {
    bundle: SirTaskBundleV1,
    sources: BTreeMap<SirTaskArtifactPath, String>,
}

#[cfg(feature = "agent-runtime")]
impl SirTaskWorkspace {
    /// Freezes an already materialized ordered source bundle at a client/application boundary.
    ///
    /// # Errors
    ///
    /// Rejects empty, duplicate, unsorted, oversized, or otherwise invalid task material.
    pub fn from_sources(
        sources: Vec<(SirTaskArtifactPath, String)>,
        limits: SirTaskLimits,
    ) -> Result<Self, SirError> {
        let source_count = u32::try_from(sources.len())
            .map_err(|_| SirError::TaskRoot("too many task files".to_owned()))?;
        if source_count == 0 || source_count > limits.max_files.get() {
            return Err(SirError::TaskRoot(
                "materialized task file count violates configured limits".to_owned(),
            ));
        }
        if sources.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(SirError::TaskRoot(
                "materialized task paths must be unique and sorted".to_owned(),
            ));
        }
        let mut total_bytes = 0_u64;
        let mut artifacts = Vec::with_capacity(sources.len());
        for (path, source) in &sources {
            total_bytes = total_bytes
                .checked_add(u64::try_from(source.len()).map_err(|_| {
                    SirError::TaskRoot("task artifact byte length overflow".to_owned())
                })?)
                .ok_or_else(|| SirError::TaskRoot("task byte total overflow".to_owned()))?;
            if total_bytes > limits.max_task_bytes.get() {
                return Err(SirError::TaskRoot(
                    "task bytes exceed configured limit".to_owned(),
                ));
            }
            let line_count = u32::try_from(source.lines().count())
                .map_err(|_| SirError::TaskRoot("too many source lines".to_owned()))?;
            artifacts.push(SirTaskArtifactV1 {
                path: path.clone(),
                identity: ContentId::<SirTaskArtifactBytes>::derive(source.as_bytes())
                    .map_err(|error| SirError::Codec(error.to_string()))?,
                line_count: SirSourceLineCount::new(line_count),
            });
        }
        let bundle = SirTaskBundleV1::try_from(SirTaskBundleWire {
            schema_version: SCHEMA_V1,
            artifacts,
        })?;
        Self::from_materialized(bundle, sources, limits)
    }

    /// Reconstructs one exact bounded task snapshot supplied by product workflow context.
    ///
    /// # Errors
    ///
    /// Rejects missing, extra, reordered, identity-mismatched, or over-limit source material.
    pub fn from_materialized(
        bundle: SirTaskBundleV1,
        sources: Vec<(SirTaskArtifactPath, String)>,
        limits: SirTaskLimits,
    ) -> Result<Self, SirError> {
        let source_count = u32::try_from(sources.len())
            .map_err(|_| SirError::TaskRoot("too many task files".to_owned()))?;
        if source_count == 0
            || source_count > limits.max_files.get()
            || sources.len() != bundle.artifacts.len()
        {
            return Err(SirError::TaskRoot(
                "materialized task file count violates its bundle or limits".to_owned(),
            ));
        }
        let mut total_bytes = 0_u64;
        let mut materialized = BTreeMap::new();
        for ((path, source), artifact) in sources.into_iter().zip(bundle.artifacts.iter()) {
            if path != artifact.path {
                return Err(SirError::TaskRoot(
                    "materialized task paths do not match the ordered bundle".to_owned(),
                ));
            }
            let bytes = source.as_bytes();
            total_bytes = total_bytes
                .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                    SirError::TaskRoot("task artifact byte length overflow".to_owned())
                })?)
                .ok_or_else(|| SirError::TaskRoot("task byte total overflow".to_owned()))?;
            if total_bytes > limits.max_task_bytes.get()
                || ContentId::<SirTaskArtifactBytes>::derive(bytes)
                    .map_err(|error| SirError::Codec(error.to_string()))?
                    != artifact.identity
                || u32::try_from(source.lines().count())
                    .map_err(|_| SirError::TaskRoot("too many source lines".to_owned()))?
                    != artifact.line_count.get()
            {
                return Err(SirError::TaskRoot(
                    "materialized task bytes do not match their frozen bundle".to_owned(),
                ));
            }
            if materialized.insert(path, source).is_some() {
                return Err(SirError::TaskRoot(
                    "materialized task paths must be unique".to_owned(),
                ));
            }
        }
        Ok(Self {
            bundle,
            sources: materialized,
        })
    }

    /// Loads a bounded UTF-8 task tree without following symlinks.
    ///
    /// # Errors
    ///
    /// Rejects a non-directory root, symlinks, special files, invalid paths/UTF-8, and limit
    /// violations.
    pub fn load(root: &Path, limits: SirTaskLimits) -> Result<Self, SirError> {
        let metadata = std::fs::symlink_metadata(root)
            .map_err(|error| SirError::TaskRoot(error.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(SirError::TaskRoot(
                "task root must be a real directory".to_owned(),
            ));
        }
        let mut files = Vec::new();
        collect_task_files(root, root, &mut files)?;
        files.sort();
        let file_count = u32::try_from(files.len())
            .map_err(|_| SirError::TaskRoot("too many task files".to_owned()))?;
        if file_count == 0 || file_count > limits.max_files.get() {
            return Err(SirError::TaskRoot(
                "task file count violates configured limits".to_owned(),
            ));
        }

        let mut total_bytes = 0_u64;
        let mut artifacts = Vec::with_capacity(files.len());
        let mut sources = BTreeMap::new();
        for file in files {
            let bytes =
                std::fs::read(&file).map_err(|error| SirError::TaskRoot(error.to_string()))?;
            total_bytes = total_bytes
                .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                    SirError::TaskRoot("task artifact byte length overflow".to_owned())
                })?)
                .ok_or_else(|| SirError::TaskRoot("task byte total overflow".to_owned()))?;
            if total_bytes > limits.max_task_bytes.get() {
                return Err(SirError::TaskRoot(
                    "task bytes exceed configured limit".to_owned(),
                ));
            }
            let text = String::from_utf8(bytes.clone())
                .map_err(|_| SirError::TaskRoot("task artifact must be UTF-8".to_owned()))?;
            let relative = file
                .strip_prefix(root)
                .map_err(|_| SirError::TaskRoot("task path escaped its root".to_owned()))?;
            let relative = relative
                .to_str()
                .ok_or_else(|| SirError::TaskRoot("task path must be UTF-8".to_owned()))?;
            let path = SirTaskArtifactPath::new(relative.replace('\\', "/"))?;
            let line_count = u32::try_from(text.lines().count())
                .map_err(|_| SirError::TaskRoot("too many source lines".to_owned()))?;
            let identity = ContentId::<SirTaskArtifactBytes>::derive(&bytes)
                .map_err(|error| SirError::Codec(error.to_string()))?;
            artifacts.push(SirTaskArtifactV1 {
                path: path.clone(),
                identity,
                line_count: SirSourceLineCount::new(line_count),
            });
            sources.insert(path, text);
        }
        let bundle = SirTaskBundleV1::try_from(SirTaskBundleWire {
            schema_version: SCHEMA_V1,
            artifacts,
        })?;
        Ok(Self { bundle, sources })
    }

    /// Returns the public task manifest.
    #[must_use]
    pub const fn bundle(&self) -> &SirTaskBundleV1 {
        &self.bundle
    }

    pub fn source(&self, path: &SirTaskArtifactPath) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    #[must_use]
    pub fn materialized_sources(&self) -> Vec<(SirTaskArtifactPath, String)> {
        self.bundle
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), self.sources[&artifact.path].clone()))
            .collect()
    }

    #[must_use]
    pub fn artifact(&self, path: &SirTaskArtifactPath) -> Option<&SirTaskArtifactV1> {
        self.bundle
            .artifacts
            .binary_search_by(|artifact| artifact.path.cmp(path))
            .ok()
            .map(|index| &self.bundle.artifacts[index])
    }

    /// Validates an exact citation against the frozen path and line-count snapshot.
    ///
    /// # Errors
    ///
    /// Rejects a missing path or a line range outside its frozen source artifact.
    pub fn validate_citation(&self, citation: &SirSourceCitationV1) -> Result<(), SirError> {
        let artifact = self
            .artifact(&citation.path)
            .ok_or(SirError::InvalidStructure(
                "citation path is not in task bundle",
            ))?;
        if citation.end_line.get() > artifact.line_count.get() {
            return Err(SirError::InvalidStructure(
                "citation line is outside task artifact",
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "agent-runtime")]
fn collect_task_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), SirError> {
    let mut entries = std::fs::read_dir(current)
        .map_err(|error| SirError::TaskRoot(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SirError::TaskRoot(error.to_string()))?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| SirError::TaskRoot(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(SirError::TaskRoot(
                "task tree must not contain symlinks".to_owned(),
            ));
        }
        if metadata.is_dir() {
            collect_task_files(root, &path, files)?;
        } else if metadata.is_file() {
            let _ = path
                .strip_prefix(root)
                .map_err(|_| SirError::TaskRoot("task path escaped its root".to_owned()))?;
            files.push(path);
        } else {
            return Err(SirError::TaskRoot(
                "task tree contains a special file".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Exact source range supporting one model-authored statement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirSourceCitationV1 {
    path: SirTaskArtifactPath,
    start_line: SirSourceLineNumber,
    end_line: SirSourceLineNumber,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirSourceCitationWire {
    path: SirTaskArtifactPath,
    start_line: SirSourceLineNumber,
    end_line: SirSourceLineNumber,
}

impl TryFrom<SirSourceCitationWire> for SirSourceCitationV1 {
    type Error = SirError;

    fn try_from(wire: SirSourceCitationWire) -> Result<Self, Self::Error> {
        if wire.start_line > wire.end_line {
            return Err(SirError::InvalidStructure("citation line range"));
        }
        Ok(Self {
            path: wire.path,
            start_line: wire.start_line,
            end_line: wire.end_line,
        })
    }
}

impl<'de> Deserialize<'de> for SirSourceCitationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirSourceCitationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirSourceCitationV1 {
    /// Returns the cited task-local path.
    #[must_use]
    pub const fn path(&self) -> &SirTaskArtifactPath {
        &self.path
    }

    /// Returns the inclusive first cited line.
    #[must_use]
    pub const fn start_line(&self) -> SirSourceLineNumber {
        self.start_line
    }

    /// Returns the inclusive final cited line.
    #[must_use]
    pub const fn end_line(&self) -> SirSourceLineNumber {
        self.end_line
    }
}
