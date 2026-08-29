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
            max_files: SirTaskFileLimit(32),
            max_task_bytes: SirTaskByteLimit(256 * 1024),
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
    /// Reconstructs one exact bounded task snapshot supplied by a Controller or Proposal Host.
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

    pub(crate) fn source(&self, path: &SirTaskArtifactPath) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    pub(crate) fn materialized_sources(&self) -> Vec<(SirTaskArtifactPath, String)> {
        self.bundle
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.clone(), self.sources[&artifact.path].clone()))
            .collect()
    }

    pub(crate) fn artifact(&self, path: &SirTaskArtifactPath) -> Option<&SirTaskArtifactV1> {
        self.bundle
            .artifacts
            .binary_search_by(|artifact| artifact.path.cmp(path))
            .ok()
            .map(|index| &self.bundle.artifacts[index])
    }

    pub(crate) fn validate_citation(&self, citation: &SirSourceCitationV1) -> Result<(), SirError> {
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

#[cfg(feature = "agent-runtime")]
mod runtime {
    use std::io::Cursor;

    use cairn_agent::{
        CanonicalToolResult, ContextBlock, EpisodeBudget, EpisodeCompletionReason, HistoryItem,
        InstructionBlock, ModelName, ModelOutputTokenLimit, ModelSelection, ModelTransport,
        NativeProtocolCodec, NativeRequestSpec, NativeToolDefinition, PolicyDocument,
        PreparedToolOperation, ToolCatalog, ToolEffectClass, ToolGateway, ToolGatewayError,
        ToolImplementationVersion, ToolName, ToolRegistration,
    };
    use cairn_protocol::{EpisodeId, TaskId};
    use cairn_record::{ContentStore, EventStore};
    use serde_json::{Value, json};

    use super::{
        ContentId, ContentType, Deserialize, Error, SCHEMA_V1, Serialize, SirError,
        SirIntentHypothesisSetProposalArtifact, SirReadLineLimit, SirSourceLineNumber,
        SirTaskArtifactBytes, SirTaskArtifactPath, SirTaskBundleArtifact, SirTaskLimits,
        SirTaskWorkspace, encode,
    };
    use crate::sir_contract::{
        AgentResolvedRuntimeModelArtifact, IntentHypothesisSetProposalV1,
        IntentRecoveryInputArtifact, IntentRecoveryInputV1, IntentRecoveryRequestV1,
        SirCapabilityManifestV1, SirProposalSubmissionV1,
    };

    const READ_TOOL: &str = "sir_read_task_artifact";
    const SUBMIT_TOOL: &str = "sir_submit_intent_hypotheses";
    const TOOL_VERSION: &str = "sir-proposal-v1";
    const SIR_USER_REQUEST_V1: &str = "Recover higher-order intent from the frozen caller declaration and offered task evidence, then submit one complete typed proposal.";

    const SIR_INSTRUCTION_V1: &str = r"You are the semantic-intent-recovery analyst for one CUDA-to-Ascend-C migration task.

Inspect only the offered task artifacts. First use sir_read_task_artifact to read the source, host launch, ABI, tests, or build files needed for your analysis. Treat observable source facts separately from intent inferences. Cite exact task-local paths and inclusive line ranges.

The caller declaration is an attributed authority source, not a fact to overwrite. Keep caller claims separate from source observations and from your hypotheses. Submit exactly one complete proposal through sir_submit_intent_hypotheses. It must contain source-observed facts with citations, at least two genuinely competing hypotheses, an explicit conflict, at least one unknown, and at least one evidence-backed invariant. Also report applicable optimization freedoms, source-behavior dispositions, and disambiguation experiments; use empty arrays when none are justified. Every reference must point to an ID declared in this proposal or the frozen caller declaration. Use lowercase kebab-case local IDs and sort every top-level collection lexicographically by its id.

The proposal is non-authoritative. Do not claim admission, correctness, a confidence score, or a migration verdict. Do not invent content identities or use paths outside the offered task bundle.";

    /// Failure while driving the product SIR workflow through the domain-neutral agent runtime.
    #[derive(Debug, Error)]
    pub(crate) enum SirProfileError {
        /// Product input, proposal, or task adapter failed.
        #[error(transparent)]
        Sir(#[from] SirError),
        /// The durable agent runtime rejected or could not reconstruct a transition.
        #[error("SIR agent episode failed: {0}")]
        Agent(String),
        /// The model requested a capability outside the fixed SIR profile.
        #[error("SIR model requested an unavailable tool: {0}")]
        UnavailableTool(String),
        /// The episode terminated without one accepted proposal.
        #[error("SIR episode terminated without a proposal: {0:?}")]
        MissingProposal(EpisodeCompletionReason),
    }

    /// Exact archived model-input projection for a SIR episode.
    #[derive(Clone, Debug)]
    struct SirPromptProjectionV1 {
        recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
        recovery_input: IntentRecoveryInputV1,
        instruction: ContentId<InstructionBlock>,
        tool_catalog: ContentId<ToolCatalog>,
        request: ContentId<HistoryItem>,
        context: ContentId<ContextBlock>,
        policy: ContentId<PolicyDocument>,
        user_text: String,
    }

    impl SirPromptProjectionV1 {
        /// Returns the frozen recovery-input identity.
        #[must_use]
        const fn recovery_input_id(&self) -> ContentId<IntentRecoveryInputArtifact> {
            self.recovery_input_id
        }

        /// Returns the exact initial provider-visible request containing only the task manifest.
        #[must_use]
        fn user_text(&self) -> &str {
            &self.user_text
        }

        /// Builds the protocol-native request contract from the same tool definitions.
        ///
        /// # Errors
        ///
        /// Returns an error only if a repository-owned tool name is invalid.
        fn native_spec(
            wire_model: ModelName,
            max_output_tokens: ModelOutputTokenLimit,
        ) -> Result<NativeRequestSpec, SirError> {
            Ok(NativeRequestSpec {
                wire_model,
                instructions: SIR_INSTRUCTION_V1.to_owned(),
                tools: sir_native_tools()?,
                max_output_tokens,
            })
        }
    }

    /// Archives the exact task-generic initial SIR prompt projection.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding, identity, or content-store failure.
    fn archive_sir_prompt<S: ContentStore>(
        store: &mut S,
        workspace: &SirTaskWorkspace,
        task_id: TaskId,
        request_input: IntentRecoveryRequestV1,
        task_limits: SirTaskLimits,
    ) -> Result<SirPromptProjectionV1, SirError> {
        for artifact in workspace.bundle().artifacts() {
            let source = workspace
                .source(&artifact.path)
                .ok_or(SirError::InvalidStructure(
                    "task bundle source bytes are unavailable",
                ))?;
            let archived = store
                .put::<SirTaskArtifactBytes>(&mut Cursor::new(source.as_bytes()))?
                .content_id;
            if archived != artifact.identity {
                return Err(SirError::InvalidStructure(
                    "archived task artifact identity changed",
                ));
            }
        }
        let bundle_bytes = encode(workspace.bundle())?;
        let task_bundle = store
            .put::<SirTaskBundleArtifact>(&mut Cursor::new(bundle_bytes))?
            .content_id;
        let recovery_input = IntentRecoveryInputV1::new(
            task_id,
            task_bundle,
            request_input,
            SirCapabilityManifestV1::proposal_only(task_limits),
        )?;
        let recovery_input_bytes = encode(&recovery_input)?;
        let recovery_input_id = store
            .put::<IntentRecoveryInputArtifact>(&mut Cursor::new(recovery_input_bytes))?
            .content_id;
        if recovery_input_id != recovery_input.identity()? {
            return Err(SirError::InvalidStructure(
                "archived recovery input identity changed",
            ));
        }
        let instruction = put_json::<InstructionBlock>(store, &json!({"text":SIR_INSTRUCTION_V1}))?;
        let tools = sir_native_tools()?;
        let tool_catalog = put_json::<ToolCatalog>(
            store,
            &json!({
                "schema_version":SCHEMA_V1,
                "tools":tools.iter().map(|tool| json!({
                    "name":tool.name.as_str(),
                    "description":tool.description,
                    "input_schema":tool.input_schema,
                    "strict":tool.strict
                })).collect::<Vec<_>>()
            }),
        )?;
        let model_context = json!({
            "schema_version":SCHEMA_V1,
            "knowledge_snapshot":{"kind":"empty"},
            "recovery_input_id":recovery_input_id,
            "recovery_input":recovery_input,
            "task_artifacts":workspace.bundle().artifacts()
        });
        let context_text = String::from_utf8(encode(&model_context)?)
            .map_err(|_| SirError::Codec("recovery input is not UTF-8".to_owned()))?;
        let user_text = format!("{SIR_USER_REQUEST_V1}\n\nFrozen recovery input:\n{context_text}");
        let request = put_json::<HistoryItem>(store, &json!({"role":"user","content":user_text}))?;
        let context = put_json::<ContextBlock>(store, &model_context)?;
        let policy = put_json::<PolicyDocument>(
            store,
            &json!({
                "schema_version":SCHEMA_V1,
                "filesystem":"frozen-task-bundle-only",
                "network":"none",
                "proposal_authority":"none",
                "hidden_material":"unavailable"
            }),
        )?;
        Ok(SirPromptProjectionV1 {
            recovery_input_id,
            recovery_input,
            instruction,
            tool_catalog,
            request,
            context,
            policy,
            user_text,
        })
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct SirReadRequestV1 {
        schema_version: u16,
        path: SirTaskArtifactPath,
        start_line: SirSourceLineNumber,
        line_count: SirReadLineLimit,
    }

    /// Bounded task-artifact read gateway.
    struct SirReadTaskArtifactGateway {
        workspace: SirTaskWorkspace,
        limits: SirTaskLimits,
    }

    impl SirReadTaskArtifactGateway {
        /// Creates a read-only gateway over one frozen workspace.
        #[must_use]
        const fn new(workspace: SirTaskWorkspace, limits: SirTaskLimits) -> Self {
            Self { workspace, limits }
        }
    }

    impl ToolGateway for SirReadTaskArtifactGateway {
        fn invoke(
            &mut self,
            operation: &PreparedToolOperation,
        ) -> Result<CanonicalToolResult, ToolGatewayError> {
            validate_operation(operation, READ_TOOL, ToolEffectClass::ReadOnly)?;
            let request: SirReadRequestV1 = decode_tool_arguments(operation.argument_bytes())?;
            if request.schema_version != SCHEMA_V1
                || request.line_count.get() > self.limits.max_read_lines.get()
            {
                return rejected("task-artifact read violates current-V1 limits");
            }
            let artifact = self.workspace.artifact(&request.path).ok_or_else(|| {
                ToolGatewayError::Rejected("task artifact is not offered".to_owned())
            })?;
            if request.start_line.get() > artifact.line_count.get() {
                return rejected("task-artifact start line is outside the file");
            }
            let source = self.workspace.source(&request.path).ok_or_else(|| {
                ToolGatewayError::Rejected("task artifact bytes are unavailable".to_owned())
            })?;
            let start = usize::try_from(request.start_line.get() - 1).map_err(|_| {
                ToolGatewayError::Rejected("task-artifact line overflow".to_owned())
            })?;
            let requested = usize::try_from(request.line_count.get()).map_err(|_| {
                ToolGatewayError::Rejected("task-artifact line overflow".to_owned())
            })?;
            let lines = source
                .lines()
                .skip(start)
                .take(requested)
                .collect::<Vec<_>>();
            let returned_bytes = lines.iter().try_fold(0_u64, |total, line| {
                total
                    .checked_add(u64::try_from(line.len()).map_err(|_| {
                        ToolGatewayError::Rejected("task-artifact byte overflow".to_owned())
                    })?)
                    .ok_or_else(|| {
                        ToolGatewayError::Rejected("task-artifact byte overflow".to_owned())
                    })
            })?;
            if returned_bytes > self.limits.max_read_bytes.get() {
                return rejected("task-artifact read exceeds byte limit");
            }
            let numbered = lines
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                json!({
                    "line":request.start_line.get().saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)),
                    "text":text
                })
            })
            .collect::<Vec<_>>();
            CanonicalToolResult::from_value(&json!({
                "schema_version":SCHEMA_V1,
                "path":request.path,
                "artifact_identity":artifact.identity,
                "lines":numbered
            }))
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
        }
    }

    /// Pure gateway that validates and binds a model-authored hypothesis submission.
    struct SirSubmitIntentHypothesesGateway {
        workspace: SirTaskWorkspace,
        recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
        recovery_input: IntentRecoveryInputV1,
        episode_id: EpisodeId,
        model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        accepted: Option<(
            ContentId<SirIntentHypothesisSetProposalArtifact>,
            IntentHypothesisSetProposalV1,
        )>,
    }

    impl SirSubmitIntentHypothesesGateway {
        /// Binds a proposal collector to exact trusted runtime provenance.
        #[must_use]
        const fn new(
            workspace: SirTaskWorkspace,
            recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
            recovery_input: IntentRecoveryInputV1,
            episode_id: EpisodeId,
            model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        ) -> Self {
            Self {
                workspace,
                recovery_input_id,
                recovery_input,
                episode_id,
                model_configuration,
                accepted: None,
            }
        }

        /// Returns the accepted proposal after a successful tool execution.
        #[must_use]
        const fn accepted(
            &self,
        ) -> Option<&(
            ContentId<SirIntentHypothesisSetProposalArtifact>,
            IntentHypothesisSetProposalV1,
        )> {
            self.accepted.as_ref()
        }
    }

    impl ToolGateway for SirSubmitIntentHypothesesGateway {
        fn invoke(
            &mut self,
            operation: &PreparedToolOperation,
        ) -> Result<CanonicalToolResult, ToolGatewayError> {
            validate_operation(operation, SUBMIT_TOOL, ToolEffectClass::Pure)?;
            let submission: SirProposalSubmissionV1 =
                decode_tool_arguments(operation.argument_bytes())?;
            submission
                .validate_against(&self.workspace, &self.recovery_input)
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
            let proposal = IntentHypothesisSetProposalV1::new(
                self.recovery_input_id,
                self.episode_id,
                self.model_configuration,
                submission,
            );
            let identity = proposal
                .identity()
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
            if let Some((accepted_identity, _)) = &self.accepted {
                if *accepted_identity != identity {
                    return rejected("a different SIR proposal was already accepted");
                }
            } else {
                self.accepted = Some((identity, proposal));
            }
            CanonicalToolResult::from_value(&json!({
                "schema_version":SCHEMA_V1,
                "accepted_proposal":identity
            }))
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
        }
    }

    struct SirProfileGateway {
        read: SirReadTaskArtifactGateway,
        submit: SirSubmitIntentHypothesesGateway,
    }

    impl ToolGateway for SirProfileGateway {
        fn invoke(
            &mut self,
            operation: &PreparedToolOperation,
        ) -> Result<CanonicalToolResult, ToolGatewayError> {
            if operation.tool().as_str() == READ_TOOL {
                self.read.invoke(operation)
            } else {
                self.submit.invoke(operation)
            }
        }
    }

    /// Trusted inputs selected before opening one SIR episode.
    pub(crate) struct SirProfileInput {
        /// Product task lifecycle identity; the exact source bytes are bound separately by the bundle.
        pub task_id: TaskId,
        /// Caller/target/evidence declaration frozen by the Controller before model work.
        pub recovery_request: IntentRecoveryRequestV1,
        /// Durable agent episode identity.
        pub episode_id: EpisodeId,
        /// Exact resolved runtime-model configuration identity.
        pub model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        /// Provider/model/deployment/adapter selection audited by `cairn-agent`.
        pub selection: ModelSelection,
        /// Durable agent budget.
        pub budget: EpisodeBudget,
        /// Per-turn provider output bound.
        pub max_output_tokens: ModelOutputTokenLimit,
        /// Task loading and read-tool bounds.
        pub task_limits: SirTaskLimits,
    }

    /// Completed proposal-only SIR workflow facts.
    pub(crate) struct SirProfileOutcome {
        proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
        proposal: IntentHypothesisSetProposalV1,
        completion_reason: EpisodeCompletionReason,
        steps_started: u32,
    }

    impl SirProfileOutcome {
        /// Returns the archived non-authoritative proposal identity.
        #[must_use]
        pub const fn proposal_id(&self) -> ContentId<SirIntentHypothesisSetProposalArtifact> {
            self.proposal_id
        }

        /// Returns the accepted proposal body and trusted provenance envelope.
        #[must_use]
        pub const fn proposal(&self) -> &IntentHypothesisSetProposalV1 {
            &self.proposal
        }

        /// Returns why the durable agent episode stopped.
        #[must_use]
        pub const fn completion_reason(&self) -> EpisodeCompletionReason {
            self.completion_reason
        }

        /// Returns the number of provider steps started.
        #[must_use]
        pub const fn steps_started(&self) -> u32 {
            self.steps_started
        }
    }

    /// Prepares the SIR domain profile and delegates its episode to the common Proposal Host loop.
    ///
    /// Recorded and live providers implement the same [`ModelTransport`] seam. All source reads and
    /// proposal submissions pass through durable operation admission and execution; the model never
    /// receives direct filesystem access or proposal authority.
    ///
    /// # Errors
    ///
    /// Returns a product-boundary, durable runtime, transport, tool, budget, or missing-proposal
    /// failure. A provider/tool ambiguity remains durable and is never retried implicitly.
    pub(crate) fn run_sir_profile<E, C, T>(
        events: &mut E,
        content: &mut C,
        transport: &mut T,
        codec: NativeProtocolCodec,
        workspace: SirTaskWorkspace,
        input: SirProfileInput,
    ) -> Result<SirProfileOutcome, SirProfileError>
    where
        E: EventStore,
        C: ContentStore,
        T: ModelTransport,
    {
        let projection = archive_sir_prompt(
            content,
            &workspace,
            input.task_id,
            input.recovery_request.clone(),
            input.task_limits,
        )?;
        let native_spec = SirPromptProjectionV1::native_spec(
            input.selection.model.clone(),
            input.max_output_tokens,
        )
        .map_err(SirProfileError::Sir)?;
        let frozen = crate::proposal_loop::FrozenProposalLoopV1 {
            task_id: input.task_id,
            episode_id: input.episode_id,
            role: cairn_agent::AgentRoleName::new("sir-intent-analyst")
                .map_err(|_| SirProfileError::Agent("invalid built-in SIR role".to_owned()))?,
            selection: input.selection,
            budget: input.budget,
            native_spec,
            user_text: projection.user_text().to_owned(),
            instruction: projection.instruction,
            tool_catalog: projection.tool_catalog,
            history: projection.request,
            context: projection.context,
            policy: projection.policy,
            capability_grant: crate::proposal_loop::ProposalLoopCapabilityGrantV1::new(
                sir_tool_registrations()?.to_vec(),
            )
            .map_err(|error| SirProfileError::Agent(error.to_string()))?,
        };
        let mut gateway = SirProfileGateway {
            read: SirReadTaskArtifactGateway::new(workspace.clone(), input.task_limits),
            submit: SirSubmitIntentHypothesesGateway::new(
                workspace,
                projection.recovery_input_id(),
                projection.recovery_input.clone(),
                input.episode_id,
                input.model_configuration,
            ),
        };

        let completion = crate::proposal_loop::run_proposal_loop(
            events,
            content,
            transport,
            codec,
            &frozen,
            &mut gateway,
        )
        .map_err(|error| match error {
            crate::proposal_loop::ProposalLoopError::UnavailableTool(tool) => {
                SirProfileError::UnavailableTool(tool)
            }
            error => SirProfileError::Agent(error.to_string()),
        })?;
        finish_sir_episode(
            content,
            &gateway.submit,
            completion.reason,
            completion.steps_started,
        )
    }

    fn finish_sir_episode<C: ContentStore>(
        content: &mut C,
        submit_gateway: &SirSubmitIntentHypothesesGateway,
        reason: EpisodeCompletionReason,
        steps_started: u32,
    ) -> Result<SirProfileOutcome, SirProfileError> {
        let Some((proposal_id, proposal)) = submit_gateway.accepted().cloned() else {
            return Err(SirProfileError::MissingProposal(reason));
        };
        let archived = content
            .put::<SirIntentHypothesisSetProposalArtifact>(&mut Cursor::new(encode(&proposal)?))
            .map_err(SirError::Content)?
            .content_id;
        if archived != proposal_id {
            return Err(SirProfileError::Agent(
                "archived SIR proposal identity changed".to_owned(),
            ));
        }
        Ok(SirProfileOutcome {
            proposal_id,
            proposal,
            completion_reason: reason,
            steps_started,
        })
    }

    /// Returns trusted registrations for the two current-V1 SIR tools.
    ///
    /// # Errors
    ///
    /// Returns an error only if repository-owned labels violate generic agent contracts.
    fn sir_tool_registrations() -> Result<[ToolRegistration; 2], SirError> {
        Ok([
            ToolRegistration::new(
                ToolName::new(READ_TOOL)
                    .map_err(|_| SirError::InvalidValue("built-in tool name"))?,
                ToolImplementationVersion::new(TOOL_VERSION)
                    .map_err(|_| SirError::InvalidValue("built-in tool version"))?,
                ToolEffectClass::ReadOnly,
            ),
            ToolRegistration::new(
                ToolName::new(SUBMIT_TOOL)
                    .map_err(|_| SirError::InvalidValue("built-in tool name"))?,
                ToolImplementationVersion::new(TOOL_VERSION)
                    .map_err(|_| SirError::InvalidValue("built-in tool version"))?,
                ToolEffectClass::Pure,
            ),
        ])
    }

    /// Returns exact protocol-native tool definitions derived from the current product contract.
    ///
    /// # Errors
    ///
    /// Returns an error only if repository-owned labels violate generic agent contracts.
    #[allow(clippy::too_many_lines)] // Keep the exact provider schema visibly aligned with the V1 types.
    fn sir_native_tools() -> Result<Vec<NativeToolDefinition>, SirError> {
        let local_id = json!({
            "type":"string",
            "minLength":1,
            "maxLength":64,
            "pattern":"^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$"
        });
        let text_1000 = json!({"type":"string","minLength":1,"maxLength":1000});
        let text_2000 = json!({"type":"string","minLength":1,"maxLength":2000});
        let citation = json!({
            "type":"object",
            "properties":{
                "path":{"type":"string","minLength":1},
                "start_line":{"type":"integer","minimum":1},
                "end_line":{"type":"integer","minimum":1}
            },
            "required":["path","start_line","end_line"],
            "additionalProperties":false
        });
        let evidence_ref = json!({
            "oneOf":[
                {
                    "type":"object",
                    "properties":{
                        "source":{"type":"string","const":"caller-claim"},
                        "claim":local_id
                    },
                    "required":["source","claim"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{
                        "source":{"type":"string","const":"observed-fact"},
                        "observation":local_id
                    },
                    "required":["source","observation"],
                    "additionalProperties":false
                }
            ]
        });
        let claim_ref = json!({
            "oneOf":[
                {
                    "type":"object",
                    "properties":{
                        "source":{"type":"string","const":"caller-claim"},
                        "claim":local_id
                    },
                    "required":["source","claim"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{
                        "source":{"type":"string","const":"hypothesis"},
                        "hypothesis":local_id
                    },
                    "required":["source","hypothesis"],
                    "additionalProperties":false
                }
            ]
        });
        let target_ref = json!({
            "oneOf":[
                {
                    "type":"object",
                    "properties":{
                        "kind":{"type":"string","const":"hypothesis"},
                        "hypothesis":local_id
                    },
                    "required":["kind","hypothesis"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{
                        "kind":{"type":"string","const":"conflict"},
                        "conflict":local_id
                    },
                    "required":["kind","conflict"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{
                        "kind":{"type":"string","const":"unknown"},
                        "unknown":local_id
                    },
                    "required":["kind","unknown"],
                    "additionalProperties":false
                }
            ]
        });
        let observed_fact = json!({
            "type":"object",
            "properties":{
                "id":local_id,
                "statement":text_2000,
                "citations":{"type":"array","minItems":1,"maxItems":8,"items":citation}
            },
            "required":["id","statement","citations"],
            "additionalProperties":false
        });
        Ok(vec![
            NativeToolDefinition {
                name: ToolName::new(READ_TOOL)
                    .map_err(|_| SirError::InvalidValue("built-in tool name"))?,
                description: "Read a bounded line range from one offered task-local artifact."
                    .to_owned(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "schema_version":{"type":"integer","const":1},
                        "path":{"type":"string","minLength":1},
                        "start_line":{"type":"integer","minimum":1},
                        "line_count":{"type":"integer","minimum":1,"maximum":200}
                    },
                    "required":["schema_version","path","start_line","line_count"],
                    "additionalProperties":false
                }),
                strict: true,
            },
            NativeToolDefinition {
                name: ToolName::new(SUBMIT_TOOL)
                    .map_err(|_| SirError::InvalidValue("built-in tool name"))?,
                description: "Submit a cited, competing, non-authoritative intent hypothesis set."
                    .to_owned(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "schema_version":{"type":"integer","const":1},
                        "observed_facts":{
                            "type":"array","minItems":1,"maxItems":64,"items":observed_fact
                        },
                        "hypotheses":{
                            "type":"array","minItems":2,"maxItems":16,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "layer":{"type":"string","enum":["algorithm","numerical","model-deployment","observable-contract"]},
                                    "claim":text_2000,
                                    "domain":text_1000,
                                    "supporting_evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref},
                                    "counter_evidence":{"type":"array","maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","layer","claim","domain","supporting_evidence","counter_evidence"],
                                "additionalProperties":false
                            }
                        },
                        "conflicts":{
                            "type":"array","minItems":1,"maxItems":16,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "statement":text_2000,
                                    "claims":{"type":"array","minItems":2,"maxItems":32,"items":claim_ref},
                                    "evidence":{"type":"array","maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","statement","claims","evidence"],
                                "additionalProperties":false
                            }
                        },
                        "unknowns":{
                            "type":"array","minItems":1,"maxItems":32,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "kind":{"type":"string","enum":["desired-semantics","source-behavior","numerical-allowance","deployment-context","tool-or-evidence-gap"]},
                                    "question":text_2000,
                                    "evidence":{"type":"array","maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","kind","question","evidence"],
                                "additionalProperties":false
                            }
                        },
                        "invariants":{
                            "type":"array","minItems":1,"maxItems":32,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "statement":text_2000,
                                    "evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","statement","evidence"],
                                "additionalProperties":false
                            }
                        },
                        "optimization_freedoms":{
                            "type":"array","maxItems":32,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "statement":text_2000,
                                    "protected_invariants":{"type":"array","minItems":1,"maxItems":32,"items":local_id},
                                    "evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","statement","protected_invariants","evidence"],
                                "additionalProperties":false
                            }
                        },
                        "source_dispositions":{
                            "type":"array","maxItems":32,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "observation":local_id,
                                    "disposition":{"type":"string","enum":["preserve-observed-behavior","follow-proposed-semantic-intent","exclude-undefined-region","split-domain","block-pending-user-decision","unknown-classification"]},
                                    "rationale":text_2000,
                                    "evidence":{"type":"array","minItems":1,"maxItems":32,"items":evidence_ref}
                                },
                                "required":["id","observation","disposition","rationale","evidence"],
                                "additionalProperties":false
                            }
                        },
                        "disambiguation_experiments":{
                            "type":"array","maxItems":32,
                            "items":{
                                "type":"object",
                                "properties":{
                                    "id":local_id,
                                    "targets":{"type":"array","minItems":1,"maxItems":32,"items":target_ref},
                                    "plan":text_2000,
                                    "predictions":{"type":"array","minItems":2,"maxItems":32,"items":text_1000}
                                },
                                "required":["id","targets","plan","predictions"],
                                "additionalProperties":false
                            }
                        }
                    },
                    "required":["schema_version","observed_facts","hypotheses","conflicts","unknowns","invariants","optimization_freedoms","source_dispositions","disambiguation_experiments"],
                    "additionalProperties":false
                }),
                strict: true,
            },
        ])
    }

    fn validate_operation(
        operation: &PreparedToolOperation,
        expected_name: &'static str,
        expected_effect: ToolEffectClass,
    ) -> Result<(), ToolGatewayError> {
        if operation.tool().as_str() != expected_name
            || operation.implementation_version().as_str() != TOOL_VERSION
            || operation.effect() != expected_effect
        {
            return Err(ToolGatewayError::NotStarted(
                "operation does not match the trusted SIR registration".to_owned(),
            ));
        }
        Ok(())
    }

    fn decode_tool_arguments<T>(bytes: &[u8]) -> Result<T, ToolGatewayError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let value: T = cairn_codec::from_slice(bytes)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        if encode(&value).map_err(|error| ToolGatewayError::Rejected(error.to_string()))? != bytes {
            return rejected("tool arguments are not canonical current-V1 bytes");
        }
        Ok(value)
    }

    fn rejected<T>(message: &str) -> Result<T, ToolGatewayError> {
        Err(ToolGatewayError::Rejected(message.to_owned()))
    }

    fn put_json<T: ContentType>(
        store: &mut impl ContentStore,
        value: &Value,
    ) -> Result<ContentId<T>, SirError> {
        let bytes = encode(value)?;
        Ok(store.put::<T>(&mut Cursor::new(bytes))?.content_id)
    }
}

#[cfg(feature = "agent-runtime")]
pub(crate) use runtime::{SirProfileInput, run_sir_profile};
